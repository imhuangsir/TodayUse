use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaybackStatus { Playing, Paused, Stopped }

/// 采集器发给存储线程的前台活动事件（时间为 UTC 秒）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActivityEvent {
    pub ts: i64,
    pub app_name: String,
    pub process_path: String,
    pub window_title: String,
    pub edge_url: Option<String>,
    pub page_title: Option<String>,
    pub is_private: bool,
    pub is_idle: bool,
}

/// 媒体事件，更新当前会话的 media_* 字段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaEvent {
    pub ts: i64,
    pub media_title: String,
    pub media_player: String,
    pub media_status: PlaybackStatus,
    pub position_sec: Option<i64>,
    pub duration_sec: Option<i64>,
}

/// 一段合并后的活动会话（对应 activity 表一行）。
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub start_ts: i64,
    pub end_ts: i64,
    pub duration_sec: i64,
    pub app_name: String,
    pub process_path: String,
    pub window_title: String,
    pub edge_url: Option<String>,
    pub page_title: Option<String>,
    pub media_title: Option<String>,
    pub media_player: Option<String>,
    pub media_status: Option<PlaybackStatus>,
    pub media_position_sec: Option<i64>,
    pub media_duration_sec: Option<i64>,
    pub is_private: bool,
    pub is_idle: bool,
}

impl Session {
    /// 用 end_ts 结算会话，重算 duration（不为负）。
    pub fn close(&mut self, end_ts: i64) {
        self.end_ts = end_ts;
        self.duration_sec = (end_ts - self.start_ts).max(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Session {
        Session {
            start_ts: 1000, end_ts: 1000, duration_sec: 0,
            app_name: "Code".into(), process_path: "C:/code.exe".into(),
            window_title: "main.rs".into(), edge_url: None, page_title: None,
            media_title: None, media_player: None, media_status: None,
            media_position_sec: None, media_duration_sec: None,
            is_private: false, is_idle: false,
        }
    }

    #[test]
    fn close_computes_duration() {
        let mut s = sample();
        s.close(1090);
        assert_eq!(s.end_ts, 1090);
        assert_eq!(s.duration_sec, 90);
    }

    #[test]
    fn close_never_negative() {
        let mut s = sample();
        s.close(900);
        assert_eq!(s.duration_sec, 0);
    }
}
