//! Four-level calendar header, milestones and progress (lightning) line.
use super::*;
use chrono::{Datelike, NaiveDate};
pub(super) struct Timeline {
    pub origin: Option<NaiveDate>,
    pub name_width: i32,
    pub day_width: i32,
    pub lightning: bool,
    pub as_of: NaiveDate,
    pub resizing: bool,
}
impl Default for Timeline {
    fn default() -> Self {
        Self {
            origin: None,
            name_width: 330,
            day_width: 26,
            lightning: true,
            as_of: chrono::Local::now().date_naive(),
            resizing: false,
        }
    }
}
#[derive(Clone, Copy)]
pub(super) struct Viewport {
    pub dpi: i32,
    pub body: RECT,
    pub x: i32,
    pub y: i32,
}
impl Viewport {
    fn px(self, v: i32) -> i32 {
        v * self.dpi / 96
    }
}
impl Timeline {
    pub fn width(&self, dpi: i32, body: RECT) -> i32 {
        (self.name_width * dpi / 96)
            .min((body.right - body.left - 90 * dpi / 96).max(120 * dpi / 96))
    }
}
impl State {
    pub(super) fn timeline_first(&self) -> Option<NaiveDate> {
        self.timeline
            .origin
            .or_else(|| self.gantt_range().map(|r| r.0))
    }
    pub(super) fn timeline_form(&mut self) -> Result<()> {
        let start = self
            .timeline_first()
            .unwrap_or_else(|| chrono::Local::now().date_naive());
        if let Some(v) = form(
            self.hwnd,
            "ガントの表示設定",
            vec![
                field("表示起点（YYYY-MM-DD）", start, 0),
                field(
                    "項目・予定日程の列幅（96dpi基準）",
                    self.timeline.name_width,
                    0,
                ),
                field("1日の幅（96dpi基準）", self.timeline.day_width, 0),
                choice(
                    "稲妻線",
                    if self.timeline.lightning { "yes" } else { "no" },
                    vec![
                        ("yes".into(), "表示".into()),
                        ("no".into(), "非表示".into()),
                    ],
                    0,
                ),
                field("稲妻線の基準日", self.timeline.as_of, 0),
            ],
            |v| {
                date(&v[0])?;
                date(&v[4])?;
                if !v[1].parse::<i32>().is_ok_and(|n| (160..=1600).contains(&n))
                    || !v[2].parse::<i32>().is_ok_and(|n| (22..=120).contains(&n))
                {
                    return Err("列幅は160～1600、1日の幅は22～120で指定してください".into());
                }
                Ok(())
            },
        ) {
            self.timeline.origin = Some(date(&v[0])?);
            self.timeline.name_width = v[1].parse()?;
            self.timeline.day_width = v[2].parse()?;
            self.timeline.lightning = v[3] == "yes";
            self.timeline.as_of = date(&v[4])?;
            self.x = 0;
            self.y = 0;
            self.layout();
            invalidate(self.hwnd);
        }
        Ok(())
    }
}
pub(super) fn paint(
    dc: HDC,
    p: &Project,
    options: &Timeline,
    v: Viewport,
    filter: &TaskFilter,
    hits: &mut Vec<(RECT, i64)>,
) {
    let a = v.body;
    let black = COLORREF(0x282828);
    let grey = COLORREF(0xf0f0f0);
    let red = COLORREF(0x4040d0);
    let first = options.origin.unwrap_or_else(|| {
        p.tasks
            .iter()
            .filter(|t| !t.archived)
            .filter_map(|t| t.start)
            .fold(p.start, std::cmp::min)
    });
    let name_w = options.width(v.dpi, a);
    let day = v.px(options.day_width);
    let row = v.px(30);
    let head = v.px(24);
    let header = head * 4;
    let base = a.top + header;
    let x = a.left + name_w - v.x;
    let timeline = unsafe { SaveDC(dc) };
    unsafe {
        IntersectClipRect(dc, a.left + name_w, a.top, a.right, a.bottom);
    }
    let begin = (v.x / day).max(0);
    let end = begin + (a.right - a.left - name_w) / day + 3;
    for n in begin..end {
        let Some(d) = first.checked_add_days(chrono::Days::new(n as u64)) else {
            continue;
        };
        let cx = x + n * day;
        if !p.is_workday(d) {
            fill(
                dc,
                rect(cx, a.top + head * 2, day, a.bottom - a.top - head * 2),
                COLORREF(0xf1f1f1),
            );
        }
        line(
            dc,
            (cx, a.top + head * 2),
            (cx, a.bottom),
            COLORREF(0xe0e0e0),
        );
        let year_end = NaiveDate::from_ymd_opt(d.year() + 1, 1, 1)
            .map_or(end, |next| (next - first).num_days() as i32);
        let next_month = if d.month() == 12 {
            NaiveDate::from_ymd_opt(d.year() + 1, 1, 1)
        } else {
            NaiveDate::from_ymd_opt(d.year(), d.month() + 1, 1)
        };
        let month_end = next_month.map_or(end, |next| (next - first).num_days() as i32);
        if n == begin || d.ordinal() == 1 {
            label(
                dc,
                rect(
                    cx.max(a.left + name_w),
                    a.top,
                    ((year_end.min(end) - n) * day).max(day),
                    head,
                ),
                &format!("{}年", d.year()),
                black,
            );
            line(dc, (cx, a.top), (cx, a.top + head), COLORREF(0xc0c0c0));
        }
        if n == begin || d.day() == 1 {
            label(
                dc,
                rect(
                    cx.max(a.left + name_w),
                    a.top + head,
                    ((month_end.min(end) - n) * day).max(day),
                    head,
                ),
                &format!("{}月", d.month()),
                black,
            );
            line(
                dc,
                (cx, a.top + head),
                (cx, a.top + head * 2),
                COLORREF(0xc0c0c0),
            );
        }
        label(
            dc,
            rect(cx + v.px(3), a.top + head * 2, day - v.px(4), head),
            &d.day().to_string(),
            black,
        );
        label(
            dc,
            rect(cx + v.px(3), a.top + head * 3, day - v.px(4), head),
            ["月", "火", "水", "木", "金", "土", "日"][d.weekday().num_days_from_monday() as usize],
            if p.is_workday(d) { black } else { red },
        );
    }
    let bars = unsafe { SaveDC(dc) };
    unsafe {
        IntersectClipRect(dc, a.left + name_w, base, a.right, a.bottom);
    }
    let rows = p
        .ordered_tasks()
        .into_iter()
        .filter(|(t, _)| p.matches_filter(filter, t))
        .collect::<Vec<_>>();
    let baseline = x + (options.as_of - first).num_days() as i32 * day + day / 2;
    let mut previous = (baseline, base);
    for (i, (t, _)) in rows.iter().enumerate() {
        let y = base - v.y + i as i32 * row;
        if y + row < base || y > a.bottom {
            continue;
        }
        if let Some((start, due)) = t.start.zip(t.due) {
            let bx = x + (start - first).num_days() as i32 * day;
            let width = ((due - start).num_days() + 1).max(1) as i32 * day;
            if t.milestone {
                let center = (bx + day / 2, y + row / 2);
                let radius = v.px(8);
                let points = [
                    POINT {
                        x: center.0,
                        y: center.1 - radius,
                    },
                    POINT {
                        x: center.0 + radius,
                        y: center.1,
                    },
                    POINT {
                        x: center.0,
                        y: center.1 + radius,
                    },
                    POINT {
                        x: center.0 - radius,
                        y: center.1,
                    },
                ];
                unsafe {
                    SelectObject(dc, GetStockObject(DC_BRUSH));
                    SelectObject(dc, GetStockObject(DC_PEN));
                    SetDCBrushColor(dc, color(t.status));
                    SetDCPenColor(dc, black);
                    let _ = Polygon(dc, &points);
                }
            } else {
                fill(
                    dc,
                    rect(bx, y + v.px(6), width, row - v.px(12)),
                    color(t.status),
                );
                fill(
                    dc,
                    rect(
                        bx,
                        y + row - v.px(9),
                        (i64::from(width) * i64::from(t.progress) / 100) as i32,
                        v.px(3),
                    ),
                    COLORREF(0xc06c28),
                );
            }
            if options.lightning {
                let point = if start <= options.as_of
                    && !matches!(t.status, Status::Done | Status::Cancelled)
                {
                    (
                        bx + (i64::from(width) * i64::from(t.progress) / 100) as i32,
                        y + row / 2,
                    )
                } else {
                    (baseline, y + row / 2)
                };
                line(dc, previous, point, red);
                previous = point;
            }
        } else if options.lightning {
            let point = (baseline, y + row / 2);
            line(dc, previous, point, red);
            previous = point;
        }
        line(
            dc,
            (a.left + name_w, y + row - 1),
            (a.right, y + row - 1),
            COLORREF(0xe0e0e0),
        );
    }
    if options.lightning {
        line(dc, previous, (baseline, a.bottom), red);
        line(
            dc,
            (baseline, base),
            (baseline, a.bottom),
            COLORREF(0xb0b0f0),
        );
    }
    unsafe {
        let _ = RestoreDC(dc, bars);
        let _ = RestoreDC(dc, timeline);
    }
    let names = unsafe { SaveDC(dc) };
    unsafe {
        IntersectClipRect(dc, a.left, base, a.left + name_w, a.bottom);
    }
    for (i, (t, depth)) in rows.iter().enumerate() {
        let y = base - v.y + i as i32 * row;
        if y + row <= base || y >= a.bottom {
            continue;
        }
        let rr = rect(a.left, y, name_w, row);
        fill(
            dc,
            rr,
            if i % 2 == 0 {
                COLORREF(0xfafafa)
            } else {
                COLORREF(0xffffff)
            },
        );
        label(
            dc,
            rect(a.left + v.px(4), y, name_w - v.px(8), row),
            &format!(
                "{}{}#{} {}  {}～{}",
                "  ".repeat((*depth).min(20)),
                if t.milestone { "◆ " } else { "" },
                t.id,
                t.title,
                t.start.map(|d| d.to_string()).unwrap_or_default(),
                t.due.map(|d| d.to_string()).unwrap_or_default()
            ),
            black,
        );
        hits.push((rect(a.left, y, a.right - a.left, row), t.id));
    }
    unsafe {
        let _ = RestoreDC(dc, names);
    }
    fill(dc, rect(a.left, a.top, name_w, header), grey);
    label(
        dc,
        rect(a.left + v.px(6), a.top, name_w - v.px(12), head * 2),
        "項目 / 予定日程",
        black,
    );
    label(
        dc,
        rect(
            a.left + v.px(6),
            a.top + head * 2,
            name_w - v.px(12),
            head * 2,
        ),
        "右端をドラッグして幅を変更",
        black,
    );
    for i in 1..=4 {
        line(
            dc,
            (a.left + name_w, a.top + i * head),
            (a.right, a.top + i * head),
            COLORREF(0xc0c0c0),
        );
    }
    line(
        dc,
        (a.left + name_w, a.top),
        (a.left + name_w, a.bottom),
        COLORREF(0xa0a0a0),
    );
}
