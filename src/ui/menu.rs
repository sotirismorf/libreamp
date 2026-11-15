//! Popup menus (the right-click menu, EQ presets), with one level of submenus. Classic skins
//! have no graphics for arbitrary menus, so they use the playlist font and colours.

use sdl3::keyboard::Keycode;

use crate::font;
use crate::gfx::{Framebuffer, Rect, rect};
use crate::skin::PlaylistColors;

const ROW_H: i32 = 12;
const SEPARATOR_H: i32 = 5;
/// Border plus padding on each side.
const EDGE: i32 = 3;
const MARK_W: i32 = font::W + 4;
const SHORTCUT_GAP: i32 = 2 * font::W;
const ARROW: &str = "►";

#[derive(Clone, Copy, PartialEq)]
enum Mark {
    None,
    Check,
    Radio,
}

#[derive(Clone)]
pub struct Item<T> {
    label: String,
    shortcut: &'static str,
    mark: Mark,
    on: bool,
    action: Option<T>,
    sub: Vec<Entry<T>>,
}

#[derive(Clone)]
pub enum Entry<T> {
    Item(Item<T>),
    Separator,
}

impl<T: Copy> Entry<T> {
    pub fn item(label: impl Into<String>, action: T) -> Self {
        Self::new(label.into(), Some(action), Vec::new())
    }

    pub fn submenu(label: impl Into<String>, entries: Vec<Entry<T>>) -> Self {
        Self::new(label.into(), None, entries)
    }

    fn new(label: String, action: Option<T>, sub: Vec<Entry<T>>) -> Self {
        Entry::Item(Item {
            label,
            shortcut: "",
            mark: Mark::None,
            on: false,
            action,
            sub,
        })
    }

    fn with(mut self, f: impl FnOnce(&mut Item<T>)) -> Self {
        if let Entry::Item(item) = &mut self {
            f(item);
        }
        self
    }

    /// The key that does the same, shown right-aligned.
    pub fn shortcut(self, keys: &'static str) -> Self {
        self.with(|i| i.shortcut = keys)
    }

    /// A toggle, ticked when `on`.
    pub fn checked(self, on: bool) -> Self {
        self.with(|i| (i.mark, i.on) = (Mark::Check, on))
    }

    /// One of a set of choices, dotted when it's the current one.
    pub fn radio(self, on: bool) -> Self {
        self.with(|i| (i.mark, i.on) = (Mark::Radio, on))
    }

    fn as_item(&self) -> Option<&Item<T>> {
        match self {
            Entry::Item(i) => Some(i),
            Entry::Separator => None,
        }
    }

    fn height(&self) -> i32 {
        match self {
            Entry::Item(_) => ROW_H,
            Entry::Separator => SEPARATOR_H,
        }
    }
}

/// One open list: the menu itself or its submenu.
struct Panel<T> {
    x: i32,
    y: i32,
    entries: Vec<Entry<T>>,
    hover: Option<usize>,
}

impl<T: Copy> Panel<T> {
    fn items(&self) -> impl Iterator<Item = &Item<T>> {
        self.entries.iter().filter_map(Entry::as_item)
    }

    fn mark_w(&self) -> i32 {
        if self.items().any(|i| i.mark != Mark::None) {
            MARK_W
        } else {
            0
        }
    }

    fn arrow_w(&self) -> i32 {
        if self.items().any(|i| !i.sub.is_empty()) {
            font::W + 2
        } else {
            0
        }
    }

    fn size(&self) -> (i32, i32) {
        let label = self
            .items()
            .map(|i| font::width(&i.label))
            .max()
            .unwrap_or(0);
        let shortcut = self
            .items()
            .map(|i| font::width(i.shortcut))
            .max()
            .unwrap_or(0);
        let gap = if shortcut > 0 { SHORTCUT_GAP } else { 0 };
        let w = 2 * EDGE + self.mark_w() + label + gap + shortcut + self.arrow_w();
        let h = 2 * EDGE + self.entries.iter().map(Entry::height).sum::<i32>();
        (w, h)
    }

    fn bounds(&self) -> Rect {
        let (w, h) = self.size();
        rect(self.x, self.y, w, h)
    }

    /// Top of entry `i`, relative to the panel.
    fn entry_y(&self, i: usize) -> i32 {
        EDGE + self.entries[..i].iter().map(Entry::height).sum::<i32>()
    }

    /// The item (not separator) under (x, y).
    fn item_at(&self, x: i32, y: i32) -> Option<usize> {
        if !self.bounds().contains(x, y) {
            return None;
        }
        (0..self.entries.len()).find(|i| {
            let top = self.y + self.entry_y(*i);
            self.entries[*i].as_item().is_some() && (top..top + ROW_H).contains(&y)
        })
    }

