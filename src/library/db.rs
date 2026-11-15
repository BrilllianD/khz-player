//! SQLite storage for the library, playlists and per-track EQ presets.

use std::collections::HashMap;
use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

use super::{ReplayGain, Track};
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
     disc_no, year, genre, duration_ms, bitrate, sample_rate, channels, \
     rg_track_gain, rg_track_peak, rg_album_gain, rg_album_peak";

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

const SCHEMA_VERSION: i64 = 6;

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
            Some(format!(
                "library.db schema version {version} is newer than this build supports \
                 ({SCHEMA_VERSION})"
            )),
        ));
    }
    if version < 1 {
        conn.execute_batch(&format!("BEGIN; {SCHEMA_V1} PRAGMA user_version = 1; COMMIT;"))?;
    }
    if version < 4 {
        // Tag reading changed (relaxed parsing, CP1251 repair, '?'-only tags);
        // drop cached metadata so the next scan re-reads every file. Playlists
        // keep their paths.
        conn.execute_batch("BEGIN; DELETE FROM tracks; PRAGMA user_version = 4; COMMIT;")?;
    }
    if version < 5 {
        // Paths become raw bytes. A TEXT-affinity column stores BLOBs unchanged, so no
        // table rebuild is needed. Old lossy rows stop matching and are pruned by the
        // next scan.
        conn.execute_batch(
            "BEGIN; \
             UPDATE tracks SET path = CAST(path AS BLOB) WHERE typeof(path) = 'text'; \
             UPDATE playlist_items SET path = CAST(path AS BLOB) WHERE typeof(path) = 'text'; \
             UPDATE eq_auto SET path = CAST(path AS BLOB) WHERE typeof(path) = 'text'; \
             DROP INDEX IF EXISTS idx_tracks_artist_album; \
             PRAGMA user_version = 5; COMMIT;",
        )?;
    }
    if version < 6 {
        // ReplayGain columns. Unlike v4 the rows stay: mtime = -1 never matches a file
        // stamp, so the next scan re-reads every file, and App::new starts that scan
        // (`needs_rescan`).
        // Another instance may be migrating at the same time: take the write lock first,
        // then re-check the version and the columns under it.
        conn.execute_batch("BEGIN IMMEDIATE;")?;
        if let Err(e) = migrate_v6(conn) {
            // Keep the migration error; a failed rollback ends with the connection anyway.
            let _ = conn.execute_batch("ROLLBACK;");
            return Err(e);
        }
        conn.execute_batch("COMMIT;")?;
    }
    Ok(())
}

/// Runs inside a write transaction. No-op when another instance already migrated.
fn migrate_v6(conn: &Connection) -> rusqlite::Result<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version >= 6 {
        return Ok(());
    }
    for col in ["rg_track_gain", "rg_track_peak", "rg_album_gain", "rg_album_peak"] {
        if !has_column(conn, "tracks", col)? {
            conn.execute_batch(&format!("ALTER TABLE tracks ADD COLUMN {col} REAL;"))?;
        }
    }
    conn.execute_batch("UPDATE tracks SET mtime = -1; PRAGMA user_version = 6;")
}

fn has_column(conn: &Connection, table: &str, col: &str) -> rusqlite::Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = stmt.query_map([], |r| r.get::<_, String>(1))?;
    for name in names {
        if name? == col {
            return Ok(true);
        }
    }
    Ok(false)
}

fn path_bytes(p: &Path) -> &[u8] {
    p.as_os_str().as_bytes()
}

fn get_path(r: &Row<'_>, i: usize) -> rusqlite::Result<PathBuf> {
    match r.get_ref(i)? {
        ValueRef::Blob(b) | ValueRef::Text(b) => {
            Ok(PathBuf::from(OsString::from_vec(b.to_vec())))
        }
        other => Err(rusqlite::Error::InvalidColumnType(
            i,
            "path".into(),
            other.data_type(),
        )),
    }
}

