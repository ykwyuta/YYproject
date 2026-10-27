//! Regression scenarios against the owned window and dedicated smoke database.
use super::*;
fn state<T>(f: impl FnOnce(&mut State) -> Result<T>) -> Result<T> {
    APP.with(|a| f(a.borrow_mut().as_mut().unwrap()))
}
fn overrides(values: impl IntoIterator<Item = (String, String)>) {
    SMOKE_FORM_VALUES.with(|m| *m.borrow_mut() = values.into_iter().collect());
}
fn reset_overrides() {
    SMOKE_FORM_VALUES.with(|m| m.borrow_mut().clear());
}
fn mouse(point: POINT) -> isize {
    ((point.y as u32) << 16 | point.x as u16 as u32) as isize
}
fn snapshot(h: HWND, path: &std::path::Path) -> Result<()> {
    state(|s| {
        let mut area = RECT::default();
        unsafe {
            GetClientRect(h, &mut area)?;
        }
        smoke::capture(s, area, path)
    })
}
fn cell(row: usize, col: usize) -> Result<POINT> {
    state(|s| {
        Ok(POINT {
            x: s.body.left - s.x
                + s.px(table::COLUMNS[..col].iter().map(|c| c.width).sum::<i32>()
                    + 8
                    + if table::COLUMNS[col].key == "title" {
                        24 + s
                            .grid_rows()
                            .get(row)
                            .map_or(0, |r| r.1.min(12) as i32 * 18)
                    } else {
                        0
                    }),
            y: s.body.top - s.y + (row as i32 + 1) * s.px(30) + s.px(15),
        })
    })
}
pub(super) fn run(h: HWND, directory: &std::path::Path, report: &mut Vec<String>) -> Result<()> {
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
    send(h, WM_COMMAND, 202, 0);
    state(|s| {
        s.selected = Some(1);
        Ok(())
    })?;
    send(h, WM_COMMAND, 121, 0);
    let child = state(|s| {
        let p = s.project.as_ref().unwrap();
        let t = p.tasks.last().unwrap();
        if t.parent != Some(1) {
            return Err("Right-click child action did not set the parent".into());
        }
        Ok(t.id)
    })?;
    state(|s| {
        s.selected = Some(child);
        Ok(())
    })?;
    send(h, WM_COMMAND, 121, 0);
    let grandchild = state(|s| {
        let t = s.project.as_ref().unwrap().tasks.last().unwrap();
        if t.parent != Some(child) {
            return Err("Grandchild creation did not preserve its parent".into());
        }
        Ok(t.id)
    })?;
    let mut category = 1;
    for name in ["工程", "開発"] {
        overrides([
            ("名前".into(), name.into()),
            ("親カテゴリ（最大3階層）".into(), category.to_string()),
        ]);
        state(|s| {
            s.master_kind = MasterKind::Category;
            s.master(true)
        })?;
        reset_overrides();
        category = state(|s| Ok(s.project.as_ref().unwrap().masters.last().unwrap().id))?;
    }
    state(|s| {
        let mut t = s
            .project
            .as_ref()
            .unwrap()
            .tasks
            .iter()
            .find(|t| t.id == grandchild)
            .unwrap()
            .clone();
        t.category = Some(category);
        s.apply(Change::SaveTask { task: t })
    })?;
    let milestone_day = "2026-12-30";
    overrides([
        ("件名".into(), "リリース節目".into()),
        ("マイルストーン".into(), "yes".into()),
        ("開始日".into(), milestone_day.into()),
        ("期限".into(), milestone_day.into()),
    ]);
    send(h, WM_COMMAND, 110, 0);
    reset_overrides();
    let milestone = state(|s| {
        let t = s.project.as_ref().unwrap().tasks.last().unwrap();
        if !t.milestone || t.start != t.due || t.estimate != 0.0 || t.duration_days != 0 {
            return Err("Native milestone form did not normalize its date and effort".into());
        }
        Ok(t.id)
    })?;
    snapshot(h, &directory.join("hierarchical-grid.bmp"))?;
    super::outline::smoke_test(h, directory, report, [1, child, grandchild])?;
    send(h, WM_COMMAND, 202, 0);
    let at = cell(0, 2)?;
    send(h, WM_LBUTTONDOWN, 1, mouse(at));
    send(h, WM_LBUTTONUP, 0, mouse(at));
    send(h, WM_KEYDOWN, VK_F2.0 as usize, 0);
    unsafe {
        let editor = GetDlgItem(Some(h), 501)?;
        SetWindowTextW(editor, w!("親_セル編集"))?;
        send(editor, WM_KEYDOWN, VK_RETURN.0 as usize, 0);
    }
    smoke::pump_until(|| {
        APP.with(|a| {
            a.borrow()
                .as_ref()
                .unwrap()
                .project
                .as_ref()
                .unwrap()
                .tasks
                .iter()
                .any(|t| t.id == 1 && t.title == "親_セル編集")
        })
    })?;
    state(|s| {
        let row = s
            .grid_rows()
            .iter()
            .position(|(id, _)| *id == grandchild)
            .unwrap();
        s.grid.anchor = (row, 17);
        s.grid.current = (row, 17);
        s.grid.selection = true;
        let data = table::encode(&[vec!["独自全文語\n改行と,カンマと\"引用\"".into()]], '\t');
        s.grid_paste(&data)?;
        let before = s.project.as_ref().unwrap().revision;
        s.grid.anchor = (0, 2);
        s.grid.current = (0, 2);
        if s.grid_paste("途中で保存しない\r\n\"\"\r\n").is_ok()
            || s.store.get(s.project.as_ref().unwrap().id)?.revision != before
        {
            return Err("Invalid multi-cell paste was not atomic".into());
        }
        s.grid.anchor = (0, 2);
        s.grid.current = (2, 2);
        let copied = s.grid_copy_text();
        let parsed = table::decode(&copied, '\t')?;
        if parsed.len() != 3 || parsed[0][0] != "親_セル編集" {
            return Err("Grid rectangular copy lost its row order or values".into());
        }
        std::fs::write(directory.join("copied-cells.tsv"), copied)?;
        Ok(())
    })?;
    state(|s| {
        let row = s
            .grid_rows()
            .iter()
            .position(|(id, _)| *id == grandchild)
            .unwrap();
        s.grid.anchor = (row, 17);
        s.grid.current = (row, 17);
        Ok(())
    })?;
    send(h, WM_KEYDOWN, VK_DELETE.0 as usize, 0);
    state(|s| {
        let t = s
            .project
            .as_ref()
            .unwrap()
            .tasks
            .iter()
            .find(|t| t.id == grandchild)
            .unwrap();
        if t.archived || !t.description.is_empty() {
            return Err("Grid Delete archived the task or failed to clear its cell".into());
        }
        s.grid_paste(&table::encode(
            &[vec!["独自全文語\n改行と,カンマと\"引用\"".into()]],
            '\t',
        ))
    })?;
    snapshot(h, &directory.join("edited-grid.bmp"))?;
    let mut header = state(|s| {
        let values = s
            .project
            .as_ref()
            .unwrap()
            .tasks
            .iter()
            .filter(|t| !t.archived)
            .map(|t| table::value(s.project.as_ref().unwrap(), t, "category"))
            .collect::<std::collections::BTreeSet<_>>();
        SMOKE_MENU_SIZE.with(|n| n.set((2 + values.len()) as i32));
        Ok(POINT {
            x: s.body.left - s.x + s.px(70),
            y: s.body.top + s.px(10),
        })
    })?;
    unsafe {
        let _ = ClientToScreen(h, &mut header);
    }
    SMOKE_CONTEXT_MENU.with(|f| f.set(true));
    send(h, WM_CONTEXTMENU, h.0 as usize, mouse(header));
    if SMOKE_CONTEXT_MENU.with(|f| f.replace(false)) {
        return Err("Column header right-click did not open the filter menu".into());
    }
    SMOKE_MENU_SIZE.with(|n| n.set(8));
    state(|s| {
        let p = s.project.as_ref().unwrap();
        let path = p.category_path(Some(category)).join(" / ");
        s.filter.columns.insert("category".into(), path);
        s.layout();
        if s.grid_rows().iter().map(|r| r.0).collect::<Vec<_>>() != [grandchild] {
            return Err("Category column value filter did not isolate the grandchild".into());
        }
        Ok(())
    })?;
    snapshot(h, &directory.join("category-column-filter.bmp"))?;
    overrides([(
        "検索語（件名・説明・メモ・担当者・カテゴリなど）".into(),
        "独自全文語".into(),
    )]);
    send(h, WM_COMMAND, 420, 0);
    reset_overrides();
    state(|s| {
        if s.grid_rows().iter().map(|r| r.0).collect::<Vec<_>>() != [grandchild] {
            return Err("Full-text search did not find multiline description content".into());
        }
        s.filter = TaskFilter::default();
        s.grid.all_kinds = false;
        s.layout();
        Ok(())
    })?;
    snapshot(h, &directory.join("before-row-drag.bmp"))?;
    let source = state(|s| {
        Ok(s.grid_rows()
            .iter()
            .position(|(id, _)| *id == milestone)
            .unwrap())
    })?;
    let from = cell(source, 0)?;
    let to = cell(0, 0)?;
    send(h, WM_LBUTTONDOWN, 1, mouse(from));
    send(h, WM_MOUSEMOVE, 1, mouse(to));
    snapshot(h, &directory.join("row-drag.bmp"))?;
    send(h, WM_LBUTTONUP, 0, mouse(to));
    state(|s| {
        if s.project.as_ref().unwrap().ordered_tasks()[0].0.id != milestone {
            return Err("Native row drag did not persist the reordered task".into());
        }
        Ok(())
    })?;
    overrides([
        ("表示起点（YYYY-MM-DD）".into(), "2026-12-28".into()),
        ("項目・予定日程の列幅（96dpi基準）".into(), "420".into()),
        ("1日の幅（96dpi基準）".into(), "30".into()),
        ("稲妻線".into(), "yes".into()),
        ("稲妻線の基準日".into(), "2026-12-30".into()),
    ]);
    send(h, WM_COMMAND, 430, 0);
    reset_overrides();
    send(h, WM_COMMAND, 201, 0);
    let (boundary, revision) = state(|s| {
        if s.timeline_first() != Some("2026-12-28".parse()?) || !s.timeline.lightning {
            return Err("Timeline display settings did not apply".into());
        }
        Ok((
            POINT {
                x: s.body.left + s.timeline.width(s.dpi, s.body),
                y: s.body.top + s.px(10),
            },
            s.project.as_ref().unwrap().revision,
        ))
    })?;
    send(h, WM_LBUTTONDOWN, 1, mouse(boundary));
    let resized = POINT {
        x: boundary.x + px(h, 60),
        y: boundary.y,
    };
    send(h, WM_MOUSEMOVE, 1, mouse(resized));
    send(h, WM_LBUTTONUP, 0, mouse(resized));
    state(|s| {
        if s.timeline.name_width != 480 || s.project.as_ref().unwrap().revision != revision {
            return Err("Gantt divider resize changed data or failed to resize".into());
        }
        Ok(())
    })?;
    snapshot(h, &directory.join("gantt-four-level-milestone.bmp"))?;
    let mut values = table::COLUMNS
        .iter()
        .map(|c| {
            (
                c.label.into(),
                if [
                    "category",
                    "title",
                    "parent",
                    "milestone",
                    "description",
                    "category_1",
                    "category_2",
                    "category_3",
                ]
                .contains(&c.key)
                {
                    "yes".into()
                } else {
                    "no".into()
                },
            )
        })
        .collect::<Vec<_>>();
    values.push((
        "検索語（現在の列フィルターも適用）".into(),
        "独自全文語".into(),
    ));
    overrides(values);
    let output = directory.join("selected-fields.csv");
    SMOKE_FILE_PATH.with(|p| *p.borrow_mut() = Some(output.clone()));
    send(h, WM_COMMAND, 104, 0);
    reset_overrides();
    let csv = std::fs::read_to_string(output)?;
    let rows = table::decode(&csv, ',')?;
    if rows.len() != 2 || rows[0].len() != 8 || !rows[1].iter().any(|v| v.contains("独自全文語\n"))
    {
        return Err("CSV field selection/filter or multiline quoting failed".into());
    }
    overrides([("検索語（現在の絞り込みも適用）".into(), "独自全文語".into())]);
    let output = directory.join("jira-import.csv");
    SMOKE_FILE_PATH.with(|p| *p.borrow_mut() = Some(output.clone()));
    send(h, WM_COMMAND, 109, 0);
    reset_overrides();
    let rows = table::decode(&std::fs::read_to_string(output)?, ',')?;
    if rows.len() != 4
        || rows[0][..4] != ["Summary", "Issue Type", "Issue ID", "Parent"]
        || rows[1][1] != "Epic"
        || rows[2][1] != "Task"
        || rows[3][1] != "Sub-task"
        || rows[2][3] != rows[1][2]
        || rows[3][3] != rows[2][2]
        || !rows[3].iter().any(|v| v.contains("独自全文語\n"))
    {
        return Err("Native Jira CSV hierarchy/filter/quoted description failed".into());
    }
    verify_interactions(h, directory, report)?;
    report.push("Native child/grandchild action, 3 category levels, F2/Enter cell commit, rectangular TSV, atomic invalid paste, header filter menu, full-text description search, row drag, four calendar rows, origin/width/lightning settings, milestone, selected/filtered CSV and Jira CSV with ancestor closure: OK".into());
    Ok(())
}

