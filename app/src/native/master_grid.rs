//! Master cells share selection, clipboard and native EDIT with task cells.
use super::*;
impl State {
    pub(super) fn select_master_kind(&mut self, kind: MasterKind) {
        self.cancel_drag();
        self.master_kind = kind;
        self.status = format!("{}マスタ", self.master_label());
        self.selected = None;
        self.grid.selection = false;
        self.grid.selecting = false;
        self.grid.row_selection = false;
        self.grid.selected_rows.clear();
        self.grid.anchor = (0, 0);
        self.grid.current = (0, 0);
        self.x = 0;
        self.y = 0;
        send(
            self.master_tabs,
            TCM_SETCURSEL,
            match kind {
                MasterKind::Category => 0,
                MasterKind::Assignee => 1,
                MasterKind::Holiday => 2,
            },
            0,
        );
        self.layout();
        invalidate(self.hwnd);
    }
    pub(super) fn master_paste(&mut self, payload: &str) -> Result<()> {
        let mut data = table::decode(payload, '\t')?;
        if data.is_empty() {
            return Ok(());
        }
        let p = self
            .project
            .as_ref()
            .ok_or("プロジェクトを開いてください")?;
        let row = self.grid.anchor.0.min(self.grid.current.0);
        let col = self.grid.anchor.1.min(self.grid.current.1);
        if data.len() == 1 && data[0].len() == 1 && self.grid.anchor.1 == self.grid.current.1 {
            data = vec![data[0].clone(); self.grid.anchor.0.abs_diff(self.grid.current.0) + 1];
        }
        let rows = self.grid_rows();
        if row + data.len() > rows.len()
            || data
                .iter()
                .any(|r| col + r.len() > self.grid_columns().len())
        {
            return Err("貼り付け範囲がマスタ一覧を超えています".into());
        }
        let mut masters = Vec::new();
        for (offset, cells) in data.iter().enumerate() {
            let mut m = p
                .masters
                .iter()
                .find(|m| m.id == rows[row + offset].0)
                .unwrap()
                .clone();
            for (i, value) in cells.iter().enumerate() {
                let c = &self.grid_columns()[col + i];
                if c.editable {
                    table::set_master_value(&mut m, c.key, value)?;
                } else if table::master_value(&m, c.key) != *value {
                    return Err("IDは読み取り専用です".into());
                }
            }
            masters.push(m);
        }
        self.apply(Change::SaveMasters { masters })
    }
    pub(super) fn master_clear(&mut self) -> Result<()> {
        let rows = self.grid.current.0.abs_diff(self.grid.anchor.0) + 1;
        let cols = self.grid.current.1.abs_diff(self.grid.anchor.1) + 1;
        self.master_paste(&table::encode(&vec![vec![String::new(); cols]; rows], '\t'))
    }
    pub(super) fn paint_masters(&self, dc: HDC, hits: &mut Vec<(RECT, i64)>) {
        let Some(p) = &self.project else {
            return;
        };
        let a = self.body;
        let x = a.left - self.x;
        let header = self.px(30);
        let mut y = a.top + header - self.y;
        for (i, m) in p
            .masters
            .iter()
            .filter(|m| m.kind == self.master_kind)
            .enumerate()
        {
            let row = self.grid_height(m.id);
            if y + row > a.top + header && y < a.bottom {
                let rr = rect(x, y, self.content.0, row);
                fill(
                    dc,
                    rr,
                    if i % 2 == 0 {
                        COLORREF(0xfafafa)
                    } else {
                        COLORREF(0xffffff)
                    },
                );
                let mut cx = x;
                for (col, c) in self.grid_columns().iter().enumerate() {
                    let w = self.px(self.grid_width(col));
                    if self.grid.contains(i, col) {
                        fill(dc, rect(cx, y, w, row), COLORREF(0xffe5c7));
                    }
                    label(
                        dc,
                        rect(cx + self.px(4), y, w - self.px(8), row),
                        &table::master_value(m, c.key),
                        COLORREF(0x282828),
                    );
                    line(dc, (cx + w, y), (cx + w, y + row), COLORREF(0xe0e0e0));
                    cx += w;
                }
                line(
                    dc,
                    (x, y + row - 1),
                    (x + self.content.0, y + row - 1),
                    COLORREF(0xd0d0d0),
                );
                hits.push((rr, m.id));
            }
            y += row;
        }
        fill(
            dc,
            rect(a.left, a.top, a.right - a.left, header),
            COLORREF(0xf0f0f0),
        );
        let mut cx = x;
        for (col, c) in self.grid_columns().iter().enumerate() {
            let w = self.px(self.grid_width(col));
            label(
                dc,
                rect(cx + self.px(4), a.top, w - self.px(8), header),
                c.label,
                COLORREF(0x282828),
            );
            line(
                dc,
                (cx + w, a.top),
                (cx + w, a.top + header),
                COLORREF(0xc0c0c0),
            );
            cx += w;
        }
    }
}
