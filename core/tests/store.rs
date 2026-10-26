use chrono::{Days, NaiveDate};
use yy_project::*;
fn date(s: &str) -> NaiveDate {
    s.parse().unwrap()
}
fn create(store: &mut Store) -> Project {
    store
        .create(
            "日本語のプロジェクト".into(),
            date("2026-10-23"),
            date("2026-11-30"),
            "test",
        )
        .unwrap()
}
fn task(title: &str) -> Change {
    Change::SaveTask {
        task: Task {
            title: title.into(),
            estimate: 8.0,
            duration_days: 1,
            ..Default::default()
        },
    }
}
#[test]
fn durable_changes_conflicts_and_ids_do_not_reuse_deleted_objects() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("project.sqlite3");
    let mut a = Store::open(&file, false).unwrap();
    let mut p = create(&mut a);
    let mut b = Store::open(&file, false).unwrap();
    let version = b.data_version().unwrap();
    p = a.apply(p.id, p.revision, &task("task1"), "GUI").unwrap();
    assert!(b.data_version().unwrap() != version);
    assert!(matches!(
        b.apply(p.id, 1, &task("stale"), "AI"),
        Err(Error::Conflict)
    ));
    p = a
        .apply(p.id, p.revision, &Change::RemoveTask { id: 1 }, "GUI")
        .unwrap();
    p = a.apply(p.id, p.revision, &task("task2"), "AI").unwrap();
    assert!(p.tasks[0].archived);
    assert_eq!(p.tasks[1].id, 2);
    drop(a);
    drop(b);
    let saved = Store::open(&file, true).unwrap();
    assert_eq!(saved.get(p.id).unwrap().tasks[1].title, "task2");
    assert_eq!(saved.audit(p.id, 0).unwrap().len(), 4);
    assert_eq!(saved.audit(p.id, 2).unwrap().len(), 2);
}
#[test]
fn cycle_missing_ids_and_master_removal_are_atomic() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(&dir.path().join("test"), false).unwrap();
    let mut p = create(&mut s);
    for title in ["a", "b", "c"] {
        p = s.apply(p.id, p.revision, &task(title), "test").unwrap();
    }
    p = s
        .apply(
            p.id,
            p.revision,
            &Change::SaveMaster {
                master: Master {
                    parent: None,
                    id: 0,
                    kind: MasterKind::Assignee,
                    name: "担当者".into(),
                    date: None,
                },
            },
            "test",
        )
        .unwrap();
    let mut t = p.tasks[0].clone();
    t.assignee = Some(1);
    p = s
        .apply(p.id, p.revision, &Change::SaveTask { task: t }, "test")
        .unwrap();
    assert!(
        s.apply(p.id, p.revision, &Change::RemoveMaster { id: 1 }, "test")
            .is_err()
    );
    for (a, b) in [(1, 2), (2, 3)] {
        p = s
            .apply(
                p.id,
                p.revision,
                &Change::AddDependency {
                    dependency: Dependency {
                        predecessor: a,
                        successor: b,
                        lag_days: 0,
                    },
                },
                "test",
            )
            .unwrap();
    }
    for (a, b) in [(3, 1), (1, 99), (1, 2)] {
        assert!(
            s.apply(
                p.id,
                p.revision,
                &Change::AddDependency {
                    dependency: Dependency {
                        predecessor: a,
                        successor: b,
                        lag_days: 0
                    }
                },
                "test"
            )
            .is_err()
        );
        assert_eq!(s.get(p.id).unwrap().revision, p.revision);
    }
    assert_eq!(p.topological().unwrap(), [1, 2, 3]);
    assert_eq!(p.network().unwrap()[&3].0, 2);
}
#[test]
fn schedule_excludes_weekends_holidays_and_applies_finish_start_lag() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(&dir.path().join("test"), false).unwrap();
    let mut p = create(&mut s);
    for title in ["Friday", "Next"] {
        p = s.apply(p.id, p.revision, &task(title), "test").unwrap();
    }
    p = s
        .apply(
            p.id,
            p.revision,
            &Change::SaveMaster {
                master: Master {
                    parent: None,
                    id: 0,
                    kind: MasterKind::Holiday,
                    name: "休日".into(),
                    date: Some(date("2026-10-26")),
                },
            },
            "test",
        )
        .unwrap();
    p = s
        .apply(
            p.id,
            p.revision,
            &Change::AddDependency {
                dependency: Dependency {
                    predecessor: 1,
                    successor: 2,
                    lag_days: 1,
                },
            },
            "test",
        )
        .unwrap();
    p = s
        .apply(
            p.id,
            p.revision,
            &Change::Schedule {
                allow_overrun: false,
            },
            "test",
        )
        .unwrap();
    assert_eq!(p.tasks[0].start, Some(date("2026-10-23")));
    assert_eq!(p.tasks[1].start, Some(date("2026-10-28")));
    assert_eq!(p.tasks[1].due, p.tasks[1].start);
}
#[test]
fn progress_history_keeps_prior_scope_and_real_recorded_values() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(&dir.path().join("test"), false).unwrap();
    let mut p = create(&mut s);
    p = s.apply(p.id, p.revision, &task("first"), "test").unwrap();
    let old = chrono::Local::now().date_naive() - Days::new(1);
    p = s
        .apply(p.id, p.revision, &Change::Capture { day: old }, "test")
        .unwrap();
    let mut first = p.tasks[0].clone();
    first.status = Status::Done;
    p = s
        .apply(p.id, p.revision, &Change::SaveTask { task: first }, "test")
        .unwrap();
    p = s
        .apply(p.id, p.revision, &task("new scope"), "test")
        .unwrap();
    let recorded = p.history.iter().find(|s| s.day == old).unwrap();
    assert_eq!(recorded.scope, 8.0);
    assert_eq!(recorded.remaining, 8.0);
    assert_eq!(recorded.completed, 0.0);
    let now = p.snapshot(chrono::Local::now().date_naive());
    assert_eq!(now.scope, 16.0);
    assert_eq!(now.completed, 8.0);
    assert_eq!(now.remaining, 8.0);
    assert_eq!(p.tasks[0].progress, 100);
}
#[test]
fn refuses_foreign_databases_invalid_values_and_read_only_writes() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("foreign");
    let c = rusqlite::Connection::open(&file).unwrap();
    c.execute("CREATE TABLE mine(value)", []).unwrap();
    drop(c);
    assert!(Store::open(&file, false).is_err());
    let file = dir.path().join("test");
    let mut s = Store::open(&file, false).unwrap();
    let p = create(&mut s);
    let bad = Change::SaveTask {
        task: Task {
            title: "bad".into(),
            estimate: f64::INFINITY,
            ..Default::default()
        },
    };
    assert!(s.apply(p.id, p.revision, &bad, "test").is_err());
    assert!(s.get(p.id).unwrap().tasks.is_empty());
    drop(s);
    let mut read = Store::open(&file, true).unwrap();
    assert!(read.apply(p.id, p.revision, &task("no"), "AI").is_err());
}

