//! Per-project task outline; folding never changes persisted tasks or exports.
use super::*;
pub(super) struct Painter<'a> {
    pub collapsed: &'a std::collections::BTreeSet<i64>,
    pub disclosures: &'a mut Vec<(RECT, i64)>,
    pub show_id: bool,
}
pub(super) fn paint_title(
    dc: HDC,
    p: &Project,
    t: &Task,
    depth: usize,
    area: RECT,
    v: &mut Painter<'_>,
    dpi: i32,
) {
    let saved = unsafe { SaveDC(dc) };
    unsafe {
        IntersectClipRect(dc, area.left, area.top, area.right, area.bottom);
    }
    let px = |n| n * dpi / 96;
    let children = p
        .tasks
        .iter()
        .filter(|c| c.parent == Some(t.id) && c.archived == t.archived)
        .count();
    let indent = px(depth.min(12) as i32 * 18);
    let center = (area.top + area.bottom) / 2;
    for level in 0..depth.min(12) {
        let x = area.left + px(level as i32 * 18 + 9);
        line(dc, (x, area.top), (x, area.bottom), COLORREF(0xdfc5a6));
    }
    if depth > 0 {
        line(
            dc,
            (area.left + indent - px(9), center),
            (area.left + indent, center),
            COLORREF(0xdfc5a6),
        );
    }
    let toggle = rect(
        area.left + indent,
        area.top + px(3),
        px(22),
        (area.bottom - area.top - px(6)).max(1),
    );
    if children > 0 {
        label(
            dc,
            toggle,
            if v.collapsed.contains(&t.id) {
                "▶"
            } else {
                "▼"
            },
            shell::BLUE,
        );
        if toggle.right <= area.right {
            v.disclosures.push((toggle, t.id));
        }
    }
    let value = format!(
        "{}{}{}{}{}",
        if v.show_id {
            format!("#{} ", t.id)
        } else {
            String::new()
        },
        t.parent
            .map(|id| format!("[親 #{id}] "))
            .unwrap_or_default(),
        if t.milestone { "◆ " } else { "" },
        t.title,
        if children > 0 {
            format!("  · 子{children}件")
        } else {
            String::new()
        }
    );
    label(
        dc,
        rect(
            toggle.right,
            area.top,
            (area.right - toggle.right).max(1),
            area.bottom - area.top,
        ),
        &value,
        if children > 0 {
            shell::BLUE
        } else {
            shell::INK
        },
    );
    unsafe {
        let _ = RestoreDC(dc, saved);
    }
}
impl State {
    pub(super) fn collapsed_tasks(&self) -> std::collections::BTreeSet<i64> {
        self.project
            .as_ref()
            .and_then(|p| self.collapsed.get(&p.id))
            .cloned()
            .unwrap_or_default()
    }
    pub(super) fn outline_rows(&self) -> Vec<(&Task, usize)> {
        self.project
            .as_ref()
            .map(|p| {
                table::outline_rows(p, &self.filter, Some(Kind::Task), &self.collapsed_tasks())
            })
            .unwrap_or_default()
    }
    pub(super) fn toggle_outline(&mut self, id: i64) {
        let Some(p) = &self.project else { return };
        if !p
            .tasks
            .iter()
            .any(|t| t.parent == Some(id) && t.archived == self.filter.archived)
        {
            return;
        }
        let folded = self.collapsed.entry(p.id).or_default();
        if !folded.remove(&id) {
            folded.insert(id);
        }
        self.grid.selected_rows.clear();
        self.grid.selection = false;
        self.grid.row_selection = false;
        self.selected = Some(id);
        if let Some(index) = self.grid_rows().iter().position(|(value, _)| *value == id) {
            self.grid.current = (
                index,
                self.grid_columns()
                    .iter()
                    .position(|c| c.key == "title")
                    .unwrap_or(0),
            );
            self.grid.anchor = self.grid.current;
        }
        self.layout();
        invalidate(self.hwnd);
    }
}

pub(super) fn smoke_test(
    h: HWND,
    dir: &std::path::Path,
    report: &mut Vec<String>,
    ids: [i64; 3],
) -> Result<()> {
    let before = APP.with(|a| {
        serde_json::to_value(a.borrow().as_ref().unwrap().project.as_ref().unwrap()).unwrap()
    });
    for view in [202, 201] {
        send(h, WM_COMMAND, view, 0);
        for id in [ids[1], ids[0], ids[0], ids[1]] {
            let point = APP.with(|a| -> Result<POINT> {
                let mut a = a.borrow_mut();
                let s = a.as_mut().unwrap();
                let mut area = RECT::default();
                unsafe {
                    GetClientRect(h, &mut area)?;
                }
                smoke::capture(s, area, &dir.join(format!("outline-{view}.bmp")))?;
                let (r, _) = s
                    .outline_hits
                    .iter()
                    .find(|(_, value)| *value == id)
                    .ok_or("Outline disclosure not visible")?;
                Ok(POINT {
                    x: (r.left + r.right) / 2,
                    y: (r.top + r.bottom) / 2,
                })
            })?;
            let l = ((point.y as u32) << 16 | point.x as u16 as u32) as isize;
            send(h, WM_LBUTTONDOWN, 1, l);
            send(h, WM_LBUTTONUP, 0, l);
            APP.with(|a| -> Result<()> {
                let a = a.borrow();
                let s = a.as_ref().unwrap();
                let folded = s.collapsed_tasks();
                let visible: std::collections::BTreeSet<_> = if view == 202 {
                    s.grid_rows().iter().map(|r| r.0).collect()
                } else {
                    s.outline_rows().iter().map(|(t, _)| t.id).collect()
                };
                let child_expected = !folded.contains(&ids[0]);
                let grand_expected = child_expected && !folded.contains(&ids[1]);
                if visible.contains(&ids[1]) != child_expected
                    || visible.contains(&ids[2]) != grand_expected
                {
                    return Err("Outline folding did not preserve nested branch state".into());
                }
                if s.grid.selected_rows.iter().any(|id| !visible.contains(id)) {
                    return Err("Hidden outline row remains selected".into());
                }
                if serde_json::to_value(s.project.as_ref().unwrap())? != before {
                    return Err("Outline folding changed project data or revision".into());
                }
                Ok(())
            })?;
        }
    }
    report.push("Task outline: real disclosure clicks in grid/Gantt hide descendants, retain nested folding and restore all rows; hidden selections clear and project/revision stay unchanged: OK".into());
    Ok(())
}
