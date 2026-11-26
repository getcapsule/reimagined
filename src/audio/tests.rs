use super::{AudioError, Reader, Source, convert::Converter};
use std::time::Duration;

#[test]
fn stereo_check_seeks_to_the_right_channel() -> Result<(), AudioError> {
    let mut reader = Reader::open(&Source::StereoCheck, Duration::from_millis(1350))?;
    let mut count = 0i32;
    let mut right_peak = 0.0f32;
    while reader.read()? {
        assert_eq!(reader.rate, 22050);
        assert_eq!(reader.channels, 2);
        for frame in reader.samples().chunks_exact(2) {
            if count < 6000 {
                assert_eq!(frame[0], 0.0);
                right_peak = right_peak.max(frame[1].abs());
            }
            count += 1;
        }
    }
    assert!(right_peak > 0.1);
    assert!((count - 58433).abs() <= 1);
    Ok(())
}

#[test]
fn resampled_clip_keeps_channels_duration_and_tail() -> Result<(), AudioError> {
    let mut reader = Reader::open(&Source::StereoCheck, Duration::ZERO)?;
    let mut converter = Converter::new(22050, 48000, 2, 2)?;
    let mut block = Vec::new();
    let mut result = Vec::new();
    while reader.read()? {
        converter.push(reader.samples(), &mut block)?;
        result.extend_from_slice(&block);
    }
    converter.finish(&mut block)?;
    result.extend_from_slice(&block);
    assert_eq!(result.len(), 4 * 48000 * 2);
    for (second, active, quiet) in [(0, 0, 1), (1, 1, 0)] {
        let start = (second * 48000 + 4000) * 2;
        let frames = result[start..start + 8000].chunks_exact(2);
        assert!(frames.clone().any(|frame| frame[active].abs() > 0.15));
        assert!(frames.clone().all(|frame| frame[quiet].abs() < 0.001));
    }
    let mut tail = Converter::new(44100, 48000, 1, 2)?;
    tail.push(&[0.5; 240], &mut block)?;
    assert!(block.is_empty());
    tail.finish(&mut block)?;
    assert_eq!(block.len(), 261 * 2);
    assert!(block.chunks_exact(2).all(|f| f[0] == f[1]));
    assert!(block[100] > 0.4);
    Ok(())
}

#[test]
fn same_rate_downmix_preserves_mid_channel() -> Result<(), AudioError> {
    let mut converter = Converter::new(48000, 48000, 2, 1)?;
    let mut block = Vec::new();
    converter.push(&[0.75, 0.25, 0.5, -0.5], &mut block)?;
    assert_eq!(block, [0.5, 0.0]);
    converter.finish(&mut block)?;
    assert!(block.is_empty());
    Ok(())
}

#[test]
fn opus_webm_preserves_both_channels_and_supports_seeking() -> Result<(), AudioError> {
    struct OpusAudio;
    impl super::AudioSource for OpusAudio {
        fn open_media(&self) -> Result<Box<dyn super::MediaSource>, AudioError> {
            Ok(Box::new(std::io::Cursor::new(
                &include_bytes!("../../tests/fixtures/opus.webm")[..],
            )))
        }
        fn extension(&self) -> Option<&str> {
            Some("webm")
        }
    }
    let mut reader = Reader::open(&OpusAudio, Duration::ZERO)?;
    let mut crossings = [0usize; 2];
    let mut previous = [0.0f32; 2];
    let mut frames = 0;
    while reader.read()? {
        assert_eq!(reader.rate, 48000);
        assert_eq!(reader.channels, 2);
        for frame in reader.samples().chunks_exact(2) {
            for channel in 0..2 {
                if previous[channel] <= 0.0 && frame[channel] > 0.0 {
                    crossings[channel] += 1;
                }
                previous[channel] = frame[channel];
            }
            frames += 1;
        }
    }
    assert!((94500..97500).contains(&frames));
    assert!((860..900).contains(&crossings[0]));
    assert!((1740..1780).contains(&crossings[1]));
    let reader =
        Reader::primed(&OpusAudio, Duration::from_millis(700))?.expect("Opus frames after seek");
    assert!(!reader.samples().is_empty());
    Ok(())
}
