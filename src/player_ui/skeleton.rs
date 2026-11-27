use iced::advanced::{
    Clipboard, Layout, Renderer as _, Shell, Widget, layout, renderer,
    widget::{Tree, tree},
};
use iced::widget::{column, container, row};
use iced::{Border, Color, Element, Event, Length, Rectangle, Renderer, Size, Theme, mouse};
use std::time::{Duration, Instant};

pub(super) fn block<'a, M: 'a>(width: impl Into<Length>, height: f32) -> Element<'a, M> {
    Element::new(Block {
        width: width.into(),
        height,
    })
}

pub(super) fn song<'a, M: 'a>(side: f32) -> Element<'a, M> {
    row![
        block(side, side),
        column![block(180., 12.), block(112., 10.)]
            .spacing(8)
            .width(Length::Fill),
    ]
    .spacing(10)
    .align_y(iced::Alignment::Center)
    .into()
}

pub(super) fn rows<'a, M: 'a>(count: usize, covers: bool) -> Element<'a, M> {
    (0..count)
        .fold(column![].spacing(8), |rows, _| {
            let line = if covers {
                song(36.)
            } else {
                column![block(240., 12.), block(112., 10.)]
                    .spacing(8)
                    .width(Length::Fill)
                    .into()
            };
            rows.push(container(line).padding(8).width(Length::Fill))
        })
        .into()
}

#[derive(Debug)]
struct Block {
    width: Length,
    height: f32,
}
#[derive(Debug)]
struct Pulse {
    started: Instant,
}
impl<M> Widget<M, Theme, Renderer> for Block {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Pulse>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(Pulse {
            started: Instant::now(),
        })
    }
    fn size(&self) -> Size<Length> {
        Size::new(self.width, Length::Fixed(self.height))
    }
    fn layout(&mut self, _: &mut Tree, _: &Renderer, limits: &layout::Limits) -> layout::Node {
        layout::atomic(limits, self.width, self.height)
    }
    fn update(
        &mut self,
        _: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        _: mouse::Cursor,
        _: &Renderer,
        _: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        if matches!(
            event,
            Event::Window(iced::window::Event::RedrawRequested(_))
        ) && layout.bounds().intersection(viewport).is_some()
        {
            shell.request_redraw_at(Instant::now() + Duration::from_millis(50));
        }
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _: &Theme,
        _: &renderer::Style,
        layout: Layout<'_>,
        _: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        if let Some(clip) = bounds.intersection(viewport) {
            let phase = tree
                .state
                .downcast_ref::<Pulse>()
                .started
                .elapsed()
                .as_secs_f32();
            let alpha = 0.055 + (phase * 2.4).sin().mul_add(0.5, 0.5) * 0.035;
            renderer.with_layer(clip, |renderer| {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds,
                        border: Border::default().rounded(super::material::RADIUS),
                        ..Default::default()
                    },
                    Color::from_rgba(0.86, 0.89, 0.95, alpha),
                )
            });
        }
    }
}
