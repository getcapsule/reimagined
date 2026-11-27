use super::{Message, artwork, background, dock, material};
use capsule::providers::youtube::{self, Track, VideoId, Youtube};
use iced::{
    Alignment, Color, ContentFit, Element, Fill, Task,
    widget::{
        Space, column, container, image, operation, row, scrollable, space::horizontal, text,
    },
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

const FEATURED: usize = 6;
const PAGE_SIZE: usize = 9;
const FEATURED_SCROLL: &str = "discover-featured-songs";
const SONG_SCROLL: &str = "discover-songs";
const MUTED: Color = material::SECONDARY;

#[derive(Debug, Clone, Default)]
enum Cover {
    #[default]
    Loading,
    Ready(image::Handle),
    Unavailable,
}
#[derive(Debug, Clone)]
pub(super) struct Pick {
    track: Track,
    cover: Cover,
}
#[derive(Default)]
enum State {
    #[default]
    Idle,
    Loading,
    Ready(Arc<[Pick]>),
    Failed(String),
}
#[derive(Debug, Clone)]
pub(super) enum Msg {
    FeaturedStep { forward: bool, width: f32 },
    FeaturedScrolled(f32),
    SongPage { page: usize, width: f32 },
    SongScrolled { offset: f32, width: f32 },
    Snap { revision: u64, width: f32 },
    Animate(Instant),
}
struct Motion {
    from: f32,
    to: f32,
    started: Instant,
}
impl Motion {
    fn offset(&self, now: Instant) -> (f32, bool) {
        let t = (now.saturating_duration_since(self.started).as_secs_f32() / 0.24).min(1.);
        (
            self.from + (self.to - self.from) * (1. - (1. - t).powi(3)),
            t == 1.,
        )
    }
}
#[derive(Default)]
pub(super) struct Pane {
    revision: u64,
    state: State,
    featured_offset: f32,
    song_offset: f32,
    song_page: usize,
    scroll_revision: u64,
    featured_motion: Option<Motion>,
    song_motion: Option<Motion>,
    artwork_error: bool,
    backdrop: Option<image::Handle>,
}
fn animate() -> Task<Message> {
    Task::perform(
        async {
            tokio::time::sleep(Duration::from_millis(16)).await;
            Instant::now()
        },
        |now| Message::Discover(Msg::Animate(now)),
    )
}
fn scroll(id: &'static str, x: f32) -> Task<Message> {
    operation::scroll_to(id, scrollable::AbsoluteOffset { x, y: 0. })
}
fn card_width(width: f32) -> f32 {
    ((width - 24.) / 3.).clamp(174., 246.)
}
fn art(pick: &Pick, width: f32, height: f32) -> Element<'_, Message> {
    if matches!(pick.cover, Cover::Loading) {
        return super::skeleton::block(width, height);
    }
    if let Cover::Ready(cover) = &pick.cover {
        return image(cover)
            .width(width)
            .height(height)
            .content_fit(ContentFit::Cover)
            .border_radius(material::RADIUS)
            .into();
    }
    let label = "Cover unavailable";
    container(text(label).size(10).color(MUTED))
        .center_x(width)
        .center_y(height)
        .style(|_| container::Style {
            background: Some(Color::from_rgb8(27, 29, 32).into()),
            border: iced::Border::default().rounded(material::RADIUS),
            ..Default::default()
        })
        .into()
}
impl Pane {
    fn picks(&self) -> &[Pick] {
        if let State::Ready(picks) = &self.state {
            picks
        } else {
            &[]
        }
    }
    pub(super) fn backdrop(&self) -> Option<&image::Handle> {
        self.backdrop.as_ref()
    }
    pub(super) fn refresh(&mut self, provider: Arc<Youtube>) -> Task<Message> {
        self.revision += 1;
        let revision = self.revision;
        self.state = State::Loading;
        self.backdrop = None;
        self.featured_offset = 0.;
        self.song_offset = 0.;
        self.song_page = 0;
        self.scroll_revision += 1;
        self.featured_motion = None;
        self.song_motion = None;
        self.artwork_error = false;
        Task::batch([
            scroll(FEATURED_SCROLL, 0.),
            scroll(SONG_SCROLL, 0.),
            Task::perform(
                async move {
                    youtube::discover(provider)
                        .await
                        .map(|tracks| {
                            Arc::from(
                                tracks
                                    .into_iter()
                                    .map(|track| Pick {
                                        track,
                                        cover: Cover::Loading,
                                    })
                                    .collect::<Vec<_>>(),
                            )
                        })
                        .map_err(|e| e.to_string())
                },
                move |result| Message::DiscoverLoaded(revision, result),
            ),
        ])
    }
    pub(super) fn loaded(
        &mut self,
        revision: u64,
        result: Result<Arc<[Pick]>, String>,
    ) -> Task<Message> {
        if revision != self.revision {
            return Task::none();
        }
        let picks = match result {
            Ok(picks) => picks,
            Err(error) => {
                self.state = State::Failed(error);
                return Task::none();
            }
        };
        let slots = Arc::new(tokio::sync::Semaphore::new(4));
        let tasks = picks
            .iter()
            .map(|pick| {
                let track = pick.track.clone();
                let slots = slots.clone();
                Task::perform(
                    async move {
                        let id = track.id.clone();
                        let result = match slots.acquire_owned().await {
                            Ok(_permit) => tokio::task::spawn_blocking(move || {
                                artwork::youtube_cover(&track, 512)
                            })
                            .await
                            .map_err(|e| e.to_string())
                            .and_then(|r| r),
                            Err(e) => Err(e.to_string()),
                        };
                        (id, result)
                    },
                    move |(id, result)| Message::DiscoverCover(revision, id, result),
                )
            })
            .collect::<Vec<_>>();
        self.state = State::Ready(picks);
        self.backdrop = None;
        Task::batch(tasks)
    }
    pub(super) fn cover(
        &mut self,
        revision: u64,
        id: &VideoId,
        result: Result<Option<image::Handle>, String>,
    ) {
        if revision != self.revision {
            return;
        }
        if let State::Ready(picks) = &mut self.state
            && let Some(index) = picks.iter().position(|p| &p.track.id == id)
        {
            let cover = match result {
                Ok(Some(cover)) => Cover::Ready(cover),
                _ => {
                    self.artwork_error = true;
                    Cover::Unavailable
                }
            };
            if index == 0 {
                self.backdrop = match &cover {
                    Cover::Ready(cover) => background::colors(cover),
                    _ => None,
                };
            }
            Arc::make_mut(picks)[index].cover = cover;
        }
    }

