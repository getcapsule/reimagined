use super::*;

#[test]
fn metadata_filters_unavailable_and_invalid_video_ids() -> Result<(), Box<dyn std::error::Error>> {
    let output = r#"{"entries":[{"id":"V_Ydoe4Q-Gg","title":"Song","uploader":"Artist","duration":12.5,"thumbnails":[{"url":"https://i.ytimg.com/test.jpg"}]},{"id":"bad","title":"Invalid"},{"id":"TNRCvG9YtYI","title":"Live","live_status":"is_live"}]}"#;
    let entries: Results = serde_json::from_str(output)?;
    let tracks: Vec<_> = entries
        .entries
        .into_iter()
        .flatten()
        .filter_map(Entry::track)
        .collect();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].artist, "Artist");
    assert_eq!(tracks[0].duration, Some(Duration::from_millis(12500)));
    assert!(serde_json::from_str::<VideoId>(r#""../unsafe""#).is_err());
    Ok(())
}

#[test]
#[ignore = "requires YouTube network and an audio output device"]
fn live_search_stream_and_seek_without_media_files() -> Result<(), Box<dyn std::error::Error>> {
    use crate::audio::{AudioSource, Event, OutputDevice, Player, Source};
    use std::{
        io::{Read, Seek, SeekFrom},
        thread,
        time::Instant,
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let provider = Youtube::default();
    let hits = runtime.block_on(provider.search("Radiohead Weird Fishes official audio", 3))?;
    assert!(!hits.is_empty());
    let source = Source::YouTube(hits[0].clone().into());
    let mut media = source.open_media()?;
    assert!(media.is_seekable());
    let length = media.byte_len().expect("known audio length");
    assert!(length > 1024);
    let mut beginning = [0; 32];
    media.read_exact(&mut beginning)?;
    media.seek(SeekFrom::End(-32))?;
    let mut tail = [0; 32];
    media.read_exact(&mut tail)?;
    media.seek(SeekFrom::Start(0))?;
    let mut replay = [0; 32];
    media.read_exact(&mut replay)?;
    assert_eq!(beginning, replay);
    drop(media);
    let player = Player::new()?;
    let playback = player.open(
        source,
        OutputDevice::System,
        Duration::from_secs(17),
        false,
        0,
    )?;
    let deadline = Instant::now() + Duration::from_secs(70);
    loop {
        match playback.poll()? {
            Some(Event::Ready(_)) => break,
            Some(Event::Failed(e) | Event::NextFailed(e)) => return Err(e.into()),
            _ => {}
        }
        assert!(Instant::now() < deadline, "stream open timed out");
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(playback.position(), Duration::from_secs(17));
    player.set_playing(true)?;
    let deadline = Instant::now() + Duration::from_secs(12);
    while playback.position() <= Duration::from_secs(18) {
        if let Some(Event::Failed(e)) = playback.poll()? {
            return Err(e.into());
        }
        assert!(Instant::now() < deadline, "stream playback stalled");
        thread::sleep(Duration::from_millis(20));
    }
    player.stop()?;
    Ok(())
}

#[test]
#[ignore = "requires YouTube network and an audio output device"]
fn live_cowboys_and_angels_streams() -> Result<(), Box<dyn std::error::Error>> {
    use crate::audio::{Event, OutputDevice, Player, Source};
    use std::{thread, time::Instant};
    let source = Source::YouTube(
        Track {
            id: "Ve-rxwKuqtI"
                .to_owned()
                .try_into()
                .map_err(std::io::Error::other)?,
            title: "Cowboys and Angels".into(),
            artist: "George Michael".into(),
            thumbnail: None,
            duration: Some(Duration::from_secs(437)),
        }
        .into(),
    );
    let player = Player::new()?;
    let playback = player.open(source, OutputDevice::System, Duration::ZERO, true, 0)?;
    let deadline = Instant::now() + Duration::from_secs(70);
    while playback.position() < Duration::from_secs(2) {
        match playback.poll()? {
            Some(
                Event::Skipped { error, .. } | Event::Failed(error) | Event::NextFailed(error),
            ) => return Err(error.into()),
            Some(Event::Ended) => return Err("Stream ended before playing".into()),
            _ => {}
        }
        assert!(Instant::now() < deadline, "YouTube playback timed out");
        thread::sleep(Duration::from_millis(20));
    }
    player.stop()?;
    Ok(())
}

#[test]
fn rejected_renditions_have_compatible_aac_alternatives() -> Result<(), Box<dyn std::error::Error>>
{
    let output = r#"{
        "format_id":"140", "url":"https://example.test/rejected", "language":"en",
        "formats":[
            {"format_id":"140-alt", "url":"https://example.test/alternate", "abr":128, "acodec":"mp4a.40.2", "ext":"m4a", "protocol":"https", "vcodec":"none", "language":"en", "audio_channels":2},
            {"format_id":"140-fr", "url":"https://example.test/french", "abr":128, "acodec":"mp4a.40.2", "ext":"m4a", "protocol":"https", "vcodec":"none", "language":"fr"},
            {"format_id":"251", "url":"https://example.test/opus", "ext":"webm", "protocol":"https", "vcodec":"none", "language":"en"},
            {"format_id":"140", "url":"https://example.test/rejected", "ext":"m4a", "protocol":"https", "vcodec":"none", "language":"en"}
        ]
    }"#;
    let resolved = streams(output)?;
    assert_eq!(resolved.current.format_id.as_deref(), Some("140"));
    assert_eq!(
        resolved.alternatives.len(),
        1,
        "Keep a compatible alternate instead of retrying only the rejected rendition"
    );
    assert_eq!(
        resolved.alternatives[0].format_id.as_deref(),
        Some("140-alt")
    );
    Ok(())
}

#[test]
fn playback_uses_music_quality_opus_and_rejects_low_bitrate_fallbacks()
-> Result<(), Box<dyn std::error::Error>> {
    let output = r#"{
      "format_id":"251", "url":"https://example.test/opus", "language":"en",
      "formats":[
        {"format_id":"139", "url":"https://example.test/low", "abr":49, "ext":"m4a", "acodec":"mp4a.40.5", "protocol":"https", "vcodec":"none", "language":"en"},
        {"format_id":"250", "url":"https://example.test/low-opus", "abr":67, "ext":"webm", "acodec":"opus", "protocol":"https", "vcodec":"none", "language":"en"},
        {"format_id":"140", "url":"https://example.test/aac", "abr":129, "ext":"m4a", "acodec":"mp4a.40.2", "protocol":"https", "vcodec":"none", "language":"en"},
        {"format_id":"251-other", "url":"https://example.test/opus-alternative", "abr":150, "ext":"webm", "acodec":"opus", "protocol":"https", "vcodec":"none", "language":"en"}
      ]
    }"#;
    let result = streams(output)?;
    let ids: Vec<_> = result
        .alternatives
        .iter()
        .filter_map(|s| s.format_id.as_deref())
        .collect();
    assert_eq!(ids, ["251-other", "140"]);
    Ok(())
}

#[test]
fn discover_excludes_playlists_compilations_and_long_mixes() {
    let mut track = Track {
        id: "V_Ydoe4Q-Gg".to_owned().try_into().unwrap(),
        title: "Weird Fishes (Official Audio)".into(),
        artist: "Radiohead".into(),
        thumbnail: None,
        duration: Some(Duration::from_secs(318)),
    };
    assert!(discover_song(&track));
    for title in [
        "Indie dream pop playlist",
        "Summer compilation",
        "Radiohead Full Album",
        "Songs that feel like unsent messages",
        "Late night mix",
        "Live DJ set",
    ] {
        track.title = title.into();
        assert!(!discover_song(&track), "{title}");
    }
    track.title = "Song (Remix)".into();
    assert!(discover_song(&track));
    track.duration = Some(Duration::from_secs(3600));
    assert!(!discover_song(&track));
    track.duration = None;
    assert!(!discover_song(&track));
}

#[test]
#[ignore = "requires YouTube discovery network"]
fn live_discover_returns_individual_songs_with_thumbnail_sources()
-> Result<(), Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let songs = runtime.block_on(discover(Arc::new(Youtube::default())))?;
    assert!(!songs.is_empty());
    assert!(songs.len() <= 24);
    assert!(songs.iter().all(discover_song));
    assert!(songs.iter().all(|t| t.thumbnail.is_some()));
    println!("{} individual songs", songs.len());
    Ok(())
}