    fn item(&self, i: usize) -> Option<&Item<T>> {
        self.entries.get(i).and_then(Entry::as_item)
    }

    /// Moves the highlight `step` items (±1) along, wrapping and skipping separators.
    fn step(&mut self, step: isize) {
        let n = self.entries.len() as isize;
        let mut i = self
            .hover
            .map_or(if step > 0 { -1 } else { n }, |h| h as isize);
        for _ in 0..n {
            i = (i + step).rem_euclid(n);
            if self.entries[i as usize].as_item().is_some() {
                self.hover = Some(i as usize);
                return;
            }
        }
    }

    fn draw(&self, fb: &mut Framebuffer, colors: PlaylistColors) {
        let (w, h) = self.size();
        fb.resize(w, h);
        fb.fill(rect(0, 0, w, h), colors.normal);
        fb.fill(rect(1, 1, w - 2, h - 2), colors.normal_bg);
        let dim = blend(colors.normal, colors.normal_bg);
        let (mark_w, arrow_w) = (self.mark_w(), self.arrow_w());
        for (i, entry) in self.entries.iter().enumerate() {
            let y = self.entry_y(i);
            let Some(item) = entry.as_item() else {
                fb.fill(rect(EDGE, y + SEPARATOR_H / 2, w - 2 * EDGE, 1), dim);
                continue;
            };
            let row = rect(1, y, w - 2, ROW_H);
            let color = if self.hover == Some(i) {
                fb.fill(row, colors.selected_bg);
                colors.current
            } else {
                colors.normal
            };
            if item.on {
                let mark = if item.mark == Mark::Radio {
                    "●"
                } else {
                    "✓"
                };
                font::draw(fb, mark, EDGE, y, color, row);
            }
            font::draw(fb, &item.label, EDGE + mark_w, y, color, row);
            let sx = w - EDGE - arrow_w - font::width(item.shortcut);
            font::draw(fb, item.shortcut, sx, y, color, row);
            if !item.sub.is_empty() {
                font::draw(fb, ARROW, w - EDGE - font::W, y, color, row);
            }
        }
    }
}

/// Halfway between two 0xRRGGBB colours.
fn blend(a: u32, b: u32) -> u32 {
    let ch = |c: u32, s: u32| (((c >> s & 0xff) + (b >> s & 0xff)) / 2) << s;
    ch(a, 16) | ch(a, 8) | ch(a, 0)
}

pub enum KeyResult<T> {
    /// The key was used by the menu (or ignored while it is open).
    Handled,
    Close,
    Chosen(T),
}

pub struct Menu<T> {
    root: Panel<T>,
    /// The open submenu and the index of the root item it belongs to.
    sub: Option<(usize, Panel<T>)>,
    /// Keyboard focus is in the submenu.
    in_sub: bool,
    bounds: (i32, i32),
}

impl<T: Copy> Menu<T> {
    /// Opens at (x, y), moved up or left as needed to fit a `bounds_w` x `bounds_h` window.
    pub fn new(entries: Vec<Entry<T>>, x: i32, y: i32, bounds_w: i32, bounds_h: i32) -> Self {
        let mut root = Panel {
            x,
            y,
            entries,
            hover: None,
        };
        let (w, h) = root.size();
        root.x = x.min(bounds_w - w).max(0);
        root.y = y.min(bounds_h - h).max(0);
        Menu {
            root,
            sub: None,
            in_sub: false,
            bounds: (bounds_w, bounds_h),
        }
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        self.root.bounds().contains(x, y)
            || self
                .sub
                .as_ref()
                .is_some_and(|(_, s)| s.bounds().contains(x, y))
    }

    /// Opens root item `i`'s submenu beside it (if it has one and it isn't open already).
    fn open_sub(&mut self, i: usize) {
        if self.sub.as_ref().is_some_and(|(open, _)| *open == i) {
            return;
        }
        self.sub = None;
        self.in_sub = false;
        let Some(item) = self.root.item(i) else {
            return;
        };
        if item.sub.is_empty() {
            return;
        }
        let mut sub = Panel {
            x: 0,
            y: 0,
            entries: item.sub.clone(),
            hover: None,
        };
        let (w, h) = sub.size();
        let root = self.root.bounds();
        let (bw, bh) = self.bounds;
        // To the right, overlapping the border; to the left if there's no room.
        sub.x = if root.x + root.w - 1 + w <= bw {
            root.x + root.w - 1
        } else {
            (root.x - w + 1).max(0)
        };
        sub.y = (root.y + self.root.entry_y(i) - EDGE).min(bh - h).max(0);
        self.sub = Some((i, sub));
    }

