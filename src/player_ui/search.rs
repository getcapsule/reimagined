use super::{App, Message, QueueItem, context, dock, library::background, material};
use capsule::library::{Library, playlists::SavedTrack};
use iced::{
    Element, Fill, Subscription, Task,
    keyboard::{self, Key, key::Named},
    widget::{column, operation, row, scrollable, text, text_input},
};
use std::sync::Arc;

type Hits = Arc<[(SavedTrack, String)]>;
pub(super) const INPUT: &str = "global-search-input";
const LIST: &str = "global-search-results";

#[derive(Debug, Clone)]
pub(super) enum Msg {
    Open,
    Close,
    Query(String),
    Debounced(u64),
    Scope(Scope),
    Move(bool),
    Submit,
    Play { revision: u64, index: usize },
    Command(Command),
    Results(u64, Result<Hits, String>),
}

#[derive(Debug, Clone, Copy)]
pub(super) enum Command {
    Library,
    Playlists,
    OpenFiles,
    Settings,
}
impl Command {
    fn label(self) -> &'static str {
        match self {
            Self::Library => "Go to library",
            Self::Playlists => "Go to playlists",
            Self::OpenFiles => "Open files",
            Self::Settings => "Open settings",
        }
    }
    fn action(self) -> Message {
        match self {
            Self::Library => Message::ShowLibrary,
            Self::Playlists => Message::ShowPlaylists,
            Self::OpenFiles => Message::Browse,
            Self::Settings => Message::ShowSettings,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Scope {
    #[default]
    YouTube,
    Local,
}

#[derive(Default, PartialEq, Eq)]
enum Lookup {
    #[default]
    Idle,
    Searching,
    SubmitOnCompletion,
    Debouncing,
    SubmitOnDebounce,
}

#[derive(Default)]
pub(super) struct Pane {
    pub(super) open: bool,
    query: String,
    scope: Scope,
    hits: Hits,
    selected: usize,
    revision: u64,
    lookup_state: Lookup,
    error: Option<String>,
}

pub(super) fn keys(search_active: bool) -> Subscription<Message> {
    let global = iced::event::listen_with(|event, _, _| {
        if let iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Character(key),
            modifiers,
            repeat: false,
            ..
        }) = event
            && modifiers == keyboard::Modifiers::CTRL
            && key.eq_ignore_ascii_case("k")
        {
            Some(Message::Search(Msg::Open))
        } else {
            None
        }
    });
    if !search_active {
        return global;
    }
    let navigation = iced::event::listen_with(|event, _, _| {
        let iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Named(key),
            ..
        }) = event
        else {
            return None;
        };
        Some(Message::Search(match key {
            Named::Escape => Msg::Close,
            Named::ArrowDown => Msg::Move(true),
            Named::ArrowUp => Msg::Move(false),
            _ => return None,
        }))
    });
    Subscription::batch([global, navigation])
}

impl Pane {
    fn lookup(
        &mut self,
        db: Option<&Library>,
        youtube: Arc<capsule::providers::youtube::Youtube>,
    ) -> Task<Message> {
        if self.lookup_state != Lookup::Idle || self.query.trim().is_empty() {
            return Task::none();
        }
        self.lookup_state = Lookup::Searching;
        let db = db.cloned();
        let query = self.query.clone();
        let revision = self.revision;
        let scope = self.scope;
        Task::perform(
            async move {
                match scope {
                    Scope::YouTube => youtube
                        .search(&query, 15)
                        .await
                        .map(|tracks| {
                            Arc::from(
                                tracks
                                    .into_iter()
                                    .map(|t| (SavedTrack::YouTube(t), String::from("YouTube")))
                                    .collect::<Vec<_>>(),
                            )
                        })
                        .map_err(|e| e.to_string()),
                    Scope::Local => {
                        background(move || {
                            let db = match db {
                                Some(db) => db,
                                None => Library::local()?,
                            };
                            db.search_tracks(&query).map(|tracks| {
                                Arc::from(
                                    tracks
                                        .into_iter()
                                        .map(|(t, a)| (SavedTrack::File(t), a))
                                        .collect::<Vec<_>>(),
                                )
                            })
                        })
                        .await
                    }
                }
            },
            move |result| Message::Search(Msg::Results(revision, result)),
        )
    }

    fn commands(&self) -> impl Iterator<Item = Command> {
        let term = self.query.trim().to_lowercase();
        [
            Command::Library,
            Command::Playlists,
            Command::OpenFiles,
            Command::Settings,
        ]
        .into_iter()
        .filter(move |cmd| cmd.label().to_lowercase().contains(&term))
    }

