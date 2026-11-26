use super::{Library, LibraryError, path};
use lofty::{
    config::ParseOptions,
    error::ErrorKind,
    prelude::{Accessor, ItemKey, TaggedFileExt},
    probe::Probe,
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use std::{fs, path::Path, time::UNIX_EPOCH};
use walkdir::WalkDir;

#[derive(Debug, Clone, Copy, Default)]
pub struct ScanStats {
    pub indexed: usize,
    pub unchanged: usize,
    pub skipped: usize,
}

impl Library {
    pub fn scan(&self, selected: Option<&Path>) -> Result<ScanStats, LibraryError> {
        let mut db = Connection::open(&self.file)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let folder = match selected {
            Some(folder) => fs::canonicalize(folder)?,
            None => tx
                .query_row("SELECT folder FROM library WHERE id=1", [], |r| {
                    path::decode(r.get(0)?)
                })
                .optional()?
                .ok_or(LibraryError::NoFolder)?,
        };
        if !fs::metadata(&folder)?.is_dir() {
            return Err(LibraryError::NotDirectory);
        }
        tx.execute_batch("CREATE TEMP TABLE seen(path BLOB PRIMARY KEY) WITHOUT ROWID")?;
        let mut stats = ScanStats::default();
        for entry in WalkDir::new(&folder).follow_links(false) {
            let entry = entry?;
            if entry.file_type().is_file() && supported(entry.path()) {
                index_file(&tx, entry.path(), &mut stats)?;
            }
        }
        tx.execute(
            "DELETE FROM tracks WHERE path NOT IN (SELECT path FROM seen)",
            [],
        )?;
        tx.execute("INSERT OR REPLACE INTO library(id,folder,total) VALUES(1,?1,(SELECT count(*) FROM tracks))", [path::encode(&folder)])?;
        tx.commit()?;
        Ok(stats)
    }
}

fn supported(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            ["mp3", "flac", "m4a", "mp4", "wav", "ogg", "oga", "aac"]
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known))
        })
}

fn index_file(
    tx: &Transaction<'_>,
    file: &Path,
    stats: &mut ScanStats,
) -> Result<(), LibraryError> {
    let metadata = fs::metadata(file)?;
    let modified = match metadata.modified()?.duration_since(UNIX_EPOCH) {
        Ok(t) => t.as_nanos().to_string(),
        Err(e) => format!("-{}", e.duration().as_nanos()),
    };
    let key = path::encode(file);
    let stamp = params![key, modified, metadata.len()];
    let unchanged: bool = tx
        .prepare_cached(
            "SELECT EXISTS(SELECT 1 FROM tracks WHERE path=?1 AND modified=?2 AND bytes=?3)",
        )?
        .query_row(stamp, |r| r.get(0))?;
    if unchanged {
        stats.unchanged += 1;
    } else {
        let options = ParseOptions::new()
            .read_cover_art(false)
            .read_properties(false);
        let probe = Probe::new(std::io::BufReader::new(fs::File::open(file)?)).guess_file_type()?;
        let tagged = match probe.options(options).read() {
            Ok(tagged) => tagged,
            Err(source) if is_file_error(&source) => {
                return Err(LibraryError::Read {
                    path: file.to_path_buf(),
                    source,
                });
            }
            Err(_) => {
                stats.skipped += 1;
                return Ok(());
            }
        };
        let tags = || tagged.primary_tag().into_iter().chain(tagged.tags().iter());
        let title = tags()
            .find_map(|t| t.title().filter(|s| !s.trim().is_empty()))
            .unwrap_or_else(|| file.file_stem().unwrap_or_default().to_string_lossy());
        let artist = tags()
            .find_map(|t| t.artist().filter(|s| !s.trim().is_empty()))
            .unwrap_or("Unknown artist".into());
        let tagged_album = tags().find_map(|t| t.album().filter(|s| !s.trim().is_empty()));
        let parent = file.parent().unwrap_or(file);
        let scope = if tagged_album.is_none() {
            path::encode(parent)
        } else {
            Vec::new()
        };
        let album = tagged_album.unwrap_or_else(|| {
            parent
                .file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or("Unknown album".into())
        });
        let album_artist = tags()
            .find_map(|t| {
                t.get_string(&ItemKey::AlbumArtist)
                    .filter(|s| !s.trim().is_empty())
            })
            .unwrap_or(&artist);
        let disc = tags().find_map(|t| t.disk()).filter(|n| *n > 0);
        let track = tags().find_map(|t| t.track()).filter(|n| *n > 0);
        let search = format!("{title}\n{artist}\n{album}\n{album_artist}").to_lowercase();
        tx.prepare_cached("INSERT OR REPLACE INTO tracks(path,modified,bytes,title,artist,album,album_artist,scope,disc,track,search_text)
          VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)")?
          .execute(params![key,modified,metadata.len(),title.trim(),artist.trim(),album.trim(),album_artist.trim(),scope,disc,track,search])?;
        stats.indexed += 1;
    }
    tx.prepare_cached("INSERT INTO seen(path) VALUES(?1)")?
        .execute([key])?;
    Ok(())
}

fn is_file_error(error: &lofty::error::LoftyError) -> bool {
    use std::io::ErrorKind::{InvalidData, InvalidInput, UnexpectedEof};
    let ErrorKind::Io(io) = error.kind() else {
        return false;
    };
    // Windows reports a malformed tag's negative seek as ERROR_NEGATIVE_SEEK.
    !(matches!(io.kind(), InvalidData | InvalidInput | UnexpectedEof)
        || cfg!(windows) && io.raw_os_error() == Some(131))
}
