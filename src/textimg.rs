//! 用 DirectWrite + Direct2D 把文本(含彩色 emoji)渲染成 RGBA 位图。
//! Slint 软件渲染器画不了彩色字体，但能画图片，所以把带 emoji 的总结渲成图再显示。
#![cfg(windows)]

use windows::core::PCWSTR;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_POINT_2F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Factory, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_METRICS,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, IWICImagingFactory, WICBitmapCacheOnLoad,
    GUID_WICPixelFormat32bppPBGRA, WICRect,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};

/// 渲染 `text` 为 (宽, 高, RGBA 直通 alpha)。max_w 为排版换行宽度(像素)，font_px 字号，color=0xRRGGBB。
pub fn render_text(text: &str, max_w: u32, font_px: f32, color: u32) -> Option<(u32, u32, Vec<u8>)> {
    if text.is_empty() {
        return None;
    }
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED); // 幂等：已初始化则忽略
        let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()?;
        let family: Vec<u16> = "Microsoft YaHei UI\0".encode_utf16().collect();
        let locale: Vec<u16> = "zh-cn\0".encode_utf16().collect();
        let format = dwrite
            .CreateTextFormat(
                PCWSTR(family.as_ptr()),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                font_px,
                PCWSTR(locale.as_ptr()),
            )
            .ok()?;
        let wtext: Vec<u16> = text.encode_utf16().collect();
        let layout = dwrite
            .CreateTextLayout(&wtext, &format, max_w as f32, 100000.0)
            .ok()?;
        let mut m = DWRITE_TEXT_METRICS::default();
        layout.GetMetrics(&mut m).ok()?;
        let width = max_w.max(1);
        let height = ((m.height.ceil() as u32) + 2).max(1);

        let wic: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let wicbmp = wic
            .CreateBitmap(width, height, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnLoad)
            .ok()?;

        let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok()?;
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        let rt = d2d.CreateWicBitmapRenderTarget(&wicbmp, &props).ok()?;
        let col = D2D1_COLOR_F {
            r: ((color >> 16) & 0xff) as f32 / 255.0,
            g: ((color >> 8) & 0xff) as f32 / 255.0,
            b: (color & 0xff) as f32 / 255.0,
            a: 1.0,
        };
        let brush = rt.CreateSolidColorBrush(&col, None).ok()?;
        let clear = D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
        rt.BeginDraw();
        rt.Clear(Some(&clear));
        rt.DrawTextLayout(
            D2D_POINT_2F { x: 0.0, y: 0.0 },
            &layout,
            &brush,
            D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
        );
        rt.EndDraw(None, None).ok()?;

        let mut buf = vec![0u8; (width * height * 4) as usize];
        let rect = WICRect {
            X: 0,
            Y: 0,
            Width: width as i32,
            Height: height as i32,
        };
        wicbmp.CopyPixels(&rect, width * 4, &mut buf).ok()?;
        // PBGRA(预乘) → RGBA(直通)：换序 + 反预乘
        for px in buf.chunks_exact_mut(4) {
            let (b, g, r, a) = (px[0] as u32, px[1] as u32, px[2] as u32, px[3] as u32);
            if a > 0 {
                px[0] = (r * 255 / a).min(255) as u8;
                px[1] = (g * 255 / a).min(255) as u8;
                px[2] = (b * 255 / a).min(255) as u8;
                px[3] = a as u8;
            } else {
                px[0] = 0;
                px[1] = 0;
                px[2] = 0;
                px[3] = 0;
            }
        }
        Some((width, height, buf))
    }
}
