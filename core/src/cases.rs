//! Bug/issue investigation data and count-based convergence analytics.
use crate::*;
use chrono::{DateTime, Utc};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Difficulty {
    #[default]
    Unspecified,
    Low,
    Medium,
    High,
    VeryHigh,
}
impl Difficulty {
    pub const ALL: [Self; 5] = [
        Self::Unspecified,
        Self::Low,
        Self::Medium,
        Self::High,
        Self::VeryHigh,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Unspecified => "未指定",
            Self::Low => "低",
            Self::Medium => "中",
            Self::High => "高",
            Self::VeryHigh => "非常に高",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transition {
    pub at: DateTime<Utc>,
    pub status: Status,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CaseData {
    pub occurrence_phase: String,
    pub cause: String,
    pub detection_phase: String,
    pub temporary_plan: String,
    pub permanent_plan: String,
    pub temporary_difficulty: Difficulty,
    pub permanent_difficulty: Difficulty,
    pub detection_difficulty: Difficulty,
    pub occurred_at: Option<DateTime<Utc>>,
    pub detected_at: Option<DateTime<Utc>>,
    pub reported_at: Option<DateTime<Utc>>,
    pub triaged_at: Option<DateTime<Utc>>,
    pub temporary_due_at: Option<DateTime<Utc>>,
    pub temporary_applied_at: Option<DateTime<Utc>>,
    pub permanent_due_at: Option<DateTime<Utc>>,
    pub permanent_applied_at: Option<DateTime<Utc>>,
    pub verified_at: Option<DateTime<Utc>>,
    pub closed_at: Option<DateTime<Utc>>,
    /// Server-maintained status history. Replacement writes cannot erase it.
    pub transitions: Vec<Transition>,
}
fn terminal(status: Status) -> bool {
    matches!(status, Status::Done | Status::Cancelled)
}
pub(crate) fn prepare(old: Option<&Task>, task: &mut Task) -> Result<()> {
    if old.is_some_and(|t| t.case.is_some() && t.kind != task.kind) {
        return Err(invalid(
            "Recorded bugs/issues cannot change kind; create a linked record instead",
        ));
    }
    if !matches!(task.kind, Kind::Bug | Kind::Issue) {
        if task.case.is_some() {
            return Err(invalid("Investigation data requires bug or issue kind"));
        }
        return Ok(());
    }
    if !terminal(task.status) && old.is_some_and(|t| terminal(t.status)) && task.progress >= 100 {
        task.progress = 0;
    }
    let data = task.case.get_or_insert_with(Box::default);
    if !terminal(task.status) && data.closed_at.is_some() && old.is_none_or(|t| !terminal(t.status))
    {
        return Err(invalid(
            "完了・中止日時は完了／中止の項目に指定してください",
        ));
    }
    let now = Utc::now();
    let previous = old.and_then(|t| t.case.as_ref());
    data.transitions = previous.map(|d| d.transitions.clone()).unwrap_or_default();
    if data.transitions.is_empty() {
        // Legacy undated issues start their recorded history here; no invented dates.
        data.detected_at = data.detected_at.or(Some(now));
        data.transitions.push(Transition {
            at: data.detected_at.unwrap(),
            status: Status::Todo,
        });
    } else if data.detected_at != previous.and_then(|d| d.detected_at) {
        // The detection date may be corrected, without rewriting later status events.
        if let Some(at) = data.detected_at {
            data.transitions[0].at = at;
        } else {
            return Err(invalid(
                "Detection timestamp cannot be cleared after recording history",
            ));
        }
    }
    if terminal(task.status) {
        data.closed_at = data.closed_at.or(Some(now));
    } else {
        data.closed_at = None;
    }
    if data
        .transitions
        .last()
        .is_none_or(|e| e.status != task.status)
    {
        let at = if terminal(task.status) {
            data.closed_at.unwrap()
        } else {
            now
        };
        data.transitions.push(Transition {
            at,
            status: task.status,
        });
    } else if terminal(task.status)
        && let Some(event) = data.transitions.last_mut()
    {
        event.at = data.closed_at.unwrap();
    }
    validate(data, task.status)
}
pub(crate) fn validate(d: &CaseData, status: Status) -> Result<()> {
    if !terminal(status) && d.closed_at.is_some() {
        return Err(invalid(
            "Open investigation cannot retain a closure timestamp",
        ));
    }
    if d.transitions.is_empty() || d.detected_at.is_none() {
        return Err(invalid(
            "Recorded investigation requires detection timestamp and status history",
        ));
    }
    if [&d.occurrence_phase, &d.cause, &d.detection_phase]
        .iter()
        .any(|s| s.len() > 4096 || s.chars().any(char::is_control))
        || [&d.temporary_plan, &d.permanent_plan]
            .iter()
            .any(|s| s.len() > 1024 * 1024)
        || d.transitions.len() > 20000
    {
        return Err(invalid("Investigation text or history limit exceeded"));
    }
    let dates = [
        d.occurred_at,
        d.detected_at,
        d.reported_at,
        d.triaged_at,
        d.temporary_due_at,
        d.temporary_applied_at,
        d.permanent_due_at,
        d.permanent_applied_at,
        d.verified_at,
        d.closed_at,
    ];
    if dates
        .into_iter()
        .flatten()
        .any(|d| !(1..=9999).contains(&d.year()))
    {
        return Err(invalid("Investigation dates must use years 0001..9999"));
    }
    if d.occurred_at.zip(d.detected_at).is_some_and(|(a, b)| a > b)
        || [
            d.reported_at,
            d.triaged_at,
            d.temporary_applied_at,
            d.permanent_applied_at,
            d.verified_at,
            d.closed_at,
        ]
        .into_iter()
        .flatten()
        .any(|v| d.detected_at.is_some_and(|first| v < first))
        || d.transitions.windows(2).any(|w| w[0].at > w[1].at)
        || d.transitions
            .iter()
            .any(|e| !(1..=9999).contains(&e.at.year()))
        || d.transitions.last().is_some_and(|e| e.status != status)
        || (!d.transitions.is_empty()
            && (d.detected_at != d.transitions.first().map(|e| e.at)
                || (terminal(status) && d.closed_at != d.transitions.last().map(|e| e.at))))
    {
        return Err(invalid(
            "Investigation timestamps or status history are inconsistent",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize)]
pub struct DailyCount {
    pub day: NaiveDate,
    pub detected: usize,
    pub resolved: usize,
    pub cancelled: usize,
    pub open: usize,
}
#[derive(Clone, Debug, Serialize)]
pub struct Statistics {
    pub kind: Kind,
    pub daily: Vec<DailyCount>,
    pub causes: BTreeMap<String, usize>,
    pub occurrence_phases: BTreeMap<String, usize>,
    pub detection_phases: BTreeMap<String, usize>,
    pub temporary_difficulties: BTreeMap<String, usize>,
    pub permanent_difficulties: BTreeMap<String, usize>,
    pub detection_difficulties: BTreeMap<String, usize>,
    pub total: usize,
    pub open: usize,
    pub resolved: usize,
    pub cancelled: usize,
    pub undated: usize,
    pub mean_resolution_hours: Option<f64>,
}
pub fn statistics(
    p: &Project,
    kind: Kind,
    from: NaiveDate,
    to: NaiveDate,
    filter: &TaskFilter,
) -> Result<Statistics> {
    filter.validate()?;
    if !matches!(kind, Kind::Bug | Kind::Issue)
        || from > to
        || (to - from).num_days() > 3660
        || from.year() < 1
        || to.year() > 9999
    {
        return Err(invalid(
            "Statistics require bug/issue and a range of at most ten years",
        ));
    }
    let mut result = Statistics {
        kind,
        daily: vec![],
        causes: BTreeMap::new(),
        occurrence_phases: BTreeMap::new(),
        detection_phases: BTreeMap::new(),
        temporary_difficulties: BTreeMap::new(),
        permanent_difficulties: BTreeMap::new(),
        detection_difficulties: BTreeMap::new(),
        total: 0,
        open: 0,
        resolved: 0,
        cancelled: 0,
        undated: 0,
        mean_resolution_hours: None,
    };
    let tasks: Vec<_> = p
        .tasks
        .iter()
        .filter(|t| t.kind == kind && p.matches_filter(filter, t))
        .collect();
    let mut durations = vec![];
    let mut delta: BTreeMap<NaiveDate, [i64; 4]> = BTreeMap::new();
    for t in tasks {
        let Some(d) = t.case.as_ref().filter(|d| !d.transitions.is_empty()) else {
            result.undated += 1;
            continue;
        };
        if d.detected_at.is_some_and(|at| at.date_naive() > to) {
            continue;
        }
        result.total += 1;
        let end_status = d
            .transitions
            .iter()
            .rev()
            .find(|e| e.at.date_naive() <= to)
            .map_or(Status::Todo, |e| e.status);
        match end_status {
            Status::Done => result.resolved += 1,
            Status::Cancelled => result.cancelled += 1,
            _ => result.open += 1,
        }
        for (map, key) in [
            (&mut result.causes, d.cause.as_str()),
            (&mut result.occurrence_phases, d.occurrence_phase.as_str()),
            (&mut result.detection_phases, d.detection_phase.as_str()),
            (
                &mut result.temporary_difficulties,
                d.temporary_difficulty.label(),
            ),
            (
                &mut result.permanent_difficulties,
                d.permanent_difficulty.label(),
            ),
            (
                &mut result.detection_difficulties,
                d.detection_difficulty.label(),
            ),
        ] {
            *map.entry(
                if key.trim().is_empty() {
                    "未分類"
                } else {
                    key
                }
                .to_owned(),
            )
            .or_default() += 1;
        }
        if end_status == Status::Done
            && let Some(end) = d.transitions.iter().rev().find(|e| e.at.date_naive() <= to)
        {
            durations.push((end.at - d.transitions[0].at).num_seconds() as f64 / 3600.);
        }
        let first = d.transitions[0].at.date_naive();
        delta.entry(first).or_default()[0] += 1;
        let mut previous = None;
        for event in &d.transitions {
            let category = match event.status {
                Status::Done => 1,
                Status::Cancelled => 2,
                _ => 3,
            };
            let change = delta.entry(event.at.date_naive()).or_default();
            if let Some(old) = previous {
                change[old] -= 1;
            }
            change[category] += 1;
            previous = Some(category);
        }
    }
    let mut counts = [0i64; 4];
    for (_, change) in delta.range(..from) {
        for i in 0..4 {
            counts[i] += change[i];
        }
    }
    for n in 0..=(to - from).num_days() {
        let day = from + Days::new(n as u64);
        if let Some(change) = delta.get(&day) {
            for i in 0..4 {
                counts[i] += change[i];
            }
        }
        result.daily.push(DailyCount {
            day,
            detected: counts[0] as usize,
            resolved: counts[1] as usize,
            cancelled: counts[2] as usize,
            open: counts[3] as usize,
        });
    }
    if !durations.is_empty() {
        result.mean_resolution_hours = Some(durations.iter().sum::<f64>() / durations.len() as f64);
    }
    Ok(result)
}