    pub fn mouse_move(&mut self, x: i32, y: i32) {
        if let Some((_, sub)) = &mut self.sub
            && sub.bounds().contains(x, y)
        {
            sub.hover = sub.item_at(x, y);
            self.in_sub = true;
            return;
        }
        if self.root.bounds().contains(x, y) {
            self.root.hover = self.root.item_at(x, y);
            self.in_sub = false;
            match self.root.hover {
                Some(i) => self.open_sub(i),
                None => self.sub = None,
            }
        } else if let Some((_, sub)) = &mut self.sub {
            sub.hover = None;
        } else {
            self.root.hover = None;
        }
    }

    /// The action of the item the pointer was released on, if any.
    pub fn mouse_up(&mut self, x: i32, y: i32) -> Option<T> {
        if let Some((_, sub)) = &self.sub
            && sub.bounds().contains(x, y)
        {
            return sub.item_at(x, y).and_then(|i| sub.item(i)?.action);
        }
        let i = self.root.item_at(x, y)?;
        self.root.item(i)?.action
    }

    pub fn key(&mut self, key: Keycode) -> KeyResult<T> {
        let in_sub = self.in_sub && self.sub.is_some();
        let panel = match &mut self.sub {
            Some((_, sub)) if in_sub => sub,
            _ => &mut self.root,
        };
        match key {
            Keycode::Up => panel.step(-1),
            Keycode::Down => panel.step(1),
            Keycode::Escape | Keycode::Left if in_sub => {
                self.in_sub = false;
                if key == Keycode::Escape {
                    self.sub = None;
                }
            }
            Keycode::Escape => return KeyResult::Close,
            Keycode::Return | Keycode::KpEnter | Keycode::Right => {
                let Some(i) = panel.hover else {
                    return KeyResult::Handled;
                };
                let action = panel.item(i).and_then(|item| item.action);
                match action {
                    Some(a) if key != Keycode::Right => return KeyResult::Chosen(a),
                    Some(_) => {}
                    None if !in_sub => {
                        self.open_sub(i);
                        if let Some((_, sub)) = &mut self.sub {
                            self.in_sub = true;
                            sub.step(1);
                        }
                    }
                    None => {}
                }
            }
            _ => {}
        }
        KeyResult::Handled
    }

    /// Draws the menu (`level` 0) or its submenu (1) into `fb`, sized to fit, and returns where
    /// it goes; None when that level isn't open.
    pub fn draw(
        &self,
        level: usize,
        fb: &mut Framebuffer,
        colors: PlaylistColors,
    ) -> Option<(i32, i32)> {
        let panel = match level {
            0 => &self.root,
            _ => &self.sub.as_ref()?.1,
        };
        panel.draw(fb, colors);
        Some((panel.x, panel.y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu() -> Menu<u8> {
        let entries = vec![
            Entry::item("One", 1).checked(true),
            Entry::Separator,
            Entry::submenu("More", vec![Entry::item("Two", 2), Entry::item("Three", 3)]),
        ];
        Menu::new(entries, 10, 10, 275, 116)
    }

    #[test]
    fn mouse_picks_items_and_opens_submenus() {
        let mut m = menu();
        let r = m.root.bounds();
        // "One" is the first row.
        assert_eq!(m.mouse_up(r.x + 5, r.y + EDGE + 1), Some(1));
        // Hovering "More" opens its submenu beside the menu.
        let more_y = r.y + m.root.entry_y(2) + 1;
        m.mouse_move(r.x + 5, more_y);
        let s = m.sub.as_ref().unwrap().1.bounds();
        assert_eq!(s.x, r.x + r.w - 1);
        assert_eq!(m.mouse_up(s.x + 5, s.y + EDGE + ROW_H + 1), Some(3));
        // The separator and the submenu's own item do nothing.
        assert_eq!(m.mouse_up(r.x + 5, r.y + m.root.entry_y(1) + 1), None);
        assert_eq!(m.mouse_up(r.x + 5, more_y), None);
    }

    #[test]
    fn keyboard_walks_items_and_submenus() {
        let mut m = menu();
        m.key(Keycode::Down);
        m.key(Keycode::Down); // Skips the separator.
        assert_eq!(m.root.hover, Some(2));
        m.key(Keycode::Right);
        m.key(Keycode::Down);
        assert!(matches!(m.key(Keycode::Return), KeyResult::Chosen(3)));
        m.key(Keycode::Left);
        assert!(matches!(m.key(Keycode::Escape), KeyResult::Close));
    }

    #[test]
    fn stays_inside_the_window() {
        let m = Menu::new(vec![Entry::item("Something long", 0u8)], 270, 110, 275, 116);
        let r = m.root.bounds();
        assert!(r.x >= 0 && r.x + r.w <= 275 && r.y >= 0 && r.y + r.h <= 116);
    }
}
