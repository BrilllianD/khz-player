//! SQLite storage for the library, playlists and per-track EQ presets.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

use super::Track;
use crate::playlist::Playlist;

const SCHEMA_V1: &str = "
CREATE TABLE tracks (
    path TEXT PRIMARY KEY,
    mtime INTEGER NOT NULL,
    size INTEGER NOT NULL,
    title TEXT, artist TEXT, album TEXT, album_artist TEXT,
    track_no INTEGER, disc_no INTEGER, year INTEGER, genre TEXT,
    duration_ms INTEGER, bitrate INTEGER, sample_rate INTEGER, channels INTEGER,
    seen INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX idx_tracks_artist_album ON tracks(artist, album, disc_no, track_no);
CREATE TABLE playlists (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    position INTEGER NOT NULL
);
CREATE TABLE playlist_items (
    playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    path TEXT NOT NULL,
    PRIMARY KEY (playlist_id, position)
);
CREATE TABLE eq_auto (
    path TEXT PRIMARY KEY,
    preset TEXT NOT NULL
);
";

const TRACK_COLS: &str = "path, mtime, size, title, artist, album, album_artist, track_no, \
     disc_no, year, genre, duration_ms, bitrate, sample_rate, channels";

pub fn default_path() -> PathBuf {
    crate::config::data_dir().join("library.db")
}

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let conn = Connection::open(path)?;
    setup(&conn)?;
    Ok(conn)
}

#[cfg(test)]
pub fn open_memory() -> rusqlite::Result<Connection> {
    let conn = Connection::open_in_memory()?;
    setup(&conn)?;
    Ok(conn)
}

fn setup(conn: &Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    migrate(conn)
}

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version < 1 {
        conn.execute_batch(&format!("BEGIN; {SCHEMA_V1} PRAGMA user_version = 1; COMMIT;"))?;
    }
    if version < 3 {
        // Tag reading changed (relaxed parsing, CP1251 repair); drop cached
        // metadata so the next scan re-reads every file. Playlists keep their paths.
        conn.execute_batch("BEGIN; DELETE FROM tracks; PRAGMA user_version = 3; COMMIT;")?;
    }
    Ok(())
}

fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn row_to_track(r: &Row<'_>) -> rusqlite::Result<Track> {
    Ok(Track {
        path: PathBuf::from(r.get::<_, String>(0)?),
        mtime: r.get(1)?,
        size: r.get(2)?,
        title: r.get(3)?,
        artist: r.get(4)?,
        album: r.get(5)?,
        album_artist: r.get(6)?,
        track_no: r.get(7)?,
        disc_no: r.get(8)?,
        year: r.get(9)?,
        genre: r.get(10)?,
        duration_ms: r.get::<_, Option<i64>>(11)?.map(|v| v as u64),
        bitrate: r.get(12)?,
        sample_rate: r.get(13)?,
        channels: r.get(14)?,
    })
}

pub fn load_tracks(conn: &Connection) -> rusqlite::Result<Vec<Track>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {TRACK_COLS} FROM tracks ORDER BY \
         COALESCE(album_artist, artist) COLLATE NOCASE, album COLLATE NOCASE, \
         disc_no, track_no, path"
    ))?;
    stmt.query_map([], row_to_track)?.collect()
}

pub fn get_track(conn: &Connection, path: &Path) -> rusqlite::Result<Option<Track>> {
    conn.query_row(
        &format!("SELECT {TRACK_COLS} FROM tracks WHERE path = ?1"),
        [path_str(path)],
        row_to_track,
    )
    .optional()
}

pub fn stamps(conn: &Connection) -> rusqlite::Result<HashMap<PathBuf, (i64, i64)>> {
    let mut stmt = conn.prepare("SELECT path, mtime, size FROM tracks")?;
    stmt.query_map([], |r| {
        Ok((
            PathBuf::from(r.get::<_, String>(0)?),
            (r.get(1)?, r.get(2)?),
        ))
    })?
    .collect()
}

