use activity_tracker::{aggregate, model::ActivityEvent, session::SessionBuilder, storage::Storage};

fn ev(ts: i64, app: &str, win: &str) -> ActivityEvent {
    ActivityEvent {
        ts,
        app_name: app.into(),
        process_path: "p".into(),
        window_title: win.into(),
        edge_url: None,
        page_title: None,
        is_private: false,
        is_idle: false,
    }
}

#[test]
fn end_to_end_merge_store_aggregate() {
    let path = std::env::temp_dir().join(format!("at_it_{}.db", std::process::id()));
    let db = path.to_str().unwrap();
    let _ = std::fs::remove_file(&path);

    let mut sb = SessionBuilder::new();
    let mut sessions = Vec::new();
    for e in [
        ev(1000, "Code", "main.rs"),
        ev(1100, "Code", "main.rs"),
        ev(1200, "Edge", "GitHub"),
    ] {
        if let Some(done) = sb.on_activity(&e) {
            sessions.push(done);
        }
    }
    if let Some(done) = sb.finish(1300) {
        sessions.push(done);
    }

    let mut st = Storage::open(db).unwrap();
    st.insert_sessions(&sessions).unwrap();
    let got = st.sessions_in_range(0, 10_000).unwrap();
    assert_eq!(got.len(), 2);

    let apps = aggregate::app_durations(&got);
    assert_eq!(apps[0].name, "Code");
    assert_eq!(apps[0].seconds, 200);
    assert_eq!(apps[1].seconds, 100);

    drop(st);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{db}-wal"));
    let _ = std::fs::remove_file(format!("{db}-shm"));
}
