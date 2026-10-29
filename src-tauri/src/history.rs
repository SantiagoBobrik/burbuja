//! Dictation history in SQLite (`{app_data}/history.db`).
//! Local only, never audio, with retention and full delete.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, params};
use rusqlite_migration::{M, Migrations};
use serde::Serialize;
use specta::Type;

const MIGRATIONS: &[M<'static>] = &[M::up(
    "CREATE TABLE dictations (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        created_at INTEGER NOT NULL,      -- unix ms
        text TEXT NOT NULL,
        raw TEXT NOT NULL,
        engine TEXT NOT NULL,
        duration_ms INTEGER NOT NULL,     -- spoken audio
        latency_ms INTEGER NOT NULL,
        words INTEGER NOT NULL,
        target_app TEXT
    );
    CREATE INDEX dictations_created ON dictations(created_at);",
)];

/// Typing speed used for "time saved".
const TYPING_WPM: f64 = 40.0;

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HistoryItem {
    pub id: f64,
    pub created_at: f64,
    pub text: String,
    pub raw: String,
    pub engine: String,
    pub duration_ms: f64,
    pub latency_ms: f64,
    pub words: f64,
    pub target_app: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub words_this_week: f64,
    /// Words per spoken minute, over the last 7 days.
    pub average_wpm: f64,
    /// Minutes saved this week vs typing at 40 wpm.
    pub minutes_saved: f64,
    pub dictations_this_week: f64,
}

pub struct NewDictation<'a> {
    pub created_at: i64,
    pub text: &'a str,
    pub raw: &'a str,
    pub engine: &'a str,
    pub duration_ms: i64,
    pub latency_ms: i64,
    pub target_app: Option<&'a str>,
}

#[derive(Debug, thiserror::Error)]
#[error("history: {0}")]
pub struct HistoryError(String);

impl From<rusqlite::Error> for HistoryError {
    fn from(e: rusqlite::Error) -> Self {
        HistoryError(e.to_string())
    }
}

pub struct History {
    conn: Mutex<Connection>,
    /// "Keep audio" WAVs (`{epoch_ms}.wav`), next to the database. They follow the history:
    /// deleting, clearing or pruning dictations deletes their audio too.
    audio_dir: Option<PathBuf>,
}

/// Margin before a dictation's release when matching its WAV name on delete.
const AUDIO_NAME_SLACK_MS: i64 = 2_000;

/// Wall-clock Unix time in ms: the unit of `created_at`, retention and stats.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

pub fn word_count(text: &str) -> i64 {
    text.split_whitespace().count() as i64
}

