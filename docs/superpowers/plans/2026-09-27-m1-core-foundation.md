# M1 基础核心 实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 subagent-driven-development（推荐）或 executing-plans 逐任务实现此计划。步骤使用复选框（`- [ ]`）语法来跟踪进度。

**目标：** 搭建纯 Rust 的活动记录核心——配置、解析、会话合并、SQLite 存储与聚合，不含任何 Win32/UI，可跨平台单测。

**架构：** 单 package（lib + bin）。lib 暴露纯逻辑模块（model/config/parse/session/storage/aggregate），采集与 UI 在后续里程碑接入。事件经会话合并→批量写 SQLite(WAL)→聚合查询。所有时间以 UTC 秒存储，日/小时分桶时传入 `UtcOffset` 参数（依赖注入，保持纯函数可测）。

**技术栈：** Rust 2021、`rusqlite`(bundled)、`serde`/`serde_json`/`toml`、`time`、`thiserror`、`log`/`simplelog`。

**规格：** `docs/superpowers/specs/2026-09-27-activity-tracker-design.md`（执行者两份都读；本计划仅覆盖规格第 2/4.5/4.6/4.7/5/14 节的纯核心部分）。

## 全局约束

- 语言：Rust edition 2021；仅稳定版工具链。
- 轻量化：不引入 M1 用不到的依赖；发布 profile 见任务 1（`opt-level="z"`、`lto=true`、`codegen-units=1`、`panic="abort"`、`strip=true`）。
- 时间：存储用 UTC Unix 秒（i64）；日/小时分桶按传入的本地 `UtcOffset`，不在库内读系统时区（由 bin 提供）。
- 数据库路径运行时来自配置；测试一律用临时文件或 `:memory:`，绝不碰真实用户数据。
- 命名：snake_case 模块/函数，类型 UpperCamelCase。字段名与规格第 5 节表结构逐字一致。
- 每个任务结束必须 `cargo test` 全绿并 commit。
- Commit 信息结尾附：`Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`

## 文件结构

- `Cargo.toml` — 包定义、依赖、发布 profile。
- `src/lib.rs` — 模块声明与再导出。
- `src/model.rs` — 核心类型：`ActivityEvent`、`MediaEvent`、`Session`、`PlaybackStatus`。
- `src/config.rs` — `Config` 结构、默认值、TOML 读写。
- `src/parse.rs` — 纯解析：`extract_domain`、`parse_edge_title`、`detect_video`。
- `src/session.rs` — `SessionBuilder`：事件→会话合并（纯逻辑）。
- `src/storage.rs` — SQLite：建表/WAL、批量写、心跳、崩溃恢复、按范围读。
- `src/aggregate.rs` — 聚合：按应用/域名/视频/时间线，本地时区分桶。
- `src/logging.rs` — 日志初始化。
- `src/main.rs` — CLI：`report`/`replay` 调试子命令，装配以上模块。
- `tests/storage_integration.rs` — 存储+聚合端到端集成测试。

---

## 里程碑路线图（M1 为本计划，M2–M5 到达时各自出计划）

- **M1（本计划）**：纯 Rust 核心——配置/解析/会话合并/存储/聚合。产出：可 `replay` 合成事件并 `report` 聚合结果的无头程序，全模块单测。
- **M2**：前台窗口采集器（`SetWinEventHook`）+ 空闲检测（`GetLastInputInfo`），接入 M1 存储。产出：能记录真实应用使用的后台程序。
- **M3**：Edge UIA 标签采集 + SMTC 媒体采集。产出：URL/页面/视频标题与进度。
- **M4**：AI 总结（聚合→脱敏→OpenAI 兼容接口）。产出：可生成/存储自然语言总结。
- **M5**：托盘 + Slint 窗口 + DPAPI 密钥 + 单实例 + 开机自启。产出：完整可交付应用。

---

### 任务 1：Cargo 脚手架 + 核心类型

**文件：**
- 创建：`Cargo.toml`
- 创建：`src/lib.rs`
- 创建：`src/model.rs`
- 创建：`src/main.rs`（占位，仅让 bin 可编译）

- [ ] **步骤 1：编写 `Cargo.toml`**

```toml
[package]
name = "activity-tracker"
version = "0.1.0"
edition = "2021"

[dependencies]
rusqlite = { version = "0.31", features = ["bundled"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
time = { version = "0.3", features = ["formatting", "parsing", "macros", "local-offset"] }
log = "0.4"
simplelog = "0.12"

[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
panic = "abort"
strip = true
```

- [ ] **步骤 2：编写 `src/model.rs`（含失败测试）**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaybackStatus { Playing, Paused, Stopped }

/// 采集器发给存储线程的前台活动事件（时间为 UTC 秒）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActivityEvent {
    pub ts: i64,
    pub app_name: String,
    pub process_path: String,
    pub window_title: String,
    pub edge_url: Option<String>,
    pub page_title: Option<String>,
    pub is_private: bool,
    pub is_idle: bool,
}

