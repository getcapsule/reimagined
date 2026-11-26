use super::*;
use crate::audio::MediaSource;
use std::time::Duration;
use std::{f64::consts::TAU, io::Cursor, sync::Arc};

struct Wav(Arc<[u8]>);

impl AudioSource for Wav {
    fn open_media(&self) -> Result<Box<dyn MediaSource>, AudioError> {
        Ok(Box::new(Cursor::new(Arc::clone(&self.0))))
    }
}

fn wav(rate: u32, samples: &[i16]) -> Wav {
    let bytes = (samples.len() * 2) as u32;
    let mut data = Vec::new();
    data.extend(b"RIFF");
    data.extend((36 + bytes).to_le_bytes());
    data.extend(b"WAVEfmt ");
    data.extend(16u32.to_le_bytes());
    data.extend(1u16.to_le_bytes());
    data.extend(1u16.to_le_bytes());
    data.extend(rate.to_le_bytes());
    data.extend((rate * 2).to_le_bytes());
    data.extend(2u16.to_le_bytes());
    data.extend(16u16.to_le_bytes());
    data.extend(b"data");
    data.extend(bytes.to_le_bytes());
    data.extend(samples.iter().flat_map(|v| v.to_le_bytes()));
    Wav(data.into())
}

fn tone(rate: u32, hz: f64) -> Vec<i16> {
    (0..rate)
        .map(|i| ((TAU * hz * f64::from(i) / f64::from(rate)).sin() * 16000.0) as i16)
        .collect()
}

fn render(tracks: Vec<Wav>, output_rate: u32) -> Result<(Vec<f32>, Vec<u64>), AudioError> {
    let mut sources = tracks.into_iter();
    let first = Reader::primed(&sources.next().unwrap(), Duration::ZERO)?.unwrap();
    let mut decoder = Sequence::new(
        first,
        Box::new(sources.map(|s| Reader::primed(&s, Duration::ZERO))),
        output_rate,
        1,
    )?;
    let mut pcm = Vec::new();
    let mut boundaries = Vec::new();
    let mut block = Vec::new();
    loop {
        let state = decoder.read(&mut block)?;
        pcm.extend_from_slice(&block);
        match state {
            Block::Boundary { frame, .. } => boundaries.push(frame),
            Block::End => return Ok((pcm, boundaries)),
            Block::NextFailed(error) => return Err(error),
            Block::Audio => {}
        }
    }
}

#[test]
fn split_wav_matches_unsplit_pcm_at_native_and_resampled_rates() -> Result<(), AudioError> {
    let rate = 44100;
    let signal = tone(rate, 437.0);
    let split = 12347;
    for output_rate in [44100, 48000] {
        let (whole, _) = render(vec![wav(rate, &signal)], output_rate)?;
        let (joined, boundaries) = render(
            vec![wav(rate, &signal[..split]), wav(rate, &signal[split..])],
            output_rate,
        )?;
        assert_eq!(
            boundaries,
            [((split as f64 * f64::from(output_rate)) / f64::from(rate)).round() as u64]
        );
        assert_eq!(whole.len(), output_rate as usize);
        assert!(joined == whole, "split changed PCM at {output_rate} Hz");
    }
    Ok(())
}

#[test]
fn mixed_rates_keep_duration_and_pitch() -> Result<(), AudioError> {
    let first = wav(44100, &tone(44100, 440.0));
    let second = wav(48000, &tone(48000, 660.0));
    let (samples, boundaries) = render(vec![first, second], 48000)?;
    assert_eq!(boundaries, [48000]);
    assert_eq!(samples.len(), 96000);
    for (part, hz) in samples.chunks_exact(48000).zip([440, 660]) {
        let rising = part
            .windows(2)
            .filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0)
            .count();
        assert!(
            rising.abs_diff(hz) <= 1,
            "measured {rising} Hz, expected {hz}"
        );
    }
    Ok(())
}
