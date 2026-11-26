use iced::{Border, Color, Element, Font, Length, Padding, Theme, widget::container};

pub(super) const RADIUS: f32 = 12.;

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
    let padding = padding.into();
    let width = width.into();
    let height = height.into();
    let style = move |_: &Theme| container::Style {
        background: Some(
            match appearance.glass() {
                false => Color::from_rgb8(29, 29, 31),
                true => Color::from_rgba(
                    0.08,
                    0.085,
                    0.095,
                    0.10 + f32::from(appearance.percent()) / 100. * 0.62,
                ),
            }
            .into(),
        ),
        border: Border {
            radius: RADIUS.into(),
            width: 1.,
            color: Color::from_rgba(1., 1., 1., 0.16),
        },
        ..Default::default()
    };
    let surface: Element<'a, M> = match appearance.backdrop() {
        false => container(content)
            .style(style)
            .padding(padding)
            .width(width)
            .height(height)
            .into(),
        true => iced_glass::widget::container(content)
            .glass_style(|_| iced_glass::Style {
                blur_radius: 18.,
                saturation: 1.2,
                lightness: -0.08,
                edge_radius: 12.,
                edge_height: 5.,
                refractive_index: 1.13,
                rim_width: 0.8,
                opacity: 0.92,
                ..Default::default()
            })
            .style(style)
            .padding(padding)
            .width(width)
            .height(height)
            .into(),
    };
    container(surface)
        .width(width)
        .height(height)
        .style(|_| container::Style {
            border: Border::default().rounded(RADIUS),
            shadow: iced::Shadow {
                color: Color::from_rgba(0., 0., 0., 0.25),
                offset: iced::Vector::new(0., 5.),
                blur_radius: 20.,
            },
            ..Default::default()
        })
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
