use super::*;
use std::{thread, time::Instant};

fn wait_for(app: &mut App, ready: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !ready(app) {
        app.poll();
        if let Transport::Failed(error) = &app.transport {
            panic!("{error}");
        }
        assert!(Instant::now() < deadline, "playback transition timed out");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "requires an audio output device"]
fn queue_transport_on_device() -> Result<(), AudioError> {
    let mut app = App::new(Theme::Dark, Player::new()?, std::iter::empty());
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/stereo-check.wav");
    let _ = app.update(Message::Volume(0));
    let _ = app.update(Message::Picked(vec![path.clone(), path]));
    wait_for(&mut app, |app| {
        matches!(app.transport, Transport::Playing(_))
    });
    let _ = app.update(Message::Toggle);
    let _ = app.update(Message::Select(1));
    wait_for(&mut app, |app| {
        matches!(app.transport, Transport::Paused(_))
    });
    assert_eq!(app.queue.index(), 1);
    assert_eq!(app.position(), Duration::ZERO);
    let _ = app.update(Message::Select(0));
    wait_for(&mut app, |app| {
        matches!(app.transport, Transport::Paused(_))
    });
    assert_eq!(app.queue.index(), 0);
    let _ = app.update(Message::Scrub(3.5));
    let _ = app.update(Message::Seek);
    wait_for(&mut app, |app| {
        matches!(app.transport, Transport::Paused(_))
    });
    assert_eq!(app.position(), Duration::from_millis(3500));
    let _ = app.update(Message::Toggle);
    wait_for(&mut app, |app| app.queue.index() == 1);
    assert!(app.position() < Duration::from_millis(500));
    wait_for(&mut app, |app| matches!(app.transport, Transport::Ended(_)));
    assert_eq!(app.position(), Duration::from_secs(4));
    let _ = app.update(Message::Toggle);
    wait_for(&mut app, |app| {
        matches!(app.transport, Transport::Playing(_))
    });
    assert_eq!(app.queue.index(), 0);
    let _ = app.update(Message::Stop);
    assert!(matches!(app.transport, Transport::Idle));
    Ok(())
}
async fn drive(app: &mut App, task: Task<Message>) {
    use iced::futures::StreamExt;
    let mut tasks = vec![task];
    while let Some(task) = tasks.pop() {
        if let Some(mut stream) = iced_runtime::task::into_stream(task) {
            while let Some(action) = stream.next().await {
                if let iced_runtime::Action::Output(msg) = action {
                    tasks.push(app.update(msg));
                }
            }
        }
    }
}

#[test]
#[ignore = "requires an audio output device"]
fn reopened_library_plays_album_without_picking_files() -> anyhow::Result<()> {
    use capsule::library::Library;
    use std::fs;
    let runtime = tokio::runtime::Builder::new_current_thread().build()?;
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("Album");
    let data = temp.path().join("index");
    fs::create_dir(&root)?;
    for file in ["Overture.wav", "Finale.wav"] {
        fs::write(
            root.join(file),
            include_bytes!("../../assets/stereo-check.wav"),
        )?;
    }
    let db = Library::open(&data)?;
    let mut first = App::new(Theme::Dark, Player::new()?, std::iter::empty());
    let task = first.update(Message::Library(library::Msg::Opened(Ok(db))));
    runtime.block_on(drive(&mut first, task));
    let task = first.update(Message::Library(library::Msg::Folder(Some(root))));
    runtime.block_on(drive(&mut first, task));
    drop(first);
    let mut app = App::new(Theme::Dark, Player::new()?, std::iter::empty());
    let db = Library::open(&data)?;
    let task = app.update(Message::Library(library::Msg::Opened(Ok(db))));
    runtime.block_on(drive(&mut app, task));
    assert!(app.screen == Screen::Discover);
    let picks = app.library.database().unwrap().discover()?;
    let (album, track) = picks.into_iter().next().unwrap();
    let _ = app.update(Message::Volume(0));
    app.load([QueueItem::Indexed(track)]);
    wait_for(&mut app, |app| app.position() > Duration::ZERO);
    let _ = app.update(Message::Stop);
    let _ = app.update(Message::ShowLibrary);
    let task = app.update(Message::Library(library::Msg::Album(album)));
    runtime.block_on(drive(&mut app, task));
    assert!(app.screen == Screen::Library);
    assert_eq!(app.library.tracks().unwrap().len(), 2);
    let task = app.update(Message::Library(library::Msg::Query("overture".into())));
    runtime.block_on(drive(&mut app, task));
    let album = app
        .library
        .albums()
        .first()
        .expect("album in search results")
        .clone();
    let task = app.update(Message::Library(library::Msg::Album(album)));
    runtime.block_on(drive(&mut app, task));
    let _ = app.update(Message::Volume(0));
    let _ = app.update(Message::Library(library::Msg::Play));
    wait_for(&mut app, |app| app.position() > Duration::ZERO);
    assert_eq!(app.queue.entries().len(), 2);
    assert_eq!(app.queue.current().unwrap().title(), "Finale");
    let _ = app.update(Message::Stop);
    Ok(())
}

#[test]
#[ignore = "requires an audio output device"]
fn global_search_plays_latest_query_and_restores_previous_screen() -> anyhow::Result<()> {
    use capsule::library::Library;
    use search::Msg;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let temp = tempfile::tempdir()?;
    let album = temp.path().join("Night");
    std::fs::create_dir(&album)?;
    for title in ["First", "Second"] {
        std::fs::write(
            album.join(format!("{title}.wav")),
            include_bytes!("../../assets/stereo-check.wav"),
        )?;
    }
    let db = Library::open(temp.path())?;
    db.scan(Some(&album))?;
    let mut app = App::new(Theme::Dark, Player::new()?, std::iter::empty());
    let task = app.update(Message::Library(library::Msg::Opened(Ok(db))));
    runtime.block_on(drive(&mut app, task));
    let _ = app.update(Message::Volume(0));
    let _ = app.update(Message::ShowSettings);
    use iced::{
        advanced::subscription,
        futures::{StreamExt, stream},
        keyboard::{self, Key},
    };
    let key = Key::Character("k".into());
    let event = subscription::Event::Interaction {
        window: iced::window::Id::unique(),
        status: iced::event::Status::Captured,
        event: iced::Event::Keyboard(keyboard::Event::KeyPressed {
            modified_key: key.clone(),
            key,
            modifiers: keyboard::Modifiers::CTRL,
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::KeyK),
            location: keyboard::Location::Standard,
            text: None,
            repeat: false,
        }),
    };
    let shortcut = subscription::into_recipes(search::keys(false))
        .pop()
        .unwrap();
    let mut events = shortcut.stream(stream::iter([event]).boxed());
    let _ = app.update(runtime.block_on(events.next()).expect("Ctrl+K shortcut"));
    assert!(app.search.open && app.screen == Screen::Settings);
    let _ = app.update(Message::Search(Msg::Scope(search::Scope::Local)));
    let pending = app.update(Message::Search(Msg::Query("first".into())));
    let latest = app.update(Message::Search(Msg::Query("sEcOnD".into())));
    let _ = app.update(Message::Search(Msg::Submit));
    runtime.block_on(drive(&mut app, pending));
    runtime.block_on(drive(&mut app, latest));
    wait_for(&mut app, |app| app.position() > Duration::ZERO);
    assert_eq!(app.queue.current().unwrap().title(), "Second");
    assert!(app.screen == Screen::Settings);
    let _ = app.update(Message::Search(Msg::Open));
    let task = app.update(Message::Search(Msg::Query("night".into())));
    runtime.block_on(drive(&mut app, task));
    let _ = app.update(Message::Search(Msg::Move(true)));
    let _ = app.update(Message::Search(Msg::Submit));
    assert_eq!(app.queue.current().unwrap().title(), "Second");
    let _ = app.update(Message::Search(Msg::Open));
    let _ = app.update(Message::Search(Msg::Close));
    assert!(app.screen == Screen::Settings);
    assert!(!app.search.open);
    let _ = app.update(Message::Search(Msg::Open));
    let _ = app.update(Message::Search(Msg::Submit));
    assert!(!app.search.open && app.screen == Screen::Library);
    Ok(())
}

