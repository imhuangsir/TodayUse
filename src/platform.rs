//! Win32 薄封装：前台窗口快照与空闲 tick。所有 unsafe 在本模块内消化。
#![cfg(windows)]

use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, FALSE};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
};

pub struct ForegroundInfo {
    pub title: String,
    pub process_path: String,
    pub pid: u32,
    pub hwnd: isize,
}

/// 系统开机以来的毫秒 tick（会每 ~49 天回绕）。
pub fn now_tick() -> u32 {
    unsafe { GetTickCount() }
}

/// 最近一次输入时的 tick。
pub fn last_input_tick() -> u32 {
    let mut lii = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    unsafe {
        let _ = GetLastInputInfo(&mut lii);
    }
    lii.dwTime
}

fn process_path_of(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid).ok()?;
        let mut buf = [0u16; 512];
        let mut size = buf.len() as u32;
        let res =
            QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut size);
        let _ = CloseHandle(handle);
        res.ok()?;
        Some(String::from_utf16_lossy(&buf[..size as usize]))
    }
}

/// 当前前台窗口快照（标题 + 进程路径 + pid）。无前台窗口时返回 None。
pub fn foreground_snapshot() -> Option<ForegroundInfo> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let len = GetWindowTextLengthW(hwnd);
        let title = if len > 0 {
            let mut buf = vec![0u16; (len as usize) + 1];
            let n = GetWindowTextW(hwnd, &mut buf);
            String::from_utf16_lossy(&buf[..n as usize])
        } else {
            String::new()
        };
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let process_path = process_path_of(pid).unwrap_or_default();
        Some(ForegroundInfo {
            title,
            process_path,
            pid,
            hwnd: hwnd.0 as isize,
        })
    }
}
