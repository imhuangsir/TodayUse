use crate::model::Session;
use crate::parse::extract_domain;
use std::collections::HashMap;
use time::{Date, Month, OffsetDateTime, Time, UtcOffset};

#[derive(Debug, Clone, PartialEq)]
pub struct Bucket {
    pub name: String,
    pub seconds: i64,
}

fn rank(map: HashMap<String, i64>) -> Vec<Bucket> {
    let mut v: Vec<Bucket> = map
        .into_iter()
        .map(|(name, seconds)| Bucket { name, seconds })
        .collect();
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn app_durations_sum_sorted_excl_idle() {
        let data = vec![
            s("Code", 100, false, None, None),
            s("Code", 50, false, None, None),
            s("Edge", 200, false, None, None),
            s("X", 999, true, None, None),
        ];
        let r = app_durations(&data);
        assert_eq!(r[0], Bucket { name: "Edge".into(), seconds: 200 });
        assert_eq!(r[1], Bucket { name: "Code".into(), seconds: 150 });
        assert_eq!(r.len(), 2);
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
        assert_eq!(local_hour(start, off8), 0);
        assert_eq!(local_hour(start - 1, off8), 23);
    }
}
