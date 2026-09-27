//! 进程可执行名 → 通用应用名映射。未命中则去 .exe 后回退（纯小写英文首字母大写）。
pub fn friendly_name(exe_basename: &str) -> String {
    let key = exe_basename.to_ascii_lowercase();
    let mapped = match key.as_str() {
        "msedge.exe" => "Edge",
        "chrome.exe" => "Chrome",
        "firefox.exe" => "Firefox",
        "iexplore.exe" => "IE",
        "code.exe" => "VS Code",
        "devenv.exe" => "Visual Studio",
        "idea64.exe" => "IntelliJ IDEA",
        "pycharm64.exe" => "PyCharm",
        "goland64.exe" => "GoLand",
        "clion64.exe" => "CLion",
        "webstorm64.exe" => "WebStorm",
        "rustrover64.exe" => "RustRover",
        "cursor.exe" => "Cursor",
        "sublime_text.exe" => "Sublime Text",
        "notepad.exe" => "记事本",
        "notepad++.exe" => "Notepad++",
        "explorer.exe" => "文件资源管理器",
        "windowsterminal.exe" | "openconsole.exe" => "终端",
        "cmd.exe" => "命令提示符",
        "powershell.exe" | "pwsh.exe" => "PowerShell",
        "wechat.exe" | "weixin.exe" => "微信",
        "qq.exe" => "QQ",
        "tim.exe" => "TIM",
        "dingtalk.exe" => "钉钉",
        "feishu.exe" | "lark.exe" => "飞书",
        "wps.exe" | "wpp.exe" | "et.exe" => "WPS",
        "winword.exe" => "Word",
        "excel.exe" => "Excel",
        "powerpnt.exe" => "PowerPoint",
        "outlook.exe" => "Outlook",
        "cloudmusic.exe" => "网易云音乐",
        "qqmusic.exe" => "QQ音乐",
        "kugou.exe" => "酷狗音乐",
        "spotify.exe" => "Spotify",
        "potplayer64.exe" | "potplayermini64.exe" => "PotPlayer",
        "vlc.exe" => "VLC",
        "photoshop.exe" => "Photoshop",
        "illustrator.exe" => "Illustrator",
        "steam.exe" => "Steam",
        "discord.exe" => "Discord",
        "telegram.exe" => "Telegram",
        "obsidian.exe" => "Obsidian",
        "doubao.exe" => "豆包",
        "python.exe" | "pythonw.exe" => "Python",
        "node.exe" => "Node.js",
        _ => "",
    };
    if !mapped.is_empty() {
        return mapped.to_string();
    }
    let stem = exe_basename
        .strip_suffix(".exe")
        .or_else(|| exe_basename.strip_suffix(".EXE"))
        .unwrap_or(exe_basename);
    if stem.is_empty() {
        return exe_basename.to_string();
    }
    if stem.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) {
        let mut it = stem.chars();
        match it.next() {
            Some(f) => f.to_uppercase().collect::<String>() + it.as_str(),
            None => stem.to_string(),
        }
    } else {
        stem.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::friendly_name;
    #[test]
    fn maps_common_and_falls_back() {
        assert_eq!(friendly_name("msedge.exe"), "Edge");
        assert_eq!(friendly_name("Code.exe"), "VS Code");
        assert_eq!(friendly_name("weixin.exe"), "微信");
        assert_eq!(friendly_name("cursor.exe"), "Cursor"); // 回退首字母大写
        assert_eq!(friendly_name("Doubao.exe"), "豆包"); // 命中(小写匹配)
        assert_eq!(friendly_name("STM32CubeMX.exe"), "STM32CubeMX"); // 非纯小写原样去后缀
    }
}