#[test]
fn palette_backdrop_dismisses_without_activating_sidebar() -> anyhow::Result<()> {
    use iced_runtime::{
        UserInterface,
        core::{clipboard, mouse, renderer::Headless},
    };
    let mut app = App::new(Theme::Dark, Player::new()?, std::iter::empty());
    let _ = app.update(Message::ShowSettings);
    let _ = app.update(Message::Search(search::Msg::Open));
    let mut renderer = iced::futures::executor::block_on(iced::Renderer::new(
        iced::Font::DEFAULT,
        iced::Pixels(13.),
        Some("tiny-skia"),
    ))
    .expect("test renderer");
    let mut ui = UserInterface::build(app.view(), WINDOW_SIZE, Default::default(), &mut renderer);
    let mut messages = Vec::new();
    let _ = ui.update(
        &[iced::Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Left,
        ))],
        mouse::Cursor::Available(iced::Point::new(55., 85.)),
        &mut renderer,
        &mut clipboard::Null,
        &mut messages,
    );
    assert!(matches!(
        messages.as_slice(),
        [Message::Search(search::Msg::Close)]
    ));
    drop(ui);
    let _ = app.update(messages.pop().unwrap());
    assert!(!app.search.open && app.screen == Screen::Settings);
    Ok(())
}

#[test]
#[ignore = "requires an audio output device"]
fn saved_playlist_reopens_and_plays_in_edited_order() -> anyhow::Result<()> {
    use capsule::library::{
        Library,
        playlists::{Edit, Store},
    };
    use std::fs;

    let runtime = tokio::runtime::Builder::new_current_thread().build()?;
    let send = |app: &mut App, message| {
        let task = app.update(message);
        runtime.block_on(drive(app, task));
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("Music");
    let data = temp.path().join("Data");
    for (album, title) in [("One", "First"), ("One", "Second"), ("Two", "Third")] {
        let folder = root.join(album);
        fs::create_dir_all(&folder)?;
        fs::write(
            folder.join(format!("{title}.wav")),
            include_bytes!("../../assets/stereo-check.wav"),
        )?;
    }
    let mut app = App::new(Theme::Dark, Player::new()?, std::iter::empty());
    send(&mut app, Message::Volume(0));
    send(
        &mut app,
        Message::Library(library::Msg::Opened(Ok(Library::open(&data)?))),
    );
    send(&mut app, Message::Library(library::Msg::Folder(Some(root))));
    let albums = app.library.albums().to_vec();
    assert_eq!(albums.len(), 2);
    send(
        &mut app,
        Message::Library(library::Msg::Album(albums[0].clone())),
    );
    send(&mut app, Message::Library(library::Msg::Play));
    send(
        &mut app,
        Message::Playlists(playlists::Msg::Listed(Ok((
            Store::open(&data)?,
            Vec::new(),
        )))),
    );
    send(&mut app, Message::SaveQueue);
    send(
        &mut app,
        Message::Playlists(playlists::Msg::Name("Night".into())),
    );
    send(&mut app, Message::Playlists(playlists::Msg::Create));
    let id = app.playlists.contents().unwrap().playlist.id;
    send(&mut app, Message::ShowLibrary);
    send(
        &mut app,
        Message::Library(library::Msg::Album(albums[1].clone())),
    );
    let track = app.library.tracks().unwrap()[0].clone();
    send(&mut app, Message::SaveTrack(SavedTrack::File(track)));
    send(&mut app, Message::Playlists(playlists::Msg::Select(id)));
    let entry = app.playlists.contents().unwrap().entries[2].id;
    for _ in 0..2 {
        send(
            &mut app,
            Message::Playlists(playlists::Msg::Edit(Edit::Earlier(entry))),
        );
    }
    assert_eq!(app.queue.entries().len(), 2);
    assert!(app.active().is_some());
    drop(app);

    let mut reopened = App::new(Theme::Dark, Player::new()?, std::iter::empty());
    send(&mut reopened, Message::Volume(0));
    let db = Store::open(&data)?;
    let lists = db.list()?;
    assert_eq!(lists[0].name, "Night");
    let id = lists[0].id;
    send(
        &mut reopened,
        Message::Playlists(playlists::Msg::Listed(Ok((db, lists)))),
    );
    send(&mut reopened, Message::ShowPlaylists);
    send(
        &mut reopened,
        Message::Playlists(playlists::Msg::Select(id)),
    );
    send(&mut reopened, Message::Playlists(playlists::Msg::Play(0)));
    for (i, title) in ["Third", "First", "Second"].into_iter().enumerate() {
        wait_for(&mut reopened, |app| {
            app.queue.index() == i && app.position() > Duration::ZERO
        });
        assert_eq!(reopened.queue.current().unwrap().title(), title);
    }
    wait_for(&mut reopened, |app| {
        matches!(app.transport, Transport::Ended(_))
    });
    send(&mut reopened, Message::Playlists(playlists::Msg::Play(1)));
    wait_for(&mut reopened, |app| {
        app.queue.index() == 1 && app.position() > Duration::ZERO
    });
    assert_eq!(reopened.queue.current().unwrap().title(), "First");
    assert_eq!(reopened.queue.entries().len(), 3);
    Ok(())
}

#[test]
#[ignore = "requires an audio output device"]
fn moved_playlist_track_is_skipped_and_restored_on_replay() -> anyhow::Result<()> {
    use capsule::library::playlists::Store;
    let temp = tempfile::tempdir()?;
    let mut tracks = Vec::new();
    for title in ["First", "Middle", "Last"] {
        let path = temp.path().join(title).with_extension("wav");
        std::fs::write(&path, include_bytes!("../../assets/stereo-check.wav"))?;
        tracks.push(SavedTrack::File(Track {
            title: title.into(),
            artist: String::new(),
            path,
        }));
    }
    let middle = temp.path().join("Middle.wav");
    let moved = temp.path().join("Moved.wav");
    let store = Store::open(temp.path())?;
    let playlist = store.create("Mix", &tracks)?.playlist;
    std::fs::rename(&middle, &moved)?;
    let runtime = tokio::runtime::Builder::new_current_thread().build()?;
    let mut app = App::new(Theme::Dark, Player::new()?, std::iter::empty());
    let _ = app.update(Message::Volume(0));
    let _ = app.update(Message::Playlists(playlists::Msg::Listed(Ok((
        store.clone(),
        store.list()?,
    )))));
    let task = app.update(Message::Playlists(playlists::Msg::Select(playlist.id)));
    runtime.block_on(drive(&mut app, task));
    let _ = app.update(Message::Playlists(playlists::Msg::Play(0)));
    wait_for(&mut app, |a| {
        a.queue.index() == 0 && a.position() > Duration::ZERO
    });
    wait_for(&mut app, |a| {
        a.queue.index() == 2 && a.position() > Duration::ZERO
    });
    assert!(app.skipped.contains_key(&1));
    assert_eq!(store.contents(playlist.id)?.entries.len(), 3);
    std::fs::rename(moved, middle)?;
    let _ = app.update(Message::Playlists(playlists::Msg::Play(0)));
    wait_for(&mut app, |a| {
        a.queue.index() == 1 && a.position() > Duration::ZERO
    });
    assert!(app.skipped.is_empty());
    wait_for(&mut app, |a| {
        a.queue.index() == 2 && a.position() > Duration::ZERO
    });
    Ok(())
}

#[test]
#[ignore = "requires an audio output device"]
fn unavailable_ends_of_queue_and_replay() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let missing = temp.path().join("Offline.wav");
    let mut app = App::new(Theme::Dark, Player::new()?, std::iter::empty());
    let _ = app.update(Message::Volume(0));
    let file = || QueueItem::Direct(Source::File(missing.clone()));
    app.load([
        file(),
        file(),
        QueueItem::Direct(Source::StereoCheck),
        file(),
    ]);
    wait_for(&mut app, |a| {
        a.queue.index() == 2 && a.position() > Duration::ZERO
    });
    wait_for(&mut app, |a| matches!(a.transport, Transport::Ended(_)));
    assert_eq!(app.skipped.keys().copied().collect::<Vec<_>>(), [0, 1, 3]);
    assert_eq!(app.queue.index(), 2);
    app.load([file(), file()]);
    wait_for(&mut app, |a| matches!(a.transport, Transport::Ended(_)));
    assert_eq!(app.skipped.len(), 2);
    assert!(app.info.is_none());
    std::fs::write(missing, include_bytes!("../../assets/stereo-check.wav"))?;
    let _ = app.update(Message::Toggle);
    wait_for(&mut app, |a| a.position() > Duration::ZERO);
    assert!(app.skipped.is_empty());
    assert_eq!(app.queue.index(), 0);
    Ok(())
}