fn row_to_track(r: &Row<'_>) -> rusqlite::Result<Track> {
    Ok(Track {
        path: get_path(r, 0)?,
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
        replay_gain: ReplayGain {
            track_gain_db: r.get(15)?,
            track_peak: r.get(16)?,
            album_gain_db: r.get(17)?,
            album_peak: r.get(18)?,
        },
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
        [path_bytes(path)],
        row_to_track,
    )
    .optional()
}

pub fn stamps(conn: &Connection) -> rusqlite::Result<HashMap<PathBuf, (i64, i64)>> {
    let mut stmt = conn.prepare("SELECT path, mtime, size FROM tracks")?;
    stmt.query_map([], |r| {
        Ok((get_path(r, 0)?, (r.get(1)?, r.get(2)?)))
    })?
    .collect()
}

pub fn upsert_track(tx: &Transaction<'_>, t: &Track) -> rusqlite::Result<()> {
    tx.prepare_cached(&format!(
        "INSERT INTO tracks ({TRACK_COLS}, seen) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, 1) \
         ON CONFLICT(path) DO UPDATE SET mtime=excluded.mtime, size=excluded.size, \
         title=excluded.title, artist=excluded.artist, album=excluded.album, \
         album_artist=excluded.album_artist, track_no=excluded.track_no, \
         disc_no=excluded.disc_no, year=excluded.year, genre=excluded.genre, \
         duration_ms=excluded.duration_ms, bitrate=excluded.bitrate, \
         sample_rate=excluded.sample_rate, channels=excluded.channels, \
         rg_track_gain=excluded.rg_track_gain, rg_track_peak=excluded.rg_track_peak, \
         rg_album_gain=excluded.rg_album_gain, rg_album_peak=excluded.rg_album_peak, seen=1"
    ))?
    .execute(params![
        path_bytes(&t.path),
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
        t.replay_gain.track_gain_db,
        t.replay_gain.track_peak,
        t.replay_gain.album_gain_db,
        t.replay_gain.album_peak,
    ])?;
    Ok(())
}

pub fn mark_seen(tx: &Transaction<'_>, path: &Path) -> rusqlite::Result<()> {
    tx.prepare_cached("UPDATE tracks SET seen = 1 WHERE path = ?1")?
        .execute([path_bytes(path)])?;
    Ok(())
}

/// Key range `[root + "/", root + "0")`: '0' is '/' + 1, so it covers exactly the subtree
/// and uses the primary key index.
fn subtree_range(root: &Path) -> (Vec<u8>, Vec<u8>) {
    let mut lo = path_bytes(root).to_vec();
    while lo.len() > 1 && lo.ends_with(b"/") {
        lo.pop();
    }
    if lo != b"/" {
        lo.push(b'/');
    }
    let mut hi = lo.clone();
    *hi.last_mut().unwrap() = b'0';
    (lo, hi)
}

/// Deletes tracks (and the auto-EQ rows of those tracks) that lie under none of `roots`.
/// Auto-EQ rows for paths not in `tracks` (files added from outside the library) are kept.
/// With no roots nothing is deleted.
pub fn delete_outside_roots(conn: &Connection, roots: &[PathBuf]) -> rusqlite::Result<usize> {
    if roots.is_empty() {
        return Ok(0);
    }
    let mut cond = String::new();
    let mut args: Vec<Vec<u8>> = Vec::new();
    for root in roots {
        let (lo, hi) = subtree_range(root);
        if !cond.is_empty() {
            cond.push_str(" OR ");
        }
        cond.push_str(&format!("(path >= ?{} AND path < ?{})", args.len() + 1, args.len() + 2));
        args.push(lo);
        args.push(hi);
    }
    conn.execute(
        &format!(
            "DELETE FROM eq_auto WHERE path IN (SELECT path FROM tracks WHERE NOT ({cond}))"
        ),
        rusqlite::params_from_iter(args.iter()),
    )?;
    conn.execute(
        &format!("DELETE FROM tracks WHERE NOT ({cond})"),
        rusqlite::params_from_iter(args.iter()),
    )
}

/// Clears `seen` for tracks under `roots` before a scan.
pub fn clear_seen(conn: &Connection, roots: &[PathBuf]) -> rusqlite::Result<()> {
    // Rows left at seen = 0 by an interrupted scan must not be deleted by this scan's prune.
    conn.execute("UPDATE tracks SET seen = 1 WHERE seen = 0", [])?;
    for root in roots {
        let (lo, hi) = subtree_range(root);
        conn.execute(
            "UPDATE tracks SET seen = 0 WHERE path >= ?1 AND path < ?2",
            params![lo, hi],
        )?;
    }
    Ok(())
}

pub fn delete_unseen(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute("DELETE FROM eq_auto WHERE path IN (SELECT path FROM tracks WHERE seen = 0)", [])?;
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
         t.channels, t.rg_track_gain, t.rg_track_peak, t.rg_album_gain, t.rg_album_peak, \
         t.path IS NOT NULL \
         FROM playlist_items i LEFT JOIN tracks t ON t.path = i.path \
         WHERE i.playlist_id = ?1 ORDER BY i.position",
    )?;
    for (id, name) in heads {
        let tracks = items
            .query_map([id], |r| {
                if r.get::<_, bool>(19)? {
                    row_to_track(r)
                } else {
                    Ok(Track::from_path(get_path(r, 0)?))
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
            ins.execute(params![id, i as i64, path_bytes(&t.path)])?;
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
        [path_bytes(path)],
        |r| r.get(0),
    )
    .optional()
}

/// Drops every track mapping that points to `preset`.
pub fn eq_auto_forget_preset(conn: &Connection, preset: &str) -> rusqlite::Result<usize> {
    conn.execute("DELETE FROM eq_auto WHERE preset = ?1", [preset])
}

pub fn eq_auto_set(conn: &Connection, path: &Path, preset: Option<&str>) -> rusqlite::Result<()> {
    match preset {
        Some(p) => conn.execute(
            "INSERT INTO eq_auto (path, preset) VALUES (?1, ?2) \
             ON CONFLICT(path) DO UPDATE SET preset = excluded.preset",
            params![path_bytes(path), p],
        )?,
        None => conn.execute("DELETE FROM eq_auto WHERE path = ?1", [path_bytes(path)])?,
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
    fn v6_migration_tolerates_a_column_added_by_another_instance() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_V1).unwrap();
        conn.execute_batch("PRAGMA user_version = 5;").unwrap();
        conn.execute_batch("ALTER TABLE tracks ADD COLUMN rg_track_gain REAL;").unwrap();
        conn.execute(
            "INSERT INTO tracks (path, mtime, size) VALUES (CAST('/m/a.mp3' AS BLOB), 7, 2)",
            [],
        )
        .unwrap();
        migrate(&conn).unwrap();
        let v: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, SCHEMA_VERSION);
        let (n, mtime): (i64, i64) = conn
            .query_row("SELECT COUNT(*), MIN(mtime) FROM tracks", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((n, mtime), (1, -1));
        assert!(has_column(&conn, "tracks", "rg_album_peak").unwrap());
    }

    #[test]
    fn v6_step_skips_when_another_instance_finished_first() {
        // The caller read v5 before the lock; under the lock the DB is already v6.
        let conn = open_memory().unwrap();
        conn.execute(
            "INSERT INTO tracks (path, mtime, size) VALUES (CAST('/m/a.mp3' AS BLOB), 7, 2)",
            [],
        )
        .unwrap();
        migrate_v6(&conn).unwrap();
        let mtime: i64 = conn.query_row("SELECT mtime FROM tracks", [], |r| r.get(0)).unwrap();
        assert_eq!(mtime, 7, "a finished migration must not be redone");
    }

    #[test]
    fn migrate_twice_is_noop() {
        let conn = open_memory().unwrap();
        migrate(&conn).unwrap();
        let v: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, SCHEMA_VERSION);
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
    fn clear_seen_is_a_subtree_range() {
        let mut conn = open_memory().unwrap();
        let tx = conn.transaction().unwrap();
        for p in ["/m/x.mp3", "/m/sub/y.mp3", "/m2/z.mp3", "/other/c.mp3"] {
            upsert_track(&tx, &track(p, "t")).unwrap();
        }
        tx.commit().unwrap();
        clear_seen(&conn, &[PathBuf::from("/m/")]).unwrap();
        assert_eq!(delete_unseen(&conn).unwrap(), 2);
        let left: Vec<_> = load_tracks(&conn).unwrap().into_iter().map(|t| t.path).collect();
        assert_eq!(
            left,
            [PathBuf::from("/m2/z.mp3"), PathBuf::from("/other/c.mp3")]
        );
        clear_seen(&conn, &[PathBuf::from("/")]).unwrap();
        assert_eq!(delete_unseen(&conn).unwrap(), 2);
    }

    #[test]
    fn delete_outside_roots_keeps_only_roots() {
        let mut conn = open_memory().unwrap();
        let tx = conn.transaction().unwrap();
        for p in ["/m/a/x.mp3", "/m/ab/y.mp3", "/n/z.mp3"] {
            upsert_track(&tx, &track(p, "t")).unwrap();
        }
        tx.commit().unwrap();
        assert_eq!(delete_outside_roots(&conn, &[]).unwrap(), 0);
        assert_eq!(delete_outside_roots(&conn, &[PathBuf::from("/m/a")]).unwrap(), 2);
        let left: Vec<_> = load_tracks(&conn).unwrap().into_iter().map(|t| t.path).collect();
        assert_eq!(left, [PathBuf::from("/m/a/x.mp3")]);
    }

    #[test]
    fn delete_outside_roots_scopes_eq_auto_to_pruned_tracks() {
        let mut conn = open_memory().unwrap();
        let tx = conn.transaction().unwrap();
        for p in ["/m/a/x.mp3", "/n/z.mp3"] {
            upsert_track(&tx, &track(p, "t")).unwrap();
        }
        tx.commit().unwrap();
        for p in ["/m/a/x.mp3", "/n/z.mp3", "/elsewhere/free.mp3"] {
            conn.execute(
                "INSERT INTO eq_auto (path, preset) VALUES (?1, 'Pop')",
                params![p.as_bytes()],
            )
            .unwrap();
        }
        // Trailing slash root keeps its tracks.
        assert_eq!(delete_outside_roots(&conn, &[PathBuf::from("/m/a/")]).unwrap(), 1);
        let mut stmt = conn.prepare("SELECT path FROM eq_auto ORDER BY path").unwrap();
        let rows: Vec<Vec<u8>> =
            stmt.query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
        assert_eq!(rows, [b"/elsewhere/free.mp3".to_vec(), b"/m/a/x.mp3".to_vec()]);
    }

    #[test]
    fn refuses_newer_schema() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", 7).unwrap();
        assert!(migrate(&conn).is_err());
        let v: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, 7);
    }

    #[test]
    fn v5_migrates_to_v6_keeping_rows() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_V1).unwrap();
        conn.execute_batch(
            "INSERT INTO tracks (path, mtime, size, title) VALUES (CAST('/m/a.mp3' AS BLOB), 1234, 2, 'Known'); \
             INSERT INTO playlists (id, name, position) VALUES (1, 'Rock', 0); \
             INSERT INTO playlist_items (playlist_id, position, path) VALUES (1, 0, CAST('/m/a.mp3' AS BLOB)); \
             INSERT INTO eq_auto (path, preset) VALUES (CAST('/m/a.mp3' AS BLOB), 'Pop'); \
             PRAGMA user_version = 5;",
        )
        .unwrap();
        migrate(&conn).unwrap();
        let v: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, 6);
        let t = get_track(&conn, Path::new("/m/a.mp3")).unwrap().unwrap();
        assert_eq!(t.title.as_deref(), Some("Known"));
        assert_eq!(t.mtime, -1);
        assert_eq!(t.replay_gain, ReplayGain::default());
        let lists = load_playlists(&conn).unwrap();
        assert_eq!(lists[0].tracks[0].title.as_deref(), Some("Known"));
        assert_eq!(
            eq_auto_get(&conn, Path::new("/m/a.mp3")).unwrap().as_deref(),
            Some("Pop")
        );
        assert_eq!(stamps(&conn).unwrap()[Path::new("/m/a.mp3")], (-1, 2));
        assert!(crate::app::needs_rescan(&load_tracks(&conn).unwrap()));
    }

    #[test]
    fn replay_gain_roundtrip() {
        let mut conn = open_memory().unwrap();
        let full = ReplayGain {
            track_gain_db: Some(-6.54),
            track_peak: Some(0.988553),
            album_gain_db: Some(-7.1),
            album_peak: Some(1.0),
        };
        let part = ReplayGain { track_gain_db: Some(-3.0), ..Default::default() };
        let tx = conn.transaction().unwrap();
        upsert_track(&tx, &Track { replay_gain: full, ..track("/m/a.mp3", "A") }).unwrap();
        upsert_track(&tx, &Track { replay_gain: part, ..track("/m/b.mp3", "B") }).unwrap();
        tx.commit().unwrap();

        let rg = |conn: &Connection, p: &str| get_track(conn, Path::new(p)).unwrap().unwrap().replay_gain;
        assert_eq!(rg(&conn, "/m/a.mp3"), full);
        assert_eq!(rg(&conn, "/m/b.mp3"), part);
        let all = load_tracks(&conn).unwrap();
        assert_eq!(all[0].replay_gain, full);
        assert_eq!(all[1].replay_gain, part);

        let mut p = Playlist::new("Rock");
        p.add([Track::from_path("/m/a.mp3".into()), Track::from_path("/m/b.mp3".into())]);
        save_playlist(&mut conn, &mut p, 0).unwrap();
        let lists = load_playlists(&conn).unwrap();
        assert_eq!(lists[0].tracks[0].replay_gain, full);
        assert_eq!(lists[0].tracks[1].replay_gain, part);

        let tx = conn.transaction().unwrap();
        upsert_track(&tx, &track("/m/a.mp3", "A")).unwrap();
        tx.commit().unwrap();
        assert_eq!(rg(&conn, "/m/a.mp3"), ReplayGain::default());
    }

    #[test]
    fn v4_text_paths_migrate() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_V1).unwrap();
        conn.execute_batch(
            "INSERT INTO tracks (path, mtime, size, title) VALUES ('/m/a.mp3', 1, 2, 'Known'); \
             INSERT INTO playlists (id, name, position) VALUES (1, 'Rock', 0); \
             INSERT INTO playlist_items (playlist_id, position, path) VALUES (1, 0, '/m/a.mp3'); \
             INSERT INTO eq_auto (path, preset) VALUES ('/m/a.mp3', 'Pop'); \
             PRAGMA user_version = 4;",
        )
        .unwrap();
        migrate(&conn).unwrap();
        let t = get_track(&conn, Path::new("/m/a.mp3")).unwrap().unwrap();
        assert_eq!(t.title.as_deref(), Some("Known"));
        let lists = load_playlists(&conn).unwrap();
        assert_eq!(lists[0].tracks[0].title.as_deref(), Some("Known"));
        assert_eq!(
            eq_auto_get(&conn, Path::new("/m/a.mp3")).unwrap().as_deref(),
            Some("Pop")
        );
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name = 'idx_tracks_artist_album'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn non_utf8_path_roundtrip() {
        let mut conn = open_memory().unwrap();
        let p = PathBuf::from(std::ffi::OsStr::from_bytes(b"/m/bad\xff.mp3"));
        let tx = conn.transaction().unwrap();
        upsert_track(&tx, &Track { path: p.clone(), ..track("x", "bad") }).unwrap();
        tx.commit().unwrap();
        assert!(get_track(&conn, &p).unwrap().is_some());
        assert_eq!(stamps(&conn).unwrap()[&p], (1, 2));
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

    #[test]
    fn eq_auto_forget_preset_removes_only_that_preset() {
        let conn = open_memory().unwrap();
        let (a, b) = (Path::new("/m/a.mp3"), Path::new("/m/b.mp3"));
        eq_auto_set(&conn, a, Some("Rock")).unwrap();
        eq_auto_set(&conn, b, Some("Pop")).unwrap();
        assert_eq!(eq_auto_forget_preset(&conn, "Rock").unwrap(), 1);
        assert_eq!(eq_auto_get(&conn, a).unwrap(), None);
        assert_eq!(eq_auto_get(&conn, b).unwrap().as_deref(), Some("Pop"));
    }

    #[test]
    fn delete_unseen_drops_eq_auto_rows() {
        let mut conn = open_memory().unwrap();
        let tx = conn.transaction().unwrap();
        upsert_track(&tx, &track("/m/a.mp3", "A")).unwrap();
        upsert_track(&tx, &track("/m/b.mp3", "B")).unwrap();
        tx.commit().unwrap();
        eq_auto_set(&conn, Path::new("/m/a.mp3"), Some("Rock")).unwrap();
        eq_auto_set(&conn, Path::new("/m/b.mp3"), Some("Pop")).unwrap();
        clear_seen(&conn, &[PathBuf::from("/m")]).unwrap();
        let tx = conn.transaction().unwrap();
        mark_seen(&tx, Path::new("/m/b.mp3")).unwrap();
        tx.commit().unwrap();
        assert_eq!(delete_unseen(&conn).unwrap(), 1);
        assert_eq!(eq_auto_get(&conn, Path::new("/m/a.mp3")).unwrap(), None);
        assert_eq!(eq_auto_get(&conn, Path::new("/m/b.mp3")).unwrap().as_deref(), Some("Pop"));
    }
}
