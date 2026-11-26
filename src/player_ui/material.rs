mod rim;

use iced::{Border, Color, Element, Font, Length, Padding, Theme, widget::container};

pub(super) const RADIUS: f32 = 12.;
pub(super) const TEXT: Color = Color::from_rgb8(244, 246, 250);
pub(super) const SECONDARY: Color = Color::from_rgb8(193, 199, 210);

#[derive(Debug, Clone, Copy)]
enum Elevation {
    Structural,
    Floating,
}
impl Elevation {
    fn raised(self) -> bool {
        matches!(self, Self::Floating)
    }
    fn rim(self) -> f32 {
        if self.raised() { 0.22 } else { 0.16 }
    }
}

pub(super) fn glaze(top: f32, bottom: f32) -> iced::Background {
    iced::gradient::Linear::new(iced::Radians(std::f32::consts::FRAC_PI_2))
        .add_stop(0., Color::from_rgba(1., 1., 1., top))
        .add_stop(1., Color::from_rgba(1., 1., 1., bottom))
        .into()
}
pub(super) fn raised_button(
    _: &Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    use iced::widget::button::Status;
    let hover = matches!(status, Status::Hovered | Status::Pressed);
    iced::widget::button::Style {
        background: Some(glaze(
            if hover { 0.19 } else { 0.12 },
            if hover { 0.10 } else { 0.055 },
        )),
        text_color: if status == Status::Disabled {
            SECONDARY
        } else {
            TEXT
        },
        border: Border {
            radius: RADIUS.into(),
            width: 1.,
            color: Color::from_rgba(1., 1., 1., if hover { 0.24 } else { 0.18 }),
        },
        shadow: iced::Shadow {
            color: Color::from_rgba(0., 0., 0., 0.25),
            offset: iced::Vector::new(0., 2.),
            blur_radius: 8.,
        },
        ..Default::default()
    }
}

pub(super) fn secondary_button(
    theme: &Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let mut style = raised_button(theme, status);
    if status != iced::widget::button::Status::Hovered {
        style.background = Some(glaze(0.065, 0.025));
        style.border.color.a = 0.12;
        style.text_color = SECONDARY;
        style.shadow.color.a = 0.12;
    }
    style
}

pub(super) const TYPE: Font = Font::with_name("Manrope");
pub(super) const HEADING: Font = Font {
    weight: iced::font::Weight::Semibold,
    ..TYPE
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Appearance {
    opacity: u8,
    pub blur: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Self::new(42)
    }
}

impl Appearance {
    pub(super) const DARK: Self = Self::new(100);
    pub(super) const fn new(percent: u8) -> Self {
        Self {
            opacity: if percent > 100 { 100 } else { percent },
            blur: true,
        }
    }
    pub(super) fn percent(self) -> u8 {
        self.opacity
    }
    pub(super) fn glass(self) -> bool {
        self.opacity < 100
    }

    pub(super) fn backdrop(self) -> bool {
        self.glass() && self.blur
    }

    pub(super) fn load() -> Self {
        capsule::library::data_dir()
            .ok()
            .map_or_else(Self::default, |dir| Self::read(&dir))
    }

    fn read(dir: &std::path::Path) -> Self {
        let file = std::fs::read_to_string(dir.join("glass-appearance"))
            .or_else(|_| std::fs::read_to_string(dir.join("window-opacity")))
            .unwrap_or_default();
        let mut values = file.split_whitespace();
        if let Some(value) = values.next().and_then(|text| text.parse().ok()) {
            Self {
                blur: values.next() != Some("clear"),
                ..Self::new(value)
            }
        } else if dir.join("opaque-window").is_file() {
            Self::DARK
        } else {
            Self::default()
        }
    }

    pub(super) fn save(self, dir: &std::path::Path) -> std::io::Result<()> {
        use std::io::Write;
        std::fs::create_dir_all(dir)?;
        let mut file = tempfile::NamedTempFile::new_in(dir)?;
        write!(
            file,
            "{} {}",
            self.percent(),
            if self.blur { "blur" } else { "clear" }
        )?;
        file.persist(dir.join("glass-appearance"))?;
        Ok(())
    }
}

pub(super) fn button<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> iced::widget::Button<'a, M> {
    iced::widget::button(content).style(primary_button)
}

pub(super) fn primary_button(
    theme: &Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let mut style = iced::widget::button::primary(theme, status);
    style.border.radius = RADIUS.into();
    style
}

pub(super) fn text_button(
    theme: &Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let mut style = iced::widget::button::text(theme, status);
    style.border.radius = RADIUS.into();
    style
}

pub(super) fn subtle_button(
    theme: &Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let mut style = iced::widget::button::subtle(theme, status);
    style.border.radius = RADIUS.into();
    style
}

pub(super) fn tooltip(theme: &Theme) -> container::Style {
    let mut style = container::rounded_box(theme);
    style.border.radius = RADIUS.into();
    style
}