#[test]
#[ignore = "requires an audio output device"]
fn dock_shuffle_restores_order_and_repeat_loops() -> anyhow::Result<()> {
    let mut app = App::new(Theme::Dark, Player::new()?, std::iter::empty());
    let _ = app.update(Message::Volume(0));
    app.queue = ["a", "b", "c", "d"]
        .map(|s| QueueItem::Direct(Source::File(s.into())))
        .into_iter()
        .collect();
    app.queue.select(1);
    let _ = app.update(Message::Shuffle);
    assert!(app.shuffle_order.is_some());
    assert_eq!(app.queue.current().unwrap().title(), "b");
    let _ = app.update(Message::Shuffle);
    assert_eq!(
        app.queue
            .entries()
            .iter()
            .map(QueueItem::title)
            .collect::<Vec<_>>(),
        ["a", "b", "c", "d"]
    );
    let _ = app.update(Message::Test);
    wait_for(&mut app, |a| a.position() > Duration::ZERO);
    let _ = app.update(Message::Repeat);
    let _ = app.update(Message::Scrub(3.8));
    let _ = app.update(Message::Seek);
    wait_for(&mut app, |a| {
        matches!(a.transport, Transport::Playing(_)) && a.position() < Duration::from_secs(1)
    });
    Ok(())
}

