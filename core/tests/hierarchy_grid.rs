use chrono::NaiveDate;
use yy_project::*;
fn day() -> NaiveDate {
    "2026-10-27".parse().unwrap()
}
#[test]
fn master_columns_are_exclusive_and_fill_down_preserves_individual_task_fields() {
    let keys = |kind| {
        table::master_columns(kind)
            .iter()
            .map(|c| c.key)
            .collect::<Vec<_>>()
    };
    assert_eq!(keys(MasterKind::Category), ["id", "name", "parent"]);
    assert_eq!(keys(MasterKind::Assignee), ["id", "name"]);
    assert_eq!(keys(MasterKind::Holiday), ["id", "name", "date"]);
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(&dir.path().join("fill.db"), false).unwrap();
    let mut p = project(&mut s);
    for title in ["先頭", "対象外", "コピー先"] {
        p = add(&mut s, &p, title, None);
    }
    for _ in 0..2 {
        p = apply(
            &mut s,
            &p,
            Change::SaveMaster {
                master: Master {
                    id: 0,
                    name: "同名担当者".into(),
                    kind: MasterKind::Assignee,
                    date: None,
                    parent: None,
                },
            },
        );
    }
    let mut task = p.tasks[0].clone();
    task.assignee = Some(2);
    task.completed_on = Some(day());
    p = apply(&mut s, &p, Change::SaveTask { task });
    let change =
        table::fill_tasks_down(&p, &[1, 3], &["assignee".into(), "completed_on".into()]).unwrap();
    let before = p.clone();
    p = apply(&mut s, &p, change);
    assert_eq!(p.tasks[2].assignee, Some(2));
    assert_eq!(p.tasks[2].completed_on, Some(day()));
    assert_eq!(p.tasks[2].title, "コピー先");
    assert_eq!(
        serde_json::to_value(&p.tasks[0]).unwrap(),
        serde_json::to_value(&before.tasks[0]).unwrap()
    );
    assert!(p.tasks[1].assignee.is_none());
    assert!(table::fill_tasks_down(&p, &[1, 3], &["title".into()]).is_err());
    assert!(
        table::batch_task_attributes(&p, &[1, 3], &[("title".into(), "同じ件名".into())]).is_err()
    );
    assert!(table::fill_tasks_down(&p, &[1], &["assignee".into()]).is_err());
}
#[test]
fn selected_rows_update_attributes_and_actual_completion_date_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(&dir.path().join("batch.db"), false).unwrap();
    let mut p = project(&mut s);
    for name in ["A", "B", "C"] {
        p = add(&mut s, &p, name, None);
    }
    p = apply(
        &mut s,
        &p,
        Change::SaveMaster {
            master: Master {
                id: 0,
                kind: MasterKind::Assignee,
                name: "担当者".into(),
                date: None,
                parent: None,
            },
        },
    );
    let change = table::batch_task_attributes(
        &p,
        &[1, 3],
        &[
            ("assignee".into(), "1".into()),
            ("completed_on".into(), "2026-10-27".into()),
        ],
    )
    .unwrap();
    let revision = p.revision;
    p = apply(&mut s, &p, change);
    assert_eq!(p.revision, revision + 1);
    for t in &p.tasks {
        assert_eq!(t.assignee, if t.id == 2 { None } else { Some(1) });
        assert_eq!(t.completed_on, if t.id == 2 { None } else { Some(day()) });
        assert_eq!(t.status, Status::Todo);
    }
    let before = serde_json::to_value(s.snapshot().unwrap()).unwrap();
    assert!(
        table::batch_task_attributes(&p, &[1, 3], &[("assignee".into(), "999".into())]).is_err()
    );
    assert!(
        table::batch_task_attributes(&p, &[1, 3], &[("completed_on".into(), "2026-02-30".into())])
            .is_err()
    );
    assert!(table::batch_task_attributes(&p, &[1, 1], &[("assignee".into(), "1".into())]).is_err());
    assert!(table::batch_task_attributes(&p, &[1, 3], &[]).is_err());
    assert_eq!(serde_json::to_value(s.snapshot().unwrap()).unwrap(), before);
    let out = dir.path().join("round-trip.db");
    s.export_sqlite(&out).unwrap();
    assert_eq!(
        Store::open(&out, true).unwrap().get(p.id).unwrap().tasks[0].completed_on,
        Some(day())
    );
    let sql = rusqlite::Connection::open(&out).unwrap();
    assert_eq!(
        sql.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        5
    );
}
#[test]
fn master_grid_batch_keeps_conflicts_audits_and_validation_atomic() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(&dir.path().join("masters.db"), false).unwrap();
    let mut p = project(&mut s);
    for name in ["親", "子"] {
        p = apply(
            &mut s,
            &p,
            Change::SaveMaster {
                master: Master {
                    id: 0,
                    kind: MasterKind::Category,
                    name: name.into(),
                    date: None,
                    parent: None,
                },
            },
        );
    }
    let mut masters = p.masters.clone();
    table::set_master_value(&mut masters[0], "name", "親,引用\"日本語").unwrap();
    table::set_master_value(&mut masters[1], "parent", "1").unwrap();
    let old_revision = p.revision;
    let audits = s.snapshot().unwrap().audit.len();
    p = apply(
        &mut s,
        &p,
        Change::SaveMasters {
            masters: masters.clone(),
        },
    );
    assert_eq!(p.revision, old_revision + 1);
    assert_eq!(s.snapshot().unwrap().audit.len(), audits + 1);
    assert_eq!(p.category_path(Some(2)), ["親,引用\"日本語", "子"]);
    assert!(
        s.apply(
            p.id,
            old_revision,
            &Change::SaveMasters {
                masters: masters.clone()
            },
            "stale"
        )
        .is_err()
    );
    let before = serde_json::to_value(s.snapshot().unwrap()).unwrap();
    masters[0].parent = Some(2);
    assert!(
        s.apply(p.id, p.revision, &Change::SaveMasters { masters }, "test")
            .is_err()
    );
    assert_eq!(serde_json::to_value(s.snapshot().unwrap()).unwrap(), before);
    let mut holiday = p.masters[0].clone();
    table::set_master_value(&mut holiday, "kind", "休日").unwrap();
    table::set_master_value(&mut holiday, "date", "2026-10-27").unwrap();
    assert_eq!(table::master_value(&holiday, "date"), "2026-10-27");
    assert!(table::set_master_value(&mut holiday, "id", "999").is_err());
    assert!(table::set_master_value(&mut holiday, "date", "2026-02-30").is_err());
}
#[test]
fn jira_export_preserves_filtered_ancestors_types_and_quoted_fields() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(&dir.path().join("jira.db"), false).unwrap();
    let mut p = project(&mut s);
    p = add(&mut s, &p, "親", None);
    p = add(&mut s, &p, "子", Some(1));
    p = add(&mut s, &p, "孫,\"引用\"", Some(2));
    p = add(&mut s, &p, "対象外", None);
    let mut task = p.tasks[2].clone();
    task.description = "日本語\n複数行,引用\"".into();
    task.due = Some(day());
    p = apply(&mut s, &p, Change::SaveTask { task });
    let filter = TaskFilter {
        query: "複数行".into(),
        ..Default::default()
    };
    let keys = vec!["description".into(), "due".into()];
    let options = table::JiraOptions::default();
    let csv = table::export_jira_csv(&p, &filter, &keys, &options).unwrap();
    assert!(csv.starts_with('\u{feff}'));
    let rows = table::decode(&csv, ',').unwrap();
    assert_eq!(rows.len(), 4);
    assert_eq!(
        rows[0],
        [
            "Summary",
            "Issue Type",
            "Issue ID",
            "Parent",
            "Description",
            "Due Date"
        ]
    );
    assert_eq!(&rows[1][..4], ["親", "Epic", "1", ""]);
    assert_eq!(&rows[2][..4], ["子", "Task", "2", "1"]);
    assert_eq!(&rows[3][..4], ["孫,\"引用\"", "Sub-task", "3", "2"]);
    assert_eq!(rows[3][4], "日本語\n複数行,引用\"");
    assert_eq!(rows[3][5], "2026-10-27");
    let two_levels = table::JiraOptions {
        epic_roots: false,
        ..Default::default()
    };
    assert!(table::export_jira_csv(&p, &filter, &keys, &two_levels).is_err());
    assert!(table::export_jira_csv(&p, &filter, &["title".into()], &options).is_err());
    assert!(table::export_jira_csv(&p, &filter, &["due".into(), "due".into()], &options).is_err());
    let filter = TaskFilter {
        query: "親".into(),
        ..Default::default()
    };
    let rows = table::decode(
        &table::export_jira_csv(&p, &filter, &[], &two_levels).unwrap(),
        ',',
    )
    .unwrap();
    assert_eq!(rows[1][1], "Task");
    p.tasks[3].kind = Kind::Issue;
    p.tasks.swap(0, 3);
    let rows = table::decode(
        &table::export_jira_csv(&p, &TaskFilter::default(), &[], &options).unwrap(),
        ',',
    )
    .unwrap();
    assert_eq!(
        rows[1..].iter().map(|r| r[1].as_str()).collect::<Vec<_>>(),
        ["Epic", "Bug", "Task", "Sub-task"]
    );
    let invalid_options = table::JiraOptions {
        subtask_type: String::new(),
        ..Default::default()
    };
    assert!(table::export_jira_csv(&p, &TaskFilter::default(), &[], &invalid_options).is_err());
    p.tasks.swap(0, 3);
    p.tasks[0].title = "あ".repeat(256);
    assert!(table::export_jira_csv(&p, &TaskFilter::default(), &[], &options).is_err());
    p.tasks[0].title = "親".into();
    p = add(&mut s, &p, "ひ孫", Some(3));
    assert!(table::export_jira_csv(&p, &TaskFilter::default(), &[], &options).is_err());
}
fn project(store: &mut Store) -> Project {
    store
        .create("階層".into(), day(), day() + chrono::Days::new(30), "test")
        .unwrap()
}
fn apply(store: &mut Store, p: &Project, c: Change) -> Project {
    store.apply(p.id, p.revision, &c, "test").unwrap()
}
fn add(store: &mut Store, p: &Project, title: &str, parent: Option<i64>) -> Project {
    apply(
        store,
        p,
        Change::SaveTask {
            task: Task {
                title: title.into(),
                parent,
                ..Default::default()
            },
        },
    )
}
#[test]
fn hierarchical_categories_and_tasks_are_validated_and_reordering_preserves_subtrees() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(&dir.path().join("data.db"), false).unwrap();
    let mut p = project(&mut s);
    for (i, parent) in [None, Some(1), Some(2)].into_iter().enumerate() {
        p = apply(
            &mut s,
            &p,
            Change::SaveMaster {
                master: Master {
                    id: 0,
                    kind: MasterKind::Category,
                    name: format!("階層{}", i + 1),
                    date: None,
                    parent,
                },
            },
        );
    }
    let revision = p.revision;
    let audit = s.audit(p.id, 0).unwrap().len();
    for parent in [Some(3), Some(999)] {
        assert!(
            s.apply(
                p.id,
                revision,
                &Change::SaveMaster {
                    master: Master {
                        id: 0,
                        kind: MasterKind::Category,
                        name: "不正".into(),
                        date: None,
                        parent
                    }
                },
                "test"
            )
            .is_err()
        );
    }
    assert_eq!(s.get(p.id).unwrap().revision, revision);
    assert_eq!(s.audit(p.id, 0).unwrap().len(), audit);
    p = add(&mut s, &p, "親", None);
    p = add(&mut s, &p, "子", Some(1));
    p = add(&mut s, &p, "孫", Some(2));
    p = add(&mut s, &p, "別の親", None);
    assert_eq!(
        p.ordered_tasks()
            .iter()
            .map(|(t, d)| (t.id, *d))
            .collect::<Vec<_>>(),
        [(1, 0), (2, 1), (3, 2), (4, 0)]
    );
    let mut root = p.tasks[0].clone();
    root.parent = Some(3);
    assert!(
        s.apply(p.id, p.revision, &Change::SaveTask { task: root }, "test")
            .is_err()
    );
    assert!(
        s.apply(
            p.id,
            p.revision,
            &Change::MoveTask {
                id: 1,
                before: Some(3)
            },
            "test"
        )
        .is_err()
    );
    p = apply(
        &mut s,
        &p,
        Change::MoveTask {
            id: 4,
            before: Some(1),
        },
    );
    assert_eq!(
        p.ordered_tasks()
            .iter()
            .map(|(t, _)| t.id)
            .collect::<Vec<_>>(),
        [4, 1, 2, 3]
    );
    p = apply(
        &mut s,
        &p,
        Change::MoveTask {
            id: 2,
            before: Some(4),
        },
    );
    assert_eq!(
        p.ordered_tasks()
            .iter()
            .map(|(t, d)| (t.id, *d))
            .collect::<Vec<_>>(),
        [(2, 0), (3, 1), (4, 0), (1, 0)]
    );
    let mut leaf = p.tasks.iter().find(|t| t.id == 3).unwrap().clone();
    leaf.category = Some(3);
    p = apply(&mut s, &p, Change::SaveTask { task: leaf });
    assert_eq!(p.category_path(Some(3)), ["階層1", "階層2", "階層3"]);
    assert!(p.matches_filter(
        &TaskFilter {
            category: Some(1),
            ..Default::default()
        },
        p.tasks.iter().find(|t| t.id == 3).unwrap()
    ));
}
#[test]
fn batch_edits_are_atomic_and_completion_gates_still_apply() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(&dir.path().join("data.db"), false).unwrap();
    let mut p = project(&mut s);
    p = add(&mut s, &p, "先行", None);
    p = add(&mut s, &p, "後続", None);
    p = apply(
        &mut s,
        &p,
        Change::AddDependency {
            dependency: Dependency {
                predecessor: 1,
                successor: 2,
                lag_days: 0,
            },
        },
    );
    let mut tasks = p.tasks.clone();
    tasks[0].title = "保存されない".into();
    tasks[1].title = "".into();
    assert!(
        s.apply(p.id, p.revision, &Change::SaveTasks { tasks }, "grid")
            .is_err()
    );
    assert_eq!(s.get(p.id).unwrap().tasks[0].title, "先行");
    let mut tasks = p.tasks.clone();
    for t in &mut tasks {
        t.status = Status::Done;
    }
    tasks.reverse();
    p = apply(&mut s, &p, Change::SaveTasks { tasks });
    assert!(p.tasks.iter().all(|t| t.progress == 100));
    let mut t = p.tasks[0].clone();
    t.status = Status::Doing;
    assert!(
        s.apply(
            p.id,
            p.revision,
            &Change::SaveTasks { tasks: vec![t] },
            "grid"
        )
        .is_err()
    );
}
#[test]
fn clipboard_csv_and_full_text_search_preserve_unicode_quotes_and_multiline_values() {
    let rows = vec![
        vec![
            "日本語\tcell".into(),
            "line1\nline2,\"quoted\"".into(),
            "".into(),
        ],
        vec!["3".into(), "=literal".into(), "終端".into()],
    ];
    for separator in ['\t', ','] {
        assert_eq!(
            table::decode(&table::encode(&rows, separator), separator).unwrap(),
            rows
        );
    }
    assert!(table::decode("\"unterminated", '\t').is_err());
    assert!(table::decode("\"a\"extra", ',').is_err());
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(&dir.path().join("data.db"), false).unwrap();
    let mut p = project(&mut s);
    p = add(&mut s, &p, "件名", None);
    let mut t = p.tasks[0].clone();
    t.description = rows[0][1].clone();
    t.review_note = "全文検索だけに含まれる語".into();
    p = apply(&mut s, &p, Change::SaveTask { task: t });
    let filter = TaskFilter {
        query: "検索だけ".into(),
        ..Default::default()
    };
    assert!(p.matches_filter(&filter, &p.tasks[0]));
    let csv = table::export_csv(&p, &filter, &["description".into(), "title".into()]).unwrap();
    let decoded = table::decode(&csv, ',').unwrap();
    assert_eq!(decoded[0], ["説明", "件名"]);
    assert_eq!(decoded[1], [rows[0][1].clone(), "件名".into()]);
    assert!(table::export_csv(&p, &filter, &["unknown".into()]).is_err());
}
#[test]
fn milestones_and_complete_sqlite_transfer_keep_ids_history_and_all_audit_rows() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(&dir.path().join("data.db"), false).unwrap();
    let mut p = project(&mut s);
    p = apply(
        &mut s,
        &p,
        Change::SaveTask {
            task: Task {
                title: "節目".into(),
                start: Some(day()),
                due: Some(day()),
                milestone: true,
                ..Default::default()
            },
        },
    );
    for i in 0..110 {
        let mut t = p.tasks[0].clone();
        t.description = format!("履歴 {i}");
        p = apply(&mut s, &p, Change::SaveTask { task: t });
    }
    let mut invalid = p.tasks[0].clone();
    invalid.due = Some(day() + chrono::Days::new(1));
    assert!(
        s.apply(
            p.id,
            p.revision,
            &Change::SaveTask { task: invalid },
            "test"
        )
        .is_err()
    );
    let before = s.snapshot().unwrap();
    assert_eq!(before.audit.len(), 112);
    let target = dir.path().join("export.sqlite3");
    s.export_sqlite(&target).unwrap();
    assert!(s.export_sqlite(&target).is_err());
    let mut exported = Store::open(&target, true).unwrap();
    assert_eq!(
        serde_json::to_value(before).unwrap(),
        serde_json::to_value(exported.snapshot().unwrap()).unwrap()
    );
    let dump = exported.snapshot().unwrap();
    assert!(s.import(&dump).is_err());
}
