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
mod advanced;
mod smoke;
thread_local! {static SMOKE:std::cell::Cell<bool>=const{std::cell::Cell::new(false)};}
thread_local! {static SMOKE_ERRORS:RefCell<Vec<String>>=const{RefCell::new(Vec::new())};}
thread_local! {static SMOKE_CANCEL_FORM:std::cell::Cell<bool>=const{std::cell::Cell::new(false)};}
thread_local! {static SMOKE_FORM_VALUES:RefCell<BTreeMap<String,String>>=const{RefCell::new(BTreeMap::new())};}
thread_local! {static SMOKE_FILE_PATH:RefCell<Option<PathBuf>>=const{RefCell::new(None)};}
thread_local! {static SMOKE_MENU_SIZE:std::cell::Cell<i32>=const{std::cell::Cell::new(8)};}
thread_local! {static SMOKE_CONTEXT_MENU:std::cell::Cell<bool>=const{std::cell::Cell::new(false)};}
const AUTO_FORM: u32 = WM_APP + 17;
const DRAG_TIMER: usize = 2;
const GRID_FINISH: u32 = WM_APP + 50;
mod bulk;
mod cases;
mod charts;
mod grid;
mod master_grid;
mod shell;
mod sizing;
mod timeline;
mod work;
use grid::GridState;
use timeline::Timeline;
#[derive(Clone, Copy)]
struct CardDrag {
    id: i64,
    start: POINT,
    pointer: POINT,
    offset: POINT,
    active: bool,
}
impl CardDrag {
    fn card(self, dpi: i32) -> RECT {
        rect(
            self.pointer.x - self.offset.x,
            self.pointer.y - self.offset.y,
            224 * dpi / 96,
            112 * dpi / 96,
        )
    }
}
const VIEWS: [&str; 15] = [
    "カンバン",
    "ガント",
    "タスク一覧",
    "課題一覧",
    "依存ネットワーク",
    "バーンダウン",
    "バーンアップ",
    "マスタ",
    "バグ一覧",
    "バグ統計",
    "課題統計",
    "依頼・質問一覧",
    "暫定対応一覧",
    "依頼・質問統計",
    "暫定対応統計",
];
fn network_family(p: &Project, selected: Option<i64>) -> std::collections::BTreeSet<i64> {
    let Some(id) = selected else {
        return Default::default();
    };
    let mut incoming: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
    let mut outgoing: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
    for d in p.active_dependencies() {
        incoming.entry(d.successor).or_default().push(d.predecessor);
        outgoing.entry(d.predecessor).or_default().push(d.successor);
    }
    let mut family = std::collections::BTreeSet::from([id]);
    for edges in [&incoming, &outgoing] {
        let mut seen = std::collections::BTreeSet::from([id]);
        let mut pending = vec![id];
        while let Some(current) = pending.pop() {
            for &to in edges.get(&current).into_iter().flatten() {
                if seen.insert(to) {
                    pending.push(to);
                    family.insert(to);
                }
            }
        }
    }
    family
}
thread_local! {static APP:RefCell<Option<State>>=const{RefCell::new(None)};}
struct State {
    hwnd: HWND,
    selector: HWND,
    tabs: HWND,
    master_tabs: HWND,
    shell_controls: Vec<(HWND, usize)>,
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
    case_range: Option<(chrono::NaiveDate, chrono::NaiveDate)>,
    selected: Option<i64>,
    x: i32,
    y: i32,
    content: (i32, i32),
    hits: Vec<(RECT, i64)>,
    drag: Option<CardDrag>,
    grid: GridState,
    timeline: Timeline,
    resize: Option<sizing::Resize>,
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
fn outline(dc: HDC, r: RECT, c: COLORREF) {
    line(dc, (r.left, r.top), (r.right - 1, r.top), c);
    line(dc, (r.right - 1, r.top), (r.right - 1, r.bottom - 1), c);
    line(dc, (r.right - 1, r.bottom - 1), (r.left, r.bottom - 1), c);
    line(dc, (r.left, r.bottom - 1), (r.left, r.top), c);
}
fn stroke(dc: HDC, a: (i32, i32), b: (i32, i32), c: COLORREF, width: i32) {
    unsafe {
        let pen = CreatePen(PS_SOLID, width.max(1), c);
        let old = SelectObject(dc, pen.into());
        let _ = MoveToEx(dc, a.0, a.1, None);
        let _ = LineTo(dc, b.0, b.1);
        SelectObject(dc, old);
        let _ = DeleteObject(pen.into());
    }
}
fn wrapped_label(dc: HDC, mut r: RECT, value: &str) {
    let mut title: Vec<_> = value.encode_utf16().collect();
    unsafe {
        SetTextColor(dc, COLORREF(0x473025));
        SetBkMode(dc, TRANSPARENT);
        DrawTextW(
            dc,
            &mut title,
            &mut r,
            DT_LEFT | DT_WORDBREAK | DT_EDITCONTROL | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
    }
}
fn window_font(h: HWND) -> HFONT {
    unsafe {
        CreateFontW(
            -px(h, 14),
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
            FF_DONTCARE.0 as u32,
            w!("Yu Gothic UI"),
        )
    }
}
fn invalidate(h: HWND) {
    unsafe {
        let _ = InvalidateRect(Some(h), None, false);
    }
}
fn file_dialog(h: HWND, save: bool, json: bool) -> Option<PathBuf> {
    if SMOKE.with(|s| s.get()) {
        return SMOKE_FILE_PATH.with(|p| p.borrow_mut().take());
    }
    unsafe {
        let filter: Vec<u16> = if json {
            "CSV\0*.csv\0\0"
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
type FormValidator = dyn Fn(&[String]) -> std::result::Result<(), String>;
struct Form {
    fields: Vec<Field>,
    controls: Vec<(HWND, HWND)>,
    tabs: Option<HWND>,
    page: usize,
    done: bool,
    result: Option<Vec<String>>,
    validate: Box<FormValidator>,
}
impl Form {
    fn layout(&self, h: HWND) {
        unsafe {
            let mut r = RECT::default();
            let _ = GetClientRect(h, &mut r);
            let margin = px(h, 14);
            let mut top = margin;
            if let Some(t) = self.tabs {
                let width = (r.right - 2 * margin).max(1);
                let _ = MoveWindow(t, margin, px(h, 6), width, px(h, 200), false);
                let tab_h = send(t, TCM_GETROWCOUNT, 0, 0).0.max(1) as i32 * px(h, 28) + px(h, 5);
                let _ = MoveWindow(t, margin, px(h, 6), width, tab_h, true);
                top = px(h, 6) + tab_h + px(h, 8);
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
                let lw = px(h, 210).min(r.right / 3);
                let ew = (r.right - lw - 3 * margin).max(1);
                let _ = MoveWindow(*l, margin, y, lw, (row - px(h, 6)).max(px(h, 24)), true);
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
                WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT => {
                    let dc = HDC(w.0 as *mut std::ffi::c_void);
                    SetTextColor(dc, shell::INK);
                    SetBkColor(dc, COLORREF(0xffffff));
                    SetDCBrushColor(dc, COLORREF(0xffffff));
                    return LRESULT(GetStockObject(DC_BRUSH).0 as isize);
                }
                WM_ERASEBKGND => {
                    let mut area = RECT::default();
                    let _ = GetClientRect(h, &mut area);
                    fill(HDC(w.0 as *mut std::ffi::c_void), area, COLORREF(0xffffff));
                    return LRESULT(1);
                }
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
                        let override_value =
                            SMOKE_FORM_VALUES.with(|m| m.borrow().get(&field.name).cloned());
                        if let Some(value) = override_value.as_deref().or(value) {
                            if field.choices.is_empty() {
                                let _ = SetWindowTextW(*edit, &HSTRING::from(value));
                            } else if let Some(index) =
                                field.choices.iter().position(|(key, _)| key == value)
                            {
                                send(*edit, CB_SETCURSEL, index, 0);
                            }
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
    validate: impl Fn(&[String]) -> std::result::Result<(), String> + 'static,
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
            validate: Box::new(validate),
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
            let labels = if title.starts_with("選択行の属性") && title.contains("依頼・質問")
            {
                vec![
                    "担当・分類",
                    "状態・優先度",
                    "依頼先",
                    "相手・回答期限",
                    "回答・完了日時",
                ]
            } else if title.starts_with("選択行の属性") && title.contains("暫定対応") {
                vec!["担当・分類", "状態・優先度", "分類・恒久予定", "実施日時"]
            } else if title.starts_with("選択行の属性") && title.contains("バグ・課題") {
                vec![
                    "担当・分類",
                    "状態・優先度",
                    "期限・発生工程",
                    "原因・検出工程",
                    "対策難易度",
                    "検知難易度・完了日時",
                    "対策予定日時",
                ]
            } else if title.starts_with("選択行の属性") {
                vec!["担当・分類", "状態・優先度", "予定日程", "完了日・進捗"]
            } else if title.starts_with("依頼・質問管理") {
                vec![
                    "基本情報",
                    "相手・種別",
                    "回答・完了日時",
                    "説明",
                    "関連タスク",
                ]
            } else if title.starts_with("暫定対応管理") {
                vec![
                    "基本情報",
                    "分類・日程",
                    "発生理由",
                    "恒久対応の条件",
                    "暫定対応の内容",
                    "関連・説明",
                ]
            } else if title.starts_with("やりとり記録") {
                vec!["記録情報", "本文"]
            } else if title.starts_with("バグ管理") || title.starts_with("課題管理") {
                vec![
                    "基本情報",
                    "工程・原因・難易度",
                    "暫定対策",
                    "恒久対策",
                    "発生・検出・報告",
                    "対策日時",
                    "確認・関連",
                    "説明",
                    "レビュー",
                ]
            } else if title.starts_with("Jira") {
                vec!["出力条件", "課題タイプ", "出力列"]
            } else if title.starts_with("CSV") {
                vec!["出力条件", "出力列1", "出力列2", "出力列3", "出力列4"]
            } else {
                vec![
                    "基本情報",
                    "日程・進捗",
                    "説明",
                    "分類・確認",
                    "確認メモ",
                    "集計・関連",
                    "階層・節目",
                ]
            };
            for i in 0..=pages {
                let name = labels.get(i).map_or_else(
                    || {
                        if title.starts_with("CSV") {
                            format!("出力列{i}")
                        } else {
                            format!("設定{}", i + 1)
                        }
                    },
                    |s| (*s).to_string(),
                );
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
    if v.len() > 20 && !v[20].is_empty() {
        date(&v[20])?;
    }
    Ok(())
}
impl State {
    fn hit_task(&self, x: i32, y: i32) -> Option<i64> {
        if matches!(self.view, 2 | 3 | 7 | 8 | 11 | 12) {
            return self
                .grid_cell(POINT { x, y })
                .and_then(|(row, _)| self.grid_rows().get(row).map(|r| r.0));
        }
        if !inside(self.body, x, y)
            || (self.view != 4
                && y < self.body.top
                    + self.px(if self.view == 0 {
                        20
                    } else if self.view == 1 {
                        96
                    } else {
                        30
                    }))
        {
            return None;
        }
        self.hits
            .iter()
            .rev()
            .find(|(r, _)| inside(*r, x, y))
            .map(|(_, id)| *id)
    }
    fn task_context_menu(&mut self, mut point: POINT) -> Result<()> {
        self.cancel_drag();
        if !matches!(self.view, 0..=4 | 8 | 11 | 12) {
            return Ok(());
        }
        unsafe {
            if point.x == -1 && point.y == -1 {
                let Some((r, _)) = self.hits.iter().find(|(r, id)| {
                    Some(*id) == self.selected
                        && r.bottom > self.body.top
                        && r.top < self.body.bottom
                        && r.right > self.body.left
                        && r.left < self.body.right
                }) else {
                    return Ok(());
                };
                point = POINT {
                    x: r.left.max(self.body.left),
                    y: r.top.max(self.body.top),
                };
                let _ = ClientToScreen(self.hwnd, &mut point);
            } else {
                let mut client = point;
                let _ = ScreenToClient(self.hwnd, &mut client);
                if matches!(self.view, 2 | 3 | 8 | 11 | 12)
                    && client.y >= self.body.top
                    && client.y < self.body.top + self.px(30)
                {
                    return self.column_filter_menu(client);
                }
                let clicked = self.hit_task(client.x, client.y);
                if matches!(self.view, 2 | 3 | 8 | 11 | 12)
                    && clicked.is_some_and(|id| !self.selected_task_ids().contains(&id))
                {
                    self.select_grid_point(client, false);
                    self.cancel_drag();
                }
                self.selected = clicked;
            }
            let Some(task) = self.task() else {
                return Ok(());
            };
            let current = task.status;
            let multi =
                matches!(self.view, 2 | 3 | 8 | 11 | 12) && self.selected_task_ids().len() > 1;
            let can_record = task.kind == Kind::Inquiry && !task.archived && !multi;
            let can_add_child = task.kind == Kind::Task && !task.archived && !multi;
            invalidate(self.hwnd);
            let popup = CreatePopupMenu()?;
            // Destroy the owned menu even when population or command handling fails.
            let result = (|| -> Result<usize> {
                AppendMenuW(
                    popup,
                    MF_STRING | if multi { MF_GRAYED } else { MF_ENABLED },
                    112,
                    w!("項目を編集（1行）…\tEnter"),
                )?;
                AppendMenuW(
                    popup,
                    MF_STRING | if can_add_child { MF_ENABLED } else { MF_GRAYED },
                    121,
                    w!("子タスクを追加…"),
                )?;
                if can_record {
                    for (id, caption) in [
                        (129, "やりとりを記録…"),
                        (133, "やりとり記録を編集…"),
                        (134, "やりとり記録を削除…"),
                        (135, "やりとり記録を確認…"),
                    ] {
                        AppendMenuW(popup, MF_STRING, id, &HSTRING::from(caption))?;
                    }
                }
                if matches!(self.view, 2 | 3 | 8 | 11 | 12) {
                    AppendMenuW(popup, MF_STRING, 122, w!("選択行の属性を一括変更…"))?;
                    AppendMenuW(
                        popup,
                        MF_STRING
                            | if self.can_fill_down() {
                                MF_ENABLED
                            } else {
                                MF_GRAYED
                            },
                        123,
                        w!("選択範囲を下方向にコピー\tCtrl+D"),
                    )?;
                }
                AppendMenuW(popup, MF_SEPARATOR, 0, PCWSTR::null())?;
                for (index, status) in Status::ALL.iter().enumerate() {
                    AppendMenuW(
                        popup,
                        MF_STRING
                            | if *status == current {
                                MF_CHECKED
                            } else {
                                MF_UNCHECKED
                            },
                        300 + index,
                        &HSTRING::from(format!("{} に変更", status.label())),
                    )?;
                }
                Ok(TrackPopupMenu(
                    popup,
                    TPM_RETURNCMD | TPM_RIGHTBUTTON,
                    point.x,
                    point.y,
                    None,
                    self.hwnd,
                    None,
                )
                .0 as usize)
            })();
            let _ = DestroyMenu(popup);
            invalidate(self.hwnd);
            let id = result?;
            if id != 0 {
                self.command(id)?;
            }
        }
        Ok(())
    }
    fn cancel_drag(&mut self) {
        self.finish_resize(false);
        let repaint = self.drag.take().is_some() || self.grid.selecting;
        self.grid.selecting = false;
        unsafe {
            let _ = KillTimer(Some(self.hwnd), DRAG_TIMER);
            if GetCapture() == self.hwnd {
                let _ = ReleaseCapture();
            }
        }
        if repaint {
            invalidate(self.hwnd);
        }
    }
    fn drop_status(&self, point: POINT) -> Option<Status> {
        if self.view != 0 || !inside(self.body, point.x, point.y) {
            return None;
        }
        Status::ALL
            .get(((point.x - self.body.left + self.x) / self.px(240)) as usize)
            .copied()
    }
    fn move_drag(&mut self, point: POINT) {
        if let Some(drag) = self.drag.as_mut() {
            drag.pointer = point;
            unsafe {
                drag.active |= (point.x - drag.start.x).abs()
                    >= GetSystemMetricsForDpi(SM_CXDRAG, self.dpi as u32)
                    || (point.y - drag.start.y).abs()
                        >= GetSystemMetricsForDpi(SM_CYDRAG, self.dpi as u32);
            }
            invalidate(self.hwnd);
        }
    }
    fn scroll_drag(&mut self) {
        let Some(drag) = self.drag.filter(|d| d.active) else {
            return;
        };
        if !inside(self.body, drag.pointer.x, drag.pointer.y) {
            return;
        }
        let edge = self.px(36);
        let step = self.px(24);
        let shift = |v: i32, low: i32, high: i32| {
            // Keep the two edge zones disjoint even in a short viewport.
            let margin = edge.min(((high - low) / 3).max(1));
            if v < low + margin {
                -step
            } else if v >= high - margin {
                step
            } else {
                0
            }
        };
        let dx = if self.view == 0 {
            shift(drag.pointer.x, self.body.left, self.body.right)
        } else {
            0
        };
        let dy = shift(
            drag.pointer.y,
            self.body.top + self.px(if self.view == 0 { 20 } else { 30 }),
            self.body.bottom,
        );
        if dx == 0 && dy == 0 {
            return;
        }
        self.x += dx;
        self.y += dy;
        self.layout();
        invalidate(self.hwnd);
    }
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
        self.cancel_drag();
        self.grid.selecting = false;
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
        self.grid.selection = false;
        self.grid.selected_rows.clear();
        self.grid.row_selection = false;
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
                        p.tasks.iter().any(|t| {
                            t.kind == Kind::Task
                                && t.status == *status
                                && p.matches_filter(&self.filter, t)
                        })
                    })
                })
                .unwrap_or(0) as i32
                * self.px(240);
            self.y = 0;
            self.layout();
        }
    }
    fn set_status(&mut self, mut task: Task, status: Status) -> Result<()> {
        if task.status == status {
            return Ok(());
        }
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
                .chain(p.masters.iter().filter(move |m| m.kind == kind).map(|m| {
                    (
                        m.id.to_string(),
                        if m.kind == MasterKind::Category {
                            p.category_path(Some(m.id)).join(" / ")
                        } else {
                            m.name.clone()
                        },
                    )
                }))
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
        if !new
            && self
                .task()
                .is_some_and(|t| matches!(t.kind, Kind::Inquiry | Kind::Workaround))
        {
            return self.edit_work(false, self.task().unwrap().kind);
        }
        if issue || (!new && self.task().is_some_and(|t| t.kind != Kind::Task)) {
            return self.edit_case(
                new,
                if new {
                    Kind::Issue
                } else {
                    self.task().unwrap().kind
                },
            );
        }
        let p = self
            .project
            .as_ref()
            .ok_or("先にプロジェクトを作成してください")?;
        let mut t = if new {
            Task {
                kind: if issue { Kind::Issue } else { Kind::Task },
                duration_days: 1,
                parent: self.grid.new_parent,
                include_in_effort: !issue,
                ..Default::default()
            }
        } else {
            self.task().ok_or("項目を選択してください")?.clone()
        };
        let masters = |kind| {
            std::iter::once((String::new(), "（未指定）".into()))
                .chain(p.masters.iter().filter(move |m| m.kind == kind).map(|m| {
                    (
                        m.id.to_string(),
                        if m.kind == MasterKind::Category {
                            p.category_path(Some(m.id)).join(" / ")
                        } else {
                            m.name.clone()
                        },
                    )
                }))
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
        fields.extend([
            choice(
                "親タスク",
                t.parent.map(|id| id.to_string()).unwrap_or_default(),
                std::iter::once((String::new(), "（親なし）".into()))
                    .chain(
                        p.tasks
                            .iter()
                            .filter(|v| v.id != t.id && v.kind == Kind::Task)
                            .map(|v| (v.id.to_string(), format!("#{} {}", v.id, v.title))),
                    )
                    .collect(),
                6,
            ),
            choice(
                "マイルストーン",
                if t.milestone { "yes" } else { "no" },
                vec![
                    ("no".into(), "通常の項目".into()),
                    ("yes".into(), "マイルストーン（同日・工数0）".into()),
                ],
                6,
            ),
        ]);
        fields[10].multi = true;
        fields[15].multi = true;
        fields.push(field(
            "完了日（実績・状態は変更しない）",
            t.completed_on.map(|d| d.to_string()).unwrap_or_default(),
            6,
        ));
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
            t.parent = v[18].parse().ok();
            t.milestone = v[19] == "yes";
            t.completed_on = if v[20].is_empty() {
                None
            } else {
                Some(date(&v[20])?)
            };
            if t.milestone {
                let day = t
                    .start
                    .or(t.due)
                    .ok_or("マイルストーンの日付を指定してください")?;
                t.start = Some(day);
                t.due = Some(day);
                t.duration_days = 0;
                t.estimate = 0.0;
            }
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
        if kind == MasterKind::Category {
            fields.push(choice(
                "親カテゴリ（最大3階層）",
                old.and_then(|m| m.parent)
                    .map(|id| id.to_string())
                    .unwrap_or_default(),
                std::iter::once((String::new(), "（最上位）".into()))
                    .chain(
                        p.masters
                            .iter()
                            .filter(|m| {
                                m.kind == MasterKind::Category && old.is_none_or(|o| o.id != m.id)
                            })
                            .map(|m| (m.id.to_string(), p.category_path(Some(m.id)).join(" / "))),
                    )
                    .collect(),
                0,
            ));
        }
        if let Some(v) = form(self.hwnd, "マスタ項目", fields, move |v| {
            if v[0].trim().is_empty() {
                return Err("名前を入力してください".into());
            }
            if kind == MasterKind::Holiday {
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
                    parent: if kind == MasterKind::Category {
                        v[1].parse().ok()
                    } else {
                        None
                    },
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
        self.finish_cell_edit(true)?;
        self.cancel_drag();
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
            104 => self.export_csv_form()?,
            122 => self.bulk_edit_form()?,
            123 => self.grid_fill_down()?,
            109 => self.export_jira_form()?,
            105 => {
                let url = std::env::var("YYPROJECT_DATABASE_URL")
                    .map_err(|_| "YYPROJECT_DATABASE_URLを設定してください")?;
                self.store = Store::connect_postgres(&url, false)?;
                self.project = None;
                self.selected = None;
                self.reload()?;
            }
            106 => {
                if let Some(path) = file_dialog(self.hwnd, true, false) {
                    self.store.export_sqlite(&path)?;
                    self.status = "SQLiteへ書き出しました".into();
                }
            }
            107 => {
                let url = std::env::var("YYPROJECT_DATABASE_URL")
                    .map_err(|_| "YYPROJECT_DATABASE_URLを設定してください")?;
                let dump = self.store.snapshot()?;
                let mut target = Store::connect_postgres(&url, false)?;
                target.import(&dump)?;
                self.store = target;
                self.project = None;
                self.selected = None;
                self.reload()?;
            }
            420 => self.search_form()?,
            430 => self.timeline_form()?,
            440 => self.case_range_form()?,
            124 => self.edit_case(true, Kind::Bug)?,
            127 => self.edit_work(true, Kind::Inquiry)?,
            128 => self.edit_work(true, Kind::Workaround)?,
            129 => self.exchange_form(true)?,
            133 => self.exchange_form(false)?,
            134 => self.remove_exchange_form()?,
            135 => self.show_exchange_log()?,
            136 | 137 => {
                self.command(if id == 136 { 211 } else { 212 })?;
                self.export_csv_form()?;
            }
            125 | 126 => {
                self.command(if id == 125 { 208 } else { 203 })?;
                self.export_csv_form()?;
            }
            110 => self.edit_task(true, false)?,
            111 => self.edit_task(true, true)?,
            121 => {
                let parent = self
                    .task()
                    .filter(|t| t.kind == Kind::Task && !t.archived)
                    .ok_or("保管されていない親タスクを選択してください")?
                    .id;
                self.grid.new_parent = Some(parent);
                let result = self.edit_task(true, false);
                self.grid.new_parent = None;
                result?;
            }
            112 => {
                if self.view == 7 {
                    self.master(false)?;
                } else if matches!(self.view, 2 | 3 | 8 | 11 | 12)
                    && self.selected_task_ids().len() > 1
                {
                    self.bulk_edit_form()?;
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
                let kind = match id {
                    130 => MasterKind::Category,
                    131 => MasterKind::Assignee,
                    _ => MasterKind::Holiday,
                };
                self.view = 7;
                self.select_master_kind(kind);
                send(self.tabs, TCM_SETCURSEL, 7, 0);
                self.master(true)?;
            }
            230..=232 => self.select_master_kind(match id {
                230 => MasterKind::Category,
                231 => MasterKind::Assignee,
                _ => MasterKind::Holiday,
            }),
            140 => {
                yy_ui_win::help::show(
                    include_str!("../../help/help.md"),
                    "yyproject ヘルプ",
                    None,
                    "yyproject-help",
                )?;
            }
            200..=214 => {
                self.view = id - 200;
                self.grid.all_kinds = false;
                self.grid.selection = false;
                self.grid.anchor = (0, 0);
                self.grid.current = (0, 0);
                self.grid.selected_rows.clear();
                self.grid.row_selection = false;
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
            let content_top = self.shell_layout(r);
            let left = self.sidebar_width() + self.px(22);
            let _ = MoveWindow(
                self.selector,
                self.px(172),
                self.px(20),
                (r.right - self.px(if r.right < self.px(700) { 194 } else { 390 }))
                    .max(self.px(50)),
                self.px(240),
                true,
            );
            let _ = ShowWindow(self.tabs, SW_HIDE);
            let master_h = if self.view == 7 { self.px(34) } else { 0 };
            let _ = ShowWindow(
                self.master_tabs,
                if self.view == 7 { SW_SHOW } else { SW_HIDE },
            );
            let _ = MoveWindow(
                self.master_tabs,
                left,
                content_top,
                (r.right - left - self.px(22)).max(1),
                master_h.max(1),
                true,
            );
            self.body = rect(
                left,
                content_top + master_h,
                (r.right - left - self.px(22)).max(1),
                (r.bottom - content_top - master_h - self.px(30)).max(0),
            );
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
                                        .filter(|t| {
                                            t.kind == Kind::Task
                                                && t.status == *status
                                                && p.matches_filter(&self.filter, t)
                                        })
                                        .count()
                                })
                                .max()
                                .unwrap_or(0)
                        }) as i32
                            * self.px(120),
                ),
                1 => (
                    self.project.as_ref().map_or(1000, |_| {
                        self.timeline.width(self.dpi, self.body)
                            + self.gantt_range().map_or(30, |(first, last)| {
                                ((last - self.timeline_first().unwrap_or(first)).num_days() + 2)
                                    .max(30)
                            }) as i32
                                * self.px(self.timeline.day_width)
                    }),
                    self.project.as_ref().map_or(0, |p| {
                        p.tasks
                            .iter()
                            .filter(|t| t.kind == Kind::Task && p.matches_filter(&self.filter, t))
                            .map(|t| self.px(self.timeline.height(t.id)))
                            .sum::<i32>()
                    }) + self.px(96),
                ),
                2 | 3 | 7 | 8 | 11 | 12 => (
                    self.px((0..self.grid_columns().len())
                        .map(|c| self.grid_width(c))
                        .sum()),
                    self.grid_rows()
                        .iter()
                        .map(|(id, _)| self.grid_height(*id))
                        .sum::<i32>()
                        + self.px(60),
                ),
                4 => (
                    (self.network.values().map(|v| v.0).max().unwrap_or(0) + 1) as i32
                        * self.px(230),
                    (self.network.values().map(|v| v.1).max().unwrap_or(0) + 1) as i32
                        * self.px(100),
                ),
                5 | 6 => (
                    self.project
                        .as_ref()
                        .map_or(r.right, |p| charts::width(p, self.dpi).max(r.right)),
                    self.body.bottom - self.body.top,
                ),
                9 | 10 | 13 | 14 => self.case_chart_size(),
                _ => (r.right, self.body.bottom - self.body.top),
            };
            self.x = self.x.clamp(
                0,
                (self.content.0 - (self.body.right - self.body.left)).max(0),
            );
            self.y = self.y.clamp(
                0,
                (self.content.1 - (self.body.bottom - self.body.top)).max(0),
            );
            for (bar, size, page, pos) in [
                (
                    SB_HORZ,
                    self.content.0,
                    self.body.right - self.body.left,
                    self.x,
                ),
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
        fill(dc, r, shell::BACKGROUND);
        unsafe {
            SelectObject(dc, self.font.into());
        }
        self.paint_shell(dc, r);
        let black = shell::INK;
        let grey = COLORREF(0xfdf9f7);
        let blue = shell::BLUE;
        let footer = self.project.as_ref().map_or_else(
            || "ファイル → プロジェクトを追加 から始めてください".into(),
            |p| {
                format!(
                    "{} | {} 表示 {} / {}件 | 版 {} | {} | {}",
                    if self.status.is_empty() {
                        VIEWS[self.view]
                    } else {
                        &self.status
                    },
                    if self.filter.archived {
                        "保管済み"
                    } else {
                        "通常"
                    },
                    if self.view == 7 {
                        p.masters
                            .iter()
                            .filter(|m| m.kind == self.master_kind)
                            .count()
                    } else if matches!(self.view, 2 | 3 | 8 | 11 | 12) {
                        self.grid_rows().len()
                    } else {
                        p.tasks
                            .iter()
                            .filter(|t| p.matches_filter(&self.filter, t))
                            .count()
                    },
                    if self.view == 7 {
                        p.masters.len()
                    } else {
                        p.tasks.len()
                    },
                    p.revision,
                    if self.store.is_postgres() {
                        "PostgreSQL".into()
                    } else {
                        self.path.display().to_string()
                    },
                    self.master_label()
                )
            },
        );
        label(
            dc,
            rect(
                self.body.left,
                r.bottom - self.px(26),
                r.right - self.body.left - self.px(16),
                self.px(26),
            ),
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
                                .filter(|t| t.kind == Kind::Task
                                    && t.status == *status
                                    && p.matches_filter(&self.filter, t))
                                .count()
                        ),
                        black,
                    );
                    for (i, t) in p
                        .tasks
                        .iter()
                        .filter(|t| {
                            t.kind == Kind::Task
                                && t.status == *status
                                && p.matches_filter(&self.filter, t)
                        })
                        .enumerate()
                    {
                        let y = top + row + self.px(2) + i as i32 * self.px(120);
                        let card = rect(cx + self.px(4), y, width - self.px(8), self.px(112));
                        if card.bottom < a.top + row || card.top > a.bottom {
                            continue;
                        }
                        fill(
                            dc,
                            card,
                            if self.drag.is_some_and(|d| d.active && d.id == t.id) {
                                COLORREF(0xeeeeee)
                            } else if self.selected == Some(t.id) {
                                COLORREF(0xfff0e6)
                            } else {
                                COLORREF(0xffffff)
                            },
                        );
                        outline(dc, card, shell::BORDER);
                        fill(
                            dc,
                            rect(card.left, card.top, self.px(3), card.bottom - card.top),
                            color(*status),
                        );
                        wrapped_label(
                            dc,
                            rect(
                                card.left + self.px(6),
                                y + self.px(4),
                                card.right - card.left - self.px(12),
                                self.px(44),
                            ),
                            &format!("#{} {}", t.id, t.title),
                        );
                        label(
                            dc,
                            rect(
                                card.left + self.px(6),
                                y + self.px(50),
                                card.right - card.left - self.px(12),
                                row,
                            ),
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
                            rect(
                                card.left + self.px(6),
                                y + self.px(74),
                                card.right - card.left - self.px(12),
                                row,
                            ),
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
                                .filter(|t| t.kind == Kind::Task
                                    && t.status == *status
                                    && p.matches_filter(&self.filter, t))
                                .count()
                        ),
                        black,
                    );
                }
            }
            2 | 3 | 8 | 11 | 12 => {
                let selected_rows = self.selected_task_ids();
                let list_kind = self.list_kind();
                for (i, (t, depth)) in p
                    .ordered_tasks()
                    .into_iter()
                    .filter(|(t, _)| p.matches_filter(&self.filter, t))
                    .filter(|(t, _)| self.grid.all_kinds || t.kind == list_kind)
                    .enumerate()
                {
                    let y = self.grid_row_top(i);
                    let row = self.grid_height(t.id);
                    if y + row <= a.top + self.px(30) || y > a.bottom {
                        continue;
                    }
                    let rr = rect(x, y, self.content.0, row);
                    fill(
                        dc,
                        rr,
                        if selected_rows.contains(&t.id) {
                            COLORREF(0xfff0e6)
                        } else if i % 2 == 0 {
                            COLORREF(0xfffcfa)
                        } else {
                            COLORREF(0xffffff)
                        },
                    );
                    let mut cx = x;
                    for (col, c) in self.grid_columns().iter().enumerate() {
                        let w = self.px(self.grid_width(col));
                        if self.grid.contains(i, col) {
                            fill(
                                dc,
                                rect(cx, y, w, row),
                                if selected_rows.len() > 1
                                    && !table::BULK_TASK_KEYS.contains(&c.key)
                                {
                                    COLORREF(0xe9e9e9)
                                } else {
                                    COLORREF(0xfff0e6)
                                },
                            );
                        }
                        let mut value = table::value(p, t, c.key);
                        if c.key == "title" {
                            value = format!(
                                "{}{}{}",
                                "  ".repeat(depth.min(20)),
                                if t.milestone { "◆ " } else { "" },
                                value
                            );
                        }
                        label(
                            dc,
                            rect(cx + self.px(4), y, w - self.px(8), row),
                            &value,
                            black,
                        );
                        line(dc, (cx + w, y), (cx + w, y + row), COLORREF(0xefe8e4));
                        cx += w;
                    }
                    line(
                        dc,
                        (x, y + row - 1),
                        (x + self.content.0, y + row - 1),
                        COLORREF(0xefe8e4),
                    );
                    self.hits.push((rr, t.id));
                }
                fill(dc, rect(a.left, a.top, a.right - a.left, row), grey);
                let mut cx = x;
                for (col, c) in self.grid_columns().iter().enumerate() {
                    let w = self.px(self.grid_width(col));
                    let filtered = self.filter.columns.contains_key(c.key);
                    label(
                        dc,
                        rect(cx + self.px(4), a.top, w - self.px(8), row),
                        &format!(
                            "{}{}{}",
                            if c.key == "id" { "↕ ID" } else { c.label },
                            if c.editable && !table::BULK_TASK_KEYS.contains(&c.key) {
                                if c.key == "title" {
                                    "（個別）"
                                } else {
                                    "※"
                                }
                            } else {
                                ""
                            },
                            if filtered { " ▼" } else { "" }
                        ),
                        if filtered { blue } else { black },
                    );
                    line(
                        dc,
                        (cx + w, a.top),
                        (cx + w, a.top + row),
                        COLORREF(0xe3d5cb),
                    );
                    cx += w;
                }
            }
            1 => timeline::paint(
                dc,
                p,
                &self.timeline,
                timeline::Viewport {
                    dpi: self.dpi,
                    body: a,
                    x: self.x,
                    y: self.y,
                },
                &self.filter,
                &mut self.hits,
            ),
            4 => {
                let graph = &self.network;
                let family = self
                    .project
                    .as_ref()
                    .map(|p| network_family(p, self.selected))
                    .unwrap_or_default();
                let focused = self.selected.is_some_and(|id| graph.contains_key(&id));
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
                    let active = !focused
                        || (family.contains(&d.predecessor) && family.contains(&d.successor));
                    let c = if active { blue } else { COLORREF(0xe5e5e5) };
                    stroke(dc, start, end, c, if active { self.px(2) } else { 1 });
                    line(dc, end, (end.0 - self.px(5), end.1 - self.px(4)), c);
                    line(dc, end, (end.0 - self.px(5), end.1 + self.px(4)), c);
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
                    let active = !focused || family.contains(&id);
                    let ink = if active { black } else { COLORREF(0xe3d5cb) };
                    fill(
                        dc,
                        box_r,
                        if active {
                            color(t.status)
                        } else {
                            COLORREF(0xf6f6f6)
                        },
                    );
                    if self.selected == Some(id) {
                        outline(dc, box_r, blue);
                    }
                    label(
                        dc,
                        rect(nx + 6, ny, box_r.right - nx - 12, row),
                        &format!("#{} {}", id, t.title),
                        ink,
                    );
                    label(
                        dc,
                        rect(nx + 6, ny + row, box_r.right - nx - 12, row),
                        &format!("{} · {}%", t.status.label(), t.progress),
                        ink,
                    );
                    self.hits.push((box_r, id));
                }
            }
            5 | 6 => charts::paint(
                dc,
                p,
                timeline::Viewport {
                    dpi: self.dpi,
                    body: a,
                    x: self.x,
                    y: self.y,
                },
                self.view == 6,
            ),
            7 => {
                let mut hits = Vec::new();
                self.paint_masters(dc, &mut hits);
                self.hits.extend(hits);
            }
            9 | 10 | 13 | 14 => self.paint_case_statistics(dc),
            _ => {}
        }
        if let Some(drag) = self.drag.filter(|d| d.active && self.view == 0) {
            if let Some(status) = self.drop_status(drag.pointer) {
                let col = Status::ALL.iter().position(|s| *s == status).unwrap();
                let target = rect(
                    x + col as i32 * self.px(240),
                    a.top,
                    self.px(232),
                    a.bottom - a.top,
                );
                outline(dc, target, blue);
                outline(
                    dc,
                    rect(
                        target.left + 1,
                        target.top + 1,
                        target.right - target.left - 2,
                        target.bottom - target.top - 2,
                    ),
                    blue,
                );
            }
            if let Some(task) = p.tasks.iter().find(|t| t.id == drag.id) {
                let card = drag.card(self.dpi);
                fill(
                    dc,
                    rect(
                        card.left + self.px(4),
                        card.top + self.px(4),
                        card.right - card.left,
                        card.bottom - card.top,
                    ),
                    COLORREF(0xb8b8b8),
                );
                fill(dc, card, COLORREF(0xfff0e6));
                outline(dc, card, blue);
                wrapped_label(
                    dc,
                    rect(
                        card.left + self.px(8),
                        card.top + self.px(6),
                        card.right - card.left - self.px(16),
                        self.px(44),
                    ),
                    &format!("#{} {}", task.id, task.title),
                );
                label(
                    dc,
                    rect(
                        card.left + self.px(8),
                        card.top + self.px(54),
                        card.right - card.left - self.px(16),
                        self.px(24),
                    ),
                    &self.drop_status(drag.pointer).map_or_else(
                        || "移動先の列で離してください".into(),
                        |s| format!("→ {} へ移動", s.label()),
                    ),
                    black,
                );
                label(
                    dc,
                    rect(
                        card.left + self.px(8),
                        card.top + self.px(82),
                        card.right - card.left - self.px(16),
                        self.px(24),
                    ),
                    "Escでキャンセル",
                    black,
                );
            }
        }
        if matches!(self.view, 2 | 3 | 8 | 11 | 12)
            && let Some(drag) = self.drag.filter(|d| d.active)
        {
            let target = self
                .hits
                .iter()
                .find(|(r, _)| drag.pointer.y >= r.top && drag.pointer.y < r.bottom)
                .map(|(r, _)| r.top)
                .unwrap_or(a.bottom - self.px(3))
                .max(a.top + row);
            fill(dc, rect(a.left, target, a.right - a.left, self.px(3)), blue);
            if let Some(t) = p.tasks.iter().find(|t| t.id == drag.id) {
                let r = rect(
                    drag.pointer.x + self.px(12),
                    drag.pointer.y + self.px(8),
                    self.px(300),
                    row,
                );
                fill(dc, r, COLORREF(0xfff0e6));
                label(dc, r, &format!("移動: #{} {}", t.id, t.title), black);
            }
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
                    (104, "プロジェクトをCSV出力…".into()),
                    (125, "バグ一覧をCSV出力…".into()),
                    (126, "課題一覧をCSV出力…".into()),
                    (136, "依頼・質問一覧をCSV出力…".into()),
                    (137, "暫定対応一覧をCSV出力…".into()),
                    (109, "Jira用CSVを出力…".into()),
                    (105, "PostgreSQLへ接続（環境変数）".into()),
                    (107, "現在のSQLiteからPostgreSQLへ移行…".into()),
                    (106, "SQLiteへ書き出し…".into()),
                    (118, "再読み込み".into()),
                ],
            ),
            (
                "編集",
                vec![
                    (110, "タスクを追加".into()),
                    (121, "選択したタスクの子タスクを追加…".into()),
                    (111, "課題を追加".into()),
                    (124, "バグを追加".into()),
                    (127, "依頼・質問を追加…".into()),
                    (128, "暫定対応を追加…".into()),
                    (129, "やりとりを記録…".into()),
                    (133, "やりとり記録を編集…".into()),
                    (134, "やりとり記録を削除…".into()),
                    (135, "やりとり記録を表示…".into()),
                    (112, "選択項目を編集".into()),
                    (122, "選択行の属性を一括変更…".into()),
                    (123, "選択範囲を下方向にコピー\tCtrl+D".into()),
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
            (
                "検索",
                vec![
                    (420, "タスクの全文検索…\tCtrl+F".into()),
                    (411, "絞り込みをすべて解除".into()),
                ],
            ),
            ("ガント設定", vec![(430, "表示起点・列幅・稲妻線…".into())]),
            ("統計設定", vec![(440, "集計期間（UTC日付）…".into())]),
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
        if m == WM_INITMENUPOPUP && SMOKE_CONTEXT_MENU.with(|flag| flag.replace(false)) {
            let popup = HMENU(w.0 as *mut std::ffi::c_void);
            if GetMenuItemCount(Some(popup)) != SMOKE_MENU_SIZE.with(|s| s.get()) {
                SMOKE_ERRORS.with(|errors| {
                    errors
                        .borrow_mut()
                        .push("Task context menu is missing actions".into())
                });
            }
            // Close only this owned popup through its real modal message loop.
            let _ = PostMessageW(Some(h), WM_CANCELMODE, WPARAM(0), LPARAM(0));
            return LRESULT(0);
        }
        if m == WM_DRAWITEM && l.0 != 0 {
            let item = &*(l.0 as *const DRAWITEMSTRUCT);
            if GetParent(item.hwndItem).ok() == Some(h) {
                shell::draw_button(item);
                return LRESULT(1);
            }
        }
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
            let buffer = CreateCompatibleDC(Some(dc));
            let bitmap = CreateCompatibleBitmap(dc, r.right.max(1), r.bottom.max(1));
            let buffered = !buffer.is_invalid() && !bitmap.is_invalid();
            let previous = if buffered {
                SelectObject(buffer, bitmap.into())
            } else {
                HGDIOBJ::default()
            };
            let painted = APP.with(|a| {
                if let Ok(mut state) = a.try_borrow_mut()
                    && let Some(s) = state.as_mut()
                {
                    s.paint(if buffered { buffer } else { dc }, r);
                    true
                } else {
                    false
                }
            });
            if buffered {
                if painted {
                    let _ = BitBlt(dc, 0, 0, r.right, r.bottom, Some(buffer), 0, 0, SRCCOPY);
                }
                SelectObject(buffer, previous);
            }
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(buffer);
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
                    } else if id == 11 && (w.0 >> 16) & 0xffff == CBN_SELCHANGE as usize {
                        let index = s
                            .shell_controls
                            .iter()
                            .find(|(_, id)| *id == 11)
                            .map(|(h, _)| send(*h, CB_GETCURSEL, 0, 0).0.max(0) as usize)
                            .unwrap_or(0);
                        s.command(200 + index)
                    } else {
                        let toolbar =
                            l.0 != 0 && s.shell_controls.iter().any(|(h, _)| h.0 as isize == l.0);
                        let id = if toolbar && id == 110 {
                            match s.view {
                                3 | 10 => 111,
                                8 | 9 => 124,
                                11 | 13 => 127,
                                12 | 14 => 128,
                                7 => match s.master_kind {
                                    MasterKind::Category => 130,
                                    MasterKind::Assignee => 131,
                                    MasterKind::Holiday => 132,
                                },
                                _ => 110,
                            }
                        } else {
                            id
                        };
                        let _ = SetFocus(Some(h));
                        s.command(id)
                    }
                }
                WM_NOTIFY => (|| -> Result<()> {
                    if (*(l.0 as *const NMHDR)).code == TCN_SELCHANGE
                        && (*(l.0 as *const NMHDR)).hwndFrom == s.master_tabs
                    {
                        s.finish_cell_edit(true)?;
                        let kind = match send(s.master_tabs, TCM_GETCURSEL, 0, 0).0 {
                            0 => MasterKind::Category,
                            1 => MasterKind::Assignee,
                            _ => MasterKind::Holiday,
                        };
                        s.select_master_kind(kind);
                    } else if (*(l.0 as *const NMHDR)).code == TCN_SELCHANGE {
                        s.finish_cell_edit(true)?;
                        s.cancel_drag();
                        s.grid.all_kinds = false;
                        s.grid.selection = false;
                        s.grid.anchor = (0, 0);
                        s.grid.current = (0, 0);
                        s.grid.selected_rows.clear();
                        s.grid.row_selection = false;
                        s.view = send(s.tabs, TCM_GETCURSEL, 0, 0).0.max(0) as usize;
                        s.selected = None;
                        s.x = 0;
                        s.y = 0;
                        s.focus_board();
                        s.layout();
                        invalidate(h);
                    }
                    Ok(())
                })(),
                WM_TIMER => {
                    if w.0 == DRAG_TIMER {
                        s.scroll_drag();
                        Ok(())
                    } else if s.store.data_version().ok().is_some_and(|v| v != s.version) {
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
                WM_LBUTTONDOWN => (|| -> Result<()> {
                    s.finish_cell_edit(true)?;
                    let x = l.0 as u16 as i16 as i32;
                    let y = (l.0 >> 16) as u16 as i16 as i32;
                    let _ = SetFocus(Some(h));
                    if s.begin_resize(POINT { x, y }) {
                        return Ok(());
                    }
                    if matches!(s.view, 2 | 3 | 8 | 11 | 12) && GetKeyState(VK_CONTROL.0 as i32) < 0
                    {
                        s.toggle_grid_row(POINT { x, y });
                        return Ok(());
                    }
                    s.selected = s.hit_task(x, y);
                    if matches!(s.view, 2 | 3 | 7 | 8 | 11 | 12) {
                        s.select_grid_point(POINT { x, y }, GetKeyState(VK_SHIFT.0 as i32) < 0);
                    }
                    if (s.view == 0
                        || (matches!(s.view, 2 | 3 | 8 | 11 | 12) && s.grid.current.1 == 0))
                        && let Some((r, id)) = s.hits.iter().find(|(_, id)| Some(*id) == s.selected)
                    {
                        s.drag = Some(CardDrag {
                            id: *id,
                            start: POINT { x, y },
                            pointer: POINT { x, y },
                            offset: POINT {
                                x: x - r.left,
                                y: y - r.top,
                            },
                            active: false,
                        });
                        let _ = SetCapture(h);
                        SetTimer(Some(h), DRAG_TIMER, 50, None);
                    }
                    invalidate(h);
                    Ok(())
                })(),
                WM_LBUTTONUP => {
                    s.finish_resize(true);
                    let drag = s.drag;
                    s.grid.selecting = false;
                    s.cancel_drag();
                    let point = POINT {
                        x: l.0 as u16 as i16 as i32,
                        y: (l.0 >> 16) as u16 as i16 as i32,
                    };
                    if matches!(s.view, 2 | 3 | 8 | 11 | 12)
                        && let Some(drag) = drag.filter(|d| d.active)
                        && inside(s.body, point.x, point.y)
                    {
                        let before = s.hit_task(point.x, point.y);
                        if before != Some(drag.id) {
                            {
                                s.selected = Some(drag.id);
                                s.grid.selection = false;
                                s.apply(Change::MoveTask {
                                    id: drag.id,
                                    before,
                                })
                            }
                        } else {
                            Ok(())
                        }
                    } else if let Some(drag) = drag.filter(|d| d.active)
                        && let Some(status) = s.drop_status(point)
                        && let Some(task) = s
                            .project
                            .as_ref()
                            .and_then(|p| p.tasks.iter().find(|t| t.id == drag.id))
                            .cloned()
                    {
                        s.set_status(task, status)
                    } else {
                        Ok(())
                    }
                }
                WM_SETCURSOR => {
                    let mut point = POINT::default();
                    let _ = GetCursorPos(&mut point);
                    let _ = ScreenToClient(h, &mut point);
                    if let Some(target) = s.resize_target(point) {
                        let cursor = if matches!(target, sizing::Target::Row(_)) {
                            IDC_SIZENS
                        } else {
                            IDC_SIZEWE
                        };
                        SetCursor(LoadCursorW(None, cursor).ok());
                        Ok(())
                    } else {
                        return false;
                    }
                }
                WM_MOUSEMOVE => {
                    if s.resize.is_some() {
                        s.move_resize(POINT {
                            x: l.0 as u16 as i16 as i32,
                            y: (l.0 >> 16) as u16 as i16 as i32,
                        });
                        return true;
                    }
                    if s.grid.selecting && s.drag.is_none() && w.0 & 1 != 0 {
                        s.select_grid_point(
                            POINT {
                                x: l.0 as u16 as i16 as i32,
                                y: (l.0 >> 16) as u16 as i16 as i32,
                            },
                            true,
                        );
                    }
                    s.move_drag(POINT {
                        x: l.0 as u16 as i16 as i32,
                        y: (l.0 >> 16) as u16 as i16 as i32,
                    });
                    Ok(())
                }
                WM_CAPTURECHANGED | WM_CANCELMODE | WM_RBUTTONDOWN => {
                    s.cancel_drag();
                    Ok(())
                }
                WM_ACTIVATEAPP => {
                    if w.0 == 0 {
                        s.cancel_drag();
                    }
                    Ok(())
                }
                GRID_FINISH => s.finish_grid_message(w.0 == 1, l.0 != 0),
                WM_LBUTTONDBLCLK => {
                    if matches!(s.view, 2 | 3 | 7 | 8 | 11 | 12) {
                        s.begin_cell_edit(None)
                    } else {
                        s.command(112)
                    }
                }
                WM_CONTEXTMENU => s.task_context_menu(POINT {
                    x: l.0 as u16 as i16 as i32,
                    y: (l.0 >> 16) as u16 as i16 as i32,
                }),
                WM_CHAR
                    if matches!(s.view, 2 | 3 | 7 | 8 | 11 | 12)
                        && w.0 >= 32
                        && GetKeyState(VK_CONTROL.0 as i32) >= 0 =>
                {
                    s.begin_cell_edit(char::from_u32(w.0 as u32))
                }
                WM_KEYDOWN => {
                    if let Some(result) = s.grid_key(w.0 as u16) {
                        result
                    } else {
                        match w.0 as u16 {
                            key if key == VK_TAB.0 => {
                                let _ = SetFocus(Some(s.selector));
                                Ok(())
                            }
                            key if key == VK_F6.0 => {
                                let id = if s.sidebar_width() == 0 {
                                    11
                                } else {
                                    200 + s.view
                                };
                                if let Some((control, _)) =
                                    s.shell_controls.iter().find(|(_, value)| *value == id)
                                {
                                    let _ = SetFocus(Some(*control));
                                }
                                Ok(())
                            }
                            key if key == VK_ESCAPE.0 => {
                                s.cancel_drag();
                                Ok(())
                            }
                            key if key == VK_APPS.0
                                || (key == VK_F10.0 && GetKeyState(VK_SHIFT.0 as i32) < 0) =>
                            {
                                s.task_context_menu(POINT { x: -1, y: -1 })
                            }
                            key if key == b'F' as u16 && GetKeyState(VK_CONTROL.0 as i32) < 0 => {
                                s.search_form()
                            }
                            key if key == VK_F1.0 => s.command(140),
                            key if key == VK_RETURN.0 => s.command(112),
                            key if key == VK_DELETE.0 => s.command(113),
                            _ => Ok(()),
                        }
                    }
                }
                WM_DPICHANGED => {
                    s.cancel_drag();
                    let font = window_font(h);
                    let old = s.font;
                    s.font = font;
                    s.dpi = GetDpiForWindow(h).max(96) as i32;
                    shell::publish(s.font, s.dpi, s.view);
                    for c in [s.selector, s.tabs, s.master_tabs]
                        .into_iter()
                        .chain(s.shell_controls.iter().map(|(h, _)| *h))
                    {
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
                s.focus_cell_edit();
                send(s.tabs, TCM_SETCURSEL, s.view, 0);
                send(
                    s.master_tabs,
                    TCM_SETCURSEL,
                    match s.master_kind {
                        MasterKind::Category => 0,
                        MasterKind::Assignee => 1,
                        MasterKind::Holiday => 2,
                    },
                    0,
                );
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
        let smoke_dir = if args
            .first()
            .is_some_and(|a| a == "--ui-smoke-test" || a == "--ui-smoke-postgres")
        {
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
        if args.first().is_none_or(|a| a != "--postgres")
            && let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let store = if args
            .first()
            .is_some_and(|a| a == "--postgres" || a == "--ui-smoke-postgres")
        {
            Store::connect_postgres(
                &std::env::var("YYPROJECT_DATABASE_URL")
                    .map_err(|_| "Set YYPROJECT_DATABASE_URL")?,
                false,
            )?
        } else {
            Store::open(&path, false)?
        };
        if args.first().is_some_and(|a| a == "--ui-smoke-postgres") && !store.list()?.is_empty() {
            return Err("PostgreSQL smoke test requires an empty dedicated database".into());
        }
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
            w!("yyproject — タスク・バグ・課題・進捗管理"),
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
        let master_tabs = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            WC_TABCONTROLW,
            w!(""),
            WS_CHILD | WS_TABSTOP,
            0,
            0,
            1,
            1,
            Some(hwnd),
            None,
            Some(instance.into()),
            None,
        )?;
        for (i, title) in [
            "カテゴリ（名前・親カテゴリ）",
            "担当者（名前）",
            "休日（名前・日付）",
        ]
        .iter()
        .enumerate()
        {
            let mut value: Vec<_> = title.encode_utf16().chain(Some(0)).collect();
            let item = TCITEMW {
                mask: TCIF_TEXT,
                pszText: windows::core::PWSTR(value.as_mut_ptr()),
                ..Default::default()
            };
            send(master_tabs, TCM_INSERTITEMW, i, &item as *const _ as isize);
        }
        for c in [selector, tabs, master_tabs] {
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
            master_tabs,
            shell_controls: shell::create(hwnd, font)?,
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
            case_range: None,
            selected: None,
            x: 0,
            y: 0,
            content: (0, 0),
            hits: vec![],
            drag: None,
            grid: GridState::default(),
            timeline: Timeline::default(),
            resize: None,
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
            let control_focus = APP.with(|a| {
                a.borrow().as_ref().is_some_and(|s| {
                    let focus = GetFocus();
                    focus == s.selector
                        || focus == s.master_tabs
                        || s.shell_controls.iter().any(|(h, _)| *h == focus)
                })
            });
            if control_focus && IsDialogMessageW(hwnd, &msg).as_bool() {
                continue;
            }
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

#[cfg(test)]
mod interaction_tests {
    use super::*;
    #[test]
    fn network_focus_follows_ancestors_and_descendants_without_sibling_branches() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("graph.db"), false).unwrap();
        let day = "2026-10-27".parse().unwrap();
        let mut p = store.create("graph".into(), day, day, "test").unwrap();
        for i in 1..=5 {
            p = store
                .apply(
                    p.id,
                    p.revision,
                    &Change::SaveTask {
                        task: Task {
                            title: i.to_string(),
                            ..Default::default()
                        },
                    },
                    "test",
                )
                .unwrap();
        }
        for (from, to) in [(1, 2), (2, 3), (1, 4)] {
            p = store
                .apply(
                    p.id,
                    p.revision,
                    &Change::AddDependency {
                        dependency: Dependency {
                            predecessor: from,
                            successor: to,
                            lag_days: 0,
                        },
                    },
                    "test",
                )
                .unwrap();
        }
        assert_eq!(
            network_family(&p, Some(2)),
            std::collections::BTreeSet::from([1, 2, 3])
        );
        assert!(network_family(&p, None).is_empty());
        assert_eq!(
            network_family(&p, Some(5)),
            std::collections::BTreeSet::from([5])
        );
    }
}
