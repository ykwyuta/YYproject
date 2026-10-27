use chrono::{DateTime, NaiveDate, Utc};
use yy_project::{work::*, *};
fn day(s: &str) -> NaiveDate {
    s.parse().unwrap()
}
fn at(s: &str) -> DateTime<Utc> {
    s.parse().unwrap()
}
fn apply(s: &mut Store, p: &Project, c: Change) -> Project {
    s.apply(p.id, p.revision, &c, "management-test").unwrap()
}
#[test]
fn exchange_identity_history_atomicity_and_sqlite_transfer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut s = Store::open(&path, false).unwrap();
    let mut p = s
        .create("管理".into(), day("2026-01-01"), day("2026-12-31"), "test")
        .unwrap();
    p = apply(
        &mut s,
        &p,
        Change::SaveTask {
            task: Task {
                title: "問い合わせ".into(),
                kind: Kind::Inquiry,
                inquiry: Some(Box::new(Inquiry {
                    target_team: "基盤".into(),
                    requested_at: Some(at("2026-01-01T00:00:00Z")),
                    ..Default::default()
                })),
                ..Default::default()
            },
        },
    );
    assert!(!p.tasks[0].include_in_effort);
    assert!(
        cases::statistics(
            &p,
            Kind::Inquiry,
            day("2026-01-01"),
            day("2026-01-02"),
            &TaskFilter::default()
        )
        .is_err()
    );
    assert!(
        table::export_jira_csv(
            &p,
            &TaskFilter::default(),
            &[],
            &table::JiraOptions::default()
        )
        .is_err()
    );
    let id = p.tasks[0].id;
    let entry = Exchange {
        at: Some(at("2026-01-02T00:00:00Z")),
        text: "回答, \"引用\"\n第二行".into(),
        ..Default::default()
    };
    p = apply(
        &mut s,
        &p,
        Change::SaveExchange {
            id,
            entry: entry.clone(),
        },
    );
    let mut replacement = p.tasks[0].clone();
    replacement.inquiry.as_mut().unwrap().exchanges.clear();
    p = apply(&mut s, &p, Change::SaveTask { task: replacement });
    assert_eq!(p.tasks[0].inquiry.as_ref().unwrap().exchanges.len(), 1);
    let before = serde_json::to_value(&p).unwrap();
    for bad in [
        Exchange {
            id: 42,
            ..entry.clone()
        },
        Exchange {
            text: String::new(),
            ..entry.clone()
        },
        Exchange {
            at: Some(at("2025-12-31T00:00:00Z")),
            ..entry.clone()
        },
    ] {
        assert!(
            s.apply(
                p.id,
                p.revision,
                &Change::SaveExchange { id, entry: bad },
                "bad"
            )
            .is_err()
        );
        assert_eq!(serde_json::to_value(s.get(p.id).unwrap()).unwrap(), before);
    }
    assert!(
        s.apply(
            p.id,
            p.revision - 1,
            &Change::SaveExchange {
                id,
                entry: entry.clone()
            },
            "stale"
        )
        .is_err()
    );
    p = apply(&mut s, &p, Change::RemoveExchange { id, entry_id: 1 });
    p = apply(&mut s, &p, Change::SaveExchange { id, entry });
    assert_eq!(p.tasks[0].inquiry.as_ref().unwrap().exchanges[0].id, 2);
    let filter = TaskFilter {
        kind: Some(Kind::Inquiry),
        query: "第二行".into(),
        ..Default::default()
    };
    let csv = table::export_csv(
        &p,
        &filter,
        &["title".into(), "inquiry.exchange_log".into()],
    )
    .unwrap();
    let rows = table::decode(&csv, ',').unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows[1][1].contains("回答, \"引用\"\n第二行"));
    let out = dir.path().join("export");
    s.export_sqlite(&out).unwrap();
    assert_eq!(
        serde_json::to_value(Store::open(&out, true).unwrap().get(p.id).unwrap()).unwrap(),
        serde_json::to_value(&p).unwrap()
    );
    assert!(
        Store::open(&path, true)
            .unwrap()
            .apply(
                p.id,
                p.revision,
                &Change::RemoveExchange { id, entry_id: 2 },
                "readonly"
            )
            .is_err()
    );
}
#[test]
fn management_daily_cutoffs_and_workaround_cancellation() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(&dir.path().join("db"), false).unwrap();
    let mut p = s
        .create("統計".into(), day("2026-01-01"), day("2026-12-31"), "test")
        .unwrap();
    for status in [Status::Done, Status::Todo] {
        p = apply(
            &mut s,
            &p,
            Change::SaveTask {
                task: Task {
                    kind: Kind::Inquiry,
                    title: format!("{status:?}"),
                    status,
                    inquiry: Some(Box::new(Inquiry {
                        target_team: "基盤".into(),
                        requested_at: Some(at("2026-01-01T00:00:00Z")),
                        response_due_at: Some(at("2026-01-02T00:00:00Z")),
                        answered_at: (status == Status::Done).then(|| at("2026-01-02T00:00:00Z")),
                        closed_at: (status == Status::Done).then(|| at("2026-01-03T00:00:00Z")),
                        ..Default::default()
                    })),
                    ..Default::default()
                },
            },
        );
    }
    let stats = statistics(
        &p,
        Kind::Inquiry,
        day("2026-01-01"),
        day("2026-01-03"),
        &TaskFilter::default(),
    )
    .unwrap();
    assert_eq!(
        stats
            .convergence
            .daily
            .iter()
            .map(|d| d.open)
            .collect::<Vec<_>>(),
        vec![2, 2, 1]
    );
    assert_eq!(
        (stats.overdue, stats.unanswered, stats.mean_answer_hours),
        (1, 1, Some(24.))
    );
    let early = statistics(
        &p,
        Kind::Inquiry,
        day("2026-01-01"),
        day("2026-01-01"),
        &TaskFilter::default(),
    )
    .unwrap();
    assert_eq!(early.mean_answer_hours, None);
    assert_eq!(early.unanswered, 2);
    p = apply(
        &mut s,
        &p,
        Change::SaveTask {
            task: Task {
                kind: Kind::Workaround,
                title: "暫定対応".into(),
                status: Status::Cancelled,
                workaround: Some(Box::new(Workaround {
                    reason: "障害".into(),
                    detected_at: Some(at("2026-01-01T00:00:00Z")),
                    cancelled_at: Some(at("2026-01-03T00:00:00Z")),
                    ..Default::default()
                })),
                ..Default::default()
            },
        },
    );
    let t = p.tasks.last().unwrap();
    assert!(t.workaround.as_ref().unwrap().permanent_at.is_none());
    let mut bad = t.clone();
    bad.workaround.as_mut().unwrap().applied_at = Some(at("2026-01-02T00:00:00Z"));
    assert!(
        s.apply(p.id, p.revision, &Change::SaveTask { task: bad }, "invalid")
            .is_err()
    );
    let mut reopened = t.clone();
    reopened.status = Status::Doing;
    reopened.progress = 100;
    p = apply(&mut s, &p, Change::SaveTask { task: reopened });
    let t = p.tasks.last().unwrap();
    assert!(t.progress < 100);
    assert!(t.workaround.as_ref().unwrap().cancelled_at.is_none());
    assert_eq!(t.workaround.as_ref().unwrap().transitions.len(), 3);
}

