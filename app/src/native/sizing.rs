//! Window-local dimensions; pointer coordinates always use current DPI and scroll.
use super::*;
#[derive(Clone, Copy)]
pub(super) enum Target {
    Column(usize),
    Row(i64),
    TimelineName,
    TimelineDay,
}
#[derive(Clone, Copy)]
pub(super) struct Resize {
    target: Target,
    start: POINT,
    original: i32,
}
impl State {
    pub(super) fn resize_target(&self, point: POINT) -> Option<Target> {
        if !inside(self.body, point.x, point.y) {
            return None;
        }
        let tolerance = self.px(4);
        if matches!(self.view, 2 | 3 | 7) {
            if point.y < self.body.top + self.px(30) {
                let mut x = self.body.left - self.x;
                for col in 0..self.grid_columns().len() {
                    x += self.px(self.grid_width(col));
                    if (point.x - x).abs() <= tolerance {
                        return Some(Target::Column(col));
                    }
                }
            } else {
                let mut y = self.body.top + self.px(30) - self.y;
                for (id, _) in self.grid_rows() {
                    y += self.grid_height(id);
                    if y > self.body.top + self.px(30) && (point.y - y).abs() <= tolerance {
                        return Some(Target::Row(id));
                    }
                }
            }
        } else if self.view == 1 {
            let boundary = self.body.left + self.timeline.width(self.dpi, self.body);
            if (point.x - boundary).abs() <= tolerance {
                return Some(Target::TimelineName);
            }
            if point.x > boundary
                && point.y < self.body.top + self.px(96)
                && point.y >= self.body.top + self.px(48)
            {
                let day = self.px(self.timeline.day_width);
                let within = (point.x - boundary + self.x).rem_euclid(day);
                if within <= tolerance || day - within <= tolerance {
                    return Some(Target::TimelineDay);
                }
            }
            if point.x < boundary && point.y >= self.body.top + self.px(96) {
                let mut y = self.body.top + self.px(96) - self.y;
                if let Some(p) = &self.project {
                    for (t, _) in p
                        .ordered_tasks()
                        .into_iter()
                        .filter(|(t, _)| p.matches_filter(&self.filter, t))
                    {
                        y += self.px(self.timeline.height(t.id));
                        if y > self.body.top + self.px(96) && (point.y - y).abs() <= tolerance {
                            return Some(Target::Row(t.id));
                        }
                    }
                }
            }
        }
        None
    }
    fn dimension(&self, target: Target) -> i32 {
        match target {
            Target::Column(c) => self.grid_width(c),
            Target::Row(id) => {
                if self.view == 1 {
                    self.timeline.height(id)
                } else {
                    self.grid_height(id) * 96 / self.dpi
                }
            }
            Target::TimelineName => self.timeline.name_width,
            Target::TimelineDay => self.timeline.day_width,
        }
    }
    fn set_dimension(&mut self, target: Target, value: i32) {
        match target {
            Target::Column(c) => {
                let widths = if self.view == 7 {
                    &mut self.grid.master_widths
                } else {
                    &mut self.grid.widths
                };
                widths.insert(c, value.clamp(40, 2000));
            }
            Target::Row(id) => {
                let heights = if self.view == 1 {
                    &mut self.timeline.heights
                } else if self.view == 7 {
                    &mut self.grid.master_heights
                } else {
                    &mut self.grid.heights
                };
                heights.insert(id, value.clamp(24, 400));
            }
            Target::TimelineName => self.timeline.name_width = value.clamp(160, 1600),
            Target::TimelineDay => self.timeline.day_width = value.clamp(12, 120),
        }
    }
    pub(super) fn begin_resize(&mut self, point: POINT) -> bool {
        let Some(target) = self.resize_target(point) else {
            return false;
        };
        self.resize = Some(Resize {
            target,
            start: point,
            original: self.dimension(target),
        });
        unsafe {
            let _ = SetCapture(self.hwnd);
        }
        true
    }
    pub(super) fn move_resize(&mut self, point: POINT) {
        if let Some(r) = self.resize {
            let delta = match r.target {
                Target::Row(_) => point.y - r.start.y,
                _ => point.x - r.start.x,
            };
            self.set_dimension(r.target, r.original + delta * 96 / self.dpi);
            self.layout();
            invalidate(self.hwnd);
        }
    }
    pub(super) fn finish_resize(&mut self, commit: bool) {
        if let Some(r) = self.resize.take() {
            if !commit {
                self.set_dimension(r.target, r.original);
            }
            unsafe {
                if GetCapture() == self.hwnd {
                    let _ = ReleaseCapture();
                }
            }
            self.layout();
            invalidate(self.hwnd);
        }
    }
}
