//! 系统托盘 + 后台常驻采集。右键弹出便当盒风格自绘菜单（非系统原生菜单）。
#![cfg(windows)]

use crate::collector::Control;
use crate::config::Config;
use slint::ComponentHandle;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetForegroundWindow};

fn make_icon() -> Option<Icon> {
    if let Some((w, h, rgba)) = crate::assets::logo_rgba(32) {
        if let Ok(icon) = Icon::from_rgba(rgba, w, h) {
            return Some(icon);
        }
    }
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

    let mut builder = TrayIconBuilder::new().with_tooltip("今天用啥");
    if let Some(icon) = make_icon() {
        builder = builder.with_icon(icon);
    }
    let _tray = builder.build().map_err(|e| e.to_string())?;

    let menu: Rc<RefCell<Option<crate::ui::TrayMenu>>> = Rc::new(RefCell::new(None));
    let win: Rc<RefCell<Option<crate::ui::Dashboard>>> = Rc::new(RefCell::new(None));
    let menu_hwnd: Rc<Cell<Option<isize>>> = Rc::new(Cell::new(None));
    let close_req = Rc::new(Cell::new(false));
    let tray_rx = TrayIconEvent::receiver();
    let timer = slint::Timer::default();
    let ctrl_c = ctrl.clone();
    let cfg_c = cfg.clone();
    let db_c = db_path.clone();

    timer.start(slint::TimerMode::Repeated, Duration::from_millis(120), move || {
        // __TIMER_BODY__
        while let Ok(ev) = tray_rx.try_recv() {
            let right_up = matches!(
                ev,
                TrayIconEvent::Click {
                    button: MouseButton::Right,
                    button_state: MouseButtonState::Up,
                    ..
                }
            );
            if !right_up {
                continue;
            }
            *menu.borrow_mut() = None;
            menu_hwnd.set(None);
            close_req.set(false);
            let Ok(m) = crate::ui::TrayMenu::new() else { continue };
            m.set_toggle_text(
                if ctrl_c.paused.load(Ordering::Relaxed) {
                    "恢复记录"
                } else {
                    "暂停记录"
                }
                .into(),
            );
            let mut pt = POINT::default();
            unsafe {
                let _ = GetCursorPos(&mut pt);
            }
            // 按系统 DPI 换算菜单物理尺寸，开在光标左上方（托盘在右下角），贴边保护
            let scale = (unsafe { GetDpiForSystem() } as f32 / 96.0).max(1.0);
            let pw = (160.0 * scale) as i32;
            let ph = (162.0 * scale) as i32;
            let mx = (pt.x - pw).max(4);
            let my = (pt.y - ph).max(4);
            m.window().set_position(slint::PhysicalPosition::new(mx, my));
            let weak = m.as_weak();
            let (c2, db2, cfg2, win2, close2) = (
                ctrl_c.clone(),
                db_c.clone(),
                cfg_c.clone(),
                win.clone(),
                close_req.clone(),
            );
            m.on_act(move |which| {
                match which {
                    0 => {
                        let mut wb = win2.borrow_mut();
                        if let Some(d) = wb.as_ref() {
                            let _ = crate::ui::refresh_dashboard(d, &db2);
                            let _ = d.show();
                        } else if let Ok(d) = crate::ui::build_dashboard(&db2) {
                            let _ = d.show();
                            *wb = Some(d);
                        }
                    }
                    1 => {
                        let p = !c2.paused.load(Ordering::Relaxed);
                        c2.paused.store(p, Ordering::Relaxed);
                    }
                    2 => spawn_summary(cfg2.clone(), db2.clone()),
                    3 => {
                        c2.stop.store(true, Ordering::Relaxed);
                        let _ = slint::quit_event_loop();
                    }
                    _ => {}
                }
                if let Some(mm) = weak.upgrade() {
                    let _ = mm.hide();
                }
                close2.set(true);
            });
            let _ = m.show();
            *menu.borrow_mut() = Some(m);
        }

        if menu.borrow().is_some() {
            if close_req.get() {
                *menu.borrow_mut() = None;
                menu_hwnd.set(None);
                close_req.set(false);
            } else {
                let fg = unsafe { GetForegroundWindow() }.0 as isize;
                match menu_hwnd.get() {
                    None => menu_hwnd.set(Some(fg)),
                    Some(h) => {
                        if fg != h {
                            *menu.borrow_mut() = None;
                            menu_hwnd.set(None);
                        }
                    }
                }
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
        let Ok(mut st) = crate::storage::Storage::open(&db) else {
            return;
        };
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
