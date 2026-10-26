//! Native Windows views; storage and scheduling stay in the portable core.
use std::{cell::RefCell, collections::BTreeMap, path::PathBuf};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::{Com::*, LibraryLoader::*},
        UI::{
            Controls::Dialogs::*, Controls::*, HiDpi::*, Input::KeyboardAndMouse::*,
            WindowsAndMessaging::*,
        },
    },
    core::{HSTRING, PCWSTR, w},
};
use yy_project::*;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
mod smoke;
thread_local! {static SMOKE:std::cell::Cell<bool>=const{std::cell::Cell::new(false)};}
thread_local! {static SMOKE_ERRORS:RefCell<Vec<String>>=const{RefCell::new(Vec::new())};}
thread_local! {static SMOKE_CANCEL_FORM:std::cell::Cell<bool>=const{std::cell::Cell::new(false)};}
const AUTO_FORM: u32 = WM_APP + 17;
const VIEWS: [&str; 8] = [
    "カンバン",
    "ガント",
    "タスク一覧",
    "課題一覧",
    "依存ネットワーク",
    "バーンダウン",
    "バーンアップ",
    "マスタ",
];
thread_local! {static APP:RefCell<Option<State>>=const{RefCell::new(None)};}
struct State {
    hwnd: HWND,
    selector: HWND,
    tabs: HWND,
    font: HFONT,
    dpi: i32,
    network: BTreeMap<i64, (usize, usize)>,
    store: Store,
    path: PathBuf,
    projects: Vec<(i64, String, i64)>,
    project: Option<Project>,
    view: usize,
    filter: TaskFilter,
    master_kind: MasterKind,
    selected: Option<i64>,
    x: i32,
    y: i32,
    content: (i32, i32),
    hits: Vec<(RECT, i64)>,
    drag: Option<i64>,
    version: i64,
    status: String,
    body: RECT,
}
fn send(h: HWND, m: u32, w: usize, l: isize) -> LRESULT {
    unsafe { SendMessageW(h, m, Some(WPARAM(w)), Some(LPARAM(l))) }
}
fn text(h: HWND) -> String {
    unsafe {
        let mut b = vec![0; GetWindowTextLengthW(h).max(0) as usize + 1];
        let n = GetWindowTextW(h, &mut b);
        String::from_utf16_lossy(&b[..n as usize])
    }
}
fn message(h: HWND, value: &str) {
    if SMOKE.with(|s| s.get()) {
        SMOKE_ERRORS.with(|errors| errors.borrow_mut().push(value.into()));
        return;
    }
    unsafe {
        MessageBoxW(
            Some(h),
            &HSTRING::from(value),
            w!("yyproject"),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}
fn px(h: HWND, value: i32) -> i32 {
    unsafe { value * GetDpiForWindow(h).max(96) as i32 / 96 }
}
fn rect(x: i32, y: i32, width: i32, height: i32) -> RECT {
    RECT {
        left: x,
        top: y,
        right: x + width,
        bottom: y + height,
    }
}
fn inside(r: RECT, x: i32, y: i32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}
fn color(status: Status) -> COLORREF {
    COLORREF(match status {
        Status::Todo => 0xded6c8,
        Status::Doing => 0xe8b46b,
        Status::Blocked => 0x98a5f5,
        Status::Done => 0xa6d6a4,
        Status::Cancelled => 0xd4d4d4,
    })
}
fn fill(dc: HDC, r: RECT, c: COLORREF) {
    unsafe {
        SetDCBrushColor(dc, c);
        FillRect(dc, &r, HBRUSH(GetStockObject(DC_BRUSH).0));
    }
}
fn label(dc: HDC, r: RECT, s: &str, c: COLORREF) {
    if s.is_empty() || r.right <= r.left || r.bottom <= r.top {
        return;
    }
    unsafe {
        SetTextColor(dc, c);
        SetBkMode(dc, TRANSPARENT);
        let mut b: Vec<_> = s.encode_utf16().chain(Some(0)).collect();
        let count = b.len() - 1;
        DrawTextW(
            dc,
            &mut b[..count],
            &mut r.clone(),
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
    }
}
fn line(dc: HDC, a: (i32, i32), b: (i32, i32), c: COLORREF) {
    unsafe {
        SelectObject(dc, GetStockObject(DC_PEN));
        SetDCPenColor(dc, c);
        let _ = MoveToEx(dc, a.0, a.1, None);
        let _ = LineTo(dc, b.0, b.1);
    }
}
fn window_font(h: HWND) -> HFONT {
    unsafe {
        CreateFontW(
            -px(h, 16),
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            FF_MODERN.0 as u32,
            &HSTRING::from(yy_ui_win::font::BUNDLED_FAMILY),
        )
    }
}
fn invalidate(h: HWND) {
    unsafe {
        let _ = InvalidateRect(Some(h), None, false);
    }
}
fn file_dialog(h: HWND, save: bool, json: bool) -> Option<PathBuf> {
    unsafe {
        let filter: Vec<u16> = if json {
            "JSON\0*.json\0\0"
        } else {
            "YYProject SQLite\0*.sqlite3;*.db\0すべてのファイル\0*.*\0\0"
        }
        .encode_utf16()
        .collect();
        let mut path = vec![0u16; 32768];
        let mut f = OPENFILENAMEW {
            lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
            hwndOwner: h,
            lpstrFilter: PCWSTR(filter.as_ptr()),
            lpstrFile: windows::core::PWSTR(path.as_mut_ptr()),
            nMaxFile: path.len() as u32,
            Flags: OFN_EXPLORER
                | OFN_NOCHANGEDIR
                | if save {
                    OFN_OVERWRITEPROMPT
                } else {
                    OFN_FILEMUSTEXIST
                },
            ..Default::default()
        };
        if (if save {
            GetSaveFileNameW(&mut f)
        } else {
            GetOpenFileNameW(&mut f)
        })
        .as_bool()
        {
            Some(PathBuf::from(String::from_utf16_lossy(
                &path[..path.iter().position(|n| *n == 0).unwrap_or(path.len())],
            )))
        } else {
            None
        }
    }
}
#[derive(Clone)]
struct Field {
    name: String,
    value: String,
    choices: Vec<(String, String)>,
    page: usize,
    multi: bool,
    read_only: bool,
}
fn field(name: &str, value: impl ToString, page: usize) -> Field {
    Field {
        name: name.into(),
        value: value.to_string(),
        choices: vec![],
        page,
        multi: false,
        read_only: false,
    }
}
fn choice(name: &str, value: impl ToString, choices: Vec<(String, String)>, page: usize) -> Field {
    Field {
        choices,
        ..field(name, value, page)
    }
}
struct Form {
    fields: Vec<Field>,
    controls: Vec<(HWND, HWND)>,
    tabs: Option<HWND>,
    page: usize,
    done: bool,
    result: Option<Vec<String>>,
    validate: fn(&[String]) -> std::result::Result<(), String>,
}
impl Form {
    fn layout(&self, h: HWND) {
        unsafe {
            let mut r = RECT::default();
            let _ = GetClientRect(h, &mut r);
            let margin = px(h, 14);
            let top = if self.tabs.is_some() {
                px(h, 42)
            } else {
                margin
            };
            if let Some(t) = self.tabs {
                let _ = MoveWindow(
                    t,
                    margin,
                    px(h, 6),
                    (r.right - 2 * margin).max(1),
                    px(h, 30),
                    true,
                );
            }
            let visible = self
                .fields
                .iter()
                .enumerate()
                .filter(|(_, f)| f.page == self.page)
                .collect::<Vec<_>>();
            let count = visible.len().max(1) as i32;
            let row = ((r.bottom - top - px(h, 58)) / count).max(px(h, 30));
            for (i, ((l, e), f)) in self.controls.iter().zip(&self.fields).enumerate() {
                let show = f.page == self.page;
                let _ = ShowWindow(*l, if show { SW_SHOW } else { SW_HIDE });
                let _ = ShowWindow(*e, if show { SW_SHOW } else { SW_HIDE });
                if !show {
                    continue;
                }
                let n = visible.iter().position(|(index, _)| *index == i).unwrap() as i32;
                let y = top + n * row;
                let lw = px(h, 125).min(r.right / 3);
                let ew = (r.right - lw - 3 * margin).max(1);
                let _ = MoveWindow(*l, margin, y, lw, px(h, 24), true);
                let eh = if f.multi {
                    (r.bottom - y - px(h, 68)).max(px(h, 36))
                } else {
                    px(h, 28)
                };
                let _ = MoveWindow(
                    *e,
                    2 * margin + lw,
                    y,
                    ew,
                    if f.choices.is_empty() { eh } else { px(h, 240) },
                    true,
                );
            }
            for (id, x) in [(1, r.right - px(h, 220)), (2, r.right - px(h, 112))] {
                let b = GetDlgItem(Some(h), id).unwrap_or_default();
                let _ = MoveWindow(
                    b,
                    x.max(margin),
                    r.bottom - px(h, 44),
                    px(h, 100),
                    px(h, 30),
                    true,
                );
            }
        }
    }
    fn values(&self) -> Vec<String> {
        self.fields
            .iter()
            .zip(&self.controls)
            .map(|(f, (_, h))| {
                if f.choices.is_empty() {
                    let value = text(*h);
                    if f.multi {
                        value.replace("\r\n", "\n")
                    } else {
                        value
                    }
                } else {
                    f.choices
                        .get(send(*h, CB_GETCURSEL, 0, 0).0 as usize)
                        .map(|(v, _)| v.clone())
                        .unwrap_or_default()
                }
            })
            .collect()
    }
}
unsafe extern "system" fn form_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        let ptr = GetWindowLongPtrW(h, GWLP_USERDATA) as *mut Form;
        if !ptr.is_null() {
            let f = &mut *ptr;
            match m {
                AUTO_FORM if SMOKE.with(|s| s.get()) => {
                    if SMOKE_CANCEL_FORM.with(|s| s.replace(false)) {
                        let _ = PostMessageW(Some(h), WM_COMMAND, WPARAM(2), LPARAM(0));
                        return LRESULT(0);
                    }
                    let pages: std::collections::BTreeSet<_> =
                        f.fields.iter().map(|field| field.page).collect();
                    for page in pages {
                        if let Some(tabs) = f.tabs
                            && send(tabs, TCM_GETITEMCOUNT, 0, 0).0 <= page as isize
                        {
                            SMOKE_ERRORS.with(|e| {
                                e.borrow_mut()
                                    .push("Form page is inaccessible from tabs".into())
                            });
                        }
                        f.page = page;
                        f.layout(h);
                        let mut area = RECT::default();
                        let _ = GetClientRect(h, &mut area);
                        let mut origin = POINT::default();
                        let _ = ClientToScreen(h, &mut origin);
                        for (field, (_, edit)) in f
                            .fields
                            .iter()
                            .zip(&f.controls)
                            .filter(|(field, _)| field.page == page)
                        {
                            let mut bounds = RECT::default();
                            let _ = GetWindowRect(*edit, &mut bounds);
                            if bounds.left < origin.x
                                || bounds.top < origin.y
                                || bounds.right > origin.x + area.right
                                || bounds.bottom > origin.y + area.bottom
                            {
                                SMOKE_ERRORS.with(|e| {
                                    e.borrow_mut().push(format!(
                                        "Form field outside client bounds: {}",
                                        field.name
                                    ))
                                });
                            }
                        }
                    }
                    f.page = 0;
                    f.layout(h);
                    for (field, (_, edit)) in f.fields.iter().zip(&f.controls) {
                        let value = match field.name.as_str() {
                            "件名" => Some(if field.value == "UI-created-task" {
                                "UI-edited-task"
                            } else {
                                "UI-created-task"
                            }),
                            "名前" => Some("UI-created-master"),
                            "件名・説明を検索" => Some("[ISS-001]"),
                            "再開後の進捗（0〜99%）" => Some("40"),
                            "予定工数（時間）" => Some("8"),
                            _ => None,
                        };
                        if field.name == "優先度" && field.value == "normal" {
                            send(*edit, CB_SETCURSEL, 2, 0);
                        }
                        if field.name == "適用の確認" {
                            send(*edit, CB_SETCURSEL, 1, 0);
                        }
                        if field.read_only
                            && GetWindowLongW(*edit, GWL_STYLE) as u32 & ES_READONLY as u32 == 0
                        {
                            SMOKE_ERRORS.with(|errors| {
                                errors.borrow_mut().push("Preview was editable".into())
                            });
                        }
                        if let Some(value) = value {
                            let _ = SetWindowTextW(*edit, &HSTRING::from(value));
                        }
                    }
                    let _ = PostMessageW(Some(h), WM_COMMAND, WPARAM(1), LPARAM(0));
                    return LRESULT(0);
                }
                WM_SIZE => {
                    f.layout(h);
                    return LRESULT(0);
                }
                WM_NOTIFY => {
                    if (*(l.0 as *const NMHDR)).code == TCN_SELCHANGE {
                        if let Some(t) = f.tabs {
                            f.page = send(t, TCM_GETCURSEL, 0, 0).0.max(0) as usize;
                            f.layout(h);
                        }
                        return LRESULT(0);
                    }
                }
                WM_COMMAND => match w.0 & 0xffff {
                    1 => {
                        let values = f.values();
                        match (f.validate)(&values) {
                            Ok(()) => {
                                f.result = Some(values);
                                f.done = true;
                            }
                            Err(e) => {
                                message(h, &e);
                                if SMOKE.with(|s| s.get()) {
                                    f.done = true;
                                }
                            }
                        }
                        return LRESULT(0);
                    }
                    2 => {
                        f.done = true;
                        return LRESULT(0);
                    }
                    _ => {}
                },
                WM_CLOSE => {
                    f.done = true;
                    return LRESULT(0);
                }
                _ => {}
            }
        }
        DefWindowProcW(h, m, w, l)
    }
}
fn form(
    parent: HWND,
    title: &str,
    fields: Vec<Field>,
    validate: fn(&[String]) -> std::result::Result<(), String>,
) -> Option<Vec<String>> {
    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        RegisterClassW(&WNDCLASSW {
            hInstance: instance.into(),
            lpszClassName: w!("YYProjectForm"),
            lpfnWndProc: Some(form_proc),
            hCursor: LoadCursorW(None, IDC_ARROW).ok()?,
            hbrBackground: GetSysColorBrush(COLOR_BTNFACE),
            ..Default::default()
        });
        let mut work = RECT::default();
        let _ = SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some((&mut work as *mut RECT).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        let width = px(parent, 650).min((work.right - work.left - 20).max(320));
        let height = px(parent, 470).min((work.bottom - work.top - 20).max(300));
        let h = CreateWindowExW(
            WS_EX_DLGMODALFRAME,
            w!("YYProjectForm"),
            &HSTRING::from(title),
            WS_CAPTION | WS_SYSMENU | WS_POPUP | WS_CLIPCHILDREN,
            work.left + (work.right - work.left - width) / 2,
            work.top + (work.bottom - work.top - height) / 2,
            width,
            height,
            Some(parent),
            None,
            Some(instance.into()),
            None,
        )
        .ok()?;
        let font = window_font(h);
        let mut f = Form {
            fields,
            controls: vec![],
            tabs: None,
            page: 0,
            done: false,
            result: None,
            validate,
        };
        let pages = f.fields.iter().map(|f| f.page).max().unwrap_or(0);
        if pages > 0 {
            let t = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                WC_TABCONTROLW,
                w!(""),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                0,
                0,
                1,
                1,
                Some(h),
                None,
                Some(instance.into()),
                None,
            )
            .ok()?;
            send(t, WM_SETFONT, font.0 as usize, 0);
            for (i, name) in [
                "基本情報",
                "日程・進捗",
                "説明",
                "分類・確認",
                "確認メモ",
                "集計・関連",
            ]
            .iter()
            .enumerate()
            {
                let mut value: Vec<_> = name.encode_utf16().chain(Some(0)).collect();
                let item = TCITEMW {
                    mask: TCIF_TEXT,
                    pszText: windows::core::PWSTR(value.as_mut_ptr()),
                    ..Default::default()
                };
                send(t, TCM_INSERTITEMW, i, &item as *const _ as isize);
            }
            f.tabs = Some(t);
        }
        for (i, item) in f.fields.iter().enumerate() {
            let label = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                &HSTRING::from(&item.name),
                WS_CHILD,
                0,
                0,
                1,
                1,
                Some(h),
                None,
                Some(instance.into()),
                None,
            )
            .ok()?;
            let style = if item.choices.is_empty() {
                WS_CHILD
                    | WS_BORDER
                    | WS_TABSTOP
                    | WINDOW_STYLE(if item.read_only {
                        ES_READONLY as u32
                    } else {
                        0
                    })
                    | WINDOW_STYLE(if item.multi {
                        ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32 | WS_VSCROLL.0
                    } else {
                        ES_AUTOHSCROLL as u32
                    })
            } else {
                WS_CHILD | WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(CBS_DROPDOWNLIST as u32)
            };
            let edit = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                if item.choices.is_empty() {
                    w!("EDIT")
                } else {
                    w!("COMBOBOX")
                },
                &HSTRING::from(if item.multi {
                    item.value.replace("\r\n", "\n").replace('\n', "\r\n")
                } else {
                    item.value.clone()
                }),
                style,
                0,
                0,
                1,
                1,
                Some(h),
                Some(HMENU(std::ptr::without_provenance_mut(100 + i))),
                Some(instance.into()),
                None,
            )
            .ok()?;
            for c in [label, edit] {
                send(c, WM_SETFONT, font.0 as usize, 0);
            }
            if item.choices.is_empty() {
                send(
                    edit,
                    EM_SETLIMITTEXT,
                    if item.multi {
                        item.value.encode_utf16().count().max(65536)
                    } else {
                        4096
                    },
                    0,
                );
            } else {
                for (n, (key, title)) in item.choices.iter().enumerate() {
                    send(
                        edit,
                        CB_ADDSTRING,
                        0,
                        HSTRING::from(title).as_ptr() as isize,
                    );
                    if *key == item.value {
                        send(edit, CB_SETCURSEL, n, 0);
                    }
                }
            }
            f.controls.push((label, edit));
        }
        for (id, title) in [(1, "保存"), (2, "キャンセル")] {
            let b = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("BUTTON"),
                &HSTRING::from(title),
                WS_CHILD
                    | WS_VISIBLE
                    | WS_TABSTOP
                    | WINDOW_STYLE(if id == 1 { BS_DEFPUSHBUTTON as u32 } else { 0 }),
                0,
                0,
                1,
                1,
                Some(h),
                Some(HMENU(std::ptr::without_provenance_mut(id))),
                Some(instance.into()),
                None,
            )
            .ok()?;
            send(b, WM_SETFONT, font.0 as usize, 0);
        }
        SetWindowLongPtrW(h, GWLP_USERDATA, (&mut f as *mut Form) as isize);
        f.layout(h);
        let _ = EnableWindow(parent, false);
        let _ = ShowWindow(
            h,
            if SMOKE.with(|s| s.get()) {
                SW_HIDE
            } else {
                SW_SHOW
            },
        );
        if SMOKE.with(|s| s.get()) {
            let _ = PostMessageW(Some(h), AUTO_FORM, WPARAM(0), LPARAM(0));
        }
        if let Some((_, e)) = f.controls.first() {
            let _ = SetFocus(Some(*e));
        }
        let mut msg = MSG::default();
        loop {
            if f.done {
                break;
            }
            let result = GetMessageW(&mut msg, None, 0, 0).0;
            if result <= 0 {
                if result == 0 {
                    PostQuitMessage(msg.wParam.0 as i32);
                }
                break;
            }
            if !IsDialogMessageW(h, &msg).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        SetWindowLongPtrW(h, GWLP_USERDATA, 0);
        let _ = DestroyWindow(h);
        let _ = EnableWindow(parent, true);
        let _ = SetForegroundWindow(parent);
        let _ = DeleteObject(font.into());
        invalidate(parent);
        f.result
    }
}
fn date(s: &str) -> std::result::Result<chrono::NaiveDate, String> {
    s.parse()
        .map_err(|_| "日付はYYYY-MM-DDで指定してください".into())
}
fn related_ids(value: &str) -> std::result::Result<Vec<i64>, String> {
    value
        .split(|c: char| c == ',' || c == '、' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.parse::<i64>()
                .ok()
                .filter(|id| *id > 0)
                .ok_or_else(|| "対応タスクは正のIDをカンマ区切りで指定してください".into())
        })
        .collect()
}
fn task_values(v: &[String]) -> std::result::Result<(), String> {
    if v[0].trim().is_empty() {
        return Err("件名を入力してください".into());
    }
    let start = if v[5].is_empty() {
        None
    } else {
        Some(date(&v[5])?)
    };
    let due = if v[6].is_empty() {
        None
    } else {
        Some(date(&v[6])?)
    };
    if start.zip(due).is_some_and(|(a, b)| a > b) {
        return Err("開始日が期限より後です".into());
    }
    if v[7].parse::<u32>().ok().is_none_or(|n| n > 3660)
        || v[8]
            .parse::<f64>()
            .ok()
            .is_none_or(|n| !n.is_finite() || !(0.0..=1e9).contains(&n))
        || v[9].parse::<u8>().ok().is_none_or(|n| n > 100)
    {
        return Err("期間は0～3660営業日、工数は0以上、進捗は0～100にしてください".into());
    }
    if v.len() > 17 {
        related_ids(&v[17])?;
        if v[15].len() > 65536 {
            return Err("確認メモはUTF-8で65536バイト以内にしてください".into());
        }
        if v[13] != "not_required" && v[14].is_empty() {
            return Err("レビューには確認者が必要です".into());
        }
        if matches!(v[13].as_str(), "approved" | "changes_requested") && v[15].trim().is_empty() {
            return Err("承認・差し戻しには確認メモを入力してください".into());
        }
        if v[2] == "done" && matches!(v[13].as_str(), "pending" | "changes_requested") {
            return Err("確認待ち・差し戻しの項目は完了にできません".into());
        }
    }
    Ok(())
}
impl State {
    fn px(&self, value: i32) -> i32 {
        value * self.dpi / 96
    }
    fn gantt_range(&self) -> Option<(chrono::NaiveDate, chrono::NaiveDate)> {
        let p = self.project.as_ref()?;
        Some((
            p.tasks
                .iter()
                .filter(|t| !t.archived)
                .filter_map(|t| t.start)
                .fold(p.start, std::cmp::min),
            p.tasks
                .iter()
                .filter(|t| !t.archived)
                .filter_map(|t| t.due)
                .fold(p.finish, std::cmp::max),
        ))
    }
    fn reload(&mut self) -> Result<()> {
        self.projects = self.store.list()?;
        send(self.selector, CB_RESETCONTENT, 0, 0);
        for (i, (id, name, _)) in self.projects.iter().enumerate() {
            send(
                self.selector,
                CB_ADDSTRING,
                0,
                HSTRING::from(name).as_ptr() as isize,
            );
            if self.project.as_ref().is_some_and(|p| p.id == *id) {
                send(self.selector, CB_SETCURSEL, i, 0);
            }
        }
        let id = self
            .project
            .as_ref()
            .map(|p| p.id)
            .or_else(|| self.projects.first().map(|p| p.0));
        self.project = id.map(|id| self.store.get(id)).transpose()?;
        if send(self.selector, CB_GETCURSEL, 0, 0).0 < 0 && !self.projects.is_empty() {
            send(self.selector, CB_SETCURSEL, 0, 0);
        }
        self.network = self
            .project
            .as_ref()
            .map(|p| p.network())
            .transpose()?
            .unwrap_or_default();
        self.version = self.store.data_version()?;
        self.layout();
        invalidate(self.hwnd);
        Ok(())
    }
    fn select_project(&mut self, index: usize) -> Result<()> {
        let id = self
            .projects
            .get(index)
            .ok_or("プロジェクトを選択してください")?
            .0;
        self.project = Some(self.store.get(id)?);
        self.filter = TaskFilter::default();
        self.selected = None;
        self.x = 0;
        self.y = 0;
        self.reload()?;
        self.focus_board();
        Ok(())
    }
    fn focus_board(&mut self) {
        if self.view == 0 {
            self.x = self
                .project
                .as_ref()
                .and_then(|p| {
                    Status::ALL.iter().position(|status| {
                        p.tasks
                            .iter()
                            .any(|t| t.status == *status && self.filter.matches(t))
                    })
                })
                .unwrap_or(0) as i32
                * self.px(240);
            self.y = 0;
            self.layout();
        }
    }
    fn set_status(&mut self, mut task: Task, status: Status) -> Result<()> {
        if task.status == Status::Done && !matches!(status, Status::Done | Status::Cancelled) {
            let Some(values) = form(
                self.hwnd,
                "完了項目を再開 — 残作業を反映",
                vec![field("再開後の進捗（0〜99%）", "", 0)],
                |v| {
                    if v[0].parse::<u8>().is_ok_and(|n| n < 100) {
                        Ok(())
                    } else {
                        Err("残作業があるため進捗を0〜99%で入力してください".into())
                    }
                },
            ) else {
                return Ok(());
            };
            task.progress = values[0].parse()?;
        }
        task.status = status;
        self.apply(Change::SaveTask { task })
    }
    fn filter_form(&mut self) -> Result<()> {
        let p = self
            .project
            .as_ref()
            .ok_or("プロジェクトを選択してください")?;
        let master_options = |kind| {
            std::iter::once((String::new(), "すべて".into()))
                .chain(
                    p.masters
                        .iter()
                        .filter(move |m| m.kind == kind)
                        .map(|m| (m.id.to_string(), m.name.clone())),
                )
                .collect()
        };
        let mut states = vec![(String::new(), "すべて".into())];
        states.extend(Status::ALL.into_iter().map(|s| {
            (
                serde_json::to_value(s).unwrap().as_str().unwrap().into(),
                s.label().into(),
            )
        }));
        let mode = if self.filter.overdue_before.is_some() {
            "overdue"
        } else if self.filter.unassigned {
            "unassigned"
        } else if self.filter.incomplete {
            "incomplete"
        } else {
            "all"
        };
        if let Some(v) = form(
            self.hwnd,
            "絞り込み — カンバン・タスク一覧・課題一覧",
            vec![
                field("件名・説明を検索", &self.filter.query, 0),
                choice(
                    "担当者",
                    self.filter
                        .assignee
                        .map(|id| id.to_string())
                        .unwrap_or_default(),
                    master_options(MasterKind::Assignee),
                    0,
                ),
                choice(
                    "カテゴリ",
                    self.filter
                        .category
                        .map(|id| id.to_string())
                        .unwrap_or_default(),
                    master_options(MasterKind::Category),
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
                    states,
                    0,
                ),
                choice(
                    "対象",
                    mode,
                    vec![
                        ("all".into(), "すべて".into()),
                        ("incomplete".into(), "未完了".into()),
                        ("overdue".into(), "本日より前に期限超過・未完了".into()),
                        ("unassigned".into(), "未割当・未完了".into()),
                    ],
                    0,
                ),
                choice(
                    "優先度",
                    self.filter
                        .priority
                        .map(|v| {
                            serde_json::to_value(v)
                                .unwrap()
                                .as_str()
                                .unwrap()
                                .to_owned()
                        })
                        .unwrap_or_default(),
                    std::iter::once((String::new(), "すべて".into()))
                        .chain(Priority::ALL.into_iter().map(|v| {
                            (
                                serde_json::to_value(v).unwrap().as_str().unwrap().into(),
                                v.label().into(),
                            )
                        }))
                        .collect(),
                    0,
                ),
                choice(
                    "重大度",
                    self.filter
                        .severity
                        .map(|v| {
                            serde_json::to_value(v)
                                .unwrap()
                                .as_str()
                                .unwrap()
                                .to_owned()
                        })
                        .unwrap_or_default(),
                    std::iter::once((String::new(), "すべて".into()))
                        .chain(Severity::ALL.into_iter().map(|v| {
                            (
                                serde_json::to_value(v).unwrap().as_str().unwrap().into(),
                                v.label().into(),
                            )
                        }))
                        .collect(),
                    0,
                ),
                choice(
                    "レビュー",
                    self.filter
                        .review
                        .map(|v| {
                            serde_json::to_value(v)
                                .unwrap()
                                .as_str()
                                .unwrap()
                                .to_owned()
                        })
                        .unwrap_or_default(),
                    std::iter::once((String::new(), "すべて".into()))
                        .chain(ReviewState::ALL.into_iter().map(|v| {
                            (
                                serde_json::to_value(v).unwrap().as_str().unwrap().into(),
                                v.label().into(),
                            )
                        }))
                        .collect(),
                    0,
                ),
            ],
            |_| Ok(()),
        ) {
            self.filter = TaskFilter {
                query: v[0].clone(),
                priority: if v[5].is_empty() {
                    None
                } else {
                    Some(serde_json::from_value(serde_json::Value::String(
                        v[5].clone(),
                    ))?)
                },
                severity: if v[6].is_empty() {
                    None
                } else {
                    Some(serde_json::from_value(serde_json::Value::String(
                        v[6].clone(),
                    ))?)
                },
                review: if v[7].is_empty() {
                    None
                } else {
                    Some(serde_json::from_value(serde_json::Value::String(
                        v[7].clone(),
                    ))?)
                },
                archived: self.filter.archived,
                assignee: v[1].parse().ok(),
                category: v[2].parse().ok(),
                status: if v[3].is_empty() {
                    None
                } else {
                    Some(serde_json::from_value(serde_json::Value::String(
                        v[3].clone(),
                    ))?)
                },
                unassigned: v[4] == "unassigned",
                incomplete: v[4] != "all",
                overdue_before: (v[4] == "overdue").then(|| chrono::Local::now().date_naive()),
                ..Default::default()
            };
            self.selected = None;
            self.x = 0;
            self.y = 0;
            self.focus_board();
        }
        Ok(())
    }
    fn archive_form(&mut self, id: i64) -> Result<()> {
        let p = self
            .project
            .as_ref()
            .ok_or("プロジェクトを選択してください")?;
        let impact = p.removal_preview(id)?;
        let mut lines = vec![
            format!("#{} {} / 版 {}", id, impact.task.title, impact.revision),
            format!(
                "保管後は通常表示・日程計算・工数集計から除外します。依存関係{}件を無効化し、関連課題{}件からの参照を保管済みとして保持します。復元すると同じIDと関係が戻ります。",
                impact.dependencies.len(),
                impact.related_issue_ids.len()
            ),
        ];
        for d in &impact.dependencies {
            lines.push(format!(
                "#{} → #{}（待機{}営業日）",
                d.predecessor, d.successor, d.lag_days
            ));
        }
        let mut details = field("影響（読み取り専用）", lines.join("\n"), 0);
        details.multi = true;
        details.read_only = true;
        if form(
            self.hwnd,
            "項目を保管 — 影響確認",
            vec![
                choice(
                    "適用の確認",
                    "no",
                    vec![
                        ("no".into(), "未確認".into()),
                        ("yes".into(), "影響を確認して保管する".into()),
                    ],
                    0,
                ),
                details,
            ],
            |v| {
                if v[0] == "yes" {
                    Ok(())
                } else {
                    Err("影響を確認して保管を選択してください".into())
                }
            },
        )
        .is_some()
        {
            self.apply(Change::RemoveTask { id })?;
            self.selected = None;
        }
        Ok(())
    }
    fn restore_form(&mut self) -> Result<()> {
        let p = self
            .project
            .as_ref()
            .ok_or("プロジェクトを選択してください")?;
        let choices: Vec<_> = p
            .tasks
            .iter()
            .filter(|t| t.archived)
            .map(|t| (t.id.to_string(), format!("#{} {}", t.id, t.title)))
            .collect();
        let first = choices
            .first()
            .ok_or("保管済みの項目はありません")?
            .0
            .clone();
        if let Some(v) = form(
            self.hwnd,
            "保管した項目を復元",
            vec![choice("復元する項目", first, choices, 0)],
            |_| Ok(()),
        ) {
            self.apply(Change::RestoreTask { id: v[0].parse()? })?;
            self.filter = TaskFilter::default();
            self.selected = None;
            self.focus_board();
        }
        Ok(())
    }
    fn schedule_form(&mut self) -> Result<()> {
        let p = self
            .project
            .as_ref()
            .ok_or("プロジェクトを選択してください")?;
        let preview = p.schedule_preview()?;
        let mut lines = vec![
            format!(
                "版 {} / 変更 {}件 / 案件終了日 {}",
                preview.revision,
                preview.changes.len(),
                p.finish
            ),
            "完了・中止済みで開始日と期限がある項目は固定します。".into(),
        ];
        if !preview.overdue_ids.is_empty() {
            lines.push(format!(
                "警告: 案件終了日を超過する項目 {}件",
                preview.overdue_ids.len()
            ));
            for t in p
                .tasks
                .iter()
                .filter(|t| preview.overdue_ids.contains(&t.id))
            {
                lines.push(format!("  #{} {}", t.id, t.title));
            }
        }
        for d in &preview.changes {
            lines.push(format!(
                "#{} {}\n  {:?} 〜 {:?} → {:?} 〜 {:?}",
                d.id, d.title, d.old_start, d.old_due, d.start, d.due
            ));
        }
        let mut details = field("変更差分（読み取り専用）", lines.join("\n"), 0);
        details.multi = true;
        details.read_only = true;
        let acceptance = if preview.overdue_ids.is_empty() {
            "差分を確認して適用する"
        } else {
            "納期超過を承認して適用する"
        };
        if form(
            self.hwnd,
            "日程の再計算 — 保存前の確認",
            vec![
                choice(
                    "適用の確認",
                    "no",
                    vec![
                        ("no".into(), "未確認".into()),
                        ("yes".into(), acceptance.into()),
                    ],
                    0,
                ),
                details,
            ],
            |v| {
                if v[0] == "yes" {
                    Ok(())
                } else {
                    Err("差分と警告を確認して、適用を選択してください".into())
                }
            },
        )
        .is_some()
        {
            self.apply(Change::Schedule {
                allow_overrun: !preview.overdue_ids.is_empty(),
            })?;
        }
        Ok(())
    }
    fn apply(&mut self, change: Change) -> Result<()> {
        let p = self
            .project
            .as_ref()
            .ok_or("先にプロジェクトを作成してください")?;
        self.project = Some(self.store.apply(p.id, p.revision, &change, "GUI")?);
        self.status = "保存しました".into();
        self.reload()
    }
    fn task(&self) -> Option<&Task> {
        self.project
            .as_ref()?
            .tasks
            .iter()
            .find(|t| Some(t.id) == self.selected)
    }
    fn edit_task(&mut self, new: bool, issue: bool) -> Result<()> {
        let p = self
            .project
            .as_ref()
            .ok_or("先にプロジェクトを作成してください")?;
        let mut t = if new {
            Task {
                kind: if issue { Kind::Issue } else { Kind::Task },
                duration_days: 1,
                include_in_effort: !issue,
                ..Default::default()
            }
        } else {
            self.task().ok_or("項目を選択してください")?.clone()
        };
        let masters = |kind| {
            std::iter::once((String::new(), "（未指定）".into()))
                .chain(
                    p.masters
                        .iter()
                        .filter(move |m| m.kind == kind)
                        .map(|m| (m.id.to_string(), m.name.clone())),
                )
                .collect()
        };
        let mut fields = vec![
            field("件名", &t.title, 0),
            choice(
                "種類",
                if t.kind == Kind::Issue {
                    "issue"
                } else {
                    "task"
                },
                vec![
                    ("task".into(), "タスク".into()),
                    ("issue".into(), "課題".into()),
                ],
                0,
            ),
            choice(
                "状態",
                serde_json::to_value(t.status)?.as_str().unwrap_or("todo"),
                Status::ALL
                    .into_iter()
                    .map(|s| {
                        (
                            serde_json::to_value(s).unwrap().as_str().unwrap().into(),
                            s.label().into(),
                        )
                    })
                    .collect(),
                0,
            ),
            choice(
                "担当者",
                t.assignee.map(|i| i.to_string()).unwrap_or_default(),
                masters(MasterKind::Assignee),
                0,
            ),
            choice(
                "カテゴリ",
                t.category.map(|i| i.to_string()).unwrap_or_default(),
                masters(MasterKind::Category),
                0,
            ),
            field(
                "開始日",
                t.start.map(|d| d.to_string()).unwrap_or_default(),
                1,
            ),
            field("期限", t.due.map(|d| d.to_string()).unwrap_or_default(), 1),
            field("期間（営業日）", t.duration_days, 1),
            field("予定工数（時間）", t.estimate, 1),
            field("進捗（%）", t.progress, 1),
            field("説明", &t.description, 2),
        ];
        fields.extend([
            choice(
                "優先度",
                serde_json::to_value(t.priority)?.as_str().unwrap(),
                Priority::ALL
                    .into_iter()
                    .map(|v| {
                        (
                            serde_json::to_value(v).unwrap().as_str().unwrap().into(),
                            v.label().into(),
                        )
                    })
                    .collect(),
                3,
            ),
            choice(
                "重大度",
                serde_json::to_value(t.severity)?.as_str().unwrap(),
                Severity::ALL
                    .into_iter()
                    .map(|v| {
                        (
                            serde_json::to_value(v).unwrap().as_str().unwrap().into(),
                            v.label().into(),
                        )
                    })
                    .collect(),
                3,
            ),
            choice(
                "レビュー",
                serde_json::to_value(t.review)?.as_str().unwrap(),
                ReviewState::ALL
                    .into_iter()
                    .map(|v| {
                        (
                            serde_json::to_value(v).unwrap().as_str().unwrap().into(),
                            v.label().into(),
                        )
                    })
                    .collect(),
                3,
            ),
            choice(
                "確認者",
                t.reviewer.map(|id| id.to_string()).unwrap_or_default(),
                masters(MasterKind::Assignee),
                3,
            ),
            field("確認メモ・理由", &t.review_note, 4),
            choice(
                "工数集計",
                if t.include_in_effort { "yes" } else { "no" },
                vec![
                    ("yes".into(), "工数に算入".into()),
                    ("no".into(), "集計から除外".into()),
                ],
                5,
            ),
            field(
                "対応タスクID（課題のみ）",
                t.related_tasks
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
                5,
            ),
        ]);
        fields[10].multi = true;
        fields[15].multi = true;
        if let Some(v) = form(
            self.hwnd,
            if new {
                "項目を追加"
            } else {
                "項目を編集"
            },
            fields,
            task_values,
        ) {
            t.title = v[0].clone();
            t.kind = if v[1] == "issue" {
                Kind::Issue
            } else {
                Kind::Task
            };
            t.status = serde_json::from_value(serde_json::Value::String(v[2].clone()))?;
            t.assignee = v[3].parse().ok();
            t.category = v[4].parse().ok();
            t.start = if v[5].is_empty() {
                None
            } else {
                Some(date(&v[5])?)
            };
            t.due = if v[6].is_empty() {
                None
            } else {
                Some(date(&v[6])?)
            };
            t.duration_days = v[7].parse()?;
            t.estimate = v[8].parse()?;
            t.progress = v[9].parse()?;
            t.description = v[10].clone();
            t.priority = serde_json::from_value(serde_json::Value::String(v[11].clone()))?;
            t.severity = serde_json::from_value(serde_json::Value::String(v[12].clone()))?;
            t.review = serde_json::from_value(serde_json::Value::String(v[13].clone()))?;
            t.reviewer = v[14].parse().ok();
            t.review_note = v[15].clone();
            t.include_in_effort = v[16] == "yes";
            t.related_tasks = related_ids(&v[17])?;
            self.apply(Change::SaveTask { task: t })?;
        }
        Ok(())
    }
    fn project_form(&mut self, new: bool) -> Result<()> {
        let today = chrono::Local::now().date_naive();
        let p = self.project.as_ref();
        let fields = vec![
            field(
                "プロジェクト名",
                if new {
                    "新しいプロジェクト"
                } else {
                    p.map_or("", |p| p.name.as_str())
                },
                0,
            ),
            field(
                "開始日",
                if new {
                    today
                } else {
                    p.ok_or("プロジェクトを選択してください")?.start
                },
                0,
            ),
            field(
                "完了予定日",
                if new {
                    today + chrono::Days::new(30)
                } else {
                    p.unwrap().finish
                },
                0,
            ),
        ];
        if let Some(v) = form(self.hwnd, "プロジェクト", fields, |v| {
            if v[0].trim().is_empty() {
                return Err("名前を入力してください".into());
            }
            let a = date(&v[1])?;
            let b = date(&v[2])?;
            if a > b || (b - a).num_days() > 3660 {
                return Err("日程は開始日から10年以内にしてください".into());
            }
            Ok(())
        }) {
            if new {
                self.project =
                    Some(
                        self.store
                            .create(v[0].clone(), date(&v[1])?, date(&v[2])?, "GUI")?,
                    );
                self.reload()?;
            } else {
                self.apply(Change::Rename {
                    name: v[0].clone(),
                    start: date(&v[1])?,
                    finish: date(&v[2])?,
                })?;
            }
        }
        Ok(())
    }
    fn master(&mut self, new: bool) -> Result<()> {
        let p = self
            .project
            .as_ref()
            .ok_or("プロジェクトを選択してください")?;
        let old = if new {
            None
        } else {
            Some(
                p.masters
                    .iter()
                    .find(|m| Some(m.id) == self.selected)
                    .ok_or("マスタ項目を選択してください")?,
            )
        };
        let kind = old.map_or(self.master_kind, |m| m.kind);
        let mut fields = vec![field("名前", old.map_or("", |m| m.name.as_str()), 0)];
        if kind == MasterKind::Holiday {
            fields.push(field(
                "休日（YYYY-MM-DD）",
                old.and_then(|m| m.date)
                    .map(|d| d.to_string())
                    .unwrap_or_default(),
                0,
            ));
        }
        if let Some(v) = form(self.hwnd, "マスタ項目", fields, |v| {
            if v[0].trim().is_empty() {
                return Err("名前を入力してください".into());
            }
            if v.len() > 1 {
                date(&v[1])?;
            }
            Ok(())
        }) {
            let day = if kind == MasterKind::Holiday {
                Some(date(&v[1])?)
            } else {
                None
            };
            self.apply(Change::SaveMaster {
                master: Master {
                    id: old.map_or(0, |m| m.id),
                    kind,
                    name: v[0].clone(),
                    date: day,
                },
            })?;
        }
        Ok(())
    }
    fn dependency(&mut self, remove: bool) -> Result<()> {
        let p = self
            .project
            .as_ref()
            .ok_or("プロジェクトを選択してください")?;
        if remove {
            let choices = p
                .dependencies
                .iter()
                .enumerate()
                .map(|(i, d)| {
                    (
                        i.to_string(),
                        format!("#{} → #{}", d.predecessor, d.successor),
                    )
                })
                .collect();
            if let Some(v) = form(
                self.hwnd,
                "依存関係を削除",
                vec![choice("関係", "0", choices, 0)],
                |_| Ok(()),
            ) {
                let index: usize = v[0].parse()?;
                let d = p.dependencies.get(index).ok_or("関係を選択してください")?;
                self.apply(Change::RemoveDependency {
                    predecessor: d.predecessor,
                    successor: d.successor,
                })?;
            }
        } else {
            let choices = p
                .tasks
                .iter()
                .map(|t| (t.id.to_string(), format!("#{} {}", t.id, t.title)))
                .collect::<Vec<_>>();
            let first = choices.first().map(|c| c.0.as_str()).unwrap_or("");
            let second = choices.get(1).map(|c| c.0.as_str()).unwrap_or("");
            if let Some(v) = form(
                self.hwnd,
                "依存関係を追加",
                vec![
                    choice("先行項目", first, choices.clone(), 0),
                    choice("後続項目", second, choices.clone(), 0),
                    field("待機営業日", 0, 0),
                ],
                |v| {
                    if v[0].is_empty() || v[1].is_empty() || v[0] == v[1] {
                        return Err("異なる2項目を選択してください".into());
                    }
                    if v[2].parse::<u32>().ok().is_none_or(|n| n > 3660) {
                        return Err("待機日数は0～3660です".into());
                    }
                    Ok(())
                },
            ) {
                self.apply(Change::AddDependency {
                    dependency: Dependency {
                        predecessor: v[0].parse()?,
                        successor: v[1].parse()?,
                        lag_days: v[2].parse()?,
                    },
                })?;
            }
        }
        Ok(())
    }
    fn command(&mut self, id: usize) -> Result<()> {
        match id {
            100 | 101 => {
                if let Some(path) = file_dialog(self.hwnd, id == 100, false) {
                    if id == 100 && path.exists() {
                        return Err(
                            "既存ファイルには作成できません。開く操作を使ってください。".into()
                        );
                    }
                    self.store = Store::open(&path, false)?;
                    self.path = path;
                    self.project = None;
                    self.selected = None;
                    self.reload()?;
                }
            }
            102 => self.project_form(true)?,
            103 => self.project_form(false)?,
            104 => {
                if let Some(path) = file_dialog(self.hwnd, true, true) {
                    let p = self
                        .project
                        .as_ref()
                        .ok_or("プロジェクトを選択してください")?;
                    if path
                        .canonicalize()
                        .ok()
                        .is_some_and(|d| self.path.canonicalize().ok().as_ref() == Some(&d))
                    {
                        return Err("使用中のデータベースには出力できません".into());
                    }
                    std::fs::write(path, serde_json::to_vec_pretty(p)?)?;
                }
            }
            110 => self.edit_task(true, false)?,
            111 => self.edit_task(true, true)?,
            112 => {
                if self.view == 7 {
                    self.master(false)?;
                } else {
                    self.edit_task(false, false)?;
                }
            }
            113 => {
                if let Some(id) = self.selected {
                    if self.view == 7 {
                        unsafe {
                            if MessageBoxW(
                                Some(self.hwnd),
                                w!("選択したマスタを削除しますか？"),
                                w!("yyproject"),
                                MB_YESNO | MB_ICONQUESTION,
                            ) == IDYES
                            {
                                self.apply(Change::RemoveMaster { id })?;
                                self.selected = None;
                            }
                        }
                    } else {
                        self.archive_form(id)?;
                    }
                }
            }
            119 => self.restore_form()?,
            114 => self.dependency(false)?,
            115 => self.dependency(true)?,
            116 => self.schedule_form()?,
            117 => self.apply(Change::Capture {
                day: chrono::Local::now().date_naive(),
            })?,
            118 => self.reload()?,
            130..=132 => {
                self.master_kind = match id {
                    130 => MasterKind::Category,
                    131 => MasterKind::Assignee,
                    _ => MasterKind::Holiday,
                };
                self.view = 7;
                self.selected = None;
                send(self.tabs, TCM_SETCURSEL, 7, 0);
                self.master(true)?;
            }
            140 => {
                yy_ui_win::help::show(
                    include_str!("../../help/help.md"),
                    "yyproject ヘルプ",
                    None,
                    "yyproject-help",
                )?;
            }
            200..=207 => {
                self.view = id - 200;
                self.x = 0;
                self.y = 0;
                self.selected = None;
                send(self.tabs, TCM_SETCURSEL, self.view, 0);
                self.focus_board();
            }
            412 => {
                self.filter = TaskFilter {
                    archived: true,
                    ..Default::default()
                };
                self.view = 0;
                self.selected = None;
                send(self.tabs, TCM_SETCURSEL, 0, 0);
                self.focus_board();
            }
            410 => self.filter_form()?,
            411 => {
                self.filter = TaskFilter::default();
                self.x = 0;
                self.y = 0;
                self.focus_board();
            }
            400..=404 => {
                self.view = 0;
                self.x = (id - 400) as i32 * self.px(240);
                self.y = 0;
                send(self.tabs, TCM_SETCURSEL, 0, 0);
            }
            300..=304 => {
                if let Some(t) = self.task().cloned() {
                    self.set_status(t, Status::ALL[id - 300])?;
                }
            }
            _ => {}
        }
        self.layout();
        invalidate(self.hwnd);
        Ok(())
    }
    fn layout(&mut self) {
        self.hits.clear();
        unsafe {
            for _ in 0..3 {
                let mut before = RECT::default();
                let _ = GetClientRect(self.hwnd, &mut before);
                self.layout_once();
                let mut after = RECT::default();
                let _ = GetClientRect(self.hwnd, &mut after);
                if before.right == after.right && before.bottom == after.bottom {
                    break;
                }
            }
        }
    }
    fn layout_once(&mut self) {
        unsafe {
            let mut r = RECT::default();
            let _ = GetClientRect(self.hwnd, &mut r);
            let top = self.px(42);
            let _ = MoveWindow(
                self.selector,
                self.px(110),
                self.px(6),
                (r.right - self.px(125)).max(1),
                self.px(240),
                true,
            );
            let _ = MoveWindow(self.tabs, 0, top, r.right.max(1), self.px(200), true);
            let rows = send(self.tabs, TCM_GETROWCOUNT, 0, 0).0.max(1) as i32;
            let tab_h = rows * self.px(28) + self.px(5);
            let _ = MoveWindow(self.tabs, 0, top, r.right.max(1), tab_h, true);
            self.body = rect(
                0,
                top + tab_h,
                r.right,
                (r.bottom - top - tab_h - self.px(28)).max(0),
            );
            let n = self
                .project
                .as_ref()
                .map_or(0, |p| p.tasks.iter().filter(|t| !t.archived).count())
                as i32;
            self.content = match self.view {
                0 => (
                    self.px(1200),
                    self.px(50)
                        + self.project.as_ref().map_or(0, |p| {
                            Status::ALL
                                .iter()
                                .map(|status| {
                                    p.tasks
                                        .iter()
                                        .filter(|t| t.status == *status && self.filter.matches(t))
                                        .count()
                                })
                                .max()
                                .unwrap_or(0)
                        }) as i32
                            * self.px(88),
                ),
                1 => (
                    self.project.as_ref().map_or(1000, |_| {
                        self.px(230)
                            + self
                                .gantt_range()
                                .map_or(30, |(first, last)| ((last - first).num_days() + 2).max(30))
                                as i32
                                * self.px(20)
                    }),
                    (n + 2) * self.px(30),
                ),
                2 | 3 => (
                    self.px(1800),
                    (self.project.as_ref().map_or(0, |p| {
                        p.tasks
                            .iter()
                            .filter(|t| self.filter.matches(t))
                            .filter(|t| {
                                if self.view == 3 {
                                    t.kind == Kind::Issue
                                } else {
                                    t.kind == Kind::Task
                                }
                            })
                            .count()
                    }) as i32
                        + 2)
                        * self.px(30),
                ),
                4 => (
                    (self.network.values().map(|v| v.0).max().unwrap_or(0) + 1) as i32
                        * self.px(230),
                    (self.network.values().map(|v| v.1).max().unwrap_or(0) + 1) as i32
                        * self.px(100),
                ),
                7 => (
                    self.px(800),
                    self.project.as_ref().map_or(1, |p| p.masters.len() + 2) as i32 * self.px(30),
                ),
                _ => (r.right, self.body.bottom - self.body.top),
            };
            self.x = self.x.clamp(0, (self.content.0 - self.body.right).max(0));
            self.y = self.y.clamp(
                0,
                (self.content.1 - (self.body.bottom - self.body.top)).max(0),
            );
            for (bar, size, page, pos) in [
                (SB_HORZ, self.content.0, self.body.right, self.x),
                (
                    SB_VERT,
                    self.content.1,
                    self.body.bottom - self.body.top,
                    self.y,
                ),
            ] {
                let info = SCROLLINFO {
                    cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
                    fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
                    nMin: 0,
                    nMax: size.max(1) - 1,
                    nPage: page.max(1) as u32,
                    nPos: pos,
                    ..Default::default()
                };
                SetScrollInfo(self.hwnd, bar, &info, true);
            }
        }
    }
    fn paint(&mut self, dc: HDC, r: RECT) {
        fill(dc, r, COLORREF(0xffffff));
        unsafe {
            SelectObject(dc, self.font.into());
        }
        let black = COLORREF(0x282828);
        let grey = COLORREF(0xf0f0f0);
        let blue = COLORREF(0xc06c28);
        label(
            dc,
            rect(self.px(10), 0, self.px(100), self.px(38)),
            "プロジェクト",
            black,
        );
        let footer = self.project.as_ref().map_or_else(
            || "ファイル → プロジェクトを追加 から始めてください".into(),
            |p| {
                format!(
                    "{} 絞込 {} / {}件（カンバン・一覧） | {} | 版 {} | {} | {}",
                    if self.filter.archived {
                        "保管済み"
                    } else {
                        "通常"
                    },
                    p.tasks.iter().filter(|t| self.filter.matches(t)).count(),
                    p.tasks.len(),
                    self.path.display(),
                    p.revision,
                    self.status,
                    self.master_label()
                )
            },
        );
        label(
            dc,
            rect(8, r.bottom - self.px(26), r.right - 16, self.px(26)),
            &footer,
            black,
        );
        self.hits.clear();
        let a = self.body;
        if a.bottom <= a.top {
            return;
        }
        let saved = unsafe { SaveDC(dc) };
        unsafe {
            IntersectClipRect(dc, a.left, a.top, a.right, a.bottom);
        }
        let Some(p) = self.project.as_ref() else {
            label(
                dc,
                a,
                "プロジェクトを追加するか、サンプルDBを開いてください。",
                black,
            );
            unsafe {
                let _ = RestoreDC(dc, saved);
            }
            return;
        };
        let row = self.px(30);
        let x = a.left - self.x;
        let top = a.top - self.y;
        let master_name = |id: Option<i64>| {
            p.masters
                .iter()
                .find(|m| Some(m.id) == id)
                .map_or("", |m| m.name.as_str())
        };
        match self.view {
            0 => {
                let row = self.px(20);
                for (col, status) in Status::ALL.iter().enumerate() {
                    let cx = x + col as i32 * self.px(240);
                    let width = self.px(232);
                    if cx > a.right || cx + width < a.left {
                        continue;
                    }
                    fill(dc, rect(cx, a.top, width, row), color(*status));
                    label(
                        dc,
                        rect(cx + 8, a.top, width - 16, row),
                        &format!(
                            "{} ({})",
                            status.label(),
                            p.tasks
                                .iter()
                                .filter(|t| t.status == *status && self.filter.matches(t))
                                .count()
                        ),
                        black,
                    );
                    for (i, t) in p
                        .tasks
                        .iter()
                        .filter(|t| t.status == *status && self.filter.matches(t))
                        .enumerate()
                    {
                        let y = top + row + self.px(2) + i as i32 * self.px(88);
                        let card = rect(cx + 4, y, width - 8, self.px(80));
                        if card.bottom < a.top + row || card.top > a.bottom {
                            continue;
                        }
                        fill(
                            dc,
                            card,
                            if self.selected == Some(t.id) {
                                COLORREF(0xf8dfbb)
                            } else {
                                grey
                            },
                        );
                        label(
                            dc,
                            rect(card.left + 6, y, card.right - card.left - 12, row),
                            &format!("#{} {}", t.id, t.title),
                            black,
                        );
                        label(
                            dc,
                            rect(card.left + 6, y + row, card.right - card.left - 12, row),
                            &format!(
                                "{} · {}% · {}h",
                                master_name(t.assignee),
                                t.progress,
                                t.estimate
                            ),
                            black,
                        );
                        label(
                            dc,
                            rect(card.left + 6, y + row * 2, card.right - card.left - 12, row),
                            &format!(
                                "{} / {} / {}",
                                t.priority.label(),
                                t.review.label(),
                                if t.include_in_effort {
                                    "工数算入"
                                } else {
                                    "工数除外"
                                }
                            ),
                            black,
                        );
                        self.hits.push((card, t.id));
                    }
                    fill(dc, rect(cx, a.top, width, row), color(*status));
                    label(
                        dc,
                        rect(cx + 8, a.top, width - 16, row),
                        &format!(
                            "{} ({})",
                            status.label(),
                            p.tasks
                                .iter()
                                .filter(|t| t.status == *status && self.filter.matches(t))
                                .count()
                        ),
                        black,
                    );
                }
            }
            2 | 3 => {
                let widths = [
                    60, 300, 100, 90, 110, 110, 130, 130, 110, 110, 80, 80, 130, 110, 140,
                ];
                let headers = [
                    "ID",
                    "件名",
                    "状態",
                    "優先度",
                    "重大度",
                    "レビュー",
                    "確認者",
                    "担当者",
                    "カテゴリ",
                    "開始日",
                    "期限",
                    "進捗",
                    "工数(h)",
                    "工数集計",
                    "対応タスクID",
                ];
                for (i, t) in p
                    .tasks
                    .iter()
                    .filter(|t| self.filter.matches(t))
                    .filter(|t| {
                        if self.view == 3 {
                            t.kind == Kind::Issue
                        } else {
                            t.kind == Kind::Task
                        }
                    })
                    .enumerate()
                {
                    let y = top + (i as i32 + 1) * row;
                    if y + row < a.top + row || y > a.bottom {
                        continue;
                    }
                    let values = [
                        t.id.to_string(),
                        t.title.clone(),
                        t.status.label().into(),
                        t.priority.label().into(),
                        t.severity.label().into(),
                        t.review.label().into(),
                        master_name(t.reviewer).into(),
                        master_name(t.assignee).into(),
                        master_name(t.category).into(),
                        t.start.map(|d| d.to_string()).unwrap_or_default(),
                        t.due.map(|d| d.to_string()).unwrap_or_default(),
                        format!("{}%", t.progress),
                        t.estimate.to_string(),
                        if t.include_in_effort {
                            "算入".into()
                        } else {
                            "除外".into()
                        },
                        t.related_tasks
                            .iter()
                            .map(|id| id.to_string())
                            .collect::<Vec<_>>()
                            .join(", "),
                    ];
                    let rr = rect(x, y, self.px(1800), row);
                    fill(
                        dc,
                        rr,
                        if self.selected == Some(t.id) {
                            COLORREF(0xf8dfbb)
                        } else if i % 2 == 0 {
                            COLORREF(0xfafafa)
                        } else {
                            COLORREF(0xffffff)
                        },
                    );
                    let mut cx = x;
                    for (value, width) in values.iter().zip(widths) {
                        let w = self.px(width);
                        label(dc, rect(cx + 4, y, w - 8, row), value, black);
                        line(dc, (cx + w, y), (cx + w, y + row), COLORREF(0xe0e0e0));
                        cx += w;
                    }
                    self.hits.push((rr, t.id));
                }
                fill(dc, rect(a.left, a.top, a.right - a.left, row), grey);
                let mut cx = x;
                for (header, width) in headers.iter().zip(widths) {
                    let w = self.px(width);
                    label(dc, rect(cx + 4, a.top, w - 8, row), header, black);
                    cx += w;
                }
            }
            1 => {
                let first = self.gantt_range().map_or(p.start, |r| r.0);
                let name_w = self.px(230);
                let day = self.px(20);
                let timeline = unsafe { SaveDC(dc) };
                unsafe {
                    IntersectClipRect(dc, a.left + name_w, a.top, a.right, a.bottom);
                }
                let day_start = (self.x / day).max(0);
                let day_end = day_start + (a.right - a.left) / day + 2;
                for n in day_start..day_end {
                    let Some(d) = first.checked_add_days(chrono::Days::new(n as u64)) else {
                        continue;
                    };
                    let cx = x + name_w + n * day;
                    if !p.is_workday(d) {
                        fill(
                            dc,
                            rect(cx, a.top, day, a.bottom - a.top),
                            COLORREF(0xf1f1f1),
                        );
                    }
                    line(dc, (cx, a.top), (cx, a.bottom), COLORREF(0xe8e8e8));
                    if n % 7 == 0 {
                        label(
                            dc,
                            rect(cx, a.top, self.px(100), row),
                            &d.format("%Y-%m-%d").to_string(),
                            black,
                        );
                    }
                }
                let bars = unsafe { SaveDC(dc) };
                unsafe {
                    IntersectClipRect(dc, a.left + name_w, a.top + row, a.right, a.bottom);
                }
                for (i, t) in p.tasks.iter().filter(|t| !t.archived).enumerate() {
                    let y = top + (i as i32 + 1) * row;
                    if y + row < a.top + row || y > a.bottom {
                        continue;
                    }
                    if let Some((start, due)) = t.start.zip(t.due) {
                        let bx = x + name_w + (start - first).num_days() as i32 * day;
                        let w = ((due - start).num_days() + 1).max(1) as i32 * day;
                        let bar = rect(bx, y + 5, w, row - 10);
                        fill(dc, bar, color(t.status));
                        fill(
                            dc,
                            rect(
                                bx,
                                y + row - 9,
                                (i64::from(w) * i64::from(t.progress) / 100) as i32,
                                4,
                            ),
                            blue,
                        );
                    }
                }
                unsafe {
                    let _ = RestoreDC(dc, bars);
                    let _ = RestoreDC(dc, timeline);
                }
                let names = unsafe { SaveDC(dc) };
                unsafe {
                    IntersectClipRect(dc, a.left, a.top + row, a.right, a.bottom);
                }
                for (i, t) in p.tasks.iter().filter(|t| !t.archived).enumerate() {
                    let y = top + (i as i32 + 1) * row;
                    if y + row <= a.top + row || y >= a.bottom {
                        continue;
                    }
                    label(
                        dc,
                        rect(a.left + 6, y, name_w - 12, row),
                        &format!("#{} {}", t.id, t.title),
                        black,
                    );
                    self.hits
                        .push((rect(a.left, y, a.right - a.left, row), t.id));
                }
                unsafe {
                    let _ = RestoreDC(dc, names);
                }
                fill(dc, rect(a.left, a.top, name_w, row), grey);
                label(
                    dc,
                    rect(a.left + 8, a.top, name_w - 16, row),
                    "項目 / 予定日程",
                    black,
                );
            }
            4 => {
                let graph = &self.network;
                let by_id: BTreeMap<_, _> = p.tasks.iter().map(|t| (t.id, t)).collect();
                let gap_x = self.px(230);
                let gap_y = self.px(100);
                let pos = |id| {
                    let (col, row) = graph[&id];
                    (x + col as i32 * gap_x + 12, top + row as i32 * gap_y + 12)
                };
                for d in p.active_dependencies() {
                    let (sx, sy) = pos(d.predecessor);
                    let (tx, ty) = pos(d.successor);
                    let start = (sx + self.px(190), sy + self.px(35));
                    let end = (tx, ty + self.px(35));
                    line(dc, start, end, blue);
                    line(dc, end, (end.0 - 8, end.1 - 5), blue);
                    line(dc, end, (end.0 - 8, end.1 + 5), blue);
                }
                for &id in graph.keys() {
                    let (nx, ny) = pos(id);
                    let box_r = rect(nx, ny, self.px(190), self.px(70));
                    if box_r.right < a.left
                        || box_r.left > a.right
                        || box_r.bottom < a.top
                        || box_r.top > a.bottom
                    {
                        continue;
                    }
                    let t = by_id[&id];
                    fill(dc, box_r, color(t.status));
                    label(
                        dc,
                        rect(nx + 6, ny, box_r.right - nx - 12, row),
                        &format!("#{} {}", id, t.title),
                        black,
                    );
                    label(
                        dc,
                        rect(nx + 6, ny + row, box_r.right - nx - 12, row),
                        &format!("{} · {}%", t.status.label(), t.progress),
                        black,
                    );
                    self.hits.push((box_r, id));
                }
            }
            5 | 6 => {
                let height = a.bottom - a.top;
                let width = a.right - a.left;
                let left = self.px(75).min(width / 4);
                let right = self.px(35).min(width / 10);
                let above = self.px(50).min(height / 3);
                let below = self.px(60).min(height / 3);
                let plot = rect(
                    a.left + left,
                    a.top + above,
                    (width - left - right).max(1),
                    (height - above - below).max(1),
                );
                let max = p
                    .history
                    .iter()
                    .map(|s| s.scope.max(s.remaining))
                    .fold(1.0, f64::max);
                let first = p.history.first().map_or(p.start, |s| s.day);
                let last = p.history.last().map_or(p.finish, |s| s.day);
                let span = (last - first).num_days().max(1) as f64;
                line(dc, (plot.left, plot.top), (plot.left, plot.bottom), black);
                line(
                    dc,
                    (plot.left, plot.bottom),
                    (plot.right, plot.bottom),
                    black,
                );
                let ticks = ((plot.bottom - plot.top) / self.px(28)).clamp(1, 4);
                for i in 0..=ticks {
                    let y = plot.bottom - (plot.bottom - plot.top) * i / ticks;
                    line(dc, (plot.left, y), (plot.right, y), COLORREF(0xe5e5e5));
                    label(
                        dc,
                        rect(
                            a.left,
                            y - self.px(12),
                            (plot.left - a.left - 4).max(1),
                            self.px(24),
                        ),
                        &format!("{:.1}h", max * i as f64 / f64::from(ticks)),
                        black,
                    );
                }
                let point = |s: &Snapshot, value: f64| {
                    (
                        plot.left
                            + ((s.day - first).num_days() as f64 / span
                                * (plot.right - plot.left) as f64)
                                as i32,
                        plot.bottom - (value / max * (plot.bottom - plot.top) as f64) as i32,
                    )
                };
                for (which, c) in if self.view == 5 {
                    vec![(0, blue)]
                } else {
                    vec![(1, COLORREF(0x398a39)), (2, COLORREF(0x999999))]
                } {
                    let value = |s: &Snapshot| match which {
                        0 => s.remaining,
                        1 => s.completed,
                        _ => s.scope,
                    };
                    for pair in p.history.windows(2) {
                        line(
                            dc,
                            point(&pair[0], value(&pair[0])),
                            point(&pair[1], value(&pair[1])),
                            c,
                        );
                    }
                    for s in &p.history {
                        let (cx, cy) = point(s, value(s));
                        fill(dc, rect(cx - 3, cy - 3, 6, 6), c);
                    }
                }
                label(
                    dc,
                    rect(plot.left, a.top, plot.right - plot.left, row),
                    if self.view == 5 {
                        "残工数（青）"
                    } else {
                        "完了工数（緑） / 総工数（灰）"
                    },
                    black,
                );
                let label_width = self.px(120).min((plot.right - plot.left) / 2);
                label(
                    dc,
                    rect(plot.left, plot.bottom + 4, label_width, row),
                    &first.to_string(),
                    black,
                );
                if last != first {
                    label(
                        dc,
                        rect(plot.right - label_width, plot.bottom + 4, label_width, row),
                        &last.to_string(),
                        black,
                    );
                }
                if p.history.len() < 2 {
                    label(
                        dc,
                        rect(plot.left, plot.top, plot.right - plot.left, row),
                        "実績は1日分です。日ごとの記録が増えると線で表示します。",
                        black,
                    );
                }
            }
            7 => {
                for (i, m) in p.masters.iter().enumerate() {
                    let y = top + (i as i32 + 1) * row;
                    if y + row < a.top + row || y > a.bottom {
                        continue;
                    }
                    let rr = rect(x, y, self.px(800), row);
                    fill(
                        dc,
                        rr,
                        if self.selected == Some(m.id) {
                            COLORREF(0xf8dfbb)
                        } else {
                            grey
                        },
                    );
                    label(
                        dc,
                        rect(x + 8, y, self.px(800) - 16, row),
                        &format!(
                            "#{}  {}  {}  {}",
                            m.id,
                            match m.kind {
                                MasterKind::Category => "カテゴリ",
                                MasterKind::Assignee => "担当者",
                                MasterKind::Holiday => "休日",
                            },
                            m.name,
                            m.date.map(|d| d.to_string()).unwrap_or_default()
                        ),
                        black,
                    );
                    self.hits.push((rr, m.id));
                }
                fill(dc, rect(a.left, a.top, a.right - a.left, row), grey);
                label(
                    dc,
                    rect(a.left + 8, a.top, a.right - a.left - 16, row),
                    "マスタ管理：編集メニューからカテゴリ・担当者・休日を追加できます。",
                    black,
                );
            }
            _ => {}
        }
        unsafe {
            let _ = RestoreDC(dc, saved);
        }
    }
    fn master_label(&self) -> &str {
        match self.master_kind {
            MasterKind::Category => "カテゴリ",
            MasterKind::Assignee => "担当者",
            MasterKind::Holiday => "休日",
        }
    }
}
fn menu() -> windows::core::Result<HMENU> {
    unsafe {
        let root = CreateMenu()?;
        let groups: Vec<(&str, Vec<(usize, String)>)> = vec![
            (
                "ファイル",
                vec![
                    (100, "データベースを作成".into()),
                    (101, "データベースを開く".into()),
                    (102, "プロジェクトを追加".into()),
                    (103, "プロジェクト設定".into()),
                    (104, "プロジェクトをJSON出力".into()),
                    (118, "再読み込み".into()),
                ],
            ),
            (
                "編集",
                vec![
                    (110, "タスクを追加".into()),
                    (111, "課題を追加".into()),
                    (112, "選択項目を編集".into()),
                    (113, "選択項目を保管（マスタは削除）…".into()),
                    (119, "保管した項目を復元…".into()),
                    (114, "依存関係を追加".into()),
                    (115, "依存関係を削除".into()),
                    (116, "営業日で日程を再計算".into()),
                    (117, "本日の進捗を記録".into()),
                    (130, "カテゴリを追加".into()),
                    (131, "担当者を追加".into()),
                    (132, "休日を追加".into()),
                ],
            ),
            (
                "表示",
                VIEWS
                    .iter()
                    .enumerate()
                    .map(|(i, s)| (200 + i, (*s).into()))
                    .collect(),
            ),
            (
                "状態",
                Status::ALL
                    .into_iter()
                    .enumerate()
                    .map(|(i, s)| (300 + i, s.label().into()))
                    .collect(),
            ),
            (
                "カンバンの列",
                Status::ALL
                    .into_iter()
                    .enumerate()
                    .map(|(i, s)| (400 + i, s.label().into()))
                    .collect(),
            ),
            (
                "絞り込み",
                vec![
                    (410, "検索条件を設定…".into()),
                    (411, "すべて解除".into()),
                    (412, "保管済みの項目を表示".into()),
                ],
            ),
            ("ヘルプ", vec![(140, "使い方 / MCP設定".into())]),
        ];
        for (title, items) in groups {
            let popup = CreatePopupMenu()?;
            for (id, name) in items {
                AppendMenuW(popup, MF_STRING, id, &HSTRING::from(name))?;
            }
            AppendMenuW(root, MF_POPUP, popup.0 as usize, &HSTRING::from(title))?;
        }
        Ok(root)
    }
}
unsafe extern "system" fn proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        if m == WM_DESTROY {
            PostQuitMessage(0);
            return LRESULT(0);
        }
        if m == WM_ERASEBKGND {
            return LRESULT(1);
        }
        if m == WM_PAINT {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(h, &mut ps);
            let mut r = RECT::default();
            let _ = GetClientRect(h, &mut r);
            APP.with(|a| {
                if let Ok(mut state) = a.try_borrow_mut()
                    && let Some(s) = state.as_mut()
                {
                    s.paint(dc, r);
                }
            });
            let _ = EndPaint(h, &ps);
            return LRESULT(0);
        }
        let handled = APP.with(|app| {
            let Ok(mut app) = app.try_borrow_mut() else {
                return false;
            };
            let Some(s) = app.as_mut() else {
                return false;
            };
            let result: Result<()> = match m {
                WM_SIZE => {
                    s.layout();
                    invalidate(h);
                    Ok(())
                }
                WM_COMMAND => {
                    let id = w.0 & 0xffff;
                    if id == 10 && (w.0 >> 16) & 0xffff == CBN_SELCHANGE as usize {
                        let index = send(s.selector, CB_GETCURSEL, 0, 0).0 as usize;
                        s.select_project(index)
                    } else {
                        s.command(id)
                    }
                }
                WM_NOTIFY => {
                    if (*(l.0 as *const NMHDR)).code == TCN_SELCHANGE {
                        s.view = send(s.tabs, TCM_GETCURSEL, 0, 0).0.max(0) as usize;
                        s.selected = None;
                        s.x = 0;
                        s.y = 0;
                        s.focus_board();
                        s.layout();
                        invalidate(h);
                    }
                    Ok(())
                }
                WM_TIMER => {
                    if s.store.data_version().ok().is_some_and(|v| v != s.version) {
                        s.reload()
                    } else {
                        Ok(())
                    }
                }
                WM_VSCROLL | WM_HSCROLL => {
                    let vertical = m == WM_VSCROLL;
                    let mut info = SCROLLINFO {
                        cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
                        fMask: SIF_TRACKPOS,
                        ..Default::default()
                    };
                    let _ = GetScrollInfo(h, if vertical { SB_VERT } else { SB_HORZ }, &mut info);
                    let pos = if vertical { &mut s.y } else { &mut s.x };
                    match (w.0 & 0xffff) as i32 {
                        0 => *pos -= 30,
                        1 => *pos += 30,
                        2 => *pos -= 200,
                        3 => *pos += 200,
                        5 => *pos = info.nTrackPos,
                        _ => {}
                    }
                    s.layout();
                    invalidate(h);
                    Ok(())
                }
                WM_MOUSEWHEEL => {
                    let delta = ((w.0 >> 16) as u16 as i16) as i32;
                    let horizontal = GetKeyState(VK_SHIFT.0 as i32) < 0;
                    if horizontal {
                        s.x -= delta / 120 * px(h, 90);
                    } else {
                        s.y -= delta / 120 * px(h, 90);
                    }
                    s.layout();
                    invalidate(h);
                    Ok(())
                }
                WM_LBUTTONDOWN => {
                    let x = l.0 as u16 as i16 as i32;
                    let y = (l.0 >> 16) as u16 as i16 as i32;
                    let _ = SetFocus(Some(h));
                    s.selected = s
                        .hits
                        .iter()
                        .rev()
                        .find(|(r, _)| {
                            inside(*r, x, y)
                                && inside(s.body, x, y)
                                && (s.view == 4
                                    || y >= s.body.top + px(h, if s.view == 0 { 20 } else { 30 }))
                        })
                        .map(|(_, id)| *id);
                    if s.view == 0 {
                        s.drag = s.selected;
                        let _ = SetCapture(h);
                    }
                    invalidate(h);
                    Ok(())
                }
                WM_LBUTTONUP => {
                    let _ = ReleaseCapture();
                    if let Some(id) = s.drag.take() {
                        let x = l.0 as u16 as i16 as i32;
                        let y = (l.0 >> 16) as u16 as i16 as i32;
                        if inside(s.body, x, y) {
                            let col = ((x + s.x) / px(h, 240)) as usize;
                            if let Some(status) = Status::ALL.get(col) {
                                if let Some(task) = s
                                    .project
                                    .as_ref()
                                    .and_then(|p| p.tasks.iter().find(|t| t.id == id))
                                    .cloned()
                                {
                                    s.set_status(task, *status)
                                } else {
                                    Ok(())
                                }
                            } else {
                                Ok(())
                            }
                        } else {
                            Ok(())
                        }
                    } else {
                        Ok(())
                    }
                }
                WM_LBUTTONDBLCLK => s.command(112),
                WM_KEYDOWN => match w.0 as u16 {
                    key if key == VK_F1.0 => s.command(140),
                    key if key == VK_RETURN.0 => s.command(112),
                    key if key == VK_DELETE.0 => s.command(113),
                    _ => Ok(()),
                },
                WM_DPICHANGED => {
                    let font = window_font(h);
                    let old = s.font;
                    s.font = font;
                    s.dpi = GetDpiForWindow(h).max(96) as i32;
                    for c in [s.selector, s.tabs] {
                        send(c, WM_SETFONT, font.0 as usize, 0);
                    }
                    let _ = DeleteObject(old.into());
                    let r = *(l.0 as *const RECT);
                    let _ = SetWindowPos(
                        h,
                        None,
                        r.left,
                        r.top,
                        r.right - r.left,
                        r.bottom - r.top,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                    s.layout();
                    invalidate(h);
                    Ok(())
                }
                _ => return false,
            };
            if let Err(e) = result {
                s.status = e.to_string();
                message(h, &e.to_string());
                invalidate(h);
            }
            true
        });
        if handled {
            LRESULT(0)
        } else {
            DefWindowProcW(h, m, w, l)
        }
    }
}
pub fn run() -> Result<()> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        let common = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_TAB_CLASSES,
        };
        let _ = InitCommonControlsEx(&common);
        yy_ui_win::font::register_gdi();
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        let smoke_dir = if args.first().is_some_and(|a| a == "--ui-smoke-test") {
            Some(PathBuf::from(args.get(1).ok_or("Missing smoke directory")?))
        } else {
            None
        };
        SMOKE.with(|s| s.set(smoke_dir.is_some()));
        let path = if let Some(directory) = &smoke_dir {
            std::fs::create_dir_all(directory)?;
            let path = directory.join("ui-test.sqlite3");
            if path.exists() {
                return Err("Smoke test requires a fresh directory".into());
            }
            path
        } else {
            args.first().map(PathBuf::from).unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_else(|| ".".into()))
                    .join("YYProject/projects.sqlite3")
            })
        };
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let store = Store::open(&path, false)?;
        let instance = GetModuleHandleW(None)?;
        RegisterClassW(&WNDCLASSW {
            style: CS_DBLCLKS,
            hInstance: instance.into(),
            lpszClassName: w!("YYProjectFrame"),
            lpfnWndProc: Some(proc),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hIcon: LoadIconW(
                Some(instance.into()),
                PCWSTR(std::ptr::without_provenance(1)),
            )?,
            ..Default::default()
        });
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("YYProjectFrame"),
            w!("yyproject — タスク・進捗・課題管理"),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN | WS_VSCROLL | WS_HSCROLL,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1180,
            760,
            None,
            Some(menu()?),
            Some(instance.into()),
            None,
        )?;
        let font = window_font(hwnd);
        let selector = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("COMBOBOX"),
            w!(""),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(CBS_DROPDOWNLIST as u32),
            0,
            0,
            1,
            1,
            Some(hwnd),
            Some(HMENU(std::ptr::without_provenance_mut(10))),
            Some(instance.into()),
            None,
        )?;
        let tabs = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            WC_TABCONTROLW,
            w!(""),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(TCS_FIXEDWIDTH),
            0,
            0,
            1,
            1,
            Some(hwnd),
            None,
            Some(instance.into()),
            None,
        )?;
        for c in [selector, tabs] {
            send(c, WM_SETFONT, font.0 as usize, 0);
        }
        send(
            tabs,
            TCM_SETITEMSIZE,
            0,
            ((px(hwnd, 28) as u32) << 16 | px(hwnd, 136) as u32) as isize,
        );
        for (i, title) in VIEWS.iter().enumerate() {
            let mut value: Vec<_> = title.encode_utf16().chain(Some(0)).collect();
            let item = TCITEMW {
                mask: TCIF_TEXT,
                pszText: windows::core::PWSTR(value.as_mut_ptr()),
                ..Default::default()
            };
            send(tabs, TCM_INSERTITEMW, i, &item as *const _ as isize);
        }
        let mut state = State {
            hwnd,
            selector,
            tabs,
            font,
            dpi: GetDpiForWindow(hwnd).max(96) as i32,
            network: BTreeMap::new(),
            store,
            path,
            projects: vec![],
            project: None,
            view: 0,
            filter: TaskFilter::default(),
            master_kind: MasterKind::Category,
            selected: None,
            x: 0,
            y: 0,
            content: (0, 0),
            hits: vec![],
            drag: None,
            version: 0,
            status: String::new(),
            body: RECT::default(),
        };
        state.reload()?;
        state.focus_board();
        APP.with(|a| *a.borrow_mut() = Some(state));
        SetTimer(Some(hwnd), 1, 1000, None);
        let _ = ShowWindow(
            hwnd,
            if smoke_dir.is_some() {
                SW_HIDE
            } else {
                SW_SHOW
            },
        );
        if let Some(directory) = &smoke_dir {
            smoke::run(hwnd, directory)?;
            DestroyWindow(hwnd)?;
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        APP.with(|a| {
            if let Some(s) = a.borrow_mut().take() {
                let _ = DeleteObject(s.font.into());
            }
        });
        CoUninitialize();
        Ok(())
    }
}
