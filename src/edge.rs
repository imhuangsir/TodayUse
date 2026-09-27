//! 通过 UI Automation 读取 Edge 当前标签地址栏 URL（启发式）。失败一律 None，绝不 panic。
//! 注意：地址栏元素随 Edge 版本变化，本实现用"找 value 形似 URL 的 Edit 控件"启发式，
//! 真机读取正确性需用户开着 Edge 交互验证。
#![cfg(windows)]

use windows::core::Interface;
use windows::core::VARIANT;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationValuePattern, TreeScope_Descendants,
    UIA_ControlTypePropertyId, UIA_EditControlTypeId, UIA_ValuePatternId,
};

/// value 是否形似 URL（含协议，或点分域名且无空格/反斜杠）。
pub fn looks_like_url(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() || s.contains(' ') || s.contains('\\') {
        return false;
    }
    s.starts_with("http://")
        || s.starts_with("https://")
        || (s.contains('.') && s.split('.').count() >= 2 && s.len() >= 4)
}

/// 前台 Edge 窗口的当前 URL（尽力而为）。hwnd 为前台窗口句柄的 isize 表示。
pub fn edge_url(hwnd_isize: isize) -> Option<String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let automation: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_ALL).ok()?;
        let hwnd = HWND(hwnd_isize as *mut core::ffi::c_void);
        let root = automation.ElementFromHandle(hwnd).ok()?;
        let value = VARIANT::from(UIA_EditControlTypeId.0);
        let cond = automation
            .CreatePropertyCondition(UIA_ControlTypePropertyId, &value)
            .ok()?;
        let edits = root.FindAll(TreeScope_Descendants, &cond).ok()?;
        let len = edits.Length().ok()?;
        for i in 0..len {
            let el = match edits.GetElement(i) {
                Ok(e) => e,
                Err(_) => continue,
            };
            let pat = match el.GetCurrentPattern(UIA_ValuePatternId) {
                Ok(p) => p,
                Err(_) => continue,
            };
            if let Ok(vp) = pat.cast::<IUIAutomationValuePattern>() {
                if let Ok(bstr) = vp.CurrentValue() {
                    let s = bstr.to_string();
                    if looks_like_url(&s) {
                        return Some(s);
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::looks_like_url;
    #[test]
    fn url_heuristic() {
        assert!(looks_like_url("https://youtube.com/watch?v=x"));
        assert!(looks_like_url("github.com/a/b"));
        assert!(!looks_like_url("hello world"));
        assert!(!looks_like_url(""));
        assert!(!looks_like_url("C:\\path\\file"));
    }
}
