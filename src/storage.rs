use crate::model::{PlaybackStatus, Session};
use rusqlite::{params, Connection, OptionalExtension, Row};

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
        // 多连接并发（采集守护写 + 仪表盘/总结读写同一库）：等锁 5s 而非立即报 BUSY。
        conn.busy_timeout(std::time::Duration::from_secs(5)).map_err(|e| e.to_string())?;
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

impl Storage {
    pub fn set_meta(&self, key: &str, value: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![key, value],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn get_meta(&self, key: &str) -> Result<Option<String>, String> {
        self.conn
            .query_row("SELECT value FROM meta WHERE key=?", params![key], |r| r.get(0))
            .optional()
            .map_err(|e| e.to_string())
    }
    pub fn set_heartbeat(&self, ts: i64) -> Result<(), String> {
        self.set_meta("heartbeat", &ts.to_string())
    }
    pub fn get_heartbeat(&self) -> Result<Option<i64>, String> {
        Ok(self.get_meta("heartbeat")?.and_then(|v| v.parse::<i64>().ok()))
    }
    /// 把 end_ts 超过心跳 + grace 的会话夹回心跳时刻，返回修正行数。
    pub fn recover_phantom(&self, grace_sec: i64) -> Result<usize, String> {
        let h = match self.get_heartbeat()? {
            Some(h) => h,
            None => return Ok(0),
        };
        let n = self
            .conn
            .execute(
                "UPDATE activity SET end_ts = ?1, duration_sec = MAX(0, ?1 - start_ts) WHERE end_ts > ?1 + ?2",
                params![h, grace_sec],
            )
            .map_err(|e| e.to_string())?;
        Ok(n)
    }
}

impl Storage {
    /// 写入一条 AI 总结。
    pub fn insert_summary(
        &self,
        range_start: i64,
        range_end: i64,
        granularity: &str,
        text: &str,
        model: &str,
    ) -> Result<(), String> {
        let created = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        self.conn
            .execute(
                "INSERT INTO summaries (range_start,range_end,granularity,text,created_ts,model) \
                 VALUES (?,?,?,?,?,?)",
                params![range_start, range_end, granularity, text, created, model],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 取某日范围[start,end)最近生成的一条总结文本（按生成时间、其次自增 id 倒序，避免同秒并列）。
    pub fn latest_summary(&self, range_start: i64, range_end: i64) -> Result<Option<String>, String> {
        self.conn
            .query_row(
                "SELECT text FROM summaries WHERE range_start = ?1 AND range_end = ?2 \
                 ORDER BY created_ts DESC, id DESC LIMIT 1",
                params![range_start, range_end],
                |r| r.get(0),
            )
            .optional()
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

    #[test]
    fn meta_roundtrip() {
        let st = Storage::open_memory().unwrap();
        assert_eq!(st.get_meta("x").unwrap(), None);
        st.set_meta("x", "42").unwrap();
        st.set_meta("x", "43").unwrap();
        assert_eq!(st.get_meta("x").unwrap().as_deref(), Some("43"));
    }

    #[test]
    fn recover_clamps_phantom_tail() {
        let mut st = Storage::open_memory().unwrap();
        st.insert_sessions(&[sess(1000, 5000, "Crashed"), sess(1000, 2000, "Fine")]).unwrap();
        st.set_heartbeat(3000).unwrap();
        let fixed = st.recover_phantom(60).unwrap();
        assert_eq!(fixed, 1);
        let all = st.sessions_in_range(0, 10000).unwrap();
        let crashed = all.iter().find(|s| s.app_name == "Crashed").unwrap();
        assert_eq!(crashed.end_ts, 3000);
        assert_eq!(crashed.duration_sec, 2000);
    }

    #[test]
    fn summary_latest_returns_newest_for_range() {
        let st = Storage::open_memory().unwrap();
        assert_eq!(st.latest_summary(0, 100).unwrap(), None);
        st.insert_summary(0, 100, "day", "旧总结", "m").unwrap();
        st.insert_summary(0, 100, "day", "新总结", "m").unwrap();
        st.insert_summary(0, 100, "day", "别的范围", "m").unwrap(); // 同范围仍取最新
        st.insert_summary(200, 300, "day", "另一天", "m").unwrap();
        assert_eq!(st.latest_summary(0, 100).unwrap().as_deref(), Some("别的范围"));
        assert_eq!(st.latest_summary(200, 300).unwrap().as_deref(), Some("另一天"));
        assert_eq!(st.latest_summary(999, 1000).unwrap(), None);
    }
}
