//! 系统托盘 + 后台常驻采集。右键弹便当盒自绘菜单（单实例复用）。启动预热窗口。
#![cfg(windows)]

use crate::collector::Control;
use crate::config::Config;
use slint::winit_030::WinitWindowAccessor;
use slint::ComponentHandle;
use std::cell::Cell;
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

fn winit_icon() -> Option<slint::winit_030::winit::window::Icon> {
    let (w, h, rgba) = crate::assets::logo_rgba(64)?;
    slint::winit_030::winit::window::Icon::from_rgba(rgba, w, h).ok()
}

/// 打开/刷新仪表盘窗口，并设置任务栏图标（winit 层，修复默认图标）。
fn open_dashboard(win: &std::cell::RefCell<Option<crate::ui::Dashboard>>, db: &str) {
    let mut wb = win.borrow_mut();
    if let Some(d) = wb.as_ref() {
        // 常规路径：窗口已在启动时预热建好，刷新数据后移回屏幕中央显示（秒显、可正常绘制）。
        let _ = crate::ui::refresh_dashboard(d, db);
        crate::ui::show_centered(d);
    } else {
        // 兜底：预热失败才走这里现建（首帧可能不显示，用 force_first_show 补救）。
        match crate::ui::build_dashboard(db) {
            Ok(d) => {
                let _ = d.show();
                crate::ui::force_first_show(&d);
                *wb = Some(d);
            }
            Err(e) => {
                log::error!("打开仪表盘失败: {e}");
                return;
            }
        }
    }
    if let Some(d) = wb.as_ref() {
        d.window().with_winit_window(|w| {
            use slint::winit_030::winit::platform::windows::WindowExtWindows;
            w.set_taskbar_icon(winit_icon());
        });
    }
}

/// 启动托盘 + 后台采集，进入事件循环（阻塞至退出）。
pub fn run_tray(cfg: Config, db_path: String) -> Result<(), String> {
    // 按配置写/删开机自启项（指向当前 exe）。
    if let Ok(exe) = std::env::current_exe() {
        if let Err(e) = crate::autostart::set_autostart(cfg.autostart, &exe.to_string_lossy()) {
            log::warn!("设置开机自启失败: {e}");
        }
    }
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

    // 仪表盘窗口预热：在事件循环启动前 build+show（走能正常绘制的路径），随后移到屏幕外并隐藏。
    // 这样首次点“查看”窗口已建好，直接复位居中+show 即可秒显、正常绘制（懒创建会首帧不显示/黑屏）。
    let win: Rc<std::cell::RefCell<Option<crate::ui::Dashboard>>> =
        Rc::new(std::cell::RefCell::new(None));
    match crate::ui::build_dashboard(&db_path) {
        Ok(d) => {
            let _ = d.show();
            crate::ui::prewarm_hide(&d);
            *win.borrow_mut() = Some(d);
        }
        Err(e) => log::error!("预热仪表盘失败: {e}"),
    }

    // 便当盒菜单：单实例，创建一次并预热；之后只重定位/显示/隐藏。
    let menu = crate::ui::TrayMenu::new().map_err(|e| e.to_string())?;
    let menu_visible = Rc::new(Cell::new(false));
    {
        let menu_weak = menu.as_weak();
        let (c2, db2, cfg2, win2, mv2) = (
            ctrl.clone(),
            db_path.clone(),
            cfg.clone(),
            win.clone(),
            menu_visible.clone(),
        );
        menu.on_act(move |which| {
            if let Some(mm) = menu_weak.upgrade() {
                let _ = mm.hide(); // 先立即收起菜单，再执行动作
            }
            mv2.set(false);
            match which {
                0 => open_dashboard(&win2, &db2),
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
        });
    }

    // __TRAY_TIMER__
    let tray_rx = TrayIconEvent::receiver();
    let timer = slint::Timer::default();
    let menu_hwnd: Cell<Option<isize>> = Cell::new(None);
    let mv = menu_visible.clone();
    let ctrl_t = ctrl.clone();

    timer.start(slint::TimerMode::Repeated, Duration::from_millis(120), move || {
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
            menu.set_toggle_text(
                if ctrl_t.paused.load(Ordering::Relaxed) {
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
            let scale = (unsafe { GetDpiForSystem() } as f32 / 96.0).max(1.0);
            let pw = (160.0 * scale) as i32;
            let ph = (162.0 * scale) as i32;
            let mx = (pt.x - pw).max(4);
            let my = (pt.y - ph).max(4);
            let _ = menu.show();
            menu.window().with_winit_window(|w| {
                use slint::winit_030::winit::platform::windows::WindowExtWindows;
                w.set_skip_taskbar(true);
                w.set_outer_position(slint::winit_030::winit::dpi::PhysicalPosition::new(mx, my));
            });
            mv.set(true);
            menu_hwnd.set(None);
        }

        if mv.get() {
            let fg = unsafe { GetForegroundWindow() }.0 as isize;
            match menu_hwnd.get() {
                None => menu_hwnd.set(Some(fg)),
                Some(h) => {
                    if fg != h {
                        let _ = menu.hide();
                        mv.set(false);
                        menu_hwnd.set(None);
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