#[test]
fn calendar_week_skipping_matches_a_day_by_day_reference() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("calendar"), false).unwrap();
    let mut p = create(&mut store);
    p.start = date("2024-02-23");
    p.finish = date("2025-01-01");
    p.masters = (0..80)
        .step_by(3)
        .enumerate()
        .map(|(i, n)| Master {
            parent: None,
            id: i as i64 + 1,
            kind: MasterKind::Holiday,
            name: format!("Holiday {n}"),
            date: Some(p.start + Days::new(n)),
        })
        .collect();
    p.tasks = (0..200)
        .map(|n| Task {
            id: n + 1,
            title: n.to_string(),
            start: Some(p.start + Days::new((n % 7) as u64)),
            duration_days: n as u32,
            ..Default::default()
        })
        .collect();
    let expected = p
        .tasks
        .iter()
        .map(|t| {
            let mut day = t.start.unwrap();
            while !p.is_workday(day) {
                day = day.succ_opt().unwrap();
            }
            let start = day;
            for _ in 1..t.duration_days.max(1) {
                day = day.succ_opt().unwrap();
                while !p.is_workday(day) {
                    day = day.succ_opt().unwrap();
                }
            }
            (start, day)
        })
        .collect::<Vec<_>>();
    p.schedule().unwrap();
    for (task, (start, due)) in p.tasks.iter().zip(expected) {
        assert_eq!(task.start, Some(start));
        assert_eq!(task.due, Some(due));
    }
}

