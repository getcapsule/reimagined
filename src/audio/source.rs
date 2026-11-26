use super::AudioError;
use crate::providers::youtube::{self, Streams, Track, VideoId};
use std::{
    fs::File,
    io::{self, Cursor},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
pub use symphonia::core::io::MediaSource;

pub trait AudioSource: Send {
    fn open_media(&self) -> Result<Box<dyn MediaSource>, AudioError>;
    fn extension(&self) -> Option<&str> {
        None
    }
}

#[derive(Debug, Clone)]
pub enum Source {
    File(PathBuf),
    StereoCheck,
    YouTube(YouTubeSource),
}

#[derive(Debug, Clone)]
pub struct YouTubeSource {
    track: Track,
    cache: Arc<StreamCache>,
}
impl From<Track> for YouTubeSource {
    fn from(track: Track) -> Self {
        Self {
            track,
            cache: Arc::default(),
        }
    }
}
impl std::ops::Deref for YouTubeSource {
    type Target = Track;
    fn deref(&self) -> &Track {
        &self.track
    }
}

impl AudioSource for Source {
    fn open_media(&self) -> Result<Box<dyn MediaSource>, AudioError> {
        match self {
            Self::YouTube(track) => Ok(Box::new(super::http::HttpAudio::open(
                &track.id,
                Arc::clone(&track.cache),
            )?)),
            Self::File(path) => Ok(Box::new(File::open(path)?)),
            Self::StereoCheck => Ok(Box::new(Cursor::new(
                &include_bytes!("../../assets/stereo-check.wav")[..],
            ))),
        }
    }

    fn extension(&self) -> Option<&str> {
        match self {
            Self::File(path) => path.extension()?.to_str(),
            Self::StereoCheck => Some("wav"),
            Self::YouTube(_) => None,
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct StreamCache(Mutex<Option<CachedStreams>>);
#[derive(Debug)]
struct CachedStreams {
    created: Instant,
    streams: Streams,
}
#[derive(Debug, Clone, Copy)]
pub(super) enum Resolution {
    Reuse,
    Refresh,
}
impl StreamCache {
    pub(super) fn resolve(
        &self,
        id: &VideoId,
        format: Option<&str>,
        mode: Resolution,
    ) -> io::Result<Streams> {
        self.get_or_load(mode, Instant::now(), || {
            youtube::resolve(id, format).map_err(io::Error::other)
        })
    }
    fn get_or_load(
        &self,
        mode: Resolution,
        now: Instant,
        load: impl FnOnce() -> io::Result<Streams>,
    ) -> io::Result<Streams> {
        {
            let mut cached = self
                .0
                .lock()
                .map_err(|_| io::Error::other("Playback cache lock failed"))?;
            if matches!(mode, Resolution::Reuse)
                && let Some(value) = cached.as_ref()
                && now.saturating_duration_since(value.created) < Duration::from_secs(600)
            {
                return Ok(value.streams.clone());
            }
            *cached = None;
        }
        let streams = load()?;
        *self
            .0
            .lock()
            .map_err(|_| io::Error::other("Playback cache lock failed"))? = Some(CachedStreams {
            created: now,
            streams: streams.clone(),
        });
        Ok(streams)
    }
}
#[cfg(test)]
mod cache_tests {
    use super::*;
    use std::{
        cell::Cell,
        time::{Duration, Instant},
    };
    #[test]
    fn repeated_opens_reuse_metadata_and_refresh_invalidates_it() -> std::io::Result<()> {
        let cache = StreamCache::default();
        let calls = Cell::new(0);
        let load = || {
            calls.set(calls.get() + 1);
            Ok(crate::providers::youtube::Streams {
                current: crate::providers::youtube::Stream {
                    format_id: Some("251".into()),
                    url: "https://example.test/audio".into(),
                    http_headers: Default::default(),
                },
                alternatives: Default::default(),
            })
        };
        let now = Instant::now();
        cache.get_or_load(Resolution::Reuse, now, load)?;
        cache.get_or_load(Resolution::Reuse, now, load)?;
        assert_eq!(calls.get(), 1, "Seeking must not re-extract the same video");
        cache.get_or_load(Resolution::Refresh, now, load)?;
        assert_eq!(calls.get(), 2);
        cache.get_or_load(Resolution::Reuse, now + Duration::from_secs(601), load)?;
        assert_eq!(calls.get(), 3, "Expired metadata must be resolved again");
        Ok(())
    }
}
