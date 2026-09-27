//! 读取开机自启程序（HKCU/HKLM Run 键），得到 exe 文件名集合（小写），用于排除记录。
#![cfg(windows)]

use std::collections::HashSet;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegEnumValueW, RegOpenKeyExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE, REG_SZ,
};

const RUN_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// 本程序在 Run 键里的值名。
const APP_RUN_NAME: &str = "今天用啥";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 从命令行提取 exe 文件名（小写）：处理引号包裹与后缀参数。
fn exe_from_cmdline(cmd: &str) -> Option<String> {
    let cmd = cmd.trim();
    let path = if let Some(rest) = cmd.strip_prefix('"') {
        rest.split('"').next().unwrap_or(rest)
    } else {
        cmd.split_whitespace().next().unwrap_or(cmd)
    };
    let base = path.rsplit(['\\', '/']).next().unwrap_or(path);
    if base.is_empty() {
        None
    } else {
        Some(base.to_ascii_lowercase())
    }
}

fn read_run_key(root: HKEY, out: &mut HashSet<String>) {
    unsafe {
        let sub = wide(RUN_PATH);
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(root, PCWSTR(sub.as_ptr()), 0, KEY_READ, &mut hkey) != ERROR_SUCCESS {
            return;
        }
        let mut index = 0u32;
        loop {
            let mut name = [0u16; 512];
            let mut name_len = name.len() as u32;
            let mut data = [0u8; 4096];
            let mut data_len = data.len() as u32;
            let r = RegEnumValueW(
                hkey,
                index,
                PWSTR(name.as_mut_ptr()),
                &mut name_len,
                None,
                None,
                Some(data.as_mut_ptr()),
                Some(&mut data_len),
            );
            if r == ERROR_MORE_DATA {
                index += 1;
                continue;
            }
            if r != ERROR_SUCCESS {
                break;
            }
            let wlen = (data_len as usize) / 2;
            let wslice = std::slice::from_raw_parts(data.as_ptr() as *const u16, wlen);
            let s = String::from_utf16_lossy(wslice);
            if let Some(exe) = exe_from_cmdline(s.trim_end_matches('\0')) {
                out.insert(exe);
            }
            index += 1;
        }
        let _ = RegCloseKey(hkey);
    }
}

/// 开机自启程序的 exe 文件名集合（小写）。
pub fn autostart_exes() -> HashSet<String> {
    let mut out = HashSet::new();
    read_run_key(HKEY_CURRENT_USER, &mut out);
    read_run_key(HKEY_LOCAL_MACHINE, &mut out);
    out
}

/// 根据 enable 在 HKCU Run 键里写入/删除本程序的开机自启项（指向 exe_path）。
pub fn set_autostart(enable: bool, exe_path: &str) -> Result<(), String> {
    unsafe {
        let sub = wide(RUN_PATH);
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(sub.as_ptr()), 0, KEY_SET_VALUE, &mut hkey)
            != ERROR_SUCCESS
        {
            return Err("打开 HKCU Run 键失败".into());
        }
        let name = wide(APP_RUN_NAME);
        let rc = if enable {
            let val = wide(&format!("\"{exe_path}\""));
            let bytes = std::slice::from_raw_parts(val.as_ptr() as *const u8, val.len() * 2);
            RegSetValueExW(hkey, PCWSTR(name.as_ptr()), 0, REG_SZ, Some(bytes))
        } else {
            RegDeleteValueW(hkey, PCWSTR(name.as_ptr()))
        };
        let _ = RegCloseKey(hkey);
        // 关闭自启时值本就不存在，视为成功
        if rc == ERROR_SUCCESS || (!enable && rc == ERROR_FILE_NOT_FOUND) {
            Ok(())
        } else {
            Err(format!("写 Run 值失败: {rc:?}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::exe_from_cmdline;
    #[test]
    fn extracts_exe() {
        assert_eq!(exe_from_cmdline("\"C:\\Program Files\\App\\app.exe\" --min").as_deref(), Some("app.exe"));
        assert_eq!(exe_from_cmdline("C:\\tools\\Tool.exe /run").as_deref(), Some("tool.exe"));
    }
}
