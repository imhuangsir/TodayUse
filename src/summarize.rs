//! AI 总结：本地算好数值 → 脱敏 → 组 prompt → 调 OpenAI 兼容接口 → 存 summaries。
//! 数值全部本地确定性计算，模型只叙述、不编数字（Yolo 原则）。
use crate::aggregate::{self, Bucket};
use crate::config::Config;
use crate::model::Session;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Digest {
    pub date: String,
    pub active_sec: i64,
    pub idle_sec: i64,
    pub apps: Vec<Bucket>,
    pub domains: Vec<Bucket>,
    pub videos: Vec<Bucket>,
}

/// 由某日会话构造结构化摘要（数值本地算）。
pub fn build_digest(date: &str, sessions: &[Session]) -> Digest {
    Digest {
        date: date.to_string(),
        active_sec: aggregate::total_active_sec(sessions),
        idle_sec: aggregate::total_idle_sec(sessions),
        apps: aggregate::app_durations(sessions),
        domains: aggregate::domain_durations(sessions),
        videos: aggregate::top_videos(sessions),
    }
}

fn mask_domain(d: &str) -> String {
    match d.rsplit_once('.') {
        Some((_, tld)) => format!("***.{tld}"),
        None => "***".to_string(),
    }
}

/// 按配置脱敏 + 过滤排除域名，返回可安全外发的摘要。
pub fn desensitize(mut d: Digest, cfg: &Config) -> Digest {
    let excl: Vec<String> = cfg
        .excluded_domains
        .iter()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    d.domains
        .retain(|b| !excl.iter().any(|e| b.name.to_ascii_lowercase().contains(e.as_str())));
    if cfg.desensitize.domains {
        for b in &mut d.domains {
            b.name = mask_domain(&b.name);
        }
    }
    if cfg.desensitize.titles {
        for b in &mut d.videos {
            b.name = "***".to_string();
        }
    }
    d
}

/// 组 (system, user) 提示词。user 内嵌摘要 JSON。
pub fn build_prompt(d: &Digest) -> (String, String) {
    let system = "你是活动日志助手。只依据给定的结构化数据，用简洁自然的中文写一段当天活动总结：\
        时间分布、主要应用/网站/视频、可能的意图。不要编造数据里没有的数字或事实。"
        .to_string();
    let json = serde_json::to_string_pretty(d).unwrap_or_default();
    let user = format!(
        "以下是今天（{}）的活动聚合数据（时长单位为秒），请据此总结：\n{}",
        d.date, json
    );
    (system, user)
}

/// 从 OpenAI 兼容响应 JSON 提取 choices[0].message.content。
pub fn parse_content(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    v.get("choices")?
        .get(0)?
        .get("message")?
        .get("content")?
        .as_str()
        .map(|s| s.to_string())
}

/// 调用 OpenAI 兼容 /chat/completions。base_url 形如 https://host/v1。
pub fn call_openai(
    base_url: &str,
    api_key: &str,
    model: &str,
    system: &str,
    user: &str,
) -> Result<String, String> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user}
        ],
        "temperature": 0.5
    });
    let resp = ureq::post(&url)
        .set("Authorization", &format!("Bearer {api_key}"))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| e.to_string())?;
    let text = resp.into_string().map_err(|e| e.to_string())?;
    parse_content(&text).ok_or_else(|| format!("无法解析响应: {text}"))
}

/// 生成某日总结并存入 summaries；返回总结文本。
pub fn generate_for_day(
    storage: &mut crate::storage::Storage,
    cfg: &Config,
    api_key: &str,
    year: i32,
    month: u8,
    day: u8,
    offset: time::UtcOffset,
) -> Result<String, String> {
    let (start, end) = aggregate::local_day_bounds(year, month, day, offset);
    let sessions = storage.sessions_in_range(start, end)?;
    let date = format!("{year:04}-{month:02}-{day:02}");
    let digest = desensitize(build_digest(&date, &sessions), cfg);
    let (system, user) = build_prompt(&digest);
    let content = call_openai(&cfg.ai_base_url, api_key, &cfg.ai_model, &system, &user)?;
    storage.insert_summary(start, end, "day", &content, &cfg.ai_model)?;
    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Desensitize;

    fn s(app: &str, dur: i64, idle: bool, url: Option<&str>, media: Option<&str>) -> Session {
        Session {
            start_ts: 0,
            end_ts: dur,
            duration_sec: dur,
            app_name: app.into(),
            process_path: "p".into(),
            window_title: "w".into(),
            edge_url: url.map(|x| x.into()),
            page_title: None,
            media_title: media.map(|x| x.into()),
            media_player: None,
            media_status: None,
            media_position_sec: None,
            media_duration_sec: None,
            is_private: false,
            is_idle: idle,
        }
    }

    #[test]
    fn digest_computes_from_sessions() {
        let data = vec![
            s("Code", 100, false, None, None),
            s("Edge", 60, false, Some("https://youtube.com/watch?v=1"), Some("片A")),
            s("Idle", 30, true, None, None),
        ];
        let d = build_digest("2026-09-27", &data);
        assert_eq!(d.active_sec, 160);
        assert_eq!(d.idle_sec, 30);
        assert_eq!(d.apps[0].name, "Code");
        assert_eq!(d.domains[0].name, "youtube.com");
        assert_eq!(d.videos[0].name, "片A");
    }

    #[test]
    fn desensitize_masks_and_filters() {
        let data = vec![
            s("Edge", 60, false, Some("https://youtube.com/x"), Some("秘密视频")),
            s("Edge", 40, false, Some("https://bank.com/x"), None),
        ];
        let cfg = Config {
            excluded_domains: vec!["bank.com".into()],
            desensitize: Desensitize { domains: true, titles: true },
            ..Config::default()
        };
        let d = desensitize(build_digest("2026-09-27", &data), &cfg);
        assert!(d.domains.iter().all(|b| !b.name.contains("bank")));
        assert_eq!(d.domains[0].name, "***.com");
        assert_eq!(d.videos[0].name, "***");
    }

    #[test]
    fn parse_openai_content() {
        let json = r#"{"choices":[{"message":{"role":"assistant","content":"今天在写代码。"}}]}"#;
        assert_eq!(parse_content(json).as_deref(), Some("今天在写代码。"));
        assert_eq!(parse_content("not json"), None);
    }

    #[test]
    fn prompt_includes_date_and_data() {
        let d = build_digest("2026-09-27", &[s("Code", 10, false, None, None)]);
        let (sys, user) = build_prompt(&d);
        assert!(sys.contains("不要编造"));
        assert!(user.contains("2026-09-27"));
        assert!(user.contains("Code"));
    }
}