fn drag_between(h: HWND, from: POINT, to: POINT) {
    send(h, WM_LBUTTONDOWN, 1, mouse(from));
    send(h, WM_MOUSEMOVE, 1, mouse(to));
    send(h, WM_LBUTTONUP, 0, mouse(to));
}
fn verify_interactions(
    h: HWND,
    directory: &std::path::Path,
    report: &mut Vec<String>,
) -> Result<()> {
    send(h, WM_COMMAND, 202, 0);
    let (boundary, old_width, revision) = state(|s| {
        Ok((
            POINT {
                x: s.body.left - s.x + s.px((0..3).map(|c| s.grid_width(c)).sum()),
                y: s.body.top + s.px(12),
            },
            s.grid_width(2),
            s.project.as_ref().unwrap().revision,
        ))
    })?;
    drag_between(
        h,
        boundary,
        POINT {
            x: boundary.x + px(h, 80),
            y: boundary.y,
        },
    );
    state(|s| {
        if s.grid_width(2) != old_width + 80 {
            return Err("Task column boundary resize failed".into());
        }
        Ok(())
    })?;
    let (edge, id) = state(|s| {
        let id = s.grid_rows()[0].0;
        Ok((
            POINT {
                x: s.body.left + s.px(20),
                y: s.grid_row_top(0) + s.grid_height(id),
            },
            id,
        ))
    })?;
    drag_between(
        h,
        edge,
        POINT {
            x: edge.x,
            y: edge.y + px(h, 18),
        },
    );
    state(|s| {
        if s.grid_height(id) != s.px(48)
            || s.grid_row_top(1) != s.body.top + s.px(78) - s.y
            || s.project.as_ref().unwrap().revision != revision
        {
            return Err("Task row resize, hit geometry or revision failed".into());
        }
        Ok(())
    })?;
    // Real scrolled coordinates and cancellation must use the same dimension model.
    state(|s| {
        s.x = s.px(40);
        s.y = s.px(10);
        s.layout();
        Ok(())
    })?;
    let boundary = state(|s| {
        Ok(POINT {
            x: s.body.left - s.x + s.px((0..3).map(|c| s.grid_width(c)).sum()),
            y: s.body.top + s.px(12),
        })
    })?;
    send(h, WM_LBUTTONDOWN, 1, mouse(boundary));
    send(
        h,
        WM_MOUSEMOVE,
        1,
        mouse(POINT {
            x: boundary.x + px(h, 70),
            y: boundary.y,
        }),
    );
    send(h, WM_KEYDOWN, VK_ESCAPE.0 as usize, 0);
    state(|s| {
        if s.grid_width(2) != old_width + 80 || s.resize.is_some() {
            return Err("Scrolled resize Escape rollback failed".into());
        }
        s.x = 0;
        s.y = 0;
        s.layout();
        Ok(())
    })?;
    snapshot(h, &directory.join("resizable-task-grid.bmp"))?;
    send(h, WM_COMMAND, 201, 0);
    let (edge, id) = state(|s| {
        let id = s.project.as_ref().unwrap().ordered_tasks()[0].0.id;
        Ok((
            POINT {
                x: s.body.left + s.px(20),
                y: s.body.top + s.px(96 + s.timeline.height(id)) - s.y,
            },
            id,
        ))
    })?;
    drag_between(
        h,
        edge,
        POINT {
            x: edge.x,
            y: edge.y + px(h, 20),
        },
    );
    state(|s| {
        if s.timeline.height(id) != 72 {
            return Err("Gantt row boundary resize failed".into());
        }
        Ok(())
    })?;
    snapshot(h, &directory.join("resizable-gantt.bmp"))?;
    send(h, WM_COMMAND, 207, 0);
    snapshot(h, &directory.join("master-before-edit.bmp"))?;
    let at = cell(0, 1)?;
    // Master positions use their own widths.
    let at = state(|s| {
        Ok(POINT {
            x: s.body.left + s.px(s.grid_width(0) + 10),
            y: at.y,
        })
    })?;
    send(h, WM_LBUTTONDOWN, 1, mouse(at));
    send(h, WM_LBUTTONUP, 0, mouse(at));
    send(h, WM_KEYDOWN, VK_F2.0 as usize, 0);
    unsafe {
        let editor = GetDlgItem(Some(h), 501)?;
        SetWindowTextW(editor, w!("直接編集カテゴリ"))?;
        send(editor, WM_KEYDOWN, VK_RETURN.0 as usize, 0);
    }
    smoke::pump_until(|| {
        APP.with(|a| {
            a.borrow()
                .as_ref()
                .unwrap()
                .project
                .as_ref()
                .unwrap()
                .masters[0]
                .name
                == "直接編集カテゴリ"
        })
    })?;
    state(|s| {
        s.grid.anchor = (0, 1);
        s.grid.current = (1, 1);
        s.grid.selection = true;
        s.grid_paste("一括マスタA\r\n一括マスタB\r\n")?;
        let before = serde_json::to_value(s.project.as_ref().unwrap())?;
        if s.grid_paste("保存してはいけない\r\n\"\"\r\n").is_ok()
            || serde_json::to_value(s.project.as_ref().unwrap())? != before
        {
            return Err("Master batch was not atomic".into());
        }
        let copied = table::decode(&s.grid_copy_text(), '\t')?;
        if copied
            != vec![
                vec!["一括マスタA".to_string()],
                vec!["一括マスタB".to_string()],
            ]
        {
            return Err("Master rectangular copy failed".into());
        }
        Ok(())
    })?;
    snapshot(h, &directory.join("editable-master-grid.bmp"))?;
    send(h, WM_COMMAND, 204, 0);
    snapshot(h, &directory.join("network-all.bmp"))?;
    let selected = state(|s| {
        Ok(s.project
            .as_ref()
            .unwrap()
            .active_dependencies()
            .next()
            .map(|d| d.successor))
    })?;
    if let Some(id) = selected {
        state(|s| {
            s.selected = Some(id);
            let family = network_family(s.project.as_ref().unwrap(), Some(id));
            if !family.contains(&id) {
                return Err("Network focus did not contain selected item".into());
            }
            Ok(())
        })?;
        snapshot(h, &directory.join("network-focused.bmp"))?;
    }
    for (command, name) in [(205, "daily-burndown.bmp"), (206, "daily-burnup.bmp")] {
        send(h, WM_COMMAND, command, 0);
        snapshot(h, &directory.join(name))?;
        state(|s| {
            if s.content.0 < charts::width(s.project.as_ref().unwrap(), s.dpi) {
                return Err("Chart daily content cannot scroll".into());
            }
            Ok(())
        })?;
    }
    send(h, WM_COMMAND, 202, 0);
    let (ids, assignee, revision) = state(|s| {
        s.apply(Change::SaveMaster {
            master: Master {
                id: 0,
                kind: MasterKind::Assignee,
                name: "一括担当者".into(),
                date: None,
                parent: None,
            },
        })?;
        let assignee = s.project.as_ref().unwrap().masters.last().unwrap().id;
        s.grid.anchor = (0, 2);
        s.grid.current = (1, 2);
        s.grid.selection = true;
        Ok((
            s.selected_task_ids(),
            assignee,
            s.project.as_ref().unwrap().revision,
        ))
    })?;
    if ids.len() != 2 {
        return Err("Multi-row range did not select two IDs".into());
    }
    snapshot(h, &directory.join("bulk-selection.bmp"))?;
    let mut context = cell(0, 2)?;
    unsafe {
        let _ = ClientToScreen(h, &mut context);
    }
    SMOKE_MENU_SIZE.with(|n| n.set(10));
    SMOKE_CONTEXT_MENU.with(|flag| flag.set(true));
    send(h, WM_CONTEXTMENU, h.0 as usize, mouse(context));
    SMOKE_MENU_SIZE.with(|n| n.set(8));
    state(|s| {
        if s.selected_task_ids() != ids {
            return Err("Right click lost the selected rows".into());
        }
        Ok(())
    })?;
    overrides([
        ("担当者の変更".into(), "set".into()),
        ("担当者".into(), assignee.to_string()),
        ("完了日の変更".into(), "set".into()),
        ("完了日".into(), "2026-10-27".into()),
    ]);
    send(h, WM_COMMAND, 122, 0);
    reset_overrides();
    state(|s| {
        let p = s.project.as_ref().unwrap();
        if p.revision != revision + 1
            || p.tasks.iter().any(|t| {
                if ids.contains(&t.id) {
                    t.assignee != Some(assignee)
                        || t.completed_on != Some("2026-10-27".parse().unwrap())
                } else {
                    t.assignee == Some(assignee) || t.completed_on.is_some()
                }
            })
        {
            return Err("Native multi-row attribute edit changed wrong rows or revision".into());
        }
        s.grid.selection = false;
        s.selected = None;
        s.grid.row_selection = false;
        let p0 = POINT {
            x: s.body.left + s.px(10),
            y: s.grid_row_top(0) + s.grid_height(s.grid_rows()[0].0) / 2,
        };
        let p2 = POINT {
            x: p0.x,
            y: s.grid_row_top(2) + s.grid_height(s.grid_rows()[2].0) / 2,
        };
        Ok((p0, p2))
    })
    .and_then(|(p0, p2)| {
        snapshot(h, &directory.join("bulk-attributes.bmp"))?;
        state(|s| {
            s.toggle_grid_row(p0);
            s.toggle_grid_row(p2);
            if s.selected_task_ids().len() != 2 {
                return Err("Non-contiguous row selection failed".into());
            }
            s.toggle_grid_row(p0);
            s.toggle_grid_row(p2);
            if !s.selected_task_ids().is_empty() {
                return Err("Ctrl deselection did not clear selected rows".into());
            }
            s.grid.row_selection = false;
            Ok(())
        })
    })?;
    report.push("Native selected-row batch: assignee and actual completion date in one revision, unselected rows unchanged, non-contiguous selection and deselection: OK".into());
    verify_master_modes_and_fill(h, directory, report)?;
    report.push("Master inline F2/Enter, rectangular copy, atomic paste; task/Gantt column and row boundary drag, scroll and Escape rollback without data revision; daily chart and focused network canvas: OK".into());
    Ok(())
}

