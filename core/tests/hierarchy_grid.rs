use chrono::NaiveDate;
use yy_project::*;
fn day() -> NaiveDate {
    "2026-10-27".parse().unwrap()
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
