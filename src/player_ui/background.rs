use super::artwork;
use iced::{
    Color, ContentFit, Element, Fill,
    widget::{container, image, stack},
};
use std::path::Path;

pub(super) const BASE: Color = Color::from_rgb8(16, 18, 24);
#[derive(Debug, Clone)]
pub(super) struct PageArt {
    pub cover: image::Handle,
    pub colors: image::Handle,
}
impl PageArt {
    fn new(cover: image::Handle) -> Option<Self> {
        let colors = colors(&cover)?;
        Some(Self { cover, colors })
    }
}
pub(super) fn colors(cover: &image::Handle) -> Option<image::Handle> {
    let image::Handle::Rgba {
        width,
        height,
        pixels,
        ..
    } = cover
    else {
        return None;
    };
    let source = ::image::ImageBuffer::<::image::Rgba<u8>, &[u8]>::from_raw(
        *width,
        *height,
        pixels.as_ref(),
    )?;
    let small = ::image::imageops::resize(&source, 32, 32, ::image::imageops::FilterType::Triangle);
    let palette = palette(&small)?;
    let mut colors = ::image::RgbaImage::new(64, 64);
    for (x, y, pixel) in colors.enumerate_pixels_mut() {
        let position = [x as f32 / 63., y as f32 / 63.];
        let rgb = palette.iter().fold([BASE.r, BASE.g, BASE.b], |base, glow| {
            let dx = position[0] - glow.position[0];
            let dy = position[1] - glow.position[1];
            let alpha = glow.strength * (-(dx * dx / 0.20 + dy * dy / 0.14)).exp();
            std::array::from_fn(|i| base[i] * (1. - alpha) + glow.rgb[i] * alpha)
        });
        *pixel = ::image::Rgba([
            (rgb[0] * 255.).round() as u8,
            (rgb[1] * 255.).round() as u8,
            (rgb[2] * 255.).round() as u8,
            255,
        ]);
    }
    Some(image::Handle::from_rgba(64, 64, colors.into_raw()))
}

