use super::{Entry, Error, Track, VideoId, Youtube, execute};
use serde::Deserialize;
use std::{sync::Arc, time::Duration};

#[derive(Debug)]
pub(super) struct CatalogSong(Track);
impl CatalogSong {
    pub(super) fn id(&self) -> &VideoId {
        &self.0.id
    }
}
#[derive(Deserialize)]
struct SongResults {
    extractor_key: String,
    webpage_url: String,
    #[serde(default)]
    entries: Vec<Option<Entry>>,
}
#[derive(Deserialize)]
struct PublicMetadata {
    author_name: String,
    thumbnail_url: String,
}
fn search_url(query: &str) -> reqwest::Url {
    let mut url =
        reqwest::Url::parse("https://music.youtube.com/search").expect("constant search URL");
    url.query_pairs_mut().append_pair("q", query.trim());
    url.set_fragment(Some("songs"));
    url
}
fn catalog(output: &str) -> Result<Vec<CatalogSong>, Error> {
    let result: SongResults = serde_json::from_str(output)?;
    let url = reqwest::Url::parse(&result.webpage_url).map_err(|_| {
        Error::Unavailable("YouTube Music returned an invalid song search URL.".into())
    })?;
    if result.extractor_key != "YoutubeMusicSearchURL"
        || url.scheme() != "https"
        || url.host_str() != Some("music.youtube.com")
        || url.path() != "/search"
        || url.fragment() != Some("songs")
    {
        return Err(Error::Unavailable(
            "Discover requires YouTube Music song results.".into(),
        ));
    }
    let mut seen = std::collections::HashSet::new();
    Ok(result
        .entries
        .into_iter()
        .flatten()
        .filter_map(Entry::track)
        .filter(|track| !track.title.trim().is_empty() && seen.insert(track.id.clone()))
        .map(CatalogSong)
        .collect())
}
pub(super) async fn search(
    provider: &Youtube,
    query: &str,
    limit: usize,
) -> Result<Vec<CatalogSong>, Error> {
    provider.prepare().await?;
    let output = execute(vec![
        "--flat-playlist".into(),
        "--playlist-end".into(),
        limit.clamp(1, 24).to_string(),
        search_url(query).into(),
    ])
    .await?;
    catalog(&output)
}
fn enrich(mut song: CatalogSong, info: PublicMetadata) -> Result<Track, Error> {
    let artist = info
        .author_name
        .trim()
        .strip_suffix(" - Topic")
        .unwrap_or(info.author_name.trim());
    let url = reqwest::Url::parse(&info.thumbnail_url)
        .map_err(|_| Error::Unavailable("Invalid song artwork URL.".into()))?;
    if artist.is_empty() || url.scheme() != "https" {
        return Err(Error::Unavailable(
            "Song artist or artwork is unavailable.".into(),
        ));
    }
    if song.0.artist.trim().is_empty() {
        song.0.artist = artist.to_owned();
    }
    song.0.thumbnail = Some(info.thumbnail_url);
    Ok(song.0)
}
async fn song_metadata(client: &reqwest::Client, song: CatalogSong) -> Result<Track, Error> {
    let mut response = client
        .get("https://www.youtube.com/oembed")
        .query(&[("url", song.id().url()), ("format", "json".to_owned())])
        .send()
        .await?
        .error_for_status()?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > 64 * 1024 {
            return Err(Error::Unavailable(
                "Song metadata response is too large.".into(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    enrich(song, serde_json::from_slice(&bytes)?)
}
pub(super) async fn metadata(songs: Vec<CatalogSong>) -> Result<Vec<Track>, Error> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    let slots = Arc::new(tokio::sync::Semaphore::new(6));
    let mut jobs = tokio::task::JoinSet::new();
    for song in songs {
        let client = client.clone();
        let slots = slots.clone();
        jobs.spawn(async move {
            let _permit = slots
                .acquire_owned()
                .await
                .map_err(|e| Error::Unavailable(e.to_string()))?;
            song_metadata(&client, song).await
        });
    }
    let mut tracks = Vec::new();
    let mut last_error = None;
    while let Some(result) = jobs.join_next().await {
        match result {
            Ok(Ok(track)) => tracks.push(track),
            Ok(Err(error)) => last_error = Some(error),
            Err(error) => last_error = Some(Error::Unavailable(error.to_string())),
        }
    }
    if tracks.is_empty() {
        return Err(last_error.unwrap_or_else(|| {
            Error::Unavailable(
                "No songs could be loaded from YouTube Music. Refresh to retry.".into(),
            )
        }));
    }
    Ok(tracks)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn result(extractor: &str, section: &str) -> String {
        serde_json::json!({
            "extractor_key": extractor,
            "webpage_url": format!("https://music.youtube.com/search?q=radiohead#{section}"),
            "entries": [
                {"id":"Xgaj5suTCgk","title":"Talk Show Host"},
                {"id":"dkOz9Ve2mio","title":"A Long Music Mix", "duration":1500},
                {"id":"Xgaj5suTCgk","title":"Duplicate"},
                {"id":"bad","title":"Not a song ID"},
                {"id":"PxOO8YkB_Og","title":"Unavailable", "availability":"private"}
            ]
        })
        .to_string()
    }
    #[test]
    fn only_the_music_songs_section_can_produce_catalog_songs() {
        let songs = catalog(&result("YoutubeMusicSearchURL", "songs")).unwrap();
        assert_eq!(songs.len(), 2);
        assert!(catalog(&result("YoutubeSearch", "songs")).is_err());
        for section in ["videos", "podcasts", "episodes", "albums", "playlists", ""] {
            assert!(catalog(&result("YoutubeMusicSearchURL", section)).is_err());
        }
        let mut wrong_host: serde_json::Value =
            serde_json::from_str(&result("YoutubeMusicSearchURL", "songs")).unwrap();
        wrong_host["webpage_url"] = "https://www.youtube.com/search?q=radiohead#songs".into();
        assert!(catalog(&wrong_host.to_string()).is_err());
    }
    #[test]
    fn user_queries_cannot_change_the_music_section() {
        let url = search_url(" Sade & podcasts#episodes ");
        assert_eq!(url.fragment(), Some("songs"));
        assert_eq!(url.query_pairs().count(), 1);
        assert_eq!(
            url.query_pairs().next().unwrap().1,
            "Sade & podcasts#episodes"
        );
    }
    #[test]
    fn real_metadata_populates_artist_and_cover_without_replacing_the_song_title() {
        let song = catalog(&result("YoutubeMusicSearchURL", "songs"))
            .unwrap()
            .remove(0);
        let track = enrich(
            song,
            PublicMetadata {
                author_name: "Radiohead - Topic".into(),
                thumbnail_url: "https://i.ytimg.com/vi/Xgaj5suTCgk/hqdefault.jpg".into(),
            },
        )
        .unwrap();
        assert_eq!(track.title, "Talk Show Host");
        assert_eq!(track.artist, "Radiohead");
        assert!(track.thumbnail.is_some());
        assert!(track.duration.is_none());
    }
    #[test]
    fn missing_artist_or_artwork_does_not_produce_a_discover_track() {
        for (author, url) in [
            ("", "https://example.test/art.jpg"),
            ("Artist", "not a URL"),
            ("Artist", "http://example.test/art.jpg"),
        ] {
            let song = catalog(&result("YoutubeMusicSearchURL", "songs"))
                .unwrap()
                .remove(0);
            assert!(
                enrich(
                    song,
                    PublicMetadata {
                        author_name: author.into(),
                        thumbnail_url: url.into()
                    }
                )
                .is_err()
            );
        }
    }
    #[test]
    #[ignore = "requires YouTube Music network and an audio output device"]
    fn live_catalog_track_keeps_stream_duration_and_seeking()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::audio::{Event, OutputDevice, Player, Source};
        use std::{thread, time::Instant};
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let tracks = runtime.block_on(async {
            let provider = Youtube::default();
            metadata(search(&provider, "Radiohead", 1).await?).await
        })?;
        let track = tracks.into_iter().next().ok_or("No catalog song")?;
        let player = Player::new()?;
        let playback = player.open(
            Source::YouTube(track.into()),
            OutputDevice::System,
            Duration::from_secs(17),
            false,
            0,
        )?;
        let deadline = Instant::now() + Duration::from_secs(65);
        loop {
            match playback.poll()? {
                Some(Event::Ready(info)) => {
                    assert!(
                        info.duration.is_some_and(|d| d > Duration::from_secs(17)),
                        "stream supplies the player duration"
                    );
                    break;
                }
                Some(Event::Failed(e) | Event::NextFailed(e)) => return Err(e.into()),
                _ => {}
            }
            assert!(Instant::now() < deadline, "catalog playback timed out");
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(playback.position(), Duration::from_secs(17));
        player.set_playing(true)?;
        let deadline = Instant::now() + Duration::from_secs(10);
        while playback.position() < Duration::from_secs(18) {
            assert!(Instant::now() < deadline, "catalog song did not advance");
            if let Some(Event::Failed(e)) = playback.poll()? {
                return Err(e.into());
            }
            thread::sleep(Duration::from_millis(20));
        }
        player.stop()?;
        Ok(())
    }
}
