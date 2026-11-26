use super::{LibraryError, Track, data_dir, path};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaylistId(i64);
#[derive(Debug, Clone, Copy)]
pub struct EntryId(i64);

#[derive(Debug, Clone)]
pub struct Playlist {
    pub id: PlaylistId,
    pub name: String,
}

#[derive(Debug, Clone)]
pub enum SavedTrack {
    File(Track),
    StereoCheck,
    YouTube(crate::providers::youtube::Track),
}

impl SavedTrack {
    pub fn title(&self) -> &str {
        match self {
            Self::File(t) => &t.title,
            Self::YouTube(t) => &t.title,
            Self::StereoCheck => "Stereo check",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub id: EntryId,
    pub track: SavedTrack,
}

#[derive(Debug, Clone)]
pub struct Contents {
    pub playlist: Playlist,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, Copy)]
pub enum Edit {
    Earlier(EntryId),
    Later(EntryId),
    Remove(EntryId),
}

#[derive(Debug, Clone)]
pub struct Store(PathBuf);

impl Store {
    pub fn local() -> Result<Self, LibraryError> {
        Self::open(&data_dir()?)
    }

    pub fn open(dir: &Path) -> Result<Self, LibraryError> {
        fs::create_dir_all(dir)?;
        let store = Self(dir.join("playlists.sqlite3"));
        let mut db = store.connect()?;
        db.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS playlists (
                id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE COLLATE NOCASE
            );
            CREATE TABLE IF NOT EXISTS entries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                playlist INTEGER NOT NULL REFERENCES playlists(id),
                position INTEGER NOT NULL, path BLOB, title TEXT NOT NULL, artist TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS playlist_order ON entries(playlist, position);
        ",
        )?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let has_provider = tx
            .prepare("PRAGMA table_info(entries)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .any(|name| name == "provider_track");
        if !has_provider {
            tx.execute_batch("ALTER TABLE entries ADD COLUMN provider_track TEXT")?;
        }
        tx.commit()?;
        Ok(store)
    }

    fn connect(&self) -> Result<Connection, LibraryError> {
        let db = Connection::open(&self.0)?;
        db.execute_batch("PRAGMA foreign_keys=ON")?;
        Ok(db)
    }

    pub fn list(&self) -> Result<Vec<Playlist>, LibraryError> {
        let db = self.connect()?;
        Ok(db
            .prepare("SELECT id,name FROM playlists ORDER BY name")?
            .query_map([], |r| {
                Ok(Playlist {
                    id: PlaylistId(r.get(0)?),
                    name: r.get(1)?,
                })
            })?
            .collect::<Result<_, _>>()?)
    }

    pub fn contents(&self, id: PlaylistId) -> Result<Contents, LibraryError> {
        let mut db = self.connect()?;
        let tx = db.transaction()?;
        read(&tx, id)
    }

    fn change(
        &self,
        action: impl FnOnce(&Transaction<'_>) -> Result<PlaylistId, LibraryError>,
    ) -> Result<Contents, LibraryError> {
        let mut db = self.connect()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = action(&tx)?;
        let contents = read(&tx, id)?;
        tx.commit()?;
        Ok(contents)
    }

    pub fn create(&self, name: &str, tracks: &[SavedTrack]) -> Result<Contents, LibraryError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(LibraryError::EmptyName);
        }
        self.change(|tx| {
            if tx.execute(
                "INSERT INTO playlists(name) VALUES(?) ON CONFLICT(name) DO NOTHING",
                [name],
            )? == 0
            {
                return Err(LibraryError::DuplicateName);
            }
            let id = PlaylistId(tx.last_insert_rowid());
            append(tx, id, tracks)?;
            Ok(id)
        })
    }

    pub fn add(&self, id: PlaylistId, track: &SavedTrack) -> Result<Contents, LibraryError> {
        self.change(|tx| {
            append(tx, id, std::slice::from_ref(track))?;
            Ok(id)
        })
    }

    pub fn edit(&self, playlist: PlaylistId, edit: Edit) -> Result<Contents, LibraryError> {
        self.change(|tx| {
            let id = match edit { Edit::Earlier(id) | Edit::Later(id) | Edit::Remove(id) => id };
            let pos: i64 = tx.query_row("SELECT position FROM entries WHERE playlist=? AND id=?",
                (playlist.0, id.0), |r| r.get(0)).optional()?.ok_or(LibraryError::MissingEntry)?;
            let neighbor = match edit {
                Edit::Remove(_) => {
                    tx.execute("DELETE FROM entries WHERE id=?", [id.0])?;
                    return Ok(playlist);
                }
                Edit::Earlier(_) => "SELECT id,position FROM entries WHERE playlist=? AND position<? ORDER BY position DESC LIMIT 1",
                Edit::Later(_) => "SELECT id,position FROM entries WHERE playlist=? AND position>? ORDER BY position LIMIT 1",
            };
            if let Some((other, other_pos)) = tx.query_row(neighbor, (playlist.0, pos),
                |r| Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?))).optional()? {
                tx.execute("UPDATE entries SET position=? WHERE id=?", (other_pos,id.0))?;
                tx.execute("UPDATE entries SET position=? WHERE id=?", (pos,other))?;
            }
            Ok(playlist)
        })
    }
}

