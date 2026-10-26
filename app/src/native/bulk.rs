//! Explicit multi-row selection and preview-validated attribute updates.
use super::*;
const ATTRIBUTES: &[(&str, &str)] = &[
    ("assignee", "担当者"),
    ("category", "カテゴリ"),
    ("status", "状態"),
    ("priority", "優先度"),
    ("start", "開始日"),
    ("due", "期限"),
    ("completed_on", "完了日"),
    ("progress", "進捗(%)"),
];
impl State {
    pub(super) fn selected_task_ids(&self) -> Vec<i64> {
        if !matches!(self.view, 2 | 3) {
            return vec![];
        }
        self.grid_rows()
            .iter()
            .enumerate()
            .filter(|(row, (id, _))| {
                if self.grid.row_selection {
                    self.grid.selected_rows.contains(id)
                } else if self.grid.selection {
                    *row >= self.grid.anchor.0.min(self.grid.current.0)
                        && *row <= self.grid.anchor.0.max(self.grid.current.0)
                } else {
                    self.selected == Some(*id)
                }
            })
            .map(|(_, r)| r.0)
            .collect()
    }
    pub(super) fn toggle_grid_row(&mut self, point: POINT) {
        let Some(id) = self.hit_task(point.x, point.y) else {
            return;
        };
        if !self.grid.row_selection {
            self.grid.selected_rows = self.selected_task_ids().into_iter().collect();
        }
        if !self.grid.selected_rows.remove(&id) {
            self.grid.selected_rows.insert(id);
        }
        self.grid.row_selection = true;
        self.grid.selection = false;
        if let Some(cell) = self.grid_cell(point) {
            self.grid.current = cell;
            self.grid.anchor = cell;
        }
        self.selected = Some(id);
        invalidate(self.hwnd);
    }
    pub(super) fn bulk_edit_form(&mut self) -> Result<()> {
        let ids = self.selected_task_ids();
        if ids.is_empty() {
            return Err("タスク一覧で対象の行を選択してください".into());
        }
        let p = self
            .project
            .as_ref()
            .ok_or("プロジェクトを開いてください")?
            .clone();
        let mut fields = Vec::new();
        for (i, (key, label)) in ATTRIBUTES.iter().enumerate() {
            fields.push(choice(
                &format!("{label}の変更"),
                "keep",
                vec![
                    ("keep".into(), "変更しない".into()),
                    ("set".into(), "指定値に変更".into()),
                    ("clear".into(), "空／初期値にする".into()),
                ],
                i / 2,
            ));
            let options = match *key {
                "assignee" | "category" => p
                    .masters
                    .iter()
                    .filter(|m| {
                        m.kind
                            == if *key == "assignee" {
                                MasterKind::Assignee
                            } else {
                                MasterKind::Category
                            }
                    })
                    .map(|m| {
                        (
                            m.id.to_string(),
                            if *key == "category" {
                                p.category_path(Some(m.id)).join(" / ")
                            } else {
                                m.name.clone()
                            },
                        )
                    })
                    .collect::<Vec<_>>(),
                "status" => Status::ALL
                    .iter()
                    .map(|s| {
                        (
                            serde_json::to_value(s).unwrap().as_str().unwrap().into(),
                            s.label().into(),
                        )
                    })
                    .collect(),
                "priority" => Priority::ALL
                    .iter()
                    .map(|s| {
                        (
                            serde_json::to_value(s).unwrap().as_str().unwrap().into(),
                            s.label().into(),
                        )
                    })
                    .collect(),
                _ => vec![],
            };
            fields.push(if options.is_empty() {
                field(label, "", i / 2)
            } else {
                let value = options[0].0.clone();
                choice(label, value, options, i / 2)
            });
        }
        let count = ids.len();
        let snapshot = p.clone();
        let make = move |v: &[String]| {
            let values = ATTRIBUTES
                .iter()
                .enumerate()
                .filter_map(|(i, (key, _))| match v[i * 2].as_str() {
                    "set" => Some(((*key).into(), v[i * 2 + 1].clone())),
                    "clear" => Some((
                        (*key).into(),
                        table::value(&snapshot, &Task::default(), key),
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>();
            table::batch_task_attributes(&snapshot, &ids, &values)
        };
        let validate = make.clone();
        if let Some(v) = form(
            self.hwnd,
            &format!("選択行の属性を一括変更（{count}件）"),
            fields,
            move |v| validate(v).map(|_| ()).map_err(|e| e.to_string()),
        ) {
            self.project =
                Some(
                    self.store
                        .apply(p.id, p.revision, &make(&v)?, "GUI bulk attributes")?,
                );
            self.grid.selected_rows.clear();
            self.grid.row_selection = false;
            self.grid.selection = false;
            self.reload()?;
            self.status = "選択行の属性を一括更新しました".into();
        }
        Ok(())
    }
}
