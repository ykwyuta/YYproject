//! Manual request/question histories and temporary workaround management.
use super::*;
use yy_project::work::{Direction, Exchange, Inquiry, Workaround};
impl State {
    pub(super) fn edit_work(&mut self, new: bool, kind: Kind) -> Result<()> {
        let p = self
            .project
            .as_ref()
            .ok_or("プロジェクトを開いてください")?
            .clone();
        let task = if new {
            Task {
                kind,
                include_in_effort: false,
                inquiry: if kind == Kind::Inquiry {
                    Some(Box::new(Inquiry {
                        requested_at: Some(chrono::Utc::now()),
                        ..Default::default()
                    }))
                } else {
                    None
                },
                workaround: if kind == Kind::Workaround {
                    Some(Box::new(Workaround {
                        detected_at: Some(chrono::Utc::now()),
                        ..Default::default()
                    }))
                } else {
                    None
                },
                ..Default::default()
            }
        } else {
            self.task().ok_or("項目を選択してください")?.clone()
        };
        let columns = if kind == Kind::Inquiry {
            table::INQUIRY_COLUMNS
        } else {
            table::WORKAROUND_COLUMNS
        };
        let keys: Vec<String> = columns
            .iter()
            .filter(|c| c.editable)
            .map(|c| c.key.into())
            .collect();
        let mut fields = vec![];
        for key in &keys {
            let c = columns.iter().find(|c| c.key == key).unwrap();
            let page = if kind == Kind::Inquiry {
                match key.as_str() {
                    "inquiry.kind" | "inquiry.target_team" | "inquiry.contact" => 1,
                    "inquiry.requested_at"
                    | "inquiry.response_due_at"
                    | "inquiry.answered_at"
                    | "inquiry.closed_at" => 2,
                    "description" => 3,
                    "related_tasks" => 4,
                    _ => 0,
                }
            } else {
                match key.as_str() {
                    "workaround.classification"
                    | "workaround.detected_at"
                    | "workaround.applied_at"
                    | "workaround.permanent_at"
                    | "workaround.permanent_due_on" => 1,
                    "workaround.reason" => 2,
                    "workaround.permanent_requirements" => 3,
                    "workaround.action" => 4,
                    "related_tasks" | "description" => 5,
                    _ => 0,
                }
            };
            let value = match key.as_str() {
                "category" => task.category.map(|n| n.to_string()).unwrap_or_default(),
                "assignee" => task.assignee.map(|n| n.to_string()).unwrap_or_default(),
                _ => table::value(&p, &task, key),
            };
            let options = match key.as_str() {
                "status" => Status::ALL
                    .iter()
                    .map(|s| (s.label().into(), s.label().into()))
                    .collect(),
                "priority" => Priority::ALL
                    .iter()
                    .map(|s| (s.label().into(), s.label().into()))
                    .collect(),
                "inquiry.kind" => vec![
                    ("依頼".into(), "依頼".into()),
                    ("質問".into(), "質問".into()),
                ],
                "category" | "assignee" => std::iter::once((String::new(), "（未指定）".into()))
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
                    .collect(),
                _ => vec![],
            };
            let mut field = if options.is_empty() {
                field(c.label, &value, page)
            } else {
                choice(c.label, &value, options, page)
            };
            field.multi = matches!(
                key.as_str(),
                "description"
                    | "workaround.reason"
                    | "workaround.permanent_requirements"
                    | "workaround.action"
            );
            fields.push(field);
        }
        let before = p.clone();
        let source = task.clone();
        let selected = keys.clone();
        if let Some(values) = form(
            self.hwnd,
            if kind == Kind::Inquiry {
                "依頼・質問管理 — 詳細"
            } else {
                "暫定対応管理 — 詳細"
            },
            fields,
            move |v| {
                let mut t = source.clone();
                for (key, value) in selected.iter().zip(v) {
                    table::set_value(&before, &mut t, key, value).map_err(|e| e.to_string())?;
                }
                before
                    .preview_change(&Change::SaveTask { task: t })
                    .map_err(|e| e.to_string())?;
                Ok(())
            },
        ) {
            let mut t = task;
            for (key, value) in keys.iter().zip(&values) {
                table::set_value(&p, &mut t, key, value)?;
            }
            self.apply(Change::SaveTask { task: t })?;
        }
        Ok(())
    }
    fn selected_inquiry(&self) -> Result<Task> {
        Ok(self
            .task()
            .filter(|t| t.kind == Kind::Inquiry && !t.archived)
            .ok_or("依頼・質問一覧で保管されていない項目を選択してください")?
            .clone())
    }
    fn choose_exchange(&self, title: &str) -> Result<Option<i64>> {
        let task = self.selected_inquiry()?;
        let d = task.inquiry.as_ref().unwrap();
        let options = d
            .exchanges
            .iter()
            .map(|e| {
                (
                    e.id.to_string(),
                    format!(
                        "#{} {} {}",
                        e.id,
                        e.at.map(|d| d.to_rfc3339()).unwrap_or_default(),
                        e.text.lines().next().unwrap_or_default()
                    ),
                )
            })
            .collect::<Vec<_>>();
        if options.is_empty() {
            return Err("やりとり記録がありません".into());
        }
        let first = options[0].0.clone();
        Ok(form(
            self.hwnd,
            title,
            vec![choice("やりとり記録", first, options, 0)],
            |_| Ok(()),
        )
        .map(|v| v[0].parse())
        .transpose()?)
    }
    pub(super) fn exchange_form(&mut self, new: bool) -> Result<()> {
        let task = self.selected_inquiry()?;
        let p = self.project.as_ref().unwrap().clone();
        let entry = if new {
            Exchange {
                at: Some(chrono::Utc::now()),
                ..Default::default()
            }
        } else {
            let Some(id) = self.choose_exchange("やりとり記録を選択")? else {
                return Ok(());
            };
            task.inquiry
                .as_ref()
                .unwrap()
                .exchanges
                .iter()
                .find(|e| e.id == id)
                .unwrap()
                .clone()
        };
        let fields = vec![
            field(
                "記録日時（タイムゾーン付き）",
                entry.at.map(|d| d.to_rfc3339()).unwrap_or_default(),
                0,
            ),
            choice(
                "方向",
                serde_json::to_value(entry.direction)?.as_str().unwrap(),
                [Direction::Outgoing, Direction::Incoming, Direction::Note]
                    .iter()
                    .map(|d| {
                        (
                            serde_json::to_value(d).unwrap().as_str().unwrap().into(),
                            d.label().into(),
                        )
                    })
                    .collect(),
                0,
            ),
            field("相手・記録者", &entry.person, 0),
            {
                let mut f = field("やりとりの内容", &entry.text, 1);
                f.multi = true;
                f
            },
        ];
        let make = move |v: &[String]| -> Result<Change> {
            Ok(Change::SaveExchange {
                id: task.id,
                entry: Exchange {
                    id: entry.id,
                    at: Some(
                        chrono::DateTime::parse_from_rfc3339(&v[0])?.with_timezone(&chrono::Utc),
                    ),
                    direction: serde_json::from_value(serde_json::Value::String(v[1].clone()))?,
                    person: v[2].clone(),
                    text: v[3].clone(),
                },
            })
        };
        let validate = make;
        let snapshot = p.clone();
        if let Some(v) = form(
            self.hwnd,
            "やりとり記録 — 手動入力（送信は行いません）",
            fields,
            move |v| {
                let change = validate(v).map_err(|e| e.to_string())?;
                snapshot
                    .preview_change(&change)
                    .map_err(|e| e.to_string())?;
                Ok(())
            },
        ) {
            self.project =
                Some(
                    self.store
                        .apply(p.id, p.revision, &make(&v)?, "GUI exchange log")?,
                );
            self.reload()?;
        }
        Ok(())
    }
    pub(super) fn remove_exchange_form(&mut self) -> Result<()> {
        let task = self.selected_inquiry()?;
        if let Some(id) = self.choose_exchange("削除するやりとり記録を選択")?
            && form(
                self.hwnd,
                "やりとり記録の削除確認",
                vec![choice(
                    "削除の確認",
                    "no",
                    vec![
                        ("no".into(), "取り消す".into()),
                        ("yes".into(), "選んだ記録を削除（監査は保持）".into()),
                    ],
                    0,
                )],
                |v| {
                    if v[0] == "yes" {
                        Ok(())
                    } else {
                        Err("削除する場合は確認を選択してください".into())
                    }
                },
            )
            .is_some()
        {
            self.apply(Change::RemoveExchange {
                id: task.id,
                entry_id: id,
            })?;
        }
        Ok(())
    }
    pub(super) fn show_exchange_log(&self) -> Result<()> {
        let task = self.selected_inquiry()?;
        let mut field = field(
            "記録（送信や添付の機能はありません）",
            yy_project::work::exchange_log(task.inquiry.as_ref().unwrap()),
            0,
        );
        field.multi = true;
        field.read_only = true;
        form(self.hwnd, "やりとりの記録を確認", vec![field], |_| Ok(()));
        Ok(())
    }
    pub(super) fn management_statistics(&self) -> yy_project::Result<yy_project::work::Statistics> {
        let p = self
            .project
            .as_ref()
            .ok_or_else(|| Error::Invalid("プロジェクトを開いてください".into()))?;
        let (from, to) = self.case_dates();
        yy_project::work::statistics(p, self.list_kind(), from, to, &self.filter)
    }
}