    pub(super) fn track(&self, revision: u64, index: usize) -> Option<&Track> {
        if revision != self.revision {
            return None;
        }
        self.picks().get(index).map(|p| &p.track)
    }
    pub(super) fn update(&mut self, msg: Msg) -> Task<Message> {
        match msg {
            Msg::FeaturedScrolled(offset) => self.featured_offset = offset,
            Msg::FeaturedStep { forward, width } => {
                let stride = card_width(width) + 12.;
                let from = self.featured_offset;
                let max = (stride * self.picks().len().min(FEATURED) as f32 - 12. - width).max(0.);
                let to = (self.featured_motion.as_ref().map_or(from, |m| m.to)
                    + if forward { stride } else { -stride })
                .clamp(0., max);
                self.featured_motion = Some(Motion {
                    from,
                    to,
                    started: Instant::now(),
                });
                return animate();
            }
            Msg::SongPage { page, width } if page < 2 => {
                self.song_page = page;
                self.song_motion = Some(Motion {
                    from: self.song_offset,
                    to: page as f32 * width,
                    started: Instant::now(),
                });
                return animate();
            }
            Msg::SongScrolled { offset, width } => {
                self.song_offset = offset;
                self.song_page = usize::from(offset >= width * 0.5);
                self.scroll_revision += 1;
                let revision = self.scroll_revision;
                return Task::perform(
                    async move {
                        tokio::time::sleep(Duration::from_millis(160)).await;
                        Msg::Snap { revision, width }
                    },
                    Message::Discover,
                );
            }
            Msg::Snap { revision, width }
                if revision == self.scroll_revision && self.song_motion.is_none() =>
            {
                let to = self.song_page as f32 * width;
                if (to - self.song_offset).abs() > 0.5 {
                    return self.update(Msg::SongPage {
                        page: self.song_page,
                        width,
                    });
                }
            }
            Msg::Animate(now) => {
                let mut tasks = Vec::new();
                for (id, motion, offset) in [
                    (
                        FEATURED_SCROLL,
                        &mut self.featured_motion,
                        &mut self.featured_offset,
                    ),
                    (SONG_SCROLL, &mut self.song_motion, &mut self.song_offset),
                ] {
                    if let Some(current) = motion {
                        let (x, finished) = current.offset(now);
                        *offset = x;
                        tasks.push(scroll(id, x));
                        if finished {
                            *motion = None;
                        }
                    }
                }
                if self.featured_motion.is_some() || self.song_motion.is_some() {
                    tasks.push(animate());
                }
                return Task::batch(tasks);
            }
            _ => {}
        }
        Task::none()
    }

