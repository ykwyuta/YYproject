//! Shared task grid, quoted TSV clipboard payloads and RFC 4180 CSV export.
use crate::*;

/// View-only folding. A filtered-out ancestor never hides a matching descendant.
pub fn outline_rows<'a>(
    project: &'a Project,
    filter: &TaskFilter,
    kind: Option<Kind>,
    collapsed: &BTreeSet<i64>,
) -> Vec<(&'a Task, usize)> {
    let ordered = project.ordered_tasks();
    let included: BTreeSet<_> = ordered
        .iter()
        .filter(|(t, _)| kind.is_none_or(|k| t.kind == k) && project.matches_filter(filter, t))
        .map(|(t, _)| t.id)
        .collect();
    let mut hidden = BTreeSet::new();
    ordered
        .into_iter()
        .filter(|(t, _)| {
            if t.parent.is_some_and(|id| {
                hidden.contains(&id) || (included.contains(&id) && collapsed.contains(&id))
            }) {
                hidden.insert(t.id);
            }
            included.contains(&t.id) && !hidden.contains(&t.id)
        })
        .collect()
}

#[derive(Clone, Copy, Serialize)]
pub struct Column {
    pub key: &'static str,
    pub label: &'static str,
    pub width: i32,
    pub editable: bool,
}
pub const MASTER_COLUMNS: &[Column] = &[
    Column {
        key: "id",
        label: "ID",
        width: 70,
        editable: false,
    },
    Column {
        key: "kind",
        label: "種類",
        width: 120,
        editable: true,
    },
    Column {
        key: "name",
        label: "名前",
        width: 300,
        editable: true,
    },
    Column {
        key: "parent",
        label: "親カテゴリID",
        width: 160,
        editable: true,
    },
    Column {
        key: "date",
        label: "休日の日付",
        width: 160,
        editable: true,
    },
];
pub fn master_columns(kind: MasterKind) -> &'static [Column] {
    const CATEGORY: &[Column] = &[MASTER_COLUMNS[0], MASTER_COLUMNS[2], MASTER_COLUMNS[3]];
    const ASSIGNEE: &[Column] = &[MASTER_COLUMNS[0], MASTER_COLUMNS[2]];
    const HOLIDAY: &[Column] = &[MASTER_COLUMNS[0], MASTER_COLUMNS[2], MASTER_COLUMNS[4]];
    match kind {
        MasterKind::Category => CATEGORY,
        MasterKind::Assignee => ASSIGNEE,
        MasterKind::Holiday => HOLIDAY,
    }
}
pub const BULK_TASK_KEYS: &[&str] = &[
    "inquiry.kind",
    "inquiry.target_team",
    "inquiry.contact",
    "inquiry.response_due_at",
    "inquiry.answered_at",
    "inquiry.closed_at",
    "workaround.classification",
    "workaround.applied_at",
    "workaround.permanent_at",
    "workaround.permanent_due_on",
    "case.closed_at",
    "case.temporary_due_at",
    "case.permanent_due_at",
    "case.occurrence_phase",
    "case.cause",
    "case.detection_phase",
    "case.temporary_difficulty",
    "case.permanent_difficulty",
    "case.detection_difficulty",
    "assignee",
    "category",
    "status",
    "priority",
    "start",
    "due",
    "completed_on",
    "progress",
];
pub fn validate_bulk_keys<'a>(keys: impl IntoIterator<Item = &'a str>) -> Result<()> {
    for key in keys {
        if !BULK_TASK_KEYS.contains(&key) {
            return Err(invalid(format!(
                "{}は個別編集専用です。一括変更できません",
                COLUMNS
                    .iter()
                    .find(|c| c.key == key)
                    .map_or(key, |c| c.label)
            )));
        }
    }
    Ok(())
}
pub fn fill_tasks_down(p: &Project, ids: &[i64], keys: &[String]) -> Result<Change> {
    if ids.len() < 2 {
        return Err(invalid("コピー元を先頭に含む2行以上を選択してください"));
    }
    validate_bulk_keys(keys.iter().map(String::as_str))?;
    let source = p
        .tasks
        .iter()
        .find(|t| t.id == ids[0])
        .ok_or_else(|| invalid("Copy source no longer exists"))?;
    let values = keys
        .iter()
        .map(|key| {
            (
                key.clone(),
                match key.as_str() {
                    "assignee" => source.assignee.map(|id| id.to_string()).unwrap_or_default(),
                    "category" => source.category.map(|id| id.to_string()).unwrap_or_default(),
                    _ => value(p, source, key),
                },
            )
        })
        .collect::<Vec<_>>();
    batch_task_attributes(p, &ids[1..], &values)
}
pub fn master_value(m: &Master, key: &str) -> String {
    match key {
        "id" => m.id.to_string(),
        "kind" => match m.kind {
            MasterKind::Category => "カテゴリ",
            MasterKind::Assignee => "担当者",
            MasterKind::Holiday => "休日",
        }
        .into(),
        "name" => m.name.clone(),
        "parent" => m.parent.map(|id| id.to_string()).unwrap_or_default(),
        "date" => m.date.map(|d| d.to_string()).unwrap_or_default(),
        _ => String::new(),
    }
}
pub fn set_master_value(m: &mut Master, key: &str, value: &str) -> Result<()> {
    match key {
        "name" => m.name = value.into(),
        "kind" => {
            m.kind = match value.trim() {
                "カテゴリ" | "category" => MasterKind::Category,
                "担当者" | "assignee" => MasterKind::Assignee,
                "休日" | "holiday" => MasterKind::Holiday,
                _ => return Err(invalid("種類はカテゴリ・担当者・休日を指定してください")),
            }
        }
        "parent" => {
            m.parent = if value.trim().is_empty() {
                None
            } else {
                Some(
                    value
                        .trim()
                        .parse()
                        .map_err(|_| invalid("親カテゴリIDは整数を指定してください"))?,
                )
            }
        }
        "date" => {
            m.date = if value.trim().is_empty() {
                None
            } else {
                Some(
                    value
                        .trim()
                        .parse()
                        .map_err(|_| invalid("日付はYYYY-MM-DDを指定してください"))?,
                )
            }
        }
        _ => return Err(invalid("読み取り専用のマスタ列です")),
    }
    Ok(())
}
macro_rules! columns { ($($key:literal,$label:literal,$width:literal,$edit:literal);* $(;)?) => { pub const COLUMNS: &[Column] = &[$(Column{key:$key,label:$label,width:$width,editable:$edit}),*]; }; }
columns! {
    "id","ID",60,false;
    "category","カテゴリ",210,true;
    "title","件名",340,true;
    "status","状態",100,true;
    "priority","優先度",90,true;
    "severity","重大度",110,true;
    "review","レビュー",110,true;
    "reviewer","確認者",130,true;
    "assignee","担当者",130,true;
    "start","開始日",120,true;
    "due","期限",120,true;
    "progress","進捗(%)",90,true;
    "estimate","工数(h)",100,true;
    "include_in_effort","工数集計",110,true;
    "related_tasks","対応タスクID",150,true;
    "parent","親タスクID",120,true;
    "milestone","マイルストーン",150,true;
    "description","説明",320,true;
    "review_note","確認メモ",260,true;
    "duration_days","期間(営業日)",140,true;
    "kind","種類",100,true;
    "category_1","カテゴリ1",160,false;
    "category_2","カテゴリ2",160,false;
    "category_3","カテゴリ3",160,false;
    "completed_on","完了日",120,true;
}
pub const CASE_COLUMNS: &[Column] = &[
    COLUMNS[0],
    COLUMNS[1],
    COLUMNS[2],
    COLUMNS[3],
    COLUMNS[8],
    COLUMNS[4],
    COLUMNS[5],
    Column {
        key: "case.occurrence_phase",
        label: "発生工程",
        width: 160,
        editable: true,
    },
    Column {
        key: "case.cause",
        label: "原因分類",
        width: 180,
        editable: true,
    },
    Column {
        key: "case.detection_phase",
        label: "検出工程",
        width: 160,
        editable: true,
    },
    Column {
        key: "case.detected_at",
        label: "検出日時",
        width: 240,
        editable: true,
    },
    Column {
        key: "case.occurred_at",
        label: "発生日時",
        width: 240,
        editable: true,
    },
    Column {
        key: "case.reported_at",
        label: "報告日時",
        width: 240,
        editable: true,
    },
    Column {
        key: "case.triaged_at",
        label: "分析日時",
        width: 240,
        editable: true,
    },
    Column {
        key: "case.temporary_plan",
        label: "暫定対策案",
        width: 360,
        editable: true,
    },
    Column {
        key: "case.permanent_plan",
        label: "恒久対策案・案件",
        width: 360,
        editable: true,
    },
    Column {
        key: "case.temporary_difficulty",
        label: "暫定対策難易度",
        width: 180,
        editable: true,
    },
    Column {
        key: "case.permanent_difficulty",
        label: "恒久対策難易度",
        width: 180,
        editable: true,
    },
    Column {
        key: "case.detection_difficulty",
        label: "検知難易度",
        width: 160,
        editable: true,
    },
    Column {
        key: "case.temporary_due_at",
        label: "暫定対策予定日時",
        width: 240,
        editable: true,
    },
    Column {
        key: "case.temporary_applied_at",
        label: "暫定対策実施日時",
        width: 240,
        editable: true,
    },
    Column {
        key: "case.permanent_due_at",
        label: "恒久対策予定日時",
        width: 240,
        editable: true,
    },
    Column {
        key: "case.permanent_applied_at",
        label: "恒久対策実施日時",
        width: 240,
        editable: true,
    },
    Column {
        key: "case.verified_at",
        label: "確認日時",
        width: 240,
        editable: true,
    },
    Column {
        key: "case.closed_at",
        label: "完了・中止日時",
        width: 240,
        editable: true,
    },
    COLUMNS[10],
    COLUMNS[14],
    COLUMNS[17],
    COLUMNS[6],
    COLUMNS[7],
    COLUMNS[18],
];
pub const INQUIRY_COLUMNS: &[Column] = &[
    COLUMNS[0],
    COLUMNS[1],
    COLUMNS[2],
    COLUMNS[3],
    COLUMNS[8],
    COLUMNS[4],
    Column {
        key: "inquiry.kind",
        label: "依頼／質問",
        width: 110,
        editable: true,
    },
    Column {
        key: "inquiry.target_team",
        label: "相手チーム",
        width: 190,
        editable: true,
    },
    Column {
        key: "inquiry.contact",
        label: "相手担当者",
        width: 180,
        editable: true,
    },
    Column {
        key: "inquiry.requested_at",
        label: "依頼・質問日時",
        width: 240,
        editable: true,
    },
    Column {
        key: "inquiry.response_due_at",
        label: "回答期限",
        width: 240,
        editable: true,
    },
    Column {
        key: "inquiry.answered_at",
        label: "回答日時",
        width: 240,
        editable: true,
    },
    Column {
        key: "inquiry.closed_at",
        label: "完了・中止日時",
        width: 240,
        editable: true,
    },
    Column {
        key: "inquiry.exchange_count",
        label: "記録件数",
        width: 100,
        editable: false,
    },
    Column {
        key: "inquiry.exchange_log",
        label: "やりとり記録",
        width: 460,
        editable: false,
    },
    COLUMNS[14],
    COLUMNS[17],
];
pub const WORKAROUND_COLUMNS: &[Column] = &[
    COLUMNS[0],
    COLUMNS[1],
    COLUMNS[2],
    COLUMNS[3],
    COLUMNS[8],
    COLUMNS[4],
    Column {
        key: "workaround.classification",
        label: "暫定対応の分類",
        width: 190,
        editable: true,
    },
    Column {
        key: "workaround.reason",
        label: "発生理由",
        width: 320,
        editable: true,
    },
    Column {
        key: "workaround.permanent_requirements",
        label: "恒久対応のために必要なもの",
        width: 360,
        editable: true,
    },
    Column {
        key: "workaround.action",
        label: "実施した暫定対応内容",
        width: 360,
        editable: true,
    },
    Column {
        key: "workaround.detected_at",
        label: "検知日時",
        width: 240,
        editable: true,
    },
    Column {
        key: "workaround.applied_at",
        label: "暫定対応日時",
        width: 240,
        editable: true,
    },
    Column {
        key: "workaround.permanent_at",
        label: "恒久対応日時",
        width: 240,
        editable: true,
    },
    Column {
        key: "workaround.permanent_due_on",
        label: "恒久対応の実施予定日",
        width: 240,
        editable: true,
    },
    COLUMNS[14],
    COLUMNS[17],
];
pub fn all_columns() -> impl Iterator<Item = &'static Column> {
    COLUMNS
        .iter()
        .chain(CASE_COLUMNS)
        .chain(INQUIRY_COLUMNS)
        .chain(WORKAROUND_COLUMNS)
}
pub fn value(p: &Project, t: &Task, key: &str) -> String {
    if key.starts_with("inquiry.") || key.starts_with("workaround.") {
        return crate::work::value(t, key);
    }
    if let Some(key) = key.strip_prefix("case.") {
        let Some(d) = &t.case else {
            return String::new();
        };
        return match key {
            "temporary_difficulty" => d.temporary_difficulty.label().into(),
            "permanent_difficulty" => d.permanent_difficulty.label().into(),
            "detection_difficulty" => d.detection_difficulty.label().into(),
            _ => serde_json::to_value(d)
                .ok()
                .and_then(|v| v.get(key).and_then(|v| v.as_str()).map(str::to_owned))
                .unwrap_or_default(),
        };
    }
    let master = |id| {
        p.masters
            .iter()
            .find(|m| Some(m.id) == id)
            .map(|m| m.name.clone())
            .unwrap_or_default()
    };
    match key {
        "category" => p.category_path(t.category).join(" / "),
        "category_1" | "category_2" | "category_3" => p
            .category_path(t.category)
            .get(key.as_bytes()[9] as usize - b'1' as usize)
            .cloned()
            .unwrap_or_default(),
        "assignee" => master(t.assignee),
        "reviewer" => master(t.reviewer),
        "status" => t.status.label().into(),
        "priority" => t.priority.label().into(),
        "severity" => t.severity.label().into(),
        "review" => t.review.label().into(),
        "kind" => match t.kind {
            Kind::Task => "タスク",
            Kind::Issue => "課題",
            Kind::Bug => "バグ",
            Kind::Inquiry => "依頼・質問",
            Kind::Workaround => "暫定対応",
        }
        .into(),
        "include_in_effort" => if t.include_in_effort {
            "算入"
        } else {
            "除外"
        }
        .into(),
        "milestone" => if t.milestone { "はい" } else { "いいえ" }.into(),
        "related_tasks" => t
            .related_tasks
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(", "),
        _ => match serde_json::to_value(t)
            .ok()
            .and_then(|v| v.get(key).cloned())
        {
            Some(serde_json::Value::Null) | None => String::new(),
            Some(serde_json::Value::String(s)) => s,
            Some(v) => v.to_string(),
        },
    }
}
fn enum_value<T: Copy + Serialize>(
    values: &[T],
    label: impl Fn(T) -> &'static str,
    text: &str,
) -> Result<serde_json::Value> {
    values
        .iter()
        .find(|v| {
            label(**v) == text
                || serde_json::to_value(**v)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .as_deref()
                    == Some(text)
        })
        .map(|v| serde_json::to_value(v).unwrap())
        .ok_or_else(|| invalid(format!("Unknown choice: {text}")))
}
pub fn set_value(p: &Project, t: &mut Task, key: &str, text: &str) -> Result<()> {
    let column = all_columns()
        .find(|c| c.key == key)
        .ok_or_else(|| invalid("Unknown column"))?;
    if !column.editable {
        return Err(invalid("Read-only column"));
    }
    if key.starts_with("inquiry.") || key.starts_with("workaround.") {
        return crate::work::set_value(t, key, text);
    }
    if let Some(key) = key.strip_prefix("case.") {
        if t.kind == Kind::Task {
            return Err(invalid("Investigation fields require bug or issue"));
        }
        let data = t.case.get_or_insert_with(Box::default);
        let value = if key.ends_with("_difficulty") {
            enum_value(
                &cases::Difficulty::ALL,
                cases::Difficulty::label,
                if text.is_empty() { "unspecified" } else { text },
            )?
        } else if key.ends_with("_at") {
            if text.trim().is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::to_value(
                    chrono::DateTime::parse_from_rfc3339(text.trim())
                        .map_err(|_| {
                            invalid("日時は2026-10-27T09:00:00+09:00の形式で指定してください")
                        })?
                        .with_timezone(&Utc),
                )?
            }
        } else {
            text.into()
        };
        let mut object = serde_json::to_value(&*data)?;
        object[key] = value;
        *data = serde_json::from_value(object)?;
        return Ok(());
    }
    let text = if matches!(key, "description" | "review_note" | "title") {
        text
    } else {
        text.trim()
    };
    let reference = |kind| -> Result<serde_json::Value> {
        if text.is_empty() {
            return Ok(serde_json::Value::Null);
        }
        if let Ok(id) = text.trim_start_matches('#').parse::<i64>() {
            return Ok(id.into());
        }
        let matches: Vec<_> = p
            .masters
            .iter()
            .filter(|m| {
                m.kind == kind
                    && (m.name == text
                        || (kind == MasterKind::Category
                            && p.category_path(Some(m.id)).join(" / ") == text))
            })
            .collect();
        if matches.len() != 1 {
            return Err(invalid(
                "Master name is missing or ambiguous; use its numeric ID or category path",
            ));
        }
        Ok(matches[0].id.into())
    };
    let boolean = || -> Result<serde_json::Value> {
        match text {
            "true" | "1" | "はい" | "算入" => Ok(true.into()),
            "false" | "0" | "いいえ" | "除外" => Ok(false.into()),
            _ => Err(invalid("Use true/false or はい/いいえ")),
        }
    };
    let v = match key {
        "status" => enum_value(&Status::ALL, Status::label, text)?,
        "priority" => enum_value(&Priority::ALL, Priority::label, text)?,
        "severity" => enum_value(&Severity::ALL, Severity::label, text)?,
        "review" => enum_value(&ReviewState::ALL, ReviewState::label, text)?,
        "kind" => match text {
            "タスク" | "task" => "task".into(),
            "課題" | "issue" => "issue".into(),
            "バグ" | "bug" => "bug".into(),
            "依頼・質問" | "inquiry" => "inquiry".into(),
            "暫定対応" | "workaround" => "workaround".into(),
            _ => return Err(invalid("Unknown task kind")),
        },
        "category" => reference(MasterKind::Category)?,
        "assignee" | "reviewer" => reference(MasterKind::Assignee)?,
        "milestone" | "include_in_effort" => boolean()?,
        "parent" => {
            if text.is_empty() {
                serde_json::Value::Null
            } else {
                text.trim_start_matches('#')
                    .parse::<i64>()
                    .map_err(|_| invalid("Invalid parent ID"))?
                    .into()
            }
        }
        "start" | "due" | "completed_on" => {
            if text.is_empty() {
                serde_json::Value::Null
            } else {
                text.parse::<NaiveDate>()
                    .map_err(|_| invalid("Use YYYY-MM-DD"))?;
                text.into()
            }
        }
        "related_tasks" => serde_json::to_value(if text.is_empty() {
            vec![]
        } else {
            text.split(',')
                .map(|s| {
                    s.trim()
                        .parse::<i64>()
                        .map_err(|_| invalid("Invalid related task ID"))
                })
                .collect::<Result<Vec<_>>>()?
        })?,
        "progress" | "duration_days" => text
            .trim_end_matches('%')
            .parse::<u32>()
            .map_err(|_| invalid("Invalid integer"))?
            .into(),
        "estimate" => {
            serde_json::to_value(text.parse::<f64>().map_err(|_| invalid("Invalid effort"))?)?
        }
        _ => text.into(),
    };
    let mut object = serde_json::to_value(&*t)?;
    object[key] = v;
    *t = serde_json::from_value(object)?;
    Ok(())
}
/// Validate a multi-row property edit against the same rules as a real transaction.
pub fn batch_task_attributes(
    p: &Project,
    ids: &[i64],
    values: &[(String, String)],
) -> Result<Change> {
    validate_bulk_keys(values.iter().map(|(key, _)| key.as_str()))?;
    if values.is_empty() {
        return Err(invalid("変更する属性を選んでください"));
    }
    let mut keys = BTreeSet::new();
    if values.iter().any(|(key, _)| !keys.insert(key)) {
        return Err(invalid("Duplicate batch attribute"));
    }
    let mut tasks = Vec::new();
    for id in ids {
        let mut task = p
            .tasks
            .iter()
            .find(|t| t.id == *id)
            .ok_or_else(|| invalid("Selected task no longer exists"))?
            .clone();
        for (key, value) in values {
            set_value(p, &mut task, key, value)?;
        }
        tasks.push(task);
    }
    let change = Change::SaveTasks { tasks };
    let mut preview = p.clone();
    preview.change(&change)?;
    Ok(change)
}
/// Quote delimiters, newlines and quotes for lossless spreadsheet interchange.
pub fn encode(rows: &[Vec<String>], separator: char) -> String {
    rows.iter()
        .map(|row| {
            row.iter()
                .map(|v| {
                    if v.contains([separator, '"', '\r', '\n']) {
                        format!("\"{}\"", v.replace('"', "\"\""))
                    } else {
                        v.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(&separator.to_string())
        })
        .collect::<Vec<_>>()
        .join("\r\n")
        + "\r\n"
}
pub fn decode(text: &str, separator: char) -> Result<Vec<Vec<String>>> {
    if text.len() > 2 * 1024 * 1024 {
        return Err(invalid("Clipboard payload exceeds 2 MiB"));
    }
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    let mut quoted = false;
    let mut closed = false;
    while let Some(ch) = chars.next() {
        if quoted {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    quoted = false;
                    closed = true;
                }
            } else {
                cell.push(ch);
            }
            continue;
        }
        if ch == separator {
            row.push(std::mem::take(&mut cell));
            closed = false;
        } else if ch == '\r' || ch == '\n' {
            if ch == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            row.push(std::mem::take(&mut cell));
            rows.push(std::mem::take(&mut row));
            closed = false;
        } else if ch == '"' && cell.is_empty() && !closed {
            quoted = true;
        } else {
            if closed {
                return Err(invalid("Characters after closing quote"));
            }
            cell.push(ch);
        }
    }
    if quoted {
        return Err(invalid("Unclosed quoted cell"));
    }
    if !row.is_empty() || !cell.is_empty() || closed {
        row.push(cell);
        rows.push(row);
    }
    Ok(rows)
}
pub fn export_csv(p: &Project, filter: &TaskFilter, keys: &[String]) -> Result<String> {
    filter.validate()?;
    if keys.is_empty() {
        return Err(invalid("Select at least one CSV column"));
    }
    let mut seen = BTreeSet::new();
    let columns = keys
        .iter()
        .map(|key| {
            if !seen.insert(key) {
                return Err(invalid("Duplicate CSV column"));
            }
            all_columns()
                .find(|c| c.key == key)
                .ok_or_else(|| invalid("Unknown CSV column"))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut rows = vec![columns.iter().map(|c| c.label.into()).collect()];
    for (t, _) in p
        .ordered_tasks()
        .into_iter()
        .filter(|(t, _)| p.matches_filter(filter, t))
    {
        rows.push(columns.iter().map(|c| value(p, t, c.key)).collect());
    }
    Ok("\u{feff}".to_string() + &encode(&rows, ','))
}

/// Optional fields supported by Jira's external-system CSV importer.
pub const JIRA_COLUMNS: &[(&str, &str)] = &[
    ("description", "Description"),
    ("status", "Status"),
    ("priority", "Priority"),
    ("assignee", "Assignee"),
    ("due", "Due Date"),
    ("category", "Component"),
];

#[derive(Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct JiraOptions {
    /// Map roots/children/grandchildren to Epic/Task/Sub-task; otherwise Task/Sub-task.
    pub epic_roots: bool,
    pub epic_type: String,
    pub task_type: String,
    pub subtask_type: String,
    pub issue_type: String,
}
impl Default for JiraOptions {
    fn default() -> Self {
        Self {
            epic_roots: true,
            epic_type: "Epic".into(),
            task_type: "Task".into(),
            subtask_type: "Sub-task".into(),
            issue_type: "Bug".into(),
        }
    }
}

/// Export new Jira issues, including ancestors of matches so no parent IDs dangle.
/// IDs are temporary import identifiers, never existing Jira issue keys.
pub fn export_jira_csv(
    p: &Project,
    filter: &TaskFilter,
    keys: &[String],
    options: &JiraOptions,
) -> Result<String> {
    p.validate()?;
    filter.validate()?;
    if [
        &options.epic_type,
        &options.task_type,
        &options.subtask_type,
        &options.issue_type,
    ]
    .iter()
    .any(|s| s.trim().is_empty() || s.contains(['\r', '\n']))
    {
        return Err(invalid(
            "Jira issue types must be nonempty single-line names",
        ));
    }
    let mut seen = BTreeSet::new();
    let columns = keys
        .iter()
        .map(|key| {
            if !seen.insert(key) {
                return Err(invalid("Duplicate Jira CSV column"));
            }
            JIRA_COLUMNS
                .iter()
                .find(|(k, _)| *k == key)
                .ok_or_else(|| invalid("Unknown Jira CSV column"))
        })
        .collect::<Result<Vec<_>>>()?;
    let tasks = p
        .tasks
        .iter()
        .map(|t| (t.id, t))
        .collect::<BTreeMap<_, _>>();
    let mut included = BTreeSet::new();
    for t in p.tasks.iter().filter(|t| p.matches_filter(filter, t)) {
        if matches!(t.kind, Kind::Inquiry | Kind::Workaround) {
            return Err(invalid(
                "Jira export supports tasks, bugs and issues. Select a kind filter or use standard CSV for inquiry/workaround records.",
            ));
        }
        let mut current = Some(t.id);
        while let Some(id) = current {
            if !included.insert(id) {
                break;
            }
            current = tasks[&id].parent;
        }
    }
    let mut rows = vec![vec![
        "Summary".into(),
        "Issue Type".into(),
        "Issue ID".into(),
        "Parent".into(),
    ]];
    rows[0].extend(columns.iter().map(|(_, header)| (*header).into()));
    let mut ordered = p.ordered_tasks();
    // Jira imports higher hierarchy levels before lower levels.
    ordered.sort_by_key(|(t, depth)| {
        if options.epic_roots && t.kind != Kind::Task && *depth == 0 {
            1
        } else {
            *depth
        }
    });
    for (t, depth) in ordered
        .into_iter()
        .filter(|(t, _)| included.contains(&t.id))
    {
        if depth > if options.epic_roots { 2 } else { 1 } {
            return Err(invalid(format!(
                "Task #{} exceeds the selected Jira hierarchy; no CSV was exported",
                t.id
            )));
        }
        if t.title.chars().count() > 255 || t.title.contains(['\r', '\n']) {
            return Err(invalid(format!(
                "Task #{} requires a single-line Jira summary of at most 255 characters",
                t.id
            )));
        }
        let issue_type = if depth == if options.epic_roots { 2 } else { 1 } {
            &options.subtask_type
        } else if t.kind != Kind::Task {
            &options.issue_type
        } else if options.epic_roots && depth == 0 {
            &options.epic_type
        } else {
            &options.task_type
        };
        let mut row = vec![
            t.title.clone(),
            issue_type.clone(),
            t.id.to_string(),
            t.parent.map(|id| id.to_string()).unwrap_or_default(),
        ];
        row.extend(columns.iter().map(|(key, _)| value(p, t, key)));
        rows.push(row);
    }
    Ok("\u{feff}".to_string() + &encode(&rows, ','))
}
