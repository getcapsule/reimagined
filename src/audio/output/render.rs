use cpal::{FromSample, SizedSample};

pub(super) fn render<T: SizedSample + FromSample<f32>>(
    out: &mut [T],
    samples: impl Iterator<Item = f32>,
    volume: u8,
) -> usize {
    let gain = f32::from(volume) / 100.0;
    let mut consumed = 0;
    for (dst, sample) in out.iter_mut().zip(samples) {
        *dst = T::from_sample((sample * gain).clamp(-1.0, 1.0));
        consumed += 1;
    }
    out[consumed..].fill(T::EQUILIBRIUM);
    consumed
}

#[cfg(test)]
mod tests {
    use super::render;

    #[test]
    fn unsigned_output_is_silent_on_underrun() {
        let mut output = [0u16; 4];
        assert_eq!(render(&mut output, [1.0, -1.0].into_iter(), 50), 2);
        assert_eq!(output, [49152, 16384, 32768, 32768]);
    }

    #[test]
    fn leaves_excess_samples_for_the_next_callback() {
        let mut source = [0.1, 0.2, 0.3, 0.4].into_iter();
        let mut buffer = [0.0f32; 2];
        assert_eq!(render(&mut buffer, &mut source, 100), 2);
        assert_eq!(buffer, [0.1, 0.2]);
        assert_eq!(source.next(), Some(0.3));
    }
}
