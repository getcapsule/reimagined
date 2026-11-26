use super::{
    AudioError, MediaSource,
    source::{Resolution, StreamCache},
};
use crate::providers::youtube::{self, VideoId};
use reqwest::{
    StatusCode,
    blocking::Client,
    header::{CONTENT_RANGE, HeaderMap, HeaderName, HeaderValue, RANGE},
};
use std::{
    collections::VecDeque,
    io::{self, Read, Seek, SeekFrom},
    sync::Arc,
    time::Duration,
};
const CHUNK: u64 = 256 * 1024;
const INITIAL_CHUNK: u64 = 16 * 1024;

pub(super) struct HttpAudio {
    client: Client,
    id: VideoId,
    cache: Arc<StreamCache>,
    stream: youtube::Stream,
    alternatives: VecDeque<youtube::Stream>,
    position: u64,
    length: Option<u64>,
    start: u64,
    bytes: Vec<u8>,
}
impl HttpAudio {
    pub(super) fn open(id: &VideoId, cache: Arc<StreamCache>) -> Result<Self, AudioError> {
        let youtube::Streams {
            current: stream,
            alternatives,
        } = cache.resolve(id, None, Resolution::Reuse)?;
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| AudioError::Io(io::Error::other(e)))?;
        let mut source = Self {
            client,
            id: id.clone(),
            cache,
            stream,
            alternatives,
            position: 0,
            length: None,
            start: 0,
            bytes: Vec::new(),
        };
        source.fill()?;
        Ok(source)
    }
    fn chunk_size(&self) -> u64 {
        if self.length.is_none() {
            INITIAL_CHUNK
        } else {
            CHUNK
        }
    }
    fn request(&mut self) -> io::Result<reqwest::blocking::Response> {
        loop {
            let mut headers = HeaderMap::new();
            for (name, value) in &self.stream.http_headers {
                headers.insert(
                    HeaderName::from_bytes(name.as_bytes()).map_err(io::Error::other)?,
                    HeaderValue::from_str(value).map_err(io::Error::other)?,
                );
            }
            let response = self
                .client
                .get(&self.stream.url)
                .headers(headers)
                .header(
                    RANGE,
                    format!(
                        "bytes={}-{}",
                        self.position,
                        self.position.saturating_add(self.chunk_size() - 1)
                    ),
                )
                .send()
                .map_err(io::Error::other)?;
            if self.length.is_none()
                && matches!(
                    response.status(),
                    StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED
                )
                && let Some(alternative) = self.alternatives.pop_front()
            {
                self.stream = alternative;
                continue;
            }
            return Ok(response);
        }
    }
    fn fill(&mut self) -> io::Result<()> {
        let mut response = self.request()?;
        if matches!(
            response.status(),
            StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED
        ) {
            let format = if self.length.is_some() {
                self.stream.format_id.as_deref()
            } else {
                None
            };
            let fresh = self.cache.resolve(&self.id, format, Resolution::Refresh)?;
            self.stream = fresh.current;
            self.alternatives = fresh.alternatives;
            response = self.request()?;
        }
        if response.status().is_client_error() || response.status().is_server_error() {
            return Err(io::Error::other(format!(
                "YouTube audio request failed ({})",
                response.status()
            )));
        }
        if response.status() != StatusCode::PARTIAL_CONTENT {
            return Err(io::Error::other(format!(
                "Audio server did not provide a seekable stream ({})",
                response.status()
            )));
        }
        let range = response
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.strip_prefix("bytes "))
            .ok_or_else(|| io::Error::other("Missing audio byte range"))?;
        let (bounds, total) = range
            .split_once('/')
            .ok_or_else(|| io::Error::other("Invalid audio byte range"))?;
        let start = bounds
            .split_once('-')
            .and_then(|(start, _)| start.parse::<u64>().ok());
        let length = total.parse::<u64>().map_err(io::Error::other)?;
        if start != Some(self.position) || self.length.is_some_and(|old| old != length) {
            return Err(io::Error::other("Audio stream changed during playback"));
        }
        let chunk = self.chunk_size();
        let mut bytes = Vec::with_capacity(chunk as usize);
        response.take(chunk + 1).read_to_end(&mut bytes)?;
        if bytes.len() > chunk as usize || bytes.is_empty() {
            return Err(io::Error::other("Invalid audio response size"));
        }
        self.length = Some(length);
        self.start = self.position;
        self.bytes = bytes;
        Ok(())
    }
}
impl Read for HttpAudio {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() || self.length.is_some_and(|len| self.position >= len) {
            return Ok(0);
        }
        if self.position < self.start || self.position - self.start >= self.bytes.len() as u64 {
            self.fill()?;
        }
        let offset = (self.position - self.start) as usize;
        let count = out.len().min(self.bytes.len() - offset);
        out[..count].copy_from_slice(&self.bytes[offset..offset + count]);
        self.position += count as u64;
        Ok(count)
    }
}
impl Seek for HttpAudio {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let target = match from {
            SeekFrom::Start(p) => i128::from(p),
            SeekFrom::Current(p) => i128::from(self.position) + i128::from(p),
            SeekFrom::End(p) => {
                i128::from(
                    self.length
                        .ok_or_else(|| io::Error::other("Unknown stream length"))?,
                ) + i128::from(p)
            }
        };
        self.position = u64::try_from(target)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Invalid stream position"))?;
        Ok(self.position)
    }
}
impl MediaSource for HttpAudio {
    fn is_seekable(&self) -> bool {
        true
    }
    fn byte_len(&self) -> Option<u64> {
        self.length
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, net::TcpListener, thread};
    #[test]
    fn bounded_ranges_support_forward_and_backward_seeks() -> Result<(), Box<dyn std::error::Error>>
    {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let total = CHUNK * 3 + 19;
        let server = thread::spawn(move || -> io::Result<()> {
            for (request_index, expected) in [0, total - 32, 0].into_iter().enumerate() {
                let (mut socket, _) = listener.accept()?;
                socket.set_read_timeout(Some(Duration::from_secs(5)))?;
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    socket.read_exact(&mut byte)?;
                    request.push(byte[0]);
                }
                let request = String::from_utf8_lossy(&request).to_lowercase();
                let size = if request_index == 0 { 16 * 1024 } else { CHUNK };
                let request_end = expected + size - 1;
                let end = request_end.min(total - 1);
                assert!(request.contains(&format!("range: bytes={expected}-{request_end}")));
                write!(
                    socket,
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {expected}-{end}/{total}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    end - expected + 1
                )?;
                let bytes: Vec<_> = (expected..=end).map(|n| (n % 251) as u8).collect();
                socket.write_all(&bytes)?;
            }
            Ok(())
        });
        let mut source = HttpAudio {
            cache: Arc::default(),
            client: Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(5))
                .build()?,
            id: "V_Ydoe4Q-Gg"
                .to_owned()
                .try_into()
                .map_err(io::Error::other)?,
            stream: youtube::Stream {
                format_id: None,
                url: format!("http://{address}/audio"),
                http_headers: Default::default(),
            },
            alternatives: VecDeque::new(),
            position: 0,
            length: None,
            start: 0,
            bytes: Vec::new(),
        };
        source.fill()?;
        assert_eq!(source.byte_len(), Some(total));
        assert!(source.bytes.len() <= CHUNK as usize);
        let mut first = [0; 32];
        source.read_exact(&mut first)?;
        source.seek(SeekFrom::End(-32))?;
        let mut last = [0; 32];
        source.read_exact(&mut last)?;
        assert_eq!(last[0], ((total - 32) % 251) as u8);
        source.seek(SeekFrom::Start(0))?;
        let mut again = [0; 32];
        source.read_exact(&mut again)?;
        assert_eq!(first, again);
        assert!(source.seek(SeekFrom::Current(-100)).is_err());
        server.join().expect("range server")?;
        Ok(())
    }
    #[test]
    fn forbidden_initial_rendition_uses_alternate_without_changing_active_streams()
    -> Result<(), Box<dyn std::error::Error>> {
        for opened in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0")?;
            let address = listener.local_addr()?;
            let server = thread::spawn(move || -> io::Result<()> {
                let paths: &[&str] = if opened {
                    &["/blocked"]
                } else {
                    &["/blocked", "/alternate"]
                };
                for path in paths {
                    let (mut socket, _) = listener.accept()?;
                    socket.set_read_timeout(Some(Duration::from_secs(3)))?;
                    let mut request = Vec::new();
                    let mut byte = [0];
                    while !request.ends_with(b"\r\n\r\n") {
                        socket.read_exact(&mut byte)?;
                        request.push(byte[0]);
                    }
                    assert!(String::from_utf8_lossy(&request).starts_with(&format!("GET {path} ")));
                    if *path == "/blocked" {
                        socket.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")?;
                    } else {
                        socket.write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-3/4\r\nContent-Length: 4\r\nConnection: close\r\n\r\ntest")?;
                    }
                }
                Ok(())
            });
            let mut source = HttpAudio {
                cache: Arc::default(),
                client: Client::builder()
                    .no_proxy()
                    .timeout(Duration::from_secs(3))
                    .build()?,
                id: "Ve-rxwKuqtI"
                    .to_owned()
                    .try_into()
                    .map_err(io::Error::other)?,
                stream: youtube::Stream {
                    format_id: Some("140".into()),
                    url: format!("http://{address}/blocked"),
                    http_headers: Default::default(),
                },
                alternatives: VecDeque::from([youtube::Stream {
                    format_id: Some("139".into()),
                    url: format!("http://{address}/alternate"),
                    http_headers: Default::default(),
                }]),
                position: if opened { 2 } else { 0 },
                length: if opened { Some(4) } else { None },
                start: 0,
                bytes: Vec::new(),
            };
            let response = source.request()?;
            assert_eq!(
                response.status(),
                if opened {
                    StatusCode::FORBIDDEN
                } else {
                    StatusCode::PARTIAL_CONTENT
                }
            );
            server.join().expect("rendition server")?;
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires YouTube network"]
    fn live_alternate_opus_decodes_and_seeks_after_403() -> Result<(), Box<dyn std::error::Error>> {
        use crate::audio::{AudioSource, Reader};
        use std::cell::RefCell;
        struct Once(RefCell<Option<HttpAudio>>);
        impl AudioSource for Once {
            fn open_media(&self) -> Result<Box<dyn MediaSource>, AudioError> {
                self.0
                    .borrow_mut()
                    .take()
                    .map(|source| Box::new(source) as Box<dyn MediaSource>)
                    .ok_or(AudioError::Unsupported("Test stream was already opened"))
            }
            fn extension(&self) -> Option<&str> {
                Some("webm")
            }
        }
        let id = "Ve-rxwKuqtI"
            .to_owned()
            .try_into()
            .map_err(io::Error::other)?;
        let alternate = youtube::resolve(&id, Some("251"))?.current;
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let server = thread::spawn(move || -> io::Result<()> {
            let (mut socket, _) = listener.accept()?;
            socket.set_read_timeout(Some(Duration::from_secs(3)))?;
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") {
                socket.read_exact(&mut byte)?;
                request.push(byte[0]);
            }
            socket.write_all(
                b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
        });
        let mut source = HttpAudio {
            cache: Arc::default(),
            client: Client::builder().timeout(Duration::from_secs(20)).build()?,
            id,
            stream: youtube::Stream {
                format_id: Some("140".into()),
                url: format!("http://{address}/blocked"),
                http_headers: Default::default(),
            },
            alternatives: VecDeque::from([alternate]),
            position: 0,
            length: None,
            start: 0,
            bytes: Vec::new(),
        };
        source.fill()?;
        server.join().expect("403 server")?;
        assert_eq!(source.stream.format_id.as_deref(), Some("251"));
        let source = Once(RefCell::new(Some(source)));
        let reader = Reader::primed(&source, Duration::from_secs(17))?.ok_or("No Opus frames")?;
        assert!(!reader.samples().is_empty());
        assert!(
            reader
                .duration
                .is_some_and(|duration| duration > Duration::from_secs(400))
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires YouTube network"]
    fn live_music_quality_startup_and_cached_seek() -> Result<(), Box<dyn std::error::Error>> {
        use crate::audio::{AudioSource, Reader};
        use std::time::Instant;
        struct CachedSource {
            id: VideoId,
            cache: Arc<StreamCache>,
        }
        impl AudioSource for CachedSource {
            fn open_media(&self) -> Result<Box<dyn MediaSource>, AudioError> {
                Ok(Box::new(HttpAudio::open(
                    &self.id,
                    Arc::clone(&self.cache),
                )?))
            }
        }
        let source = CachedSource {
            id: "Ve-rxwKuqtI"
                .to_owned()
                .try_into()
                .map_err(io::Error::other)?,
            cache: Arc::default(),
        };
        let start = Instant::now();
        let streams = source.cache.resolve(&source.id, None, Resolution::Reuse)?;
        let resolution_ms = start.elapsed().as_millis();
        let start = Instant::now();
        let reader = Reader::primed(&source, Duration::ZERO)?.ok_or("No music frames")?;
        assert!(matches!(reader.rate, 44100 | 48000));
        let first_decode_ms = start.elapsed().as_millis();
        drop(reader);
        let start = Instant::now();
        let reader =
            Reader::primed(&source, Duration::from_secs(17))?.ok_or("No frames after seek")?;
        assert!(!reader.samples().is_empty());
        eprintln!(
            "format={:?} resolution_ms={resolution_ms} first_decode_ms={first_decode_ms} cached_seek_ms={} rate={} channels={}",
            streams.current.format_id,
            start.elapsed().as_millis(),
            reader.rate,
            reader.channels
        );
        Ok(())
    }
}