pub(super) fn smoke_test(
    h: HWND,
    directory: &std::path::Path,
    report: &mut Vec<String>,
) -> Result<()> {
    let original = APP.with(|a| -> Result<_> {
        let mut a = a.borrow_mut();
        let s = a.as_mut().unwrap();
        let old = s.project.clone();
        s.project = Some(s.store.create(
            "依頼・暫定対応 GUI専用検証".into(),
            date("2026-01-01")?,
            date("2026-12-31")?,
            "GUI-test",
        )?);
        s.filter = TaskFilter::default();
        s.reload()?;
        Ok(old)
    })?;
    for (cmd, title) in [
        (127, "依頼1"),
        (127, "依頼2"),
        (128, "暫定1"),
        (128, "暫定2"),
    ] {
        SMOKE_FORM_VALUES.with(|m| {
            *m.borrow_mut() = [
                ("件名", title),
                ("相手チーム", "基盤チーム"),
                ("依頼・質問日時", "2026-01-01T09:00:00Z"),
                ("発生理由", "修正待ち"),
                ("検知日時", "2026-01-01T09:00:00Z"),
            ]
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect()
        });
        send(h, WM_COMMAND, cmd, 0);
        SMOKE_FORM_VALUES.with(|m| m.borrow_mut().clear());
    }
    APP.with(|a| -> Result<()> {
        let mut a = a.borrow_mut();
        let s = a.as_mut().unwrap();
        if s.project.as_ref().unwrap().tasks.len() != 4 {
            return Err("Management native forms did not save four records".into());
        }
        s.command(211)?;
        s.selected = Some(s.grid_rows()[0].0);
        SMOKE_FORM_VALUES.with(|m| {
            *m.borrow_mut() = [
                ("記録日時（タイムゾーン付き）", "2026-01-02T09:00:00Z"),
                ("方向", "incoming"),
                ("相手・記録者", "窓口"),
                ("やりとりの内容", "回答,\"引用\"\n2行目"),
            ]
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect()
        });
        s.exchange_form(true)?;
        SMOKE_FORM_VALUES.with(|m| m.borrow_mut().clear());
        if s.task().unwrap().inquiry.as_ref().unwrap().exchanges.len() != 1 {
            return Err("Native exchange form did not persist log".into());
        }
        s.show_exchange_log()?;
        SMOKE_FORM_VALUES.with(|m| {
            *m.borrow_mut() = [("やりとりの内容".into(), "編集済み回答".into())]
                .into_iter()
                .collect()
        });
        s.exchange_form(false)?;
        SMOKE_FORM_VALUES.with(|m| m.borrow_mut().clear());
        if s.task().unwrap().inquiry.as_ref().unwrap().exchanges[0].text != "編集済み回答" {
            return Err("Native exchange edit failed".into());
        }
        SMOKE_FORM_VALUES.with(|m| {
            *m.borrow_mut() = [("削除の確認".into(), "yes".into())].into_iter().collect()
        });
        s.remove_exchange_form()?;
        SMOKE_FORM_VALUES.with(|m| m.borrow_mut().clear());
        if !s
            .task()
            .unwrap()
            .inquiry
            .as_ref()
            .unwrap()
            .exchanges
            .is_empty()
        {
            return Err("Native exchange removal failed".into());
        }
        for (view, key, label, bulk_label, cmd) in [
            (11, "inquiry.target_team", "相手チーム", "相手担当者", 136),
            (
                12,
                "workaround.classification",
                "暫定対応の分類",
                "暫定対応の分類",
                137,
            ),
        ] {
            s.command(200 + view)?;
            s.layout();
            if s.grid_rows().len() != 2 {
                return Err("Management grid kind isolation failed".into());
            }
            let col = s.grid_columns().iter().position(|c| c.key == key).unwrap();
            s.grid.anchor = (0, col);
            s.grid.current = (0, col);
            s.grid.selection = true;
            s.x = s.px((0..col).map(|i| s.grid_width(i)).sum());
            s.layout();
            s.begin_cell_edit(None)?;
            unsafe {
                SetWindowTextW(GetDlgItem(Some(h), 501)?, w!("セル編集"))?;
            }
            s.finish_cell_edit(true)?;
            if table::value(
                s.project.as_ref().unwrap(),
                &s.project
                    .as_ref()
                    .unwrap()
                    .tasks
                    .iter()
                    .find(|t| t.id == s.grid_rows()[0].0)
                    .unwrap()
                    .clone(),
                key,
            ) != "セル編集"
            {
                return Err("Management native cell edit failed".into());
            }
            s.grid.anchor = (0, col);
            s.grid.current = (1, col);
            s.grid.selection = true;
            s.grid_paste("共通分類")?;
            s.grid_fill_down()?;
            if table::decode(&s.grid_copy_text(), '\t')?.len() != 2 {
                return Err("Management clipboard selection failed".into());
            }
            SMOKE_FORM_VALUES.with(|m| {
                *m.borrow_mut() = [
                    (format!("{bulk_label}の変更"), "set".into()),
                    (bulk_label.into(), "一括属性".into()),
                ]
                .into_iter()
                .collect()
            });
            s.bulk_edit_form()?;
            SMOKE_FORM_VALUES.with(|m| m.borrow_mut().clear());
            s.filter.columns.insert(
                key.into(),
                if view == 11 {
                    "共通分類"
                } else {
                    "一括属性"
                }
                .into(),
            );
            if s.grid_rows().len() != 2 {
                return Err("Management bulk/column filter failed".into());
            }
            s.filter = TaskFilter::default();
            s.x = 0;
            s.layout();
            let width = s.grid_width(1);
            let edge = POINT {
                x: s.body.left + s.px(s.grid_width(0) + width),
                y: s.body.top + s.px(12),
            };
            if !s.begin_resize(edge) {
                return Err("Management column boundary not draggable".into());
            }
            s.move_resize(POINT {
                x: edge.x + s.px(25),
                y: edge.y,
            });
            s.finish_resize(true);
            if s.grid_width(1) != width + 25 {
                return Err("Management column width not retained".into());
            }
            let id = s.grid_rows()[0].0;
            let height = s.grid_height(id);
            let edge = POINT {
                x: s.body.left + s.px(20),
                y: s.grid_row_top(0) + height,
            };
            if !s.begin_resize(edge) {
                return Err("Management row boundary not draggable".into());
            }
            s.move_resize(POINT {
                x: edge.x,
                y: edge.y + s.px(12),
            });
            s.finish_resize(true);
            if s.grid_height(id) != height + s.px(12) {
                return Err("Management row height not retained".into());
            }
            let path = directory.join(format!("management-{view}.csv"));
            SMOKE_FILE_PATH.with(|p| *p.borrow_mut() = Some(path.clone()));
            s.command(cmd)?;
            SMOKE_FILE_PATH.with(|p| p.borrow_mut().take());
            let rows = table::decode(&std::fs::read_to_string(path)?, ',')?;
            if rows.len() != 3 || !rows[0].iter().any(|v| v == label) {
                return Err("Management CSV columns or isolation failed".into());
            }
            let mut area = RECT::default();
            unsafe {
                GetClientRect(h, &mut area)?;
            }
            smoke::capture(
                s,
                area,
                &directory.join(format!("management-grid-{view}.bmp")),
            )?;
        }
        s.case_range = Some((date("2026-01-01")?, date("2026-01-04")?));
        for view in [13, 14] {
            s.command(200 + view)?;
            s.layout();
            let stats = s.management_statistics()?;
            if stats.convergence.total != 2 || stats.convergence.daily.len() != 4 {
                return Err("Management daily statistics failed".into());
            }
            let mut area = RECT::default();
            unsafe {
                GetClientRect(h, &mut area)?;
            }
            smoke::capture(
                s,
                area,
                &directory.join(format!("management-statistics-{view}.bmp")),
            )?;
        }
        s.project = original;
        s.case_range = None;
        s.filter = TaskFilter::default();
        s.command(202)?;
        s.reload()?;
        Ok(())
    })?;
    report.push("Management native forms and multiline exchange add/edit/delete, isolated grids, inline cell edit, clipboard/fill-down, bulk attributes, column filter, column/row boundary resize, CSV menus and daily convergence/classification charts: OK".into());
    Ok(())
}