pub(super) fn input(
    theme: &Theme,
    status: iced::widget::text_input::Status,
) -> iced::widget::text_input::Style {
    let mut style = iced::widget::text_input::default(theme, status);
    style.border.radius = RADIUS.into();
    style
}

pub(super) fn panel<'a, M: 'a>(
    appearance: Appearance,
    content: impl Into<Element<'a, M>>,
    padding: impl Into<Padding>,
    width: impl Into<Length>,
    height: impl Into<Length>,
) -> Element<'a, M> {
    surface(
        appearance,
        content.into(),
        padding.into(),
        width.into(),
        height.into(),
        Elevation::Structural,
    )
}
pub(super) fn floating<'a, M: 'a>(
    appearance: Appearance,
    content: impl Into<Element<'a, M>>,
    padding: impl Into<Padding>,
    width: impl Into<Length>,
    height: impl Into<Length>,
) -> Element<'a, M> {
    surface(
        appearance,
        content.into(),
        padding.into(),
        width.into(),
        height.into(),
        Elevation::Floating,
    )
}
fn surface<'a, M: 'a>(
    appearance: Appearance,
    content: Element<'a, M>,
    padding: Padding,
    width: Length,
    height: Length,
    elevation: Elevation,
) -> Element<'a, M> {
    let style = move |_: &Theme| container::Style {
        text_color: Some(TEXT),
        background: Some(
            if appearance.glass() {
                let rgb = if elevation.raised() {
                    [0.115, 0.125, 0.15]
                } else {
                    [0.075, 0.082, 0.10]
                };
                Color::from_rgba(rgb[0], rgb[1], rgb[2], {
                    let opacity = f32::from(appearance.percent()) / 100.;
                    let frost = if elevation.raised() { 0.08 } else { 0.14 };
                    frost + (1. - frost) * opacity.powf(1.6)
                })
            } else if elevation.raised() {
                Color::from_rgb8(34, 37, 44)
            } else {
                Color::from_rgb8(24, 27, 33)
            }
            .into(),
        ),
        border: Border::default().rounded(RADIUS),
        ..Default::default()
    };
    let surface: Element<'a, M> = if appearance.backdrop() {
        iced_glass::widget::container(content)
            .glass_style(move |_| iced_glass::Style {
                blur_radius: if elevation.raised() { 22. } else { 28. },
                saturation: 1.12,
                lightness: 0.02,
                edge_radius: 8.,
                edge_height: 3.,
                refractive_index: 1.12,
                chromatic_aberration: 0.0,
                rim_width: 0.65,
                rim_angle: -std::f32::consts::FRAC_PI_2,
                opacity: 0.98,
                ..Default::default()
            })
            .style(style)
            .padding(padding)
            .width(width)
            .height(height)
            .into()
    } else {
        container(content)
            .style(style)
            .padding(padding)
            .width(width)
            .height(height)
            .into()
    };
    let shadow = container(surface)
        .width(width)
        .height(height)
        .style(move |_| container::Style {
            border: Border::default().rounded(RADIUS),
            shadow: iced::Shadow {
                color: Color::from_rgba(0., 0., 0., if elevation.raised() { 0.38 } else { 0.20 }),
                offset: iced::Vector::new(0., if elevation.raised() { 6. } else { 3. }),
                blur_radius: if elevation.raised() { 22. } else { 14. },
            },
            ..Default::default()
        });
    iced::widget::stack![
        shadow,
        Element::new(rim::Rim {
            top: elevation.rim()
        })
    ]
    .width(width)
    .height(height)
    .into()
}

pub(super) fn search_field(
    theme: &Theme,
    status: iced::widget::text_input::Status,
) -> iced::widget::text_input::Style {
    let mut style = iced::widget::text_input::default(theme, status);
    style.background = Color::TRANSPARENT.into();
    style.border = Border::default();
    style
}

#[cfg(test)]
mod tests {
    use super::Appearance;
    #[test]
    fn glass_settings_migrate_and_survive_relaunch() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        assert_eq!(Appearance::read(dir.path()), Appearance::default());
        std::fs::File::create(dir.path().join("opaque-window"))?;
        assert_eq!(Appearance::read(dir.path()), Appearance::DARK);
        std::fs::write(dir.path().join("window-opacity"), "32 clear")?;
        assert_eq!(Appearance::read(dir.path()).percent(), 32);
        assert!(!Appearance::read(dir.path()).blur);
        let clear = Appearance {
            blur: false,
            ..Appearance::new(75)
        };
        clear.save(dir.path())?;
        assert_eq!(Appearance::read(dir.path()), clear);
        for value in [0, 67, 100] {
            Appearance::new(value).save(dir.path())?;
            assert_eq!(Appearance::read(dir.path()).percent(), value);
        }
        Ok(())
    }
}
