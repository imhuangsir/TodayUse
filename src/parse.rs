/// 从 URL 提取主机域名（去 scheme/userinfo/port/path/www.）。
pub fn extract_domain(url: &str) -> Option<String> {
    let after_scheme = url.split("://").nth(1).unwrap_or(url);
    let host = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("");
    let host = host.rsplit('@').next().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host);
    let host = host.strip_prefix("www.").unwrap_or(host);
    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

fn strip_leading_count(t: &str) -> &str {
    // 去掉未读计数前缀，如 "(3) "
    if let Some(rest) = t.strip_prefix('(') {
        if let Some(idx) = rest.find(") ") {
            if !rest[..idx].is_empty() && rest[..idx].chars().all(|c| c.is_ascii_digit()) {
                return &rest[idx + 2..];
            }
        }
    }
    t
}

/// 从 Edge 窗口标题解析页面标题。真实格式为 "页面 - [配置名 - ]Microsoft Edge"，
/// 且 "Microsoft Edge" 中可能含特殊空格，故按 " - " 分段、去掉末尾含 "Edge" 的段。
pub fn parse_edge_title(window_title: &str) -> Option<String> {
    let t = strip_leading_count(window_title.trim());
    let parts: Vec<&str> = t.split(" - ").collect();
    if parts.len() < 2 || !parts.last().unwrap().contains("Edge") {
        return None;
    }
    let page = parts[..parts.len() - 1].join(" - ");
    let page = page.trim();
    if page.is_empty() {
        None
    } else {
        Some(page.to_string())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VideoInfo {
    pub site: String,
    pub title: String,
}

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
            if let Some(s) = t.strip_suffix(suf) {
                t = s.trim_end_matches('_');
                break;
            }
        }
        return Some(VideoInfo { site: "Bilibili".into(), title: t.trim().to_string() });
    }
    None
}

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
        assert_eq!(parse_edge_title("记事本"), None);
        // 真实格式：含配置名段 + 特殊空格（此处用普通空格模拟分段结构）
        assert_eq!(
            parse_edge_title("标题_哔哩哔哩_bilibili - 个人 - Microsoft Edge").as_deref(),
            Some("标题_哔哩哔哩_bilibili - 个人")
        );
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