/// 媒体事件，更新当前会话的 media_* 字段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaEvent {
    pub ts: i64,
    pub media_title: String,
    pub media_player: String,
    pub media_status: PlaybackStatus,
    pub position_sec: Option<i64>,
    pub duration_sec: Option<i64>,
}
```

继续 `src/model.rs`，追加 `Session` 与合并期用的 `close`：

```rust
/// 一段合并后的活动会话（对应 activity 表一行）。
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub start_ts: i64,
    pub end_ts: i64,
    pub duration_sec: i64,
    pub app_name: String,
    pub process_path: String,
    pub window_title: String,
    pub edge_url: Option<String>,
    pub page_title: Option<String>,
    pub media_title: Option<String>,
    pub media_player: Option<String>,
    pub media_status: Option<PlaybackStatus>,
    pub media_position_sec: Option<i64>,
    pub media_duration_sec: Option<i64>,
    pub is_private: bool,
    pub is_idle: bool,
}

impl Session {
    /// 用 end_ts 结算会话，重算 duration（不为负）。
    pub fn close(&mut self, end_ts: i64) {
        self.end_ts = end_ts;
        self.duration_sec = (end_ts - self.start_ts).max(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Session {
        Session {
            start_ts: 1000, end_ts: 1000, duration_sec: 0,
            app_name: "Code".into(), process_path: "C:/code.exe".into(),
            window_title: "main.rs".into(), edge_url: None, page_title: None,
            media_title: None, media_player: None, media_status: None,
            media_position_sec: None, media_duration_sec: None,
            is_private: false, is_idle: false,
        }
    }

    #[test]
    fn close_computes_duration() {
        let mut s = sample();
        s.close(1090);
        assert_eq!(s.end_ts, 1090);
        assert_eq!(s.duration_sec, 90);
    }

    #[test]
    fn close_never_negative() {
        let mut s = sample();
        s.close(900);
        assert_eq!(s.duration_sec, 0);
    }
}
```

- [ ] **步骤 3：编写 `src/lib.rs` 与占位 `src/main.rs`**

```rust
// src/lib.rs
pub mod model;
```

```rust
// src/main.rs
fn main() {
    println!("activity-tracker core (M1)");
}
```

- [ ] **步骤 4：运行测试验证通过**

运行：`cargo test`
预期：PASS（`close_computes_duration`、`close_never_negative`），bin 与 lib 均编译通过。

- [ ] **步骤 5：Commit**

```bash
git add Cargo.toml Cargo.lock src/
git commit -m "feat(model): 核心类型 ActivityEvent/MediaEvent/Session 与 close 时长计算"
```

### 任务 2：配置（config.rs）

**文件：**
- 创建：`src/config.rs`
- 修改：`src/lib.rs`（加 `pub mod config;`）

- [ ] **步骤 1：编写 `src/config.rs`（结构 + 默认 + 读写 + 失败测试）**

```rust
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiGranularity { Day, Hour }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Desensitize {
    pub domains: bool,
    pub titles: bool,
}
impl Default for Desensitize {
    fn default() -> Self { Self { domains: false, titles: false } }
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
```

继续 `src/config.rs`，加载入/保存与测试：

```rust
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
        assert_eq!(c.poll_interval_sec, 5); // 缺失字段用默认
    }
}
```

- [ ] **步骤 2：在 `src/lib.rs` 加 `pub mod config;`**

- [ ] **步骤 3：运行测试验证通过**

运行：`cargo test config`
预期：4 个测试 PASS。

- [ ] **步骤 4：Commit**

```bash
git add src/config.rs src/lib.rs
git commit -m "feat(config): Config 默认值 + TOML 读写 + 缺字段补默认"
```

### 任务 3：解析（parse.rs）

**文件：**
- 创建：`src/parse.rs`
- 修改：`src/lib.rs`（加 `pub mod parse;`）

- [ ] **步骤 1：编写 `src/parse.rs`（纯函数）**

```rust
/// 从 URL 提取主机域名（去 scheme/userinfo/port/path/www.）。
pub fn extract_domain(url: &str) -> Option<String> {
    let after_scheme = url.split("://").nth(1).unwrap_or(url);
    let host = after_scheme.split(|c| c == '/' || c == '?' || c == '#').next().unwrap_or("");
    let host = host.rsplit('@').next().unwrap_or(host); // 去 userinfo
    let host = host.split(':').next().unwrap_or(host);   // 去端口
    let host = host.strip_prefix("www.").unwrap_or(host);
    if host.is_empty() { None } else { Some(host.to_ascii_lowercase()) }
}

fn strip_leading_count(t: &str) -> &str {
    // 去掉未读计数前缀，如 "(3) "
    if let Some(rest) = t.strip_prefix('(') {
        if let Some(idx) = rest.find(") ") {
            if rest[..idx].chars().all(|c| c.is_ascii_digit()) && !rest[..idx].is_empty() {
                return &rest[idx + 2..];
            }
        }
    }
    t
}

/// 从 Edge 窗口标题解析页面标题（去尾部 " - Microsoft Edge"）。
pub fn parse_edge_title(window_title: &str) -> Option<String> {
    let t = strip_leading_count(window_title.trim());
    for suffix in [" - Microsoft Edge", " – Microsoft Edge"] {
        if let Some(idx) = t.rfind(suffix) {
            let page = t[..idx].trim();
            return if page.is_empty() { None } else { Some(page.to_string()) };
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq)]
pub struct VideoInfo { pub site: String, pub title: String }

/// 识别常见视频站点并提取视频标题。page_title 传已解析的页面标题。
pub fn detect_video(url: &str, page_title: Option<&str>) -> Option<VideoInfo> {
    let domain = extract_domain(url)?;
    let title = page_title.unwrap_or("").trim();
    if (domain.ends_with("youtube.com") && url.contains("/watch")) || domain == "youtu.be" {
        let t = title.strip_suffix(" - YouTube").unwrap_or(title).trim();
        return Some(VideoInfo { site: "YouTube".into(), title: t.to_string() });
    }
    if domain.ends_with("bilibili.com") && url.contains("/video/") {
        let mut t = title;
        for suf in ["_哔哩哔哩_bilibili", "_哔哩哔哩bilibili", "_bilibili"] {
            if let Some(s) = t.strip_suffix(suf) { t = s.trim_end_matches('_'); break; }
        }
        return Some(VideoInfo { site: "Bilibili".into(), title: t.trim().to_string() });
    }
    None
}
```

继续 `src/parse.rs`，追加测试：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_basic() {
        assert_eq!(extract_domain("https://www.youtube.com/watch?v=abc").as_deref(), Some("youtube.com"));
        assert_eq!(extract_domain("http://user:pw@Example.COM:8080/x").as_deref(), Some("example.com"));
        assert_eq!(extract_domain("https://youtu.be/abc").as_deref(), Some("youtu.be"));
        assert_eq!(extract_domain(""), None);
    }

    #[test]
    fn edge_title_strips_suffix_and_count() {
        assert_eq!(parse_edge_title("GitHub - Microsoft Edge").as_deref(), Some("GitHub"));
        assert_eq!(parse_edge_title("(3) 收件箱 - Microsoft Edge").as_deref(), Some("收件箱"));
        assert_eq!(parse_edge_title("某视频 - YouTube - Microsoft Edge").as_deref(), Some("某视频 - YouTube"));
        assert_eq!(parse_edge_title("记事本"), None); // 非 Edge 标题
    }

    #[test]
    fn detect_youtube() {
        let v = detect_video("https://www.youtube.com/watch?v=abc", Some("某视频 - YouTube")).unwrap();
        assert_eq!(v.site, "YouTube");
        assert_eq!(v.title, "某视频");
    }

    #[test]
    fn detect_bilibili() {
        let v = detect_video("https://www.bilibili.com/video/BV1xx", Some("标题_哔哩哔哩_bilibili")).unwrap();
        assert_eq!(v.site, "Bilibili");
        assert_eq!(v.title, "标题");
    }

    #[test]
    fn detect_none_for_normal_site() {
        assert_eq!(detect_video("https://github.com/x", Some("GitHub")), None);
    }
}
```

- [ ] **步骤 2：在 `src/lib.rs` 加 `pub mod parse;`**

- [ ] **步骤 3：运行测试验证通过**

运行：`cargo test parse`
预期：5 个测试 PASS。

- [ ] **步骤 4：Commit**

```bash
git add src/parse.rs src/lib.rs
git commit -m "feat(parse): 域名提取 + Edge 标题解析 + YouTube/B站视频识别"
```

### 任务 4：会话合并（session.rs）

**文件：**
- 创建：`src/session.rs`
- 修改：`src/lib.rs`（加 `pub mod session;`）

> **合并键（M1）**：`(app_name, window_title, edge_url, is_private, is_idle)`。规格第 4.5 节键里含 `media_title`，但媒体经 `on_media` 富化当前会话；由于浏览器标签标题通常随视频变化（window_title 变→自然切分），M1 不把 media_title 纳入切分键（简化，后续里程碑可加"媒体标题变化即切分"）。

- [ ] **步骤 1：编写 `src/session.rs`（实现）**

```rust
use crate::model::{ActivityEvent, MediaEvent, Session};

fn same_key(s: &Session, ev: &ActivityEvent) -> bool {
    s.app_name == ev.app_name
        && s.window_title == ev.window_title
        && s.edge_url == ev.edge_url
        && s.is_private == ev.is_private
        && s.is_idle == ev.is_idle
}

fn session_from_event(ev: &ActivityEvent) -> Session {
    Session {
        start_ts: ev.ts, end_ts: ev.ts, duration_sec: 0,
        app_name: ev.app_name.clone(), process_path: ev.process_path.clone(),
        window_title: ev.window_title.clone(), edge_url: ev.edge_url.clone(),
        page_title: ev.page_title.clone(),
        media_title: None, media_player: None, media_status: None,
        media_position_sec: None, media_duration_sec: None,
        is_private: ev.is_private, is_idle: ev.is_idle,
    }
}

/// 把事件流合并为会话。切分键变化时结算上一段并开新段。
#[derive(Default)]
pub struct SessionBuilder {
    current: Option<Session>,
}

impl SessionBuilder {
    pub fn new() -> Self { Self { current: None } }

    /// 处理活动事件；若发生切换，返回被结算的旧会话。
    pub fn on_activity(&mut self, ev: &ActivityEvent) -> Option<Session> {
        let changed = self.current.as_ref().map_or(true, |c| !same_key(c, ev));
        if changed {
            let closed = self.current.take().map(|mut s| { s.close(ev.ts); s });
            self.current = Some(session_from_event(ev));
            closed
        } else {
            if let Some(c) = self.current.as_mut() { c.close(ev.ts); }
            None
        }
    }

    /// 媒体事件富化当前会话（标题/状态/进度）。
    pub fn on_media(&mut self, m: &MediaEvent) {
        if let Some(c) = self.current.as_mut() {
            c.media_title = Some(m.media_title.clone());
            c.media_player = Some(m.media_player.clone());
            c.media_status = Some(m.media_status);
            c.media_position_sec = m.position_sec;
            c.media_duration_sec = m.duration_sec;
        }
    }

    /// 心跳：把当前会话 end_ts 延长到 ts（不结算）。
    pub fn touch(&mut self, ts: i64) {
        if let Some(c) = self.current.as_mut() { c.close(ts); }
    }

    /// 结算并取出当前会话（暂停/退出时用）。
    pub fn finish(&mut self, ts: i64) -> Option<Session> {
        self.current.take().map(|mut s| { s.close(ts); s })
    }
}
```

继续 `src/session.rs`，追加测试：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PlaybackStatus;

    fn act(ts: i64, app: &str, win: &str) -> ActivityEvent {
        ActivityEvent { ts, app_name: app.into(), process_path: "x".into(),
            window_title: win.into(), edge_url: None, page_title: None,
            is_private: false, is_idle: false }
    }

    #[test]
    fn same_activity_extends() {
        let mut b = SessionBuilder::new();
        assert!(b.on_activity(&act(1000, "Code", "main.rs")).is_none());
        assert!(b.on_activity(&act(1030, "Code", "main.rs")).is_none());
        let s = b.finish(1060).unwrap();
        assert_eq!(s.start_ts, 1000);
        assert_eq!(s.duration_sec, 60);
    }

    #[test]
    fn key_change_closes_previous() {
        let mut b = SessionBuilder::new();
        b.on_activity(&act(1000, "Code", "main.rs"));
        let closed = b.on_activity(&act(1040, "Edge", "GitHub")).unwrap();
        assert_eq!(closed.app_name, "Code");
        assert_eq!(closed.end_ts, 1040);
        assert_eq!(closed.duration_sec, 40);
        let s = b.finish(1100).unwrap();
        assert_eq!(s.app_name, "Edge");
        assert_eq!(s.duration_sec, 60);
    }

    #[test]
    fn idle_flag_splits_session() {
        let mut b = SessionBuilder::new();
        b.on_activity(&act(1000, "Code", "main.rs"));
        let mut idle_ev = act(1050, "Code", "main.rs");
        idle_ev.is_idle = true;
        let closed = b.on_activity(&idle_ev).unwrap();
        assert!(!closed.is_idle);
        assert_eq!(closed.duration_sec, 50);
    }

    #[test]
    fn media_enriches_current() {
        let mut b = SessionBuilder::new();
        b.on_activity(&act(2000, "Edge", "vid - YouTube - Microsoft Edge"));
        b.on_media(&MediaEvent { ts: 2005, media_title: "vid".into(),
            media_player: "msedge".into(), media_status: PlaybackStatus::Playing,
            position_sec: Some(30), duration_sec: Some(600) });
        let s = b.finish(2100).unwrap();
        assert_eq!(s.media_title.as_deref(), Some("vid"));
        assert_eq!(s.media_position_sec, Some(30));
        assert_eq!(s.media_status, Some(PlaybackStatus::Playing));
    }
}
```

- [ ] **步骤 2：在 `src/lib.rs` 加 `pub mod session;`**

- [ ] **步骤 3：运行测试验证通过**

运行：`cargo test session`
预期：4 个测试 PASS。

- [ ] **步骤 4：Commit**

```bash
git add src/session.rs src/lib.rs
git commit -m "feat(session): SessionBuilder 事件→会话合并（切分键/心跳/媒体富化）"
```

### 任务 5：存储 schema + 批量写入 + 范围查询（storage.rs）

**文件：**
- 创建：`src/storage.rs`
- 修改：`src/lib.rs`（加 `pub mod storage;`）

- [ ] **步骤 1：编写 `src/storage.rs`（schema/open/insert/query）**

```rust
use crate::model::{PlaybackStatus, Session};
use rusqlite::{params, Connection, Row};

const SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS activity (
  id INTEGER PRIMARY KEY,
  start_ts INTEGER NOT NULL, end_ts INTEGER NOT NULL, duration_sec INTEGER NOT NULL,
  app_name TEXT NOT NULL, process_path TEXT NOT NULL, window_title TEXT NOT NULL,
  edge_url TEXT, page_title TEXT,
  media_title TEXT, media_player TEXT, media_status TEXT,
  media_position_sec INTEGER, media_duration_sec INTEGER,
  is_private INTEGER NOT NULL, is_idle INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS idx_activity_start ON activity(start_ts);
CREATE INDEX IF NOT EXISTS idx_activity_app_start ON activity(app_name, start_ts);
CREATE TABLE IF NOT EXISTS summaries (
  id INTEGER PRIMARY KEY, range_start INTEGER NOT NULL, range_end INTEGER NOT NULL,
  granularity TEXT NOT NULL, text TEXT NOT NULL, created_ts INTEGER NOT NULL, model TEXT);
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);";

fn status_to_str(s: PlaybackStatus) -> &'static str {
    match s { PlaybackStatus::Playing => "playing", PlaybackStatus::Paused => "paused", PlaybackStatus::Stopped => "stopped" }
}
fn status_from_str(s: &str) -> Option<PlaybackStatus> {
    match s { "playing" => Some(PlaybackStatus::Playing), "paused" => Some(PlaybackStatus::Paused), "stopped" => Some(PlaybackStatus::Stopped), _ => None }
}

