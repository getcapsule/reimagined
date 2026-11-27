use super::{background as page_background, material};
use capsule::library::{Album, AlbumPage, Library, LibraryError, ScanStats, Track};
use iced::{
    Alignment, Element, Length, Task,
    widget::{
        column, container, image, row, rule::horizontal as horizontal_rule, scrollable,
        space::horizontal as horizontal_space, text, text_input,
    },
};
use std::{path::PathBuf, sync::Arc};

#[derive(Debug, Clone)]
pub(super) enum Msg {
    Open,
    Opened(Result<Library, String>),
    Choose,
    Folder(Option<PathBuf>),
    Rescan,
    Scanned(Result<ScanStats, String>),
    Query(String),
    Page(usize),
    Results(u64, Result<AlbumPage, String>),
    Album(Album),
    Albums,
    AllAlbums,
    Artist(String),
    Art(u64, Result<Option<page_background::PageArt>, String>),
    Tracks(u64, Result<Arc<[Track]>, String>),
    Play,
    TrackMenu(usize, iced::Point),
}

#[derive(Default)]
pub(super) struct Pane {
    db: Option<Library>,
    opening: bool,
    scanning: bool,
    searching: bool,
    revision: u64,
    query: String,
    page: usize,
    results: Option<AlbumPage>,
    selected: Option<Album>,
    artist: Option<String>,
    art: Option<page_background::PageArt>,
    tracks: Option<Arc<[Track]>>,
    selection_id: u64,
    notice: String,
}

pub(super) async fn background<T: Send + 'static>(
    job: impl FnOnce() -> Result<T, LibraryError> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(job)
        .await
        .map_err(|err| err.to_string())?
        .map_err(|err| err.to_string())
}

impl Pane {
    pub(super) fn database(&self) -> Option<&Library> {
        self.db.as_ref()
    }

    pub(super) fn albums(&self) -> &[Album] {
        self.results.as_ref().map_or(&[], |page| &page.albums)
    }

    pub(super) fn tracks(&self) -> Option<&[Track]> {
        self.tracks.as_deref()
    }

    fn refresh(&mut self) -> Task<Msg> {
        self.revision += 1;
        if self.searching {
            return Task::none();
        }
        let Some(db) = self.db.clone() else {
            return Task::none();
        };
        let query = self.query.clone();
        let artist = self.artist.clone();
        let page = self.page;
        let revision = self.revision;
        self.searching = true;
        Task::perform(
            background(move || match artist {
                Some(artist) => db.artist_albums(&artist, page),
                None => db.search(&query, page),
            }),
            move |result| Msg::Results(revision, result),
        )
    }

    fn scan(&mut self, folder: Option<PathBuf>) -> Task<Msg> {
        let Some(db) = self.db.clone() else {
            return Task::none();
        };
        if self.scanning {
            return Task::none();
        }
        self.scanning = true;
        self.notice.clear();
        Task::perform(background(move || db.scan(folder.as_deref())), Msg::Scanned)
    }

