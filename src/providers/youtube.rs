use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use thiserror::Error;
use yt_dlp::executor::Executor;

#[derive(Debug, Error)]
pub enum Error {
    #[error("YouTube helper: {0}")]
    Helper(#[from] yt_dlp::error::Error),
    #[error("YouTube network request: {0}")]
    Http(#[from] reqwest::Error),
    #[error("YouTube metadata: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Unavailable(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct VideoId(String);
impl TryFrom<String> for VideoId {
    type Error = String;
    fn try_from(value: String) -> Result<Self, String> {
        if value.len() == 11
            && value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        {
            Ok(Self(value))
        } else {
            Err("Invalid YouTube video ID".into())
        }
    }
}
impl From<VideoId> for String {
    fn from(id: VideoId) -> Self {
        id.0
    }
}
impl VideoId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn url(&self) -> String {
        format!("https://www.youtube.com/watch?v={}", self.0)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: VideoId,
    pub title: String,
    pub artist: String,
    pub thumbnail: Option<String>,
    pub duration: Option<Duration>,
}
#[derive(Default)]
pub struct Youtube {
    install: tokio::sync::Mutex<()>,
}
#[derive(Deserialize)]
struct Entry {
    id: String,
    title: Option<String>,
    uploader: Option<String>,
    channel: Option<String>,
    artist: Option<String>,
    thumbnail: Option<String>,
    #[serde(default)]
    thumbnails: Vec<Thumbnail>,
    duration: Option<f64>,
    live_status: Option<String>,
    availability: Option<String>,
}
#[derive(Deserialize)]
struct Thumbnail {
    url: String,
}
#[derive(Deserialize)]
struct Results {
    #[serde(default)]
    entries: Vec<Option<Entry>>,
}
impl Entry {
    fn track(self) -> Option<Track> {
        if self
            .live_status
            .as_deref()
            .is_some_and(|s| s == "is_live" || s == "is_upcoming")
            || self
                .availability
                .as_deref()
                .is_some_and(|s| !matches!(s, "public" | "unlisted"))
        {
            return None;
        }
        Some(Track {
            id: self.id.try_into().ok()?,
            title: self.title?,
            artist: self
                .artist
                .or(self.uploader)
                .or(self.channel)
                .unwrap_or_default(),
            thumbnail: self
                .thumbnail
                .or_else(|| self.thumbnails.last().map(|t| t.url.clone())),
            duration: self
                .duration
                .filter(|s| s.is_finite() && *s > 0. && *s < 86400.)
                .map(Duration::from_secs_f64),
        })
    }
}

fn tools_dir() -> Result<PathBuf, Error> {
    crate::library::data_dir()
        .map(|d| d.join("tools"))
        .map_err(|e| Error::Unavailable(e.to_string()))
}
fn binary() -> Result<PathBuf, Error> {
    if let Some(path) = std::env::var_os("CAPSULE_YTDLP") {
        return Ok(path.into());
    }
    let name = if cfg!(windows) {
        "yt-dlp.exe"
    } else {
        "yt-dlp"
    };
    let local = std::env::current_exe()?
        .parent()
        .map(|p| p.join("tools").join(name));
    if let Some(path) = local.filter(|p| p.is_file()) {
        return Ok(path);
    }
    Ok(tools_dir()?.join(name))
}
impl Youtube {
    async fn prepare(&self) -> Result<(), Error> {
        let _guard = self.install.lock().await;
        let path = binary()?;
        if !path.is_file() {
            if std::env::var_os("CAPSULE_YTDLP").is_some() {
                return Err(Error::Unavailable(
                    "CAPSULE_YTDLP points to a missing executable".into(),
                ));
            }
            let dir = tools_dir()?;
            let parent = dir
                .parent()
                .ok_or_else(|| Error::Unavailable("Missing tools directory".into()))?;
            std::fs::create_dir_all(parent)?;
            let staging = tempfile::tempdir_in(parent)?;
            let installer =
                yt_dlp::client::deps::LibraryInstaller::new(staging.path().join("install"));
            let downloaded =
                tokio::time::timeout(Duration::from_secs(90), installer.install_youtube(None))
                    .await
                    .map_err(|_| {
                        Error::Unavailable("YouTube helper download timed out".into())
                    })??;
            std::fs::create_dir_all(&dir)?;
            std::fs::rename(downloaded, path)?;
        }
        prepare_runtime().await?;
        Ok(())
    }
    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<Track>, Error> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        self.prepare().await?;
        let output = execute(vec![
            "--flat-playlist".into(),
            format!("ytsearch{}:{}", limit.clamp(1, 20), query.trim()),
        ])
        .await?;
        let result: Results = serde_json::from_str(&output)?;
        let mut seen = std::collections::HashSet::new();
        Ok(result
            .entries
            .into_iter()
            .flatten()
            .filter_map(Entry::track)
            .filter(|t| seen.insert(t.id.as_str().to_owned()))
            .collect())
    }
}
fn runtime() -> Option<(&'static str, PathBuf)> {
    for name in ["deno", "node"] {
        let executable = if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.to_owned()
        };
        if let Some(paths) = std::env::var_os("PATH")
            && let Some(path) = std::env::split_paths(&paths)
                .map(|p| p.join(&executable))
                .find(|p| p.is_file())
        {
            return Some((if name == "deno" { "deno" } else { "node" }, path));
        }
        if let Ok(path) = tools_dir().map(|p| p.join(&executable))
            && path.is_file()
        {
            return Some((if name == "deno" { "deno" } else { "node" }, path));
        }
    }
    None
}
async fn prepare_runtime() -> Result<(), Error> {
    if runtime().is_some() {
        return Ok(());
    }
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        _ => {
            return Err(Error::Unavailable(
                "Install Node.js or Deno to enable YouTube playback on this platform".into(),
            ));
        }
    };
    let url =
        format!("https://github.com/denoland/deno/releases/latest/download/deno-{target}.zip");
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(90))
        .build()?
        .get(url)
        .send()
        .await?
        .error_for_status()?;
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > 64 * 1024 * 1024 {
            return Err(Error::Unavailable("Runtime archive is too large".into()));
        }
        bytes.extend_from_slice(&chunk);
    }
    let dir = tools_dir()?;
    tokio::task::spawn_blocking(move || -> Result<(), Error> {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
            .map_err(|e| Error::Unavailable(e.to_string()))?;
        let name = if cfg!(windows) { "deno.exe" } else { "deno" };
        let mut entry = archive
            .by_name(name)
            .map_err(|e| Error::Unavailable(e.to_string()))?;
        std::fs::create_dir_all(&dir)?;
        let mut file = tempfile::NamedTempFile::new_in(&dir)?;
        if entry.size() > 160 * 1024 * 1024 {
            return Err(Error::Unavailable("Runtime executable is too large".into()));
        }
        std::io::copy(&mut entry, &mut file)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.as_file()
                .set_permissions(std::fs::Permissions::from_mode(0o755))?;
        }
        file.persist(dir.join(name))
            .map_err(|e| Error::Io(e.error))?;
        Ok(())
    })
    .await
    .map_err(|e| Error::Unavailable(e.to_string()))?
}

