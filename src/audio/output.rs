mod render;

use super::{AudioError, Meters};
use cpal::{
    FromSample, SampleFormat, SizedSample, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use ringbuf::{HeapCons, HeapProd, HeapRb, traits::*};
use std::{
    sync::{Arc, atomic::Ordering::Relaxed},
    time::Duration,
};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum OutputDevice {
    #[default]
    System,
    Named(String),
}

impl std::fmt::Display for OutputDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::System => f.write_str("System default"),
            Self::Named(name) => f.write_str(name),
        }
    }
}

impl OutputDevice {
    pub fn available() -> Vec<Self> {
        let mut devices = vec![Self::System];
        if let Ok(outputs) = cpal::default_host().output_devices() {
            devices.extend(
                outputs
                    .filter_map(|device| device.name().ok())
                    .map(Self::Named),
            );
        }
        devices
    }

    fn resolve(&self) -> Result<cpal::Device, AudioError> {
        let host = cpal::default_host();
        let device = match self {
            Self::System => host.default_output_device(),
            Self::Named(name) => host
                .output_devices()?
                .find(|device| device.name().as_ref().ok() == Some(name)),
        };
        device.ok_or(AudioError::DeviceUnavailable)
    }
}

pub(super) struct Output {
    stream: cpal::Stream,
    producer: HeapProd<f32>,
    written_frames: u64,
    pub(super) rate: u32,
    pub(super) channels: usize,
}

impl Output {
    pub(super) fn new(choice: &OutputDevice, meters: Arc<Meters>) -> Result<Self, AudioError> {
        let device = choice.resolve()?;
        let supported = device.default_output_config()?;
        let config = supported.config();
        let rate = config.sample_rate.0;
        let channels = usize::from(config.channels);
        if channels == 0 || rate == 0 {
            return Err(AudioError::DeviceUnavailable);
        }
        let (producer, consumer) = HeapRb::new((rate as usize / 8).max(1) * channels).split();
        let stream = match supported.sample_format() {
            SampleFormat::F32 => build::<f32>(&device, &config, consumer, meters),
            SampleFormat::F64 => build::<f64>(&device, &config, consumer, meters),
            SampleFormat::I8 => build::<i8>(&device, &config, consumer, meters),
            SampleFormat::I16 => build::<i16>(&device, &config, consumer, meters),
            SampleFormat::I24 => build::<cpal::I24>(&device, &config, consumer, meters),
            SampleFormat::I32 => build::<i32>(&device, &config, consumer, meters),
            SampleFormat::I64 => build::<i64>(&device, &config, consumer, meters),
            SampleFormat::U8 => build::<u8>(&device, &config, consumer, meters),
            SampleFormat::U16 => build::<u16>(&device, &config, consumer, meters),
            SampleFormat::U32 => build::<u32>(&device, &config, consumer, meters),
            SampleFormat::U64 => build::<u64>(&device, &config, consumer, meters),
            _ => {
                return Err(AudioError::Unsupported(
                    "Unsupported audio device sample format",
                ));
            }
        }?;
        Ok(Self {
            stream,
            producer,
            written_frames: 0,
            rate,
            channels,
        })
    }

    pub(super) fn write(&mut self, samples: &[f32]) -> usize {
        let count = self.producer.vacant_len().min(samples.len()) / self.channels * self.channels;
        let written = self.producer.push_slice(&samples[..count]);
        self.written_frames += (written / self.channels) as u64;
        written
    }

    pub(super) fn start(&self) -> Result<(), AudioError> {
        Ok(self.stream.play()?)
    }
    pub(super) fn pause(&self) -> Result<(), AudioError> {
        Ok(self.stream.pause()?)
    }
    pub(super) fn drained(&self, meters: &Meters) -> bool {
        meters.frames.load(std::sync::atomic::Ordering::Acquire) == self.written_frames
    }
}

fn build<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: &StreamConfig,
    mut consumer: HeapCons<f32>,
    meters: Arc<Meters>,
) -> Result<cpal::Stream, cpal::BuildStreamError> {
    let errors = Arc::clone(&meters);
    let channels = usize::from(config.channels);
    let rate = config.sample_rate.0;
    device.build_output_stream(
        config,
        move |out: &mut [T], info| {
            let consumed = render::render(out, consumer.pop_iter(), meters.volume.load(Relaxed));
            if consumed > 0 {
                let stamp = info.timestamp();
                let latency = stamp
                    .playback
                    .duration_since(&stamp.callback)
                    .unwrap_or_default();
                let batch = Duration::from_secs_f64((consumed / channels) as f64 / f64::from(rate));
                meters
                    .tail_ms
                    .store((latency + batch).as_millis() as u64 + 1, Relaxed);
                meters.frames.fetch_add(
                    (consumed / channels) as u64,
                    std::sync::atomic::Ordering::Release,
                );
            }
        },
        move |_| errors.device_failed.store(true, Relaxed),
        None,
    )
}
