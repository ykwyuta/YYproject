//! Shared transactional project model for native UI and MCP.
use chrono::{Datelike, Days, NaiveDate, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::Duration,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("Concurrent update: reload project before retrying")]
    Conflict,
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
pub type Result<T> = std::result::Result<T, Error>;
fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Todo,
    Doing,
    Blocked,
    Done,
    Cancelled,
}
impl Status {
    pub const ALL: [Self; 5] = [
        Self::Todo,
        Self::Doing,
        Self::Blocked,
        Self::Done,
        Self::Cancelled,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Todo => "未着手",
            Self::Doing => "進行中",
            Self::Blocked => "保留",
            Self::Done => "完了",
            Self::Cancelled => "中止",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[default]
    Task,
    Issue,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MasterKind {
    Category,
    Assignee,
    Holiday,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Master {
    pub id: i64,
    pub kind: MasterKind,
    pub name: String,
    #[serde(default)]
    pub date: Option<NaiveDate>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Task {
    pub id: i64,
    pub title: String,
    pub description: String,
    pub kind: Kind,
    pub status: Status,
    pub assignee: Option<i64>,
    pub category: Option<i64>,
    pub start: Option<NaiveDate>,
    pub due: Option<NaiveDate>,
    pub duration_days: u32,
    pub estimate: f64,
    pub progress: u8,
    pub priority: Priority,
    pub severity: Severity,
    pub review: ReviewState,
    pub reviewer: Option<i64>,
    pub review_note: String,
    pub include_in_effort: bool,
    pub related_tasks: Vec<i64>,
    pub archived: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Low,
    #[default]
    Normal,
    High,
    Critical,
}
impl Priority {
    pub const ALL: [Self; 4] = [Self::Low, Self::Normal, Self::High, Self::Critical];
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "低",
            Self::Normal => "通常",
            Self::High => "高",
            Self::Critical => "最優先",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    #[default]
    Unspecified,
    Minor,
    Major,
    Critical,
}
impl Severity {
    pub const ALL: [Self; 4] = [Self::Unspecified, Self::Minor, Self::Major, Self::Critical];
    pub fn label(self) -> &'static str {
        match self {
            Self::Unspecified => "未指定",
            Self::Minor => "軽微",
            Self::Major => "重大",
            Self::Critical => "致命的",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    #[default]
    NotRequired,
    Pending,
    Approved,
    ChangesRequested,
}
impl ReviewState {
    pub const ALL: [Self; 4] = [
        Self::NotRequired,
        Self::Pending,
        Self::Approved,
        Self::ChangesRequested,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::NotRequired => "確認不要",
            Self::Pending => "確認待ち",
            Self::Approved => "承認済み",
            Self::ChangesRequested => "差し戻し",
        }
    }
}
impl Default for Task {
    fn default() -> Self {
        Self {
            id: 0,
            title: String::new(),
            description: String::new(),
            kind: Kind::Task,
            status: Status::Todo,
            assignee: None,
            category: None,
            start: None,
            due: None,
            duration_days: 0,
            estimate: 0.,
            progress: 0,
            priority: Priority::Normal,
            severity: Severity::Unspecified,
            review: ReviewState::NotRequired,
            reviewer: None,
            review_note: String::new(),
            include_in_effort: true,
            related_tasks: vec![],
            archived: false,
        }
    }
}
impl Task {
    pub fn remaining(&self) -> f64 {
        if self.archived
            || !self.include_in_effort
            || matches!(self.status, Status::Done | Status::Cancelled)
        {
            0.0
        } else {
            self.estimate * (100 - self.progress) as f64 / 100.0
        }
    }
}
/// Shared task selection for GUI lists and paged MCP queries. Conditions are ANDed.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TaskFilter {
    pub query: String,
    pub status: Option<Status>,
    pub kind: Option<Kind>,
    pub assignee: Option<i64>,
    pub category: Option<i64>,
    pub unassigned: bool,
    pub incomplete: bool,
    pub overdue_before: Option<NaiveDate>,
    pub priority: Option<Priority>,
    pub severity: Option<Severity>,
    pub review: Option<ReviewState>,
    pub archived: bool,
}
impl TaskFilter {
    pub fn matches(&self, t: &Task) -> bool {
        self.archived == t.archived
            && self.priority.is_none_or(|p| p == t.priority)
            && self.severity.is_none_or(|s| s == t.severity)
            && self.review.is_none_or(|r| r == t.review)
            && self.status.is_none_or(|s| s == t.status)
            && self.kind.is_none_or(|k| k == t.kind)
            && self.assignee.is_none_or(|id| t.assignee == Some(id))
            && self.category.is_none_or(|id| t.category == Some(id))
            && (!self.unassigned || t.assignee.is_none())
            && (!self.incomplete || !matches!(t.status, Status::Done | Status::Cancelled))
            && self.overdue_before.is_none_or(|day| {
                !matches!(t.status, Status::Done | Status::Cancelled)
                    && t.due.is_some_and(|due| due < day)
            })
            && (self.query.is_empty()
                || t.title.to_lowercase().contains(&self.query.to_lowercase())
                || t.description
                    .to_lowercase()
                    .contains(&self.query.to_lowercase()))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dependency {
    pub predecessor: i64,
    pub successor: i64,
    #[serde(default)]
    pub lag_days: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub day: NaiveDate,
    pub scope: f64,
    pub completed: f64,
    pub remaining: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub id: i64,
    pub revision: i64,
    pub name: String,
    pub start: NaiveDate,
    pub finish: NaiveDate,
    pub tasks: Vec<Task>,
    pub masters: Vec<Master>,
    pub dependencies: Vec<Dependency>,
    pub history: Vec<Snapshot>,
    pub next_task_id: i64,
    pub next_master_id: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Change {
    Rename {
        name: String,
        start: NaiveDate,
        finish: NaiveDate,
    },
    SaveTask {
        task: Task,
    },
    RemoveTask {
        id: i64,
    },
    RestoreTask {
        id: i64,
    },
    SaveMaster {
        master: Master,
    },
    RemoveMaster {
        id: i64,
    },
    AddDependency {
        dependency: Dependency,
    },
    RemoveDependency {
        predecessor: i64,
        successor: i64,
    },
    Schedule {
        #[serde(default)]
        allow_overrun: bool,
    },
    Capture {
        day: NaiveDate,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemovalPreview {
    pub revision: i64,
    pub task: Task,
    pub dependencies: Vec<Dependency>,
    pub related_issue_ids: Vec<i64>,
}
/// Read-only planning result. Applying it still requires the observed revision.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SchedulePreview {
    pub revision: i64,
    pub changes: Vec<ScheduleDifference>,
    pub overdue_ids: Vec<i64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScheduleDifference {
    pub id: i64,
    pub title: String,
    pub old_start: Option<NaiveDate>,
    pub old_due: Option<NaiveDate>,
    pub start: Option<NaiveDate>,
    pub due: Option<NaiveDate>,
}
fn next_id(values: impl Iterator<Item = i64>) -> Result<i64> {
    values
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| invalid("ID exhausted"))
}
fn name(value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 4096 || value.chars().any(|c| c.is_control()) {
        Err(invalid(
            "Name must contain 1..4096 bytes without control characters",
        ))
    } else {
        Ok(())
    }
}
impl Project {
    pub fn validate(&self) -> Result<()> {
        name(&self.name)?;
        if !(1..=9999).contains(&self.start.year()) || !(1..=9999).contains(&self.finish.year()) {
            return Err(invalid("Dates must use years 0001..9999"));
        }
        if self.start > self.finish || (self.finish - self.start).num_days() > 3660 {
            return Err(invalid("Project dates must cover at most ten years"));
        }
        if self.tasks.len() > 20000
            || self.masters.len() > 20000
            || self.dependencies.len() > 100000
        {
            return Err(invalid("Project item limit exceeded"));
        }
        let mut masters = BTreeMap::new();
        for m in &self.masters {
            name(&m.name)?;
            if m.id <= 0 || masters.insert(m.id, m.kind).is_some() {
                return Err(invalid("Duplicate or invalid master ID"));
            }
            if (m.kind == MasterKind::Holiday) != m.date.is_some() {
                return Err(invalid("Only holidays require a date"));
            }
        }
        let mut ids = BTreeSet::new();
        for t in &self.tasks {
            name(&t.title)?;
            if [t.start, t.due]
                .into_iter()
                .flatten()
                .any(|d| !(1..=9999).contains(&d.year()))
            {
                return Err(invalid("Task dates must use years 0001..9999"));
            }
            if t.id <= 0 || !ids.insert(t.id) {
                return Err(invalid("Duplicate or invalid task ID"));
            }
            if t.progress > 100
                || !t.estimate.is_finite()
                || !(0.0..=1e9).contains(&t.estimate)
                || t.description.len() > 1024 * 1024
                || t.review_note.len() > 65536
                || t.related_tasks.len() > 1000
                || t.duration_days > 3660
            {
                return Err(invalid("Invalid progress, effort, description or duration"));
            }
            if t.start.zip(t.due).is_some_and(|(a, b)| a > b) {
                return Err(invalid("Task start is after due date"));
            }
            for (id, kind) in [
                (t.assignee, MasterKind::Assignee),
                (t.reviewer, MasterKind::Assignee),
                (t.category, MasterKind::Category),
            ] {
                if id.is_some_and(|id| masters.get(&id) != Some(&kind)) {
                    return Err(invalid("Task refers to a missing or incompatible master"));
                }
            }
        }
        let task_kinds: BTreeMap<_, _> = self.tasks.iter().map(|t| (t.id, t.kind)).collect();
        for t in &self.tasks {
            if t.review != ReviewState::NotRequired && t.reviewer.is_none() {
                return Err(invalid("Review requires a reviewer"));
            }
            if matches!(
                t.review,
                ReviewState::Approved | ReviewState::ChangesRequested
            ) && t.review_note.trim().is_empty()
            {
                return Err(invalid("Approval or change request requires a review note"));
            }
            if t.status == Status::Done
                && matches!(
                    t.review,
                    ReviewState::Pending | ReviewState::ChangesRequested
                )
            {
                return Err(invalid("Pending or rejected review cannot be completed"));
            }
            let mut related = BTreeSet::new();
            if !t.related_tasks.is_empty() && t.kind != Kind::Issue {
                return Err(invalid("Only issues can link to response tasks"));
            }
            for id in &t.related_tasks {
                if *id == t.id || !related.insert(*id) || task_kinds.get(id) != Some(&Kind::Task) {
                    return Err(invalid(
                        "Issue refers to a missing, duplicate or incompatible response task",
                    ));
                }
            }
        }
        self.topological()?;
        Ok(())
    }
    pub fn active_dependencies(&self) -> impl Iterator<Item = &Dependency> {
        let active: BTreeSet<_> = self
            .tasks
            .iter()
            .filter(|t| !t.archived)
            .map(|t| t.id)
            .collect();
        self.dependencies
            .iter()
            .filter(move |d| active.contains(&d.predecessor) && active.contains(&d.successor))
    }
    pub fn removal_preview(&self, id: i64) -> Result<RemovalPreview> {
        let task = self
            .tasks
            .iter()
            .find(|t| t.id == id && !t.archived)
            .ok_or_else(|| invalid("Active task does not exist"))?;
        Ok(RemovalPreview {
            revision: self.revision,
            task: task.clone(),
            dependencies: self
                .dependencies
                .iter()
                .filter(|d| d.predecessor == id || d.successor == id)
                .cloned()
                .collect(),
            related_issue_ids: self
                .tasks
                .iter()
                .filter(|t| t.related_tasks.contains(&id))
                .map(|t| t.id)
                .collect(),
        })
    }
    pub fn topological(&self) -> Result<Vec<i64>> {
        let mut counts: BTreeMap<i64, usize> = self.tasks.iter().map(|t| (t.id, 0)).collect();
        let mut outgoing: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
        let mut edges = BTreeSet::new();
        for d in &self.dependencies {
            if !counts.contains_key(&d.predecessor)
                || !counts.contains_key(&d.successor)
                || d.predecessor == d.successor
                || !edges.insert((d.predecessor, d.successor))
                || d.lag_days > 3660
            {
                return Err(invalid("Invalid or duplicate dependency"));
            }
            *counts.get_mut(&d.successor).unwrap() += 1;
            outgoing.entry(d.predecessor).or_default().push(d.successor);
        }
        let mut ready: BTreeSet<i64> = counts
            .iter()
            .filter(|(_, n)| **n == 0)
            .map(|(&id, _)| id)
            .collect();
        let mut order = vec![];
        while let Some(id) = ready.pop_first() {
            order.push(id);
            for successor in outgoing.get(&id).into_iter().flatten() {
                let count = counts.get_mut(successor).unwrap();
                *count -= 1;
                if *count == 0 {
                    ready.insert(*successor);
                }
            }
        }
        if order.len() != self.tasks.len() {
            return Err(invalid("Dependency cycle detected"));
        }
        Ok(order)
    }
    pub fn is_workday(&self, date: NaiveDate) -> bool {
        date.weekday().num_days_from_monday() < 5
            && !self
                .masters
                .iter()
                .any(|m| m.kind == MasterKind::Holiday && m.date == Some(date))
    }
    pub fn schedule(&mut self) -> Result<()> {
        let order = self.topological()?;
        let calendar = Calendar {
            holidays: self
                .masters
                .iter()
                .filter(|m| m.kind == MasterKind::Holiday)
                .filter_map(|m| m.date)
                .collect(),
        };
        let index: BTreeMap<_, _> = self
            .tasks
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id, i))
            .collect();
        let mut incoming: BTreeMap<i64, Vec<Dependency>> = BTreeMap::new();
        for d in self.active_dependencies() {
            incoming.entry(d.successor).or_default().push(d.clone());
        }
        for id in order {
            let i = index[&id];
            if self.tasks[i].archived {
                continue;
            }
            if matches!(self.tasks[i].status, Status::Done | Status::Cancelled)
                && self.tasks[i].start.is_some()
                && self.tasks[i].due.is_some()
            {
                continue;
            }
            let mut start =
                calendar.workday(self.tasks[i].start.unwrap_or(self.start).max(self.start))?;
            for d in incoming.get(&id).into_iter().flatten() {
                let end = self.tasks[index[&d.predecessor]]
                    .due
                    .ok_or_else(|| invalid("Predecessor is not scheduled"))?;
                let next = end
                    .checked_add_days(Days::new(1))
                    .ok_or_else(|| invalid("Date overflow"))?;
                start = start.max(calendar.advance(next, d.lag_days)?);
            }
            let end = calendar.advance(start, self.tasks[i].duration_days.max(1) - 1)?;
            self.tasks[i].start = Some(start);
            self.tasks[i].due = Some(end);
        }
        Ok(())
    }
    pub fn schedule_preview(&self) -> Result<SchedulePreview> {
        let mut scheduled = self.clone();
        scheduled.schedule()?;
        scheduled.validate()?;
        Ok(SchedulePreview {
            revision: self.revision,
            changes: self
                .tasks
                .iter()
                .zip(&scheduled.tasks)
                .filter(|(old, new)| old.start != new.start || old.due != new.due)
                .map(|(old, new)| ScheduleDifference {
                    id: old.id,
                    title: old.title.clone(),
                    old_start: old.start,
                    old_due: old.due,
                    start: new.start,
                    due: new.due,
                })
                .collect(),
            overdue_ids: scheduled
                .tasks
                .iter()
                .filter(|t| {
                    !t.archived
                        && !matches!(t.status, Status::Done | Status::Cancelled)
                        && t.due.is_some_and(|d| d > self.finish)
                })
                .map(|t| t.id)
                .collect(),
        })
    }
    pub fn snapshot(&self, day: NaiveDate) -> Snapshot {
        let active = self
            .tasks
            .iter()
            .filter(|t| !t.archived && t.include_in_effort && t.status != Status::Cancelled);
        let scope = active.clone().map(|t| t.estimate).sum();
        let completed = active
            .clone()
            .filter(|t| t.status == Status::Done)
            .map(|t| t.estimate)
            .sum();
        let remaining = active.map(Task::remaining).sum();
        Snapshot {
            day,
            scope,
            completed,
            remaining,
        }
    }
    fn capture(&mut self, day: NaiveDate) {
        let value = self.snapshot(day);
        self.history.retain(|s| s.day != day);
        self.history.push(value);
        self.history.sort_by_key(|s| s.day);
    }
    fn change(&mut self, change: &Change) -> Result<()> {
        match change {
            Change::Rename {
                name,
                start,
                finish,
            } => {
                self.name = name.clone();
                self.start = *start;
                self.finish = *finish;
            }
            Change::SaveTask { task } => {
                let mut task = task.clone();
                let previous = self.tasks.iter().find(|t| t.id == task.id);
                if previous.is_some_and(|t| t.status == Status::Done)
                    && !matches!(task.status, Status::Done | Status::Cancelled)
                    && task.progress >= 100
                {
                    return Err(invalid(
                        "Reopening a completed task requires progress below 100%",
                    ));
                }
                if task.status == Status::Done && previous.is_none_or(|t| t.status != Status::Done)
                {
                    let blocked: Vec<_> = self
                        .dependencies
                        .iter()
                        .filter(|d| d.successor == task.id)
                        .filter_map(|d| self.tasks.iter().find(|t| t.id == d.predecessor))
                        .filter(|t| !t.archived && t.status != Status::Done)
                        .map(|t| format!("#{} {}", t.id, t.title))
                        .collect();
                    if !blocked.is_empty() {
                        return Err(invalid(format!(
                            "Complete predecessors first: {}",
                            blocked.join(", ")
                        )));
                    }
                }
                if task.status == Status::Done {
                    task.progress = 100;
                }
                if task.id == 0 {
                    task.id = self.next_task_id;
                    self.next_task_id = next_id(std::iter::once(task.id))?;
                    self.tasks.push(task);
                } else {
                    let old = self
                        .tasks
                        .iter_mut()
                        .find(|t| t.id == task.id)
                        .ok_or_else(|| invalid("Task does not exist"))?;
                    *old = task;
                }
            }
            Change::RemoveTask { id } => {
                self.removal_preview(*id)?;
                self.tasks
                    .iter_mut()
                    .find(|t| t.id == *id)
                    .unwrap()
                    .archived = true;
            }
            Change::RestoreTask { id } => {
                let task = self
                    .tasks
                    .iter_mut()
                    .find(|t| t.id == *id && t.archived)
                    .ok_or_else(|| invalid("Archived task does not exist"))?;
                task.archived = false;
            }
            Change::SaveMaster { master } => {
                let mut master = master.clone();
                if master.id == 0 {
                    master.id = self.next_master_id;
                    self.next_master_id = next_id(std::iter::once(master.id))?;
                    self.masters.push(master);
                } else {
                    let old = self
                        .masters
                        .iter_mut()
                        .find(|m| m.id == master.id)
                        .ok_or_else(|| invalid("Master does not exist"))?;
                    *old = master;
                }
            }
            Change::RemoveMaster { id } => {
                if !self.masters.iter().any(|m| m.id == *id) {
                    return Err(invalid("Master does not exist"));
                }
                self.masters.retain(|m| m.id != *id);
            }
            Change::AddDependency { dependency } => self.dependencies.push(dependency.clone()),
            Change::RemoveDependency {
                predecessor,
                successor,
            } => {
                let count = self.dependencies.len();
                self.dependencies
                    .retain(|d| d.predecessor != *predecessor || d.successor != *successor);
                if count == self.dependencies.len() {
                    return Err(invalid("Dependency does not exist"));
                }
            }
            Change::Schedule { allow_overrun } => {
                let preview = self.schedule_preview()?;
                if !allow_overrun && !preview.overdue_ids.is_empty() {
                    return Err(invalid(format!(
                        "Schedule exceeds project finish for task IDs {:?}; inspect schedule_preview and explicitly allow_overrun",
                        preview.overdue_ids
                    )));
                }
                self.schedule()?;
            }
            Change::Capture { day } => self.capture(*day),
        }
        self.validate()?;
        if !matches!(change, Change::Capture { .. }) {
            self.capture(chrono::Local::now().date_naive());
        }
        Ok(())
    }
    pub fn network(&self) -> Result<BTreeMap<i64, (usize, usize)>> {
        let mut layers: BTreeMap<i64, usize> = BTreeMap::new();
        let mut incoming: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
        for d in self.active_dependencies() {
            incoming.entry(d.successor).or_default().push(d.predecessor);
        }
        let mut slots: BTreeMap<usize, usize> = BTreeMap::new();
        let mut result = BTreeMap::new();
        let active: BTreeSet<_> = self
            .tasks
            .iter()
            .filter(|t| !t.archived)
            .map(|t| t.id)
            .collect();
        for id in self.topological()? {
            if !active.contains(&id) {
                continue;
            }
            let layer = incoming
                .get(&id)
                .into_iter()
                .flatten()
                .map(|p| layers[p] + 1)
                .max()
                .unwrap_or(0);
            let slot = slots.entry(layer).or_default();
            result.insert(id, (layer, *slot));
            *slot += 1;
            layers.insert(id, layer);
        }
        Ok(result)
    }
}
struct Calendar {
    holidays: BTreeSet<NaiveDate>,
}
impl Calendar {
    fn working(&self, d: NaiveDate) -> bool {
        d.weekday().num_days_from_monday() < 5 && !self.holidays.contains(&d)
    }
    fn workday(&self, mut d: NaiveDate) -> Result<NaiveDate> {
        for _ in 0..36610 {
            if self.working(d) {
                return Ok(d);
            }
            d = d
                .checked_add_days(Days::new(1))
                .ok_or_else(|| invalid("Date overflow"))?;
        }
        Err(invalid("No working day found"))
    }
    fn advance(&self, date: NaiveDate, mut days: u32) -> Result<NaiveDate> {
        let mut base = self.workday(date)?;
        while days > 0 {
            if days >= 5 {
                let next = base
                    .checked_add_days(Days::new(u64::from(days / 5) * 7))
                    .ok_or_else(|| invalid("Date overflow"))?;
                let missed = self
                    .holidays
                    .range((
                        std::ops::Bound::Excluded(base),
                        std::ops::Bound::Included(next),
                    ))
                    .filter(|d| d.weekday().num_days_from_monday() < 5)
                    .count() as u32;
                days = days % 5 + missed;
                base = next;
            } else {
                base = self.workday(
                    base.checked_add_days(Days::new(1))
                        .ok_or_else(|| invalid("Date overflow"))?,
                )?;
                days -= 1;
            }
        }
        Ok(base)
    }
}
pub struct Store {
    connection: Connection,
    read_only: bool,
}
impl Store {
    pub fn open(path: &Path, read_only: bool) -> Result<Self> {
        let flags = if read_only {
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
        } else {
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_CREATE
        };
        let c = Connection::open_with_flags(path, flags)?;
        c.busy_timeout(Duration::from_secs(5))?;
        c.pragma_update(None, "foreign_keys", true)?;
        let application: i64 = c.pragma_query_value(None, "application_id", |r| r.get(0))?;
        let version: i64 = c.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if application == 0 && version == 0 && !read_only {
            let tables: i64 = c.query_row(
                "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
                [],
                |r| r.get(0),
            )?;
            if tables != 0 {
                return Err(invalid("This SQLite file is not a yyproject database"));
            }
            c.execute_batch("BEGIN IMMEDIATE; CREATE TABLE projects(id INTEGER PRIMARY KEY,name TEXT NOT NULL,revision INTEGER NOT NULL,document TEXT NOT NULL CHECK(json_valid(document))); CREATE TABLE audit(id INTEGER PRIMARY KEY,project_id INTEGER NOT NULL REFERENCES projects(id),at TEXT NOT NULL,actor TEXT NOT NULL,revision INTEGER NOT NULL,change TEXT NOT NULL CHECK(json_valid(change))); CREATE INDEX audit_project ON audit(project_id,id); PRAGMA application_id=1499025482; PRAGMA user_version=2; COMMIT;")?;
        } else if application != 1499025482 || !matches!(version, 1 | 2) {
            return Err(invalid("Unsupported yyproject database format"));
        }
        if !read_only {
            c.pragma_update(None, "journal_mode", "WAL")?;
        }
        Ok(Self {
            connection: c,
            read_only,
        })
    }
    pub fn list(&self) -> Result<Vec<(i64, String, i64)>> {
        let mut q = self
            .connection
            .prepare("SELECT id,name,revision FROM projects ORDER BY id")?;
        Ok(q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn get(&self, id: i64) -> Result<Project> {
        let row: Option<(i64, String, String)> = self
            .connection
            .query_row(
                "SELECT revision,name,document FROM projects WHERE id=?",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let (revision, name, text) = row.ok_or_else(|| invalid("Project does not exist"))?;
        if text.len() > 16 * 1024 * 1024 {
            return Err(invalid("Project document exceeds 16 MiB"));
        }
        let project: Project = serde_json::from_str(&text)?;
        project.validate()?;
        if project.id != id || project.revision != revision || project.name != name {
            return Err(invalid("Project metadata is inconsistent"));
        }
        Ok(project)
    }
    pub fn create(
        &mut self,
        name: String,
        start: NaiveDate,
        finish: NaiveDate,
        actor: &str,
    ) -> Result<Project> {
        if self.read_only {
            return Err(invalid("Read-only database"));
        }
        if actor.trim().is_empty() || actor.len() > 256 {
            return Err(invalid("Actor is required (up to 256 bytes)"));
        }
        let mut p = Project {
            id: 0,
            revision: 1,
            name,
            start,
            finish,
            tasks: vec![],
            masters: vec![],
            dependencies: vec![],
            history: vec![],
            next_task_id: 1,
            next_master_id: 1,
        };
        p.validate()?;
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO projects(name,revision,document) VALUES(?,1,'{}')",
            [&p.name],
        )?;
        p.id = tx.last_insert_rowid();
        p.capture(chrono::Local::now().date_naive());
        tx.execute(
            "UPDATE projects SET document=? WHERE id=?",
            params![serde_json::to_string(&p)?, p.id],
        )?;
        tx.execute(
            "INSERT INTO audit(project_id,at,actor,revision,change) VALUES(?,?,?,1,?)",
            params![
                p.id,
                Utc::now().to_rfc3339(),
                actor,
                "{\"operation\":\"create\"}"
            ],
        )?;
        tx.pragma_update(None, "user_version", 2)?;
        tx.commit()?;
        Ok(p)
    }
    pub fn apply(
        &mut self,
        id: i64,
        revision: i64,
        change: &Change,
        actor: &str,
    ) -> Result<Project> {
        if self.read_only {
            return Err(invalid("Read-only database"));
        }
        if actor.trim().is_empty() || actor.len() > 256 {
            return Err(invalid("Actor is required (up to 256 bytes)"));
        }
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let text: Option<String> = tx
            .query_row("SELECT document FROM projects WHERE id=?", [id], |r| {
                r.get(0)
            })
            .optional()?;
        let mut p: Project =
            serde_json::from_str(&text.ok_or_else(|| invalid("Project does not exist"))?)?;
        if p.revision != revision {
            return Err(Error::Conflict);
        }
        p.change(change)?;
        p.revision = p
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid("Revision exhausted"))?;
        if serde_json::to_vec(&p)?.len() > 16 * 1024 * 1024 {
            return Err(invalid("Project document exceeds 16 MiB"));
        }
        tx.execute(
            "UPDATE projects SET name=?,revision=?,document=? WHERE id=?",
            params![p.name, p.revision, serde_json::to_string(&p)?, id],
        )?;
        tx.execute(
            "INSERT INTO audit(project_id,at,actor,revision,change) VALUES(?,?,?,?,?)",
            params![
                id,
                Utc::now().to_rfc3339(),
                actor,
                p.revision,
                serde_json::to_string(change)?
            ],
        )?;
        tx.pragma_update(None, "user_version", 2)?;
        tx.commit()?;
        Ok(p)
    }
    pub fn audit(&self, id: i64, after: i64) -> Result<Vec<serde_json::Value>> {
        let mut q=self.connection.prepare("SELECT id,at,actor,revision,change FROM audit WHERE project_id=? AND id>? ORDER BY id LIMIT 100")?;
        let rows = q.query_map(params![id, after], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?;
        let mut result = vec![];
        for row in rows {
            let (id, at, actor, revision, change) = row?;
            result.push(serde_json::json!({"id":id,"at":at,"actor":actor,"revision":revision,"change":serde_json::from_str::<serde_json::Value>(&change)?}));
        }
        Ok(result)
    }
    pub fn data_version(&self) -> Result<i64> {
        Ok(self
            .connection
            .pragma_query_value(None, "data_version", |r| r.get(0))?)
    }
}