async fn execute(extra: Vec<String>) -> Result<String, Error> {
    let mut args: Vec<String> = [
        "--ignore-config",
        "--no-cache-dir",
        "--skip-download",
        "--no-check-formats",
        "--no-progress",
        "--no-warnings",
        "--socket-timeout",
        "12",
        "--retries",
        "1",
        "--extractor-retries",
        "1",
        "--dump-single-json",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    if let Some((name, path)) = runtime() {
        args.extend(["--js-runtimes".into(), format!("{name}:{}", path.display())]);
    }
    args.extend(extra);
    let result = Executor {
        executable_path: binary()?,
        timeout: Duration::from_secs(45),
        args,
    }
    .execute()
    .await?;
    Ok(result.stdout)
}
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Stream {
    #[serde(default)]
    pub format_id: Option<String>,
    pub url: String,
    #[serde(default)]
    pub http_headers: HashMap<String, String>,
}
#[derive(Debug, Clone)]
pub(crate) struct Streams {
    pub current: Stream,
    pub alternatives: VecDeque<Stream>,
}
#[derive(Deserialize)]
struct Format {
    format_id: Option<String>,
    url: Option<String>,
    #[serde(default)]
    http_headers: HashMap<String, String>,
    ext: Option<String>,
    protocol: Option<String>,
    vcodec: Option<String>,
    language: Option<String>,
    audio_channels: Option<u32>,
    abr: Option<f64>,
    acodec: Option<String>,
}
#[derive(Deserialize)]
struct Selection {
    #[serde(flatten)]
    current: Stream,
    language: Option<String>,
    #[serde(default)]
    formats: Vec<Format>,
}
fn streams(output: &str) -> Result<Streams, Error> {
    let Selection {
        current,
        language,
        formats,
    } = serde_json::from_str(output)?;
    let mut alternatives: VecDeque<Stream> = VecDeque::new();
    for format in formats.into_iter().rev().filter(|f| {
        matches!(
            (f.ext.as_deref(), f.acodec.as_deref()),
            (Some("m4a"), Some("mp4a.40.2")) | (Some("webm"), Some("opus"))
        ) && f.abr.is_some_and(|rate| rate >= 96.)
            && matches!(f.protocol.as_deref(), Some("https" | "http"))
            && f.vcodec.as_deref() == Some("none")
            && f.language == language
            && f.audio_channels
                .is_none_or(|channels| matches!(channels, 1 | 2))
    }) {
        let Some(url) = format.url else { continue };
        if url != current.url && !alternatives.iter().any(|s| s.url == url) {
            alternatives.push_back(Stream {
                format_id: format.format_id,
                url,
                http_headers: format.http_headers,
            });
        }
    }
    Ok(Streams {
        current,
        alternatives,
    })
}
pub(crate) fn resolve(id: &VideoId, format: Option<&str>) -> Result<Streams, Error> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        if !binary()?.is_file() {
            return Err(Error::Unavailable(
                "Open YouTube search first to install the playback helper.".into(),
            ));
        }
        let arguments = || vec![
            "--no-playlist".into(),
            "-f".into(),
            format
                .unwrap_or("bestaudio[ext=webm][acodec=opus][abr>=96][protocol=https]/bestaudio[ext=m4a][acodec=mp4a.40.2][abr>=96][protocol=https]")
                .into(),
            id.url(),
        ];
        let mut primary = arguments();
        primary.extend(["--extractor-args".into(), "youtube:player_client=visionos;skip=hls,dash".into()]);
        let output = match execute(primary).await {
            Ok(output) => output,
            Err(Error::Helper(yt_dlp::error::Error::CommandFailed { stderr, .. }))
                if stderr.contains("Requested format is not available") => execute(arguments()).await?,
            Err(error) => return Err(error),
        };
        streams(&output)
    })
}