#[test]
fn completion_requires_predecessors_and_reopening_requires_remaining_work() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("guards"), false).unwrap();
    let mut p = create(&mut store);
    for title in ["先行作業", "後続作業"] {
        p = store.apply(p.id, p.revision, &task(title), "test").unwrap();
    }
    p = store
        .apply(
            p.id,
            p.revision,
            &Change::AddDependency {
                dependency: Dependency {
                    predecessor: 1,
                    successor: 2,
                    lag_days: 0,
                },
            },
            "test",
        )
        .unwrap();
    let revision = p.revision;
    let audit = store.audit(p.id, 0).unwrap().len();
    let mut successor = p.tasks[1].clone();
    successor.status = Status::Done;
    let error = store
        .apply(
            p.id,
            p.revision,
            &Change::SaveTask {
                task: successor.clone(),
            },
            "test",
        )
        .unwrap_err();
    assert!(error.to_string().contains("#1"));
    assert_eq!(store.get(p.id).unwrap().revision, revision);
    assert_eq!(store.audit(p.id, 0).unwrap().len(), audit);
    let mut predecessor = p.tasks[0].clone();
    predecessor.status = Status::Done;
    p = store
        .apply(
            p.id,
            p.revision,
            &Change::SaveTask { task: predecessor },
            "test",
        )
        .unwrap();
    p = store
        .apply(
            p.id,
            p.revision,
            &Change::SaveTask { task: successor },
            "test",
        )
        .unwrap();
    let mut reopened = p.tasks[1].clone();
    reopened.status = Status::Doing;
    assert!(
        store
            .apply(
                p.id,
                p.revision,
                &Change::SaveTask {
                    task: reopened.clone()
                },
                "test"
            )
            .is_err()
    );
    assert_eq!(store.get(p.id).unwrap().revision, p.revision);
    reopened.progress = 40;
    p = store
        .apply(
            p.id,
            p.revision,
            &Change::SaveTask { task: reopened },
            "test",
        )
        .unwrap();
    assert_eq!(p.tasks[1].remaining(), 4.8);
}

#[test]
fn schedule_preview_preserves_finished_dates_and_overrun_requires_explicit_consent() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("preview"), false).unwrap();
    let mut p = create(&mut store);
    let done = Task {
        title: "実績を固定".into(),
        status: Status::Done,
        start: Some(date("2026-10-24")),
        due: Some(date("2026-10-25")),
        ..Default::default()
    };
    p = store
        .apply(p.id, p.revision, &Change::SaveTask { task: done }, "test")
        .unwrap();
    let late = Task {
        title: "長い作業".into(),
        duration_days: 90,
        ..Default::default()
    };
    p = store
        .apply(p.id, p.revision, &Change::SaveTask { task: late }, "test")
        .unwrap();
    let readonly = Store::open(&dir.path().join("preview"), true).unwrap();
    let preview = readonly.get(p.id).unwrap().schedule_preview().unwrap();
    assert_eq!(preview.revision, p.revision);
    assert_eq!(preview.overdue_ids, vec![2]);
    assert_eq!(preview.changes.len(), 1);
    assert_eq!(preview.changes[0].id, 2);
    assert_eq!(store.get(p.id).unwrap().tasks[1].due, None);
    let audit = store.audit(p.id, 0).unwrap().len();
    // Old wire syntax remains accepted, with safe defaults.
    let default: Change = serde_json::from_str(r#"{"operation":"schedule"}"#).unwrap();
    assert!(store.apply(p.id, p.revision, &default, "test").is_err());
    assert_eq!(store.get(p.id).unwrap().revision, p.revision);
    assert_eq!(store.audit(p.id, 0).unwrap().len(), audit);
    p = store
        .apply(
            p.id,
            p.revision,
            &Change::Schedule {
                allow_overrun: true,
            },
            "approved",
        )
        .unwrap();
    assert_eq!(p.tasks[0].start, Some(date("2026-10-24")));
    assert_eq!(p.tasks[0].due, Some(date("2026-10-25")));
    assert_eq!(p.tasks[1].due, preview.changes[0].due);
    assert!(
        serde_json::to_string(&store.audit(p.id, 0).unwrap())
            .unwrap()
            .contains("allow_overrun")
    );
}

#[test]
fn filters_compose_and_exclude_finished_or_undated_tasks_from_overdue() {
    let mut t = Task {
        title: "[SSH] Gateway".into(),
        description: "日本語の確認".into(),
        assignee: Some(4),
        category: Some(2),
        due: Some(date("2026-10-25")),
        ..Default::default()
    };
    let filter = TaskFilter {
        query: "gateway".into(),
        assignee: Some(4),
        category: Some(2),
        incomplete: true,
        overdue_before: Some(date("2026-10-26")),
        ..Default::default()
    };
    assert!(filter.matches(&t));
    t.status = Status::Done;
    assert!(!filter.matches(&t));
    t.status = Status::Cancelled;
    assert!(!filter.matches(&t));
    t.status = Status::Todo;
    t.due = None;
    assert!(!filter.matches(&t));
    t.due = Some(date("2026-10-26"));
    assert!(!filter.matches(&t));
    let unassigned = TaskFilter {
        unassigned: true,
        ..Default::default()
    };
    assert!(!unassigned.matches(&t));
    t.assignee = None;
    assert!(unassigned.matches(&t));
    assert!(
        TaskFilter {
            query: "日本語".into(),
            ..Default::default()
        }
        .matches(&t)
    );
}

