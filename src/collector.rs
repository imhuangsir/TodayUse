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

/// 系统/外壳进程（锁屏、开始菜单、搜索、UWP 框架宿主等）与本程序自身，无记录意义，恒排除。
fn is_system_noise(exe_lower: &str) -> bool {
    const NOISE: &[&str] = &[
        "lockapp.exe",
        "shellexperiencehost.exe",
        "startmenuexperiencehost.exe",
        "searchhost.exe",
        "searchapp.exe",
        "textinputhost.exe",
        "applicationframehost.exe",
        "dwm.exe",
        "sihost.exe",
        "ctfmon.exe",
        "activity-tracker.exe",
    ];
    NOISE.contains(&exe_lower)
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

/// 单轮采集：抓前台/媒体/空闲，按排除规则并入会话构建器（run_for 与 run_daemon 共用）。
#[cfg(windows)]
fn collect_once(
    cfg: &crate::config::Config,
    autostart_set: &std::collections::HashSet<String>,
    threshold_ms: u32,
    ts: i64,
    sb: &mut crate::session::SessionBuilder,
    buffer: &mut Vec<crate::model::Session>,
) {
    use crate::platform;
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
        // 排除用户配置的应用 + 开机自启程序（那些是工具，无记录意义）+ 系统外壳噪声
        if !is_excluded(&raw, &info.process_path, &cfg.excluded_apps)
            && !autostart_set.contains(&raw_lower)
            && !is_system_noise(&raw_lower)
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
}

/// 落库会话；失败只记日志并保留 buffer 下轮重试（绝不让采集线程因偶发 DB 错误而退出）。
#[cfg(windows)]
fn flush_buffer(storage: &mut crate::storage::Storage, buffer: &mut Vec<crate::model::Session>) {
    if buffer.is_empty() {
        return;
    }
    match storage.insert_sessions(buffer) {
        Ok(()) => buffer.clear(),
        Err(e) => {
            log::warn!("写入会话失败(下轮重试): {e}");
            if buffer.len() > 5000 {
                buffer.clear(); // 兜底：避免异常持续导致无限增长
            }
        }
    }
}

/// 轮询采集循环：跑 `seconds` 秒后收尾退出（供托盘/CLI 调用）。
#[cfg(windows)]
pub fn run_for(cfg: &crate::config::Config, db_path: &str, seconds: u64) -> Result<(), String> {
    use crate::model::Session;
    use crate::session::SessionBuilder;
    use crate::storage::Storage;
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
    let now_unix = || SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;

    loop {
        let ts = now_unix();
        collect_once(cfg, &autostart_set, threshold_ms, ts, &mut sb, &mut buffer);
        let _ = storage.set_heartbeat(ts);

        if buffer.len() >= cfg.flush_max_events
            || last_flush.elapsed() >= Duration::from_secs(cfg.flush_interval_sec.max(1))
        {
            flush_buffer(&mut storage, &mut buffer);
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
    flush_buffer(&mut storage, &mut buffer);
    Ok(())
}

/// 采集控制：暂停/停止标志（托盘线程与采集线程共享）。
#[cfg(windows)]
pub struct Control {
    pub paused: std::sync::atomic::AtomicBool,
    pub stop: std::sync::atomic::AtomicBool,
}
#[cfg(windows)]
impl Control {
    pub fn new() -> Self {
        Self {
            paused: std::sync::atomic::AtomicBool::new(false),
            stop: std::sync::atomic::AtomicBool::new(false),
        }
    }
}
#[cfg(windows)]
impl Default for Control {
    fn default() -> Self {
        Self::new()
    }
}

/// 常驻采集：一直跑到 ctrl.stop 置位；ctrl.paused 时结算并暂停累计。
#[cfg(windows)]
pub fn run_daemon(
    cfg: &crate::config::Config,
    db_path: &str,
    ctrl: std::sync::Arc<Control>,
) -> Result<(), String> {
    use crate::model::Session;
    use crate::session::SessionBuilder;
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    let mut storage = crate::storage::Storage::open(db_path)?;
    let _ = storage.recover_phantom(cfg.poll_interval_sec as i64 * 6);
    let autostart_set = crate::autostart::autostart_exes();

    let mut sb = SessionBuilder::new();
    let mut buffer: Vec<Session> = Vec::new();
    let poll = Duration::from_secs(cfg.poll_interval_sec.max(1));
    let threshold_ms = cfg.idle_threshold_sec.saturating_mul(1000).min(u32::MAX as u64) as u32;
    let mut last_flush = Instant::now();
    let mut last_heartbeat = Instant::now();
    let now_unix = || SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
    let _ = storage.set_heartbeat(now_unix());

    while !ctrl.stop.load(Ordering::Relaxed) {
        let ts = now_unix();
        if ctrl.paused.load(Ordering::Relaxed) {
            if let Some(done) = sb.finish(ts) {
                buffer.push(done);
            }
            flush_buffer(&mut storage, &mut buffer);
            std::thread::sleep(poll);
            continue;
        }
        collect_once(cfg, &autostart_set, threshold_ms, ts, &mut sb, &mut buffer);

        // 心跳每 ~30s 写一次即可（仅用于崩溃残留兜底），不必每轮写盘。
        if last_heartbeat.elapsed() >= Duration::from_secs(30) {
            let _ = storage.set_heartbeat(ts);
            last_heartbeat = Instant::now();
        }

        if buffer.len() >= cfg.flush_max_events
            || last_flush.elapsed() >= Duration::from_secs(cfg.flush_interval_sec.max(1))
        {
            flush_buffer(&mut storage, &mut buffer);
            last_flush = Instant::now();
        }
        std::thread::sleep(poll);
    }

    if let Some(done) = sb.finish(now_unix()) {
        buffer.push(done);
    }
    flush_buffer(&mut storage, &mut buffer);
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
