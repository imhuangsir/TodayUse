//! Slint 仪表盘窗口（Bento Box 风格）。UI 定义在 ui/dashboard.slint，由 build.rs 编译。
use crate::{aggregate, storage::Storage};
use slint::ComponentHandle;
use slint::winit_030::WinitWindowAccessor;
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
pub fn refresh_dashboard(ui: &Dashboard, db_path: &str) -> Result<(), String> {
    let off = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    let now = OffsetDateTime::now_utc().to_offset(off);
    let (y, m, d) = (now.year(), u8::from(now.month()), now.day());
    let (start, end) = aggregate::local_day_bounds(y, m, d, off);
    let st = Storage::open(db_path)?;
    let sessions = st.sessions_in_range(start, end)?;

    ui.set_app_icon(app_icon_image());
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
    let summary = st
        .latest_summary(start, end)?
        .unwrap_or_else(|| "（未生成：配置 AI 后，点托盘『立即生成总结』，再重开本窗口即可显示）".to_string());
    ui.set_summary(summary.into());
    Ok(())
}

fn app_icon_image() -> slint::Image {
    if let Some((w, h, rgba)) = crate::assets::logo_rgba(144) {
        let mut pb = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(w, h);
        pb.make_mut_bytes().copy_from_slice(&rgba);
        slint::Image::from_rgba8(pb)
    } else {
        slint::Image::default()
    }
}

/// 新建并填充仪表盘窗口。
pub fn build_dashboard(db_path: &str) -> Result<Dashboard, String> {
    let ui = Dashboard::new().map_err(|e| e.to_string())?;
    refresh_dashboard(&ui, db_path)?;

    // 无边框窗口：自定义关闭按钮 → 隐藏(保活, 再开不再白屏)；标题栏拖动 → 移动窗口
    let w1 = ui.as_weak();
    ui.on_close_clicked(move || {
        if let Some(u) = w1.upgrade() {
            let _ = u.hide();
        }
    });
    // 无边框窗口拖动：交给 winit 的原生拖动（比手动 set_outer_position 稳，能真正跟手）。
    let w2 = ui.as_weak();
    ui.on_start_drag(move || {
        if let Some(u) = w2.upgrade() {
            let mut got = false;
            u.window().with_winit_window(|win| {
                got = true;
                if let Err(e) = win.drag_window() {
                    log::warn!("drag_window 失败: {e}");
                }
            });
            if !got {
                log::warn!("start-drag: 无 winit 窗口");
            }
        }
    });
    ui.window()
        .on_close_requested(|| slint::CloseRequestResponse::HideWindow);
    Ok(ui)
}

/// 阻塞式弹窗（CLI `ui` 子命令用）。
pub fn show_dashboard(db_path: &str) -> Result<(), String> {
    let ui = build_dashboard(db_path)?;
    ui.run().map_err(|e| e.to_string())?;
    Ok(())
}

/// 诊断：打印 winit 窗口当前状态。
pub fn diag_window_state(d: &Dashboard, tag: &str) {
    let mut got = false;
    d.window().with_winit_window(|w| {
        got = true;
        let sz = w.inner_size();
        println!(
            "[{tag}] visible={:?} minimized={:?} focused={:?} pos={:?} inner={}x{} scale={}",
            w.is_visible(),
            w.is_minimized(),
            w.has_focus(),
            w.outer_position().ok(),
            sz.width,
            sz.height,
            w.scale_factor()
        );
    });
    if !got {
        println!("[{tag}] (no winit window yet)");
    }
}

/// 首次把无边框透明窗口显示出来。
///
/// 根因（diagwin 实测）：在已运行的事件循环里对新建窗口调用 slint `show()`，winit 会**延迟建窗**，
/// 并陷入“内部 visible=true 但系统 `IsWindowVisible`=False、根本没绘制”的假可见状态；
/// 此时再调 slint `show()` 是空操作（PHASE-C 证实）。唯一能真正显示的办法是等窗口真正建好后，
/// 在 winit 层强制 `set_visible(false)`→`set_visible(true)`，触发系统 ShowWindow+重绘（PHASE-A 证实）。
/// 由于建窗时机不定，这里在多个延迟点重试切换，确保命中。
pub fn force_first_show(d: &Dashboard) {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::Duration;
    // 1) 窗口一旦建好，做**一次** winit set_visible(false->true) 触发系统 ShowWindow。
    //    只能一次：重复切换会让窗口可见却不重绘(黑屏)。
    let toggled = Rc::new(Cell::new(false));
    for delay in [80u64, 180, 320, 500, 720, 1000, 1400] {
        let weak = d.as_weak();
        let toggled = toggled.clone();
        slint::Timer::single_shot(Duration::from_millis(delay), move || {
            if toggled.get() {
                return;
            }
            if let Some(d) = weak.upgrade() {
                let mut ok = false;
                d.window().with_winit_window(|w| {
                    w.set_visible(false);
                    w.set_visible(true);
                    w.focus_window();
                    ok = true;
                });
                if ok {
                    toggled.set(true);
                }
            }
        });
    }
    // 2) 反复请求重绘，直到软件渲染器把首帧真正画出来（切换后首帧常没画=黑屏）。request_redraw 幂等安全。
    for delay in [120u64, 260, 420, 620, 850, 1150, 1500, 2000] {
        let weak = d.as_weak();
        slint::Timer::single_shot(Duration::from_millis(delay), move || {
            if let Some(d) = weak.upgrade() {
                d.window().request_redraw();
            }
        });
    }
}