#[test]
fn review_classification_and_issue_effort_are_validated_and_persisted() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("review");
    let mut store = Store::open(&file, false).unwrap();
    let mut p = create(&mut store);
    p = store
        .apply(
            p.id,
            p.revision,
            &Change::SaveMaster {
                master: Master {
                    parent: None,
                    id: 0,
                    kind: MasterKind::Assignee,
                    name: "QA".into(),
                    date: None,
                },
            },
            "test",
        )
        .unwrap();
    p = store
        .apply(p.id, p.revision, &task("対応作業"), "test")
        .unwrap();
    let mut issue = Task {
        title: "課題票".into(),
        kind: Kind::Issue,
        estimate: 8.,
        include_in_effort: false,
        related_tasks: vec![1],
        priority: Priority::Critical,
        severity: Severity::Major,
        review: ReviewState::Pending,
        reviewer: Some(1),
        ..Default::default()
    };
    p = store
        .apply(
            p.id,
            p.revision,
            &Change::SaveTask {
                task: issue.clone(),
            },
            "test",
        )
        .unwrap();
    assert_eq!(p.snapshot(date("2026-10-26")).scope, 8.);
    assert_eq!(p.tasks[1].remaining(), 0.);
    issue = p.tasks[1].clone();
    issue.status = Status::Done;
    assert!(
        store
            .apply(
                p.id,
                p.revision,
                &Change::SaveTask {
                    task: issue.clone()
                },
                "test"
            )
            .is_err()
    );
    issue.review = ReviewState::Approved;
    assert!(
        store
            .apply(
                p.id,
                p.revision,
                &Change::SaveTask {
                    task: issue.clone()
                },
                "test"
            )
            .is_err()
    );
    issue.review_note = "受入条件を満たすことを確認".into();
    p = store
        .apply(
            p.id,
            p.revision,
            &Change::SaveTask { task: issue },
            "reviewer",
        )
        .unwrap();
    let readonly = Store::open(&file, true).unwrap();
    let saved = readonly.get(p.id).unwrap();
    assert_eq!(saved.tasks[1].priority, Priority::Critical);
    assert_eq!(saved.tasks[1].review, ReviewState::Approved);
    assert_eq!(saved.tasks[1].progress, 100);
    assert_eq!(saved.tasks[1].related_tasks, vec![1]);
    assert_eq!(saved.snapshot(date("2026-10-26")).scope, 8.);
    let mut bad = saved.tasks[1].clone();
    bad.related_tasks = vec![1, 1];
    assert!(
        store
            .apply(p.id, p.revision, &Change::SaveTask { task: bad }, "test")
            .is_err()
    );
    let mut bad = saved.tasks[0].clone();
    bad.kind = Kind::Issue;
    assert!(
        store
            .apply(p.id, p.revision, &Change::SaveTask { task: bad }, "test")
            .is_err()
    );
    let mut changed = saved.tasks[1].clone();
    changed.include_in_effort = true;
    p = store
        .apply(
            p.id,
            p.revision,
            &Change::SaveTask { task: changed },
            "explicit opt-in",
        )
        .unwrap();
    assert_eq!(p.snapshot(date("2026-10-26")).scope, 16.);
    assert_eq!(p.snapshot(date("2026-10-26")).completed, 8.);
    let filter = TaskFilter {
        review: Some(ReviewState::Approved),
        priority: Some(Priority::Critical),
        ..Default::default()
    };
    assert_eq!(p.tasks.iter().filter(|t| filter.matches(t)).count(), 1);
}

