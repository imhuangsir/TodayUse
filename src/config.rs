use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiGranularity {
    Day,
    Hour,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Desensitize {
    pub domains: bool,
    pub titles: bool,
}
impl Default for Desensitize {
    fn default() -> Self {
        Self { domains: false, titles: false }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub poll_interval_sec: u64,
    pub idle_threshold_sec: u64,
    pub flush_interval_sec: u64,
    pub flush_max_events: usize,
    pub excluded_apps: Vec<String>,
    pub excluded_domains: Vec<String>,
    pub record_private: bool,
    pub ai_enabled: bool,
    pub ai_granularity: AiGranularity,
    pub ai_base_url: String,
    pub ai_model: String,
    pub reduced_motion: bool,
    pub autostart: bool,
    pub desensitize: Desensitize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval_sec: 5,
            idle_threshold_sec: 60,
            flush_interval_sec: 15,
            flush_max_events: 200,
            excluded_apps: Vec::new(),
            excluded_domains: Vec::new(),
            record_private: false,
            ai_enabled: false,
            ai_granularity: AiGranularity::Day,
            ai_base_url: String::new(),
            ai_model: String::new(),
            reduced_motion: false,
            autostart: false,
            desensitize: Desensitize::default(),
        }
    }
}

impl Config {
    /// 文件不存在 → 返回默认；存在 → 解析（缺字段用默认补齐）。
    pub fn load(path: &Path) -> Result<Config, String> {
        if !path.exists() {
            return Ok(Config::default());
        }
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        toml::from_str(&text).map_err(|e| e.to_string())
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_spec() {
        let c = Config::default();
        assert_eq!(c.poll_interval_sec, 5);
        assert_eq!(c.idle_threshold_sec, 60);
        assert_eq!(c.flush_interval_sec, 15);
        assert_eq!(c.flush_max_events, 200);
        assert_eq!(c.ai_granularity, AiGranularity::Day);
        assert!(!c.ai_enabled);
        assert!(!c.record_private);
    }

    #[test]
    fn missing_file_returns_default() {
        let p = std::env::temp_dir().join("at_no_such_config_xyz.toml");
        let _ = std::fs::remove_file(&p);
        assert_eq!(Config::load(&p).unwrap(), Config::default());
    }

    #[test]
    fn roundtrip_preserves_values() {
        let mut c = Config::default();
        c.idle_threshold_sec = 120;
        c.excluded_apps = vec!["private.exe".into()];
        c.ai_enabled = true;
        let p = std::env::temp_dir().join("at_roundtrip_cfg.toml");
        c.save(&p).unwrap();
        let loaded = Config::load(&p).unwrap();
        assert_eq!(loaded, c);
        std::fs::remove_file(&p).unwrap();
    }

    #[test]
    fn partial_toml_fills_defaults() {
        let c: Config = toml::from_str("idle_threshold_sec = 30").unwrap();
        assert_eq!(c.idle_threshold_sec, 30);
        assert_eq!(c.poll_interval_sec, 5);
    }
}
