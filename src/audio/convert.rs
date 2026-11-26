use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};

use super::AudioError;

const CHUNK: usize = 512;

pub(super) struct Converter {
    input_channels: usize,
    output_channels: usize,
    filter: Option<SincFilter>,
}

impl Converter {
    pub(super) fn new(
        input_rate: u32,
        output_rate: u32,
        input_channels: usize,
        output_channels: usize,
    ) -> Result<Self, AudioError> {
        let channels = output_channels.min(input_channels);
        let filter = if input_rate == output_rate {
            None
        } else {
            Some(SincFilter::new(input_rate, output_rate, channels)?)
        };
        Ok(Self {
            input_channels,
            output_channels,
            filter,
        })
    }

    pub(super) fn push(
        &mut self,
        samples: &[f32],
        output: &mut Vec<f32>,
    ) -> Result<(), AudioError> {
        output.clear();
        for frame in samples.chunks_exact(self.input_channels) {
            let left = if self.output_channels == 1 && frame.len() == 2 {
                (frame[0] + frame[1]) * 0.5
            } else {
                frame[0]
            };
            let right = frame.get(1).copied().unwrap_or(left);
            if let Some(filter) = &mut self.filter {
                filter.input[0].push(left);
                if filter.input.len() == 2 {
                    filter.input[1].push(right);
                }
                filter.received += 1;
                if filter.input[0].len() == CHUNK {
                    filter.process(self.output_channels, output)?;
                }
            } else {
                write_frame(left, right, self.output_channels, output);
            }
        }
        Ok(())
    }

    pub(super) fn finish(&mut self, output: &mut Vec<f32>) -> Result<(), AudioError> {
        output.clear();
        if let Some(filter) = &mut self.filter {
            while filter.written < filter.target_frames() {
                for channel in &mut filter.input {
                    channel.resize(CHUNK, 0.0);
                }
                filter.process(self.output_channels, output)?;
            }
        }
        Ok(())
    }
}

fn write_frame(left: f32, right: f32, channels: usize, output: &mut Vec<f32>) {
    output.push(left);
    if channels > 1 {
        output.push(right);
        output.resize(output.len() + channels - 2, 0.0);
    }
}

struct SincFilter {
    resampler: SincFixedIn<f32>,
    input: Vec<Vec<f32>>,
    converted: Vec<Vec<f32>>,
    received: u64,
    written: u64,
    input_rate: u32,
    output_rate: u32,
}

impl SincFilter {
    fn new(input_rate: u32, output_rate: u32, channels: usize) -> Result<Self, AudioError> {
        let resampler = SincFixedIn::new(
            f64::from(output_rate) / f64::from(input_rate),
            1.0,
            SincInterpolationParameters {
                sinc_len: 128,
                f_cutoff: 0.94,
                interpolation: SincInterpolationType::Linear,
                oversampling_factor: 128,
                window: WindowFunction::BlackmanHarris2,
            },
            CHUNK,
            channels,
        )?;
        Ok(Self {
            converted: resampler.output_buffer_allocate(true),
            input: (0..channels).map(|_| Vec::with_capacity(CHUNK)).collect(),
            resampler,
            received: 0,
            written: 0,
            input_rate,
            output_rate,
        })
    }

    fn target_frames(&self) -> u64 {
        (self.received * u64::from(self.output_rate) + u64::from(self.input_rate) / 2)
            / u64::from(self.input_rate)
    }

    fn process(&mut self, channels: usize, output: &mut Vec<f32>) -> Result<(), AudioError> {
        let (_, count) =
            self.resampler
                .process_into_buffer(&self.input, &mut self.converted, None)?;
        for input in &mut self.input {
            input.clear();
        }
        let available = count.min((self.target_frames() - self.written) as usize);
        let (first, rest) = self
            .converted
            .split_first()
            .ok_or(AudioError::Unsupported("Resampler returned no channels"))?;
        let left = &first[..available];
        let right = rest.last().unwrap_or(first);
        for (&left, &right) in left.iter().zip(right) {
            write_frame(left, right, channels, output);
        }
        self.written += available as u64;
        Ok(())
    }
}
