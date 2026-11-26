use iced::advanced::{Layout, Renderer as _, Widget, layout, renderer, widget::Tree};
use iced::{Border, Color, Length, Rectangle, Renderer, Size, Theme, mouse};

pub(super) struct Rim {
    pub top: f32,
}
impl<M> Widget<M, Theme, Renderer> for Rim {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }
    fn layout(&mut self, _: &mut Tree, _: &Renderer, limits: &layout::Limits) -> layout::Node {
        layout::atomic(limits, Length::Fill, Length::Fill)
    }
    fn draw(
        &self,
        _: &Tree,
        renderer: &mut Renderer,
        _: &Theme,
        _: &renderer::Style,
        layout: Layout<'_>,
        _: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        for band in 0..8 {
            let t = (band as f32 + 0.5) / 8.;
            let strip = Rectangle {
                y: bounds.y + bounds.height * band as f32 / 8.,
                height: bounds.height / 8.,
                ..bounds
            };
            if let Some(clip) = strip.intersection(viewport) {
                renderer.with_layer(clip, |renderer| {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds,
                            border: Border {
                                radius: super::RADIUS.into(),
                                width: 1.,
                                color: Color::from_rgba(
                                    1.,
                                    1.,
                                    1.,
                                    0.025 + (self.top - 0.025) * (1. - t).powf(1.4),
                                ),
                            },
                            ..Default::default()
                        },
                        Color::TRANSPARENT,
                    );
                });
            }
        }
    }
}