pub fn upsert_track(tx: &Transaction<'_>, t: &Track) -> rusqlite::Result<()> {
    tx.prepare_cached(&format!(
        "INSERT INTO tracks ({TRACK_COLS}, seen) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, 1) \
         ON CONFLICT(path) DO UPDATE SET mtime=excluded.mtime, size=excluded.size, \
         title=excluded.title, artist=excluded.artist, album=excluded.album, \
         album_artist=excluded.album_artist, track_no=excluded.track_no, \
         disc_no=excluded.disc_no, year=excluded.year, genre=excluded.genre, \
         duration_ms=excluded.duration_ms, bitrate=excluded.bitrate, \
         sample_rate=excluded.sample_rate, channels=excluded.channels, seen=1"
    ))?
    .execute(params![
        path_str(&t.path),
        t.mtime,
        t.size,
        t.title,
        t.artist,
        t.album,
        t.album_artist,
        t.track_no,
        t.disc_no,
        t.year,
        t.genre,
        t.duration_ms.map(|v| v as i64),
        t.bitrate,
        t.sample_rate,
        t.channels,
    ])?;
    Ok(())
}

pub fn mark_seen(tx: &Transaction<'_>, path: &Path) -> rusqlite::Result<()> {
    tx.prepare_cached("UPDATE tracks SET seen = 1 WHERE path = ?1")?
        .execute([path_str(path)])?;
    Ok(())
}

/// Clears `seen` for tracks under `roots` before a scan.
pub fn clear_seen(conn: &Connection, roots: &[PathBuf]) -> rusqlite::Result<()> {
    for root in roots {
        let prefix = format!("{}/", path_str(root).trim_end_matches('/'));
        conn.execute(
            "UPDATE tracks SET seen = 0 WHERE substr(path, 1, length(?1)) = ?1",
            [prefix],
        )?;
    }
    Ok(())
}

pub fn delete_unseen(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute("DELETE FROM tracks WHERE seen = 0", [])
}

// --- playlists ---

pub fn load_playlists(conn: &Connection) -> rusqlite::Result<Vec<Playlist>> {
    let mut lists = Vec::new();
    let mut stmt = conn.prepare("SELECT id, name FROM playlists ORDER BY position, id")?;
    let heads: Vec<(i64, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut items = conn.prepare(
        "SELECT i.path, t.mtime, t.size, t.title, t.artist, t.album, t.album_artist, \
         t.track_no, t.disc_no, t.year, t.genre, t.duration_ms, t.bitrate, t.sample_rate, \
         t.channels, t.path IS NOT NULL \
         FROM playlist_items i LEFT JOIN tracks t ON t.path = i.path \
         WHERE i.playlist_id = ?1 ORDER BY i.position",
    )?;
    for (id, name) in heads {
        let tracks = items
            .query_map([id], |r| {
                if r.get::<_, bool>(15)? {
                    row_to_track(r)
                } else {
                    Ok(Track::from_path(PathBuf::from(r.get::<_, String>(0)?)))
                }
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut p = Playlist::new(name);
        p.id = Some(id);
        p.tracks = tracks;
        lists.push(p);
    }
    Ok(lists)
}

/// Inserts or updates the playlist and replaces its items.
pub fn save_playlist(conn: &mut Connection, p: &mut Playlist, position: usize) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    let id = match p.id {
        Some(id) => {
            tx.execute(
                "UPDATE playlists SET name = ?1, position = ?2 WHERE id = ?3",
                params![p.name, position as i64, id],
            )?;
            id
        }
        None => {
            tx.execute(
                "INSERT INTO playlists (name, position) VALUES (?1, ?2)",
                params![p.name, position as i64],
            )?;
            tx.last_insert_rowid()
        }
    };
    tx.execute("DELETE FROM playlist_items WHERE playlist_id = ?1", [id])?;
    {
        let mut ins = tx.prepare_cached(
            "INSERT INTO playlist_items (playlist_id, position, path) VALUES (?1, ?2, ?3)",
        )?;
        for (i, t) in p.tracks.iter().enumerate() {
            ins.execute(params![id, i as i64, path_str(&t.path)])?;
        }
    }
    tx.commit()?;
    p.id = Some(id);
    p.dirty = false;
    Ok(())
}

pub fn delete_playlist(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM playlists WHERE id = ?1", [id])?;
    Ok(())
}

// --- EQ auto presets ---

pub fn eq_auto_get(conn: &Connection, path: &Path) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT preset FROM eq_auto WHERE path = ?1",
        [path_str(path)],
        |r| r.get(0),
    )
    .optional()
}

