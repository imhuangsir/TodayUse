#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use activity_tracker::{
    aggregate, config::Config, logging, model::ActivityEvent, session::SessionBuilder,
    storage::Storage,
};
use std::path::PathBuf;
use time::UtcOffset;

fn local_offset() -> UtcOffset {
    UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC)
}

#[cfg(windows)]
fn set_app_user_model_id() {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
    let id: Vec<u16> = "HuangYonghao.JinTianYongSha\0".encode_utf16().collect();
    unsafe {
        let _ = SetCurrentProcessExplicitAppUserModelID(PCWSTR(id.as_ptr()));
    }
}

/// 命名互斥量做单实例：已存在则返回 false（应退出）。句柄随进程存活到结束（HANDLE 无 Drop，不会关闭）。
#[cfg(windows)]
fn acquire_single_instance() -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;
    let name: Vec<u16> = "JinTianYongSha_SingleInstance\0".encode_utf16().collect();
    unsafe {
        match CreateMutexW(None, true, PCWSTR(name.as_ptr())) {
            // 互斥量已存在 → 已有实例在运行
            Ok(_h) => GetLastError() != ERROR_ALREADY_EXISTS,
            Err(_) => true, // 创建失败就不拦，照常运行
        }
    }
}

fn main() {
    #[cfg(windows)]
    set_app_user_model_id();
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("replay") => {
            let text = std::fs::read_to_string(&args[2]).expect("read events");
            let mut sb = SessionBuilder::new();
            let mut sessions = Vec::new();
            let mut last_ts = 0i64;
            for line in text.lines().filter(|l| !l.trim().is_empty()) {
                let ev: ActivityEvent = serde_json::from_str(line).expect("parse event");
                last_ts = ev.ts;
                if let Some(done) = sb.on_activity(&ev) {
                    sessions.push(done);
                }
            }
            if let Some(done) = sb.finish(last_ts) {
                sessions.push(done);
            }
            let mut st = Storage::open(&args[3]).expect("open db");
            st.insert_sessions(&sessions).expect("insert");
            println!("replayed -> {} sessions", sessions.len());
        }
        Some("report") => {
            let p: Vec<i64> = args[3].split('-').map(|x| x.parse().unwrap()).collect();
            let (start, end) =
                aggregate::local_day_bounds(p[0] as i32, p[1] as u8, p[2] as u8, local_offset());
            let st = Storage::open(&args[2]).expect("open db");
            let sessions = st.sessions_in_range(start, end).expect("query");
            println!(
                "有效 {}s / 空闲 {}s",
                aggregate::total_active_sec(&sessions),
                aggregate::total_idle_sec(&sessions)
            );
            for b in aggregate::app_durations(&sessions) {
                println!("  {} - {}s", b.name, b.seconds);
            }
        }
        Some("collect") => {
            let cfg = Config::load(&config_path()).unwrap_or_default();
            let secs: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(10);
            activity_tracker::collector::run_for(&cfg, &args[2], secs).expect("collect");
            println!("collected {}s -> {}", secs, args[2]);
        }
        Some("ui") => {
            activity_tracker::ui::show_dashboard(&args[2]).expect("ui");
        }
        Some("diagwin") => {
            activity_tracker::ui::diag_run(&args[2]);
        }
        Some("set-key") => {
            // 从 stdin 读入 API key（避免出现在命令行/进程列表），DPAPI 加密存到 key.bin。
            use std::io::Read;
            let mut key = String::new();
            let _ = std::io::stdin().read_to_string(&mut key);
            let key = key.trim();
            if key.is_empty() {
                eprintln!("未读到 key（用法：echo <key> | activity-tracker set-key）");
            } else {
                let path = appdata_dir("LOCALAPPDATA").join("key.bin");
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                match activity_tracker::secret::save_key(&path, key) {
                    Ok(()) => println!("API key 已加密(DPAPI)保存到 {}", path.display()),
                    Err(e) => eprintln!("保存失败: {e}"),
                }
            }
        }
        Some("dump") => {
            let p: Vec<i64> = args[3].split('-').map(|x| x.parse().unwrap()).collect();
            let (start, end) =
                aggregate::local_day_bounds(p[0] as i32, p[1] as u8, p[2] as u8, local_offset());
            let st = Storage::open(&args[2]).expect("open db");
            let sessions = st.sessions_in_range(start, end).expect("query");
            println!("共 {} 条会话:", sessions.len());
            for s in &sessions {
                println!(
                    "[{}s idle={} priv={}] app={}\n    title  : {}\n    edge   : {:?}  page: {:?}\n    media  : {:?} status={:?} pos={:?}/{:?}",
                    s.duration_sec, s.is_idle, s.is_private, s.app_name,
                    s.window_title, s.edge_url, s.page_title,
                    s.media_title, s.media_status, s.media_position_sec, s.media_duration_sec
                );
            }
        }
        Some("summarize") => {
            let cfg = Config::load(&config_path()).unwrap_or_default();
            let p: Vec<i64> = args[3].split('-').map(|x| x.parse().unwrap()).collect();
            let mut st = Storage::open(&args[2]).expect("open db");
            let key = std::env::var("AT_API_KEY")
                .ok()
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    let base = std::env::var("LOCALAPPDATA").ok()?;
                    activity_tracker::secret::load_key(
                        &PathBuf::from(base).join("ActivityTracker").join("key.bin"),
                    )
                })
                .unwrap_or_default();
            if cfg.ai_enabled && !key.is_empty() && !cfg.ai_base_url.is_empty() {
                match activity_tracker::summarize::generate_for_day(
                    &mut st, &cfg, &key, p[0] as i32, p[1] as u8, p[2] as u8, local_offset(),
                ) {
                    Ok(text) => println!("{text}"),
                    Err(e) => eprintln!("AI 总结失败: {e}"),
                }
            } else {
                // 未配置 AI/密钥：只打印将要外发的脱敏摘要（dry-run）
                let (start, end) = aggregate::local_day_bounds(
                    p[0] as i32, p[1] as u8, p[2] as u8, local_offset(),
                );
                let sessions = st.sessions_in_range(start, end).expect("query");
                let date = format!("{:04}-{:02}-{:02}", p[0], p[1], p[2]);
                let digest = activity_tracker::summarize::desensitize(
                    activity_tracker::summarize::build_digest(&date, &sessions),
                    &cfg,
                );
                println!("[dry-run 未启用AI/无密钥] 将外发的脱敏摘要：");
                println!("{}", serde_json::to_string_pretty(&digest).unwrap());
            }
        }
        None | Some("tray") => {
            #[cfg(windows)]
            if !acquire_single_instance() {
                // 已有一个实例在运行，直接退出，避免两个采集守护抢写同一个库。
                return;
            }
            let cfg = Config::load(&config_path()).unwrap_or_default();
            logging::init(&log_path());
            let db = data_db_path();
            if let Some(dir) = std::path::Path::new(&db).parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Err(e) = activity_tracker::tray::run_tray(cfg, db) {
                log::error!("托盘启动失败: {e}");
                eprintln!("托盘启动失败: {e}");
            }
        }
        _ => {
            println!("用法: (无参=托盘常驻) | collect <db> <秒> | ui <db> | dump <db> <日期> | report <db> <日期> | summarize <db> <日期> | replay <jsonl> <db>");
        }
    }
}

fn appdata_dir(env_key: &str) -> PathBuf {
    let base = std::env::var(env_key).unwrap_or_else(|_| ".".to_string());
    PathBuf::from(base).join("ActivityTracker")
}
fn data_db_path() -> String {
    appdata_dir("LOCALAPPDATA")
        .join("data.db")
        .to_string_lossy()
        .into_owned()
}
fn config_path() -> PathBuf {
    appdata_dir("APPDATA").join("config.toml")
}
fn log_path() -> PathBuf {
    appdata_dir("LOCALAPPDATA").join("activity-tracker.log")
}