#[derive(Debug, Clone, Copy)]
struct Hsl {
    hue: f32,
    saturation: f32,
    lightness: f32,
}
impl Hsl {
    fn from_rgb(rgb: [f32; 3]) -> Self {
        let [r, g, b] = rgb;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;
        let lightness = (max + min) * 0.5;
        if delta < 0.0001 {
            return Self {
                hue: 0.,
                saturation: 0.,
                lightness,
            };
        }
        let hue = if max == r {
            ((g - b) / delta).rem_euclid(6.)
        } else if max == g {
            (b - r) / delta + 2.
        } else {
            (r - g) / delta + 4.
        } / 6.;
        Self {
            hue,
            saturation: delta / (1. - (2. * lightness - 1.).abs()),
            lightness,
        }
    }
    fn normalized(self) -> Self {
        Self {
            lightness: self.lightness.clamp(0.58, 0.68),
            saturation: if self.saturation < 0.04 {
                0.
            } else {
                (self.saturation * 1.18 + 0.05).clamp(0.36, 0.64)
            },
            ..self
        }
    }
    fn rgb(self) -> [f32; 3] {
        let chroma = (1. - (2. * self.lightness - 1.).abs()) * self.saturation;
        let h = self.hue * 6.;
        let x = chroma * (1. - (h.rem_euclid(2.) - 1.).abs());
        let rgb = match h as usize {
            0 => [chroma, x, 0.],
            1 => [x, chroma, 0.],
            2 => [0., chroma, x],
            3 => [0., x, chroma],
            4 => [x, 0., chroma],
            _ => [chroma, 0., x],
        };
        rgb.map(|c| c + self.lightness - chroma * 0.5)
    }
}
#[derive(Debug, Default, Clone, Copy)]
struct Bucket {
    weight: f32,
    rgb: [f32; 3],
    position: [f32; 2],
}
#[derive(Debug, Clone, Copy)]
struct Glow {
    rgb: [f32; 3],
    position: [f32; 2],
    strength: f32,
}
impl Bucket {
    fn glow(self, strength: f32) -> Glow {
        Glow {
            rgb: Hsl::from_rgb(self.rgb.map(|c| c / self.weight))
                .normalized()
                .rgb(),
            position: [
                ((self.position[0] / self.weight - 0.5) * 1.6 + 0.5).clamp(0.05, 0.95),
                ((self.position[1] / self.weight - 0.5) * 1.0 + 0.22).clamp(0.05, 0.65),
            ],
            strength,
        }
    }
}
fn palette(source: &::image::RgbaImage) -> Option<[Glow; 4]> {
    let mut buckets = [Bucket::default(); 24];
    let mut neutral = Bucket::default();
    for (x, y, pixel) in source.enumerate_pixels() {
        let alpha = f32::from(pixel[3]) / 255.;
        if alpha == 0. {
            continue;
        }
        let rgb = [pixel[0], pixel[1], pixel[2]].map(|c| f32::from(c) / 255.);
        let hsl = Hsl::from_rgb(rgb);
        let position = [
            x as f32 / source.width().saturating_sub(1).max(1) as f32,
            y as f32 / source.height().saturating_sub(1).max(1) as f32,
        ];
        let weight = alpha * hsl.saturation.powi(2) * (1. - (2. * hsl.lightness - 1.).abs());
        let bucket = &mut buckets[(hsl.hue * 24.) as usize % 24];
        bucket.weight += weight;
        neutral.weight += alpha;
        for (i, component) in rgb.iter().enumerate() {
            bucket.rgb[i] += component * weight;
            neutral.rgb[i] += component * alpha;
        }
        for (i, component) in position.iter().enumerate() {
            bucket.position[i] += component * weight;
            neutral.position[i] += component * alpha;
        }
    }
    let mut order: Vec<_> = (0..24).collect();
    order.sort_by(|a, b| buckets[*b].weight.total_cmp(&buckets[*a].weight));
    let first = order[0];
    let main = buckets[first];
    if main.weight < 0.001 {
        if neutral.weight < 0.001 {
            return None;
        }
        let mut glow = neutral.glow(0.08);
        let grey = glow.rgb.iter().sum::<f32>() / 3.;
        glow.rgb = [grey; 3];
        return Some([
            glow,
            Glow {
                strength: 0.03,
                position: [0.9, 0.8],
                ..glow
            },
            Glow {
                strength: 0.045,
                position: [0.12, 0.85],
                ..glow
            },
            Glow {
                strength: 0.03,
                position: [0.9, 0.95],
                ..glow
            },
        ]);
    }
    let second = order.into_iter().find(|i| {
        let distance = first.abs_diff(*i);
        distance.min(24 - distance) >= 4 && buckets[*i].weight >= main.weight * 0.2
    });
    let primary = main.glow(0.22);
    let secondary = second.map_or(
        Glow {
            strength: 0.05,
            position: [0.9, 0.8],
            ..primary
        },
        |i| buckets[i].glow(0.13),
    );
    Some([
        primary,
        secondary,
        Glow {
            strength: 0.09,
            position: [0.12, 0.85],
            ..primary
        },
        Glow {
            strength: 0.07,
            position: [0.9, 0.95],
            ..secondary
        },
    ])
}

