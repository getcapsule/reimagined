mod path;
pub mod playlists;
mod scan;

use rusqlite::{Connection, OptionalExtension, params};
pub use scan::ScanStats;
use std::{
    fs,
    path::{Path, PathBuf},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LibraryError {
    #[error("Library database: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("Library file: {0}")]
    Io(#[from] std::io::Error),
    #[error("Cannot scan folder: {0}")]
    Walk(#[from] walkdir::Error),
    #[error("Cannot read {path}: {source}")]
    Read {
        path: PathBuf,
        source: lofty::error::LoftyError,
    },
    #[error("Choose a music folder first.")]
    NoFolder,
    #[error("Choose a directory to scan.")]
    NotDirectory,
    #[error("The local application data directory is unavailable.")]
    NoDataDirectory,
    #[error("Enter a playlist name.")]
    EmptyName,
    #[error("A playlist with that name already exists.")]
    DuplicateName,
    #[error("This playlist entry is no longer available.")]
    MissingEntry,
    #[error("Stored provider track: {0}")]
    ProviderTrack(#[from] serde_json::Error),
}

#[derive(Debug, Clone)]
pub struct Library {
    file: PathBuf,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Album {
    pub title: String,
    pub artist: String,
    scope: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Track {
    pub title: String,
    pub artist: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct AlbumPage {
    pub folder: Option<PathBuf>,
    pub total_tracks: usize,
    pub albums: Vec<Album>,
    pub has_more: bool,
}

const PAGE_SIZE: usize = 30;

pub fn data_dir() -> Result<PathBuf, LibraryError> {
    std::env::var_os("CAPSULE_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::data_local_dir().map(|dir| dir.join("Capsule").join("v2")))
        .ok_or(LibraryError::NoDataDirectory)
}

impl Library {
    pub fn local() -> Result<Self, LibraryError> {
        Self::open(&data_dir()?)
    }

    pub fn open(dir: &Path) -> Result<Self, LibraryError> {
        fs::create_dir_all(dir)?;
        let file = dir.join("library.sqlite3");
        let db = Connection::open(&file)?;
        db.execute_batch("PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS library(id INTEGER PRIMARY KEY CHECK(id=1), folder BLOB NOT NULL, total INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS tracks(
                path BLOB PRIMARY KEY, modified TEXT NOT NULL, bytes INTEGER NOT NULL,
                title TEXT NOT NULL, artist TEXT NOT NULL, album TEXT NOT NULL COLLATE NOCASE,
                album_artist TEXT NOT NULL COLLATE NOCASE, scope BLOB NOT NULL,
                disc INTEGER, track INTEGER, search_text TEXT NOT NULL
            ) WITHOUT ROWID;
            CREATE INDEX IF NOT EXISTS album_order ON tracks(album,album_artist,scope,disc,track,title);")?;
        Ok(Self { file })
    }

    pub fn search(&self, query: &str, page: usize) -> Result<AlbumPage, LibraryError> {
        self.albums(query, None, page)
    }
    pub fn artist_albums(&self, artist: &str, page: usize) -> Result<AlbumPage, LibraryError> {
        self.albums("", Some(artist), page)
    }
    fn albums(
        &self,
        query: &str,
        artist: Option<&str>,
        page: usize,
    ) -> Result<AlbumPage, LibraryError> {
        let mut db = Connection::open(&self.file)?;
        let tx = db.transaction()?;
        let state = tx
            .query_row("SELECT folder,total FROM library WHERE id=1", [], |r| {
                Ok((path::decode(r.get(0)?)?, r.get(1)?))
            })
            .optional()?;
        let mut stmt = tx.prepare(
            "SELECT album,album_artist,scope FROM tracks
            WHERE (?1='' OR instr(search_text,?1)>0) AND (?4 IS NULL OR album_artist=?4 COLLATE NOCASE) GROUP BY album,album_artist,scope
            ORDER BY album,album_artist,scope LIMIT ?2 OFFSET ?3",
        )?;
        let mut albums = stmt
            .query_map(
                params![
                    query.trim().to_lowercase(),
                    PAGE_SIZE + 1,
                    page.saturating_mul(PAGE_SIZE),
                    artist
                ],
                |r| {
                    Ok(Album {
                        title: r.get(0)?,
                        artist: r.get(1)?,
                        scope: r.get(2)?,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        let has_more = albums.len() > PAGE_SIZE;
        albums.truncate(PAGE_SIZE);
        let (folder, total_tracks) =
            state.map_or((None, 0), |(folder, total)| (Some(folder), total));
        Ok(AlbumPage {
            folder,
            total_tracks,
            albums,
            has_more,
        })
    }

    pub fn discover(&self) -> Result<Vec<(Album, Track)>, LibraryError> {
        let db = Connection::open(&self.file)?;
        let mut stmt = db.prepare(
            "WITH picks AS (
                SELECT MIN(path) AS path FROM tracks GROUP BY album,album_artist,scope
                ORDER BY random() LIMIT 9
             )
             SELECT t.album,t.album_artist,t.scope,t.title,t.artist,t.path
             FROM picks JOIN tracks t ON t.path=picks.path ORDER BY random()",
        )?;
        Ok(stmt
            .query_map([], |r| {
                Ok((
                    Album {
                        title: r.get(0)?,
                        artist: r.get(1)?,
                        scope: r.get(2)?,
                    },
                    Track {
                        title: r.get(3)?,
                        artist: r.get(4)?,
                        path: path::decode(r.get(5)?)?,
                    },
                ))
            })?
            .collect::<Result<_, _>>()?)
    }

    pub fn search_tracks(&self, query: &str) -> Result<Vec<(Track, String)>, LibraryError> {
        let term = query.trim().to_lowercase();
        if term.is_empty() {
            return Ok(Vec::new());
        }
        let db = Connection::open(&self.file)?;
        let mut stmt = db.prepare(
            "SELECT path,title,artist,album FROM tracks WHERE instr(search_text,?1)>0
             ORDER BY CASE WHEN substr(search_text,1,instr(search_text,char(10))-1)=?1 THEN 0
                           WHEN instr(search_text,?1)=1 THEN 1
                           WHEN instr(substr(search_text,1,instr(search_text,char(10))-1),?1)>0 THEN 2
                           ELSE 3 END, album,album_artist,scope,coalesce(disc,1),track IS NULL,track,title,path
             LIMIT 30"
        )?;
        Ok(stmt
            .query_map([term], |r| {
                Ok((
                    Track {
                        path: path::decode(r.get(0)?)?,
                        title: r.get(1)?,
                        artist: r.get(2)?,
                    },
                    r.get(3)?,
                ))
            })?
            .collect::<Result<_, _>>()?)
    }

    pub fn tracks(&self, album: &Album) -> Result<Vec<Track>, LibraryError> {
        let db = Connection::open(&self.file)?;
        let mut stmt = db.prepare(
            "SELECT path,title,artist FROM tracks
            WHERE album=?1 AND album_artist=?2 AND scope=?3
            ORDER BY coalesce(disc,1), track IS NULL, track, title COLLATE NOCASE, path",
        )?;
        Ok(stmt
            .query_map(params![album.title, album.artist, album.scope], |r| {
                Ok(Track {
                    path: path::decode(r.get(0)?)?,
                    title: r.get(1)?,
                    artist: r.get(2)?,
                })
            })?
            .collect::<Result<_, _>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artist_albums_match_the_artist_instead_of_title_or_search_text() -> Result<(), LibraryError>
    {
        let temp = tempfile::tempdir()?;
        let library = Library::open(temp.path())?;
        let db = Connection::open(&library.file)?;
        for (id, title, artist) in [
            (1u8, "First", "Sade"),
            (2, "Second", "sade"),
            (3, "Sade", "Other"),
            (4, "Third", "Sade Tribute"),
        ] {
            db.execute(
                "INSERT INTO tracks VALUES(?1,'0',0,'Song',?2,?3,?2,x'00',1,1,'sade')",
                params![vec![id], artist, title],
            )?;
        }
        let results = library.artist_albums("Sade", 0)?;
        assert_eq!(
            results
                .albums
                .iter()
                .map(|a| a.title.as_str())
                .collect::<Vec<_>>(),
            ["First", "Second"]
        );
        assert!(!results.has_more);
        assert_eq!(library.search("sade", 0)?.albums.len(), 4);
        Ok(())
    }
}