#[test]
fn version_five_upgrades_only_after_atomic_management_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    let mut s = Store::open(&path, false).unwrap();
    let p = s
        .create("旧DB".into(), day("2026-01-01"), day("2026-12-31"), "test")
        .unwrap();
    let sql = rusqlite::Connection::open(&path).unwrap();
    sql.pragma_update(None, "user_version", 5).unwrap();
    let version = || {
        sql.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap()
    };
    let bad = Task {
        kind: Kind::Inquiry,
        title: "失敗する依頼".into(),
        ..Default::default()
    };
    assert!(
        s.apply(p.id, p.revision, &Change::SaveTask { task: bad }, "bad")
            .is_err()
    );
    assert_eq!(version(), 5);
    assert_eq!(
        Store::open(&path, true)
            .unwrap()
            .get(p.id)
            .unwrap()
            .revision,
        p.revision
    );
    assert_eq!(version(), 5);
    let mut p = apply(
        &mut s,
        &p,
        Change::SaveTask {
            task: Task {
                kind: Kind::Inquiry,
                title: "移行する依頼".into(),
                inquiry: Some(Box::new(Inquiry {
                    target_team: "基盤".into(),
                    ..Default::default()
                })),
                ..Default::default()
            },
        },
    );
    assert_eq!(version(), 6);
    let mut second = p.tasks[0].clone();
    second.id = 0;
    second.title = "二件目".into();
    p = apply(&mut s, &p, Change::SaveTask { task: second });
    let before = serde_json::to_value(&p).unwrap();
    let audit = s.audit(p.id, 0).unwrap().len();
    let mut tasks = p.tasks.clone();
    tasks[0].inquiry.as_mut().unwrap().target_team = "変更".into();
    tasks[1].inquiry.as_mut().unwrap().target_team.clear();
    assert!(
        s.apply(p.id, p.revision, &Change::SaveTasks { tasks }, "bad-batch")
            .is_err()
    );
    assert_eq!(serde_json::to_value(s.get(p.id).unwrap()).unwrap(), before);
    assert_eq!(s.audit(p.id, 0).unwrap().len(), audit);
}