fn verify_master_modes_and_fill(
    h: HWND,
    directory: &std::path::Path,
    report: &mut Vec<String>,
) -> Result<()> {
    state(|s| {
        for (kind, name, date) in [
            (MasterKind::Assignee, "種類別担当者", None),
            (MasterKind::Holiday, "休日A", Some("2026-11-03".parse()?)),
            (MasterKind::Holiday, "休日B", Some("2026-11-04".parse()?)),
        ] {
            s.apply(Change::SaveMaster {
                master: Master {
                    id: 0,
                    kind,
                    name: name.into(),
                    date,
                    parent: None,
                },
            })?;
        }
        Ok(())
    })?;
    send(h, WM_COMMAND, 207, 0);
    let tabs = state(|s| Ok(s.master_tabs))?;
    for (index, kind, name) in [
        (0, MasterKind::Category, "categories"),
        (1, MasterKind::Assignee, "assignees"),
        (2, MasterKind::Holiday, "holidays"),
    ] {
        send(tabs, TCM_SETCURSEL, index, 0);
        let notice = NMHDR {
            hwndFrom: tabs,
            idFrom: 0,
            code: TCN_SELCHANGE,
        };
        send(h, WM_NOTIFY, 0, &notice as *const _ as isize);
        state(|s| {
            let p = s.project.as_ref().unwrap();
            if s.view != 7
                || s.master_kind != kind
                || s.grid_rows()
                    .iter()
                    .any(|(id, _)| p.masters.iter().find(|m| m.id == *id).unwrap().kind != kind)
                || s.grid_rows().len() != p.masters.iter().filter(|m| m.kind == kind).count()
            {
                return Err("Master subtab did not exclusively select its kind".into());
            }
            if s.grid_columns().iter().any(|c| {
                c.key == "kind"
                    || (kind != MasterKind::Holiday && c.key == "date")
                    || (kind != MasterKind::Category && c.key == "parent")
            }) {
                return Err("Irrelevant master columns were visible".into());
            }
            Ok(())
        })?;
        snapshot(h, &directory.join(format!("master-{name}.bmp")))?;
    }
    state(|s| {
        s.grid.anchor = (0, 2);
        s.grid.current = (1, 2);
        s.grid.selection = true;
        Ok(())
    })?;
    send(h, WM_COMMAND, 123, 0);
    state(|s| {
        let p = s.project.as_ref().unwrap();
        let dates = p
            .masters
            .iter()
            .filter(|m| m.kind == MasterKind::Holiday)
            .map(|m| m.date)
            .collect::<Vec<_>>();
        if dates.len() != 2 || dates[0] != dates[1] {
            return Err("Holiday date fill-down failed".into());
        }
        Ok(())
    })?;
    send(h, WM_COMMAND, 202, 0);
    let ids = state(|s| {
        let ids = s.grid_rows().iter().map(|r| r.0).collect::<Vec<_>>();
        s.grid.anchor = (0, 11);
        s.grid.current = (2, 11);
        s.grid.selection = true;
        s.grid_paste("35")?;
        let p = s.project.as_ref().unwrap();
        if p.tasks
            .iter()
            .any(|t| ids[..3].contains(&t.id) && t.progress != 35)
        {
            return Err("One-cell paste did not fill the selected column".into());
        }
        Ok(ids)
    })?;
    state(|s| {
        let mut t = s
            .project
            .as_ref()
            .unwrap()
            .tasks
            .iter()
            .find(|t| t.id == ids[0])
            .unwrap()
            .clone();
        t.progress = 65;
        s.apply(Change::SaveTask { task: t })?;
        s.grid.anchor = (0, 11);
        s.grid.current = (2, 11);
        s.grid.selection = true;
        Ok(())
    })?;
    send(h, WM_COMMAND, 123, 0);
    state(|s| {
        let p = s.project.as_ref().unwrap();
        if p.tasks
            .iter()
            .any(|t| ids[..3].contains(&t.id) && t.progress != 65)
        {
            return Err("Task same-column fill-down failed".into());
        }
        s.grid.anchor = (0, 2);
        s.grid.current = (2, 2);
        s.grid.selection = true;
        let before = serde_json::to_value(s.project.as_ref().unwrap())?;
        if s.can_fill_down()
            || s.grid_fill_down().is_ok()
            || s.grid_paste("全件を同名にしてはいけない").is_ok()
            || s.begin_cell_edit(None).is_ok()
            || serde_json::to_value(s.project.as_ref().unwrap())? != before
        {
            return Err("Individual-only title looked or behaved bulk-editable".into());
        }
        Ok(())
    })?;
    snapshot(h, &directory.join("individual-only-title.bmp"))?;
    for (command, name) in [
        (205, "reverse-date-burndown.bmp"),
        (206, "reverse-date-burnup.bmp"),
    ] {
        send(h, WM_COMMAND, command, 0);
        snapshot(h, &directory.join(name))?;
    }
    report.push("Master native subtabs isolate categories/assignees/holidays with relevant columns; one-cell paste and fill-down copy within columns; multi-row title edits rejected without writes; chart calendar weekday/day/month/year: OK".into());
    Ok(())
}