    pub(super) fn view(
        &self,
        height: f32,
        appearance: material::Appearance,
    ) -> Element<'_, Message> {
        use iced::{
            Alignment, Border, Color,
            widget::{container, rule::horizontal},
        };
        let input = text_input("Search music or run a command", &self.query)
            .id(INPUT)
            .on_input(|s| Message::Search(Msg::Query(s)))
            .on_submit(Message::Search(Msg::Submit))
            .size(16)
            .padding([9, 3])
            .style(|theme, status| {
                let mut field = text_input::default(theme, status);
                field.border = Border::default();
                field.placeholder = Color::from_rgb8(165, 165, 173);
                field.value = Color::WHITE;
                field.background = Color::TRANSPARENT.into();
                field
            });
        let header = row![
            dock::icon!("search"),
            input,
            super::material::button(text("esc").size(11))
                .style(dock::soft_button)
                .on_press(Message::Search(Msg::Close))
        ]
        .align_y(Alignment::Center)
        .spacing(9);
        let sources = row![
            super::material::button("YouTube")
                .style(if self.scope == Scope::YouTube {
                    dock::soft_button
                } else {
                    super::material::text_button
                })
                .on_press(Message::Search(Msg::Scope(Scope::YouTube))),
            super::material::button("Local library")
                .style(if self.scope == Scope::Local {
                    dock::soft_button
                } else {
                    super::material::text_button
                })
                .on_press(Message::Search(Msg::Scope(Scope::Local)))
        ]
        .spacing(5);
        let mut page = column![header, sources, horizontal(1)].spacing(7);
        let mut rows = column![].spacing(2);
        let commands: Vec<_> = self.commands().collect();
        if let Some(error) = &self.error {
            page = page.push(text(error));
        } else if !self.query.trim().is_empty()
            && self.hits.is_empty()
            && self.lookup_state != Lookup::Idle
        {
            rows = rows.push(super::skeleton::rows(5, false));
        } else if !self.query.trim().is_empty() && self.hits.is_empty() && commands.is_empty() {
            page = page.push(text("No matching tracks.").size(13));
        } else if !self.hits.is_empty() {
            rows = rows.push(
                text("Tracks")
                    .size(11)
                    .color(Color::from_rgb8(176, 176, 183)),
            );
            for (i, (track, album)) in self.hits.iter().enumerate() {
                let artist = match track {
                    SavedTrack::File(t) => t.artist.as_str(),
                    SavedTrack::YouTube(t) => t.artist.as_str(),
                    SavedTrack::StereoCheck => "",
                };
                let label = [artist, album.as_str()]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · ");
                let item = super::material::button(column![
                    text(track.title()).size(14),
                    text(label).size(12)
                ])
                .width(Fill)
                .padding([5, 8])
                .style(if self.selected == i {
                    dock::soft_button
                } else {
                    super::material::text_button
                })
                .on_press(Message::Search(Msg::Play {
                    revision: self.revision,
                    index: i,
                }));
                rows = rows.push(context::row(item, move |point| {
                    Message::TrackMenu(track.clone(), point)
                }));
            }
        }
        if !commands.is_empty() {
            rows = rows.push(
                text("Commands")
                    .size(11)
                    .color(Color::from_rgb8(176, 176, 183)),
            );
            for (i, command) in commands.into_iter().enumerate() {
                let icon = match command {
                    Command::Library => dock::icon!("library"),
                    Command::Playlists => dock::icon!("playlists"),
                    Command::OpenFiles => dock::icon!("folder"),
                    Command::Settings => dock::icon!("settings"),
                };
                rows = rows.push(
                    super::material::button(
                        row![
                            icon,
                            text(command.label())
                                .size(14)
                                .color(Color::from_rgb8(233, 233, 236))
                        ]
                        .align_y(Alignment::Center)
                        .spacing(9),
                    )
                    .padding([6, 8])
                    .width(Fill)
                    .style(if self.selected == self.hits.len() + i {
                        dock::soft_button
                    } else {
                        super::material::text_button
                    })
                    .on_press(Message::Search(Msg::Command(command))),
                );
            }
        }
        page = page.push(
            container(scrollable(rows).id(LIST)).max_height((height * 0.55).clamp(95., 330.)),
        );
        container(material::floating(appearance, page, 12, Fill, iced::Shrink))
            .width(Fill)
            .max_width(520)
            .into()
    }
}

