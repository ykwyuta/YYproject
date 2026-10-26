use chrono::{NaiveDate, Utc};
use yy_project::{cases::*, *};

fn day(s: &str) -> NaiveDate {
    s.parse().unwrap()
}
fn at(s: &str) -> chrono::DateTime<Utc> {
    s.parse().unwrap()
}
fn save(s: &mut Store, p: &Project, t: Task) -> Project {
    s.apply(p.id, p.revision, &Change::SaveTask { task: t }, "case-test")
        .unwrap()
}
fn item(kind: Kind, title: &str, detected: &str, status: Status, closed: Option<&str>) -> Task {
    Task {
        kind,
        title: title.into(),
        status,
        include_in_effort: false,
        case: Some(Box::new(CaseData {
            detected_at: Some(at(detected)),
            closed_at: closed.map(at),
            cause: "仕様漏れ".into(),
            occurrence_phase: "基本設計".into(),
            detection_phase: "結合試験".into(),
            permanent_difficulty: Difficulty::High,
            ..Default::default()
        })),
        ..Default::default()
    }
}
#[test]
fn convergence_separates_bugs_issues_and_cancellation_and_retains_reopening_history() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(&dir.path().join("db"), false).unwrap();
    let mut p = s
        .create("cases".into(), day("2026-01-01"), day("2026-12-31"), "test")
        .unwrap();
    p = save(
        &mut s,
        &p,
        item(
            Kind::Bug,
            "解決済み",
            "2026-01-01T09:00:00Z",
            Status::Done,
            Some("2026-01-02T09:00:00Z"),
        ),
    );
    p = save(
        &mut s,
        &p,
        item(
            Kind::Bug,
            "中止",
            "2026-01-02T09:00:00Z",
            Status::Cancelled,
            Some("2026-01-03T09:00:00Z"),
        ),
    );
    p = save(
        &mut s,
        &p,
        item(
            Kind::Bug,
            "継続",
            "2026-01-03T09:00:00Z",
            Status::Todo,
            None,
        ),
    );
    p = save(
        &mut s,
        &p,
        item(
            Kind::Issue,
            "別管理の課題",
            "2026-01-01T09:00:00Z",
            Status::Todo,
            None,
        ),
    );
    let stats = statistics(
        &p,
        Kind::Bug,
        day("2026-01-01"),
        day("2026-01-04"),
        &TaskFilter::default(),
    )
    .unwrap();
    assert_eq!(
        stats
            .daily
            .iter()
            .map(|d| (d.detected, d.open, d.resolved, d.cancelled))
            .collect::<Vec<_>>(),
        vec![(1, 1, 0, 0), (2, 1, 1, 0), (3, 1, 1, 1), (3, 1, 1, 1)]
    );
    assert_eq!(
        (stats.total, stats.open, stats.resolved, stats.cancelled),
        (3, 1, 1, 1)
    );
    assert_eq!(stats.causes["仕様漏れ"], 3);
    assert_eq!(stats.mean_resolution_hours, Some(24.));
    let old_history = serde_json::to_value(&p.tasks[0].case.as_ref().unwrap().transitions).unwrap();
    let mut reopened = p.tasks[0].clone();
    reopened.status = Status::Doing;
    reopened.progress = 20;
    reopened.case.as_mut().unwrap().transitions.clear(); // Client cannot erase recorded history.
    p = save(&mut s, &p, reopened);
    let history = &p.tasks[0].case.as_ref().unwrap().transitions;
    assert_eq!(serde_json::to_value(&history[..2]).unwrap(), old_history);
    assert_eq!(history.last().unwrap().status, Status::Doing);
    assert!(p.tasks[0].case.as_ref().unwrap().closed_at.is_none());
    let stats = statistics(
        &p,
        Kind::Bug,
        day("2026-01-01"),
        Utc::now().date_naive(),
        &TaskFilter::default(),
    )
    .unwrap();
    assert_eq!(stats.daily[1].resolved, 1);
    assert_eq!(
        (
            stats.daily.last().unwrap().open,
            stats.daily.last().unwrap().resolved
        ),
        (2, 0)
    );
    assert_eq!(
        statistics(
            &p,
            Kind::Issue,
            day("2026-01-01"),
            Utc::now().date_naive(),
            &TaskFilter::default()
        )
        .unwrap()
        .total,
        1
    );
    let clipped = statistics(
        &p,
        Kind::Bug,
        day("2026-01-04"),
        day("2026-01-05"),
        &TaskFilter::default(),
    )
    .unwrap();
    assert_eq!(clipped.daily[0].detected, 3); // Carry events before the visible range.
    assert!(
        statistics(
            &p,
            Kind::Task,
            day("2026-01-01"),
            day("2026-01-02"),
            &TaskFilter::default()
        )
        .is_err()
    );
}
#[test]
fn case_edits_validate_timestamps_atomically_preserve_conflicts_and_export_full_details() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("db");
    let mut s = Store::open(&file, false).unwrap();
    let mut p = s
        .create("分析".into(), day("2026-01-01"), day("2026-12-31"), "test")
        .unwrap();
    let mut bug = item(
        Kind::Bug,
        "Unicode バグ",
        "2026-01-01T09:00:00Z",
        Status::Todo,
        None,
    );
    table::set_value(
        &p,
        &mut bug,
        "case.temporary_plan",
        "回避策,\"引用\"\n2行目",
    )
    .unwrap();
    table::set_value(&p, &mut bug, "case.permanent_plan", "恒久対策案件ABC").unwrap();
    table::set_value(
        &p,
        &mut bug,
        "case.reported_at",
        "2026-01-01T18:05:00+09:00",
    )
    .unwrap();
    p = save(&mut s, &p, bug);
    assert_eq!(p.tasks[0].case.as_ref().unwrap().transitions.len(), 1);
    let audit = s.audit(p.id, 0).unwrap();
    let mut bad = p.tasks[0].clone();
    bad.case.as_mut().unwrap().occurred_at = Some(at("2026-01-02T00:00:00Z"));
    assert!(
        s.apply(p.id, p.revision, &Change::SaveTask { task: bad }, "test")
            .is_err()
    );
    assert_eq!(s.get(p.id).unwrap().revision, p.revision);
    assert_eq!(s.audit(p.id, 0).unwrap().len(), audit.len());
    let mut filter = TaskFilter {
        query: "恒久対策案件ABC".into(),
        kind: Some(Kind::Bug),
        ..Default::default()
    };
    assert!(p.matches_filter(&filter, &p.tasks[0]));
    filter
        .columns
        .insert("case.cause".into(), "仕様漏れ".into());
    filter.validate().unwrap();
    let keys = vec![
        "title".into(),
        "case.temporary_plan".into(),
        "case.reported_at".into(),
        "case.permanent_difficulty".into(),
    ];
    let csv = table::export_csv(&p, &filter, &keys).unwrap();
    let rows = table::decode(&csv, ',').unwrap();
    assert_eq!(rows[1][1], "回避策,\"引用\"\n2行目");
    assert_eq!(rows[1][2], "2026-01-01T09:05:00Z");
    assert_eq!(rows[1][3], "高");
    let change = table::batch_task_attributes(
        &p,
        &[p.tasks[0].id],
        &[("case.cause".into(), "実装誤り".into())],
    )
    .unwrap();
    let stale = p.revision;
    p = s.apply(p.id, p.revision, &change, "test").unwrap();
    assert_eq!(p.tasks[0].case.as_ref().unwrap().transitions.len(), 1);
    assert!(matches!(
        s.apply(p.id, stale, &change, "test"),
        Err(Error::Conflict)
    ));
    let close_change = table::batch_task_attributes(
        &p,
        &[p.tasks[0].id],
        &[
            ("status".into(), "完了".into()),
            ("case.closed_at".into(), "2026-01-02T09:00:00Z".into()),
        ],
    )
    .unwrap();
    p = s.apply(p.id, p.revision, &close_change, "test").unwrap();
    assert_eq!(p.tasks[0].case.as_ref().unwrap().transitions.len(), 2);
    let out = dir.path().join("export.db");
    s.export_sqlite(&out).unwrap();
    let restored = Store::open(&out, true).unwrap().get(p.id).unwrap();
    assert_eq!(
        serde_json::to_value(&restored.tasks).unwrap(),
        serde_json::to_value(&p.tasks).unwrap()
    );
    assert!(table::set_value(&p, &mut Task::default(), "case.cause", "x").is_err());
    assert!(
        table::set_value(
            &p,
            &mut p.tasks[0].clone(),
            "case.detected_at",
            "2026-01-01 09:00"
        )
        .is_err()
    );
}
#[test]
fn undated_legacy_issues_are_not_given_invented_detection_dates_and_archived_cases_are_separate() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(&dir.path().join("db"), false).unwrap();
    let p = s
        .create(
            "legacy".into(),
            day("2026-01-01"),
            day("2026-12-31"),
            "test",
        )
        .unwrap();
    let p = save(
        &mut s,
        &p,
        item(
            Kind::Issue,
            "legacy",
            "2026-01-01T00:00:00Z",
            Status::Todo,
            None,
        ),
    );
    let mut p = p;
    p.tasks[0].case = None; // Same representation as older JSON documents.
    p.validate().unwrap();
    let stats = statistics(
        &p,
        Kind::Issue,
        day("2026-01-01"),
        day("2026-01-02"),
        &TaskFilter::default(),
    )
    .unwrap();
    assert_eq!((stats.undated, stats.total), (1, 0));
    assert!(stats.daily.iter().all(|d| d.open == 0));
    let p = s.get(p.id).unwrap();
    let p = s
        .apply(p.id, p.revision, &Change::RemoveTask { id: 1 }, "test")
        .unwrap();
    assert_eq!(
        statistics(
            &p,
            Kind::Issue,
            day("2026-01-01"),
            day("2026-01-02"),
            &TaskFilter::default()
        )
        .unwrap()
        .total,
        0
    );
    assert_eq!(
        statistics(
            &p,
            Kind::Issue,
            day("2026-01-01"),
            day("2026-01-02"),
            &TaskFilter {
                archived: true,
                ..Default::default()
            }
        )
        .unwrap()
        .total,
        1
    );
}
