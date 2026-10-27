//! Web-inspired native shell. Controls remain real, keyboard-accessible Win32 buttons.
use super::*;
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
#[derive(Clone, Copy)]
struct ButtonStyle {
    font: HFONT,
    dpi: i32,
    view: usize,
}
impl ButtonStyle {
    fn px(self, n: i32) -> i32 {
        n * self.dpi / 96
    }
}
thread_local! { static BUTTON_STYLE: std::cell::Cell<Option<ButtonStyle>> = const { std::cell::Cell::new(None) }; }
pub(super) fn publish(font: HFONT, dpi: i32, view: usize) {
    BUTTON_STYLE.with(|s| s.set(Some(ButtonStyle { font, dpi, view })));
}
unsafe extern "system" fn button_proc(
    h: HWND,
    m: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    _: usize,
) -> LRESULT {
    unsafe {
        if m == WM_ERASEBKGND {
            return LRESULT(1);
        }
        if m == WM_NCDESTROY {
            let _ = RemoveWindowSubclass(h, Some(button_proc), id);
        }
        DefSubclassProc(h, m, w, l)
    }
}
/// Independent of APP's RefCell: Win32 sends this synchronously during layout/focus changes.
pub(super) fn draw_button(item: &DRAWITEMSTRUCT) {
    unsafe {
        let style = BUTTON_STYLE.with(|s| s.get()).unwrap_or(ButtonStyle {
            font: HFONT(GetStockObject(DEFAULT_GUI_FONT).0),
            dpi: GetDpiForWindow(item.hwndItem).max(96) as i32,
            view: usize::MAX,
        });
        let width = (item.rcItem.right - item.rcItem.left).max(1);
        let height = (item.rcItem.bottom - item.rcItem.top).max(1);
        let dc = CreateCompatibleDC(Some(item.hDC));
        let bitmap = CreateCompatibleBitmap(item.hDC, width, height);
        if dc.is_invalid() || bitmap.is_invalid() {
            paint_button(style, item);
        } else {
            let old = SelectObject(dc, bitmap.into());
            paint_button(
                style,
                &DRAWITEMSTRUCT {
                    hDC: dc,
                    rcItem: rect(0, 0, width, height),
                    ..*item
                },
            );
            let _ = BitBlt(
                item.hDC,
                item.rcItem.left,
                item.rcItem.top,
                width,
                height,
                Some(dc),
                0,
                0,
                SRCCOPY,
            );
            SelectObject(dc, old);
        }
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(dc);
    }
}
fn paint_button(style: ButtonStyle, item: &DRAWITEMSTRUCT) {
    let nav = (200..215).contains(&(item.CtlID as usize));
    let active = nav && item.CtlID as usize - 200 == style.view;
    let primary = item.CtlID == 110;
    let pressed = item.itemState.0 & ODS_SELECTED.0 != 0;
    let bg = if active {
        COLORREF(0x8e532e)
    } else if nav {
        NAV
    } else if primary {
        BLUE
    } else if pressed {
        SELECTED
    } else {
        COLORREF(0xffffff)
    };
    fill(item.hDC, item.rcItem, bg);
    if !nav {
        outline(item.hDC, item.rcItem, BORDER);
    }
    let mut area = item.rcItem;
    area.left += style.px(10);
    unsafe {
        SelectObject(item.hDC, style.font.into());
    }
    label(
        item.hDC,
        area,
        &text(item.hwndItem),
        if item.itemState.0 & ODS_DISABLED.0 != 0 {
            MUTED
        } else if nav || primary {
            COLORREF(0xffffff)
        } else {
            INK
        },
    );
    if item.itemState.0 & ODS_FOCUS.0 != 0 {
        let mut focus = item.rcItem;
        unsafe {
            let _ = InflateRect(&mut focus, -3, -3);
            let _ = DrawFocusRect(item.hDC, &focus);
        }
    }
}