pub struct Storage { conn: Connection }

impl Storage {
    pub fn open(path: &str) -> Result<Storage, String> {
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.pragma_update(None, "journal_mode", "WAL").map_err(|e| e.to_string())?;
        conn.pragma_update(None, "synchronous", "NORMAL").map_err(|e| e.to_string())?;
        conn.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
        Ok(Storage { conn })
    }

    #[cfg(test)]
    pub fn open_memory() -> Result<Storage, String> {
        let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
        conn.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
        Ok(Storage { conn })
    }

    pub fn insert_sessions(&mut self, sessions: &[Session]) -> Result<(), String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO activity (start_ts,end_ts,duration_sec,app_name,process_path,window_title,\
                 edge_url,page_title,media_title,media_player,media_status,media_position_sec,\
                 media_duration_sec,is_private,is_idle) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)"
            ).map_err(|e| e.to_string())?;
            for s in sessions {
                stmt.execute(params![
                    s.start_ts, s.end_ts, s.duration_sec, s.app_name, s.process_path, s.window_title,
                    s.edge_url, s.page_title, s.media_title, s.media_player,
                    s.media_status.map(status_to_str), s.media_position_sec, s.media_duration_sec,
                    s.is_private as i64, s.is_idle as i64
                ]).map_err(|e| e.to_string())?;
            }
        }
        tx.commit().map_err(|e| e.to_string())
    }
}
```

继续 `src/storage.rs`，加范围查询 + 行映射 + 单测：

```rust
fn row_to_session(r: &Row) -> rusqlite::Result<Session> {
    let status: Option<String> = r.get("media_status")?;
    Ok(Session {
        start_ts: r.get("start_ts")?, end_ts: r.get("end_ts")?, duration_sec: r.get("duration_sec")?,
        app_name: r.get("app_name")?, process_path: r.get("process_path")?, window_title: r.get("window_title")?,
        edge_url: r.get("edge_url")?, page_title: r.get("page_title")?,
        media_title: r.get("media_title")?, media_player: r.get("media_player")?,
        media_status: status.as_deref().and_then(status_from_str),
        media_position_sec: r.get("media_position_sec")?, media_duration_sec: r.get("media_duration_sec")?,
        is_private: r.get::<_, i64>("is_private")? != 0, is_idle: r.get::<_, i64>("is_idle")? != 0,
    })
}

