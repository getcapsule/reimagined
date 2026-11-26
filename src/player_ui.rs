use capsule::audio::{AudioError, Event, OutputDevice, Playback, Player, Source, TrackInfo};
mod artwork;
mod background;
mod context;
mod discover;
mod dock;
#[cfg(windows)]
mod frame;
mod library;
mod material;
mod playlists;
mod search;
use capsule::library::Track;
use capsule::library::playlists::SavedTrack;
use capsule::queue::Queue;
use iced::{Color, Element, Size, Subscription, Task, Theme};
use std::{path::PathBuf, time::Duration};

pub(super) fn run(player: Player, paths: impl Iterator<Item = PathBuf> + 'static) -> iced::Result {
    let theme = Theme::custom(
        String::from("Capsule"),
        iced::theme::Palette {
            background: background::BASE,
            text: Color::from_rgb8(244, 244, 246),
            primary: Color::from_rgb8(213, 214, 218),
            success: Color::from_rgb8(124, 205, 165),
            warning: Color::from_rgb8(224, 181, 113),
            danger: Color::from_rgb8(255, 153, 139),
        },
    );
    let paths: Vec<_> = paths.collect();
    let boot = move || {
        let mut app = App::new(theme.clone(), player.clone(), paths.iter().cloned());
        let task = Task::batch([
            app.sync_art(),
            app.discover.refresh(app.youtube.clone()),
            app.library.update(library::Msg::Open).map(Message::Library),
            #[cfg(windows)]
            frame::install(),
        ]);
        (app, task)
    };
    iced::application(boot, App::update, App::view)
        .title("capsule")
        .settings(iced::Settings {
            default_text_size: iced::Pixels(13.),
            antialiasing: true,
            ..Default::default()
        })
        .font(include_bytes!("../assets/fonts/Manrope.ttf").as_slice())
        .default_font(material::TYPE)
        .theme(|app: &App| app.theme.clone())
        .style(|_, theme| iced::theme::Style {
            background_color: background::BASE,
            text_color: theme.palette().text,
        })
        .window(iced::window::Settings {
            size: WINDOW_SIZE,
            transparent: false,
            decorations: !cfg!(windows),
            icon: image::load_from_memory(include_bytes!("../assets/logo.png"))
                .ok()
                .and_then(|img| {
                    let rgba = img.thumbnail(48, 48).to_rgba8();
                    let (w, h) = rgba.dimensions();
                    iced::window::icon::from_rgba(rgba.into_raw(), w, h).ok()
                }),
            min_size: Some(Size::new(570.0, 445.0)),
            ..Default::default()
        })
        .subscription(App::subscription)
        .run()
}

const SIDEBAR_CONTROL_WIDTH: u32 = 44;
const WINDOW_SIZE: Size = Size::new(1080., 740.);

enum Transport {
    Idle,
    Opening { playback: Playback, playing: bool },
    Playing(Playback),
    Paused(Playback),
    Ended(Duration),
    Failed(String),
}