pub fn eq_auto_set(conn: &Connection, path: &Path, preset: Option<&str>) -> rusqlite::Result<()> {
    match preset {
        Some(p) => conn.execute(
            "INSERT INTO eq_auto (path, preset) VALUES (?1, ?2) \
             ON CONFLICT(path) DO UPDATE SET preset = excluded.preset",
            params![path_str(path), p],
        )?,
        None => conn.execute("DELETE FROM eq_auto WHERE path = ?1", [path_str(path)])?,
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(p: &str, title: &str) -> Track {
        Track {
            path: PathBuf::from(p),
            mtime: 1,
            size: 2,
            title: Some(title.into()),
            artist: Some("A".into()),
            duration_ms: Some(1000),
            ..Default::default()
        }
    }

    #[test]
    fn migrate_twice_is_noop() {
        let conn = open_memory().unwrap();
        migrate(&conn).unwrap();
        let v: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, 3);
    }

    #[test]
    fn tracks_upsert_and_seen() {
        let mut conn = open_memory().unwrap();
        let tx = conn.transaction().unwrap();
        upsert_track(&tx, &track("/m/a.mp3", "a")).unwrap();
        upsert_track(&tx, &track("/m/b.mp3", "b")).unwrap();
        upsert_track(&tx, &track("/other/c.mp3", "c")).unwrap();
        tx.commit().unwrap();
        let tx = conn.transaction().unwrap();
        upsert_track(&tx, &track("/m/a.mp3", "a2")).unwrap();
        tx.commit().unwrap();
        assert_eq!(load_tracks(&conn).unwrap().len(), 3);
        let a = get_track(&conn, Path::new("/m/a.mp3")).unwrap().unwrap();
        assert_eq!(a.title.as_deref(), Some("a2"));

        clear_seen(&conn, &[PathBuf::from("/m")]).unwrap();
        let tx = conn.transaction().unwrap();
        mark_seen(&tx, Path::new("/m/a.mp3")).unwrap();
        tx.commit().unwrap();
        assert_eq!(delete_unseen(&conn).unwrap(), 1);
        let left: Vec<_> = load_tracks(&conn).unwrap().into_iter().map(|t| t.path).collect();
        assert!(left.contains(&PathBuf::from("/m/a.mp3")));
        assert!(left.contains(&PathBuf::from("/other/c.mp3")));
        assert_eq!(stamps(&conn).unwrap()[Path::new("/m/a.mp3")], (1, 2));
    }

    #[test]
    fn playlist_roundtrip() {
        let mut conn = open_memory().unwrap();
        let tx = conn.transaction().unwrap();
        upsert_track(&tx, &track("/m/a.mp3", "Known")).unwrap();
        tx.commit().unwrap();

        let mut p = Playlist::new("Rock");
        p.add([
            Track::from_path("/m/a.mp3".into()),
            Track::from_path("/m/unknown.flac".into()),
        ]);
        save_playlist(&mut conn, &mut p, 0).unwrap();
        assert!(p.id.is_some() && !p.dirty);
        let mut q = Playlist::new("Default");
        save_playlist(&mut conn, &mut q, 1).unwrap();

        let lists = load_playlists(&conn).unwrap();
        assert_eq!(lists.len(), 2);
        assert_eq!(lists[0].name, "Rock");
        assert_eq!(lists[0].tracks[0].title.as_deref(), Some("Known"));
        assert_eq!(lists[0].tracks[1].path, PathBuf::from("/m/unknown.flac"));
        assert_eq!(lists[0].tracks[1].title, None);

        // Re-save with fewer items replaces them.
        p.remove(&[0].into_iter().collect());
        save_playlist(&mut conn, &mut p, 0).unwrap();
        assert_eq!(load_playlists(&conn).unwrap()[0].tracks.len(), 1);

        delete_playlist(&conn, p.id.unwrap()).unwrap();
        let n: i64 = conn
            .query_row("SELECT count(*) FROM playlist_items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn eq_auto() {
        let conn = open_memory().unwrap();
        let p = Path::new("/m/a.mp3");
        assert_eq!(eq_auto_get(&conn, p).unwrap(), None);
        eq_auto_set(&conn, p, Some("Rock")).unwrap();
        eq_auto_set(&conn, p, Some("Pop")).unwrap();
        assert_eq!(eq_auto_get(&conn, p).unwrap().as_deref(), Some("Pop"));
        eq_auto_set(&conn, p, None).unwrap();
        assert_eq!(eq_auto_get(&conn, p).unwrap(), None);
    }
}