#[test]
fn listening_view_and_duration_follow_available_track_data() -> anyhow::Result<()> {
    let mut app = App::new(Theme::Dark, Player::new()?, std::iter::empty());
    app.queue = [QueueItem::Direct(Source::StereoCheck)]
        .into_iter()
        .collect();
    assert!(!app.queue_is_open());
    let _ = app.update(Message::ToggleQueue);
    assert!(app.queue_is_open());
    let _ = app.update(Message::Resize(Size::new(590., 450.)));
    assert!(app.queue_is_open());
    let _ = app.update(Message::ToggleQueue);
    assert!(!app.queue_is_open());
    app.info = Some(TrackInfo {
        input_rate: 44100,
        input_channels: 2,
        output_rate: 48000,
        output_channels: 2,
        duration: Some(Duration::ZERO),
    });
    assert!(app.track_duration(Duration::from_secs(1)).is_none());
    app.artwork.details = Some(artwork::Details {
        duration: Some(Duration::from_secs(125)),
        ..Default::default()
    });
    assert_eq!(
        app.track_duration(Duration::from_secs(1)),
        Some(Duration::from_secs(125))
    );
    assert!(app.track_duration(Duration::from_secs(130)).is_none());
    Ok(())
}

#[test]
#[ignore = "requires an audio output device"]
fn dragging_the_playback_bar_seeks_the_paused_track() -> anyhow::Result<()> {
    use iced_runtime::{
        UserInterface,
        core::{clipboard, mouse, renderer::Headless},
    };
    let mut app = App::new(Theme::Dark, Player::new()?, std::iter::empty());
    let _ = app.update(Message::Volume(0));
    app.load([QueueItem::Direct(Source::StereoCheck)]);
    wait_for(&mut app, |a| matches!(a.transport, Transport::Playing(_)));
    let _ = app.update(Message::Toggle);
    app.screen = Screen::Player;
    app.appearance = material::Appearance::default();
    let mut renderer = iced::futures::executor::block_on(iced::Renderer::new(
        iced::Font::DEFAULT,
        iced::Pixels(13.),
        Some("tiny-skia"),
    ))
    .expect("test renderer");
    let mut ui = UserInterface::build(app.view(), WINDOW_SIZE, Default::default(), &mut renderer);
    let mut messages = Vec::new();
    for (x, event) in [
        (320., mouse::Event::ButtonPressed(mouse::Button::Left)),
        (
            700.,
            mouse::Event::CursorMoved {
                position: iced::Point::new(700., 707.),
            },
        ),
        (700., mouse::Event::ButtonReleased(mouse::Button::Left)),
    ] {
        let _ = ui.update(
            &[iced::Event::Mouse(event)],
            mouse::Cursor::Available(iced::Point::new(x, 707.)),
            &mut renderer,
            &mut clipboard::Null,
            &mut messages,
        );
    }
    drop(ui);
    assert!(
        matches!(messages.last(), Some(Message::Seek)),
        "{messages:?}"
    );
    for msg in messages {
        let _ = app.update(msg);
    }
    wait_for(&mut app, |a| matches!(a.transport, Transport::Paused(_)));
    assert!((2.0..3.5).contains(&app.position().as_secs_f32()));
    Ok(())
}

