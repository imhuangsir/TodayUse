use simplelog::{ConfigBuilder, LevelFilter, WriteLogger};
use std::fs::OpenOptions;
use std::path::Path;

/// 初始化写文件日志（append）。失败不 panic（无日志也要能跑）。
pub fn init(log_path: &Path) {
    if let Some(dir) = log_path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(file) = OpenOptions::new().create(true).append(true).open(log_path) {
        let _ = WriteLogger::init(LevelFilter::Info, ConfigBuilder::new().build(), file);
    }
}
