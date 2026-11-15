//! A simple text popup menu. Classic skins have no graphics for arbitrary menus, so it uses
//! the playlist font and colours.

use crate::font;
use crate::gfx::{Framebuffer, Rect, rect};
use crate::skin::PlaylistColors;

const ROW_H: i32 = 12;
const PAD: i32 = 2;

pub struct Popup {
    /// Position within the whole window, in skin pixels.
    pub x: i32,
    pub y: i32,
    items: Vec<String>,
    hover: Option<usize>,
}

impl Popup {
    /// Opens below (x, y), moved up or left as needed to fit a `bounds_w` x `bounds_h` window.
    pub fn new(items: Vec<String>, x: i32, y: i32, bounds_w: i32, bounds_h: i32) -> Popup {
        let mut p = Popup {
            x,
            y,
            items,
            hover: None,
        };
        let (w, h) = p.size();
        p.x = p.x.min(bounds_w - w).max(0);
        p.y = p.y.min(bounds_h - h).max(0);
        p
    }

    pub fn size(&self) -> (i32, i32) {
        let widest = self.items.iter().map(|s| font::width(s)).max().unwrap_or(0);
        (
            widest + 2 * PAD + 2,
            self.items.len() as i32 * ROW_H + 2 * PAD,
        )
    }

    fn bounds(&self) -> Rect {
        let (w, h) = self.size();
        rect(self.x, self.y, w, h)
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        self.bounds().contains(x, y)
    }

    fn item_at(&self, x: i32, y: i32) -> Option<usize> {
        if !self.contains(x, y) {
            return None;
        }
        let i = (y - self.y - PAD).max(0) / ROW_H;
        ((i as usize) < self.items.len()).then_some(i as usize)
    }

    pub fn mouse_move(&mut self, x: i32, y: i32) {
        self.hover = self.item_at(x, y);
    }

    /// The chosen item index, if the pointer was released on one.
    pub fn mouse_up(&self, x: i32, y: i32) -> Option<usize> {
        self.item_at(x, y)
    }

    /// Draws into `fb`, which is sized to the popup and placed at (x, y) by the caller.
    pub fn draw(&self, fb: &mut Framebuffer, colors: PlaylistColors) {
        let (w, h) = self.size();
        fb.resize(w, h);
        let all = rect(0, 0, w, h);
        fb.fill(all, colors.normal);
        fb.fill(rect(1, 1, w - 2, h - 2), colors.normal_bg);
        for (i, item) in self.items.iter().enumerate() {
            let row = rect(1, PAD + i as i32 * ROW_H, w - 2, ROW_H);
            let color = if self.hover == Some(i) {
                fb.fill(row, colors.selected_bg);
                colors.current
            } else {
                colors.normal
            };
            font::draw(fb, item, PAD + 1, row.y, color, row);
        }
    }
}
