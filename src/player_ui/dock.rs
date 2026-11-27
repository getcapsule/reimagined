use super::{App, Message, QueueItem, Screen, Transport, clock, material};
use iced::{
    Alignment, Border, color,
    widget::{Space, image, scrollable, stack, tooltip},
};
use iced::{
    Color, Element, Length,
    widget::{
        button, column, container, pick_list, row, rule::horizontal as horizontal_rule, slider,
        space::horizontal as horizontal_space, text, vertical_slider,
    },
};
use std::time::Duration;

const PLAYER_PILL_HEIGHT: f32 = 48.;

const MUTED: Color = material::SECONDARY;
const SECONDARY: Color = material::TEXT;

pub(super) fn soft_button(theme: &iced::Theme, status: button::Status) -> button::Style {
    material::raised_button(theme, status)
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
            Some(material::glaze(
                if hover { 0.27 } else { 0.21 },
                if hover { 0.15 } else { 0.10 },
            ))
        } else if hover || active {
            Some(material::glaze(if hover { 0.16 } else { 0.12 }, 0.05))
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
                color: Color::from_rgba(1., 1., 1., if hover { 0.30 } else { 0.21 }),
            },
            shadow: if primary || active {
                iced::Shadow {
                    color: Color::from_rgba(0., 0., 0., 0.28),
                    offset: iced::Vector::new(0., 2.),
                    blur_radius: 8.,
                }
            } else {
                iced::Shadow::default()
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
        if !self.sidebar_expanded {
            52.
        } else if self.window_size.width < 760. {
            112.
        } else {
            144.
        }
    }

    fn content_width(&self) -> f32 {
        self.window_size.width - self.sidebar_width() - 8.
    }

    pub(super) fn shell(&self) -> Element<'_, Message> {
        let compact = self.window_size.width < 760.;
        let busy = self.screen == Screen::Playlists && self.playlists.busy();
        let nav = |symbol: iced::widget::Svg<'static>,
                   name: &'static str,
                   screen,
                   msg|
         -> Element<'_, Message> {
            let active = Some(self.screen) == screen;
            let face: Element<'_, Message> = if self.sidebar_expanded {
                row![symbol, text(name).size(13)]
                    .align_y(Alignment::Center)
                    .spacing(7)
                    .into()
            } else {
                container(symbol).center_x(Length::Fill).into()
            };
            let button = material::button(face)
                .width(Length::Fill)
                .padding(if self.sidebar_expanded {
                    [5, 6]
                } else {
                    [9, 0]
                })
                .on_press_maybe((!busy && !active).then_some(msg))
                .style(move |theme, state| {
                    if active {
                        soft_button(theme, state)
                    } else {
                        material::text_button(theme, state)
                    }
                });
            if self.sidebar_expanded {
                button.into()
            } else {
                tooltip(
                    button,
                    text(if name == "Search" {
                        "Search (Ctrl+K)"
                    } else {
                        name
                    })
                    .size(13),
                    tooltip::Position::Right,
                )
                .style(material::tooltip)
                .into()
            }
        };
        let brand = text("capsule").size(17).font(material::HEADING);
        let sidebar = material::panel(
            self.appearance,
            column![
                self.sidebar_expanded
                    .then(|| text("Browse").size(11).color(MUTED)),
                nav(
                    icon!("search"),
                    "Search",
                    None,
                    Message::Search(super::search::Msg::Open)
                ),
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
                self.sidebar_expanded
                    .then(|| text("Controls").size(11).color(MUTED)),
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
            ]
            .spacing(5),
            [8, if self.sidebar_expanded { 9 } else { 6 }],
            self.sidebar_width(),
            Length::Fill,
        );
        let body = match self.screen {
            Screen::Discover => self
                .discover
                .view(self.content_width() - if compact { 68. } else { 80. }),
            Screen::Player => container(self.listening_room())
                .padding(iced::Padding::default().bottom(96))
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
                .padding(iced::Padding::default().bottom(96))
                .align_right(Length::Fill)
                .align_bottom(Length::Fill)
            ]
            .into()
        } else {
            body
        };
        let middle = row![
            container(sidebar).padding(iced::Padding::default().top(20).bottom(76)),
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
                .style(if self.sidebar_expanded {
                    soft_button
                } else {
                    super::material::text_button
                })
                .on_press(Message::ToggleSidebar),
                text(if self.sidebar_expanded {
                    "Collapse sidebar"
                } else {
                    "Expand sidebar"
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
        let art = self.backdrop();
        stack![
            super::background::view(art),
            chrome,
            container(self.dock())
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
        material::floating(self.appearance, queue, 12, Length::Fill, Length::Shrink)
    }

    pub(super) fn volume_popup(&self) -> Element<'_, Message> {
        let level = vertical_slider(0..=100, self.volume, Message::Volume)
            .height(136)
            .width(24)
            .style(|theme, status| {
                let mut style = slider::default(theme, status).with_circular_handle(5.);
                style.rail.width = 3.;
                style.rail.backgrounds = (
                    Color::WHITE.into(),
                    Color::from_rgba(1., 1., 1., 0.19).into(),
                );
                style
            });
        let popup = material::floating(
            self.appearance,
            column![
                text(format!("{}%", self.volume)).size(11).color(SECONDARY),
                level
            ]
            .spacing(10)
            .align_x(Alignment::Center)
            .width(Length::Fill),
            [12, 10],
            56,
            184,
        );
        container(iced::widget::opaque(popup))
            .padding(iced::Padding::default().right(40).bottom(65))
            .align_right(Length::Fill)
            .align_bottom(Length::Fill)
            .into()
    }

    fn dock(&self) -> Element<'_, Message> {
        let compact = self.window_size.width < 760.;
        let item = self.queue.current();
        let title = self.track_title();
        let artist = self.artist();
        let mut meta = column![
            text(title)
                .size(if compact { 12 } else { 13 })
                .wrapping(text::Wrapping::None)
        ]
        .spacing(4);
        if let Some(artist) = artist {
            meta = meta.push(
                text(artist)
                    .size(11)
                    .color(MUTED)
                    .wrapping(text::Wrapping::None),
            );
        }
        let mut metadata = row![].spacing(9).align_y(Alignment::Center);
        let cover = self
            .artwork
            .details
            .as_ref()
            .and_then(|d| d.art.as_ref())
            .filter(|_| !compact);
        if let Some(art) = cover {
            metadata = metadata.push(
                image(&art.cover)
                    .width(PLAYER_PILL_HEIGHT - 4.)
                    .height(PLAYER_PILL_HEIGHT - 4.)
                    .border_radius(material::RADIUS),
            );
        }
        if cover.is_none() && !compact && self.artwork.loading() {
            metadata = metadata.push(super::skeleton::block(
                PLAYER_PILL_HEIGHT - 4.,
                PLAYER_PILL_HEIGHT - 4.,
            ));
        }
        let metadata = material::button(
            container(metadata.push(container(meta).width(Length::Fill).clip(true)))
                .center_y(Length::Fill),
        )
        .padding(0)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(material::text_button)
        .on_press_maybe(
            (item.is_some() && !self.playlists.busy() && self.screen != Screen::Player)
                .then_some(Message::ShowPlayer),
        );
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
            .height(18)
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
                .center_y(18)
                .width(Length::Fill)
                .into()
        };

        let transport = row![
            control(
                icon!("shuffle"),
                "Shuffle upcoming tracks",
                (self.queue.entries().len() > 1).then_some(Message::Shuffle),
                false,
                self.shuffle_order.is_some()
            ),
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
                if playing { "Pause" } else { "Play" },
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
            control(
                icon!("repeat"),
                "Repeat queue",
                item.map(|_| Message::Repeat),
                false,
                self.repeat
            ),
        ]
        .spacing(4)
        .align_y(Alignment::Center);
        let timeline = row![
            text(clock(elapsed)).size(11),
            seek,
            text(duration.map_or_else(|| "--:--".to_owned(), clock)).size(11),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        let options = row![
            control(
                icon!("audio"),
                "Volume",
                Some(Message::ToggleVolume),
                false,
                self.volume_open,
            ),
            control(
                icon!("queue"),
                if self.queue_is_open() {
                    "Hide queue"
                } else {
                    "Show queue"
                },
                Some(Message::ToggleQueue),
                false,
                self.queue_is_open()
            ),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        let playback = column![
            container(iced::widget::opaque(material::floating(
                self.appearance,
                transport,
                [4, 10],
                Length::Shrink,
                42,
            )))
            .center_x(Length::Fill),
            iced::widget::opaque(material::floating(
                self.appearance,
                container(timeline).center_y(Length::Fill),
                [6, 10],
                Length::Fill,
                PLAYER_PILL_HEIGHT,
            )),
        ]
        .spacing(8)
        .width(Length::Fill);
        let pills = row![
            iced::widget::opaque(material::floating(
                self.appearance,
                metadata,
                [2, 8],
                if compact { 106 } else { 220 },
                PLAYER_PILL_HEIGHT,
            )),
            playback,
            iced::widget::opaque(material::floating(
                self.appearance,
                container(options).center_y(Length::Fill),
                [8, 10],
                82,
                PLAYER_PILL_HEIGHT,
            )),
        ]
        .spacing(10)
        .align_y(Alignment::End);
        let mut dock = column![].spacing(6);
        if let Some(error) = &self.artwork.error {
            dock = dock.push(text(error).size(12).color(MUTED));
        }
        if let Transport::Failed(error) = &self.transport {
            dock = dock.push(text(error).size(12).color(MUTED));
        }
        if matches!(self.transport, Transport::Ended(_))
            && self.skipped.contains_key(&self.queue.index())
        {
            dock = dock.push(
                text("No available tracks. Restore the files or reconnect the drive, then replay.")
                    .size(12),
            );
        }
        dock.push(pills).into()
    }
}
