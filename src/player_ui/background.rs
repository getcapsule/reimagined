use super::artwork;
use iced::{
    Color, ContentFit, Element, Fill,
    widget::{container, image, stack},
};
use std::path::Path;

pub(super) const BASE: Color = Color::from_rgb8(17, 18, 20);
#[derive(Debug, Clone)]
pub(super) struct PageArt {
    pub cover: image::Handle,
    pub colors: image::Handle,
}
impl PageArt {
    fn new(cover: image::Handle) -> Option<Self> {
        let image::Handle::Rgba {
            width,
            height,
            pixels,
            ..
        } = &cover
        else {
            return None;
        };
        let source = ::image::ImageBuffer::<::image::Rgba<u8>, &[u8]>::from_raw(
            *width,
            *height,
            pixels.as_ref(),
        )?;
        let small =
            ::image::imageops::resize(&source, 64, 64, ::image::imageops::FilterType::Triangle);
        let mut colors = ::image::imageops::blur(&small, 16.0);
        for pixel in colors.pixels_mut() {
            let rgb = [
                f32::from(pixel[0]),
                f32::from(pixel[1]),
                f32::from(pixel[2]),
            ];
            let grey = rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
            let saturated = rgb.map(|c| (grey + (c - grey) * 1.45).clamp(0., 255.));
            let luminance = saturated[0] * 0.2126 + saturated[1] * 0.7152 + saturated[2] * 0.0722;
            let gain = (56. / luminance.max(1.)).min(0.5);
            *pixel = ::image::Rgba([
                (saturated[0] * gain) as u8,
                (saturated[1] * gain) as u8,
                (saturated[2] * gain) as u8,
                255,
            ]);
        }
        Some(Self {
            cover,
            colors: image::Handle::from_rgba(64, 64, colors.into_raw()),
        })
    }
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
}