#[test]
fn opacity_drag_continues_across_the_opaque_endpoint() -> anyhow::Result<()> {
    use iced_runtime::{
        UserInterface,
        core::{clipboard, mouse, renderer::Headless},
    };
    let mut app = App::new(Theme::Dark, Player::new()?, std::iter::empty());
    let _ = app.update(Message::ShowSettings);
    app.appearance = material::Appearance::DARK;
    let mut renderer = iced::futures::executor::block_on(iced::Renderer::new(
        iced::Font::DEFAULT,
        iced::Pixels(13.),
        Some("tiny-skia"),
    ))
    .expect("test renderer");
    let mut ui = UserInterface::build(app.view(), WINDOW_SIZE, Default::default(), &mut renderer);
    let mut messages = Vec::new();
    let mut active_row = None;
    for y in (100..220).step_by(4) {
        let cursor = mouse::Cursor::Available(iced::Point::new(450., y as f32));
        let _ = ui.update(
            &[iced::Event::Mouse(mouse::Event::ButtonPressed(
                mouse::Button::Left,
            ))],
            cursor,
            &mut renderer,
            &mut clipboard::Null,
            &mut messages,
        );
        if let Some(Message::Opacity(value)) = messages.last() {
            active_row = Some((y, *value));
            break;
        }
        let _ = ui.update(
            &[iced::Event::Mouse(mouse::Event::ButtonReleased(
                mouse::Button::Left,
            ))],
            cursor,
            &mut renderer,
            &mut clipboard::Null,
            &mut messages,
        );
    }
    let (y, value) = active_row.expect("opacity slider responds to a press");
    let cache = ui.into_cache();
    app.appearance = material::Appearance::new(value);
    assert!(app.appearance.glass());
    let mut ui = UserInterface::build(app.view(), WINDOW_SIZE, cache, &mut renderer);
    let pos = iced::Point::new(280., y as f32);
    messages.clear();
    let _ = ui.update(
        &[iced::Event::Mouse(mouse::Event::CursorMoved {
            position: pos,
        })],
        mouse::Cursor::Available(pos),
        &mut renderer,
        &mut clipboard::Null,
        &mut messages,
    );
    assert!(matches!(messages.last(), Some(Message::Opacity(lower)) if *lower < value));
    Ok(())
}