pub(crate) fn discover_song(track: &Track) -> bool {
    let Some(duration) = track.duration else {
        return false;
    };
    if !(45..=900).contains(&duration.as_secs()) {
        return false;
    }
    let title = track.title.to_lowercase();
    ![
        "playlist",
        "compilation",
        "full album",
        "full ep",
        "greatest hits",
        "songs that",
        "hour",
        "dj set",
        "megamix",
    ]
    .iter()
    .any(|term| title.contains(term))
        && !title
            .split(|c: char| !c.is_alphanumeric())
            .any(|word| matches!(word, "mix" | "mixes"))
}
pub async fn discover(provider: Arc<Youtube>) -> Result<Vec<Track>, Error> {
    let mut artists = [
        "Radiohead",
        "Sade",
        "Beach House",
        "Slowdive",
        "Brent Faiyaz",
        "Daft Punk",
        "Mazzy Star",
        "Erykah Badu",
        "Cocteau Twins",
        "Massive Attack",
        "Panchiko",
        "Frank Ocean",
        "The Cure",
        "Gorillaz",
        "Kendrick Lamar",
        "Portishead",
        "Tame Impala",
        "Aaliyah",
        "Björk",
        "D'Angelo",
        "Yung Lean",
        "Men I Trust",
        "Deftones",
        "FKA twigs",
    ];
    fastrand::shuffle(&mut artists);
    let queries = artists[..3]
        .iter()
        .map(|artist| format!("{artist} official audio -playlist -compilation"))
        .collect::<Vec<_>>();
    let (a, b, c) = tokio::join!(
        provider.search(&queries[0], 12),
        provider.search(&queries[1], 12),
        provider.search(&queries[2], 12)
    );
    let mut songs = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut last_error = None;
    for result in [a, b, c] {
        match result {
            Ok(tracks) => songs.extend(
                tracks
                    .into_iter()
                    .filter(discover_song)
                    .filter(|t| seen.insert(t.id.as_str().to_owned()))
                    .take(8),
            ),
            Err(error) => last_error = Some(error),
        }
    }
    if songs.is_empty() {
        return Err(last_error.unwrap_or_else(|| {
            Error::Unavailable(
                "No individual songs found. Refresh to try another selection.".into(),
            )
        }));
    }
    fastrand::shuffle(&mut songs);
    songs.truncate(24);
    Ok(songs)
}

#[cfg(test)]
#[path = "youtube_tests.rs"]
mod tests;
