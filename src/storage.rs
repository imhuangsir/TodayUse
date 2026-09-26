use crate::model::{PlaybackStatus, Session};
use rusqlite::{params, Connection, Row};

const SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS activity (
  id INTEGER PRIMARY KEY,
  start_ts INTEGER NOT NULL, end_ts INTEGER NOT NULL, duration_sec INTEGER NOT NULL,
  app_name TEXT NOT NULL, process_path TEXT NOT NULL, window_title TEXT NOT NULL,
  edge_url TEXT, page_title TEXT,
  media_title TEXT, media_player TEXT, media_status TEXT,
  media_position_sec INTEGER, media_duration_sec INTEGER,
  is_private INTEGER NOT NULL, is_idle INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS idx_activity_start ON activity(start_ts);
CREATE INDEX IF NOT EXISTS idx_activity_app_start ON activity(app_name, start_ts);
CREATE TABLE IF NOT EXISTS summaries (
  id INTEGER PRIMARY KEY, range_start INTEGER NOT NULL, range_end INTEGER NOT NULL,
  granularity TEXT NOT NULL, text TEXT NOT NULL, created_ts INTEGER NOT NULL, model TEXT);
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);";

fn status_to_str(s: PlaybackStatus) -> &'static str {
    match s {
        PlaybackStatus::Playing => "playing",
        PlaybackStatus::Paused => "paused",
        PlaybackStatus::Stopped => "stopped",
    }
}
fn status_from_str(s: &str) -> Option<PlaybackStatus> {
    match s {
        "playing" => Some(PlaybackStatus::Playing),
        "paused" => Some(PlaybackStatus::Paused),
        "stopped" => Some(PlaybackStatus::Stopped),
        _ => None,
    }
}

pub struct Storage {
    conn: Connection,
}

impl Storage {
    pub fn open(path: &str) -> Result<Storage, String> {
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.pragma_update(None, "journal_mode", "WAL").map_err(|e| e.to_string())?;
        conn.pragma_update(None, "synchronous", "NORMAL").map_err(|e| e.to_string())?;
        conn.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
        Ok(Storage { conn })
    }

    #[cfg(test)]
    pub fn open_memory() -> Result<Storage, String> {
        let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
        conn.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
        Ok(Storage { conn })
    }

    pub fn insert_sessions(&mut self, sessions: &[Session]) -> Result<(), String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        {
            let mut stmt = tx
                .prepare_cached(
                    "INSERT INTO activity (start_ts,end_ts,duration_sec,app_name,process_path,window_title,\
                     edge_url,page_title,media_title,media_player,media_status,media_position_sec,\
                     media_duration_sec,is_private,is_idle) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
                )
                .map_err(|e| e.to_string())?;
            for s in sessions {
                stmt.execute(params![
                    s.start_ts, s.end_ts, s.duration_sec, s.app_name, s.process_path, s.window_title,
                    s.edge_url, s.page_title, s.media_title, s.media_player,
                    s.media_status.map(status_to_str), s.media_position_sec, s.media_duration_sec,
                    s.is_private as i64, s.is_idle as i64
                ])
                .map_err(|e| e.to_string())?;
            }
        }
        tx.commit().map_err(|e| e.to_string())
    }
}

fn row_to_session(r: &Row) -> rusqlite::Result<Session> {
    let status: Option<String> = r.get("media_status")?;
    Ok(Session {
        start_ts: r.get("start_ts")?,
        end_ts: r.get("end_ts")?,
        duration_sec: r.get("duration_sec")?,
        app_name: r.get("app_name")?,
        process_path: r.get("process_path")?,
        window_title: r.get("window_title")?,
        edge_url: r.get("edge_url")?,
        page_title: r.get("page_title")?,
        media_title: r.get("media_title")?,
        media_player: r.get("media_player")?,
        media_status: status.as_deref().and_then(status_from_str),
        media_position_sec: r.get("media_position_sec")?,
        media_duration_sec: r.get("media_duration_sec")?,
        is_private: r.get::<_, i64>("is_private")? != 0,
        is_idle: r.get::<_, i64>("is_idle")? != 0,
    })
}

impl Storage {
    /// 与 [start,end) 有重叠的会话，按开始时间升序。
    pub fn sessions_in_range(&self, start: i64, end: i64) -> Result<Vec<Session>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM activity WHERE start_ts < ? AND end_ts > ? ORDER BY start_ts")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![end, start], row_to_session)
            .map_err(|e| e.to_string())?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sess(start: i64, end: i64, app: &str) -> Session {
        let mut s = Session {
            start_ts: start,
            end_ts: end,
            duration_sec: end - start,
            app_name: app.into(),
            process_path: "p".into(),
            window_title: "w".into(),
            edge_url: None,
            page_title: None,
            media_title: None,
            media_player: None,
            media_status: None,
            media_position_sec: None,
            media_duration_sec: None,
            is_private: false,
            is_idle: false,
        };
        s.close(end);
        s
    }

    #[test]
    fn insert_and_query_roundtrip() {
        let mut st = Storage::open_memory().unwrap();
        let mut a = sess(1000, 1100, "Code");
        a.media_title = Some("v".into());
        a.media_status = Some(PlaybackStatus::Playing);
        a.media_position_sec = Some(42);
        st.insert_sessions(&[a.clone(), sess(2000, 2050, "Edge")]).unwrap();
        let got = st.sessions_in_range(900, 1500).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].app_name, "Code");
        assert_eq!(got[0].media_status, Some(PlaybackStatus::Playing));
        assert_eq!(got[0].media_position_sec, Some(42));
    }

    #[test]
    fn range_overlap_is_inclusive_of_partial() {
        let mut st = Storage::open_memory().unwrap();
        st.insert_sessions(&[sess(1000, 2000, "A")]).unwrap();
        assert_eq!(st.sessions_in_range(1500, 3000).unwrap().len(), 1);
        assert_eq!(st.sessions_in_range(3000, 4000).unwrap().len(), 0);
    }
}