#[test]
#[ignore = "requires an audio output device"]
fn context_menu_enqueues_without_restarting_playback() -> anyhow::Result<()> {
    use iced::Point;
    use iced_runtime::{
        UserInterface,
        core::{clipboard, mouse, renderer::Headless},
    };
    let mut app = App::new(Theme::Dark, Player::new()?, std::iter::empty());
    app.volume = 0;
    app.load([QueueItem::Direct(Source::StereoCheck)]);
    wait_for(&mut app, |a| matches!(a.transport, Transport::Playing(_)));
    let _ = app.update(Message::Toggle);
    let _ = app.update(Message::Scrub(3.5));
    let _ = app.update(Message::Seek);
    wait_for(&mut app, |a| matches!(a.transport, Transport::Paused(_)));
    let point = Point::new(90., 80.);
    let row = context::row(iced::widget::Space::new().width(200).height(100), |p| {
        Message::TrackMenu(SavedTrack::StereoCheck, p)
    });
    let mut renderer = iced::futures::executor::block_on(iced::Renderer::new(
        iced::Font::DEFAULT,
        iced::Pixels(13.),
        Some("tiny-skia"),
    ))
    .expect("test renderer");
    let mut ui = UserInterface::build(row, WINDOW_SIZE, Default::default(), &mut renderer);
    let mut messages = Vec::new();
    let _ = ui.update(
        &[iced::Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Right,
        ))],
        mouse::Cursor::Available(point),
        &mut renderer,
        &mut clipboard::Null,
        &mut messages,
    );
    let _ = app.update(messages.pop().expect("right-click opens menu"));
    let mut ui = UserInterface::build(app.view(), WINDOW_SIZE, Default::default(), &mut renderer);
    let cursor = mouse::Cursor::Available(Point::new(point.x + 40., point.y + 18.));
    let _ = ui.update(
        &[
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ],
        cursor,
        &mut renderer,
        &mut clipboard::Null,
        &mut messages,
    );
    drop(ui);
    for msg in messages {
        let _ = app.update(msg);
    }
    assert!(app.menu.is_none());
    assert_eq!(app.queue.entries().len(), 2);
    assert_eq!(app.position(), Duration::from_millis(3500));
    assert!(matches!(app.transport, Transport::Paused(_)));
    let _ = app.update(Message::Toggle);
    wait_for(&mut app, |a| {
        a.queue.index() == 1 && matches!(a.transport, Transport::Playing(_))
    });
    Ok(())
}