pub(super) const BACKGROUND: COLORREF = COLORREF(0xfaf6f4);
pub(super) const INK: COLORREF = COLORREF(0x473025);
pub(super) const MUTED: COLORREF = COLORREF(0x8c7566);
pub(super) const BORDER: COLORREF = COLORREF(0xece2db);
pub(super) const BLUE: COLORREF = COLORREF(0xc75b25);
pub(super) const SELECTED: COLORREF = COLORREF(0xfff0e6);
pub(super) const NAV: COLORREF = COLORREF(0x2f2119);
const ACTIONS: &[(usize, &str, i32)] = &[
    (102, "新規案件", 88),
    (103, "案件設定", 88),
    (110, "追加", 62),
    (112, "編集", 62),
    (122, "一括編集", 88),
    (410, "絞り込み", 88),
    (411, "解除", 62),
    (118, "再読込", 76),
    (104, "CSV出力", 88),
    (140, "使い方", 76),
];
pub(super) fn create(h: HWND, font: HFONT) -> Result<Vec<(HWND, usize)>> {
    publish(font, unsafe { GetDpiForWindow(h).max(96) } as i32, 0);
    let mut controls = Vec::new();
    for (id, title) in VIEWS
        .iter()
        .enumerate()
        .map(|(i, t)| (200 + i, *t))
        .chain(ACTIONS.iter().map(|(id, title, _)| (*id, *title)))
    {
        let button = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("BUTTON"),
                &HSTRING::from(title),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32),
                0,
                0,
                1,
                1,
                Some(h),
                Some(HMENU(std::ptr::without_provenance_mut(id))),
                Some(GetModuleHandleW(None)?.into()),
                None,
            )?
        };
        unsafe {
            SetWindowSubclass(button, Some(button_proc), 1, 0).ok()?;
        }
        send(button, WM_SETFONT, font.0 as usize, 0);
        controls.push((button, id));
    }
    let chooser = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("COMBOBOX"),
            w!(""),
            WS_CHILD | WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(CBS_DROPDOWNLIST as u32),
            0,
            0,
            1,
            1,
            Some(h),
            Some(HMENU(std::ptr::without_provenance_mut(11))),
            Some(GetModuleHandleW(None)?.into()),
            None,
        )?
    };
    for title in VIEWS {
        let title = HSTRING::from(title);
        send(chooser, CB_ADDSTRING, 0, title.as_ptr() as isize);
    }
    send(chooser, WM_SETFONT, font.0 as usize, 0);
    controls.push((chooser, 11));
    Ok(controls)
}
impl State {
    pub(super) fn sidebar_width(&self) -> i32 {
        let mut r = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut r);
        }
        if r.right < self.px(700) || r.bottom < self.px(620) {
            0
        } else {
            self.px(if r.right < self.px(900) { 160 } else { 198 })
        }
    }
    pub(super) fn shell_layout(&self, r: RECT) -> i32 {
        publish(self.font, self.dpi, self.view);
        let left = self.sidebar_width();
        let nav_top = self.px(80);
        let nav_height = ((r.bottom - nav_top - self.px(36)) / VIEWS.len() as i32)
            .clamp(self.px(18), self.px(40));
        let mut x = left + self.px(22);
        let mut y = self.px(if left == 0 { 114 } else { 126 });
        for &(h, id) in &self.shell_controls {
            let compact = left == 0;
            let hidden = if id == 11 {
                !compact
            } else if (200..215).contains(&id) {
                compact
            } else if id == 102 || id == 103 {
                r.right < self.px(700)
            } else {
                false
            };
            unsafe {
                let visible = GetWindowLongW(h, GWL_STYLE) as u32 & WS_VISIBLE.0 != 0;
                if visible == hidden {
                    let _ = ShowWindow(h, if hidden { SW_HIDE } else { SW_SHOW });
                }
            }
            if hidden {
                continue;
            }
            let (bx, by, width, height) = if id == 11 {
                send(h, CB_SETCURSEL, self.view, 0);
                (
                    self.px(22),
                    self.px(78),
                    (r.right - self.px(44)).max(1),
                    self.px(400),
                )
            } else if (200..215).contains(&id) {
                (
                    self.px(10),
                    nav_top + (id - 200) as i32 * nav_height,
                    left - self.px(20),
                    nav_height - self.px(2),
                )
            } else if id == 102 || id == 103 {
                (
                    r.right - self.px(if id == 102 { 204 } else { 108 }),
                    self.px(17),
                    self.px(88),
                    self.px(32),
                )
            } else {
                let width = self.px(ACTIONS.iter().find(|a| a.0 == id).unwrap().2);
                if x + width > r.right - self.px(20) {
                    if compact {
                        unsafe {
                            let _ = ShowWindow(h, SW_HIDE);
                        }
                        continue;
                    }
                    x = left + self.px(22);
                    y += self.px(38);
                }
                let pos = (x, y, width, self.px(32));
                x += width + self.px(6);
                pos
            };
            unsafe {
                let mut old = RECT::default();
                let mut origin = POINT::default();
                let _ = GetWindowRect(h, &mut old);
                let _ = ClientToScreen(self.hwnd, &mut origin);
                let moved = old.left - origin.x != bx
                    || old.top - origin.y != by
                    || old.right - old.left != width.max(1)
                    || old.bottom - old.top != height.max(1);
                if id == 11 || moved {
                    let _ = MoveWindow(h, bx, by, width.max(1), height.max(1), false);
                }
                if (100..200).contains(&id) {
                    let enabled = match id {
                        102 | 140 => true,
                        112 => self.project.is_some(),
                        122 => matches!(self.view, 2 | 3 | 8 | 11 | 12) && self.project.is_some(),
                        _ => self.project.is_some(),
                    };
                    let _ = EnableWindow(h, enabled);
                }
            }
            // Repaint active navigation and keyboard focus after switching views.
            invalidate(h);
        }
        y + self.px(46)
    }
    pub(super) fn capture_shell_controls(&self, dc: HDC) {
        // Use each live child's actual client bounds, font and production renderer.
        // This also works while the dedicated smoke window is hidden.
        unsafe {
            let mut origin = POINT::default();
            let _ = ClientToScreen(self.hwnd, &mut origin);
            for (h, id) in self
                .shell_controls
                .iter()
                .copied()
                .chain([(self.selector, 10), (self.master_tabs, 12)])
            {
                if GetWindowLongW(h, GWL_STYLE) as u32 & WS_VISIBLE.0 == 0 {
                    continue;
                }
                let mut bounds = RECT::default();
                let mut client = RECT::default();
                let _ = GetWindowRect(h, &mut bounds);
                let _ = GetClientRect(h, &mut client);
                let saved = SaveDC(dc);
                let _ = SetViewportOrgEx(dc, bounds.left - origin.x, bounds.top - origin.y, None);
                IntersectClipRect(dc, 0, 0, client.right, client.bottom);
                if id >= 100 {
                    let item = DRAWITEMSTRUCT {
                        CtlID: id as u32,
                        hwndItem: h,
                        hDC: dc,
                        rcItem: client,
                        ..Default::default()
                    };
                    send(self.hwnd, WM_DRAWITEM, id, &item as *const _ as isize);
                } else {
                    send(
                        h,
                        WM_PRINTCLIENT,
                        dc.0 as usize,
                        (PRF_CLIENT | PRF_ERASEBKGND) as isize,
                    );
                    if id == 10 || id == 11 {
                        // The themed closed combo does not print its text child on
                        // a hidden window; use the live control's selected label.
                        label(
                            dc,
                            rect(
                                self.px(8),
                                0,
                                (client.right - self.px(32)).max(1),
                                client.bottom,
                            ),
                            &text(h),
                            INK,
                        );
                    }
                }
                let _ = RestoreDC(dc, saved);
            }
        }
    }
    pub(super) fn paint_shell(&self, dc: HDC, r: RECT) {
        let left = self.sidebar_width();
        fill(dc, rect(0, 0, r.right, self.px(66)), COLORREF(0xffffff));
        fill(dc, rect(0, self.px(66), left, r.bottom - self.px(66)), NAV);
        label(
            dc,
            rect(self.px(22), self.px(12), self.px(140), self.px(42)),
            "YYproject",
            BLUE,
        );
        line(dc, (0, self.px(65)), (r.right, self.px(65)), BORDER);
        if left > 0 {
            label(
                dc,
                rect(
                    left + self.px(22),
                    self.px(74),
                    r.right - left - self.px(44),
                    self.px(20),
                ),
                "PROJECT WORKSPACE",
                MUTED,
            );
            label(
                dc,
                rect(
                    left + self.px(22),
                    self.px(94),
                    r.right - left - self.px(44),
                    self.px(30),
                ),
                VIEWS[self.view],
                INK,
            );
        }
        fill(dc, self.body, COLORREF(0xffffff));
        outline(dc, self.body, BORDER);
        line(
            dc,
            (left, r.bottom - self.px(28)),
            (r.right, r.bottom - self.px(28)),
            BORDER,
        );
    }
}

