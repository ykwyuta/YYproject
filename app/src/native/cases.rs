//! Dedicated bug/issue forms and count-based analysis views.
use super::*;

impl State {
    pub(super) fn list_kind(&self) -> Kind {
        match self.view {
            3 | 10 => Kind::Issue,
            8 | 9 => Kind::Bug,
            11 | 13 => Kind::Inquiry,
            12 | 14 => Kind::Workaround,
            _ => Kind::Task,
        }
    }
    pub(super) fn edit_case(&mut self, new: bool, kind: Kind) -> Result<()> {
        let p = self
            .project
            .as_ref()
            .ok_or("プロジェクトを開いてください")?
            .clone();
        let task = if new {
            Task {
                kind,
                include_in_effort: false,
                case: Some(Box::new(yy_project::cases::CaseData {
                    detected_at: Some(chrono::Utc::now()),
                    ..Default::default()
                })),
                ..Default::default()
            }
        } else {
            self.task().ok_or("項目を選択してください")?.clone()
        };
        let mut fields = vec![];
        let keys: Vec<_> = table::CASE_COLUMNS
            .iter()
            .filter(|c| c.editable)
            .map(|c| c.key.to_owned())
            .collect();
        for key in &keys {
            let c = table::CASE_COLUMNS.iter().find(|c| c.key == key).unwrap();
            let page = match key.as_str() {
                "case.occurrence_phase"
                | "case.cause"
                | "case.detection_phase"
                | "case.temporary_difficulty"
                | "case.permanent_difficulty"
                | "case.detection_difficulty" => 1,
                "case.temporary_plan" => 2,
                "case.permanent_plan" => 3,
                "case.detected_at" | "case.occurred_at" | "case.reported_at"
                | "case.triaged_at" => 4,
                "case.temporary_due_at"
                | "case.temporary_applied_at"
                | "case.permanent_due_at"
                | "case.permanent_applied_at" => 5,
                "case.verified_at" | "case.closed_at" | "due" | "related_tasks" => 6,
                "description" => 7,
                "review" | "reviewer" | "review_note" => 8,
                _ => 0,
            };
            let value = match key.as_str() {
                "category" => task.category.map(|n| n.to_string()).unwrap_or_default(),
                "assignee" => task.assignee.map(|n| n.to_string()).unwrap_or_default(),
                "reviewer" => task.reviewer.map(|n| n.to_string()).unwrap_or_default(),
                _ => table::value(&p, &task, key),
            };
            let choices = match key.as_str() {
                "status" => Status::ALL
                    .iter()
                    .map(|v| (v.label().into(), v.label().into()))
                    .collect(),
                "priority" => Priority::ALL
                    .iter()
                    .map(|v| (v.label().into(), v.label().into()))
                    .collect(),
                "severity" => Severity::ALL
                    .iter()
                    .map(|v| (v.label().into(), v.label().into()))
                    .collect(),
                "review" => ReviewState::ALL
                    .iter()
                    .map(|v| (v.label().into(), v.label().into()))
                    .collect(),
                "assignee" | "reviewer" | "category" => {
                    std::iter::once((String::new(), "（未指定）".into()))
                        .chain(
                            p.masters
                                .iter()
                                .filter(|m| {
                                    m.kind
                                        == if key == "category" {
                                            MasterKind::Category
                                        } else {
                                            MasterKind::Assignee
                                        }
                                })
                                .map(|m| {
                                    (
                                        m.id.to_string(),
                                        if key == "category" {
                                            p.category_path(Some(m.id)).join(" / ")
                                        } else {
                                            m.name.clone()
                                        },
                                    )
                                }),
                        )
                        .collect()
                }
                k if k.ends_with("_difficulty") => yy_project::cases::Difficulty::ALL
                    .iter()
                    .map(|d| (d.label().into(), d.label().into()))
                    .collect(),
                _ => vec![],
            };
            let mut f = if choices.is_empty() {
                field(c.label, &value, page)
            } else {
                choice(c.label, &value, choices, page)
            };
            f.multi = matches!(
                key.as_str(),
                "description" | "review_note" | "case.temporary_plan" | "case.permanent_plan"
            );
            fields.push(f);
        }
        let preview_p = p.clone();
        let preview_t = task.clone();
        let preview_keys = keys.clone();
        if let Some(values) = form(
            self.hwnd,
            if task.kind == Kind::Bug {
                "バグ管理 — 詳細"
            } else {
                "課題管理 — 詳細"
            },
            fields,
            move |v| {
                let mut t = preview_t.clone();
                for (key, value) in preview_keys.iter().zip(v) {
                    table::set_value(&preview_p, &mut t, key, value).map_err(|e| e.to_string())?;
                }
                if t.status != Status::Done && t.progress == 100 {
                    t.progress = 0;
                }
                preview_p
                    .preview_change(&Change::SaveTask { task: t })
                    .map_err(|e| e.to_string())?;
                Ok(())
            },
        ) {
            let mut t = task;
            for (key, value) in keys.iter().zip(&values) {
                table::set_value(&p, &mut t, key, value)?;
            }
            if t.status != Status::Done && t.progress == 100 {
                t.progress = 0;
            }
            self.apply(Change::SaveTask { task: t })?;
        }
        Ok(())
    }
    pub(super) fn case_dates(&self) -> (chrono::NaiveDate, chrono::NaiveDate) {
        self.case_range.unwrap_or_else(|| {
            let today = chrono::Utc::now().date_naive();
            let first = self
                .project
                .as_ref()
                .and_then(|p| {
                    p.tasks
                        .iter()
                        .filter(|t| t.kind == self.list_kind() && !t.archived)
                        .filter_map(|t| yy_project::work::opened_at(t).map(|d| d.date_naive()))
                        .min()
                })
                .unwrap_or(today);
            (first.max(today - chrono::Days::new(3660)).min(today), today)
        })
    }
    pub(super) fn case_range_form(&mut self) -> Result<()> {
        let (first, last) = self.case_dates();
        if let Some(v) = form(
            self.hwnd,
            "統計 — 集計期間（UTCの日付）",
            vec![field("開始日", first, 0), field("終了日", last, 0)],
            |v| {
                let a = date(&v[0])?;
                let b = date(&v[1])?;
                if a > b || (b - a).num_days() > 3660 {
                    return Err("集計期間は開始日から10年以内で指定してください".into());
                }
                Ok(())
            },
        ) {
            self.case_range = Some((date(&v[0])?, date(&v[1])?));
            self.x = 0;
            self.y = 0;
            self.layout();
            invalidate(self.hwnd);
        }
        Ok(())
    }
    pub(super) fn case_statistics(&self) -> yy_project::Result<yy_project::cases::Statistics> {
        let p = self
            .project
            .as_ref()
            .ok_or_else(|| yy_project::Error::Invalid("プロジェクトを開いてください".into()))?;
        let (a, b) = self.case_dates();
        if matches!(self.list_kind(), Kind::Inquiry | Kind::Workaround) {
            return Ok(self.management_statistics()?.convergence);
        }
        yy_project::cases::statistics(p, self.list_kind(), a, b, &self.filter)
    }
    pub(super) fn case_chart_size(&self) -> (i32, i32) {
        let (a, b) = self.case_dates();
        let management = self.management_statistics().ok();
        let rows = self
            .case_statistics()
            .map(|s| {
                panels(&s, management.as_ref())
                    .iter()
                    .map(|(_, m)| m.len().max(1) as i32 * 30 + 50)
                    .sum::<i32>()
            })
            .unwrap_or(0);
        (
            self.px((((b - a).num_days() + 1) * 20 + 120).max(1000) as i32),
            self.px(150 + rows) + self.case_chart_height(),
        )
    }
    fn case_chart_height(&self) -> i32 {
        (self.body.bottom - self.body.top - self.px(65)).max(self.px(220))
    }
    pub(super) fn paint_case_statistics(&self, dc: HDC) {
        let Ok(stats) = self.case_statistics() else {
            return;
        };
        let Some(p) = &self.project else {
            return;
        };
        let management = self.management_statistics().ok();
        let a = self.body;
        let black = COLORREF(0x473025);
        let left = a.left + self.px(12) - self.x;
        let top = a.top - self.y;
        label(
            dc,
            rect(left, top, self.content.0, self.px(30)),
            &format!(
                "{}：{}件 / 未完了{} / 完了{} / 中止{} / 日時未記録{}（別枠）",
                match self.list_kind() {
                    Kind::Bug => "バグ",
                    Kind::Issue => "課題",
                    Kind::Inquiry => "依頼・質問",
                    Kind::Workaround => "暫定対応",
                    _ => "タスク",
                },
                stats.total,
                stats.open,
                stats.resolved,
                stats.cancelled,
                stats.undated
            ),
            black,
        );
        label(
            dc,
            rect(left, top + self.px(32), self.content.0, self.px(28)),
            &if let Some(extra) = &management {
                if self.list_kind() == Kind::Inquiry {
                    format!(
                        "未回答{} / 期限超過{} / 平均回答{} / 期間内の記録：こちら{}・相手{}・メモ{}",
                        extra.unanswered,
                        extra.overdue,
                        extra
                            .mean_answer_hours
                            .map_or("未算出".into(), |n| format!("{n:.1}h")),
                        extra.outgoing,
                        extra.incoming,
                        extra.notes
                    )
                } else {
                    format!(
                        "暫定未実施{} / 恒久予定超過{} / 平均暫定対応{} / 平均恒久対応{}",
                        extra.not_applied,
                        extra.overdue,
                        extra
                            .mean_temporary_hours
                            .map_or("未算出".into(), |n| format!("{n:.1}h")),
                        extra
                            .convergence
                            .mean_resolution_hours
                            .map_or("未算出".into(), |n| format!("{n:.1}h"))
                    )
                }
            } else {
                format!(
                    "平均解決時間 {} / 集計期間・UTC日付は「統計設定」で変更",
                    stats
                        .mean_resolution_hours
                        .map_or("未算出".into(), |n| format!("{n:.1}時間"))
                )
            },
            black,
        );
        let mut chart = p.clone();
        let (from, to) = self.case_dates();
        chart.start = from;
        chart.finish = to;
        chart.history = stats
            .daily
            .iter()
            .map(|d| Snapshot {
                day: d.day,
                scope: d.detected as f64,
                completed: d.resolved as f64,
                remaining: d.open as f64,
            })
            .collect();
        charts::paint_counts(
            dc,
            &chart,
            timeline::Viewport {
                dpi: self.dpi,
                body: rect(
                    a.left,
                    top + self.px(65),
                    a.right - a.left,
                    self.case_chart_height(),
                ),
                x: self.x,
                y: 0,
            },
        );
        let mut y = top + self.px(85) + self.case_chart_height();
        label(
            dc,
            rect(left, y, self.content.0, self.px(28)),
            "以下は集計終了日までの対象の分類（現時点の属性・現在の絞り込みを使用）。未完了件数には再オープンを反映。",
            black,
        );
        y += self.px(35);
        for (title, map) in panels(&stats, management.as_ref()) {
            label(dc, rect(left, y, self.px(960), self.px(30)), title, black);
            y += self.px(32);
            let mut values: Vec<_> = map.iter().collect();
            values.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
            let max = values.first().map_or(1, |(_, n)| **n).max(1);
            if values.is_empty() {
                label(
                    dc,
                    rect(left, y, self.px(900), self.px(30)),
                    "対象データがありません",
                    black,
                );
                y += self.px(30);
            }
            for (name, n) in values {
                if y + self.px(30) > a.top && y < a.bottom {
                    label(dc, rect(left, y, self.px(260), self.px(28)), name, black);
                    fill(
                        dc,
                        rect(
                            left + self.px(270),
                            y + self.px(5),
                            self.px(500) * (*n as i32) / (max as i32),
                            self.px(18),
                        ),
                        COLORREF(0xc76c2d),
                    );
                    label(
                        dc,
                        rect(left + self.px(785), y, self.px(160), self.px(28)),
                        &format!(
                            "{}件 ({:.1}%)",
                            n,
                            *n as f64 / stats.total.max(1) as f64 * 100.
                        ),
                        black,
                    );
                }
                y += self.px(30);
            }
            y += self.px(20);
        }
    }
}
fn panels<'a>(
    s: &'a yy_project::cases::Statistics,
    management: Option<&'a yy_project::work::Statistics>,
) -> Vec<(&'static str, &'a BTreeMap<String, usize>)> {
    if let Some(m) = management {
        if s.kind == Kind::Inquiry {
            vec![
                ("相手チーム（件数順）", &s.causes),
                ("依頼／質問", &m.classifications),
                ("状態", &m.states),
            ]
        } else {
            vec![
                ("暫定対応の分類（件数順）", &m.classifications),
                ("状態", &m.states),
            ]
        }
    } else {
        vec![
            ("原因分類（件数順）", &s.causes),
            ("発生工程", &s.occurrence_phases),
            ("検出工程", &s.detection_phases),
            ("暫定対策難易度", &s.temporary_difficulties),
            ("恒久対策難易度", &s.permanent_difficulties),
            ("検知難易度", &s.detection_difficulties),
        ]
    }
}