/// 预热隐藏：窗口（在事件循环启动前已 show）真正建好后，移到屏幕外 + 从任务栏隐藏 + hide。
/// 之后再 show 就能正常显示且能绘制（见 [[slint-tray-window-first-show]]）。
pub fn prewarm_hide(d: &Dashboard) {
    use slint::winit_030::winit::dpi::PhysicalPosition;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::Duration;
    let done = Rc::new(Cell::new(false));
    for delay in [40u64, 90, 160, 280, 450, 700] {
        let weak = d.as_weak();
        let done = done.clone();
        slint::Timer::single_shot(Duration::from_millis(delay), move || {
            if done.get() {
                return;
            }
            if let Some(d) = weak.upgrade() {
                let mut ok = false;
                d.window().with_winit_window(|w| {
                    use slint::winit_030::winit::platform::windows::WindowExtWindows;
                    w.set_skip_taskbar(true);
                    w.set_outer_position(PhysicalPosition::new(-32000, -32000));
                    ok = true;
                });
                if ok {
                    let _ = d.hide();
                    done.set(true);
                }
            }
        });
    }
}

/// 把（预热过、当前隐藏在屏幕外的）窗口移回主屏幕居中并显示出来。
pub fn show_centered(d: &Dashboard) {
    use slint::winit_030::winit::dpi::PhysicalPosition;
    d.window().with_winit_window(|w| {
        use slint::winit_030::winit::platform::windows::WindowExtWindows;
        w.set_skip_taskbar(false);
        if let Some(mon) = w.primary_monitor() {
            let mp = mon.position();
            let ms = mon.size();
            let ws = w.outer_size();
            let x = mp.x + ((ms.width as i32 - ws.width as i32) / 2).max(0);
            let y = mp.y + ((ms.height as i32 - ws.height as i32) / 2).max(0);
            w.set_outer_position(PhysicalPosition::new(x, y));
        }
    });
    let _ = d.show();
    d.window().request_redraw();
    d.window().with_winit_window(|w| w.focus_window());
}

/// 诊断辅助：当前前台窗口标题（判断我们的窗口是否真的到了最前）。
#[cfg(windows)]
fn diag_foreground_title() -> String {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};
    unsafe {
        let h = GetForegroundWindow();
        if h.0.is_null() {
            return "<none>".into();
        }
        let mut buf = [0u16; 128];
        let n = GetWindowTextW(h, &mut buf);
        String::from_utf16_lossy(&buf[..n as usize])
    }
}

/// 诊断：预热后反复 开→检查→关，统计首次显示偶发失败的形态（可见? 到最前?）。
pub fn diag_run(db_path: &str) {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::time::Duration;

    let win: Rc<RefCell<Option<Dashboard>>> = Rc::new(RefCell::new(None));
    match build_dashboard(db_path) {
        Ok(d) => {
            let _ = d.show();
            prewarm_hide(&d);
            *win.borrow_mut() = Some(d);
            println!("[t0] 预热完成，开始循环开关窗测试");
        }
        Err(e) => {
            println!("build 失败: {e}");
            return;
        }
    }

    let step = Rc::new(Cell::new(0u32));
    let timer = slint::Timer::default();
    let w = win.clone();
    let step2 = step.clone();
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(500), move || {
        let s = step2.get();
        step2.set(s + 1);
        let cycle = s / 2;
        if cycle >= 20 {
            let _ = slint::quit_event_loop();
            return;
        }
        let Some(d) = w.borrow().as_ref().map(|d| d.clone_strong()) else { return };
        if s % 2 == 0 {
            show_centered(&d);
        } else {
            let mut vis = None;
            let mut lvl_ok = false;
            d.window().with_winit_window(|win| {
                vis = win.is_visible();
                lvl_ok = true;
            });
            let fg = diag_foreground_title();
            let is_front = fg == "今天用啥";
            let mark = if vis == Some(true) && is_front { "OK" } else { "**FAIL**" };
            println!(
                "cycle {cycle:02}: {mark} visible={vis:?} foreground_is_us={is_front} fg='{fg}' winit_ok={lvl_ok}"
            );
            let _ = d.hide();
        }
    });

    let _keep = timer;
    let _ = slint::run_event_loop_until_quit();
    println!("[end] 循环退出");
}
