use super::library::background;
use capsule::library::{
    LibraryError,
    playlists::{Contents, Edit, Playlist, PlaylistId, SavedTrack, Store},
};
use iced::widget::image::Handle;
use iced::{
    Alignment, Element, Length, Task,
    widget::{column, row, scrollable, space::horizontal as horizontal_space, text, text_input},
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[derive(Default)]
pub(super) enum Mode {
    #[default]
    Browse,
    SaveQueue(Arc<[SavedTrack]>),
    AddTrack(SavedTrack),
}

#[derive(Debug, Clone)]
pub(super) enum Msg {
    Listed(Result<(Store, Vec<Playlist>), String>),
    List,
    Retry,
    Name(String),
    Create,
    Select(PlaylistId),
    TrackMenu(usize, iced::Point),
    Edit(Edit),
    Loaded(Result<Arc<Contents>, String>),
    Play(usize),
    Cover(
        Arc<Contents>,
        super::artwork::Key,
        Result<Option<Handle>, String>,
    ),
}

#[derive(Default)]
pub(super) struct Pane {
    db: Option<Store>,
    lists: Vec<Playlist>,
    contents: Option<Arc<Contents>>,
    covers: HashMap<super::artwork::Key, Option<Handle>>,
    mode: Mode,
    name: String,
    error: String,
    busy: bool,
}

impl Pane {
    pub(super) fn busy(&self) -> bool {
        self.busy
    }
    pub(super) fn contents(&self) -> Option<&Contents> {
        self.contents.as_deref()
    }

    pub(super) fn open(&mut self, mode: Mode) -> Task<Msg> {
        self.mode = mode;
        self.name.clear();
        self.contents = None;
        self.covers.clear();
        self.list()
    }

    fn list(&mut self) -> Task<Msg> {
        self.busy = true;
        self.error.clear();
        let db = self.db.clone();
        Task::perform(
            background(move || {
                let db = match db {
                    Some(db) => db,
                    None => Store::local()?,
                };
                let lists = db.list()?;
                Ok((db, lists))
            }),
            Msg::Listed,
        )
    }

    fn load(
        &mut self,
        job: impl FnOnce(Store) -> Result<Contents, LibraryError> + Send + 'static,
    ) -> Task<Msg> {
        let Some(db) = self.db.clone() else {
            return Task::none();
        };
        self.busy = true;
        self.error.clear();
        Task::perform(background(move || job(db).map(Arc::new)), Msg::Loaded)
    }

    pub(super) fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Listed(result) => {
                self.busy = false;
                match result {
                    Ok((db, lists)) => {
                        self.db = Some(db);
                        self.lists = lists;
                    }
                    Err(e) => self.error = e,
                }
            }
            Msg::Loaded(result) => {
                self.busy = false;
                match result {
                    Ok(contents) => {
                        self.contents = Some(contents.clone());
                        self.mode = Mode::Browse;
                        self.name.clear();
                        let jobs: HashMap<_, _> = contents
                            .entries
                            .iter()
                            .filter_map(|entry| match &entry.track {
                                SavedTrack::File(track) => {
                                    Some((super::artwork::Key::File(track.path.clone()), None))
                                }
                                SavedTrack::YouTube(track) => Some((
                                    super::artwork::Key::YouTube(track.id.clone()),
                                    track.thumbnail.clone(),
                                )),
                                SavedTrack::StereoCheck => None,
                            })
                            .collect();
                        let keys: HashSet<_> = jobs.keys().collect();
                        self.covers.retain(|key, _| keys.contains(key));
                        let slots = Arc::new(tokio::sync::Semaphore::new(4));
                        let tasks = jobs
                            .into_iter()
                            .filter(|(key, _)| !self.covers.contains_key(key))
                            .map(|(key, url)| {
                                let contents = contents.clone();
                                let slots = slots.clone();
                                Task::perform(
                                    async move {
                                        let job_key = key.clone();
                                        let cover = match slots.acquire_owned().await {
                                            Ok(_permit) => {
                                                tokio::task::spawn_blocking(move || match job_key {
                                                    super::artwork::Key::File(path) => {
                                                        Ok(super::artwork::cover(&path, 64))
                                                    }
                                                    super::artwork::Key::YouTube(_) => url
                                                        .as_deref()
                                                        .map(|url| {
                                                            super::artwork::remote_cover(url, 64)
                                                        })
                                                        .transpose()
                                                        .map(Option::flatten),
                                                })
                                                .await
                                                .map_err(|e| e.to_string())
                                                .and_then(|r| r)
                                            }
                                            Err(e) => Err(e.to_string()),
                                        };
                                        (contents, key, cover)
                                    },
                                    |(contents, key, cover)| Msg::Cover(contents, key, cover),
                                )
                            })
                            .collect::<Vec<_>>();
                        return Task::batch(tasks);
                    }
                    Err(e) => self.error = e,
                }
            }
            Msg::Cover(contents, key, cover) => {
                if self
                    .contents
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, &contents))
                {
                    match cover {
                        Ok(cover) => {
                            self.covers.insert(key, cover);
                        }
                        Err(error) => self.error = error,
                    }
                }
            }
            _ if self.busy => {}
            Msg::Retry => return self.list(),
            Msg::List => return self.open(Mode::Browse),
            Msg::Name(s) => self.name = s,
            Msg::Create => {
                let tracks = match &self.mode {
                    Mode::SaveQueue(items) => Arc::clone(items),
                    Mode::AddTrack(track) => Arc::from([track.clone()]),
                    Mode::Browse => Arc::from([]),
                };
                let name = self.name.clone();
                return self.load(move |db| db.create(&name, &tracks));
            }
            Msg::Select(id) => {
                if let Mode::AddTrack(track) = &self.mode {
                    let track = track.clone();
                    return self.load(move |db| db.add(id, &track));
                }
                return self.load(move |db| db.contents(id));
            }
            Msg::Edit(edit) => {
                if let Some(contents) = &self.contents {
                    let id = contents.playlist.id;
                    return self.load(move |db| db.edit(id, edit));
                }
            }
            Msg::TrackMenu(..) | Msg::Play(_) => {}
        }
        Task::none()
    }

    pub(super) fn view<'a>(&'a self, fallback: &'a Handle) -> Element<'a, Msg> {
        let ready = !self.busy;
        let mut col = column![].spacing(9);
        if !self.error.is_empty() {
            col = col.push(text(&self.error));
        }
        if let Some(contents) = &self.contents {
            col = col
                .push(
                    super::material::button("All playlists")
                        .style(super::material::text_button)
                        .on_press_maybe(ready.then_some(Msg::List)),
                )
                .push(
                    row![
                        text(&contents.playlist.name).size(22),
                        super::dock::control(
                            super::dock::icon!("play"),
                            "Play playlist",
                            (ready && !contents.entries.is_empty()).then_some(Msg::Play(0)),
                            true,
                            false
                        )
                    ]
                    .align_y(Alignment::Center)
                    .spacing(10),
                );
            if contents.entries.is_empty() {
                col = col.push(text("Add tracks from an album in your library."));
            }
            let mut rows = column![].spacing(6);
            for (i, entry) in contents.entries.iter().enumerate() {
                let mut label = column![text(entry.track.title())].width(Length::Fill);
                let artist = match &entry.track {
                    SavedTrack::File(t) => t.artist.as_str(),
                    SavedTrack::YouTube(t) => t.artist.as_str(),
                    SavedTrack::StereoCheck => "",
                };
                if !artist.is_empty() {
                    label = label.push(text(artist).size(13));
                }
                let cover = match &entry.track {
                    SavedTrack::File(track) => self
                        .covers
                        .get(&super::artwork::Key::File(track.path.clone()))
                        .and_then(Option::as_ref),
                    SavedTrack::YouTube(track) => self
                        .covers
                        .get(&super::artwork::Key::YouTube(track.id.clone()))
                        .and_then(Option::as_ref),
                    _ => None,
                }
                .unwrap_or(fallback);
                let row = row![
                    iced::widget::image(cover)
                        .height(36)
                        .width(36)
                        .border_radius(super::material::RADIUS),
                    label
                ]
                .align_y(Alignment::Center)
                .spacing(10);
                rows = if ready {
                    rows.push(super::context::row(
                        iced::widget::mouse_area(row).on_double_click(Msg::Play(i)),
                        move |p| Msg::TrackMenu(i, p),
                    ))
                } else {
                    rows.push(row)
                };
            }
            col = col.push(scrollable(rows.push(iced::widget::Space::new().height(96))));
        } else if self.busy {
            col = col.push(text("Loading playlists…"));
        } else if self.db.is_none() {
            col = col.push(super::material::button("Retry").on_press(Msg::Retry));
        } else {
            let action = match &self.mode {
                Mode::SaveQueue(tracks) => {
                    col = col.push(text(format!("Save queue ({} tracks)", tracks.len())).size(17));
                    "Save queue"
                }
                Mode::AddTrack(track) => {
                    col = col.push(text(format!("Add “{}” to a playlist", track.title())).size(17));
                    "Create and add"
                }
                Mode::Browse => {
                    col = col.push(text("Playlists").size(23).font(super::material::HEADING));
                    "Create playlist"
                }
            };
            let create = (!self.name.trim().is_empty()).then_some(Msg::Create);
            col = col.push(
                row![
                    text_input("Playlist name", &self.name)
                        .style(super::material::input)
                        .on_input(Msg::Name)
                        .on_submit_maybe(create.clone()),
                    super::material::button(action).on_press_maybe(create)
                ]
                .spacing(8),
            );
            if !matches!(self.mode, Mode::SaveQueue(_)) {
                if self.lists.is_empty() {
                    col = col.push(text("No saved playlists."));
                }
                let mut lists = column![].spacing(4);
                for playlist in &self.lists {
                    lists = lists.push(
                        super::material::button(row![text(&playlist.name), horizontal_space()])
                            .style(super::dock::soft_button)
                            .width(Length::Fill)
                            .on_press(Msg::Select(playlist.id)),
                    );
                }
                col = col.push(scrollable(
                    lists.push(iced::widget::Space::new().height(96)),
                ));
            }
        }
        col.into()
    }
}