fn read(db: &Connection, id: PlaylistId) -> Result<Contents, LibraryError> {
    let name = db.query_row("SELECT name FROM playlists WHERE id=?", [id.0], |r| {
        r.get(0)
    })?;
    let entries = db
        .prepare("SELECT id,path,title,artist,provider_track FROM entries WHERE playlist=? ORDER BY position")?
        .query_map([id.0], |r| {
            let track = if let Some(json)=r.get::<_,Option<String>>(4)? {
                SavedTrack::YouTube(serde_json::from_str(&json).map_err(|err|rusqlite::Error::FromSqlConversionFailure(4,rusqlite::types::Type::Text,Box::new(err)))?)
            } else {match r.get::<_, Option<Vec<u8>>>(1)? {
                Some(bytes) => SavedTrack::File(Track {
                    path: path::decode(bytes)?,
                    title: r.get(2)?,
                    artist: r.get(3)?,
                }),
                None => SavedTrack::StereoCheck,
            }};
            Ok(Entry {
                id: EntryId(r.get(0)?),
                track,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(Contents {
        playlist: Playlist { id, name },
        entries,
    })
}

fn append(tx: &Transaction<'_>, id: PlaylistId, tracks: &[SavedTrack]) -> Result<(), LibraryError> {
    let first: i64 = tx.query_row(
        "SELECT coalesce(max(position),-1)+1 FROM entries WHERE playlist=?",
        [id.0],
        |r| r.get(0),
    )?;
    let mut insert =
        tx.prepare("INSERT INTO entries(playlist,position,path,title,artist,provider_track) VALUES(?,?,?,?,?,?)")?;
    for (pos, track) in (first..).zip(tracks) {
        let (file, artist) = match track {
            SavedTrack::File(t) => (
                Some(path::encode(&std::path::absolute(&t.path)?)),
                t.artist.as_str(),
            ),
            SavedTrack::StereoCheck => (None, ""),
            SavedTrack::YouTube(t) => (None, t.artist.as_str()),
        };
        let remote = if let SavedTrack::YouTube(t) = track {
            Some(serde_json::to_string(t)?)
        } else {
            None
        };
        insert.execute(params![id.0, pos, file, track.title(), artist, remote])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn youtube_entries_survive_reopen_and_legacy_migration() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let db = Connection::open(dir.path().join("playlists.sqlite3"))?;
        db.execute_batch("CREATE TABLE playlists(id INTEGER PRIMARY KEY,name TEXT NOT NULL UNIQUE COLLATE NOCASE);
            CREATE TABLE entries(id INTEGER PRIMARY KEY AUTOINCREMENT,playlist INTEGER NOT NULL,position INTEGER NOT NULL,path BLOB,title TEXT NOT NULL,artist TEXT NOT NULL);
            INSERT INTO playlists VALUES(1,'Legacy');
            INSERT INTO entries(playlist,position,path,title,artist) VALUES(1,0,NULL,'Stereo check','');")?;
        drop(db);
        let store = Store::open(dir.path())?;
        let id = store.list()?[0].id;
        let remote = SavedTrack::YouTube(crate::providers::youtube::Track {
            id: "V_Ydoe4Q-Gg"
                .to_owned()
                .try_into()
                .map_err(anyhow::Error::msg)?,
            title: "Song".into(),
            artist: "Artist".into(),
            thumbnail: None,
            duration: Some(std::time::Duration::from_secs(13)),
        });
        store.add(id, &remote)?;
        drop(store);
        let store = Store::open(dir.path())?;
        let contents = store.contents(id)?;
        assert!(matches!(
            &contents.entries[0].track,
            SavedTrack::StereoCheck
        ));
        assert!(
            matches!(&contents.entries[1].track,SavedTrack::YouTube(t) if t.id.as_str()=="V_Ydoe4Q-Gg")
        );
        Ok(())
    }

    #[test]
    fn duplicate_entries_are_independent_and_names_cannot_overwrite() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let store = Store::open(temp.path())?;
        let tracks = [SavedTrack::StereoCheck, SavedTrack::StereoCheck];
        let original = store.create("  Mix  ", &tracks)?;
        let id = original.playlist.id;
        assert!(matches!(
            store.create("mix", &[]),
            Err(LibraryError::DuplicateName)
        ));
        assert!(matches!(
            store.create("  ", &[]),
            Err(LibraryError::EmptyName)
        ));
        let first = original.entries[0].id;
        let second = original.entries[1].id;
        let swapped = store.edit(id, Edit::Later(first))?;
        assert_eq!(swapped.entries[0].id.0, second.0);
        store.edit(id, Edit::Remove(second))?;
        assert!(matches!(
            store.edit(id, Edit::Earlier(second)),
            Err(LibraryError::MissingEntry)
        ));
        let reopened = Store::open(temp.path())?;
        assert_eq!(reopened.list()?.len(), 1);
        let contents = reopened.contents(id)?;
        assert_eq!(contents.entries.len(), 1);
        assert_eq!(contents.entries[0].id.0, first.0);
        assert_eq!(contents.playlist.name, "Mix");
        Ok(())
    }
}