#[test]
fn sidebar_and_empty_queue_toggle_through_visible_buttons() -> anyhow::Result<()> {
    use iced_runtime::{
        UserInterface,
        core::{clipboard, mouse, renderer::Headless},
    };
    let mut app = App::new(Theme::Dark, Player::new()?, std::iter::empty());
    app.appearance = material::Appearance::default();
    let mut renderer = iced::futures::executor::block_on(iced::Renderer::new(
        iced::Font::DEFAULT,
        iced::Pixels(13.),
        Some("tiny-skia"),
    ))
    .expect("test renderer");
    for (x, y, sidebar, queue) in [
        (22., 16., false, false),
        (1045., 708., false, true),
        (1045., 708., false, false),
        (22., 16., true, false),
    ] {
        let mut ui =
            UserInterface::build(app.view(), WINDOW_SIZE, Default::default(), &mut renderer);
        let mut messages = Vec::new();
        let _ = ui.update(
            &[
                iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            ],
            mouse::Cursor::Available(iced::Point::new(x, y)),
            &mut renderer,
            &mut clipboard::Null,
            &mut messages,
        );
        drop(ui);
        assert_eq!(messages.len(), 1, "toggle emits one action");
        let _ = app.update(messages.pop().unwrap());
        assert_eq!(app.sidebar_expanded, sidebar);
        assert_eq!(app.queue_is_open(), queue);
    }
    Ok(())
}

#[test]
#[ignore = "requires YouTube network and an audio output device"]
fn youtube_search_enters_queue_and_seeks() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let mut app = App::new(Theme::Dark, Player::new()?, std::iter::empty());
    app.volume = 0;
    let _ = app.update(Message::Search(search::Msg::Open));
    let task = app.update(Message::Search(search::Msg::Query(
        "Radiohead Weird Fishes official audio".into(),
    )));
    let _ = app.update(Message::Search(search::Msg::Submit));
    runtime.block_on(drive(&mut app, task));
    assert!(!app.search.open, "Enter should play when results arrive");
    assert!(matches!(
        app.queue.current(),
        Some(QueueItem::Direct(Source::YouTube(_)))
    ));
    let deadline = Instant::now() + Duration::from_secs(70);
    while !matches!(app.transport, Transport::Playing(_)) {
        app.poll();
        if let Transport::Failed(error) = &app.transport {
            anyhow::bail!("{error}");
        }
        assert!(Instant::now() < deadline, "YouTube open timed out");
        thread::sleep(Duration::from_millis(20));
    }
    let _ = app.update(Message::Toggle);
    let _ = app.update(Message::Scrub(45.));
    let _ = app.update(Message::Seek);
    let deadline = Instant::now() + Duration::from_secs(70);
    while !matches!(app.transport, Transport::Paused(_)) {
        app.poll();
        if let Transport::Failed(error) = &app.transport {
            anyhow::bail!("{error}");
        }
        assert!(Instant::now() < deadline, "YouTube seek timed out");
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(app.position(), Duration::from_secs(45));
    let _ = app.update(Message::Stop);
    Ok(())
}