#[test]
fn archive_preview_preserves_ids_links_and_masters_and_restore_is_atomic() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("archive"), false).unwrap();
    let mut p = create(&mut store);
    for title in ["先行", "後続"] {
        p = store.apply(p.id, p.revision, &task(title), "test").unwrap();
    }
    p = store
        .apply(
            p.id,
            p.revision,
            &Change::AddDependency {
                dependency: Dependency {
                    predecessor: 1,
                    successor: 2,
                    lag_days: 2,
                },
            },
            "test",
        )
        .unwrap();
    p = store
        .apply(
            p.id,
            p.revision,
            &Change::SaveMaster {
                master: Master {
                    parent: None,
                    id: 0,
                    kind: MasterKind::Assignee,
                    name: "保管後も必要な担当者".into(),
                    date: None,
                },
            },
            "test",
        )
        .unwrap();
    let mut assigned = p.tasks[0].clone();
    assigned.assignee = Some(1);
    p = store
        .apply(
            p.id,
            p.revision,
            &Change::SaveTask { task: assigned },
            "test",
        )
        .unwrap();
    let before = serde_json::to_value(&p.tasks[0]).unwrap();
    let preview = p.removal_preview(1).unwrap();
    assert_eq!(preview.dependencies.len(), 1);
    assert_eq!(preview.revision, p.revision);
    let audit = store.audit(p.id, 0).unwrap().len();
    assert_eq!(store.audit(p.id, 0).unwrap().len(), audit);
    p = store
        .apply(p.id, p.revision, &Change::RemoveTask { id: 1 }, "test")
        .unwrap();
    assert_eq!(p.tasks.len(), 2);
    assert!(p.tasks[0].archived);
    assert_eq!(p.dependencies.len(), 1);
    assert!(
        store
            .apply(p.id, p.revision, &Change::RemoveMaster { id: 1 }, "test")
            .is_err()
    );

    assert_eq!(p.active_dependencies().count(), 0);
    assert_eq!(p.network().unwrap().len(), 1);
    assert_eq!(p.snapshot(date("2026-10-26")).scope, 8.);
    assert!(!TaskFilter::default().matches(&p.tasks[0]));
    assert!(
        TaskFilter {
            archived: true,
            ..Default::default()
        }
        .matches(&p.tasks[0])
    );
    assert!(
        store
            .apply(p.id, p.revision, &Change::RemoveTask { id: 1 }, "test")
            .is_err()
    );
    assert!(
        !p.schedule_preview()
            .unwrap()
            .changes
            .iter()
            .any(|c| c.id == 1)
    );
    let stale = p.revision;
    p = store
        .apply(p.id, p.revision, &Change::RestoreTask { id: 1 }, "test")
        .unwrap();
    assert_eq!(serde_json::to_value(&p.tasks[0]).unwrap(), before);
    assert_eq!(p.active_dependencies().count(), 1);
    assert_eq!(p.network().unwrap().len(), 2);
    assert!(matches!(
        store.apply(p.id, stale, &Change::RemoveTask { id: 1 }, "test"),
        Err(Error::Conflict)
    ));
    assert!(
        store
            .apply(p.id, p.revision, &Change::RestoreTask { id: 1 }, "test")
            .is_err()
    );
    assert_eq!(store.get(p.id).unwrap().revision, p.revision);
}

#[test]
fn legacy_documents_keep_effort_defaults_and_only_successful_writes_upgrade_format() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("legacy");
    let mut store = Store::open(&file, false).unwrap();
    let mut p = create(&mut store);
    p = store
        .apply(p.id, p.revision, &task("従来データ"), "test")
        .unwrap();
    drop(store);
    let mut document = serde_json::to_value(&p).unwrap();
    for key in [
        "priority",
        "severity",
        "review",
        "reviewer",
        "review_note",
        "include_in_effort",
        "related_tasks",
        "archived",
        "completed_on",
    ] {
        document["tasks"][0].as_object_mut().unwrap().remove(key);
    }
    let sql = rusqlite::Connection::open(&file).unwrap();
    sql.pragma_update(None, "user_version", 1).unwrap();
    sql.execute(
        "UPDATE projects SET document=? WHERE id=?",
        rusqlite::params![document.to_string(), p.id],
    )
    .unwrap();
    drop(sql);
    let readonly = Store::open(&file, true).unwrap();
    let legacy = readonly.get(p.id).unwrap();
    assert!(legacy.tasks[0].include_in_effort);
    assert!(!legacy.tasks[0].archived);
    assert_eq!(legacy.tasks[0].completed_on, None);
    assert_eq!(legacy.tasks[0].priority, Priority::Normal);
    assert_eq!(legacy.snapshot(date("2026-10-26")).scope, 8.);
    drop(readonly);
    let mut store = Store::open(&file, false).unwrap();
    assert!(
        store
            .apply(p.id, p.revision, &Change::RestoreTask { id: 1 }, "test")
            .is_err()
    );
    let sql = rusqlite::Connection::open(&file).unwrap();
    assert_eq!(
        sql.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    p = store
        .apply(p.id, p.revision, &Change::RemoveTask { id: 1 }, "test")
        .unwrap();
    assert_eq!(
        sql.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        5
    );
    assert!(store.get(p.id).unwrap().tasks[0].archived);
}
