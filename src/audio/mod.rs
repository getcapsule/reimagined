mod convert;
mod decode;
mod http;
mod output;
mod sequence;
mod source;
#[cfg(test)]
mod tests;

use convert::Converter;
use decode::Reader;
use output::Output;
pub use output::OutputDevice;
use sequence::{Block, Sequence, Sources};
pub use source::{AudioSource, MediaSource, Source, YouTubeSource};
use std::{
    cell::Cell,
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering::Relaxed},
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread,
    time::{Duration, Instant},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("Could not read audio: {0}")]
    Io(#[from] std::io::Error),
    #[error("Could not decode the track: {0}")]
    Decode(#[from] symphonia::core::errors::Error),
    #[error("{0}")]
    Unsupported(&'static str),
    #[error("The selected audio output is unavailable")]
    DeviceUnavailable,
    #[error("Could not enumerate outputs: {0}")]
    Devices(#[from] cpal::DevicesError),
    #[error("Could not configure the output: {0}")]
    Config(#[from] cpal::DefaultStreamConfigError),
    #[error("Could not open the output: {0}")]
    Stream(#[from] cpal::BuildStreamError),
    #[error("Could not start playback: {0}")]
    Play(#[from] cpal::PlayStreamError),
    #[error("Could not pause playback: {0}")]
    Pause(#[from] cpal::PauseStreamError),
    #[error("Could not configure the resampler: {0}")]
    Resampler(#[from] rubato::ResamplerConstructionError),
    #[error("Could not resample the track: {0}")]
    Convert(#[from] rubato::ResampleError),
    #[error("The playback worker stopped")]
    Disconnected,
}

#[derive(Debug, Clone, Copy)]
pub struct TrackInfo {
    pub input_rate: u32,
    pub input_channels: usize,
    pub output_rate: u32,
    pub output_channels: usize,
    pub duration: Option<Duration>,
}

#[derive(Debug)]
pub enum Event {
    Ready(TrackInfo),
    Advanced {
        frame: u64,
        info: TrackInfo,
    },
    /// Index relative to the first source in this playback request.
    Skipped {
        index: usize,
        error: AudioError,
    },
    NextFailed(AudioError),
    Ended,
    Failed(AudioError),
}

#[derive(Default)]
struct Meters {
    rate: AtomicU32,
    frames: AtomicU64,
    tail_ms: AtomicU64,
    volume: AtomicU8,
    device_failed: AtomicBool,
}

pub struct Playback {
    events: Receiver<Event>,
    meters: Arc<Meters>,
    origin: Cell<(u64, Duration)>,
}

impl Playback {
    pub fn poll(&self) -> Result<Option<Event>, AudioError> {
        match self.events.try_recv() {
            Ok(event) => {
                if let Event::Advanced { frame, .. } = &event {
                    self.origin.set((*frame, Duration::ZERO));
                }
                if matches!(event, Event::Skipped { index: 0, .. }) {
                    self.origin.set((0, Duration::ZERO));
                }
                Ok(Some(event))
            }
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err(AudioError::Disconnected),
        }
    }

    pub fn position(&self) -> Duration {
        let output_rate = self.meters.rate.load(Relaxed);
        let (frame, offset) = self.origin.get();
        if output_rate == 0 {
            return offset;
        }
        offset
            + Duration::from_secs_f64(
                self.meters.frames.load(Relaxed).saturating_sub(frame) as f64
                    / f64::from(output_rate),
            )
    }

    pub fn set_volume(&self, percent: u8) {
        self.meters.volume.store(percent.min(100), Relaxed);
    }
}

enum Command {
    Open {
        sources: Sources,
        device: OutputDevice,
        offset: Duration,
        playing: bool,
        meters: Arc<Meters>,
        events: Sender<Event>,
    },
    SetPlaying(bool),
    Stop,
}

#[derive(Clone)]
pub struct Player {
    commands: Sender<Command>,
}

impl Player {
    pub fn new() -> Result<Self, AudioError> {
        let (commands, inbox) = mpsc::channel();
        thread::Builder::new()
            .name("capsule-audio".into())
            .spawn(move || worker(inbox))?;
        Ok(Self { commands })
    }

    pub fn open(
        &self,
        source: impl AudioSource + 'static,
        device: OutputDevice,
        offset: Duration,
        playing: bool,
        volume: u8,
    ) -> Result<Playback, AudioError> {
        self.open_queue(std::iter::once(source), device, offset, playing, volume)
    }

    pub fn open_queue<S: AudioSource + 'static>(
        &self,
        sources: impl Iterator<Item = S> + Send + 'static,
        device: OutputDevice,
        offset: Duration,
        playing: bool,
        volume: u8,
    ) -> Result<Playback, AudioError> {
        let (events, receiver) = mpsc::channel();
        let meters = Arc::new(Meters::default());
        meters.volume.store(volume.min(100), Relaxed);
        self.send(Command::Open {
            sources: Box::new(sources.map(|source| Box::new(source) as Box<dyn AudioSource>)),
            device,
            offset,
            playing,
            meters: Arc::clone(&meters),
            events,
        })?;
        Ok(Playback {
            events: receiver,
            origin: Cell::new((0, offset)),
            meters,
        })
    }

    pub fn set_playing(&self, playing: bool) -> Result<(), AudioError> {
        self.send(Command::SetPlaying(playing))
    }
    pub fn stop(&self) -> Result<(), AudioError> {
        self.send(Command::Stop)
    }
    fn send(&self, command: Command) -> Result<(), AudioError> {
        self.commands
            .send(command)
            .map_err(|_| AudioError::Disconnected)
    }
}

struct Session {
    sequence: Sequence,
    boundaries: VecDeque<(u64, TrackInfo)>,
    output: Output,
    pending: Vec<f32>,
    sent: usize,
    ending: Option<Event>,
    drain_since: Option<Instant>,
    meters: Arc<Meters>,
    events: Sender<Event>,
    playing: bool,
}

enum Progress {
    More,
    Wait(Duration),
    Finished(Event),
}

impl Session {
    fn open(
        sources: Sources,
        device: &OutputDevice,
        offset: Duration,
        meters: Arc<Meters>,
        events: Sender<Event>,
    ) -> Result<Option<Self>, AudioError> {
        let notices = events.clone();
        let mut readers = sources.enumerate().filter_map(move |(index, source)| {
            let position = if index == 0 { offset } else { Duration::ZERO };
            match Reader::primed(source.as_ref(), position) {
                Err(error @ AudioError::Io(_)) => {
                    if notices.send(Event::Skipped { index, error }).is_err() {
                        return Some(Err(AudioError::Disconnected));
                    }
                    None
                }
                result => Some(result),
            }
        });
        let Some(first) = readers.next() else {
            return Ok(None);
        };
        let Some(reader) = first? else {
            if readers.next().is_some() {
                return Err(AudioError::Unsupported(
                    "Track contains no playable frames at this position",
                ));
            }
            return Ok(None);
        };
        let output = Output::new(device, Arc::clone(&meters))?;
        meters.rate.store(output.rate, Relaxed);
        let mut sequence = Sequence::new(reader, Box::new(readers), output.rate, output.channels)?;
        let mut pending = Vec::new();
        sequence.read(&mut pending)?;
        let info = sequence.info();
        if events.send(Event::Ready(info)).is_err() {
            return Ok(None);
        }
        Ok(Some(Self {
            sequence,
            boundaries: VecDeque::new(),
            output,
            pending,
            sent: 0,
            ending: None,
            drain_since: None,
            meters,
            events,
            playing: false,
        }))
    }

    fn set_playing(&mut self, playing: bool) -> Result<(), AudioError> {
        if playing == self.playing {
            return Ok(());
        }
        if playing {
            self.sent += self.output.write(&self.pending[self.sent..]);
            self.output.start()?;
        } else {
            self.output.pause()?;
            self.drain_since = None;
        }
        self.playing = playing;
        Ok(())
    }

    fn pump(&mut self) -> Result<Progress, AudioError> {
        if self.meters.device_failed.load(Relaxed) {
            return Err(AudioError::DeviceUnavailable);
        }
        let played = self.meters.frames.load(Relaxed);
        while self
            .boundaries
            .front()
            .is_some_and(|(frame, _)| *frame <= played)
        {
            if let Some((frame, info)) = self.boundaries.pop_front() {
                let _ = self.events.send(Event::Advanced { frame, info });
            }
        }
        if !self.playing {
            return Ok(Progress::Wait(Duration::from_millis(500)));
        }
        self.sent += self.output.write(&self.pending[self.sent..]);
        if self.sent < self.pending.len() {
            return Ok(Progress::Wait(Duration::from_millis(12)));
        }
        if self.ending.is_some() {
            if self.output.drained(&self.meters) {
                let drained = self.drain_since.get_or_insert_with(Instant::now);
                if drained.elapsed() >= Duration::from_millis(self.meters.tail_ms.load(Relaxed))
                    && let Some(end) = self.ending.take()
                {
                    return Ok(Progress::Finished(end));
                }
            }
            return Ok(Progress::Wait(Duration::from_millis(10)));
        }
        match self.sequence.read(&mut self.pending)? {
            Block::Audio => {}
            Block::Boundary { frame, info } => self.boundaries.push_back((frame, info)),
            Block::End => self.ending = Some(Event::Ended),
            Block::NextFailed(error) => self.ending = Some(Event::NextFailed(error)),
        }
        self.sent = 0;
        Ok(Progress::More)
    }
}

fn worker(inbox: Receiver<Command>) {
    let mut session: Option<Session> = None;
    loop {
        let command = if let Some(active) = &mut session {
            let delay = match active.pump() {
                Ok(Progress::More) => Duration::ZERO,
                Ok(Progress::Wait(delay)) => delay,
                Ok(Progress::Finished(end)) => {
                    let _ = active.events.send(end);
                    session = None;
                    continue;
                }
                Err(error) => {
                    let _ = active.events.send(Event::Failed(error));
                    session = None;
                    continue;
                }
            };
            match inbox.recv_timeout(delay) {
                Ok(command) => command,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            let Ok(command) = inbox.recv() else {
                break;
            };
            command
        };
        match command {
            Command::Stop => session = None,
            Command::Open {
                sources,
                device,
                offset,
                playing,
                meters,
                events,
            } => {
                drop(session.take());
                match Session::open(sources, &device, offset, meters, events.clone()) {
                    Ok(Some(mut next)) => match next.set_playing(playing) {
                        Ok(()) => session = Some(next),
                        Err(error) => {
                            let _ = events.send(Event::Failed(error));
                        }
                    },
                    Ok(None) => {
                        let _ = events.send(Event::Ended);
                    }
                    Err(error) => {
                        let _ = events.send(Event::Failed(error));
                    }
                }
            }
            Command::SetPlaying(playing) => {
                if let Some(active) = &mut session
                    && let Err(error) = active.set_playing(playing)
                {
                    let _ = active.events.send(Event::Failed(error));
                    session = None;
                }
            }
        }
    }
}