pub(super) fn smoke_test(
    h: HWND,
    directory: &std::path::Path,
    report: &mut Vec<String>,
) -> Result<()> {
    for (width, height, compact) in [(1100, 760, false), (640, 480, true)] {
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
        for view in 0..VIEWS.len() {
            let control = APP.with(|a| {
                let a = a.borrow();
                let s = a.as_ref().unwrap();
                s.shell_controls
                    .iter()
                    .find(|(_, id)| *id == if compact { 11 } else { 200 + view })
                    .unwrap()
                    .0
            });
            if compact {
                send(control, CB_SETCURSEL, view, 0);
                send(
                    h,
                    WM_COMMAND,
                    11 | ((CBN_SELCHANGE as usize) << 16),
                    control.0 as isize,
                );
            } else {
                send(control, BM_CLICK, 0, 0);
            }
            smoke::pump_until(|| {
                APP.with(|a| a.borrow().as_ref().is_some_and(|s| s.view == view))
            })?;
            APP.with(|a| -> Result<()> {
                // Exercise synchronous owner draw while layout owns APP.
                let a = a.borrow_mut();
                let s = a.as_ref().unwrap();
                if !compact {
                    verify_nested_paint(s)?;
                }
                if (s.sidebar_width() == 0) != compact || s.body.bottom <= s.body.top + s.px(30) {
                    return Err(
                        "Native responsive navigation did not preserve usable content".into(),
                    );
                }
                let mut client = RECT::default();
                unsafe {
                    GetClientRect(h, &mut client)?;
                }
                for &(child, id) in &s.shell_controls {
                    if unsafe { GetWindowLongW(child, GWL_STYLE) as u32 } & WS_VISIBLE.0 == 0 {
                        continue;
                    }
                    let mut bounds = RECT::default();
                    let mut origin = POINT::default();
                    unsafe {
                        GetWindowRect(child, &mut bounds)?;
                        ClientToScreen(h, &mut origin).ok()?;
                    }
                    if bounds.left < origin.x
                        || bounds.right > origin.x + client.right
                        || bounds.top < origin.y
                        || bounds.bottom > origin.y + client.bottom
                    {
                        return Err(
                            format!("Native shell control outside actual client: {id}").into()
                        );
                    }
                }
                Ok(())
            })?;
        }
        send(h, WM_COMMAND, 202, 0);
        APP.with(|a| -> Result<()> {
            let mut a = a.borrow_mut();
            let s = a.as_mut().unwrap();
            let mut r = RECT::default();
            unsafe {
                GetClientRect(h, &mut r)?;
            }
            smoke::capture(
                s,
                r,
                &directory.join(if compact {
                    "shell-compact.bmp"
                } else {
                    "shell-sidebar.bmp"
                }),
            )
        })?;
    }
    report.push("Web-inspired native shell: actual sidebar buttons and compact chooser navigate all 15 views; visible control bounds and content checked at actual DPI, no data writes: OK".into());
    report.push("Owner-draw reentrancy: all 15 active/inactive sidebar buttons render background and text through real WM_DRAWITEM while APP is mutably borrowed; WM_ERASEBKGND preserves pixels: OK".into());
    Ok(())
}