impl Storage {
    /// 与 [start,end) 有重叠的会话，按开始时间升序。
    pub fn sessions_in_range(&self, start: i64, end: i64) -> Result<Vec<Session>, String> {
        let mut stmt = self.conn.prepare(
            "SELECT * FROM activity WHERE start_ts < ? AND end_ts > ? ORDER BY start_ts"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map(params![end, start], row_to_session).map_err(|e| e.to_string())?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sess(start: i64, end: i64, app: &str) -> Session {
        let mut s = Session {
            start_ts: start, end_ts: end, duration_sec: end - start,
            app_name: app.into(), process_path: "p".into(), window_title: "w".into(),
            edge_url: None, page_title: None, media_title: None, media_player: None,
            media_status: None, media_position_sec: None, media_duration_sec: None,
            is_private: false, is_idle: false,
        };
        s.close(end); s
    }

    #[test]
    fn insert_and_query_roundtrip() {
        let mut st = Storage::open_memory().unwrap();
        let mut a = sess(1000, 1100, "Code");
        a.media_title = Some("v".into());
        a.media_status = Some(PlaybackStatus::Playing);
        a.media_position_sec = Some(42);
        st.insert_sessions(&[a.clone(), sess(2000, 2050, "Edge")]).unwrap();
        let got = st.sessions_in_range(900, 1500).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].app_name, "Code");
        assert_eq!(got[0].media_status, Some(PlaybackStatus::Playing));
        assert_eq!(got[0].media_position_sec, Some(42));
    }