#[test]
fn failed_youtube_open_reports_the_stream_error() -> anyhow::Result<()> {
    struct RejectedStream;
    impl capsule::audio::AudioSource for RejectedStream {
        fn open_media(&self) -> Result<Box<dyn capsule::audio::MediaSource>, AudioError> {
            Err(std::io::Error::other("Audio server returned HTTP 403").into())
        }
    }
    let mut app = App::new(Theme::Dark, Player::new()?, std::iter::empty());
    app.queue = [QueueItem::Direct(Source::YouTube(
        capsule::providers::youtube::Track {
            id: "Ve-rxwKuqtI"
                .to_owned()
                .try_into()
                .map_err(anyhow::Error::msg)?,
            title: "Cowboys and Angels".into(),
            artist: "George Michael".into(),
            thumbnail: None,
            duration: Some(Duration::from_secs(437)),
        }
        .into(),
    ))]
    .into_iter()
    .collect();
    app.request_start = 0;
    app.request_end = 1;
    app.transport = Transport::Opening {
        playback: app.player.open(
            RejectedStream,
            OutputDevice::System,
            Duration::ZERO,
            true,
            0,
        )?,
        playing: true,
    };
    let deadline = Instant::now() + Duration::from_secs(3);
    while matches!(app.transport, Transport::Opening { .. }) {
        app.poll();
        assert!(Instant::now() < deadline, "failed stream did not finish");
        thread::sleep(Duration::from_millis(5));
    }
    assert!(
        matches!(&app.transport, Transport::Failed(error) if error.contains("HTTP 403")),
        "An unavailable YouTube stream must report its actual error"
    );
    Ok(())
}

#[test]
fn collapsed_sidebar_keeps_navigation_actions_accessible() -> anyhow::Result<()> {
    use iced_runtime::{
        UserInterface,
        core::{clipboard, mouse, renderer::Headless},
    };
    let mut app = App::new(Theme::Dark, Player::new()?, std::iter::empty());
    app.sidebar_expanded = false;
    app.screen = Screen::Settings;
    let mut renderer = iced::futures::executor::block_on(iced::Renderer::new(
        iced::Font::DEFAULT,
        iced::Pixels(13.),
        Some("tiny-skia"),
    ))
    .expect("test renderer");
    for (y, screen) in [
        (135., Screen::Library),
        (580., Screen::Audio),
        (620., Screen::Settings),
        (95., Screen::Discover),
    ] {
        let mut ui =
            UserInterface::build(app.view(), WINDOW_SIZE, Default::default(), &mut renderer);
        let mut messages = Vec::new();
        let _ = ui.update(
            &[
                iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            ],
            mouse::Cursor::Available(iced::Point::new(35., y)),
            &mut renderer,
            &mut clipboard::Null,
            &mut messages,
        );
        drop(ui);
        assert_eq!(messages.len(), 1, "collapsed icon emits its action");
        let _ = app.update(messages.pop().unwrap());
        assert!(app.screen == screen);
        assert!(!app.sidebar_expanded);
    }
    Ok(())
}

#[test]
fn volume_pill_works_at_normal_and_minimum_widths() -> anyhow::Result<()> {
    use iced_runtime::{
        UserInterface,
        core::{clipboard, mouse, renderer::Headless},
    };
    let mut app = App::new(Theme::Dark, Player::new()?, std::iter::empty());
    let mut renderer = iced::futures::executor::block_on(iced::Renderer::new(
        iced::Font::DEFAULT,
        iced::Pixels(13.),
        Some("tiny-skia"),
    ))
    .expect("test renderer");
    for (size, point) in [
        (WINDOW_SIZE, iced::Point::new(970., 707.)),
        (iced::Size::new(570., 700.), iced::Point::new(480., 667.)),
    ] {
        app.window_size = size;
        let mut ui = UserInterface::build(app.view(), size, Default::default(), &mut renderer);
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
            messages
                .iter()
                .any(|m| matches!(m, Message::Volume(value) if *value < 65)),
            "volume slider remains usable"
        );
    }
    Ok(())
}