#[derive(Debug, Clone)]
enum QueueItem {
    Direct(Source),
    Indexed(Track),
}
impl QueueItem {
    fn path(&self) -> Option<&std::path::Path> {
        match self {
            Self::Direct(Source::File(path)) => Some(path),
            Self::Indexed(track) => Some(&track.path),
            Self::Direct(Source::StereoCheck | Source::YouTube(_)) => None,
        }
    }
    fn saved(&self) -> SavedTrack {
        match self {
            Self::Indexed(t) => SavedTrack::File(t.clone()),
            Self::Direct(Source::StereoCheck) => SavedTrack::StereoCheck,
            Self::Direct(Source::YouTube(t)) => SavedTrack::YouTube((**t).clone()),
            Self::Direct(Source::File(path)) => SavedTrack::File(Track {
                path: path.clone(),
                title: self.title().to_owned(),
                artist: String::new(),
            }),
        }
    }
    fn from_saved(track: &SavedTrack) -> Self {
        match track {
            SavedTrack::File(t) => Self::Indexed(t.clone()),
            SavedTrack::StereoCheck => Self::Direct(Source::StereoCheck),
            SavedTrack::YouTube(t) => Self::Direct(Source::YouTube(t.clone().into())),
        }
    }
    fn into_source(self) -> Source {
        match self {
            Self::Direct(s) => s,
            Self::Indexed(t) => Source::File(t.path),
        }
    }
    fn title(&self) -> &str {
        match self {
            Self::Direct(s) => source_name(s),
            Self::Indexed(t) => &t.title,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Settings,
    Discover,
    Audio,
    Player,
    Library,
    Playlists,
}

struct App {
    search: search::Pane,
    youtube: std::sync::Arc<capsule::providers::youtube::Youtube>,
    library: library::Pane,
    playlists: playlists::Pane,
    screen: Screen,
    theme: Theme,
    player: Player,
    queue: Queue<QueueItem>,
    menu: Option<(iced::Point, Vec<(&'static str, Message)>)>,
    request_end: usize,
    request_start: usize,
    skipped: std::collections::BTreeMap<usize, String>,
    artwork: artwork::Artwork,
    appearance: material::Appearance,
    appearance_error: Option<String>,
    logo: iced::widget::image::Handle,
    shuffle_order: Option<Vec<usize>>,
    repeat: bool,
    discover: discover::Pane,
    #[cfg(windows)]
    maximized: bool,
    window_size: Size,
    sidebar_visible: bool,
    queue_visible: bool,
    transport: Transport,
    info: Option<TrackInfo>,
    devices: Vec<OutputDevice>,
    device: OutputDevice,
    volume: u8,
    scrub: Option<f32>,
}

#[derive(Debug, Clone)]
enum Message {
    Search(search::Msg),
    ShowDiscover,
    RefreshDiscover,
    Discover(discover::Msg),
    DiscoverLoaded(u64, Result<std::sync::Arc<[discover::Pick]>, String>),
    DiscoverPlay {
        revision: u64,
        index: usize,
    },
    DiscoverCover(
        u64,
        capsule::providers::youtube::VideoId,
        Result<Option<iced::widget::image::Handle>, String>,
    ),
    #[cfg(windows)]
    Window(frame::Action),
    #[cfg(windows)]
    FrameUnavailable,
    ShowLibrary,
    ShowPlaylists,
    SaveQueue,
    Shuffle,
    Repeat,
    AudioSettings,
    CloseMenu,
    TrackMenu(SavedTrack, iced::Point),
    QueueMenu(usize, iced::Point),
    Enqueue(SavedTrack),
    SaveTrack(SavedTrack),
    ShowSettings,
    Opacity(u8),
    Blur(bool),
    #[cfg(windows)]
    Maximized(bool),
    Resize(Size),
    ToggleSidebar,
    ToggleQueue,
    Artwork(artwork::Key, Result<artwork::Details, String>),
    Playlists(playlists::Msg),
    ShowPlayer,
    Library(library::Msg),
    Browse,
    Picked(Vec<PathBuf>),
    Select(usize),
    Test,
    Toggle,
    Stop,
    Tick,
    Scrub(f32),
    Seek,
    Volume(u8),
    Device(OutputDevice),
}

impl App {
    fn new(theme: Theme, player: Player, paths: impl Iterator<Item = PathBuf>) -> Self {
        let mut app = App {
            theme,
            player,
            search: Default::default(),
            youtube: Default::default(),
            library: library::Pane::default(),
            playlists: playlists::Pane::default(),
            screen: Screen::Discover,
            queue: paths.map(Source::File).map(QueueItem::Direct).collect(),
            menu: None,
            request_end: 0,
            request_start: 0,
            skipped: Default::default(),
            artwork: Default::default(),
            appearance: material::Appearance::load(),
            appearance_error: None,
            logo: iced::widget::image::Handle::from_bytes(
                include_bytes!("../assets/logo.png").as_slice(),
            ),
            shuffle_order: None,
            repeat: false,
            discover: Default::default(),
            #[cfg(windows)]
            maximized: false,
            window_size: WINDOW_SIZE,
            sidebar_visible: true,
            queue_visible: false,
            transport: Transport::Idle,
            info: None,
            device: OutputDevice::System,
            devices: OutputDevice::available(),
            volume: 65,
            scrub: None,
        };
        if app.queue.current().is_some() {
            app.start(Duration::ZERO, true);
        }
        app
    }
    fn load(&mut self, sources: impl IntoIterator<Item = QueueItem>) {
        self.shuffle_order = None;
        self.queue = sources.into_iter().collect();
        self.info = None;
        self.start(Duration::ZERO, true);
    }

    fn start(&mut self, offset: Duration, playing: bool) {
        if self.queue.current().is_none() {
            return;
        }
        self.scrub = None;
        self.request_start = self.queue.index();
        self.request_end = self.queue.entries().len();
        self.skipped.clear();
        self.transport = match self.player.open_queue(
            self.queue.remaining().map(QueueItem::into_source),
            self.device.clone(),
            offset,
            playing,
            self.volume,
        ) {
            Ok(playback) => Transport::Opening { playback, playing },
            Err(e) => Transport::Failed(e.to_string()),
        };
    }

    fn active(&self) -> Option<&Playback> {
        match &self.transport {
            Transport::Playing(p)
            | Transport::Paused(p)
            | Transport::Opening { playback: p, .. } => Some(p),
            _ => None,
        }
    }

    fn position(&self) -> Duration {
        if let Transport::Ended(position) = self.transport {
            return position;
        }
        self.active().map_or(Duration::ZERO, Playback::position)
    }

    fn track_duration(&self, position: Duration) -> Option<Duration> {
        [
            self.info.and_then(|i| i.duration),
            self.artwork.details.as_ref().and_then(|d| d.duration),
        ]
        .into_iter()
        .flatten()
        .find(|d| !d.is_zero() && d.as_secs() >= position.as_secs())
    }

    fn select_available(&mut self, start: usize) {
        if let Some(index) =
            (start..self.queue.entries().len()).find(|i| !self.skipped.contains_key(i))
        {
            self.queue.select(index);
        }
    }

    fn poll(&mut self) {
        while let Some(playback) = self.active() {
            match playback.poll() {
                Ok(Some(Event::Skipped { index, error })) => {
                    self.skipped
                        .insert(self.request_start + index, error.to_string());
                }
                Ok(Some(Event::Ready(info))) => {
                    self.select_available(self.request_start);
                    self.info = Some(info);
                    if let Transport::Opening { playback, playing } =
                        std::mem::replace(&mut self.transport, Transport::Idle)
                    {
                        self.transport = if playing {
                            Transport::Playing(playback)
                        } else {
                            Transport::Paused(playback)
                        };
                    }
                }
                Ok(Some(Event::Advanced { info, .. })) => {
                    self.select_available(self.queue.index() + 1);
                    self.info = Some(info);
                    self.scrub = None;
                }
                Ok(Some(Event::NextFailed(error))) => {
                    self.select_available(self.queue.index() + 1);
                    self.info = None;
                    self.fail(error);
                }
                Ok(Some(Event::Ended)) if self.queue.select(self.request_end) => {
                    self.info = None;
                    self.start(Duration::ZERO, true);
                }
                Ok(Some(Event::Ended)) => {
                    if matches!(self.transport, Transport::Opening { .. }) {
                        self.info = None;
                        let failed_online = self.skipped.keys().rev().copied().find(|&index| {
                            matches!(
                                self.queue.entries().get(index),
                                Some(QueueItem::Direct(Source::YouTube(_)))
                            )
                        });
                        if let Some(error) =
                            failed_online.and_then(|index| self.skipped.remove(&index))
                        {
                            self.transport = Transport::Failed(error);
                            self.scrub = None;
                            continue;
                        }
                    }
                    let position = self.position();
                    if let Some(info) = &mut self.info {
                        info.duration = Some(position);
                    }
                    self.transport = Transport::Ended(position);
                    if self.repeat && self.info.is_some() && !position.is_zero() {
                        self.queue.select(0);
                        self.info = None;
                        self.start(Duration::ZERO, true);
                    }
                    self.scrub = None;
                }
                Ok(Some(Event::Failed(e))) | Err(e) => {
                    if matches!(self.transport, Transport::Opening { .. }) {
                        self.select_available(self.request_start);
                    }
                    self.fail(e);
                }
                Ok(None) => break,
            }
        }
    }

    fn fail(&mut self, error: AudioError) {
        self.transport = Transport::Failed(error.to_string());
        self.scrub = None;
    }

    fn track_menu(&mut self, track: SavedTrack, point: iced::Point) {
        self.menu = Some((
            point,
            vec![
                ("Add to queue", Message::Enqueue(track.clone())),
                ("Add to playlist", Message::SaveTrack(track)),
            ],
        ));
    }

    fn open_playlists(&mut self, mode: playlists::Mode) -> Task<Message> {
        self.screen = Screen::Playlists;
        self.playlists.open(mode).map(Message::Playlists)
    }

    fn sync_art(&mut self) -> Task<Message> {
        self.artwork
            .sync(self.queue.current().map(|item| match item {
                QueueItem::Direct(Source::YouTube(track)) => artwork::Request::YouTube(track),
                _ => artwork::Request::File(item.path()),
            }))
    }

    fn shuffle(&mut self) {
        let len = self.queue.entries().len();
        if len < 2 {
            return;
        }
        let index = self.queue.index();
        let (order, current) = if let Some(original) = self.shuffle_order.take() {
            let mut order = vec![0; len];
            for (pos, &old) in original.iter().enumerate() {
                order[old] = pos;
            }
            (order, original[index])
        } else {
            let mut order: Vec<_> = (0..len).collect();
            fastrand::shuffle(&mut order[index + 1..]);
            self.shuffle_order = Some(order.clone());
            (order, index)
        };
        let offset = self.position();
        self.queue = order
            .iter()
            .map(|&i| self.queue.entries()[i].clone())
            .collect();
        self.queue.select(current);
        if self.active().is_some() {
            let playing = !matches!(
                self.transport,
                Transport::Paused(_) | Transport::Opening { playing: false, .. }
            );
            self.start(offset, playing);
        } else {
            self.skipped.clear();
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        let action = self.handle(message);
        Task::batch([action, self.sync_art()])
    }

    fn set_appearance(&mut self, next: material::Appearance) -> Task<Message> {
        self.appearance = next;
        self.appearance_error = capsule::library::data_dir()
            .and_then(|dir| Ok(next.save(&dir)?))
            .err()
            .map(|e| format!("Could not save appearance: {e}"));
        Task::none()
    }

    fn handle(&mut self, message: Message) -> Task<Message> {
        self.poll();
        match message {
            Message::Search(msg) => return self.search_action(msg),
            #[cfg(windows)]
            Message::Maximized(maximized) => self.maximized = maximized,
            Message::Resize(size) => {
                self.window_size = size;
                #[cfg(windows)]
                return iced::window::oldest()
                    .and_then(iced::window::is_maximized)
                    .map(Message::Maximized);
            }
            Message::ToggleSidebar => self.sidebar_visible = !self.sidebar_visible,
            Message::ToggleQueue => self.queue_visible = !self.queue_is_open(),
            Message::CloseMenu => {
                self.menu = None;
                if self.search.open {
                    return iced::widget::operation::focus(search::INPUT);
                }
            }
            Message::QueueMenu(i, point) => {
                if let Some(item) = self.queue.entries().get(i) {
                    self.menu = Some((
                        point,
                        vec![("Add to playlist", Message::SaveTrack(item.saved()))],
                    ));
                }
            }
            Message::TrackMenu(track, point) => self.track_menu(track, point),
            Message::Library(library::Msg::TrackMenu(i, point)) => {
                if let Some(track) = self.library.tracks().and_then(|tracks| tracks.get(i)) {
                    self.track_menu(SavedTrack::File(track.clone()), point);
                }
            }
            Message::Playlists(playlists::Msg::TrackMenu(i, point)) => {
                if !self.playlists.busy()
                    && let Some(list) = self.playlists.contents()
                    && let Some(entry) = list.entries.get(i)
                {
                    use capsule::library::playlists::Edit;
                    let edit = |action| Message::Playlists(playlists::Msg::Edit(action));
                    let mut actions = vec![
                        ("Add to queue", Message::Enqueue(entry.track.clone())),
                        ("Add to playlist", Message::SaveTrack(entry.track.clone())),
                    ];
                    if i > 0 {
                        actions.push(("Move up", edit(Edit::Earlier(entry.id))));
                    }
                    if i + 1 < list.entries.len() {
                        actions.push(("Move down", edit(Edit::Later(entry.id))));
                    }
                    actions.push(("Remove from playlist", edit(Edit::Remove(entry.id))));
                    self.menu = Some((point, actions));
                }
            }
            Message::Enqueue(track) => {
                self.menu = None;
                let index = self.queue.entries().len();
                if let Some(order) = &mut self.shuffle_order {
                    order.push(index);
                }
                self.queue.push_back(QueueItem::from_saved(&track));
                if matches!(self.transport, Transport::Ended(_)) {
                    self.queue.select(index);
                    self.info = None;
                    self.transport = Transport::Idle;
                }
            }
            Message::SaveTrack(track) => {
                self.search.open = false;
                self.menu = None;
                return self.open_playlists(playlists::Mode::AddTrack(track));
            }
            Message::ShowSettings => self.screen = Screen::Settings,
            Message::Opacity(value) => {
                let mut next = material::Appearance::new(value);
                next.blur = self.appearance.blur;
                return self.set_appearance(next);
            }
            Message::Blur(blur) => {
                let mut next = self.appearance;
                next.blur = blur;
                return self.set_appearance(next);
            }
            #[cfg(windows)]
            Message::FrameUnavailable => {
                self.appearance_error = Some("Could not configure window controls.".into());
            }
            Message::AudioSettings => self.screen = Screen::Audio,
            #[cfg(windows)]
            Message::Window(action) => return frame::perform(action),
            Message::ShowDiscover => self.screen = Screen::Discover,
            Message::Discover(msg) => return self.discover.update(msg),
            Message::RefreshDiscover => return self.discover.refresh(self.youtube.clone()),
            Message::DiscoverLoaded(id, result) => return self.discover.loaded(id, result),
            Message::DiscoverCover(revision, id, cover) => {
                self.discover.cover(revision, &id, cover)
            }
            Message::DiscoverPlay { revision, index } => {
                if let Some(track) = self.discover.track(revision, index) {
                    self.load([QueueItem::Direct(Source::YouTube(track.clone().into()))]);
                }
            }
            Message::Repeat => self.repeat = !self.repeat,
            Message::Shuffle => self.shuffle(),
            Message::Artwork(path, details) => self.artwork.received(&path, details),
            Message::ShowPlaylists => return self.open_playlists(playlists::Mode::Browse),
            Message::SaveQueue => {
                if !self.queue.entries().is_empty() {
                    let tracks = self.queue.entries().iter().map(QueueItem::saved).collect();
                    return self.open_playlists(playlists::Mode::SaveQueue(tracks));
                }
            }
            Message::Playlists(playlists::Msg::Play(index)) => {
                if !self.playlists.busy()
                    && let Some(list) = self.playlists.contents()
                    && index < list.entries.len()
                {
                    self.shuffle_order = None;
                    self.queue = list
                        .entries
                        .iter()
                        .map(|entry| QueueItem::from_saved(&entry.track))
                        .collect();
                    self.queue.select(index);
                    self.info = None;
                    self.start(Duration::ZERO, true);
                    self.screen = Screen::Player;
                }
            }
            Message::Playlists(msg @ playlists::Msg::Cover(..)) => {
                return self.playlists.update(msg).map(Message::Playlists);
            }
            Message::Playlists(msg) => {
                self.menu = None;
                return self.playlists.update(msg).map(Message::Playlists);
            }
            Message::ShowLibrary => {
                self.screen = Screen::Library;
                return self
                    .library
                    .update(library::Msg::Open)
                    .map(Message::Library);
            }
            Message::ShowPlayer => self.screen = Screen::Player,
            Message::Library(library::Msg::Play) => {
                if let Some(tracks) = self.library.tracks()
                    && !tracks.is_empty()
                {
                    self.shuffle_order = None;
                    self.queue = tracks.iter().cloned().map(QueueItem::Indexed).collect();
                    self.info = None;
                    self.start(Duration::ZERO, true);
                    self.screen = Screen::Player;
                }
            }
            Message::Library(msg) => {
                return self.library.update(msg).map(Message::Library);
            }
            Message::Browse => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .add_filter(
                                "Audio",
                                &["mp3", "flac", "m4a", "mp4", "wav", "ogg", "oga", "aac"],
                            )
                            .pick_files()
                            .await
                            .unwrap_or_default()
                            .into_iter()
                            .map(|file| file.path().to_owned())
                            .collect()
                    },
                    Message::Picked,
                );
            }
            Message::Picked(paths) if !paths.is_empty() => {
                self.load(paths.into_iter().map(Source::File).map(QueueItem::Direct))
            }
            Message::Picked(_) | Message::Tick => {}
            Message::Test => self.load([QueueItem::Direct(Source::StereoCheck)]),
            Message::Select(index) => {
                if self.queue.select(index) {
                    let playing = !matches!(
                        self.transport,
                        Transport::Paused(_) | Transport::Opening { playing: false, .. }
                    );
                    self.info = None;
                    self.start(Duration::ZERO, playing);
                }
            }
            Message::Toggle => {
                let state = std::mem::replace(&mut self.transport, Transport::Idle);
                let playing = matches!(state, Transport::Paused(_));
                match state {
                    Transport::Playing(p) | Transport::Paused(p) => {
                        match self.player.set_playing(playing) {
                            Ok(()) => {
                                self.transport = if playing {
                                    Transport::Playing(p)
                                } else {
                                    Transport::Paused(p)
                                }
                            }
                            Err(e) => self.fail(e),
                        }
                    }
                    other @ Transport::Opening { .. } => self.transport = other,
                    Transport::Ended(_) => {
                        self.queue.select(0);
                        self.info = None;
                        self.start(Duration::ZERO, true);
                    }
                    _ => self.start(Duration::ZERO, true),
                }
            }
            Message::Stop => {
                self.transport = Transport::Idle;
                self.scrub = None;
                if let Err(e) = self.player.stop() {
                    self.fail(e);
                }
            }
            Message::Volume(volume) => {
                self.volume = volume;
                if let Some(playback) = self.active() {
                    playback.set_volume(volume);
                }
            }
            Message::Scrub(value) => self.scrub = Some(value),
            Message::Seek => {
                if let Some(secs) = self.scrub.take() {
                    let playing =
                        matches!(self.transport, Transport::Playing(_) | Transport::Ended(_));
                    let end = self
                        .track_duration(self.position())
                        .unwrap_or(Duration::MAX);
                    if Duration::from_secs_f32(secs) >= end {
                        if self.queue.select(self.queue.index() + 1) {
                            self.info = None;
                            self.start(Duration::ZERO, playing);
                        } else if let Err(e) = self.player.stop() {
                            self.fail(e);
                        } else {
                            self.transport = Transport::Ended(end);
                        }
                    } else {
                        self.start(Duration::from_secs_f32(secs), playing);
                    }
                }
            }
            Message::Device(device) => {
                self.device = device;
                if self.active().is_some() {
                    let playing = matches!(
                        self.transport,
                        Transport::Playing(_) | Transport::Opening { playing: true, .. }
                    );
                    self.start(self.position(), playing);
                }
            }
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        let dismiss = if self.menu.is_some() {
            iced::event::listen_with(|event, _, _| match event {
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                    ..
                })
                | iced::Event::Window(iced::window::Event::Unfocused) => Some(Message::CloseMenu),
                _ => None,
            })
        } else {
            Subscription::none()
        };
        let resize = iced::window::resize_events().map(|(_, size)| Message::Resize(size));
        let resize = Subscription::batch([
            resize,
            dismiss,
            search::keys(self.search.open && self.menu.is_none()),
        ]);
        let period = match self.transport {
            Transport::Opening { .. } | Transport::Playing(_) => 100,
            Transport::Paused(_) => 500,
            _ => return resize,
        };
        Subscription::batch([
            resize,
            iced::time::every(Duration::from_millis(period)).map(|_| Message::Tick),
        ])
    }

    fn view(&self) -> Element<'_, Message> {
        use iced::widget::{Space, column, container, mouse_area, opaque, pin, stack, text};
        let mut layers = stack![self.shell()];
        if self.search.open {
            layers = layers.push(Element::new(context::PaletteBackdrop)).push(
                container(opaque(
                    self.search.view(self.window_size.height, self.appearance),
                ))
                .center_x(iced::Fill)
                .padding(iced::Padding::new(12.).top((self.window_size.height * 0.16).min(110.))),
            );
        }
        let Some((point, actions)) = &self.menu else {
            return layers.into();
        };
        let width = 184.;
        let height = actions.len() as f32 * 32. + 8.;
        let x = point.x.min(self.window_size.width - width).max(0.);
        let y = point.y.min(self.window_size.height - height).max(0.);
        let mut items = column![];
        for (label, action) in actions {
            items = items.push(
                material::button(text(*label).size(13))
                    .width(iced::Fill)
                    .height(32)
                    .style(material::subtle_button)
                    .on_press(action.clone()),
            );
        }
        let menu = material::panel(self.appearance, items, 4, width, iced::Shrink);
        layers
            .push(opaque(
                mouse_area(Space::new().width(iced::Fill).height(iced::Fill))
                    .on_press(Message::CloseMenu)
                    .on_right_press(Message::CloseMenu),
            ))
            .push(pin(opaque(menu)).x(x).y(y))
            .into()
    }
}

fn clock(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn source_name(source: &Source) -> &str {
    match source {
        Source::File(path) => path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Local audio"),
        Source::StereoCheck => "Stereo check",
        Source::YouTube(t) => &t.title,
    }
}