pub(super) fn album(path: &Path) -> Option<PageArt> {
    PageArt::new(artwork::cover(path, 512)?)
}
pub(super) fn artist(path: &Path) -> Option<PageArt> {
    let dir = path.parent()?;
    dir.ancestors()
        .take(2)
        .find_map(|dir| {
            ["artist.jpg", "artist.png"]
                .into_iter()
                .find_map(|name| artwork::image_file(&dir.join(name), 512).and_then(PageArt::new))
        })
        .or_else(|| album(path))
}
pub(super) fn view<'a, M: 'a>(art: Option<&'a image::Handle>) -> Element<'a, M> {
    let base = container(iced::widget::Space::new())
        .width(Fill)
        .height(Fill)
        .style(|_| container::Style {
            background: Some(BASE.into()),
            ..Default::default()
        });
    if let Some(art) = art {
        stack![
            base,
            image(art)
                .width(Fill)
                .height(Fill)
                .content_fit(ContentFit::Cover)
        ]
        .into()
    } else {
        base.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn artwork_becomes_opaque_color_without_recognizable_detail() {
        let mut source = ::image::RgbaImage::new(128, 128);
        for (x, y, p) in source.enumerate_pixels_mut() {
            let detail = if (x + y) % 2 == 0 { 35 } else { 0 };
            *p = if x < 64 {
                ::image::Rgba([220, detail, 30, 255])
            } else {
                ::image::Rgba([30, detail, 220, 255])
            };
        }
        let result = PageArt::new(image::Handle::from_rgba(128, 128, source.into_raw()))
            .expect("RGBA artwork");
        let image::Handle::Rgba {
            width,
            height,
            pixels,
            ..
        } = result.colors
        else {
            panic!("RGBA backdrop")
        };
        assert_eq!((width, height), (64, 64));
        assert!(pixels.chunks_exact(4).all(|p| p[3] == 255));
        let at = |x: usize, y: usize| &pixels[(y * 64 + x) * 4..][..4];
        assert!(at(5, 32)[0] > at(5, 32)[2]);
        assert!(at(58, 32)[2] > at(58, 32)[0]);
        assert!((i16::from(at(30, 30)[1]) - i16::from(at(31, 30)[1])).abs() <= 2);
    }
    #[test]
    fn earthy_tints_are_lifted_without_changing_their_hue() {
        for rgb in [[0.32, 0.24, 0.12], [0.28, 0.30, 0.12], [0.18, 0.12, 0.10]] {
            let original = Hsl::from_rgb(rgb);
            let normalized = original.normalized();
            assert!((0.58..=0.68).contains(&normalized.lightness));
            assert!((0.36..=0.64).contains(&normalized.saturation));
            assert!((original.hue - normalized.hue).abs() < 0.001);
        }
    }
    #[test]
    fn ambient_canvas_preserves_readable_secondary_text_for_extreme_covers() {
        let luminance = |rgb: [f32; 3]| {
            let linear = rgb.map(|c| {
                if c <= 0.04045 {
                    c / 12.92
                } else {
                    ((c + 0.055) / 1.055).powf(2.4)
                }
            });
            linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722
        };
        let text = super::super::material::SECONDARY;
        let foreground = luminance([text.r, text.g, text.b]);
        for rgb in [
            [255, 255, 255],
            [0, 0, 0],
            [108, 78, 36],
            [90, 95, 30],
            [255, 0, 0],
            [0, 255, 0],
            [0, 0, 255],
        ] {
            let source = ::image::RgbaImage::from_pixel(
                16,
                16,
                ::image::Rgba([rgb[0], rgb[1], rgb[2], 255]),
            );
            let result = colors(&image::Handle::from_rgba(16, 16, source.into_raw())).unwrap();
            let image::Handle::Rgba { pixels, .. } = result else {
                panic!("RGBA canvas")
            };
            for pixel in pixels.chunks_exact(4) {
                let background =
                    luminance([pixel[0], pixel[1], pixel[2]].map(|c| f32::from(c) / 255.));
                assert!(
                    (foreground + 0.05) / (background + 0.05) >= 4.5,
                    "secondary text contrast for {rgb:?}"
                );
                assert_eq!(pixel[3], 255);
            }
        }
    }
    #[test]
    fn neutral_and_transparent_covers_do_not_invent_a_saturated_accent() {
        let neutral = ::image::RgbaImage::from_pixel(8, 8, ::image::Rgba([128, 128, 128, 255]));
        let palette = palette(&neutral).unwrap();
        assert!(
            palette
                .iter()
                .all(|glow| Hsl::from_rgb(glow.rgb).saturation < 0.01)
        );
        let transparent = image::Handle::from_rgba(2, 2, vec![0; 16]);
        assert!(colors(&transparent).is_none());
    }
    #[test]
    fn cover_light_reaches_sidebar_and_lower_controls() {
        let cover = image::Handle::from_rgba(1, 1, vec![30, 160, 220, 255]);
        let image::Handle::Rgba { pixels, .. } = colors(&cover).expect("ambient cover") else {
            panic!("RGBA canvas")
        };
        let base = [BASE.r, BASE.g, BASE.b].map(|value| (value * 255.).round() as i16);
        for (x, y) in [(4, 48), (26, 60), (55, 60)] {
            let pixel = &pixels[(y * 64 + x) * 4..][..3];
            let light = pixel
                .iter()
                .zip(base)
                .map(|(value, base)| (i16::from(*value) - base).abs())
                .sum::<i16>();
            assert!(
                light >= 12,
                "cover light missing below glass at ({x}, {y}): {light}"
            );
        }
    }
}