    pub(super) fn update(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Open if self.db.is_none() && !self.opening => {
                self.opening = true;
                return Task::perform(background(Library::local), Msg::Opened);
            }
            Msg::Opened(result) => {
                self.opening = false;
                match result {
                    Ok(db) => {
                        self.db = Some(db);
                        self.notice.clear();
                        return self.refresh();
                    }
                    Err(error) => self.notice = error,
                }
            }
            Msg::Choose => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .pick_folder()
                            .await
                            .map(|f| f.path().to_owned())
                    },
                    Msg::Folder,
                );
            }
            Msg::Folder(Some(folder)) => return self.scan(Some(folder)),
            Msg::Rescan => return self.scan(None),
            Msg::Scanned(result) => {
                self.scanning = false;
                match result {
                    Ok(stats) => {
                        self.notice = format!(
                            "{} indexed · {} unchanged · {} skipped",
                            stats.indexed, stats.unchanged, stats.skipped
                        );
                        self.selected = None;
                        self.artist = None;
                        self.art = None;
                        self.tracks = None;
                        self.selection_id += 1;
                        self.page = 0;
                        return self.refresh();
                    }
                    Err(error) => self.notice = error,
                }
            }
            Msg::Query(query) => {
                self.query = query;
                self.page = 0;
                return self.refresh();
            }
            Msg::Page(page) => {
                self.page = page;
                return self.refresh();
            }
            Msg::Results(revision, result) => {
                self.searching = false;
                if revision != self.revision {
                    return self.refresh();
                }
                match result {
                    Ok(page) => self.results = Some(page),
                    Err(error) => {
                        self.results = None;
                        self.notice = error;
                    }
                }
            }
            Msg::Album(album) => {
                let Some(db) = self.db.clone() else {
                    return Task::none();
                };
                self.selected = Some(album.clone());
                self.art = None;
                self.tracks = None;
                self.selection_id += 1;
                let id = self.selection_id;
                return Task::perform(
                    background(move || db.tracks(&album).map(Arc::from)),
                    move |r| Msg::Tracks(id, r),
                );
            }
            Msg::Tracks(id, result) if id == self.selection_id => match result {
                Ok(tracks) => {
                    let path = tracks.first().map(|track| track.path.clone());
                    self.tracks = Some(tracks);
                    if let Some(path) = path {
                        return Task::perform(
                            background(move || Ok(page_background::album(&path))),
                            move |result| Msg::Art(id, result),
                        );
                    }
                }
                Err(error) => {
                    self.selected = None;
                    self.notice = error;
                }
            },
            Msg::Artist(name) => {
                let origin = self
                    .selected
                    .as_ref()
                    .or_else(|| self.albums().iter().find(|a| a.artist == name))
                    .cloned();
                self.artist = Some(name);
                self.selected = None;
                self.art = None;
                self.tracks = None;
                self.page = 0;
                self.results = None;
                self.selection_id += 1;
                let id = self.selection_id;
                let art = if let (Some(db), Some(album)) = (self.db.clone(), origin) {
                    Task::perform(
                        background(move || {
                            let tracks = db.tracks(&album)?;
                            Ok(tracks
                                .first()
                                .and_then(|track| page_background::artist(&track.path)))
                        }),
                        move |result| Msg::Art(id, result),
                    )
                } else {
                    Task::none()
                };
                return Task::batch([art, self.refresh()]);
            }
            Msg::Art(id, result) if id == self.selection_id => match result {
                Ok(art) => self.art = art,
                Err(error) => self.notice = error,
            },
            Msg::AllAlbums => {
                self.artist = None;
                self.selected = None;
                self.tracks = None;
                self.art = None;
                self.selection_id += 1;
                self.page = 0;
                return self.refresh();
            }
            Msg::Albums => {
                if let Some(artist) = &self.artist {
                    return self.update(Msg::Artist(artist.clone()));
                }
                self.selected = None;
                self.art = None;
                self.tracks = None;
                self.selection_id += 1;
            }
            _ => {}
        }
        Task::none()
    }

    pub(super) fn backdrop(&self) -> Option<&image::Handle> {
        self.art.as_ref().map(|art| &art.colors)
    }
    pub(super) fn view(&self, appearance: material::Appearance) -> Element<'_, Msg> {
        let mut content = column![].spacing(9);
        if let Some(album) = &self.selected {
            let info = column![
                super::material::button(if self.artist.is_some() {
                    "Artist"
                } else {
                    "All albums"
                })
                .on_press(Msg::Albums)
                .style(super::material::text_button),
                text(&album.title).size(26).font(material::HEADING),
                super::material::button(text(&album.artist).size(15))
                    .padding(0)
                    .style(super::material::text_button)
                    .on_press(Msg::Artist(album.artist.clone())),
            ]
            .spacing(8);
            let mut header = row![].spacing(20).align_y(Alignment::Center);
            if let Some(art) = &self.art {
                header = header.push(
                    image(&art.cover)
                        .width(140)
                        .height(140)
                        .border_radius(material::RADIUS)
                        .content_fit(iced::ContentFit::Cover),
                );
            }
            content = content.push(header.push(info));
            if let Some(tracks) = &self.tracks {
                content = content.push(
                    super::material::button("Play album")
                        .on_press_maybe((!tracks.is_empty()).then_some(Msg::Play)),
                );
                let list = tracks
                    .iter()
                    .enumerate()
                    .fold(column![].spacing(8), |list, (i, t)| {
                        list.push(super::context::row(
                            row![
                                text((i + 1).to_string()).width(40),
                                column![text(t.title.as_str()), text(t.artist.as_str()).size(13)]
                                    .width(Length::Fill)
                            ],
                            move |point| Msg::TrackMenu(i, point),
                        ))
                    });
                content =
                    content.push(scrollable(list.push(iced::widget::Space::new().height(96))));
            } else {
                content = content.push(super::skeleton::rows(6, false));
            }
        } else {
            if let Some(artist) = &self.artist {
                content = content.push(
                    super::material::button("All albums")
                        .style(super::material::text_button)
                        .on_press(Msg::AllAlbums),
                );
                let mut header = row![].spacing(20).align_y(Alignment::Center);
                if let Some(art) = &self.art {
                    header =
                        header.push(image(&art.cover).width(112).height(112).border_radius(56));
                }
                content = content
                    .push(header.push(text(artist).size(28).font(material::HEADING)))
                    .push(text("Albums").size(17).font(material::HEADING));
            } else {
                content = content.push(text("Library").size(23).font(super::material::HEADING));
            }
            let root = self.results.as_ref().and_then(|p| p.folder.as_deref());
            let ready = self.db.is_some() && !self.scanning;
            if self.artist.is_none() {
                content = content.push(
                    row![
                        super::material::button(if root.is_some() {
                            "Change folder"
                        } else {
                            "Choose music folder"
                        })
                        .on_press_maybe(ready.then_some(Msg::Choose)),
                        super::material::button("Rescan")
                            .style(super::dock::soft_button)
                            .on_press_maybe((ready && root.is_some()).then_some(Msg::Rescan))
                    ]
                    .spacing(8),
                );
                if let (Some(folder), Some(page)) = (root, &self.results) {
                    content = content.push(
                        text(format!(
                            "{} tracks · {}",
                            page.total_tracks,
                            folder.display()
                        ))
                        .size(14),
                    );
                }
            }
            if self.scanning {
                content = content.push(text("Scanning music folder…"));
            }
            if !self.notice.is_empty() {
                content = content.push(text(self.notice.as_str()).size(14));
            }
            if self.db.is_none() && !self.opening {
                content = content.push(super::material::button("Retry").on_press(Msg::Open));
            }
            if root.is_some() {
                if self.artist.is_none() {
                    content = content.push(material::floating(
                        appearance,
                        text_input("Search albums, artists, or tracks", &self.query)
                            .on_input(Msg::Query)
                            .style(material::search_field),
                        [6, 10],
                        Length::Fill,
                        Length::Shrink,
                    ));
                }
                content = content.push(horizontal_rule(1));
                if let Some(page) = &self.results {
                    if page.albums.is_empty() {
                        content = content.push(text(if page.total_tracks == 0 {
                            "No audio files indexed."
                        } else {
                            "No matching albums."
                        }));
                    }
                    if page.has_more || self.page > 0 {
                        content = content.push(
                            row![
                                super::material::button("Previous page")
                                    .on_press_maybe(self.page.checked_sub(1).map(Msg::Page)),
                                horizontal_space(),
                                text(format!("Page {}", self.page + 1)),
                                horizontal_space(),
                                super::material::button("Next page").on_press_maybe(
                                    page.has_more.then_some(Msg::Page(self.page + 1))
                                )
                            ]
                            .align_y(Alignment::Center),
                        );
                    }
                    let list = self
                        .albums()
                        .iter()
                        .fold(column![].spacing(3), |list, album| {
                            list.push(
                                super::material::button(column![
                                    text(album.title.as_str()).size(15),
                                    text(album.artist.as_str()).size(13)
                                ])
                                .style(super::material::text_button)
                                .width(Length::Fill)
                                .on_press(Msg::Album(album.clone())),
                            )
                        });
                    content =
                        content.push(scrollable(list.push(iced::widget::Space::new().height(96))));
                }
            }
        }
        if (self.opening || self.searching || self.scanning)
            && self.results.is_none()
            && self.selected.is_none()
        {
            content = content.push(super::skeleton::rows(6, false));
        }
        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::futures::StreamExt;

    async fn drive(pane: &mut Pane, task: Task<Msg>) {
        let mut tasks = vec![task];
        while let Some(task) = tasks.pop() {
            if let Some(mut stream) = iced_runtime::task::into_stream(task) {
                while let Some(iced_runtime::Action::Output(msg)) = stream.next().await {
                    tasks.push(pane.update(msg));
                }
            }
        }
    }

    #[test]
    fn detail_pages_load_art_and_leaving_discards_late_artwork() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let music = temp.path().join("Album");
        std::fs::create_dir(&music)?;
        std::fs::write(
            music.join("Song.wav"),
            include_bytes!("../../assets/stereo-check.wav"),
        )?;
        ::image::RgbaImage::from_pixel(16, 16, ::image::Rgba([200, 40, 20, 255]))
            .save(music.join("cover.png"))?;
        let db = Library::open(&temp.path().join("data"))?;
        db.scan(Some(&music))?;
        let album = db.search("", 0)?.albums.remove(0);
        let runtime = tokio::runtime::Builder::new_current_thread().build()?;
        let mut pane = Pane::default();
        let task = pane.update(Msg::Opened(Ok(db)));
        runtime.block_on(drive(&mut pane, task));
        assert!(pane.backdrop().is_none());
        let task = pane.update(Msg::Album(album.clone()));
        runtime.block_on(drive(&mut pane, task));
        assert!(pane.backdrop().is_some());
        assert_eq!(pane.tracks().unwrap().len(), 1);
        let task = pane.update(Msg::Artist(album.artist));
        runtime.block_on(drive(&mut pane, task));
        assert!(pane.backdrop().is_some());
        assert_eq!(pane.albums().len(), 1);
        let old_id = pane.selection_id;
        let late_art = pane.art.clone();
        let task = pane.update(Msg::AllAlbums);
        runtime.block_on(drive(&mut pane, task));
        let _ = pane.update(Msg::Art(old_id, Ok(late_art)));
        assert!(pane.backdrop().is_none());
        assert!(pane.artist.is_none());
        Ok(())
    }
}
