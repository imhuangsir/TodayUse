use activity_tracker::{
    aggregate, config::Config, logging, model::ActivityEvent, session::SessionBuilder,
    storage::Storage,
};
use std::path::PathBuf;
use time::UtcOffset;

fn local_offset() -> UtcOffset {
    UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC)
}

fn main() {
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
            let cfg = Config::load(&PathBuf::from("config.toml")).unwrap_or_default();
            let secs: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(10);
            activity_tracker::collector::run_for(&cfg, &args[2], secs).expect("collect");
            println!("collected {}s -> {}", secs, args[2]);
        }
        Some("summarize") => {
            let cfg = Config::load(&PathBuf::from("config.toml")).unwrap_or_default();
            let p: Vec<i64> = args[3].split('-').map(|x| x.parse().unwrap()).collect();
            let mut st = Storage::open(&args[2]).expect("open db");
            let key = std::env::var("AT_API_KEY").unwrap_or_default();
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
        _ => {
            let cfg = Config::load(&PathBuf::from("config.toml")).unwrap_or_default();
            logging::init(&PathBuf::from("activity-tracker.log"));
            let _ = cfg;
            println!("activity-tracker core (M1). 用法: replay <events.jsonl> <db> | report <db> <YYYY-MM-DD>");
        }
    }
}