pub(super) fn demo_smoke(
    h: HWND,
    directory: &std::path::Path,
    report: &mut Vec<String>,
) -> Result<()> {
    let path = directory.join("management-demo.sqlite3");
    std::fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../demo/yyproject-management-demo.sqlite3"),
        &path,
    )?;
    APP.with(|a| -> Result<()> {
        let mut a = a.borrow_mut();
        let s = a.as_mut().unwrap();
        s.store = Store::open(&path, false)?;
        s.path = path;
        s.project = Some(s.store.get(4)?);
        s.filter = TaskFilter::default();
        s.reload()?;
        Ok(())
    })?;
    for (width, height) in [(1100, 700), (640, 480)] {
        unsafe {
            SetWindowPos(
                h,
                None,
                0,
                0,
                px(h, width),
                px(h, height),
                SWP_NOZORDER | SWP_NOACTIVATE,
            )?;
        }
        for view in [11, 12, 13, 14] {
            send(h, WM_COMMAND, 200 + view, 0);
            APP.with(|a| -> Result<()> {
                let mut a = a.borrow_mut();
                let s = a.as_mut().unwrap();
                s.layout();
                if view < 13 && s.grid_rows().len() != 6 {
                    return Err("Expanded management demo list did not show six records".into());
                }
                if view >= 13 && s.management_statistics()?.convergence.total != 6 {
                    return Err("Expanded management demo statistics failed".into());
                }
                let mut area = RECT::default();
                unsafe {
                    GetClientRect(h, &mut area)?;
                }
                smoke::capture(
                    s,
                    area,
                    &directory.join(format!("management-demo-{view}-{width}.bmp")),
                )?;
                s.y = s.content.1;
                s.layout();
                smoke::capture(
                    s,
                    area,
                    &directory.join(format!("management-demo-{view}-{width}-scrolled.bmp")),
                )?;
                s.x = 0;
                s.y = 0;
                s.layout();
                Ok(())
            })?;
        }
    }
    // Use actual ID-cell centers and the window's DPI/scroll position for row DnD.
    for view in [11, 12] {
        send(h, WM_COMMAND, 200 + view, 0);
        let (from, to, id, mut header) = APP.with(|a| -> Result<_> {
            let mut a = a.borrow_mut();
            let s = a.as_mut().unwrap();
            s.x = 0;
            s.y = 0;
            s.layout();
            let mut area = RECT::default();
            unsafe {
                GetClientRect(h, &mut area)?;
            }
            smoke::capture(
                s,
                area,
                &directory.join(format!("management-before-row-drag-{view}.bmp")),
            )?;
            let rows = s.grid_rows();
            let id = rows[1].0;
            Ok((
                POINT {
                    x: s.body.left + s.px(15),
                    y: s.grid_row_top(1) + s.grid_height(id) / 2,
                },
                POINT {
                    x: s.body.left + s.px(15),
                    y: s.grid_row_top(0) + s.grid_height(rows[0].0) / 2,
                },
                id,
                POINT {
                    x: s.body.left + s.px(80),
                    y: s.body.top + s.px(10),
                },
            ))
        })?;
        let mouse = |p: POINT| ((p.y as u32) << 16 | p.x as u16 as u32) as isize;
        send(h, WM_LBUTTONDOWN, 1, mouse(from));
        send(h, WM_MOUSEMOVE, 1, mouse(to));
        send(h, WM_LBUTTONUP, 0, mouse(to));
        smoke::pump_until(|| APP.with(|a| a.borrow().as_ref().unwrap().grid_rows()[0].0 == id))?;
        SMOKE_MENU_SIZE.with(|n| n.set(3));
        SMOKE_CONTEXT_MENU.with(|f| f.set(true));
        unsafe {
            let _ = ClientToScreen(h, &mut header);
        }
        send(h, WM_CONTEXTMENU, h.0 as usize, mouse(header));
        if SMOKE_CONTEXT_MENU.with(|f| f.replace(false)) {
            return Err("Management column right-click did not open filter menu".into());
        }
    }
    report.push("Expanded demo: 4 projects/114 records, populated management lists/statistics at two sizes and scroll positions; actual ID-cell row DnD and header filter menus: OK".into());
    Ok(())
}
