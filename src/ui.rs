//! Slint 仪表盘窗口（Bento Box 风格）。UI 定义在 ui/dashboard.slint，由 build.rs 编译。
use crate::{aggregate, storage::Storage};
use std::rc::Rc;
use time::{OffsetDateTime, UtcOffset};

slint::include_modules!();

fn fmt_dur(sec: i64) -> String {
    let h = sec / 3600;
    let m = (sec % 3600) / 60;
    let s = sec % 60;
    if h > 0 {
        format!("{h}小时{m}分")
    } else if m > 0 {
        format!("{m}分")
    } else {
        format!("{s}秒")
    }
}

fn rows(buckets: &[aggregate::Bucket], n: usize) -> Vec<slint::SharedString> {
    buckets
        .iter()
        .take(n)
        .map(|b| slint::SharedString::from(format!("{}  ·  {}", b.name, fmt_dur(b.seconds))))
        .collect()
}

/// 构建并填充仪表盘窗口（不阻塞；调用方负责 show/run 并保持其存活）。
pub fn build_dashboard(db_path: &str) -> Result<Dashboard, String> {
    let off = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    let now = OffsetDateTime::now_utc().to_offset(off);
    let (y, m, d) = (now.year(), u8::from(now.month()), now.day());
    let (start, end) = aggregate::local_day_bounds(y, m, d, off);
    let st = Storage::open(db_path)?;
    let sessions = st.sessions_in_range(start, end)?;

    let ui = Dashboard::new().map_err(|e| e.to_string())?;
    if let Some((w, h, rgba)) = crate::assets::logo_rgba(64) {
        let mut pb = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(w, h);
        pb.make_mut_bytes().copy_from_slice(&rgba);
        ui.set_app_icon(slint::Image::from_rgba8(pb));
    }
    ui.set_date(format!("{y:04}-{m:02}-{d:02}").into());
    ui.set_active(fmt_dur(aggregate::total_active_sec(&sessions)).into());
    ui.set_idle(fmt_dur(aggregate::total_idle_sec(&sessions)).into());
    let apps = aggregate::app_durations(&sessions);
    let max = apps.first().map(|b| b.seconds).unwrap_or(1).max(1);

    // app_name -> 代表性进程路径（取图标用）
    let mut path_of: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for s in &sessions {
        path_of
            .entry(s.app_name.clone())
            .or_insert_with(|| s.process_path.clone());
    }

    let top: Vec<&aggregate::Bucket> = apps.iter().take(8).collect();
    let appbars: Vec<AppBar> = top
        .iter()
        .map(|b| AppBar {
            name: b.name.clone().into(),
            dur: fmt_dur(b.seconds).into(),
            frac: b.seconds as f32 / max as f32,
        })
        .collect();
    let icons: Vec<slint::Image> = top
        .iter()
        .map(|b| {
            path_of
                .get(&b.name)
                .and_then(|p| crate::icon::app_icon_rgba(p))
                .map(|(w, h, rgba)| {
                    let mut pb = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(w, h);
                    pb.make_mut_bytes().copy_from_slice(&rgba);
                    slint::Image::from_rgba8(pb)
                })
                .unwrap_or_default()
        })
        .collect();
    ui.set_appbars(Rc::new(slint::VecModel::from(appbars)).into());
    ui.set_icons(Rc::new(slint::VecModel::from(icons)).into());
    ui.set_sites(Rc::new(slint::VecModel::from(rows(&aggregate::domain_durations(&sessions), 6))).into());
    ui.set_videos(Rc::new(slint::VecModel::from(rows(&aggregate::top_videos(&sessions), 6))).into());
    ui.set_summary("（未生成：配置 AI 或点『立即生成总结』后显示）".into());
    Ok(ui)
}

/// 阻塞式弹窗（CLI `ui` 子命令用）。
pub fn show_dashboard(db_path: &str) -> Result<(), String> {
    let ui = build_dashboard(db_path)?;
    ui.run().map_err(|e| e.to_string())?;
    Ok(())
}
