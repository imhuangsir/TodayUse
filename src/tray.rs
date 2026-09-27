//! 系统托盘 + 后台常驻采集。托盘菜单：暂停/恢复、查看统计、立即总结、退出。
#![cfg(windows)]

use crate::collector::Control;
use crate::config::Config;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use slint::ComponentHandle;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder};

fn make_icon() -> Option<Icon> {
    if let Some((w, h, rgba)) = crate::assets::logo_rgba(32) {
        if let Ok(icon) = Icon::from_rgba(rgba, w, h) {
            return Some(icon);
        }
    }
    // 回退：蓝色圆点
    let size = 32u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let (cx, cy, r) = (16.0f32, 16.0f32, 14.0f32);
    for y in 0..size {
        for x in 0..size {
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            if dx * dx + dy * dy <= r * r {
                let i = ((y * size + x) * 4) as usize;
                rgba[i] = 37;
                rgba[i + 1] = 99;
                rgba[i + 2] = 235;
                rgba[i + 3] = 255;
            }
        }
    }
    Icon::from_rgba(rgba, size, size).ok()
}

/// 启动托盘 + 后台采集，进入事件循环（阻塞至退出）。
pub fn run_tray(cfg: Config, db_path: String) -> Result<(), String> {
    let ctrl = Arc::new(Control::new());
    {
        let ctrl = ctrl.clone();
        let cfg = cfg.clone();
        let db = db_path.clone();
        std::thread::spawn(move || {
            let _ = crate::collector::run_daemon(&cfg, &db, ctrl);
        });
    }

    let menu = Menu::new();
    let mi_show = MenuItem::new("查看今日统计", true, None);
    let mi_toggle = MenuItem::new("暂停记录", true, None);
    let mi_sum = MenuItem::new("立即生成总结", true, None);
    let mi_quit = MenuItem::new("退出", true, None);
    let sep = PredefinedMenuItem::separator();
    for it in [
        &mi_show as &dyn tray_icon::menu::IsMenuItem,
        &mi_toggle,
        &sep,
        &mi_sum,
        &mi_quit,
    ] {
        menu.append(it).map_err(|e| e.to_string())?;
    }

    let mut builder = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("今天用啥");
    if let Some(icon) = make_icon() {
        builder = builder.with_icon(icon);
    }
    let _tray = builder.build().map_err(|e| e.to_string())?;

    let (id_show, id_toggle, id_sum, id_quit) = (
        mi_show.id().clone(),
        mi_toggle.id().clone(),
        mi_sum.id().clone(),
        mi_quit.id().clone(),
    );
    let win: Rc<RefCell<Option<crate::ui::Dashboard>>> = Rc::new(RefCell::new(None));
    let menu_rx = MenuEvent::receiver();
    let timer = slint::Timer::default();
    let ctrl_t = ctrl.clone();
    let db_t = db_path.clone();
    let cfg_t = cfg.clone();

    timer.start(slint::TimerMode::Repeated, Duration::from_millis(150), move || {
        while let Ok(ev) = menu_rx.try_recv() {
            if ev.id == id_quit {
                ctrl_t.stop.store(true, Ordering::Relaxed);
                let _ = slint::quit_event_loop();
            } else if ev.id == id_toggle {
                let paused = !ctrl_t.paused.load(Ordering::Relaxed);
                ctrl_t.paused.store(paused, Ordering::Relaxed);
                mi_toggle.set_text(if paused { "恢复记录" } else { "暂停记录" });
            } else if ev.id == id_show {
                match crate::ui::build_dashboard(&db_t) {
                    Ok(d) => {
                        let _ = d.show();
                        *win.borrow_mut() = Some(d);
                    }
                    Err(e) => eprintln!("打开窗口失败: {e}"),
                }
            } else if ev.id == id_sum {
                spawn_summary(cfg_t.clone(), db_t.clone());
            }
        }
    });

    slint::run_event_loop_until_quit().map_err(|e| e.to_string())?;
    ctrl.stop.store(true, Ordering::Relaxed);
    Ok(())
}

fn spawn_summary(cfg: Config, db: String) {
    std::thread::spawn(move || {
        if !cfg.ai_enabled || cfg.ai_base_url.is_empty() {
            return;
        }
        let key = std::env::var("AT_API_KEY")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| {
                let base = std::env::var("LOCALAPPDATA").ok()?;
                crate::secret::load_key(
                    &std::path::PathBuf::from(base)
                        .join("ActivityTracker")
                        .join("key.bin"),
                )
            });
        let Some(key) = key else { return };
        let Ok(mut st) = crate::storage::Storage::open(&db) else { return };
        let off = time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);
        let now = time::OffsetDateTime::now_utc().to_offset(off);
        let _ = crate::summarize::generate_for_day(
            &mut st,
            &cfg,
            &key,
            now.year(),
            u8::from(now.month()),
            now.day(),
            off,
        );
    });
}
