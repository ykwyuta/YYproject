//! Typed native task grid; selection/TSV/clipboard conventions follow yysheet.
use super::*;
#[derive(Default)]
pub(super) struct GridState {
    pub anchor: (usize, usize),
    pub current: (usize, usize),
    pub selecting: bool,
    pub selection: bool,
    pub all_kinds: bool,
    pub new_parent: Option<i64>,
    edit: Option<CellEdit>,
}
struct CellEdit {
    hwnd: HWND,
    id: i64,
    column: usize,
    revision: i64,
}
impl GridState {
    pub fn contains(&self, row: usize, col: usize) -> bool {
        self.selection
            && row >= self.anchor.0.min(self.current.0)
            && row <= self.anchor.0.max(self.current.0)
            && col >= self.anchor.1.min(self.current.1)
            && col <= self.anchor.1.max(self.current.1)
    }
}
unsafe extern "system" fn edit_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        let old: WNDPROC = std::mem::transmute(GetWindowLongPtrW(h, GWLP_USERDATA));
        if m == WM_KEYDOWN
            && (w.0 == VK_ESCAPE.0 as usize
                || w.0 == VK_TAB.0 as usize
                || (w.0 == VK_RETURN.0 as usize && GetKeyState(VK_SHIFT.0 as i32) >= 0))
        {
            if let Ok(parent) = GetParent(h) {
                let _ = PostMessageW(
                    Some(parent),
                    GRID_FINISH,
                    WPARAM(usize::from(w.0 != VK_ESCAPE.0 as usize)),
                    LPARAM(isize::from(w.0 == VK_TAB.0 as usize)),
                );
            }
            return LRESULT(0);
        }
        if m == WM_CHAR
            && (w.0 == VK_ESCAPE.0 as usize
                || w.0 == VK_TAB.0 as usize
                || (w.0 == VK_RETURN.0 as usize && GetKeyState(VK_SHIFT.0 as i32) >= 0))
        {
            return LRESULT(0);
        }
        CallWindowProcW(old, h, m, w, l)
    }
}
impl State {
    pub(super) fn grid_rows(&self) -> Vec<(i64, usize)> {
        self.project
            .as_ref()
            .map(|p| {
                p.ordered_tasks()
                    .into_iter()
                    .filter(|(t, _)| p.matches_filter(&self.filter, t))
                    .filter(|(t, _)| {
                        self.grid.all_kinds
                            || t.kind
                                == if self.view == 3 {
                                    Kind::Issue
                                } else {
                                    Kind::Task
                                }
                    })
                    .map(|(t, d)| (t.id, d))
                    .collect()
            })
            .unwrap_or_default()
    }
    pub(super) fn grid_column(&self, x: i32) -> Option<usize> {
        let mut left = self.body.left - self.x;
        for (i, c) in table::COLUMNS.iter().enumerate() {
            let right = left + self.px(c.width);
            if x >= left && x < right {
                return Some(i);
            }
            left = right;
        }
        None
    }
    fn grid_cell(&self, point: POINT) -> Option<(usize, usize)> {
        if !inside(self.body, point.x, point.y) || point.y < self.body.top + self.px(30) {
            return None;
        }
        let row = ((point.y - self.body.top + self.y) / self.px(30) - 1) as usize;
        (row < self.grid_rows().len()).then_some((row, self.grid_column(point.x)?))
    }
    pub(super) fn select_grid_point(&mut self, point: POINT, extend: bool) {
        if let Some(cell) = self.grid_cell(point) {
            if !extend || !self.grid.selection {
                self.grid.anchor = cell;
            }
            if !self.grid.selecting {
                unsafe {
                    let _ = SetCapture(self.hwnd);
                }
            }
            self.grid.current = cell;
            self.grid.selection = true;
            self.grid.selecting = true;
            self.selected = self.grid_rows().get(cell.0).map(|r| r.0);
            invalidate(self.hwnd);
        }
    }
    fn grid_rect(&self) -> RECT {
        let (row, col) = self.grid.current;
        rect(
            self.body.left - self.x + self.px(table::COLUMNS[..col].iter().map(|c| c.width).sum()),
            self.body.top - self.y + (row as i32 + 1) * self.px(30),
            self.px(table::COLUMNS[col].width),
            self.px(30),
        )
    }
    fn grid_move(&mut self, dr: i32, dc: i32, extend: bool) {
        let count = self.grid_rows().len();
        if count == 0 {
            return;
        }
        self.grid.current.0 = (self.grid.current.0 as i32 + dr).clamp(0, count as i32 - 1) as usize;
        self.grid.current.1 =
            (self.grid.current.1 as i32 + dc).clamp(0, table::COLUMNS.len() as i32 - 1) as usize;
        if !extend {
            self.grid.anchor = self.grid.current;
        }
        self.grid.selection = true;
        self.selected = Some(self.grid_rows()[self.grid.current.0].0);
        let r = self.grid_rect();
        let header = self.body.top + self.px(30);
        if r.top < header {
            self.y -= header - r.top;
        }
        if r.bottom > self.body.bottom {
            self.y += r.bottom - self.body.bottom;
        }
        if r.left < self.body.left {
            self.x -= self.body.left - r.left;
        }
        if r.right > self.body.right {
            self.x += r.right - self.body.right;
        }
        self.layout();
        invalidate(self.hwnd);
    }
    pub(super) fn begin_cell_edit(&mut self, initial: Option<char>) -> Result<()> {
        if self.grid.edit.is_some() {
            return Ok(());
        }
        let rows = self.grid_rows();
        let (row, col) = self.grid.current;
        let Some((id, _)) = rows.get(row) else {
            return Ok(());
        };
        let c = &table::COLUMNS[col];
        if !c.editable {
            return Ok(());
        }
        let p = self.project.as_ref().unwrap();
        let t = p.tasks.iter().find(|t| t.id == *id).unwrap();
        let value = initial
            .map(|c| c.to_string())
            .unwrap_or_else(|| table::value(p, t, c.key));
        let r = self.grid_rect();
        let revision = p.revision;
        unsafe {
            let h = CreateWindowExW(
                WS_EX_CLIENTEDGE,
                w!("EDIT"),
                &HSTRING::from(value),
                WS_CHILD
                    | WS_VISIBLE
                    | WS_TABSTOP
                    | WINDOW_STYLE((ES_AUTOHSCROLL | ES_MULTILINE) as u32),
                r.left.max(self.body.left),
                r.top.max(self.body.top + self.px(30)),
                (r.right.min(self.body.right) - r.left.max(self.body.left)).max(self.px(80)),
                self.px(30),
                Some(self.hwnd),
                Some(HMENU(std::ptr::without_provenance_mut(501))),
                None,
                None,
            )?;
            send(h, WM_SETFONT, self.font.0 as usize, 1);
            let old = SetWindowLongPtrW(h, GWLP_WNDPROC, edit_proc as *const () as isize);
            SetWindowLongPtrW(h, GWLP_USERDATA, old);
            self.grid.edit = Some(CellEdit {
                hwnd: h,
                id: *id,
                column: col,
                revision,
            });
            let _ = SetFocus(Some(h));
            send(
                h,
                EM_SETSEL,
                if initial.is_some() { usize::MAX } else { 0 },
                -1,
            );
        }
        Ok(())
    }
    pub(super) fn finish_cell_edit(&mut self, commit: bool) -> Result<()> {
        if commit && let Some(edit) = &self.grid.edit {
            let p = self.project.as_ref().ok_or("Project is no longer open")?;
            if p.revision != edit.revision {
                return Err("編集中に変更されています。Escで取り消し、再読込してください".into());
            }
            let mut task = p
                .tasks
                .iter()
                .find(|t| t.id == edit.id)
                .ok_or("Task no longer exists")?
                .clone();
            table::set_value(
                p,
                &mut task,
                table::COLUMNS[edit.column].key,
                &text(edit.hwnd),
            )?;
            if serde_json::to_value(&task)?
                != serde_json::to_value(p.tasks.iter().find(|t| t.id == edit.id).unwrap())?
            {
                self.project = Some(self.store.apply(
                    p.id,
                    edit.revision,
                    &Change::SaveTask { task },
                    "GUI cell",
                )?);
            }
        }
        if let Some(edit) = self.grid.edit.take() {
            unsafe {
                let _ = DestroyWindow(edit.hwnd);
                let _ = SetFocus(Some(self.hwnd));
            }
            self.reload()?;
        }
        Ok(())
    }
    pub(super) fn focus_cell_edit(&self) {
        if let Some(edit) = &self.grid.edit {
            unsafe {
                let _ = SetFocus(Some(edit.hwnd));
            }
        }
    }
    pub(super) fn finish_grid_message(&mut self, commit: bool, next: bool) -> Result<()> {
        self.finish_cell_edit(commit)?;
        if next {
            self.grid_move(0, 1, false);
        }
        Ok(())
    }
    pub(super) fn grid_copy_text(&self) -> String {
        let Some(p) = &self.project else {
            return String::new();
        };
        let rows = self.grid_rows();
        let (a, b) = (self.grid.anchor, self.grid.current);
        let data: Vec<_> = rows
            .iter()
            .enumerate()
            .filter(|(i, _)| *i >= a.0.min(b.0) && *i <= a.0.max(b.0))
            .map(|(_, (id, _))| {
                let t = p.tasks.iter().find(|t| t.id == *id).unwrap();
                table::COLUMNS
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i >= a.1.min(b.1) && *i <= a.1.max(b.1))
                    .map(|(_, c)| table::value(p, t, c.key))
                    .collect()
            })
            .collect();
        table::encode(&data, '\t')
    }
    pub(super) fn grid_paste(&mut self, text: &str) -> Result<()> {
        self.finish_cell_edit(true)?;
        let data = table::decode(text, '\t')?;
        if data.is_empty() {
            return Ok(());
        }
        let rows = self.grid_rows();
        let p = self
            .project
            .as_ref()
            .ok_or("先にプロジェクトを開いてください")?;
        let (row, col) = (
            self.grid.anchor.0.min(self.grid.current.0),
            self.grid.anchor.1.min(self.grid.current.1),
        );
        if row + data.len() > rows.len()
            || data.iter().any(|r| col + r.len() > table::COLUMNS.len())
        {
            return Err("貼り付け範囲が一覧の行・列を超えています".into());
        }
        let mut tasks = Vec::new();
        for (offset, values) in data.iter().enumerate() {
            let mut t = p
                .tasks
                .iter()
                .find(|t| t.id == rows[row + offset].0)
                .unwrap()
                .clone();
            for (i, v) in values.iter().enumerate() {
                let c = &table::COLUMNS[col + i];
                if !c.editable {
                    if table::value(p, &t, c.key) != *v {
                        return Err("ID・カテゴリ階層列は読み取り専用です".into());
                    }
                } else {
                    table::set_value(p, &mut t, c.key, v)?;
                }
            }
            tasks.push(t);
        }
        self.apply(Change::SaveTasks { tasks })?;
        Ok(())
    }
    fn grid_clear(&mut self) -> Result<()> {
        let Some(p) = &self.project else {
            return Ok(());
        };
        let rows = self.grid_rows();
        let (a, b) = (self.grid.anchor, self.grid.current);
        let mut tasks = Vec::new();
        for (_, (id, _)) in rows
            .iter()
            .enumerate()
            .filter(|(i, _)| *i >= a.0.min(b.0) && *i <= a.0.max(b.0))
        {
            let mut t = p.tasks.iter().find(|t| t.id == *id).unwrap().clone();
            for (_, c) in table::COLUMNS
                .iter()
                .enumerate()
                .filter(|(i, c)| c.editable && *i >= a.1.min(b.1) && *i <= a.1.max(b.1))
            {
                table::set_value(p, &mut t, c.key, &table::value(p, &Task::default(), c.key))?;
            }
            tasks.push(t);
        }
        if !tasks.is_empty() {
            self.apply(Change::SaveTasks { tasks })?;
        }
        Ok(())
    }
    pub(super) fn grid_key(&mut self, key: u16) -> Option<Result<()>> {
        let ctrl = unsafe { GetKeyState(VK_CONTROL.0 as i32) < 0 };
        let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) < 0 };
        if ctrl && key == b'F' as u16 {
            return Some(self.search_form());
        }
        if !matches!(self.view, 2 | 3) {
            return None;
        }
        let result = if key == VK_DELETE.0 {
            self.grid_clear()
        } else if ctrl && key == b'C' as u16 {
            if !SMOKE.with(|s| s.get())
                && !yy_ui_win::clipboard::set_text(self.hwnd, &self.grid_copy_text(), true)
            {
                Err("クリップボードへコピーできません".into())
            } else {
                Ok(())
            }
        } else if ctrl && key == b'V' as u16 {
            if let Some((text, _)) = yy_ui_win::clipboard::get_text(self.hwnd) {
                self.grid_paste(&text)
            } else {
                Err("クリップボードにテキストがありません".into())
            }
        } else if ctrl && key == b'A' as u16 {
            self.grid.anchor = (0, 0);
            self.grid.current = (
                self.grid_rows().len().saturating_sub(1),
                table::COLUMNS.len() - 1,
            );
            self.grid.selection = true;
            invalidate(self.hwnd);
            Ok(())
        } else if key == VK_F2.0 || key == VK_RETURN.0 {
            if key == VK_RETURN.0 && unsafe { GetKeyState(VK_MENU.0 as i32) < 0 } {
                self.command(112)
            } else {
                self.begin_cell_edit(None)
            }
        } else if [
            VK_LEFT.0, VK_RIGHT.0, VK_UP.0, VK_DOWN.0, VK_TAB.0, VK_PRIOR.0, VK_NEXT.0,
        ]
        .contains(&key)
        {
            let page = ((self.body.bottom - self.body.top) / self.px(30) - 1).max(1);
            let (dr, dc) = match key {
                k if k == VK_LEFT.0 => (0, -1),
                k if k == VK_RIGHT.0 => (0, 1),
                k if k == VK_UP.0 => (-1, 0),
                k if k == VK_DOWN.0 => (1, 0),
                k if k == VK_PRIOR.0 => (-page, 0),
                k if k == VK_NEXT.0 => (page, 0),
                _ => (0, if shift { -1 } else { 1 }),
            };
            self.grid_move(dr, dc, shift && key != VK_TAB.0);
            Ok(())
        } else {
            return None;
        };
        Some(result)
    }
    pub(super) fn column_filter_menu(&mut self, point: POINT) -> Result<()> {
        let Some(col) = self.grid_column(point.x) else {
            return Ok(());
        };
        let c = &table::COLUMNS[col];
        let p = self
            .project
            .as_ref()
            .ok_or("先にプロジェクトを開いてください")?;
        let values: Vec<_> = p
            .tasks
            .iter()
            .filter(|t| !t.archived)
            .map(|t| table::value(p, t, c.key))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .take(500)
            .collect();
        unsafe {
            let menu = CreatePopupMenu()?;
            let result = (|| -> Result<usize> {
                AppendMenuW(menu, MF_STRING, 1, w!("この列の絞り込みを解除"))?;
                AppendMenuW(menu, MF_STRING, 2, w!("値を入力して絞り込み…"))?;
                for (i, value) in values.iter().enumerate() {
                    AppendMenuW(
                        menu,
                        MF_STRING,
                        10 + i,
                        &HSTRING::from(if value.is_empty() {
                            "（空白）".into()
                        } else {
                            value
                                .replace(['\r', '\n'], " ↵ ")
                                .chars()
                                .take(100)
                                .collect::<String>()
                                .replace('&', "&&")
                        }),
                    )?;
                }
                let mut screen = point;
                let _ = ClientToScreen(self.hwnd, &mut screen);
                Ok(TrackPopupMenu(
                    menu,
                    TPM_RETURNCMD | TPM_RIGHTBUTTON,
                    screen.x,
                    screen.y,
                    None,
                    self.hwnd,
                    None,
                )
                .0 as usize)
            })();
            let _ = DestroyMenu(menu);
            match result? {
                1 => {
                    self.filter.columns.remove(c.key);
                }
                2 => {
                    if let Some(v) = form(
                        self.hwnd,
                        &format!("{}で絞り込み", c.label),
                        vec![field(
                            "一致する値",
                            self.filter.columns.get(c.key).cloned().unwrap_or_default(),
                            0,
                        )],
                        |_| Ok(()),
                    ) {
                        self.filter.columns.insert(c.key.into(), v[0].clone());
                    }
                }
                id if id >= 10 => {
                    if let Some(value) = values.get(id - 10) {
                        self.filter.columns.insert(c.key.into(), value.clone());
                    }
                }
                _ => return Ok(()),
            }
        }
        self.y = 0;
        self.grid.selection = false;
        self.layout();
        invalidate(self.hwnd);
        Ok(())
    }
    pub(super) fn search_form(&mut self) -> Result<()> {
        if let Some(v) = form(
            self.hwnd,
            "タスクの全文検索",
            vec![field(
                "検索語（件名・説明・メモ・担当者・カテゴリなど）",
                &self.filter.query,
                0,
            )],
            |v| {
                if v[0].len() <= 4096 {
                    Ok(())
                } else {
                    Err("検索語は4096バイト以内で指定してください".into())
                }
            },
        ) {
            self.filter.query = v[0].clone();
            self.view = 2;
            self.grid.all_kinds = true;
            self.grid.selection = false;
            self.x = 0;
            self.y = 0;
            send(self.tabs, TCM_SETCURSEL, 2, 0);
            self.layout();
            invalidate(self.hwnd);
        }
        Ok(())
    }
    pub(super) fn export_csv_form(&mut self) -> Result<()> {
        let p = self
            .project
            .as_ref()
            .ok_or("プロジェクトを選択してください")?;
        let choices = |kind| {
            std::iter::once((String::new(), "すべて".into()))
                .chain(p.masters.iter().filter(|m| m.kind == kind).map(|m| {
                    (
                        m.id.to_string(),
                        if kind == MasterKind::Category {
                            p.category_path(Some(m.id)).join(" / ")
                        } else {
                            m.name.clone()
                        },
                    )
                }))
                .collect()
        };
        let mut fields = vec![
            field("検索語（現在の列フィルターも適用）", &self.filter.query, 0),
            choice(
                "カテゴリ",
                self.filter
                    .category
                    .map(|i| i.to_string())
                    .unwrap_or_default(),
                choices(MasterKind::Category),
                0,
            ),
            choice(
                "担当者",
                self.filter
                    .assignee
                    .map(|i| i.to_string())
                    .unwrap_or_default(),
                choices(MasterKind::Assignee),
                0,
            ),
            choice(
                "状態",
                self.filter
                    .status
                    .map(|s| {
                        serde_json::to_value(s)
                            .unwrap()
                            .as_str()
                            .unwrap()
                            .to_owned()
                    })
                    .unwrap_or_default(),
                std::iter::once((String::new(), "すべて".into()))
                    .chain(Status::ALL.iter().map(|s| {
                        (
                            serde_json::to_value(s).unwrap().as_str().unwrap().into(),
                            s.label().into(),
                        )
                    }))
                    .collect(),
                0,
            ),
            choice(
                "種類",
                "all",
                vec![
                    ("all".into(), "タスクと課題".into()),
                    ("task".into(), "タスク".into()),
                    ("issue".into(), "課題".into()),
                ],
                0,
            ),
        ];
        for (i, c) in table::COLUMNS.iter().enumerate() {
            fields.push(choice(
                c.label,
                "yes",
                vec![
                    ("yes".into(), "出力する".into()),
                    ("no".into(), "出力しない".into()),
                ],
                1 + i / 6,
            ));
        }
        if let Some(v) = form(self.hwnd, "CSV出力 — 列と絞り込み", fields, |v| {
            if v[5..].iter().any(|s| s == "yes") {
                Ok(())
            } else {
                Err("出力する列を1つ以上選んでください".into())
            }
        }) {
            let mut filter = self.filter.clone();
            filter.query = v[0].clone();
            filter.category = v[1].parse().ok();
            filter.assignee = v[2].parse().ok();
            filter.status = if v[3].is_empty() {
                None
            } else {
                Some(serde_json::from_value(serde_json::Value::String(
                    v[3].clone(),
                ))?)
            };
            filter.kind = match v[4].as_str() {
                "task" => Some(Kind::Task),
                "issue" => Some(Kind::Issue),
                _ => None,
            };
            let keys = table::COLUMNS
                .iter()
                .zip(&v[5..])
                .filter(|(_, v)| *v == "yes")
                .map(|(c, _)| c.key.into())
                .collect::<Vec<_>>();
            let csv = table::export_csv(self.project.as_ref().unwrap(), &filter, &keys)?;
            if let Some(path) = file_dialog(self.hwnd, true, true) {
                if path
                    .canonicalize()
                    .ok()
                    .is_some_and(|d| self.path.canonicalize().ok().as_ref() == Some(&d))
                {
                    return Err("使用中のDBには出力できません".into());
                }
                std::fs::write(path, csv)?;
                self.status = "CSVへ出力しました".into();
            }
        }
        Ok(())
    }
    pub(super) fn export_jira_form(&mut self) -> Result<()> {
        let p = self
            .project
            .as_ref()
            .ok_or("プロジェクトを選択してください")?;
        let options = table::JiraOptions::default();
        let mut fields = vec![
            field("検索語（現在の絞り込みも適用）", &self.filter.query, 0),
            choice(
                "Jiraの階層",
                "epic",
                vec![
                    ("epic".into(), "親=Epic / 子=Task / 孫=Sub-task".into()),
                    ("task".into(), "親=Task / 子=Sub-task".into()),
                ],
                0,
            ),
            field("Epicの課題タイプ名", &options.epic_type, 1),
            field("Taskの課題タイプ名", &options.task_type, 1),
            field("Sub-taskの課題タイプ名", &options.subtask_type, 1),
            field("課題のタイプ名", &options.issue_type, 1),
        ];
        for (key, header) in table::JIRA_COLUMNS {
            fields.push(choice(
                header,
                if *key == "assignee" { "no" } else { "yes" },
                vec![
                    ("yes".into(), "出力する".into()),
                    ("no".into(), "出力しない".into()),
                ],
                2,
            ));
        }
        let p = p.clone();
        let current_filter = self.filter.clone();
        let build = move |v: &[String]| {
            let mut filter = current_filter.clone();
            filter.query = v[0].clone();
            let options = table::JiraOptions {
                epic_roots: v[1] == "epic",
                epic_type: v[2].trim().into(),
                task_type: v[3].trim().into(),
                subtask_type: v[4].trim().into(),
                issue_type: v[5].trim().into(),
            };
            let keys = table::JIRA_COLUMNS
                .iter()
                .zip(&v[6..])
                .filter(|(_, v)| *v == "yes")
                .map(|((key, _), _)| (*key).into())
                .collect::<Vec<_>>();
            table::export_jira_csv(&p, &filter, &keys, &options)
        };
        let validate = build.clone();
        if let Some(v) = form(
            self.hwnd,
            "Jira用CSV出力 — 親も含めて出力",
            fields,
            move |v| validate(v).map(|_| ()).map_err(|e| e.to_string()),
        ) {
            let csv = build(&v)?;
            if let Some(path) = file_dialog(self.hwnd, true, true) {
                if path
                    .canonicalize()
                    .ok()
                    .is_some_and(|d| self.path.canonicalize().ok().as_ref() == Some(&d))
                {
                    return Err("使用中のDBには出力できません".into());
                }
                std::fs::write(path, csv)?;
                self.status =
                    "Jira用CSVへ出力しました（親を含む・取り込み時に項目と値を対応付け）".into();
            }
        }
        Ok(())
    }
}
