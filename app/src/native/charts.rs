//! Daily chart coordinates, work-calendar shading and weekly date labels.
use super::*;
use chrono::Datelike;
fn range(p: &Project) -> (chrono::NaiveDate, chrono::NaiveDate) {
    (
        p.history.first().map_or(p.start, |s| s.day.min(p.start)),
        p.history.last().map_or(p.finish, |s| s.day.max(p.finish)),
    )
}
pub(super) fn width(p: &Project, dpi: i32) -> i32 {
    let (first, last) = range(p);
    (((last - first).num_days().max(1) + 1) * 20 + 120) as i32 * dpi / 96
}
pub(super) fn paint(dc: HDC, p: &Project, v: timeline::Viewport, up: bool) {
    paint_impl(dc, p, v, up, false);
}
pub(super) fn paint_counts(dc: HDC, p: &Project, v: timeline::Viewport) {
    paint_impl(dc, p, v, false, true);
}
fn paint_impl(dc: HDC, p: &Project, v: timeline::Viewport, up: bool, counts: bool) {
    let px = |n: i32| n * v.dpi / 96;
    let a = v.body;
    let plot = rect(
        a.left + px(75) - v.x,
        a.top + px(50),
        (width(p, v.dpi) - px(110)).max(a.right - a.left - px(110)),
        (a.bottom - a.top - px(154)).max(1),
    );
    let (first, last) = range(p);
    let days = (last - first).num_days().max(1);
    let step = (plot.right - plot.left) as f64 / (days + 1) as f64;
    let date_x = |d: chrono::NaiveDate| plot.left + ((d - first).num_days() as f64 * step) as i32;
    let begin = (((a.left - plot.left) as f64 / step).floor() as i64).max(0);
    let end = (((a.right - plot.left) as f64 / step).ceil() as i64 + 1).min(days + 1);
    let black = COLORREF(0x282828);
    for n in begin..end {
        let Some(day) = first.checked_add_days(chrono::Days::new(n as u64)) else {
            continue;
        };
        let x = date_x(day);
        let next = plot.left + ((n + 1) as f64 * step) as i32;
        if !p.is_workday(day) {
            fill(
                dc,
                rect(x, plot.top, next - x, plot.bottom - plot.top + px(48)),
                COLORREF(0xf1f1f1),
            );
        }
        line(dc, (x, plot.top), (x, plot.bottom), COLORREF(0xe0e0e0));
        label(
            dc,
            rect(x, plot.bottom, next - x, px(24)),
            ["月", "火", "水", "木", "金", "土", "日"]
                [day.weekday().num_days_from_monday() as usize],
            if p.is_workday(day) {
                black
            } else {
                COLORREF(0x4040d0)
            },
        );
        if n % 7 == 0 {
            label(
                dc,
                rect(x, plot.bottom + px(24), (next - x).max(px(20)), px(24)),
                &day.day().to_string(),
                black,
            );
        }
        for (offset, next_date, value) in [
            (
                2,
                if day.month() == 12 {
                    chrono::NaiveDate::from_ymd_opt(day.year() + 1, 1, 1)
                } else {
                    chrono::NaiveDate::from_ymd_opt(day.year(), day.month() + 1, 1)
                },
                format!("{}月", day.month()),
            ),
            (
                3,
                chrono::NaiveDate::from_ymd_opt(day.year() + 1, 1, 1),
                format!("{}年", day.year()),
            ),
        ] {
            if n == begin || (offset == 2 && day.day() == 1) || (offset == 3 && day.ordinal() == 1)
            {
                let end_x = next_date.map_or(plot.right, |d| date_x(d).min(plot.right));
                label(
                    dc,
                    rect(
                        x.max(a.left + px(75)),
                        plot.bottom + px(24) * offset,
                        (end_x - x.max(a.left + px(75))).max(1),
                        px(24),
                    ),
                    &value,
                    black,
                );
            }
        }
        line(
            dc,
            (x, plot.bottom),
            (x, plot.bottom + px(48)),
            COLORREF(0xe0e0e0),
        );
    }
    let mut max = p
        .history
        .iter()
        .map(|s| s.scope.max(s.remaining).max(s.completed))
        .fold(1.0, f64::max);
    let ticks = ((plot.bottom - plot.top) / px(28))
        .clamp(1, if counts { (max as i32).clamp(1, 4) } else { 4 });
    if counts {
        max = (max / f64::from(ticks)).ceil() * f64::from(ticks);
    }
    for i in 0..=ticks {
        let y = plot.bottom - (plot.bottom - plot.top) * i / ticks;
        line(dc, (plot.left, y), (plot.right, y), COLORREF(0xe5e5e5));
    }
    let saved = unsafe { SaveDC(dc) };
    unsafe {
        IntersectClipRect(dc, a.left + px(75), plot.top, a.right, plot.bottom + px(1));
    }
    let point = |s: &Snapshot, value: f64| {
        (
            date_x(s.day) + (step / 2.0) as i32,
            plot.bottom - (value / max * (plot.bottom - plot.top) as f64) as i32,
        )
    };
    let series = if up {
        vec![(1, COLORREF(0x398a39)), (2, COLORREF(0x777777))]
    } else {
        vec![(0, COLORREF(0xc06c28))]
    };
    for (which, c) in series {
        let value = |s: &Snapshot| match which {
            0 => s.remaining,
            1 => s.completed,
            _ => s.scope,
        };
        for pair in p.history.windows(2) {
            stroke(
                dc,
                point(&pair[0], value(&pair[0])),
                point(&pair[1], value(&pair[1])),
                c,
                px(3),
            );
        }
        for s in &p.history {
            let (x, y) = point(s, value(s));
            fill(dc, rect(x - px(3), y - px(3), px(6), px(6)), c);
        }
    }
    unsafe {
        let _ = RestoreDC(dc, saved);
    }
    // Keep the value axis readable while the daily calendar scrolls horizontally.
    fill(
        dc,
        rect(a.left, a.top, px(75), a.bottom - a.top),
        COLORREF(0xffffff),
    );
    for i in 0..=ticks {
        let y = plot.bottom - (plot.bottom - plot.top) * i / ticks;
        label(
            dc,
            rect(a.left, y - px(12), px(70), px(24)),
            &if counts {
                format!("{:.0}件", max * i as f64 / f64::from(ticks))
            } else {
                format!("{:.1}h", max * i as f64 / f64::from(ticks))
            },
            black,
        );
    }
    line(
        dc,
        (a.left + px(75), plot.top),
        (a.left + px(75), plot.bottom),
        black,
    );
    line(
        dc,
        (a.left + px(75), plot.bottom),
        (a.right, plot.bottom),
        black,
    );
    label(
        dc,
        rect(a.left + px(75), a.top, a.right - a.left - px(75), px(30)),
        if counts {
            "未完了件数（青）— 完了・中止を除外、再オープンを加算"
        } else if up {
            "完了工数（緑） / 総工数（灰）"
        } else {
            "残工数（青）"
        },
        black,
    );
    if p.history.len() < 2 {
        label(
            dc,
            rect(a.left + px(80), plot.top, a.right - a.left - px(85), px(30)),
            "実績は1日分です。日ごとの記録が増えると線で表示します。",
            black,
        );
    }
}
