//! 内嵌应用 Logo（软件使用记录图标），供托盘图标与窗口图标使用。
const LOGO_PNG: &[u8] = include_bytes!("../assets/icon.png");

/// 解码内嵌 Logo 并缩放到 size×size 的 RGBA。
pub fn logo_rgba(size: u32) -> Option<(u32, u32, Vec<u8>)> {
    let img = image::load_from_memory(LOGO_PNG).ok()?;
    let img = img
        .resize_exact(size, size, image::imageops::FilterType::Triangle)
        .to_rgba8();
    Some((size, size, img.into_raw()))
}
