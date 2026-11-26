use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, renderer, widget::Tree};
use iced::widget::stack;
use iced::{Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, mouse};

pub(super) fn row<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
    open: impl Fn(Point) -> M + 'a,
) -> Element<'a, M> {
    stack![content.into(), Element::new(RightClick(open))].into()
}

pub(super) struct PaletteBackdrop;

impl Widget<super::Message, Theme, Renderer> for PaletteBackdrop {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }
    fn layout(&mut self, _: &mut Tree, _: &Renderer, limits: &layout::Limits) -> layout::Node {
        layout::atomic(limits, Length::Fill, Length::Fill)
    }
    fn update(
        &mut self,
        _: &mut Tree,
        event: &Event,
        _: Layout<'_>,
        cursor: mouse::Cursor,
        _: &Renderer,
        _: &mut dyn Clipboard,
        shell: &mut Shell<'_, super::Message>,
        _: &Rectangle,
    ) {
        if matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(
                mouse::Button::Left | mouse::Button::Right
            )) | Event::Touch(iced::touch::Event::FingerPressed { .. })
        ) && !cursor.is_levitating()
        {
            shell.publish(super::Message::Search(super::search::Msg::Close));
        }
        if matches!(
            event,
            Event::Mouse(_) | Event::Keyboard(_) | Event::Touch(_) | Event::InputMethod(_)
        ) {
            shell.capture_event();
        }
    }
    fn draw(
        &self,
        _: &Tree,
        renderer: &mut Renderer,
        _: &Theme,
        _: &renderer::Style,
        layout: Layout<'_>,
        _: mouse::Cursor,
        _: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;
        renderer.fill_quad(
            renderer::Quad {
                bounds: layout.bounds(),
                ..Default::default()
            },
            iced::Color::from_rgba(0., 0., 0., 0.46),
        );
    }
}

struct RightClick<F>(F);

impl<M, F: Fn(Point) -> M> Widget<M, Theme, Renderer> for RightClick<F> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }
    fn layout(&mut self, _: &mut Tree, _: &Renderer, limits: &layout::Limits) -> layout::Node {
        layout::atomic(limits, Length::Fill, Length::Fill)
    }
    fn update(
        &mut self,
        _: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _: &Renderer,
        _: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        if matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right))
        ) && let Some(position) = cursor.position_over(layout.bounds())
            && viewport.contains(position)
        {
            shell.publish((self.0)(position));
            shell.capture_event();
        }
    }
    fn draw(
        &self,
        _: &Tree,
        _: &mut Renderer,
        _: &Theme,
        _: &renderer::Style,
        _: Layout<'_>,
        _: mouse::Cursor,
        _: &Rectangle,
    ) {
    }
}
