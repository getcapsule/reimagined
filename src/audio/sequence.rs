use super::{AudioError, AudioSource, Converter, Reader, TrackInfo};

#[cfg(test)]
mod tests;

pub(super) type Sources = Box<dyn Iterator<Item = Box<dyn AudioSource>> + Send>;

type Readers = Box<dyn Iterator<Item = Result<Option<Reader>, AudioError>> + Send>;

pub(super) enum Block {
    Audio,
    Boundary { frame: u64, info: TrackInfo },
    End,
    NextFailed(AudioError),
}

pub(super) struct Sequence {
    reader: Reader,
    remaining: Readers,
    converter: Converter,
    output_rate: u32,
    output_channels: usize,
    run_start: u64,
    input_frames: u64,
    primed: bool,
}

impl Sequence {
    pub(super) fn new(
        reader: Reader,
        remaining: Readers,
        output_rate: u32,
        output_channels: usize,
    ) -> Result<Self, AudioError> {
        let converter = Converter::new(reader.rate, output_rate, reader.channels, output_channels)?;
        Ok(Self {
            reader,
            remaining,
            converter,
            output_rate,
            output_channels,
            run_start: 0,
            input_frames: 0,
            primed: true,
        })
    }

    pub(super) fn info(&self) -> TrackInfo {
        TrackInfo {
            input_rate: self.reader.rate,
            input_channels: self.reader.channels,
            output_rate: self.output_rate,
            output_channels: self.output_channels,
            duration: self.reader.duration,
        }
    }

    pub(super) fn read(&mut self, pcm: &mut Vec<f32>) -> Result<Block, AudioError> {
        if self.primed || self.reader.read()? {
            self.primed = false;
            let samples = self.reader.samples();
            self.input_frames += (samples.len() / self.reader.channels) as u64;
            self.converter.push(samples, pcm)?;
            return Ok(Block::Audio);
        }
        let Some(result) = self.remaining.next() else {
            self.converter.finish(pcm)?;
            return Ok(Block::End);
        };
        let next = match result {
            Ok(Some(reader)) => reader,
            result => {
                self.converter.finish(pcm)?;
                return Ok(Block::NextFailed(result.err().unwrap_or(
                    AudioError::Unsupported("Track contains no audio frames"),
                )));
            }
        };
        let rate = u64::from(self.reader.rate);
        let frame =
            self.run_start + (self.input_frames * u64::from(self.output_rate) + rate / 2) / rate;
        if (next.rate, next.channels) == (self.reader.rate, self.reader.channels) {
            pcm.clear();
        } else {
            self.converter.finish(pcm)?;
            self.converter = Converter::new(
                next.rate,
                self.output_rate,
                next.channels,
                self.output_channels,
            )?;
            self.run_start = frame;
            self.input_frames = 0;
        }
        self.reader = next;
        self.primed = true;
        Ok(Block::Boundary {
            frame,
            info: self.info(),
        })
    }
}
