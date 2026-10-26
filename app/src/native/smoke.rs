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
        p = s.store.apply(
            p.id,
            p.revision,
            &Change::Schedule {
                allow_overrun: false,
            },
            "UI-test",
        )?;
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
        if p.tasks.len() != 3
            || !p
                .tasks
                .iter()
                .any(|t| t.title == "UI-created-task" && t.priority == Priority::High)
        {
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
    send(h, WM_COMMAND, 301, 0);
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
        if t.status != Status::Doing || t.progress != 40 {
            return Err("Reopening dialog did not persist remaining work".into());
        }
        Ok(())
    })?;
    let revision = APP.with(|a| {
        a.borrow()
            .as_ref()
            .unwrap()
            .project
            .as_ref()
            .unwrap()
            .revision
    });
    SMOKE_CANCEL_FORM.with(|s| s.set(true));
    send(h, WM_COMMAND, 116, 0);
    APP.with(|a| -> Result<()> {
        let a = a.borrow();
        let s = a.as_ref().unwrap();
        let p = s.project.as_ref().unwrap();
        if s.store.get(p.id)?.revision != revision {
            return Err("Cancelling schedule wrote a change".into());
        }
        Ok(())
    })?;
    send(h, WM_COMMAND, 116, 0);
    APP.with(|a| -> Result<()> {
        let a = a.borrow();
        let s = a.as_ref().unwrap();
        if s.project.as_ref().unwrap().revision != revision + 1 {
            return Err("Confirmed schedule did not save".into());
        }
        Ok(())
    })?;
    report.push(
        "Native reopening/progress and schedule cancel/confirmation/read-only details: OK".into(),
    );
    report.push("Native task/master forms, AI refresh, multiline/master preservation, project switching, eight views at two sizes, real client hit tests and kanban drag: OK".into());
    let demo_path = directory.join("yyproject-demo.sqlite3");
    std::fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../demo/yyproject-demo.sqlite3"),
        &demo_path,
    )?;
    unsafe {
        SetWindowPos(h, None, 0, 0, 1100, 700, SWP_NOZORDER | SWP_NOACTIVATE)?;
    }
    APP.with(|a| -> Result<()> {
        let mut a = a.borrow_mut();
        let s = a.as_mut().unwrap();
        s.store = Store::open(&demo_path, false)?;
        s.path = demo_path;
        s.project = None;
        s.selected = None;
        s.x = 0;
        s.y = 0;
        s.reload()?;
        if s.project
            .as_ref()
            .is_none_or(|p| p.tasks.len() != 64 || p.history.len() != 8)
        {
            return Err("Bundled demo did not load into the native UI".into());
        }
        Ok(())
    })?;
    let projects = APP.with(|a| a.borrow().as_ref().unwrap().projects.clone());
    if projects.len() != 3 {
        return Err("Expected three operating scenarios".into());
    }
    for (index, (pid, _, _)) in projects.iter().enumerate() {
        let selector = APP.with(|a| a.borrow().as_ref().unwrap().selector);
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
                    .is_some_and(|p| p.id == *pid)
            })
        })?;
        for (width, height) in [(1100, 700), (640, 480)] {
            unsafe {
                SetWindowPos(h, None, 0, 0, width, height, SWP_NOZORDER | SWP_NOACTIVATE)?;
            }
            for (view, title) in VIEWS.iter().enumerate() {
                if view == 0 {
                    let tabs = APP.with(|a| {
                        let mut a = a.borrow_mut();
                        let s = a.as_mut().unwrap();
                        s.x = 0;
                        s.tabs
                    });
                    send(tabs, TCM_SETCURSEL, 0, 0);
                    let notification = NMHDR {
                        hwndFrom: tabs,
                        code: TCN_SELCHANGE,
                        ..Default::default()
                    };
                    send(h, WM_NOTIFY, 0, &notification as *const _ as isize);
                } else {
                    send(h, WM_COMMAND, 200 + view, 0);
                }
                pump_until(|| APP.with(|a| a.borrow().as_ref().unwrap().view == view))?;
                APP.with(|a| -> Result<()> {
                    let mut a=a.borrow_mut(); let s=a.as_mut().unwrap();
                    let mut area=RECT::default(); unsafe { GetClientRect(h,&mut area)?; }
                    let p=s.project.as_ref().unwrap();
                    if s.network.len()!=p.tasks.len() { return Err("Project switch used a stale graph".into()); }
                    capture(s,area,&directory.join(format!("demo-{pid}-{view}-{width}.bmp")))?;
                    if view == 0 && s.hits.is_empty() {
                        return Err("Nonempty demo project appeared empty in Kanban".into());
                    }
                    report.push(format!("Demo project={pid} view={title} width={width} visible={} content={}x{} viewport={}x{}: rendered",s.hits.len(),s.content.0,s.content.1,s.body.right-s.body.left,s.body.bottom-s.body.top));
                    Ok(())
                })?;
            }
        }
    }
    APP.with(|a| -> Result<()> {
        let mut a = a.borrow_mut();
        let s = a.as_mut().unwrap();
        s.select_project(0)?;
        s.command(203)?;
        s.command(410)?;
        s.layout();
        let mut area = RECT::default();
        unsafe {
            GetClientRect(h, &mut area)?;
        }
        capture(s, area, &directory.join("filtered-issue.bmp"))?;
        if s.hits.len() != 1 {
            return Err("Search did not isolate the selected issue".into());
        }
        s.filter = TaskFilter {
            unassigned: true,
            incomplete: true,
            ..Default::default()
        };
        s.layout();
        capture(s, area, &directory.join("filtered-unassigned-issue.bmp"))?;
        if s.hits.len() != 1 {
            return Err("Unassigned issue filter rendered incorrect rows".into());
        }
        s.command(411)?;
        capture(s, area, &directory.join("filter-cleared.bmp"))?;
        if s.hits.is_empty() {
            return Err("Clearing filters did not restore rows".into());
        }
        report.push("Search/unassigned filters, completed-only Kanban and clear action: OK".into());
        Ok(())
    })?;
    unsafe {
        SetWindowPos(h, None, 0, 0, 1100, 700, SWP_NOZORDER | SWP_NOACTIVATE)?;
    }
    APP.with(|a| -> Result<()> {
        let mut a=a.borrow_mut();let s=a.as_mut().unwrap();
        s.command(411)?;s.command(202)?;
        let before=s.project.as_ref().unwrap().tasks.iter().find(|t|t.title.starts_with("[PRJ-03]")).unwrap().clone();
        s.selected=Some(before.id);
        s.edit_task(false,false)?;
        let edited=s.project.as_ref().unwrap().tasks.iter().find(|t|t.id==before.id).unwrap();
        if edited.review!=ReviewState::Pending || edited.reviewer!=before.reviewer || edited.review_note!=before.review_note || edited.priority!=Priority::High { return Err("Native task form lost review fields".into()); }
        s.filter=TaskFilter{review:Some(ReviewState::Pending),..Default::default()};s.layout();
        let mut area=RECT::default();unsafe{GetClientRect(h,&mut area)?;}
        capture(s,area,&directory.join("review-pending.bmp"))?;
        if s.hits.len()!=1 {return Err("Review filter did not render pending task".into());}
        s.x=s.px(400);s.layout();capture(s,area,&directory.join("review-columns.bmp"))?;
        if s.hits.len()!=1 {return Err("Scrolling review columns hid the pending task".into());}
        s.x=0;s.layout();
        let task=s.project.as_ref().unwrap().tasks.iter().find(|t|t.title.starts_with("[SSH-02]")).unwrap().clone();
        s.selected=Some(task.id);
        let revision=s.project.as_ref().unwrap().revision;
        SMOKE_CANCEL_FORM.with(|flag|flag.set(true));s.command(113)?;
        if s.project.as_ref().unwrap().revision!=revision {return Err("Cancelled archive wrote a revision".into());}
        let dependencies=serde_json::to_value(&s.project.as_ref().unwrap().dependencies)?;
        s.command(113)?;
        if !s.project.as_ref().unwrap().tasks.iter().find(|t|t.id==task.id).unwrap().archived {return Err("Archive dialog did not preserve item".into());}
        for view in [1,4,5] {s.command(200+view)?;capture(s,area,&directory.join(format!("archived-view-{view}.bmp")))?;}
        s.command(412)?;capture(s,area,&directory.join("archived-item.bmp"))?;
        if s.hits.len()!=1 {return Err("Archived item is inaccessible from archive menu".into());}
        s.command(119)?;
        let restored=s.project.as_ref().unwrap().tasks.iter().find(|t|t.id==task.id).unwrap();
        if serde_json::to_value(restored)?!=serde_json::to_value(&task)? || serde_json::to_value(&s.project.as_ref().unwrap().dependencies)?!=dependencies {return Err("Restore lost fields or relationships".into());}
        report.push("Native classification/review fields, pending filter, archive cancel/confirm, archived graph/gantt/chart, archive menu and restore: OK".into());
        Ok(())
    })?;
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
        let started = std::time::Instant::now();
        state.paint(dc, area);
        let paint_us = started.elapsed().as_micros();
        std::fs::write(
            path.with_extension("json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "project_id":state.project.as_ref().map(|p|p.id),"view":state.view,"view_name":VIEWS[state.view],
                "dpi":state.dpi,"client":[area.right,area.bottom],"body":[state.body.left,state.body.top,state.body.right,state.body.bottom],
                "content":[state.content.0,state.content.1],"scroll":[state.x,state.y],"visible_hit_targets":state.hits.len(),
                "project_items":state.project.as_ref().map_or(0,|p|p.tasks.len()),"paint_microseconds":paint_us,
                "capture_note":"Production canvas painter; native child controls are not included. Single offscreen draw, not an interactive performance benchmark."
            }))?,
        )?;
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
