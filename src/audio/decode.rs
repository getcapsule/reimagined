use super::{AudioError, AudioSource};
use std::time::Duration;
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{CODEC_TYPE_OPUS, CodecParameters, CodecRegistry, Decoder},
    errors::Error,
    formats::{FormatReader, SeekMode, SeekTo},
    io::MediaSourceStream,
    probe::Hint,
    units::{Time, TimeBase},
};

pub(super) struct Reader {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track: u32,
    clock: Option<TimeBase>,
    seek_target: u64,
    buffer: Option<SampleBuffer<f32>>,
    offset: usize,
    pub(super) rate: u32,
    pub(super) channels: usize,
    pub(super) duration: Option<Duration>,
}

impl Reader {
    pub(super) fn primed(
        source: &dyn AudioSource,
        position: Duration,
    ) -> Result<Option<Self>, AudioError> {
        let mut reader = Self::open(source, position)?;
        Ok(reader.read()?.then_some(reader))
    }

    pub(super) fn open(source: &dyn AudioSource, position: Duration) -> Result<Self, AudioError> {
        let mut hint = Hint::new();
        if let Some(extension) = source.extension() {
            hint.with_extension(extension);
        }
        let stream = MediaSourceStream::new(source.open_media()?, Default::default());
        let mut format = symphonia::default::get_probe()
            .format(&hint, stream, &Default::default(), &Default::default())?
            .format;
        let mut codecs = CodecRegistry::new();
        symphonia::default::register_enabled_codecs(&mut codecs);
        codecs.register_all::<symphonia_adapter_libopus::OpusDecoder>();
        let (track, decoder) = format
            .tracks()
            .iter()
            .find_map(|track| {
                let mut opus_params = CodecParameters::new();
                let params = if track.codec_params.codec == CODEC_TYPE_OPUS {
                    opus_params.codec = CODEC_TYPE_OPUS;
                    opus_params.sample_rate = track.codec_params.sample_rate;
                    opus_params.channels = track.codec_params.channels.or_else(|| {
                        track
                            .codec_params
                            .channel_layout
                            .map(|layout| layout.into_channels())
                    });
                    &opus_params
                } else {
                    &track.codec_params
                };
                codecs
                    .make(params, &Default::default())
                    .ok()
                    .map(|decoder| (track, decoder))
            })
            .ok_or(AudioError::Unsupported("No supported audio track"))?;
        let id = track.id;
        let params = &track.codec_params;
        let clock = params.time_base.or_else(|| {
            Some(TimeBase::new(
                1,
                params.sample_rate.filter(|rate| *rate > 0)?,
            ))
        });
        let duration = clock.zip(params.n_frames).map(|(base, frames)| {
            let time = base.calc_time(frames);
            Duration::from_secs(time.seconds) + Duration::from_secs_f64(time.frac)
        });
        let seek_target = if position.is_zero() {
            0
        } else {
            format
                .seek(
                    SeekMode::Accurate,
                    SeekTo::Time {
                        time: Time::new(
                            position.as_secs(),
                            f64::from(position.subsec_nanos()) / 1e9,
                        ),
                        track_id: Some(id),
                    },
                )?
                .required_ts
        };
        Ok(Self {
            format,
            decoder,
            track: id,
            clock,
            seek_target,
            buffer: None,
            offset: 0,
            rate: 0,
            channels: 0,
            duration,
        })
    }

    pub(super) fn samples(&self) -> &[f32] {
        self.buffer
            .as_ref()
            .map_or(&[], |buffer| &buffer.samples()[self.offset..])
    }

    pub(super) fn read(&mut self) -> Result<bool, AudioError> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    return Ok(false);
                }
                Err(e) => return Err(e.into()),
            };
            if packet.track_id() != self.track {
                continue;
            }
            let audio = self.decoder.decode(&packet)?;
            let spec = *audio.spec();
            let channels = spec.channels.count();
            if spec.rate == 0 || channels == 0 || channels > 2 {
                return Err(AudioError::Unsupported(
                    "This player supports mono and stereo tracks",
                ));
            }
            if self.rate != 0 && (self.rate != spec.rate || self.channels != channels) {
                return Err(AudioError::Unsupported(
                    "The audio format changed within the track",
                ));
            }
            self.rate = spec.rate;
            self.channels = channels;
            let skip = self.clock.map_or(0, |clock| {
                let time = clock.calc_time(self.seek_target.saturating_sub(packet.ts()));
                ((time.seconds as f64 + time.frac) * f64::from(spec.rate)).round() as usize
            });
            if skip >= audio.frames() {
                continue;
            }
            self.seek_target = 0;
            let capacity = audio.capacity();
            let buffer = self
                .buffer
                .get_or_insert_with(|| SampleBuffer::new(capacity as u64, spec));
            if buffer.capacity() < capacity * channels {
                *buffer = SampleBuffer::new(capacity as u64, spec);
            }
            buffer.copy_interleaved_ref(audio);
            self.offset = skip * channels;
            return Ok(true);
        }
    }
}
