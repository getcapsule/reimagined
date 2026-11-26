use super::Message;
use iced::{Task, widget::image::Handle};
use lofty::{
    file::AudioFile,
    picture::PictureType,
    prelude::{Accessor, TaggedFileExt},
    probe::Probe,
};
use std::{
    fs::File,
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub(super) struct Sleeve {
    pub cover: Handle,
}

#[derive(Default, Debug, Clone)]
pub(super) struct Details {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub duration: Option<std::time::Duration>,
    pub art: Option<Sleeve>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum Key {
    File(PathBuf),
    YouTube(capsule::providers::youtube::VideoId),
}
pub(super) enum Request<'a> {
    File(Option<&'a Path>),
    YouTube(&'a capsule::providers::youtube::Track),
}
#[derive(Default)]
pub(super) struct Artwork {
    key: Option<Key>,
    pending: bool,
    pub details: Option<Details>,
    pub error: Option<String>,
}
impl Artwork {
    pub(super) fn sync(&mut self, request: Option<Request<'_>>) -> Task<Message> {
        let key = match &request {
            Some(Request::File(Some(path))) => Some(Key::File(path.to_path_buf())),
            Some(Request::YouTube(track)) => Some(Key::YouTube(track.id.clone())),
            _ => None,
        };
        if self.key != key {
            self.key = key;
            self.details = None;
            self.error = None;
        }
        if self.pending || self.details.is_some() || self.error.is_some() {
            return Task::none();
        }
        let Some(key) = self.key.clone() else {
            return Task::none();
        };
        let remote = match request {
            Some(Request::YouTube(t)) => Some(t.clone()),
            _ => None,
        };
        self.pending = true;
        Task::perform(
            async move {
                let job_key = key.clone();
                let result = tokio::task::spawn_blocking(move || -> Result<Details, String> {
                    if let Some(track) = remote {
                        let art = youtube_cover(&track, 512)?;
                        Ok(Details {
                            title: Some(track.title),
                            artist: Some(track.artist),
                            duration: track.duration,
                            art: art.map(|cover| Sleeve { cover }),
                        })
                    } else if let Key::File(path) = job_key {
                        Ok(read(&path))
                    } else {
                        Ok(Details::default())
                    }
                })
                .await
                .map_err(|e| format!("Could not load artwork: {e}"))
                .and_then(|r| r);
                (key, result)
            },
            |(key, result)| Message::Artwork(key, result),
        )
    }
    pub(super) fn received(&mut self, key: &Key, result: Result<Details, String>) {
        self.pending = false;
        if self.key.as_ref() == Some(key) {
            match result {
                Ok(details) => self.details = Some(details),
                Err(e) => self.error = Some(e),
            }
        }
    }
}

pub(super) fn youtube_cover(
    track: &capsule::providers::youtube::Track,
    side: u32,
) -> Result<Option<Handle>, String> {
    if let Some(url) = &track.thumbnail
        && let Ok(Some(cover)) = remote_cover(url, side)
    {
        return Ok(Some(cover));
    }
    remote_cover(
        &format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", track.id.as_str()),
        side,
    )
}

pub(super) fn remote_cover(url: &str, side: u32) -> Result<Option<Handle>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(url)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    response
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    Ok(thumbnail(&bytes, side).map(|s| s.cover))
}

const MAX_BYTES: usize = 12 * 1024 * 1024;
fn thumbnail(bytes: &[u8], side: u32) -> Option<Sleeve> {
    if bytes.len() > MAX_BYTES {
        return None;
    }
    let reader = || {
        image::io::Reader::new(Cursor::new(bytes))
            .with_guessed_format()
            .ok()
    };
    let (w, h) = reader()?.into_dimensions().ok()?;
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 16_000_000 {
        return None;
    }
    let source = reader()?.decode().ok()?;
    let thumb = source.resize_to_fill(side, side, image::imageops::FilterType::Triangle);
    let rgba = thumb.into_rgba8();
    Some(Sleeve {
        cover: Handle::from_rgba(side, side, rgba.into_raw()),
    })
}

pub(super) fn image_file(path: &Path, side: u32) -> Option<Handle> {
    let file = File::open(path).ok()?;
    if file.metadata().ok()?.len() > MAX_BYTES as u64 {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    thumbnail(&bytes, side).map(|art| art.cover)
}

fn folder_art(dir: &Path, side: u32) -> Option<Sleeve> {
    [
        "cover.jpg",
        "cover.png",
        "folder.jpg",
        "folder.png",
        "front.jpg",
        "front.png",
    ]
    .iter()
    .find_map(|name| {
        let file = File::open(dir.join(name)).ok()?;
        if file.metadata().ok()?.len() > MAX_BYTES as u64 {
            return None;
        }
        let mut bytes = Vec::new();
        file.take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        thumbnail(&bytes, side)
    })
}

fn packet_duration(path: &Path) -> Option<std::time::Duration> {
    use symphonia::core::{errors::Error, io::MediaSourceStream, probe::Hint, units::TimeBase};
    let stream = MediaSourceStream::new(Box::new(File::open(path).ok()?), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .format(&hint, stream, &Default::default(), &Default::default())
        .ok()?
        .format;
    let track = format.default_track()?;
    let (id, start) = (track.id, track.codec_params.start_ts);
    let clock = track.codec_params.time_base.or_else(|| {
        Some(TimeBase::new(
            1,
            track.codec_params.sample_rate.filter(|rate| *rate > 0)?,
        ))
    })?;
    let mut end = start;
    loop {
        match format.next_packet() {
            Ok(p) if p.track_id() == id => end = end.max(p.ts().saturating_add(p.dur())),
            Ok(_) => {}
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(_) => return None,
        }
    }
    let time = clock.calc_time(end.checked_sub(start)?);
    Some(
        std::time::Duration::from_secs(time.seconds)
            + std::time::Duration::from_secs_f64(time.frac),
    )
}

pub(super) fn read(path: &Path) -> Details {
    read_details(path, 512, true)
}

pub(super) fn cover(path: &Path, side: u32) -> Option<Handle> {
    read_details(path, side, false).art.map(|art| art.cover)
}

fn read_details(path: &Path, side: u32, resolve_duration: bool) -> Details {
    let mut result = Details::default();
    if let Some(tagged) = Probe::open(path).ok().and_then(|probe| probe.read().ok()) {
        result.duration = Some(tagged.properties().duration());
        let tags = || tagged.primary_tag().into_iter().chain(tagged.tags());
        result.title = tags().find_map(|t| t.title().map(|s| s.into_owned()));
        result.artist = tags().find_map(|t| t.artist().map(|s| s.into_owned()));
        let pictures = || tagged.tags().iter().flat_map(|t| t.pictures());
        result.art = pictures()
            .filter(|p| p.pic_type() == PictureType::CoverFront)
            .chain(pictures().filter(|p| p.pic_type() != PictureType::CoverFront))
            .find_map(|p| thumbnail(p.data(), side));
    }
    if resolve_duration && result.duration.is_none_or(|d| d.is_zero()) {
        result.duration = packet_duration(path);
    }
    if result.art.is_none() {
        result.art = path.parent().and_then(|dir| folder_art(dir, side));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn worker_failure_is_reported_once_and_cleared_on_track_change() {
        let mut artwork = Artwork::default();
        let first = Path::new("first.wav");
        let second = Path::new("second.wav");
        let _ = artwork.sync(Some(Request::File(Some(first))));
        artwork.received(
            &Key::File(first.to_path_buf()),
            Err("worker cancelled".into()),
        );
        assert_eq!(artwork.error.as_deref(), Some("worker cancelled"));
        let _ = artwork.sync(Some(Request::File(Some(first))));
        assert!(!artwork.pending);
        let _ = artwork.sync(Some(Request::File(Some(second))));
        assert!(artwork.error.is_none() && artwork.pending);
    }

    #[test]
    fn folder_thumbnail_survives_missing_embedded_art_and_corrupt_cover() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let song = dir.path().join("Song.wav");
        std::fs::write(&song, include_bytes!("../../assets/stereo-check.wav"))?;
        assert_eq!(
            packet_duration(&song),
            Some(std::time::Duration::from_secs(4))
        );
        assert!(read(&song).art.is_none());
        std::fs::write(dir.path().join("cover.jpg"), b"broken")?;
        std::fs::write(
            dir.path().join("folder.png"),
            include_bytes!("../../assets/logo.png"),
        )?;
        assert!(read(&song).art.is_some());
        assert!(matches!(
            cover(&song, 64),
            Some(Handle::Rgba {
                width: 64,
                height: 64,
                ..
            })
        ));
        Ok(())
    }
}

#[cfg(test)]
mod youtube_art_tests {
    use super::*;

    #[test]
    fn webp_thumbnails_decode_into_real_artwork() {
        let sleeve =
            thumbnail(include_bytes!("../../tests/fixtures/cover.webp"), 32).expect("WebP cover");
        assert!(matches!(
            sleeve.cover,
            Handle::Rgba {
                width: 32,
                height: 32,
                ..
            }
        ));
    }

    #[test]
    #[ignore = "requires YouTube artwork network"]
    fn youtube_thumbnail_fallback_loads_without_metadata_url() {
        let track = capsule::providers::youtube::Track {
            id: "V_Ydoe4Q-Gg".to_owned().try_into().unwrap(),
            title: "Weird Fishes".into(),
            artist: "Radiohead".into(),
            thumbnail: None,
            duration: None,
        };
        assert!(
            youtube_cover(&track, 128)
                .expect("YouTube artwork request")
                .is_some()
        );
    }
}