    fn song<'a>(&'a self, index: usize, pick: &'a Pick, compact: bool) -> Element<'a, Message> {
        let title = column![
            text(&pick.track.title)
                .size(13)
                .wrapping(text::Wrapping::None),
            text(&pick.track.artist)
                .size(12)
                .color(MUTED)
                .wrapping(text::Wrapping::None),
        ]
        .spacing(4);
        let side = if compact { 32. } else { 44. };
        let content = row![
            art(pick, side, side),
            container(title).width(Fill).clip(true),
            dock::icon!("play").width(12).height(12),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        let button = material::button(content)
            .padding(8)
            .width(Fill)
            .style(material::text_button)
            .on_press(Message::DiscoverPlay {
                revision: self.revision,
                index,
            });
        super::context::row(button, move |point| {
            Message::TrackMenu(
                capsule::library::playlists::SavedTrack::YouTube(pick.track.clone()),
                point,
            )
        })
    }
    pub(super) fn view(&self, pane_width: f32) -> Element<'_, Message> {
        let width = (pane_width - 26.).max(220.);
        let arrow = |forward, msg| {
            dock::control(
                if forward {
                    dock::icon!("chevron-right")
                } else {
                    dock::icon!("chevron-left")
                },
                if forward { "Next" } else { "Previous" },
                msg,
                false,
                false,
            )
        };
        let mut page = column![
            row![
                text("Discover").size(23).font(material::HEADING),
                horizontal(),
                material::button("Refresh picks")
                    .style(dock::soft_button)
                    .on_press(Message::RefreshDiscover),
            ]
            .align_y(Alignment::Center)
        ]
        .spacing(12);
        match &self.state {
            State::Failed(error) => {
                return page
                    .push(text(error).size(13))
                    .push(material::button("Try again").on_press(Message::RefreshDiscover))
                    .into();
            }
            State::Idle | State::Loading => {
                let side = card_width(width);
                let cards = (0..FEATURED).fold(row![].spacing(12), |cards, _| {
                    cards.push(
                        column![
                            super::skeleton::block(side, side * 0.75),
                            super::skeleton::block(side * 0.75, 16.),
                            super::skeleton::block(side * 0.45, 12.),
                        ]
                        .spacing(10)
                        .width(side),
                    )
                });
                let mut grid = column![].spacing(8);
                for _ in 0..3 {
                    grid = grid.push((0..3).fold(row![].spacing(12), |line, _| {
                        line.push(
                            container(super::skeleton::song(if width < 620. { 32. } else { 44. }))
                                .padding(8)
                                .width((width - 24.) / 3.),
                        )
                    }));
                }
                return scrollable(
                    page.push(
                        text("For your next listen")
                            .size(18)
                            .font(material::HEADING),
                    )
                    .push(scrollable(cards).width(width).direction(
                        scrollable::Direction::Horizontal(scrollable::Scrollbar::hidden()),
                    ))
                    .push(text("More to explore").size(18).font(material::HEADING))
                    .push(grid)
                    .push(Space::new().height(96))
                    .padding(iced::Padding::default().left(5).right(18)),
                )
                .direction(scrollable::Direction::Vertical(
                    scrollable::Scrollbar::hidden(),
                ))
                .into();
            }
            _ => {}
        }
        if self.artwork_error {
            page = page.push(
                text("Some covers are unavailable. Refresh to retry.")
                    .size(12)
                    .color(MUTED),
            );
        }
        let count = self.picks().len().min(FEATURED);
        page = page.push(
            row![
                text("For your next listen")
                    .size(18)
                    .font(material::HEADING),
                horizontal(),
                arrow(
                    false,
                    (self.featured_offset > 0.5).then_some(Message::Discover(Msg::FeaturedStep {
                        forward: false,
                        width
                    }))
                ),
                arrow(
                    true,
                    (self.featured_offset
                        < (card_width(width) + 12.) * count as f32 - 12. - width - 0.5)
                        .then_some(Message::Discover(Msg::FeaturedStep {
                            forward: true,
                            width
                        }))
                ),
            ]
            .align_y(Alignment::Center)
            .spacing(5),
        );
        let card_width = card_width(width);
        let cards = self.picks().iter().take(FEATURED).enumerate().fold(
            row![].spacing(12),
            |cards, (index, pick)| {
                cards.push(super::context::row(
                    material::button(
                        column![
                            art(pick, card_width, card_width * 0.75),
                            container(
                                column![
                                    text(&pick.track.title)
                                        .size(15)
                                        .font(material::HEADING)
                                        .wrapping(text::Wrapping::None),
                                    text(&pick.track.artist)
                                        .size(12)
                                        .color(MUTED)
                                        .wrapping(text::Wrapping::None),
                                ]
                                .spacing(5)
                            )
                            .width(card_width)
                            .clip(true),
                        ]
                        .spacing(10),
                    )
                    .style(material::text_button)
                    .padding(0)
                    .width(card_width)
                    .on_press(Message::DiscoverPlay {
                        revision: self.revision,
                        index,
                    }),
                    move |point| {
                        Message::TrackMenu(
                            capsule::library::playlists::SavedTrack::YouTube(pick.track.clone()),
                            point,
                        )
                    },
                ))
            },
        );
        page = page.push(
            scrollable(cards)
                .width(width)
                .id(FEATURED_SCROLL)
                .direction(scrollable::Direction::Horizontal(
                    scrollable::Scrollbar::hidden(),
                ))
                .on_scroll(|viewport| {
                    Message::Discover(Msg::FeaturedScrolled(viewport.absolute_offset().x))
                }),
        );
        let page_control = |page| Message::Discover(Msg::SongPage { page, width });
        page = page.push(
            row![
                text("More to explore").size(18).font(material::HEADING),
                horizontal(),
                text(format!("{} / 2", self.song_page + 1))
                    .size(12)
                    .color(MUTED),
                arrow(false, (self.song_page > 0).then(|| page_control(0))),
                arrow(true, (self.song_page == 0).then(|| page_control(1))),
            ]
            .align_y(Alignment::Center)
            .spacing(5),
        );
        let songs = &self.picks()[count..];
        let mut pages = row![];
        for page_index in 0..2 {
            if songs.len() <= page_index * PAGE_SIZE {
                pages = pages.push(
                    container(
                        text("No more songs in this selection.")
                            .size(13)
                            .color(MUTED),
                    )
                    .width(width)
                    .height(196)
                    .center_y(196),
                );
                continue;
            }
            let mut grid = column![].spacing(8).width(width);
            for r in 0..3 {
                let mut line = row![].spacing(12);
                for c in 0..3 {
                    let index = page_index * PAGE_SIZE + r * 3 + c;
                    let slot = match songs.get(index) {
                        Some(pick) => self.song(count + index, pick, width < 620.),
                        None => container(Space::new().height(44)).padding(8).into(),
                    };
                    line = line.push(container(slot).width((width - 24.) / 3.));
                }
                grid = grid.push(line);
            }
            pages = pages.push(grid);
        }
        page = page.push(
            scrollable(pages)
                .width(width)
                .id(SONG_SCROLL)
                .direction(scrollable::Direction::Horizontal(
                    scrollable::Scrollbar::hidden(),
                ))
                .on_scroll(move |viewport| {
                    Message::Discover(Msg::SongScrolled {
                        offset: viewport.absolute_offset().x,
                        width,
                    })
                }),
        );
        page = page.push(
            container(
                row![
                    material::button("1")
                        .style(if self.song_page == 0 {
                            dock::soft_button
                        } else {
                            material::secondary_button
                        })
                        .on_press(page_control(0)),
                    material::button("2")
                        .style(if self.song_page == 1 {
                            dock::soft_button
                        } else {
                            material::secondary_button
                        })
                        .on_press(page_control(1)),
                ]
                .spacing(6),
            )
            .align_right(Fill),
        );
        scrollable(
            page.push(Space::new().height(96))
                .padding(iced::Padding::default().left(5).right(18)),
        )
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::hidden(),
        ))
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Pane {
        let picks = (0..24)
            .map(|i| Pick {
                track: Track {
                    id: format!("{i:011}").try_into().unwrap(),
                    title: format!("Song {i}"),
                    artist: format!("Artist {i}"),
                    thumbnail: None,
                    duration: Some(Duration::from_secs(180)),
                },
                cover: Cover::Loading,
            })
            .collect::<Vec<_>>();
        Pane {
            revision: 3,
            state: State::Ready(Arc::from(picks)),
            ..Default::default()
        }
    }
    #[test]
    fn unavailable_art_is_never_replaced_by_generated_content() {
        let mut pane = fixture();
        let id = pane.picks()[0].track.id.clone();
        pane.cover(3, &id, Ok(None));
        assert!(matches!(pane.picks()[0].cover, Cover::Unavailable));
        let actual = image::Handle::from_rgba(1, 1, vec![30, 60, 90, 255]);
        pane.cover(2, &id, Ok(Some(actual.clone())));
        assert!(matches!(pane.picks()[0].cover, Cover::Unavailable));
        pane.cover(3, &id, Ok(Some(actual.clone())));
        assert!(matches!(&pane.picks()[0].cover, Cover::Ready(cover) if cover == &actual));
        assert!(pane.track(2, 0).is_none());
        assert!(pane.track(3, 24).is_none());
    }
    #[test]
    fn both_song_pages_and_featured_carousel_scroll() {
        let mut pane = fixture();
        let _ = pane.update(Msg::SongPage {
            page: 1,
            width: 820.,
        });
        let _ = pane.update(Msg::Animate(Instant::now() + Duration::from_secs(1)));
        assert_eq!(pane.song_page, 1);
        assert_eq!(pane.song_offset, 820.);
        let _ = pane.update(Msg::FeaturedStep {
            forward: true,
            width: 820.,
        });
        let _ = pane.update(Msg::Animate(Instant::now() + Duration::from_secs(1)));
        assert!(pane.featured_offset > 0.);
    }
    #[test]
    fn featured_cards_and_grid_rows_play_individual_songs() {
        use iced_runtime::{
            UserInterface,
            core::{clipboard, mouse, renderer::Headless},
        };
        let pane = fixture();
        let mut renderer = iced::futures::executor::block_on(iced::Renderer::new(
            iced::Font::DEFAULT,
            iced::Pixels(13.),
            Some("tiny-skia"),
        ))
        .expect("test renderer");
        for (point, index) in [
            (iced::Point::new(100., 150.), 0),
            (iced::Point::new(150., 410.), 6),
        ] {
            let mut ui = UserInterface::build(
                pane.view(860.),
                iced::Size::new(860., 700.),
                Default::default(),
                &mut renderer,
            );
            let mut messages = Vec::new();
            let _ = ui.update(
                &[
                    iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                    iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
                ],
                mouse::Cursor::Available(point),
                &mut renderer,
                &mut clipboard::Null,
                &mut messages,
            );
            assert!(
                messages.iter().any(
                    |m| matches!(m, Message::DiscoverPlay { revision: 3, index: i } if *i == index)
                ),
                "song {index} is playable"
            );
        }
    }
    #[test]
    fn backdrop_follows_only_the_first_cover_and_discards_previous_refreshes() {
        let mut pane = fixture();
        let first = pane.picks()[0].track.id.clone();
        let second = pane.picks()[1].track.id.clone();
        let cover = || image::Handle::from_rgba(4, 4, vec![120; 4 * 4 * 4]);
        pane.cover(3, &second, Ok(Some(cover())));
        assert!(pane.backdrop().is_none());
        pane.cover(3, &first, Ok(Some(cover())));
        let backdrop = pane.backdrop().expect("first cover colors").clone();
        pane.cover(3, &second, Ok(Some(cover())));
        assert_eq!(pane.backdrop(), Some(&backdrop));
        let _ = pane.refresh(Arc::new(Youtube::default()));
        assert!(pane.backdrop().is_none());
        pane.cover(3, &first, Ok(Some(cover())));
        assert!(pane.backdrop().is_none());
    }
    #[test]
    fn cover_tint_follows_navigation_across_the_app() -> anyhow::Result<()> {
        use super::super::{App, Screen};
        let mut app = App::new(
            iced::Theme::Dark,
            capsule::audio::Player::new()?,
            std::iter::empty(),
        );
        app.discover = fixture();
        let id = app.discover.picks()[0].track.id.clone();
        app.discover.cover(
            3,
            &id,
            Ok(Some(image::Handle::from_rgba(
                1,
                1,
                vec![40, 120, 180, 255],
            ))),
        );
        let tint = app.backdrop().expect("cover tint").clone();
        for screen in [
            Screen::Discover,
            Screen::Library,
            Screen::Playlists,
            Screen::Audio,
            Screen::Settings,
        ] {
            app.screen = screen;
            assert_eq!(app.backdrop(), Some(&tint));
        }
        Ok(())
    }
    #[test]
    fn only_loaded_discover_songs_can_be_activated() {
        use iced_runtime::{
            UserInterface,
            core::{clipboard, mouse, renderer::Headless},
        };
        let mut renderer = iced::futures::executor::block_on(iced::Renderer::new(
            iced::Font::DEFAULT,
            iced::Pixels(13.),
            Some("tiny-skia"),
        ))
        .expect("test renderer");
        for (state, playable) in [
            (State::Loading, false),
            (fixture().state, true),
            (State::Failed(String::from("Offline")), false),
        ] {
            let pane = Pane {
                state,
                revision: 3,
                ..Default::default()
            };
            let mut ui = UserInterface::build(
                pane.view(860.),
                iced::Size::new(860., 700.),
                Default::default(),
                &mut renderer,
            );
            let mut messages = Vec::new();
            let _ = ui.update(
                &[
                    iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                    iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
                ],
                mouse::Cursor::Available(iced::Point::new(100., 150.)),
                &mut renderer,
                &mut clipboard::Null,
                &mut messages,
            );
            assert_eq!(
                messages
                    .iter()
                    .any(|message| matches!(message, Message::DiscoverPlay { .. })),
                playable
            );
        }
    }
}