    #[test]
    fn range_overlap_is_inclusive_of_partial() {
        let mut st = Storage::open_memory().unwrap();
        st.insert_sessions(&[sess(1000, 2000, "A")]).unwrap();
        assert_eq!(st.sessions_in_range(1500, 3000).unwrap().len(), 1); // 部分重叠
        assert_eq!(st.sessions_in_range(3000, 4000).unwrap().len(), 0); // 不重叠
    }
}
```

- [ ] **步骤 2：在 `src/lib.rs` 加 `pub mod storage;`**

- [ ] **步骤 3：运行测试验证通过**

运行：`cargo test storage`
预期：2 个测试 PASS（bundled SQLite 首次编译较慢属正常）。

- [ ] **步骤 4：Commit**

```bash
git add src/storage.rs src/lib.rs
git commit -m "feat(storage): SQLite schema/WAL + 批量写入 + 范围查询"
```

### 任务 6：心跳 + 崩溃恢复（storage.rs 续）

**文件：**
- 修改：`src/storage.rs`

> **原理**：运行时（M2+）会周期性把"当前打开的会话"作为临时行写入并更新，同时写心跳时间戳 H。若进程崩溃，最后那行的 `end_ts` 可能被写到崩溃后（幻影尾巴）。启动时用 H 把超出的 `end_ts` 夹回，避免虚增时长（DayLens 思路）。

- [ ] **步骤 1：在 `src/storage.rs` 顶部加 `use rusqlite::OptionalExtension;`，并在 `impl Storage` 中追加：**

```rust
    pub fn set_meta(&self, key: &str, value: &str) -> Result<(), String> {
        self.conn.execute(
            "INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value]).map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn get_meta(&self, key: &str) -> Result<Option<String>, String> {
        self.conn.query_row("SELECT value FROM meta WHERE key=?", params![key], |r| r.get(0))
            .optional().map_err(|e| e.to_string())
    }
    pub fn set_heartbeat(&self, ts: i64) -> Result<(), String> { self.set_meta("heartbeat", &ts.to_string()) }
    pub fn get_heartbeat(&self) -> Result<Option<i64>, String> {
        Ok(self.get_meta("heartbeat")?.and_then(|v| v.parse::<i64>().ok()))
    }
    /// 把 end_ts 超过心跳 + grace 的会话夹回心跳时刻，返回修正行数。
    pub fn recover_phantom(&self, grace_sec: i64) -> Result<usize, String> {
        let h = match self.get_heartbeat()? { Some(h) => h, None => return Ok(0) };
        let n = self.conn.execute(
            "UPDATE activity SET end_ts = ?1, duration_sec = MAX(0, ?1 - start_ts) WHERE end_ts > ?1 + ?2",
            params![h, grace_sec]).map_err(|e| e.to_string())?;
        Ok(n)
    }
```

- [ ] **步骤 2：在 `src/storage.rs` 的 `mod tests` 追加测试**

```rust
    #[test]
    fn meta_roundtrip() {
        let st = Storage::open_memory().unwrap();
        assert_eq!(st.get_meta("x").unwrap(), None);
        st.set_meta("x", "42").unwrap();
        st.set_meta("x", "43").unwrap(); // upsert
        assert_eq!(st.get_meta("x").unwrap().as_deref(), Some("43"));
    }

    #[test]
    fn recover_clamps_phantom_tail() {
        let mut st = Storage::open_memory().unwrap();
        st.insert_sessions(&[sess(1000, 5000, "Crashed"), sess(1000, 2000, "Fine")]).unwrap();
        st.set_heartbeat(3000).unwrap();
        let fixed = st.recover_phantom(60).unwrap();
        assert_eq!(fixed, 1); // 只夹 end_ts=5000 那条
        let all = st.sessions_in_range(0, 10000).unwrap();
        let crashed = all.iter().find(|s| s.app_name == "Crashed").unwrap();
        assert_eq!(crashed.end_ts, 3000);
        assert_eq!(crashed.duration_sec, 2000);
    }
```

- [ ] **步骤 3：运行测试验证通过**

运行：`cargo test storage`
预期：全部 PASS（新增 `meta_roundtrip`、`recover_clamps_phantom_tail`）。

- [ ] **步骤 4：Commit**

```bash
git add src/storage.rs
git commit -m "feat(storage): meta/心跳 + 崩溃恢复夹回幻影尾巴"
```

### 任务 7：聚合（aggregate.rs）

**文件：**
- 创建：`src/aggregate.rs`
- 修改：`src/lib.rs`（加 `pub mod aggregate;`）

> 聚合函数接收 `&[Session]`（由 `storage.sessions_in_range` 取得），并接收 `UtcOffset`（由 bin 提供系统本地偏移），保持纯函数可测；不在库内读系统时区。

- [ ] **步骤 1：编写 `src/aggregate.rs`（实现）**

```rust
use crate::model::Session;
use crate::parse::extract_domain;
use std::collections::HashMap;
use time::{Date, Month, OffsetDateTime, Time, UtcOffset};

#[derive(Debug, Clone, PartialEq)]
pub struct Bucket { pub name: String, pub seconds: i64 }

fn rank(map: HashMap<String, i64>) -> Vec<Bucket> {
    let mut v: Vec<Bucket> = map.into_iter().map(|(name, seconds)| Bucket { name, seconds }).collect();
    v.sort_by(|a, b| b.seconds.cmp(&a.seconds).then(a.name.cmp(&b.name)));
    v
}

/// 各应用时长（排除空闲），降序。
pub fn app_durations(sessions: &[Session]) -> Vec<Bucket> {
    let mut m: HashMap<String, i64> = HashMap::new();
    for s in sessions.iter().filter(|s| !s.is_idle) {
        *m.entry(s.app_name.clone()).or_default() += s.duration_sec;
    }
    rank(m)
}

/// 各域名时长（来自 edge_url，排除空闲），降序。
pub fn domain_durations(sessions: &[Session]) -> Vec<Bucket> {
    let mut m: HashMap<String, i64> = HashMap::new();
    for s in sessions.iter().filter(|s| !s.is_idle) {
        if let Some(url) = &s.edge_url {
            if let Some(d) = extract_domain(url) {
                *m.entry(d).or_default() += s.duration_sec;
            }
        }
    }
    rank(m)
}

/// 各视频标题时长（排除空闲/隐私），降序。
pub fn top_videos(sessions: &[Session]) -> Vec<Bucket> {
    let mut m: HashMap<String, i64> = HashMap::new();
    for s in sessions.iter().filter(|s| !s.is_idle && !s.is_private) {
        if let Some(t) = &s.media_title {
            *m.entry(t.clone()).or_default() += s.duration_sec;
        }
    }
    rank(m)
}

pub fn total_active_sec(sessions: &[Session]) -> i64 {
    sessions.iter().filter(|s| !s.is_idle).map(|s| s.duration_sec).sum()
}
pub fn total_idle_sec(sessions: &[Session]) -> i64 {
    sessions.iter().filter(|s| s.is_idle).map(|s| s.duration_sec).sum()
}

/// 某本地日历日的 UTC 起止秒 [start, end)。
pub fn local_day_bounds(year: i32, month: u8, day: u8, offset: UtcOffset) -> (i64, i64) {
    let date = Date::from_calendar_date(year, Month::try_from(month).unwrap(), day).unwrap();
    let start = date.with_time(Time::MIDNIGHT).assume_offset(offset);
    let end = start + time::Duration::days(1);
    (start.unix_timestamp(), end.unix_timestamp())
}

/// 某 UTC 秒对应的本地小时 0-23。
pub fn local_hour(ts: i64, offset: UtcOffset) -> u8 {
    OffsetDateTime::from_unix_timestamp(ts).unwrap().to_offset(offset).hour()
}
```

继续 `src/aggregate.rs`，追加测试：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn s(app: &str, dur: i64, idle: bool, url: Option<&str>, media: Option<&str>) -> Session {
        Session {
            start_ts: 0, end_ts: dur, duration_sec: dur,
            app_name: app.into(), process_path: "p".into(), window_title: "w".into(),
            edge_url: url.map(|x| x.into()), page_title: None,
            media_title: media.map(|x| x.into()), media_player: None, media_status: None,
            media_position_sec: None, media_duration_sec: None,
            is_private: false, is_idle: idle,
        }
    }

    #[test]
    fn app_durations_sum_sorted_excl_idle() {
        let data = vec![s("Code", 100, false, None, None), s("Code", 50, false, None, None),
                        s("Edge", 200, false, None, None), s("X", 999, true, None, None)];
        let r = app_durations(&data);
        assert_eq!(r[0], Bucket { name: "Edge".into(), seconds: 200 });
        assert_eq!(r[1], Bucket { name: "Code".into(), seconds: 150 });
        assert_eq!(r.len(), 2); // 空闲的 X 被排除
    }

    #[test]
    fn domain_and_video_and_totals() {
        let data = vec![
            s("Edge", 60, false, Some("https://www.youtube.com/watch?v=1"), Some("片A")),
            s("Edge", 40, false, Some("https://github.com/x"), None),
            s("Idle", 30, true, None, None),
        ];
        assert_eq!(domain_durations(&data)[0], Bucket { name: "youtube.com".into(), seconds: 60 });
        assert_eq!(top_videos(&data)[0], Bucket { name: "片A".into(), seconds: 60 });
        assert_eq!(total_active_sec(&data), 100);
        assert_eq!(total_idle_sec(&data), 30);
    }

    #[test]
    fn day_bounds_and_hour_local() {
        let off8 = UtcOffset::from_hms(8, 0, 0).unwrap();
        let (start, end) = local_day_bounds(2026, 1, 15, off8);
        assert_eq!(end - start, 86400);
        assert_eq!(local_hour(start, off8), 0);      // 本地零点
        assert_eq!(local_hour(start - 1, off8), 23); // 前一秒是前一天 23 点
    }
}
```

- [ ] **步骤 2：在 `src/lib.rs` 加 `pub mod aggregate;`**

- [ ] **步骤 3：运行测试验证通过**

运行：`cargo test aggregate`
预期：3 个测试 PASS。

- [ ] **步骤 4：Commit**

```bash
git add src/aggregate.rs src/lib.rs
git commit -m "feat(aggregate): 应用/域名/视频时长 + 有效/空闲 + 本地时区日界与小时"
```

### 任务 8：日志 + main 装配（CLI）+ 端到端集成测试

**文件：**
- 创建：`src/logging.rs`
- 修改：`src/main.rs`（替换任务 1 的占位）
- 修改：`src/lib.rs`（加 `pub mod logging;`）
- 创建：`tests/storage_integration.rs`

- [ ] **步骤 1：编写 `src/logging.rs`**

```rust
use simplelog::{ConfigBuilder, LevelFilter, WriteLogger};
use std::fs::OpenOptions;
use std::path::Path;

/// 初始化写文件日志（append）。失败不 panic（无日志也要能跑）。
pub fn init(log_path: &Path) {
    if let Some(dir) = log_path.parent() { let _ = std::fs::create_dir_all(dir); }
    if let Ok(file) = OpenOptions::new().create(true).append(true).open(log_path) {
        let _ = WriteLogger::init(LevelFilter::Info, ConfigBuilder::new().build(), file);
    }
}
```

- [ ] **步骤 2：把 `pub mod logging;` 加入 `src/lib.rs`，并替换 `src/main.rs`**

```rust
use activity_tracker::{aggregate, config::Config, logging,
    model::ActivityEvent, session::SessionBuilder, storage::Storage};
use std::path::PathBuf;
use time::UtcOffset;

fn local_offset() -> UtcOffset { UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC) }

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("replay") => {
            let text = std::fs::read_to_string(&args[2]).expect("read events");
            let mut sb = SessionBuilder::new();
            let mut sessions = Vec::new();
            let mut last_ts = 0i64;
            for line in text.lines().filter(|l| !l.trim().is_empty()) {
                let ev: ActivityEvent = serde_json::from_str(line).expect("parse event");
                last_ts = ev.ts;
                if let Some(done) = sb.on_activity(&ev) { sessions.push(done); }
            }
            if let Some(done) = sb.finish(last_ts) { sessions.push(done); }
            let mut st = Storage::open(&args[3]).expect("open db");
            st.insert_sessions(&sessions).expect("insert");
            println!("replayed -> {} sessions", sessions.len());
        }
        Some("report") => {
            let p: Vec<i64> = args[3].split('-').map(|x| x.parse().unwrap()).collect();
            let (start, end) = aggregate::local_day_bounds(p[0] as i32, p[1] as u8, p[2] as u8, local_offset());
            let st = Storage::open(&args[2]).expect("open db");
            let sessions = st.sessions_in_range(start, end).expect("query");
            println!("有效 {}s / 空闲 {}s", aggregate::total_active_sec(&sessions), aggregate::total_idle_sec(&sessions));
            for b in aggregate::app_durations(&sessions) { println!("  {} - {}s", b.name, b.seconds); }
        }
        _ => {
            let cfg = Config::load(&PathBuf::from("config.toml")).unwrap_or_default();
            logging::init(&PathBuf::from("activity-tracker.log"));
            let _ = cfg;
            println!("activity-tracker core (M1). 用法: replay <events.jsonl> <db> | report <db> <YYYY-MM-DD>");
        }
    }
}
```

- [ ] **步骤 3：编写 `tests/storage_integration.rs`（端到端）**

```rust
use activity_tracker::{aggregate, model::ActivityEvent, session::SessionBuilder, storage::Storage};

fn ev(ts: i64, app: &str, win: &str) -> ActivityEvent {
    ActivityEvent { ts, app_name: app.into(), process_path: "p".into(), window_title: win.into(),
        edge_url: None, page_title: None, is_private: false, is_idle: false }
}

#[test]
fn end_to_end_merge_store_aggregate() {
    let path = std::env::temp_dir().join(format!("at_it_{}.db", std::process::id()));
    let db = path.to_str().unwrap();
    let _ = std::fs::remove_file(&path);

    let mut sb = SessionBuilder::new();
    let mut sessions = Vec::new();
    for e in [ev(1000, "Code", "main.rs"), ev(1100, "Code", "main.rs"), ev(1200, "Edge", "GitHub")] {
        if let Some(done) = sb.on_activity(&e) { sessions.push(done); }
    }
    if let Some(done) = sb.finish(1300) { sessions.push(done); }

    let mut st = Storage::open(db).unwrap();
    st.insert_sessions(&sessions).unwrap();
    let got = st.sessions_in_range(0, 10_000).unwrap();
    assert_eq!(got.len(), 2);

    let apps = aggregate::app_durations(&got);
    assert_eq!(apps[0].name, "Code");
    assert_eq!(apps[0].seconds, 200); // 1000..1200
    assert_eq!(apps[1].seconds, 100); // 1200..1300

    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{db}-wal"));
    let _ = std::fs::remove_file(format!("{db}-shm"));
}
```

- [ ] **步骤 4：运行完整测试与构建验证通过**

运行：`cargo test` 然后 `cargo build`
预期：所有单测 + `end_to_end_merge_store_aggregate` PASS；`cargo build` 无警告级错误。

- [ ] **步骤 5：手动冒烟（可选）**

```bash
printf '{"ts":1000,"app_name":"Code","process_path":"p","window_title":"main.rs","edge_url":null,"page_title":null,"is_private":false,"is_idle":false}\n{"ts":1200,"app_name":"Edge","process_path":"p","window_title":"GitHub","edge_url":null,"page_title":null,"is_private":false,"is_idle":false}\n' > /tmp/ev.jsonl
cargo run -- replay /tmp/ev.jsonl /tmp/at.db
cargo run -- report /tmp/at.db 1970-01-01   # 视本地时区，1000s 落在哪天则查哪天
```

- [ ] **步骤 6：Commit**

```bash
git add src/logging.rs src/main.rs src/lib.rs tests/storage_integration.rs
git commit -m "feat(cli): 日志 + replay/report 装配 + 端到端集成测试"
```

---

## 自检结论

- **规格覆盖度（M1 范围）**：配置(4.11 部分)→任务2；解析→任务3；会话合并(4.5)→任务4；存储/WAL/批量(4.6)→任务5；心跳/崩溃恢复(4.6)→任务6；聚合/本地时区(4.7)→任务7；数据模型(5)→任务1+5。采集器(4.1-4.4)、AI(4.8)、托盘/UI(4.9-4.10)、DPAPI/系统集成(4.12) 明确留给 M2–M5。
- **占位符扫描**：无 TODO/待定；每个代码步骤含完整可编译代码与测试。
- **类型一致性**：`Session`/`ActivityEvent`/`MediaEvent`/`PlaybackStatus`(任务1)、`Bucket`(任务7)、`Storage`(任务5-6) 跨任务签名一致；`close()`、`app_durations()`、`sessions_in_range()`、`local_day_bounds()` 名称统一。
- **已知 M1 简化**：合并键不含 media_title（见任务4 说明）；`recover_phantom` 依赖运行时写心跳与临时行（M2 接入）。

## 计划结束















