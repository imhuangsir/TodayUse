//! SMTC（系统媒体传输控件）媒体快照。所有 WinRT 调用失败一律降级为 None，绝不 panic。
#![cfg(windows)]

use crate::model::PlaybackStatus;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSessionManager as Mgr,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};

pub struct MediaSnapshot {
    pub title: String,
    pub player: String,
    pub status: PlaybackStatus,
    pub position_sec: Option<i64>,
    pub duration_sec: Option<i64>,
}

thread_local! {
    // 媒体会话管理器创建一次即可复用，避免每轮 RequestAsync().get() 的开销。
    static MGR: std::cell::RefCell<Option<Mgr>> = const { std::cell::RefCell::new(None) };
}

/// 当前系统媒体会话快照（YouTube/B站/播放器等上报 SMTC 时可得）。
pub fn media_snapshot() -> Option<MediaSnapshot> {
    let session = MGR.with(|cell| {
        let mut m = cell.borrow_mut();
        if m.is_none() {
            *m = Mgr::RequestAsync().ok()?.get().ok();
        }
        m.as_ref()?.GetCurrentSession().ok()
    })?;

    let props = session.TryGetMediaPropertiesAsync().ok()?.get().ok()?;
    let title = props.Title().ok()?.to_string();
    if title.is_empty() {
        return None;
    }
    let player = session
        .SourceAppUserModelId()
        .map(|h| h.to_string())
        .unwrap_or_default();

    let ps = session.GetPlaybackInfo().ok()?.PlaybackStatus().ok()?;
    let status = if ps == Status::Playing {
        PlaybackStatus::Playing
    } else if ps == Status::Paused {
        PlaybackStatus::Paused
    } else {
        PlaybackStatus::Stopped
    };

    let (position_sec, duration_sec) = match session.GetTimelineProperties() {
        Ok(tl) => {
            let pos = tl.Position().map(|d| d.Duration / 10_000_000).ok();
            let end = tl.EndTime().map(|d| d.Duration / 10_000_000).ok();
            (pos.filter(|&p| p >= 0), end.filter(|&e| e > 0))
        }
        Err(_) => (None, None),
    };

    Some(MediaSnapshot {
        title,
        player,
        status,
        position_sec,
        duration_sec,
    })
}
