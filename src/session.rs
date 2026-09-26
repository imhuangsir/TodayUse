use crate::model::{ActivityEvent, MediaEvent, Session};

fn same_key(s: &Session, ev: &ActivityEvent) -> bool {
    s.app_name == ev.app_name
        && s.window_title == ev.window_title
        && s.edge_url == ev.edge_url
        && s.is_private == ev.is_private
        && s.is_idle == ev.is_idle
}

fn session_from_event(ev: &ActivityEvent) -> Session {
    Session {
        start_ts: ev.ts,
        end_ts: ev.ts,
        duration_sec: 0,
        app_name: ev.app_name.clone(),
        process_path: ev.process_path.clone(),
        window_title: ev.window_title.clone(),
        edge_url: ev.edge_url.clone(),
        page_title: ev.page_title.clone(),
        media_title: None,
        media_player: None,
        media_status: None,
        media_position_sec: None,
        media_duration_sec: None,
        is_private: ev.is_private,
        is_idle: ev.is_idle,
    }
}

/// 把事件流合并为会话。切分键变化时结算上一段并开新段。
#[derive(Default)]
pub struct SessionBuilder {
    current: Option<Session>,
}

impl SessionBuilder {
    pub fn new() -> Self {
        Self { current: None }
    }

    /// 处理活动事件；若发生切换，返回被结算的旧会话。
    pub fn on_activity(&mut self, ev: &ActivityEvent) -> Option<Session> {
        let changed = self.current.as_ref().is_none_or(|c| !same_key(c, ev));
        if changed {
            let closed = self.current.take().map(|mut s| {
                s.close(ev.ts);
                s
            });
            self.current = Some(session_from_event(ev));
            closed
        } else {
            if let Some(c) = self.current.as_mut() {
                c.close(ev.ts);
            }
            None
        }
    }

    /// 媒体事件富化当前会话（标题/状态/进度）。
    pub fn on_media(&mut self, m: &MediaEvent) {
        if let Some(c) = self.current.as_mut() {
            c.media_title = Some(m.media_title.clone());
            c.media_player = Some(m.media_player.clone());
            c.media_status = Some(m.media_status);
            c.media_position_sec = m.position_sec;
            c.media_duration_sec = m.duration_sec;
        }
    }

    /// 心跳：把当前会话 end_ts 延长到 ts（不结算）。
    pub fn touch(&mut self, ts: i64) {
        if let Some(c) = self.current.as_mut() {
            c.close(ts);
        }
    }

    /// 结算并取出当前会话（暂停/退出时用）。
    pub fn finish(&mut self, ts: i64) -> Option<Session> {
        self.current.take().map(|mut s| {
            s.close(ts);
            s
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PlaybackStatus;

    fn act(ts: i64, app: &str, win: &str) -> ActivityEvent {
        ActivityEvent {
            ts,
            app_name: app.into(),
            process_path: "x".into(),
            window_title: win.into(),
            edge_url: None,
            page_title: None,
            is_private: false,
            is_idle: false,
        }
    }

    #[test]
    fn same_activity_extends() {
        let mut b = SessionBuilder::new();
        assert!(b.on_activity(&act(1000, "Code", "main.rs")).is_none());
        assert!(b.on_activity(&act(1030, "Code", "main.rs")).is_none());
        let s = b.finish(1060).unwrap();
        assert_eq!(s.start_ts, 1000);
        assert_eq!(s.duration_sec, 60);
    }

    #[test]
    fn key_change_closes_previous() {
        let mut b = SessionBuilder::new();
        b.on_activity(&act(1000, "Code", "main.rs"));
        let closed = b.on_activity(&act(1040, "Edge", "GitHub")).unwrap();
        assert_eq!(closed.app_name, "Code");
        assert_eq!(closed.end_ts, 1040);
        assert_eq!(closed.duration_sec, 40);
        let s = b.finish(1100).unwrap();
        assert_eq!(s.app_name, "Edge");
        assert_eq!(s.duration_sec, 60);
    }

    #[test]
    fn idle_flag_splits_session() {
        let mut b = SessionBuilder::new();
        b.on_activity(&act(1000, "Code", "main.rs"));
        let mut idle_ev = act(1050, "Code", "main.rs");
        idle_ev.is_idle = true;
        let closed = b.on_activity(&idle_ev).unwrap();
        assert!(!closed.is_idle);
        assert_eq!(closed.duration_sec, 50);
    }

    #[test]
    fn media_enriches_current() {
        let mut b = SessionBuilder::new();
        b.on_activity(&act(2000, "Edge", "vid - YouTube - Microsoft Edge"));
        b.on_media(&MediaEvent {
            ts: 2005,
            media_title: "vid".into(),
            media_player: "msedge".into(),
            media_status: PlaybackStatus::Playing,
            position_sec: Some(30),
            duration_sec: Some(600),
        });
        let s = b.finish(2100).unwrap();
        assert_eq!(s.media_title.as_deref(), Some("vid"));
        assert_eq!(s.media_position_sec, Some(30));
        assert_eq!(s.media_status, Some(PlaybackStatus::Playing));
    }
}
