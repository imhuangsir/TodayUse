//! 采集逻辑：纯函数（可单测）+ 轮询采集循环（Windows）。
use crate::model::ActivityEvent;

/// 距上次输入超过阈值即判空闲（用 wrapping_sub 处理 GetTickCount 回绕）。
pub fn is_idle(last_input_tick: u32, now_tick: u32, threshold_ms: u32) -> bool {
    now_tick.wrapping_sub(last_input_tick) >= threshold_ms
}

/// 从进程完整路径取文件名作为应用名。
pub fn basename(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_string()
}

/// app_name 或路径是否命中排除表（大小写不敏感包含匹配）。
pub fn is_excluded(app_name: &str, process_path: &str, excluded: &[String]) -> bool {
    let a = app_name.to_ascii_lowercase();
    let p = process_path.to_ascii_lowercase();
    excluded.iter().any(|e| {
        let e = e.trim().to_ascii_lowercase();
        !e.is_empty() && (a.contains(&e) || p.contains(&e))
    })
}

/// 从前台快照字段组装活动事件（app_name = 进程路径的文件名）。
pub fn build_event(ts: i64, process_path: &str, window_title: &str, is_idle: bool) -> ActivityEvent {
    ActivityEvent {
        ts,
        app_name: basename(process_path),
        process_path: process_path.to_string(),
        window_title: window_title.to_string(),
        edge_url: None,
        page_title: None,
        is_private: false,
        is_idle,
    }
}

/// 轮询采集循环：跑 `seconds` 秒后收尾退出（供托盘/CLI 调用）。
#[cfg(windows)]
pub fn run_for(cfg: &crate::config::Config, db_path: &str, seconds: u64) -> Result<(), String> {
    use crate::model::Session;
    use crate::session::SessionBuilder;
    use crate::storage::Storage;
    use crate::platform;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    let mut storage = Storage::open(db_path)?;
    storage.recover_phantom(cfg.poll_interval_sec as i64 * 3)?;
    let autostart_set = crate::autostart::autostart_exes();

    let mut sb = SessionBuilder::new();
    let mut buffer: Vec<Session> = Vec::new();
    let poll = Duration::from_secs(cfg.poll_interval_sec.max(1));
    let threshold_ms = cfg.idle_threshold_sec.saturating_mul(1000).min(u32::MAX as u64) as u32;
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut last_flush = Instant::now();
    let now_unix = || {
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
    };

    loop {
        let ts = now_unix();
        let media = crate::media::media_snapshot();
        let fg = platform::foreground_snapshot();
        let fg_is_edge = fg
            .as_ref()
            .map(|i| basename(&i.process_path).to_ascii_lowercase().contains("msedge"))
            .unwrap_or(false);
        let media_playing = matches!(
            media.as_ref().map(|m| m.status),
            Some(crate::model::PlaybackStatus::Playing)
        );
        // 真正在看视频 = 前台是 Edge 且视频在播放；后台放音乐或暂停都不算
        let watching = fg_is_edge && media_playing;
        // 只有"前台看视频"能抵消无键鼠输入的空闲判定
        let idle = is_idle(platform::last_input_tick(), platform::now_tick(), threshold_ms) && !watching;

        if let Some(info) = fg {
            let raw = basename(&info.process_path);
            let raw_lower = raw.to_ascii_lowercase();
            // 排除用户配置的应用 + 开机自启程序（那些是工具，无记录意义）
            if !is_excluded(&raw, &info.process_path, &cfg.excluded_apps)
                && !autostart_set.contains(&raw_lower)
            {
                let is_edge = raw_lower.contains("msedge");
                let is_private = is_edge && info.title.contains("InPrivate");
                // 隐私模式默认不记录
                if !(is_private && !cfg.record_private) {
                    let mut ev = build_event(ts, &info.process_path, &info.title, idle);
                    ev.app_name = crate::friendly::friendly_name(&raw);
                    ev.is_private = is_private;
                    if is_edge {
                        ev.edge_url = crate::edge::edge_url(info.hwnd);
                        ev.page_title = crate::parse::parse_edge_title(&info.title);
                    }
                    if let Some(done) = sb.on_activity(&ev) {
                        buffer.push(done);
                    }
                }
            }
        }
        // 媒体只在"真正在看"时计入（视频播放时长精确到前台+播放）
        if watching {
            if let Some(m) = media {
                sb.on_media(&crate::model::MediaEvent {
                    ts,
                    media_title: m.title,
                    media_player: m.player,
                    media_status: m.status,
                    position_sec: m.position_sec,
                    duration_sec: m.duration_sec,
                });
            }
        }
        sb.touch(ts);
        storage.set_heartbeat(ts)?;

        if buffer.len() >= cfg.flush_max_events
            || last_flush.elapsed() >= Duration::from_secs(cfg.flush_interval_sec.max(1))
        {
            if !buffer.is_empty() {
                storage.insert_sessions(&buffer)?;
                buffer.clear();
            }
            last_flush = Instant::now();
        }

        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(poll);
    }

    if let Some(done) = sb.finish(now_unix()) {
        buffer.push(done);
    }
    if !buffer.is_empty() {
        storage.insert_sessions(&buffer)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_normal_and_boundary() {
        assert!(!is_idle(1000, 1500, 1000)); // 空闲 500ms < 阈值
        assert!(is_idle(1000, 61000, 60000)); // 恰好达到阈值
    }

    #[test]
    fn idle_handles_tick_wrap() {
        // last 接近 u32::MAX，now 已回绕到 500，实际空闲约 1501ms
        assert!(is_idle(u32::MAX - 1000, 500, 1000));
    }

    #[test]
    fn basename_from_path() {
        assert_eq!(basename("C:\\Program Files\\X\\app.exe"), "app.exe");
        assert_eq!(basename("/usr/bin/code"), "code");
        assert_eq!(basename("noslash"), "noslash");
    }

    #[test]
    fn excluded_matches_name_or_path() {
        let ex = vec!["private.exe".to_string(), "secret".to_string()];
        assert!(!is_excluded("Code.exe", "C:\\a\\Code.exe", &ex));
        assert!(is_excluded("private.exe", "C:\\a\\private.exe", &ex)); // 命中应用名
        assert!(is_excluded("x.exe", "C:\\secret\\x.exe", &ex)); // 命中路径
        assert!(!is_excluded("x.exe", "C:\\a\\x.exe", &[]));
    }

    #[test]
    fn build_event_derives_app_and_flags() {
        let ev = build_event(100, "C:\\a\\Code.exe", "main.rs", false);
        assert_eq!(ev.app_name, "Code.exe");
        assert_eq!(ev.ts, 100);
        assert!(!ev.is_idle);
        assert!(ev.edge_url.is_none());
    }
}
