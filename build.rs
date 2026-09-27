fn main() {
    slint_build::compile("ui/dashboard.slint").expect("compile dashboard.slint");

    // Windows: 把 Logo 嵌成 exe 图标资源（任务栏/资源管理器/Alt-Tab 都用它）。
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=assets/icon.png");
        let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("icon.ico");
        if let Ok(img) = image::open("assets/icon.png") {
            let ico = img.resize_exact(256, 256, image::imageops::FilterType::Lanczos3);
            if ico.save_with_format(&out, image::ImageFormat::Ico).is_ok() {
                let mut res = winresource::WindowsResource::new();
                res.set_icon(out.to_str().unwrap());
                if let Err(e) = res.compile() {
                    println!("cargo:warning=winresource embed failed: {e}");
                }
            }
        }
    }
}