impl App {
    pub(super) fn search_action(&mut self, msg: Msg) -> Task<Message> {
        let active = self.search.open;
        match msg {
            Msg::Open => {
                if !active {
                    self.search.query.clear();
                    self.search.hits = Arc::default();
                    self.search.selected = 0;
                    if self.search.lookup_state == Lookup::SubmitOnCompletion {
                        self.search.lookup_state = Lookup::Searching;
                    }
                    self.search.error = None;
                    self.search.revision += 1;
                }
                self.menu = None;
                self.search.open = !active;
                if active {
                    return Task::none();
                }
                return operation::focus(INPUT);
            }
            Msg::Results(revision, result) => {
                let submit =
                    std::mem::take(&mut self.search.lookup_state) == Lookup::SubmitOnCompletion;
                if !active {
                    return Task::none();
                }
                if revision != self.search.revision {
                    let task = self
                        .search
                        .lookup(self.library.database(), self.youtube.clone());
                    if submit && self.search.lookup_state == Lookup::Searching {
                        self.search.lookup_state = Lookup::SubmitOnCompletion;
                    }
                    return task;
                }
                match result {
                    Ok(hits) => self.search.hits = hits,
                    Err(e) => self.search.error = Some(e),
                }
                if submit {
                    return self.search_action(Msg::Submit);
                }
            }
            _ if !active => {}
            Msg::Close => self.search.open = false,
            Msg::Scope(scope) => {
                self.search.scope = scope;
                return self.search_action(Msg::Query(self.search.query.clone()));
            }
            Msg::Debounced(revision) => {
                if revision == self.search.revision
                    && matches!(
                        self.search.lookup_state,
                        Lookup::Debouncing | Lookup::SubmitOnDebounce
                    )
                {
                    let submit = self.search.lookup_state == Lookup::SubmitOnDebounce;
                    self.search.lookup_state = Lookup::Idle;
                    let task = self
                        .search
                        .lookup(self.library.database(), self.youtube.clone());
                    if submit && self.search.lookup_state == Lookup::Searching {
                        self.search.lookup_state = Lookup::SubmitOnCompletion;
                    }
                    return task;
                }
            }
            Msg::Query(query) => {
                self.search.query = query;
                self.search.revision += 1;
                self.search.selected = 0;
                self.search.error = None;
                self.search.hits = Arc::default();
                self.search.lookup_state = match self.search.lookup_state {
                    Lookup::Searching | Lookup::SubmitOnCompletion => Lookup::Searching,
                    _ => Lookup::Debouncing,
                };
                let revision = self.search.revision;
                return Task::perform(
                    async move {
                        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                        revision
                    },
                    |revision| Message::Search(Msg::Debounced(revision)),
                );
            }
            Msg::Move(down) => {
                let Some(last) =
                    (self.search.hits.len() + self.search.commands().count()).checked_sub(1)
                else {
                    return Task::none();
                };
                self.search.selected = if down {
                    (self.search.selected + 1).min(last)
                } else {
                    self.search.selected.saturating_sub(1)
                };
                return operation::snap_to(
                    LIST,
                    scrollable::RelativeOffset {
                        x: 0.,
                        y: self.search.selected as f32 / last.max(1) as f32,
                    },
                );
            }
            Msg::Command(command) => {
                self.search.open = false;
                return self.handle(command.action());
            }
            Msg::Submit => {
                if self.search.selected < self.search.hits.len() {
                    return self.search_action(Msg::Play {
                        revision: self.search.revision,
                        index: self.search.selected,
                    });
                }
                let command = self
                    .search
                    .commands()
                    .nth(self.search.selected.saturating_sub(self.search.hits.len()));
                if (self.search.lookup_state == Lookup::Idle || self.search.query.trim().is_empty())
                    && let Some(command) = command
                {
                    return self.search_action(Msg::Command(command));
                }
                if !self.search.query.trim().is_empty() {
                    self.search.lookup_state = match self.search.lookup_state {
                        Lookup::Searching => Lookup::SubmitOnCompletion,
                        Lookup::Debouncing => Lookup::SubmitOnDebounce,
                        Lookup::SubmitOnCompletion => Lookup::SubmitOnCompletion,
                        Lookup::SubmitOnDebounce => Lookup::SubmitOnDebounce,
                        Lookup::Idle => Lookup::Idle,
                    };
                }
            }
            Msg::Play { revision, index } => {
                if revision == self.search.revision
                    && let Some((track, _)) = self.search.hits.get(index)
                {
                    self.search.open = false;
                    self.load([QueueItem::from_saved(track)]);
                }
            }
        }
        Task::none()
    }
}