impl History {
    pub fn open(path: &Path) -> Result<Self, HistoryError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| HistoryError(e.to_string()))?;
        }
        let mut history = Self::with_connection(Connection::open(path)?)?;
        history.audio_dir = path.parent().map(|d| d.join("audio"));
        Ok(history)
    }

    /// Where "Keep audio" writes its WAVs.
    pub fn audio_dir(&self) -> Option<&Path> {
        self.audio_dir.as_deref()
    }

    pub fn in_memory() -> Result<Self, HistoryError> {
        Self::with_connection(Connection::open_in_memory()?)
    }

    fn with_connection(mut conn: Connection) -> Result<Self, HistoryError> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        // Deleted dictations are overwritten with zeros instead of lingering in free pages.
        conn.pragma_update(None, "secure_delete", "ON")?;
        Migrations::from_slice(MIGRATIONS)
            .to_latest(&mut conn)
            .map_err(|e| HistoryError(e.to_string()))?;
        Ok(Self {
            conn: Mutex::new(conn),
            audio_dir: None,
        })
    }

    pub fn insert(&self, d: &NewDictation) -> Result<i64, HistoryError> {
        let conn = self.conn.lock().expect("history");
        conn.execute(
            "INSERT INTO dictations (created_at, text, raw, engine, duration_ms, latency_ms, words, target_app)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                d.created_at,
                d.text,
                d.raw,
                d.engine,
                d.duration_ms,
                d.latency_ms,
                word_count(d.text),
                d.target_app
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Newest first. `before` is the id to page from (exclusive); `query` matches text or raw.
    pub fn page(
        &self,
        before: Option<i64>,
        limit: u32,
        query: &str,
    ) -> Result<Vec<HistoryItem>, HistoryError> {
        let conn = self.conn.lock().expect("history");
        let like = format!(
            "%{}%",
            query
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let mut stmt = conn.prepare(
            "SELECT id, created_at, text, raw, engine, duration_ms, latency_ms, words, target_app
             FROM dictations
             WHERE (?1 IS NULL OR id < ?1)
               AND (?2 = '' OR text LIKE ?3 ESCAPE '\\' OR raw LIKE ?3 ESCAPE '\\')
             ORDER BY id DESC LIMIT ?4",
        )?;
        let rows = stmt.query_map(params![before, query, like, limit.min(500)], row_to_item)?;
        rows.collect::<Result<_, _>>().map_err(Into::into)
    }

    pub fn get(&self, id: i64) -> Result<Option<HistoryItem>, HistoryError> {
        let conn = self.conn.lock().expect("history");
        conn.query_row(
            "SELECT id, created_at, text, raw, engine, duration_ms, latency_ms, words, target_app
             FROM dictations WHERE id = ?1",
            [id],
            row_to_item,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn update_text(&self, id: i64, text: &str) -> Result<(), HistoryError> {
        let conn = self.conn.lock().expect("history");
        conn.execute(
            "UPDATE dictations SET text = ?2, words = ?3 WHERE id = ?1",
            params![id, text, word_count(text)],
        )?;
        Ok(())
    }

    pub fn delete(&self, id: i64) -> Result<(), HistoryError> {
        let item = self.get(id)?;
        let conn = self.conn.lock().expect("history");
        conn.execute("DELETE FROM dictations WHERE id = ?1", [id])?;
        scrub_wal(&conn)?;
        if let Some(item) = item {
            // The WAV is named when recording stops, before the transcription finishes.
            let end = item.created_at as i64;
            let start = end - item.latency_ms as i64 - AUDIO_NAME_SLACK_MS;
            self.remove_audio(|t| (start..=end).contains(&t));
        }
        Ok(())
    }

    pub fn clear(&self) -> Result<(), HistoryError> {
        let conn = self.conn.lock().expect("history");
        conn.execute("DELETE FROM dictations", [])?;
        conn.execute_batch("VACUUM")?;
        scrub_wal(&conn)?;
        self.remove_audio(|_| true);
        Ok(())
    }

    /// Deletes rows older than `days` (0 = keep forever).
    pub fn prune(&self, days: u32, now_ms: i64) -> Result<usize, HistoryError> {
        if days == 0 {
            return Ok(0);
        }
        let cutoff = now_ms - i64::from(days) * 86_400_000;
        let conn = self.conn.lock().expect("history");
        let n = conn.execute("DELETE FROM dictations WHERE created_at < ?1", [cutoff])?;
        if n > 0 {
            scrub_wal(&conn)?;
        }
        self.remove_audio(|t| t < cutoff);
        Ok(n)
    }

    /// Deletes kept WAVs whose timestamp name matches `doomed`. Other files are left alone.
    fn remove_audio(&self, doomed: impl Fn(i64) -> bool) -> usize {
        let Some(entries) = self
            .audio_dir
            .as_ref()
            .and_then(|d| std::fs::read_dir(d).ok())
        else {
            return 0;
        };
        let mut removed = 0;
        for path in entries.flatten().map(|e| e.path()) {
            let stamp = path
                .extension()
                .filter(|e| e.eq_ignore_ascii_case("wav"))
                .and_then(|_| path.file_stem()?.to_str()?.parse::<i64>().ok());
            if stamp.is_some_and(&doomed) {
                match std::fs::remove_file(&path) {
                    Ok(()) => removed += 1,
                    Err(e) => log::warn!("could not delete kept audio: {e}"),
                }
            }
        }
        removed
    }

    pub fn stats(&self, now_ms: i64) -> Result<Stats, HistoryError> {
        let since = now_ms - 7 * 86_400_000;
        let (words, spoken_ms, count): (i64, i64, i64) =
            self.conn.lock().expect("history").query_row(
                "SELECT COALESCE(SUM(words), 0), COALESCE(SUM(duration_ms), 0), COUNT(*)
             FROM dictations WHERE created_at >= ?1",
                [since],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
        Ok(stats_from(words, spoken_ms, count))
    }
}

fn stats_from(words: i64, spoken_ms: i64, count: i64) -> Stats {
    let spoken_min = spoken_ms as f64 / 60_000.0;
    let typing_min = words as f64 / TYPING_WPM;
    Stats {
        words_this_week: words as f64,
        average_wpm: if spoken_min > 0.0 {
            (words as f64 / spoken_min).round()
        } else {
            0.0
        },
        minutes_saved: (typing_min - spoken_min).max(0.0),
        dictations_this_week: count as f64,
    }
}

/// With `secure_delete` the database file no longer holds deleted text, but the write-ahead
/// log still has the pages as they were before the delete: checkpoint it and truncate it.
fn scrub_wal(conn: &Connection) -> Result<(), HistoryError> {
    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
    Ok(())
}

fn row_to_item(r: &rusqlite::Row) -> rusqlite::Result<HistoryItem> {
    Ok(HistoryItem {
        id: r.get::<_, i64>(0)? as f64,
        created_at: r.get::<_, i64>(1)? as f64,
        text: r.get(2)?,
        raw: r.get(3)?,
        engine: r.get(4)?,
        duration_ms: r.get::<_, i64>(5)? as f64,
        latency_ms: r.get::<_, i64>(6)? as f64,
        words: r.get::<_, i64>(7)? as f64,
        target_app: r.get(8)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400_000;

    fn add(h: &History, at: i64, text: &str, ms: i64) -> i64 {
        h.insert(&NewDictation {
            created_at: at,
            text,
            raw: text,
            engine: "whisper-large-v3-turbo",
            duration_ms: ms,
            latency_ms: 300,
            target_app: Some("notepad"),
        })
        .unwrap()
    }

    #[test]
    fn insert_page_and_search() {
        let h = History::in_memory().unwrap();
        for i in 0..5 {
            add(&h, i * 1000, &format!("note {i} about Córdoba"), 2000);
        }
        add(&h, 9000, "100% done_now", 1000);
        let first = h.page(None, 3, "").unwrap();
        assert_eq!(first.len(), 3);
        assert!(first[0].id > first[1].id, "newest first");
        let next = h.page(Some(first[2].id as i64), 10, "").unwrap();
        assert_eq!(next.len(), 3);
        assert_eq!(
            h.page(None, 10, "córdoba").unwrap().len(),
            5,
            "ASCII letters match any case"
        );
        assert_eq!(
            h.page(None, 10, "CÓRDOBA").unwrap().len(),
            0,
            "SQLite LIKE folds ASCII case only"
        );
        assert_eq!(h.page(None, 10, "100%").unwrap().len(), 1, "% is escaped");
        assert_eq!(h.page(None, 10, "e_n").unwrap().len(), 1, "_ is escaped");
    }

    /// Every byte of the database and its WAL, to look for leftovers of deleted dictations.
    fn on_disk(db: &Path) -> Vec<u8> {
        let mut bytes = std::fs::read(db).unwrap_or_default();
        bytes.extend(std::fs::read(db.with_extension("db-wal")).unwrap_or_default());
        bytes
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    #[test]
    fn deleted_dictations_do_not_linger_on_disk() {
        let dir = std::env::temp_dir().join(format!("burbuja-history-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let db = dir.join("history.db");
        let h = History::open(&db).unwrap();
        let secret = "zebra quartz confidential phrase";
        let id = add(&h, 0, secret, 1000);
        add(&h, 1, "keep me", 1000);
        h.delete(id).unwrap();
        assert!(!contains(&on_disk(&db), secret.as_bytes()), "single delete");

        add(&h, 2, secret, 1000);
        h.clear().unwrap();
        assert!(!contains(&on_disk(&db), secret.as_bytes()), "clear");
        assert!(!contains(&on_disk(&db), b"keep me"), "clear");
        drop(h);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn kept_audio_follows_the_history() {
        let dir = std::env::temp_dir().join(format!("burbuja-audio-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let h = History::open(&dir.join("history.db")).unwrap();
        let audio = h.audio_dir().unwrap().to_path_buf();
        std::fs::create_dir_all(&audio).unwrap();
        let wav = |t: i64| audio.join(format!("{t}.wav"));
        let day = 86_400_000;
        let now = 100 * day;
        for t in [now - 10 * day, now - 5_000, now - 100, now + 5_000] {
            std::fs::write(wav(t), b"RIFF").unwrap();
        }
        std::fs::write(audio.join("notes.txt"), b"mine").unwrap();

        // Deleting a dictation deletes the WAV named when its recording stopped.
        let id = add(&h, now - 100 + 300, "x", 1000);
        h.delete(id).unwrap();
        assert!(!wav(now - 100).exists());
        assert!(wav(now - 5_000).exists() && wav(now + 5_000).exists());

        // Retention deletes old audio only.
        h.prune(7, now).unwrap();
        assert!(!wav(now - 10 * day).exists());
        assert!(wav(now + 5_000).exists());

        // Clear deletes all kept audio, never other files.
        h.clear().unwrap();
        assert!(!wav(now + 5_000).exists() && !wav(now - 5_000).exists());
        assert!(audio.join("notes.txt").exists());
        drop(h);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn update_delete_clear() {
        let h = History::in_memory().unwrap();
        let id = add(&h, 0, "one two", 1000);
        h.update_text(id, "one two three").unwrap();
        assert_eq!(h.get(id).unwrap().unwrap().words, 3.0);
        h.delete(id).unwrap();
        assert!(h.get(id).unwrap().is_none());
        add(&h, 0, "x", 1);
        h.clear().unwrap();
        assert!(h.page(None, 10, "").unwrap().is_empty());
    }

    #[test]
    fn prune_respects_retention_and_forever() {
        let h = History::in_memory().unwrap();
        let now = 100 * DAY;
        add(&h, now - 40 * DAY, "old", 1000);
        add(&h, now - DAY, "new", 1000);
        assert_eq!(h.prune(0, now).unwrap(), 0, "0 = forever");
        assert_eq!(h.prune(30, now).unwrap(), 1);
        assert_eq!(h.page(None, 10, "").unwrap().len(), 1);
    }

    #[test]
    fn stats_count_the_last_seven_days() {
        let h = History::in_memory().unwrap();
        let now = 100 * DAY;
        add(&h, now - 10 * DAY, "too old to count at all", 60_000);
        // 120 words spoken in 1 minute.
        add(&h, now - DAY, &"word ".repeat(120), 60_000);
        let s = h.stats(now).unwrap();
        assert_eq!(s.words_this_week, 120.0);
        assert_eq!(s.dictations_this_week, 1.0);
        assert_eq!(s.average_wpm, 120.0);
        assert!(
            (s.minutes_saved - 2.0).abs() < 1e-9,
            "120/40 = 3 min typing − 1 min speaking"
        );
    }

    #[test]
    fn empty_stats_are_zero() {
        assert_eq!(stats_from(0, 0, 0).average_wpm, 0.0);
        assert_eq!(
            stats_from(10, 600_000, 1).minutes_saved,
            0.0,
            "never negative"
        );
    }
}
