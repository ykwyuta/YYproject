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
    assert_eq!(p.tasks[0].id, 2);
    drop(a);
    drop(b);
    let saved = Store::open(&file, true).unwrap();
    assert_eq!(saved.get(p.id).unwrap().tasks[0].title, "task2");
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
        .apply(p.id, p.revision, &Change::Schedule, "test")
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
