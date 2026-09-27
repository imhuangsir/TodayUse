//! 应用 Logo：优先读用户可替换的图标文件，否则用内嵌默认图。
//! 用户可把自己的 icon.png 或 icon.ico 放到 %APPDATA%\ActivityTracker\ 下替换（重启生效）。
const LOGO_PNG: &[u8] = include_bytes!("../assets/icon.png");

fn user_icon_bytes() -> Option<Vec<u8>> {
    let base = std::env::var("APPDATA").ok()?;
    let dir = std::path::PathBuf::from(base).join("ActivityTracker");
    for name in ["icon.png", "icon.ico"] {
        if let Ok(bytes) = std::fs::read(dir.join(name)) {
            if !bytes.is_empty() {
                return Some(bytes);
            }
        }
    }
    None
}

/// 解码 Logo（用户自定义优先）并高质量缩放到 size×size 的 RGBA。
pub fn logo_rgba(size: u32) -> Option<(u32, u32, Vec<u8>)> {
    let img = match user_icon_bytes() {
        Some(b) => image::load_from_memory(&b).ok()?,
        None => image::load_from_memory(LOGO_PNG).ok()?,
    };
    let img = img
        .resize_exact(size, size, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    Some((size, size, img.into_raw()))
}