pub(super) fn smoke_test(
    h: HWND,
    directory: &std::path::Path,
    report: &mut Vec<String>,
) -> Result<()> {
    unsafe {
        SetWindowPos(
            h,
            None,
            0,
            0,
            px(h, 1100),
            px(h, 700),
            SWP_NOZORDER | SWP_NOACTIVATE,
        )?;
    }
    let original = APP.with(|a| -> Result<_> {
        let mut a = a.borrow_mut();
        let s = a.as_mut().unwrap();
        let old = s.project.clone();
        s.project = Some(s.store.create(
            "バグ・課題専用検証".into(),
            date("2026-01-01")?,
            date("2026-12-31")?,
            "GUI-test",
        )?);
        s.filter = TaskFilter::default();
        s.reload()?;
        Ok(old)
    })?;
    for (command, title, status, closed) in [
        (124, "バグ：解決済み", "完了", "2026-01-02T09:00:00Z"),
        (124, "バグ：継続", "未着手", ""),
        (111, "課題：継続", "未着手", ""),
        (111, "課題：二件目", "未着手", ""),
    ] {
        SMOKE_FORM_VALUES.with(|m| {
            *m.borrow_mut() = [
                ("件名", title),
                ("状態", status),
                ("検出日時", "2026-01-01T09:00:00Z"),
                ("原因分類", "仕様漏れ"),
                ("発生工程", "設計"),
                ("検出工程", "結合試験"),
                ("暫定対策案", "一時的な回避策\n2行目"),
                ("恒久対策案・案件", "恒久対策案件ABC"),
                ("恒久対策難易度", "高"),
                ("完了・中止日時", closed),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect()
        });
        send(h, WM_COMMAND, command, 0);
        SMOKE_FORM_VALUES.with(|m| m.borrow_mut().clear());
    }
    APP.with(|a| -> Result<()> {
        let mut a = a.borrow_mut();
        let s = a.as_mut().unwrap();
        let p = s.project.as_ref().unwrap();
        if p.tasks.len() != 4
            || p.tasks
                .iter()
                .any(|t| t.include_in_effort || t.case.is_none())
        {
            return Err("Dedicated native case forms failed to save separate records".into());
        }
        if p.tasks[0].case.as_ref().unwrap().permanent_difficulty
            != yy_project::cases::Difficulty::High
        {
            return Err("Case difficulty choice did not persist".into());
        }
        for (view, expected, name) in [
            (2, 0, "case-empty-task-list"),
            (8, 2, "bugs-grid"),
            (3, 2, "issues-grid"),
        ] {
            s.command(200 + view)?;
            s.layout();
            let mut area = RECT::default();
            unsafe {
                GetClientRect(h, &mut area)?;
            }
            smoke::capture(s, area, &directory.join(format!("{name}.bmp")))?;
            if s.grid_rows().len() != expected {
                return Err("Task/bug/issue lists did not isolate record kinds".into());
            }
        }
        s.command(208)?;
        let cause = s
            .grid_columns()
            .iter()
            .position(|c| c.key == "case.cause")
            .unwrap();
        s.grid.anchor = (1, cause);
        s.grid.current = (1, cause);
        s.grid.selection = true;
        s.x = s.px((0..cause).map(|i| s.grid_width(i)).sum());
        s.layout();
        s.begin_cell_edit(None)?;
        unsafe {
            let edit = GetDlgItem(Some(h), 501)?;
            SetWindowTextW(edit, w!("実装誤り"))?;
        }
        s.finish_cell_edit(true)?;
        s.filter
            .columns
            .insert("case.cause".into(), "実装誤り".into());
        if s.grid_rows().len() != 1 {
            return Err("Case column filtering failed".into());
        }
        s.filter = TaskFilter::default();
        for view in [8, 3] {
            s.command(200 + view)?;
            let ids = s.grid_rows().iter().map(|r| r.0).collect::<Vec<_>>();
            let cause = s
                .grid_columns()
                .iter()
                .position(|c| c.key == "case.cause")
                .unwrap();
            s.grid.anchor = (0, cause);
            s.grid.current = (1, cause);
            s.grid.selection = true;
            s.grid_fill_down()?;
            let copied = s.grid_copy_text();
            if table::decode(&copied, '\t')?.len() != 2 {
                return Err("Case same-column copy selection was lost".into());
            }
            s.grid_paste("一括コピー分類")?;
            SMOKE_FORM_VALUES.with(|m| {
                *m.borrow_mut() = [
                    ("検出工程の変更", "set"),
                    ("検出工程", "システム試験"),
                    ("恒久対策難易度の変更", "set"),
                    ("恒久対策難易度", "very_high"),
                ]
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect()
            });
            s.command(122)?;
            SMOKE_FORM_VALUES.with(|m| m.borrow_mut().clear());
            if s.project
                .as_ref()
                .unwrap()
                .tasks
                .iter()
                .filter(|t| ids.contains(&t.id))
                .any(|t| {
                    t.case.as_ref().unwrap().detection_phase != "システム試験"
                        || t.case.as_ref().unwrap().cause != "一括コピー分類"
                })
            {
                return Err("Case bulk attributes or one-cell paste did not persist".into());
            }
            s.x = 0;
            s.layout();
            let before = s.project.as_ref().unwrap().revision;
            let width = s.grid_width(1);
            let edge = POINT {
                x: s.body.left + s.px(s.grid_width(0) + width),
                y: s.body.top + s.px(12),
            };
            if !s.begin_resize(edge) {
                return Err("Case column boundary is not draggable".into());
            }
            s.move_resize(POINT {
                x: edge.x + s.px(35),
                y: edge.y,
            });
            s.finish_resize(true);
            if s.grid_width(1) != width + 35 || s.project.as_ref().unwrap().revision != before {
                return Err("Case column resize changed data or failed".into());
            }
            let row_id = s.grid_rows()[0].0;
            let height = s.grid_height(row_id);
            let row_edge = POINT {
                x: s.body.left + s.px(20),
                y: s.grid_row_top(0) + height,
            };
            if !s.begin_resize(row_edge) {
                return Err("Case row boundary is not draggable".into());
            }
            s.move_resize(POINT {
                x: row_edge.x,
                y: row_edge.y + s.px(12),
            });
            s.finish_resize(true);
            if s.grid_height(row_id) != height + s.px(12) {
                return Err("Case row resize did not persist".into());
            }
            let csv_path = directory.join(format!("case-list-{view}.csv"));
            SMOKE_FILE_PATH.with(|p| *p.borrow_mut() = Some(csv_path.clone()));
            s.command(if view == 8 { 125 } else { 126 })?;
            SMOKE_FILE_PATH.with(|p| p.borrow_mut().take());
            let rows = table::decode(&std::fs::read_to_string(csv_path)?, ',')?;
            if rows.len() != 3
                || !rows[0].iter().any(|v| v == "原因分類")
                || !rows[0].iter().any(|v| v == "検出日時")
            {
                return Err(
                    "Case native CSV export omitted dedicated columns or mixed kinds".into(),
                );
            }
        }
        s.case_range = Some((date("2026-01-01")?, date("2026-01-04")?));
        for (view, total, open, name) in [
            (9, 2, 1, "bugs-statistics"),
            (10, 2, 2, "issues-statistics"),
        ] {
            s.command(200 + view)?;
            s.layout();
            let stat = s.case_statistics()?;
            if stat.total != total || stat.open != open || stat.daily.len() != 4 {
                return Err("Dedicated case daily statistics are incorrect".into());
            }
            let mut area = RECT::default();
            unsafe {
                GetClientRect(h, &mut area)?;
            }
            smoke::capture(s, area, &directory.join(format!("{name}.bmp")))?;
            s.y = s.content.1;
            s.layout();
            smoke::capture(s, area, &directory.join(format!("{name}-scrolled.bmp")))?;
        }
        s.project = original;
        s.case_range = None;
        s.filter = TaskFilter::default();
        s.command(202)?;
        s.reload()?;
        Ok(())
    })?;
    report.push("Dedicated bug/issue native forms, phase/cause/plans/difficulty/timestamps, isolated grids, F2 cell persistence, fill-down/one-cell paste, native bulk attributes, column/row boundary resize, dedicated CSV menus, cause column filter, daily count convergence and six analysis charts with scrolling: OK".into());
    Ok(())
}
