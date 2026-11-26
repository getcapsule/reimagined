use super::{App, Message, QueueItem, Screen, Transport, clock, material};
use iced::{
    Alignment, Border, color,
    widget::{Space, image, scrollable, stack, tooltip},
};
use iced::{
    Color, Element, Length,
    widget::{
        button, column, container, pick_list, row, rule::horizontal as horizontal_rule, slider,
        space::horizontal as horizontal_space, text,
    },
};
use std::time::Duration;

const MUTED: Color = Color {
    r: 0.76,
    g: 0.76,
    b: 0.77,
    a: 1.0,
};
const SECONDARY: Color = Color {
    r: 0.9,
    g: 0.9,
    b: 0.91,
    a: 1.0,
};

pub(super) fn soft_button(_: &iced::Theme, status: button::Status) -> button::Style {
    button::Style {
        background: Some(
            Color::from_rgba(
                0.92,
                0.92,
                0.92,
                if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                    0.20
                } else {
                    0.085
                },
            )
            .into(),
        ),
        text_color: if status == button::Status::Hovered {
            Color::WHITE
        } else {
            SECONDARY
        },
        border: Border {
            radius: material::RADIUS.into(),
            width: 1.0,
            color: Color::from_rgba(
                1.,
                1.,
                1.,
                if status == button::Status::Hovered {
                    0.28
                } else {
                    0.12
                },
            ),
        },
        ..Default::default()
    }
}

macro_rules! icon {
    ($name:literal) => {
        iced::widget::svg(iced::widget::svg::Handle::from_memory(
            include_bytes!(concat!("../../assets/icons/", $name, ".svg")).as_slice(),
        ))
        .width(16)
        .height(16)
    };
}

pub(super) use icon;

pub(super) fn control<'a, M: Clone + 'a>(
    glyph: iced::widget::Svg<'a>,
    label: &'a str,
    msg: Option<M>,
    primary: bool,
    active: bool,
) -> Element<'a, M> {
    let size = if primary { 34 } else { 27 };
    let face = glyph.opacity(if msg.is_some() { 1. } else { 0.38 });
    let b = super::material::button(
        container(face)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    )
    .width(size)
    .height(size)
    .padding(0)
    .on_press_maybe(msg)
    .style(move |_, state| {
        let hover = matches!(state, button::Status::Hovered | button::Status::Pressed);
        let fill = if primary {
            Some(Color::from_rgba(1., 1., 1., if hover { 0.21 } else { 0.13 }).into())
        } else if hover || active {
            Some(Color::from_rgba(1., 1., 1., if hover { 0.16 } else { 0.11 }).into())
        } else {
            None
        };
        button::Style {
            background: fill,
            text_color: if matches!(state, button::Status::Disabled) {
                Color::from_rgb8(113, 110, 127)
            } else if active {
                SECONDARY
            } else {
                Color::WHITE
            },
            border: Border {
                radius: 22.into(),
                width: if primary { 1.0 } else { 0.0 },
                color: Color::from_rgba(1., 1., 1., if hover { 0.34 } else { 0.15 }),
            },
            ..Default::default()
        }
    });
    tooltip(b, text(label).size(13), tooltip::Position::Top)
        .gap(6)
        .style(super::material::tooltip)
        .into()
}

impl App {
    fn sidebar_width(&self) -> f32 {
        if self.window_size.width < 760. {
            112.
        } else {
            144.
        }
    }

    fn content_width(&self) -> f32 {
        self.window_size.width
            - if self.sidebar_visible {
                self.sidebar_width() + 8.
            } else {
                0.
            }
    }

