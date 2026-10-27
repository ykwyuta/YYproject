//! Manual cross-team inquiry and temporary workaround tracking. No message delivery.
use crate::*;
use chrono::{DateTime, Utc};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InquiryKind {
    #[default]
    Request,
    Question,
}
impl InquiryKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Request => "依頼",
            Self::Question => "質問",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    #[default]
    Outgoing,
    Incoming,
    Note,
}
impl Direction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Outgoing => "こちらから",
            Self::Incoming => "相手から",
            Self::Note => "メモ",
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Exchange {
    pub id: i64,
    pub at: Option<DateTime<Utc>>,
    pub direction: Direction,
    pub person: String,
    pub text: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Inquiry {
    pub kind: InquiryKind,
    pub target_team: String,
    pub contact: String,
    pub requested_at: Option<DateTime<Utc>>,
    pub response_due_at: Option<DateTime<Utc>>,
    pub answered_at: Option<DateTime<Utc>>,
    pub closed_at: Option<DateTime<Utc>>,
    pub exchanges: Vec<Exchange>,
    pub next_exchange_id: i64,
    pub transitions: Vec<cases::Transition>,
}
impl Default for Inquiry {
    fn default() -> Self {
        Self {
            kind: InquiryKind::Request,
            target_team: String::new(),
            contact: String::new(),
            requested_at: None,
            response_due_at: None,
            answered_at: None,
            closed_at: None,
            exchanges: vec![],
            next_exchange_id: 1,
            transitions: vec![],
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Workaround {
    pub reason: String,
    pub permanent_requirements: String,
    pub action: String,
    pub classification: String,
    pub detected_at: Option<DateTime<Utc>>,
    pub applied_at: Option<DateTime<Utc>>,
    pub permanent_at: Option<DateTime<Utc>>,
    pub cancelled_at: Option<DateTime<Utc>>,
    pub permanent_due_on: Option<NaiveDate>,
    pub transitions: Vec<cases::Transition>,
}
pub fn lifecycle(t: &Task) -> Option<cases::CaseData> {
    if let Some(d) = &t.inquiry {
        Some(cases::CaseData {
            detected_at: d.requested_at,
            closed_at: d.closed_at,
            reported_at: d.answered_at,
            cause: d.target_team.clone(),
            occurrence_phase: d.kind.label().into(),
            detection_phase: d.contact.clone(),
            transitions: d.transitions.clone(),
            ..Default::default()
        })
    } else {
        t.workaround.as_ref().map(|d| cases::CaseData {
            detected_at: d.detected_at,
            closed_at: if t.status == Status::Cancelled {
                d.cancelled_at
            } else {
                d.permanent_at
            },
            temporary_applied_at: d.applied_at,
            cause: d.classification.clone(),
            occurrence_phase: d.classification.clone(),
            transitions: d.transitions.clone(),
            ..Default::default()
        })
    }
}
pub fn opened_at(t: &Task) -> Option<DateTime<Utc>> {
    t.inquiry
        .as_ref()
        .and_then(|d| d.requested_at)
        .or_else(|| t.workaround.as_ref().and_then(|d| d.detected_at))
        .or_else(|| t.case.as_ref().and_then(|d| d.detected_at))
}
pub fn exchange_log(d: &Inquiry) -> String {
    d.exchanges
        .iter()
        .map(|e| {
            format!(
                "#{} {} {} {}\n{}",
                e.id,
                e.at.map(|d| d.to_rfc3339()).unwrap_or_default(),
                e.direction.label(),
                e.person,
                e.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}
pub fn value(t: &Task, key: &str) -> String {
    let (prefix, key) = key.split_once('.').unwrap_or(("", ""));
    if prefix == "inquiry" {
        let Some(d) = &t.inquiry else {
            return String::new();
        };
        match key {
            "kind" => return d.kind.label().into(),
            "exchange_count" => return d.exchanges.len().to_string(),
            "exchange_log" => return exchange_log(d),
            _ => {}
        }
        serde_json::to_value(d)
            .ok()
            .and_then(|v| v.get(key).and_then(|v| v.as_str()).map(str::to_owned))
            .unwrap_or_default()
    } else {
        t.workaround
            .as_ref()
            .and_then(|d| serde_json::to_value(d).ok())
            .and_then(|v| v.get(key).and_then(|v| v.as_str()).map(str::to_owned))
            .unwrap_or_default()
    }
}
pub fn set_value(t: &mut Task, key: &str, text: &str) -> Result<()> {
    let (prefix, key) = key
        .split_once('.')
        .ok_or_else(|| invalid("Invalid management column"))?;
    let data = if prefix == "inquiry" && t.kind == Kind::Inquiry {
        serde_json::to_value(t.inquiry.get_or_insert_with(Box::default))?
    } else if prefix == "workaround" && t.kind == Kind::Workaround {
        serde_json::to_value(t.workaround.get_or_insert_with(Box::default))?
    } else {
        return Err(invalid("Column has incompatible management kind"));
    };
    let value = if prefix == "inquiry" && key == "kind" {
        match text.trim() {
            "" | "依頼" | "request" => "request".into(),
            "質問" | "question" => "question".into(),
            _ => return Err(invalid("依頼／質問を指定してください")),
        }
    } else if key.ends_with("_at") {
        if text.trim().is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::to_value(
                chrono::DateTime::parse_from_rfc3339(text.trim())
                    .map_err(|_| invalid("日時にはタイムゾーン付きRFC3339を指定してください"))?
                    .with_timezone(&Utc),
            )?
        }
    } else if key.ends_with("_on") {
        if text.trim().is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::to_value(
                text.trim()
                    .parse::<NaiveDate>()
                    .map_err(|_| invalid("予定日はYYYY-MM-DDを指定してください"))?,
            )?
        }
    } else {
        text.into()
    };
    let mut data = data;
    data[key] = value;
    if prefix == "inquiry" {
        t.inquiry = Some(serde_json::from_value(data)?);
    } else {
        t.workaround = Some(serde_json::from_value(data)?);
    }
    Ok(())
}
pub fn matches_query(t: &Task, needle: &str) -> bool {
    t.inquiry.as_ref().is_some_and(|d| {
        [d.target_team.as_str(), d.contact.as_str(), d.kind.label()]
            .iter()
            .any(|s| s.to_lowercase().contains(needle))
            || d.exchanges.iter().any(|e| {
                [e.person.as_str(), e.text.as_str()]
                    .iter()
                    .any(|s| s.to_lowercase().contains(needle))
            })
    }) || t.workaround.as_ref().is_some_and(|d| {
        [
            &d.reason,
            &d.action,
            &d.permanent_requirements,
            &d.classification,
        ]
        .iter()
        .any(|s| s.to_lowercase().contains(needle))
    })
}
pub(crate) fn prepare(old: Option<&Task>, t: &mut Task) -> Result<()> {
    if old.is_some_and(|o| (o.inquiry.is_some() || o.workaround.is_some()) && o.kind != t.kind) {
        return Err(invalid("Recorded inquiry/workaround cannot change kind"));
    }
    if !matches!(t.kind, Kind::Inquiry | Kind::Workaround) {
        if t.inquiry.is_some() || t.workaround.is_some() {
            return Err(invalid("Management data has incompatible kind"));
        }
        return Ok(());
    }
    if t.kind == Kind::Inquiry {
        if t.workaround.is_some() {
            return Err(invalid("Inquiry cannot contain workaround data"));
        }
        let d = t.inquiry.get_or_insert_with(Box::default);
        d.exchanges = old
            .and_then(|o| o.inquiry.as_ref())
            .map(|d| d.exchanges.clone())
            .unwrap_or_default();
        d.next_exchange_id = old
            .and_then(|o| o.inquiry.as_ref())
            .map_or(1, |d| d.next_exchange_id);
    } else {
        if t.inquiry.is_some() {
            return Err(invalid("Workaround cannot contain inquiry data"));
        }
        t.workaround.get_or_insert_with(Box::default);
    }
    let previous = old.and_then(lifecycle).map(|d| Task {
        kind: Kind::Bug,
        status: old.unwrap().status,
        case: Some(Box::new(d)),
        ..Default::default()
    });
    let mut record = Task {
        kind: Kind::Bug,
        status: t.status,
        progress: t.progress,
        case: Some(Box::new(lifecycle(t).unwrap())),
        ..Default::default()
    };
    cases::prepare(previous.as_ref(), &mut record)?;
    t.progress = record.progress;
    let data = record.case.unwrap();
    if let Some(d) = t.inquiry.as_mut() {
        d.requested_at = data.detected_at;
        d.closed_at = data.closed_at;
        d.transitions = data.transitions;
    } else if let Some(d) = t.workaround.as_mut() {
        d.detected_at = data.detected_at;
        if t.status == Status::Cancelled {
            d.cancelled_at = data.closed_at;
        } else {
            d.permanent_at = data.closed_at;
            d.cancelled_at = None;
        }
        d.transitions = data.transitions;
    }
    t.include_in_effort = false;
    t.due = t
        .inquiry
        .as_ref()
        .and_then(|d| d.response_due_at.map(|at| at.date_naive()))
        .or_else(|| t.workaround.as_ref().and_then(|d| d.permanent_due_on));
    validate(t)
}
pub(crate) fn validate(t: &Task) -> Result<()> {
    let text = |s: &str, max| -> Result<()> {
        if s.len() > max || s.contains('\0') {
            Err(invalid("Management text limit or NUL character"))
        } else {
            Ok(())
        }
    };
    let common = lifecycle(t);
    if let Some(d) = &t.inquiry {
        if t.kind != Kind::Inquiry || t.workaround.is_some() {
            return Err(invalid("Invalid inquiry kind"));
        }
        text(&d.target_team, 4096)?;
        text(&d.contact, 4096)?;
        if d.target_team.trim().is_empty() {
            return Err(invalid("相手チームを入力してください"));
        }
        if d.next_exchange_id <= 0 || d.exchanges.len() > 5000 {
            return Err(invalid("Invalid exchange count or sequence"));
        }
        let mut ids = BTreeSet::new();
        for entry in &d.exchanges {
            if entry.id <= 0
                || entry.id >= d.next_exchange_id
                || !ids.insert(entry.id)
                || entry.at.is_none()
                || entry.text.trim().is_empty()
            {
                return Err(invalid("Invalid exchange ID, timestamp or empty text"));
            }
            text(&entry.person, 4096)?;
            text(&entry.text, 65536)?;
            if entry.at.is_some_and(|at| {
                !(1..=9999).contains(&at.year()) || d.requested_at.is_some_and(|first| at < first)
            }) {
                return Err(invalid("やりとり日時は依頼日時以降にしてください"));
            }
        }
        if d.response_due_at.is_some_and(|at| {
            !(1..=9999).contains(&at.year()) || d.requested_at.is_some_and(|first| at < first)
        }) {
            return Err(invalid("回答期限は依頼日時以降にしてください"));
        }
    } else if t.kind == Kind::Inquiry {
        return Err(invalid("Inquiry data is required"));
    }
    if let Some(d) = &t.workaround {
        if [d.detected_at, d.applied_at, d.permanent_at, d.cancelled_at]
            .into_iter()
            .flatten()
            .any(|at| {
                !(1..=9999).contains(&at.year()) || d.detected_at.is_some_and(|first| at < first)
            })
        {
            return Err(invalid(
                "Workaround timestamps must be after detection and use years 0001..9999",
            ));
        }
        if t.kind != Kind::Workaround || t.inquiry.is_some() {
            return Err(invalid("Invalid workaround kind"));
        }
        for s in [&d.reason, &d.permanent_requirements, &d.action] {
            text(s, 1024 * 1024)?;
        }
        text(&d.classification, 4096)?;
        if d.reason.trim().is_empty() {
            return Err(invalid("発生理由を入力してください"));
        }
        if d.applied_at.is_some() && d.action.trim().is_empty() {
            return Err(invalid("暫定対応日時には実施内容も必要です"));
        }
        if d.permanent_due_on
            .is_some_and(|day| !(1..=9999).contains(&day.year()))
            || d.applied_at.zip(d.permanent_at).is_some_and(|(a, b)| a > b)
        {
            return Err(invalid("暫定・恒久対応の日程が不正です"));
        }
    } else if t.kind == Kind::Workaround {
        return Err(invalid("Workaround data is required"));
    }
    if let Some(d) = common {
        cases::validate(&d, t.status)?;
    }
    Ok(())
}
pub(crate) fn save_exchange(t: &mut Task, entry: &Exchange) -> Result<()> {
    if t.archived {
        return Err(invalid("保管済みの依頼は復元してから記録してください"));
    }
    let d = t
        .inquiry
        .as_mut()
        .ok_or_else(|| invalid("Exchange requires an inquiry"))?;
    let mut entry = entry.clone();
    entry.at = entry
        .at
        .or_else(|| {
            d.exchanges
                .iter()
                .find(|e| e.id == entry.id)
                .and_then(|e| e.at)
        })
        .or(Some(Utc::now()));
    if entry.id == 0 {
        entry.id = d.next_exchange_id;
        d.next_exchange_id = next_id(std::iter::once(entry.id))?;
        d.exchanges.push(entry);
    } else {
        let id = entry.id;
        *d.exchanges
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or_else(|| invalid("Exchange does not exist"))? = entry;
    }
    d.exchanges.sort_by_key(|e| (e.at, e.id));
    validate(t)
}

#[derive(Debug, Serialize)]
pub struct Statistics {
    #[serde(flatten)]
    pub convergence: cases::Statistics,
    pub classifications: BTreeMap<String, usize>,
    pub states: BTreeMap<String, usize>,
    pub overdue: usize,
    pub unanswered: usize,
    pub not_applied: usize,
    pub outgoing: usize,
    pub incoming: usize,
    pub notes: usize,
    pub mean_answer_hours: Option<f64>,
    pub mean_temporary_hours: Option<f64>,
}
pub fn statistics(
    p: &Project,
    kind: Kind,
    from: NaiveDate,
    to: NaiveDate,
    filter: &TaskFilter,
) -> Result<Statistics> {
    if !matches!(kind, Kind::Inquiry | Kind::Workaround) {
        return Err(invalid(
            "Management statistics require inquiry or workaround kind",
        ));
    }
    filter.validate()?;
    let mut adapted = p.clone();
    adapted.tasks = p
        .tasks
        .iter()
        .filter(|t| t.kind == kind && p.matches_filter(filter, t))
        .cloned()
        .collect();
    let mut result = Statistics {
        convergence: cases::Statistics {
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
        },
        classifications: BTreeMap::new(),
        states: BTreeMap::new(),
        overdue: 0,
        unanswered: 0,
        not_applied: 0,
        outgoing: 0,
        incoming: 0,
        notes: 0,
        mean_answer_hours: None,
        mean_temporary_hours: None,
    };
    let mut answer = vec![];
    let mut temporary = vec![];
    for t in &mut adapted.tasks {
        let d = lifecycle(t).ok_or_else(|| invalid("Missing management lifecycle"))?;
        let visible = d.detected_at.is_some_and(|d| d.date_naive() <= to);
        if visible {
            let status = d
                .transitions
                .iter()
                .rev()
                .find(|e| e.at.date_naive() <= to)
                .map_or(Status::Todo, |e| e.status);
            *result.states.entry(status.label().into()).or_default() += 1;
            let open = !matches!(status, Status::Done | Status::Cancelled);
            if let Some(i) = &t.inquiry {
                *result
                    .classifications
                    .entry(i.kind.label().into())
                    .or_default() += 1;
                if i.answered_at.is_none_or(|at| at.date_naive() > to) {
                    result.unanswered += usize::from(open);
                }
                if open && i.response_due_at.is_some_and(|at| at.date_naive() < to) {
                    result.overdue += 1;
                }
                if let Some((a, b)) = i
                    .requested_at
                    .zip(i.answered_at)
                    .filter(|(_, b)| b.date_naive() <= to)
                {
                    answer.push((b - a).num_seconds() as f64 / 3600.);
                }
                for e in i.exchanges.iter().filter(|e| {
                    e.at.is_some_and(|at| at.date_naive() >= from && at.date_naive() <= to)
                }) {
                    match e.direction {
                        Direction::Outgoing => result.outgoing += 1,
                        Direction::Incoming => result.incoming += 1,
                        Direction::Note => result.notes += 1,
                    }
                }
            }
            if let Some(w) = &t.workaround {
                *result
                    .classifications
                    .entry(if w.classification.is_empty() {
                        "未分類".into()
                    } else {
                        w.classification.clone()
                    })
                    .or_default() += 1;
                if w.applied_at.is_none_or(|at| at.date_naive() > to) {
                    result.not_applied += usize::from(open);
                }
                if open && w.permanent_due_on.is_some_and(|day| day < to) {
                    result.overdue += 1;
                }
                if let Some((a, b)) = w
                    .detected_at
                    .zip(w.applied_at)
                    .filter(|(_, b)| b.date_naive() <= to)
                {
                    temporary.push((b - a).num_seconds() as f64 / 3600.);
                }
            }
        }
        t.kind = Kind::Bug;
        t.case = Some(Box::new(d));
        t.inquiry = None;
        t.workaround = None;
    }
    result.convergence = cases::statistics(
        &adapted,
        Kind::Bug,
        from,
        to,
        &TaskFilter {
            archived: filter.archived,
            ..Default::default()
        },
    )?;
    result.convergence.kind = kind;
    if !answer.is_empty() {
        result.mean_answer_hours = Some(answer.iter().sum::<f64>() / answer.len() as f64);
    }
    if !temporary.is_empty() {
        result.mean_temporary_hours = Some(temporary.iter().sum::<f64>() / temporary.len() as f64);
    }
    Ok(result)
}
