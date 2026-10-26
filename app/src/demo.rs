//! Replay the fictional operating scenario through the real transactional Store.
use chrono::{Duration, Local, NaiveDate};
use serde::Deserialize;
use std::{collections::BTreeMap, path::Path};
use yy_project::{Change, Dependency, Kind, Master, MasterKind, Project, Status, Store, Task};

#[derive(Deserialize)]
struct Scenario {
    schema_version: u32,
    projects: Vec<SeedProject>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SeedProject {
    key: String,
    name: String,
    start_offset: i64,
    finish_offset: i64,
    masters: Vec<SeedMaster>,
    items: Vec<SeedTask>,
    checkpoints: Vec<Checkpoint>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SeedMaster {
    key: String,
    kind: MasterKind,
    name: String,
    day_offset: Option<i64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SeedTask {
    key: String,
    title: String,
    description: String,
    kind: Kind,
    assignee: Option<String>,
    category: String,
    duration_days: u32,
    estimate: f64,
    start_offset: i64,
    due_offset: Option<i64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    day_offset: i64,
    actor: String,
    summary: String,
    changes: Vec<Event>,
}
#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum Event {
    Add {
        item: SeedTask,
    },
    Update {
        key: String,
        patch: Patch,
    },
    Link {
        predecessor: String,
        successor: String,
        lag_days: u32,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Patch {
    status: Option<Status>,
    progress: Option<u8>,
    estimate: Option<f64>,
    assignee: Option<String>,
    note: Option<String>,
    priority: Option<yy_project::Priority>,
    severity: Option<yy_project::Severity>,
    review: Option<yy_project::ReviewState>,
    reviewer: Option<String>,
    review_note: Option<String>,
    include_in_effort: Option<bool>,
    related_tasks: Option<Vec<String>>,
}
fn apply(
    store: &mut Store,
    project: &mut Project,
    change: Change,
    actor: &str,
) -> yy_project::Result<()> {
    *project = store.apply(project.id, project.revision, &change, actor)?;
    Ok(())
}
fn reference(map: &BTreeMap<String, i64>, key: &str) -> Result<i64, Box<dyn std::error::Error>> {
    map.get(key)
        .copied()
        .ok_or_else(|| format!("Unknown demo reference: {key}").into())
}
fn add(
    store: &mut Store,
    project: &mut Project,
    item: &SeedTask,
    masters: &BTreeMap<String, i64>,
    tasks: &mut BTreeMap<String, i64>,
    anchor: NaiveDate,
    actor: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if tasks.contains_key(&item.key) {
        return Err(format!("Duplicate demo key: {}", item.key).into());
    }
    let mut start = anchor + Duration::days(item.start_offset);
    while !project.is_workday(start) {
        start += Duration::days(1);
    }
    let mut due = start;
    for _ in 1..item.duration_days.max(1) {
        due += Duration::days(1);
        while !project.is_workday(due) {
            due += Duration::days(1);
        }
    }
    if let Some(offset) = item.due_offset {
        due = anchor + Duration::days(offset);
    }
    apply(
        store,
        project,
        Change::SaveTask {
            task: Task {
                title: format!("[{}] {}", item.key, item.title),
                description: item.description.clone(),
                kind: item.kind,
                include_in_effort: item.kind != Kind::Issue,
                assignee: item
                    .assignee
                    .as_deref()
                    .map(|k| reference(masters, &format!("assignee:{k}")))
                    .transpose()?,
                category: Some(reference(masters, &format!("category:{}", item.category))?),
                duration_days: item.duration_days,
                estimate: item.estimate,
                start: Some(start),
                due: Some(due),
                ..Default::default()
            },
        },
        actor,
    )?;
    tasks.insert(item.key.clone(), project.tasks.last().unwrap().id);
    Ok(())
}
pub fn create(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if path.exists() {
        return Err("Demo destination already exists".into());
    }
    let scenario: Scenario = serde_json::from_str(include_str!("../../demo/scenario.json"))?;
    if scenario.schema_version != 1 || scenario.projects.is_empty() {
        return Err("Unsupported demo scenario".into());
    }
    let anchor = Local::now().date_naive();
    let mut store = Store::open(path, false)?;
    for seed in scenario.projects {
        let actor = format!("demo:{}:setup", seed.key);
        let mut project = store.create(
            seed.name,
            anchor + Duration::days(seed.start_offset),
            anchor + Duration::days(seed.finish_offset),
            &actor,
        )?;
        let mut masters = BTreeMap::new();
        for m in seed.masters {
            let key = format!(
                "{}:{}",
                match m.kind {
                    MasterKind::Category => "category",
                    MasterKind::Assignee => "assignee",
                    MasterKind::Holiday => "holiday",
                },
                m.key
            );
            if masters.contains_key(&key) {
                return Err("Duplicate master key".into());
            }
            apply(
                &mut store,
                &mut project,
                Change::SaveMaster {
                    master: Master {
                        id: 0,
                        kind: m.kind,
                        name: m.name,
                        date: m.day_offset.map(|d| anchor + Duration::days(d)),
                    },
                },
                &actor,
            )?;
            masters.insert(key, project.masters.last().unwrap().id);
        }
        let mut tasks = BTreeMap::new();
        for item in seed.items {
            add(
                &mut store,
                &mut project,
                &item,
                &masters,
                &mut tasks,
                anchor,
                &actor,
            )?;
        }
        let mut prior = None;
        for checkpoint in seed.checkpoints {
            if prior.is_some_and(|d| d >= checkpoint.day_offset) || checkpoint.summary.is_empty() {
                return Err("Invalid demo checkpoint sequence".into());
            }
            prior = Some(checkpoint.day_offset);
            let day = anchor + Duration::days(checkpoint.day_offset);
            let actor = format!("demo:{day}:{}", checkpoint.actor);
            for event in checkpoint.changes {
                match event {
                    Event::Add { item } => add(
                        &mut store,
                        &mut project,
                        &item,
                        &masters,
                        &mut tasks,
                        anchor,
                        &actor,
                    )?,
                    Event::Update { key, patch } => {
                        let id = reference(&tasks, &key)?;
                        let mut task = project.tasks.iter().find(|t| t.id == id).unwrap().clone();
                        if let Some(status) = patch.status {
                            task.status = status;
                        }
                        if let Some(progress) = patch.progress {
                            task.progress = progress;
                        }
                        if let Some(estimate) = patch.estimate {
                            task.estimate = estimate;
                        }
                        if let Some(owner) = patch.assignee {
                            task.assignee =
                                Some(reference(&masters, &format!("assignee:{owner}"))?);
                        }
                        if let Some(value) = patch.priority {
                            task.priority = value;
                        }
                        if let Some(value) = patch.severity {
                            task.severity = value;
                        }
                        if let Some(value) = patch.review {
                            task.review = value;
                        }
                        if let Some(value) = patch.reviewer {
                            task.reviewer =
                                Some(reference(&masters, &format!("assignee:{value}"))?);
                        }
                        if let Some(value) = patch.review_note {
                            task.review_note = value;
                        }
                        if let Some(value) = patch.include_in_effort {
                            task.include_in_effort = value;
                        }
                        if let Some(values) = patch.related_tasks {
                            task.related_tasks = values
                                .iter()
                                .map(|v| reference(&tasks, v))
                                .collect::<Result<_, _>>()?;
                        }
                        if let Some(note) = patch.note {
                            task.description
                                .push_str(&format!("\n\n{day} / {}: {note}", checkpoint.actor));
                        }
                        apply(&mut store, &mut project, Change::SaveTask { task }, &actor)?;
                    }
                    Event::Link {
                        predecessor,
                        successor,
                        lag_days,
                    } => apply(
                        &mut store,
                        &mut project,
                        Change::AddDependency {
                            dependency: Dependency {
                                predecessor: reference(&tasks, &predecessor)?,
                                successor: reference(&tasks, &successor)?,
                                lag_days,
                            },
                        },
                        &actor,
                    )?,
                }
            }
            apply(&mut store, &mut project, Change::Capture { day }, &actor)?;
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn verify(path: &Path) {
        let store = Store::open(path, true).unwrap();
        assert_eq!(store.list().unwrap().len(), 3);
        let primary = store.get(1).unwrap();
        assert_eq!(primary.tasks.len(), 64);
        assert_eq!(
            primary
                .tasks
                .iter()
                .filter(|t| t.kind == Kind::Issue)
                .count(),
            16
        );
        assert_eq!(primary.history.len(), 8);
        let review = primary
            .tasks
            .iter()
            .find(|t| t.title.starts_with("[PRJ-03]"))
            .unwrap();
        assert_eq!(review.review, yy_project::ReviewState::Pending);
        assert!(review.reviewer.is_some());
        let issue = primary
            .tasks
            .iter()
            .find(|t| t.title.starts_with("[ISS-001]"))
            .unwrap();
        assert_eq!(issue.related_tasks.len(), 2);
        assert!(!issue.include_in_effort);
        assert_eq!(issue.severity, yy_project::Severity::Critical);

        for status in Status::ALL {
            assert!(primary.tasks.iter().any(|t| t.status == status));
        }
        assert!(primary.tasks.iter().any(|t| t.assignee.is_none()));
        assert!(
            primary
                .tasks
                .iter()
                .any(|t| t.status == Status::Doing && t.progress == 100)
        );
        assert!(primary.history.windows(2).any(|w| w[1].scope > w[0].scope));
        assert!(
            primary
                .history
                .windows(2)
                .any(|w| w[1].completed < w[0].completed)
        );
        for (id, count) in [(1, 64), (2, 16), (3, 10)] {
            let p = store.get(id).unwrap();
            p.validate().unwrap();
            assert_eq!(p.tasks.len(), count);
            assert!(
                p.tasks
                    .iter()
                    .all(|t| t.start.zip(t.due).is_some_and(|(a, b)| a <= b))
            );
            assert!(p.masters.iter().any(|m| m.kind == MasterKind::Holiday));
        }
        assert!(
            store
                .get(3)
                .unwrap()
                .tasks
                .iter()
                .all(|t| t.status == Status::Done)
        );
    }
    #[test]
    fn generated_scenario_covers_changes_rework_and_parallel_projects_without_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("demo.sqlite3");
        create(&path).unwrap();
        verify(&path);
        let before = std::fs::read(&path).unwrap();
        assert!(create(&path).is_err());
        assert_eq!(before, std::fs::read(&path).unwrap());
    }
    #[test]
    fn bundled_database_is_complete_and_independent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bundled.sqlite3");
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../demo/yyproject-demo.sqlite3"),
            &path,
        )
        .unwrap();
        verify(&path);
    }
}