fn verify_nested_paint(s: &State) -> Result<()> {
    unsafe {
        let screen = GetDC(Some(s.hwnd));
        let dc = CreateCompatibleDC(Some(screen));
        let width = s.px(160);
        let height = s.px(38);
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        let old = SelectObject(dc, bitmap.into());
        let result = (|| -> Result<()> {
            for id in [200 + s.view, 200 + (s.view + 1) % VIEWS.len()] {
                let button = s
                    .shell_controls
                    .iter()
                    .find(|(_, value)| *value == id)
                    .unwrap()
                    .0;
                let area = rect(0, 0, width, height);
                fill(dc, area, COLORREF(0x00ff00));
                if send(button, WM_ERASEBKGND, dc.0 as usize, 0).0 != 1
                    || GetPixel(dc, 2, 2) != COLORREF(0x00ff00)
                {
                    return Err("Owner button erased its background before drawing".into());
                }
                let item = DRAWITEMSTRUCT {
                    CtlID: id as u32,
                    hwndItem: button,
                    hDC: dc,
                    rcItem: area,
                    ..Default::default()
                };
                let drawn = send(s.hwnd, WM_DRAWITEM, id, &item as *const _ as isize);
                let expected = if id == 200 + s.view {
                    COLORREF(0x8e532e)
                } else {
                    NAV
                };
                if drawn.0 != 1 || GetPixel(dc, 2, 2) != expected {
                    return Err(format!(
                        "Reentrant owner draw lost its background: view={} id={id}",
                        s.view
                    )
                    .into());
                }
                let mut ink = 0;
                for y in 0..height {
                    for x in s.px(10)..width - s.px(4) {
                        let pixel = GetPixel(dc, x, y).0;
                        if pixel & 255 > 180
                            && (pixel >> 8) & 255 > 180
                            && (pixel >> 16) & 255 > 180
                        {
                            ink += 1;
                        }
                    }
                }
                if ink < 10 {
                    return Err(format!(
                        "Reentrant owner draw lost its text: id={id} dpi={} ink={ink}",
                        s.dpi
                    )
                    .into());
                }
            }
            Ok(())
        })();
        SelectObject(dc, old);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(dc);
        ReleaseDC(Some(s.hwnd), screen);
        result
    }
}
