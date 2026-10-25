//! Runs only against a newly created test database and app-owned windows.
use super::*;
pub(super) fn run(h: HWND, directory: &std::path::Path) -> Result<()> {
    unsafe {
        let _ = SetWindowPos(h, None, 0, 0, 1100, 700, SWP_NOZORDER | SWP_NOACTIVATE);
    }
    APP.with(|a| -> Result<()> {
        let mut a = a.borrow_mut();
        let s = a.as_mut().unwrap();
        let day = chrono::Local::now().date_naive();
        let mut p = s.store.create(
            "UI検証プロジェクト".into(),
            day,
            day + chrono::Days::new(30),
            "UI-test",
        )?;
        for title in ["最初のタスク", "後続タスク"] {
            p = s.store.apply(
                p.id,
                p.revision,
                &Change::SaveTask {
                    task: Task {
                        title: title.into(),
                        kind: if title == "後続タスク" {
                            Kind::Issue
                        } else {
                            Kind::Task
                        },
                        duration_days: 2,
                        estimate: 16.0,
                        ..Default::default()
                    },
                },
                "UI-test",
            )?;
        }
        p = s.store.apply(
            p.id,
            p.revision,
            &Change::AddDependency {
                dependency: Dependency {
                    predecessor: 1,
                    successor: 2,
                    lag_days: 0,
                },
            },
            "UI-test",
        )?;
        p = s
            .store
            .apply(p.id, p.revision, &Change::Schedule, "UI-test")?;
        s.project = Some(p);
        s.reload()?;
        Ok(())
    })?;
    // Exercise the actual native modal editor and its OK handler.
    send(h, WM_COMMAND, 110, 0);
    APP.with(|a| -> Result<()> {
        let a = a.borrow();
        let s = a.as_ref().unwrap();
        let p = s.project.as_ref().unwrap();
        if p.tasks.len() != 3 || !p.tasks.iter().any(|t| t.title == "UI-created-task") {
            return Err("Native task form did not commit the expected values".into());
        }
        Ok(())
    })?;
    send(h, WM_COMMAND, 130, 0);
    APP.with(|a| -> Result<()> {
        let a = a.borrow();
        if !a
            .as_ref()
            .unwrap()
            .project
            .as_ref()
            .unwrap()
            .masters
            .iter()
            .any(|m| m.name == "UI-created-master")
        {
            return Err("Native master form did not commit".into());
        }
        Ok(())
    })?;
    let (project_id, revision, task_id) = APP.with(|a| -> Result<_> {
        let a = a.borrow();
        let s = a.as_ref().unwrap();
        let current = s.project.as_ref().unwrap();
        let mut other = Store::open(&s.path, false)?;
        let mut p = other.apply(
            current.id,
            current.revision,
            &Change::Capture {
                day: chrono::Local::now().date_naive() - chrono::Days::new(2),
            },
            "AI-test",
        )?;
        let mut task = p
            .tasks
            .iter()
            .find(|t| t.title == "UI-created-task")
            .unwrap()
            .clone();
        let task_id = task.id;
        task.description = "AIからの変更\n2行目".into();
        task.progress = 35;
        task.status = Status::Doing;
        task.category = Some(p.masters[0].id);
        p = other.apply(p.id, p.revision, &Change::SaveTask { task }, "AI-test")?;
        Ok((p.id, p.revision, task_id))
    })?;
    send(h, WM_TIMER, 1, 0);
    pump_until(|| {
        APP.with(|a| {
            a.borrow()
                .as_ref()
                .unwrap()
                .project
                .as_ref()
                .is_some_and(|p| p.revision == revision)
        })
    })?;
    send(h, WM_COMMAND, 202, 0);
    let edit_point = APP.with(|a| -> Result<_> {
        let mut a = a.borrow_mut();
        let s = a.as_mut().unwrap();
        let mut area = RECT::default();
        unsafe {
            GetClientRect(h, &mut area)?;
        }
        capture(s, area, &directory.join("before-edit.bmp"))?;
        let (r, _) = s
            .hits
            .iter()
            .find(|(_, id)| *id == task_id)
            .ok_or("Task for editing was not visible")?;
        Ok((r.left + 12, (r.top + r.bottom) / 2))
    })?;
    let click_position = ((edit_point.1 as u32) << 16 | edit_point.0 as u16 as u32) as isize;
    send(h, WM_LBUTTONDOWN, 1, click_position);
    send(h, WM_LBUTTONUP, 0, click_position);
    send(h, WM_COMMAND, 112, 0);
    APP.with(|a| -> Result<()> {
        let a = a.borrow();
        let task = a
            .as_ref()
            .unwrap()
            .project
            .as_ref()
            .unwrap()
            .tasks
            .iter()
            .find(|t| t.id == task_id)
            .unwrap();
        if task.title != "UI-edited-task"
            || task.description != "AIからの変更\n2行目"
            || task.progress != 35
            || task.category.is_none()
        {
            return Err(
                "Native editor failed to preserve AI changes, multiline text or master selection"
                    .into(),
            );
        }
        Ok(())
    })?;
    let second_id = APP.with(|a| -> Result<_> {
        let a = a.borrow();
        let s = a.as_ref().unwrap();
        let mut other = Store::open(&s.path, false)?;
        let day = chrono::Local::now().date_naive();
        Ok(other
            .create(
                "空の別プロジェクト".into(),
                day,
                day + chrono::Days::new(30),
                "AI-test",
            )?
            .id)
    })?;
    send(h, WM_TIMER, 1, 0);
    for id in [second_id, project_id] {
        let (selector, index) = APP.with(|a| -> Result<_> {
            let a = a.borrow();
            let s = a.as_ref().unwrap();
            let index = s
                .projects
                .iter()
                .position(|p| p.0 == id)
                .ok_or("New project was not refreshed")?;
            Ok((s.selector, index))
        })?;
        send(selector, CB_SETCURSEL, index, 0);
        send(
            h,
            WM_COMMAND,
            (CBN_SELCHANGE as usize) << 16 | 10,
            selector.0 as isize,
        );
        pump_until(|| {
            APP.with(|a| {
                a.borrow()
                    .as_ref()
                    .unwrap()
                    .project
                    .as_ref()
                    .is_some_and(|p| p.id == id)
            })
        })?;
        APP.with(|a| -> Result<()> {
            let a = a.borrow();
            let s = a.as_ref().unwrap();
            if id == second_id && !s.network.is_empty() {
                return Err("Project switch retained another project's dependency graph".into());
            }
            if id == project_id && s.network.len() != 3 {
                return Err("Project switch did not restore the correct dependency graph".into());
            }
            Ok(())
        })?;
    }
    let mut report = vec![];
    for (width, height) in [(1100, 700), (640, 480)] {
        unsafe {
            SetWindowPos(h, None, 0, 0, width, height, SWP_NOZORDER | SWP_NOACTIVATE)?;
        }
        for (view, title) in VIEWS.iter().enumerate() {
            send(h, WM_COMMAND, 200 + view, 0);
            pump_until(|| {
                APP.with(|a| {
                    let a = a.borrow();
                    let s = a.as_ref().unwrap();
                    s.view == view && s.body.right > 0 && s.body.bottom > s.body.top
                })
            })?;
            unsafe {
                let _ = UpdateWindow(h);
            }
            APP.with(|a| -> Result<()> {
                let mut a = a.borrow_mut();
                let s = a.as_mut().unwrap();
                let mut area = RECT::default();
                unsafe {
                    GetClientRect(h, &mut area)?;
                }
                if s.body.top < 0 || s.body.bottom > area.bottom || s.body.right > area.right {
                    return Err("View bounds exceed actual client area".into());
                }
                capture(s, area, &directory.join(format!("view-{view}-{width}.bmp")))?;
                if matches!(view, 0..=4) && s.hits.is_empty() {
                    return Err(format!("View {view} did not expose any task hit targets").into());
                }
                report.push(format!(
                    "{}: {}x{} client={}x{} body={}..{} targets={}",
                    title,
                    width,
                    height,
                    area.right,
                    area.bottom,
                    s.body.top,
                    s.body.bottom,
                    s.hits.len()
                ));
                if view == 1 {
                    s.x = s.px(280);
                    s.y = s.px(10);
                    s.layout();
                    capture(
                        s,
                        area,
                        &directory.join(format!("gantt-scrolled-{width}.bmp")),
                    )?;
                    if s.hits.is_empty() {
                        return Err("Scrolling hid all Gantt row hit targets".into());
                    }
                    s.x = 0;
                    s.y = 0;
                    s.layout();
                }
                Ok(())
            })?;
        }
    }
    send(h, WM_COMMAND, 200, 0);
    unsafe {
        let _ = UpdateWindow(h);
    }
    APP.with(|a| -> Result<()> {
        let mut a = a.borrow_mut();
        let s = a.as_mut().unwrap();
        let mut area = RECT::default();
        unsafe {
            GetClientRect(h, &mut area)?;
        }
        capture(s, area, &directory.join("drag-start.bmp"))
    })?;
    let (task, from, to) = APP.with(|a| -> Result<_> {
        let a = a.borrow();
        let s = a.as_ref().unwrap();
        let (r, id) = s.hits.first().ok_or("Kanban card missing")?;
        let from = (
            (r.left.max(s.body.left) + r.right.min(s.body.right)) / 2,
            (r.top.max(s.body.top + s.px(30)) + r.bottom.min(s.body.bottom)) / 2,
        );
        let target_world = 3 * s.px(240) + s.px(30);
        Ok((
            *id,
            from,
            (
                target_world,
                s.body.top + (s.body.bottom - s.body.top).min(s.px(30)) / 2,
            ),
        ))
    })?;
    let mouse = |point: (i32, i32)| ((point.1 as u32) << 16 | (point.0 as u16 as u32)) as isize;
    send(h, WM_LBUTTONDOWN, 1, mouse(from));
    APP.with(|a| {
        let mut a = a.borrow_mut();
        let s = a.as_mut().unwrap();
        s.x = (to.0 - s.body.right + s.px(60)).max(0);
        s.layout();
    });
    let target = APP.with(|a| {
        let a = a.borrow();
        (to.0 - a.as_ref().unwrap().x, to.1)
    });
    send(h, WM_LBUTTONUP, 0, mouse(target));
    APP.with(|a| -> Result<()> {
        let a = a.borrow();
        let s = a.as_ref().unwrap();
        let t = s
            .project
            .as_ref()
            .unwrap()
            .tasks
            .iter()
            .find(|t| t.id == task)
            .unwrap();
        if t.status != Status::Done || t.progress != 100 {
            return Err(format!("Kanban drag did not persist completion: id={task} status={:?} progress={} from={from:?} target={target:?} body=({},{}..{},{}), dpi={} errors={:?}",t.status,t.progress,s.body.left,s.body.top,s.body.right,s.body.bottom,s.dpi,SMOKE_ERRORS.with(|e|e.borrow().clone())).into());
        }
        Ok(())
    })?;
    report.push("Native task/master forms, AI refresh, multiline/master preservation, project switching, eight views at two sizes, real client hit tests and kanban drag: OK".into());
    let errors = SMOKE_ERRORS.with(|e| e.borrow().clone());
    if !errors.is_empty() {
        return Err(errors.join("\n").into());
    }
    std::fs::write(directory.join("ui-smoke.txt"), report.join("\n"))?;
    Ok(())
}
fn pump_until(mut predicate: impl FnMut() -> bool) -> Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    return Err("Application closed during test".into());
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        if predicate() {
            return Ok(());
        }
        if std::time::Instant::now() > deadline {
            let detail = APP.with(|a| {
                let a = a.borrow();
                let s = a.as_ref().unwrap();
                format!(
                    "UI state did not converge: view={} dpi={} body=({},{} .. {},{}) tab rows={}",
                    s.view,
                    s.dpi,
                    s.body.left,
                    s.body.top,
                    s.body.right,
                    s.body.bottom,
                    send(s.tabs, TCM_GETROWCOUNT, 0, 0).0
                )
            });
            return Err(detail.into());
        }
        std::thread::yield_now();
    }
}
fn capture(state: &mut State, area: RECT, path: &std::path::Path) -> Result<()> {
    unsafe {
        let screen = GetDC(Some(state.hwnd));
        let dc = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, area.right.max(1), area.bottom.max(1));
        let old = SelectObject(dc, bitmap.into());
        state.paint(dc, area);
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: area.right,
                biHeight: -area.bottom,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0u8; (area.right * area.bottom * 4) as usize];
        SelectObject(dc, old);
        let rows = GetDIBits(
            screen,
            bitmap,
            0,
            area.bottom as u32,
            Some(pixels.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        );
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(dc);
        ReleaseDC(Some(state.hwnd), screen);
        if rows != area.bottom
            || !pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[0] != 255 || p[1] != 255 || p[2] != 255)
        {
            return Err("Native rendering produced no pixel output".into());
        }
        std::fs::write(
            path,
            yy_ui_win::util::encode_bmp(area.right as u32, area.bottom as u32, &pixels),
        )?;
        Ok(())
    }
}