    pub(super) fn shell(&self) -> Element<'_, Message> {
        let compact = self.sidebar_width() < 120.;
        let busy = self.screen == Screen::Playlists && self.playlists.busy();
        let nav = |symbol: iced::widget::Svg<'static>, name: &'static str, screen, msg| {
            let active = Some(self.screen) == screen;
            super::material::button(
                row![symbol, text(name).size(13)]
                    .align_y(Alignment::Center)
                    .spacing(7),
            )
            .width(Length::Fill)
            .padding([5, 6])
            .on_press_maybe((!busy && !active).then_some(msg))
            .style(move |theme, state| {
                if active {
                    soft_button(theme, state)
                } else {
                    super::material::text_button(theme, state)
                }
            })
        };
        let brand = text("capsule").size(17).font(material::HEADING);
        let sidebar = self.sidebar_visible.then(|| {
            material::panel(
                self.appearance,
                column![
                    text("Browse").size(11).color(MUTED),
                    tooltip(
                        nav(
                            icon!("search"),
                            "Search",
                            None,
                            Message::Search(super::search::Msg::Open)
                        ),
                        text("Search library (Ctrl+K)").size(13),
                        tooltip::Position::Right
                    )
                    .style(super::material::tooltip),
                    nav(
                        icon!("discover"),
                        "Discover",
                        Some(Screen::Discover),
                        Message::ShowDiscover
                    ),
                    nav(
                        icon!("library"),
                        "Library",
                        Some(Screen::Library),
                        Message::ShowLibrary
                    ),
                    nav(
                        icon!("playlists"),
                        "Playlists",
                        Some(Screen::Playlists),
                        Message::ShowPlaylists
                    ),
                    Space::new().height(Length::Fill),
                    text("Controls").size(11).color(MUTED),
                    nav(icon!("folder"), "Open files", None, Message::Browse),
                    nav(
                        icon!("audio"),
                        "Audio",
                        Some(Screen::Audio),
                        Message::AudioSettings
                    ),
                    nav(
                        icon!("settings"),
                        "Settings",
                        Some(Screen::Settings),
                        Message::ShowSettings
                    ),
                    self.artwork
                        .details
                        .as_ref()
                        .and_then(|d| d.art.as_ref())
                        .map(|art| {
                            let side = (self.sidebar_width() - 20.)
                                .min((self.window_size.height - 370.).max(44.));
                            image(&art.cover)
                                .width(side)
                                .height(side)
                                .border_radius(material::RADIUS)
                        })
                ]
                .spacing(5),
                [8, 9],
                self.sidebar_width(),
                Length::Fill,
            )
        });
        let body = match self.screen {
            Screen::Discover => self
                .discover
                .view(self.content_width() - if compact { 68. } else { 80. }),
            Screen::Player => container(self.listening_room())
                .padding(iced::Padding::default().bottom(84))
                .into(),
            Screen::Library => self.library.view(self.appearance).map(Message::Library),
            Screen::Playlists => self.playlists.view(&self.logo).map(Message::Playlists),
            Screen::Settings => self.settings(),
            Screen::Audio => scrollable(
                column![
                    text("Audio").size(23).font(super::material::HEADING),
                    self.audio_controls()
                ]
                .spacing(18),
            )
            .into(),
        };
        let body: Element<'_, Message> = if self.queue_is_open() {
            stack![
                container(body).width(Length::Fill).height(Length::Fill),
                container(iced::widget::opaque(
                    container(self.queue_view()).width(244)
                ))
                .padding(iced::Padding::default().bottom(84))
                .align_right(Length::Fill)
                .align_bottom(Length::Fill)
            ]
            .into()
        } else {
            body
        };
        let middle = row![
            sidebar.map(|sidebar| container(sidebar).padding(iced::Padding::default().bottom(84))),
            container(body)
                .padding(if compact { 12 } else { 18 })
                .width(Length::Fill)
                .height(Length::Fill)
                .clip(true)
        ]
        .spacing(8)
        .height(Length::Fill);
        let bar = row![
            tooltip(
                super::material::button(
                    container(icon!("sidebar"))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill)
                )
                .width(super::SIDEBAR_CONTROL_WIDTH)
                .height(28)
                .padding(0)
                .style(if self.sidebar_visible {
                    soft_button
                } else {
                    super::material::text_button
                })
                .on_press(Message::ToggleSidebar),
                text(if self.sidebar_visible {
                    "Hide sidebar"
                } else {
                    "Show sidebar"
                })
                .size(13),
                tooltip::Position::Bottom,
            )
            .style(super::material::tooltip),
            container(brand).padding([0, 9])
        ]
        .align_y(Alignment::Center)
        .height(32);
        #[cfg(windows)]
        let bar = bar.push(super::frame::controls(self.maximized));
        let chrome = column![bar, container(middle).padding([0, 9]).height(Length::Fill)]
            .width(Length::Fill)
            .height(Length::Fill);
        let art = (self.screen == Screen::Library)
            .then(|| self.library.backdrop())
            .flatten();
        stack![
            super::background::view(art),
            chrome,
            container(iced::widget::opaque(self.dock()))
                .padding(9)
                .align_bottom(Length::Fill)
                .width(Length::Fill),
        ]
        .into()
    }

    fn audio_controls(&self) -> Element<'_, Message> {
        let settings = column![
            row![
                text("Output").size(14),
                pick_list(self.devices.as_slice(), Some(&self.device), Message::Device)
                    .width(Length::FillPortion(2))
                    .style(|theme, status| {
                        let mut style = pick_list::default(theme, status);
                        style.border.radius = material::RADIUS.into();
                        style
                    })
                    .menu_style(|theme| {
                        let mut style = iced::widget::overlay::menu::default(theme);
                        style.border.radius = material::RADIUS.into();
                        style
                    }),
                text("Volume").size(14),
                slider(0..=100, self.volume, Message::Volume).width(Length::FillPortion(1)),
                text(format!("{}%", self.volume)).size(13),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
        ]
        .spacing(6);
        let mut second = row![
            super::material::button("Stereo check")
                .style(super::material::text_button)
                .on_press(Message::Test),
            super::material::button("Stop")
                .style(super::material::text_button)
                .on_press_maybe(self.active().map(|_| Message::Stop))
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        if let Some(info) = self.info {
            second = second.push(
                text(format!(
                    "{} Hz / {} ch to {} Hz / {} ch",
                    info.input_rate, info.input_channels, info.output_rate, info.output_channels
                ))
                .size(12)
                .color(MUTED),
            );
        }
        settings.push(second).into()
    }

    fn settings(&self) -> Element<'_, Message> {
        let value = self.appearance.percent();
        let mut settings = column![
            text("Settings").size(23).font(material::HEADING),
            text("Appearance").size(16).font(material::HEADING),
            row![
                text("Glass opacity"),
                horizontal_space(),
                text(format!("{value}%"))
            ],
            text("Applies to floating controls and panels.")
                .size(12)
                .color(color!(0x70, 0x70, 0x70)),
            slider(0..=100, value, Message::Opacity),
            iced::widget::toggler(self.appearance.blur)
                .label("Glass blur")
                .on_toggle(Message::Blur),
        ]
        .max_width(360)
        .spacing(12);
        if let Some(error) = &self.appearance_error {
            settings = settings.push(text(error).size(12));
        }
        settings.into()
    }

    pub(super) fn queue_is_open(&self) -> bool {
        self.queue_visible
    }

    fn track_title(&self) -> &str {
        self.artwork
            .details
            .as_ref()
            .and_then(|d| d.title.as_deref())
            .or_else(|| self.queue.current().map(QueueItem::title))
            .unwrap_or("Play your music")
    }

    fn artist(&self) -> Option<&str> {
        self.artwork
            .details
            .as_ref()
            .and_then(|d| d.artist.as_deref())
            .or_else(|| match self.queue.current() {
                Some(QueueItem::Indexed(t)) => Some(t.artist.as_str()),
                Some(QueueItem::Direct(super::Source::YouTube(t))) => Some(t.artist.as_str()),
                _ => None,
            })
    }

    fn listening_room(&self) -> Element<'_, Message> {
        let has_queue = self.queue.current().is_some();
        let short = self.window_size.height < 560.;
        let stage_width = self.content_width() - 44.;
        let inset = if short || stage_width < 640. { 20 } else { 36 };
        let status = match &self.transport {
            Transport::Playing(_) => "Now playing",
            Transport::Paused(_) => "Paused",
            Transport::Opening { .. } => "Opening",
            Transport::Failed(_) => "Playback error",
            Transport::Ended(_) => "Finished",
            Transport::Idle if has_queue => "Stopped",
            Transport::Idle => "",
        };
        let title_size = if short || stage_width < 640. { 23 } else { 30 };
        let mut caption = column![].spacing(5);
        if !status.is_empty() {
            caption = caption.push(text(status).size(14).color(SECONDARY));
        }
        caption = caption.push(
            text(self.track_title())
                .size(title_size)
                .font(material::TYPE)
                .line_height(1.03),
        );
        if let Some(artist) = self.artist() {
            caption = caption.push(text(artist).size(14).color(SECONDARY));
        }
        if !has_queue {
            caption = caption.push(
                row![
                    super::material::button("Open files")
                        .on_press(Message::Browse)
                        .style(super::material::primary_button),
                    super::material::button("Browse library")
                        .on_press(Message::ShowLibrary)
                        .style(soft_button)
                ]
                .spacing(9),
            );
        }
        container(caption)
            .padding(inset)
            .align_bottom(Length::Fill)
            .width(Length::Fill)
            .clip(true)
            .into()
    }

    fn queue_view(&self) -> Element<'_, Message> {
        let mut rows = column![].spacing(6);
        for (index, item) in self.queue.entries().iter().enumerate() {
            let selected = index == self.queue.index();
            let title = if selected {
                self.track_title()
            } else {
                item.title()
            };
            let mut label = column![text(title).size(14)].width(Length::Fill);
            let artist = if selected {
                self.artist()
            } else if let QueueItem::Direct(super::Source::YouTube(t)) = item {
                Some(t.artist.as_str())
            } else if let QueueItem::Indexed(t) = item {
                Some(t.artist.as_str())
            } else {
                None
            };
            if let Some(name) = artist.filter(|s| !s.is_empty()) {
                label = label.push(text(name).size(13).color(MUTED));
            }
            if let Some(reason) = self.skipped.get(&index) {
                label = label.push(text(format!("Skipped: {reason}")).size(12).color(MUTED));
            }
            rows = rows.push(super::context::row(
                super::material::button(
                    row![
                        text(format!("{:02}", index + 1))
                            .width(24)
                            .size(13)
                            .color(if selected { SECONDARY } else { MUTED }),
                        label
                    ]
                    .align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .padding([4, 6])
                .style(if selected {
                    soft_button
                } else {
                    super::material::text_button
                })
                .on_press(Message::Select(index)),
                move |point| Message::QueueMenu(index, point),
            ));
        }
        let len = self.queue.entries().len();
        let track_count = if len == 0 {
            String::new()
        } else {
            format!("{}/{len}", self.queue.index() + 1)
        };
        if len == 0 {
            rows = rows.push(text("The queue is empty.").size(13).color(MUTED));
        }
        let queue = column![
            row![
                text("Play queue").size(18),
                horizontal_space(),
                text(track_count).color(MUTED).size(14)
            ]
            .align_y(Alignment::Center),
            horizontal_rule(1),
            container(scrollable(rows).height(Length::Shrink))
                .max_height((self.window_size.height - 240.).clamp(60., 300.)),
            row![
                horizontal_space(),
                super::material::button("Save queue")
                    .style(soft_button)
                    .on_press_maybe((len > 0).then_some(Message::SaveQueue))
            ]
        ]
        .spacing(6);
        material::panel(self.appearance, queue, 12, Length::Fill, Length::Shrink)
    }

    fn dock(&self) -> Element<'_, Message> {
        let item = self.queue.current();
        let title = self.track_title();
        let artist = self.artist();
        let mut meta = column![
            text(title)
                .size(14)
                .height(19)
                .wrapping(text::Wrapping::None)
        ]
        .spacing(2)
        .width(Length::Fill);
        if let Some(artist) = artist {
            meta = meta.push(
                text(artist)
                    .height(16)
                    .size(12)
                    .color(MUTED)
                    .wrapping(text::Wrapping::None),
            );
        }
        let position = self.position();
        let duration = self.track_duration(position);
        let elapsed = self.scrub.map(Duration::from_secs_f32).unwrap_or(position);
        let playing = matches!(self.transport, Transport::Playing(_));
        let toggle = (item.is_some() && !matches!(self.transport, Transport::Opening { .. }))
            .then_some(Message::Toggle);
        let seek: Element<'_, Message> = if let Some(total) = duration.filter(|d| {
            !d.is_zero()
                && matches!(
                    self.transport,
                    Transport::Playing(_) | Transport::Paused(_) | Transport::Ended(_)
                )
        }) {
            slider(
                0.0..=total.as_secs_f32(),
                elapsed.as_secs_f32().min(total.as_secs_f32()),
                Message::Scrub,
            )
            .height(28)
            .step(0.05)
            .on_release(Message::Seek)
            .style(|theme, status| {
                let mut s = slider::default(theme, status).with_circular_handle(
                    if matches!(status, slider::Status::Active) {
                        3.0
                    } else {
                        4.0
                    },
                );
                s.rail.width = 2.;
                s.rail.backgrounds = (
                    Color::WHITE.into(),
                    Color::from_rgba(1., 1., 1., 0.19).into(),
                );
                s.handle.background = Color::WHITE.into();
                s
            })
            .into()
        } else {
            container(horizontal_rule(2))
                .center_y(28)
                .width(Length::Fill)
                .into()
        };
        let line = row![
            control(
                icon!("previous"),
                "Previous",
                self.queue.index().checked_sub(1).map(Message::Select),
                false,
                false
            ),
            control(
                if playing {
                    icon!("pause")
                } else {
                    icon!("play")
                },
                if playing {
                    "Pause"
                } else if matches!(self.transport, Transport::Ended(_)) {
                    "Replay"
                } else {
                    "Play"
                },
                toggle,
                true,
                false
            ),
            control(
                icon!("next"),
                "Next",
                (self.queue.index() + 1 < self.queue.entries().len())
                    .then(|| Message::Select(self.queue.index() + 1)),
                false,
                false
            ),
            text(clock(elapsed)).size(13),
            seek,
            text(duration.map_or_else(|| "--:--".to_owned(), clock)).size(13),
            Space::new().width(8),
            control(
                icon!("shuffle"),
                "Shuffle upcoming tracks",
                (self.queue.entries().len() > 1).then_some(Message::Shuffle),
                false,
                self.shuffle_order.is_some()
            ),
            control(
                icon!("repeat"),
                "Repeat queue",
                item.map(|_| Message::Repeat),
                false,
                self.repeat
            ),
            tooltip(
                super::material::button(icon!("queue"))
                    .padding(6)
                    .on_press(Message::ToggleQueue)
                    .style(if self.queue_is_open() {
                        soft_button
                    } else {
                        super::material::text_button
                    }),
                text(if self.queue_is_open() {
                    "Hide queue"
                } else {
                    "Show queue"
                })
                .size(13),
                tooltip::Position::Top
            )
            .style(super::material::tooltip)
        ]
        .align_y(Alignment::Center)
        .spacing(4);
        let mut dock = column![].spacing(4);
        if let Some(error) = &self.artwork.error {
            dock = dock.push(text(error).size(13));
        }
        if let Transport::Failed(error) = &self.transport {
            dock = dock.push(text(error).color(MUTED).size(13));
        }
        if matches!(self.transport, Transport::Ended(_))
            && self.skipped.contains_key(&self.queue.index())
        {
            dock = dock.push(
                text("No available tracks. Restore the files or reconnect the drive, then replay.")
                    .size(13),
            );
        }
        let mut strip = row![].align_y(Alignment::Center).spacing(8);
        if self.screen != Screen::Player && item.is_some() {
            let metadata = super::material::button(container(meta).width(Length::Fill).clip(true))
                .padding([0, 8])
                .style(super::material::text_button)
                .on_press_maybe((!self.playlists.busy()).then_some(Message::ShowPlayer));
            if self.window_size.width >= 760. {
                strip = strip.push(metadata.width(190));
            } else {
                dock = dock.push(metadata.width(Length::Fill));
            }
        }
        material::panel(
            self.appearance,
            dock.push(strip.push(container(line).width(Length::Fill))),
            [6, 10],
            Length::Fill,
            Length::Shrink,
        )
    }
}
