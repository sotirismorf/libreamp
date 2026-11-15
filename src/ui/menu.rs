//! Popup menus (the right-click menu, EQ presets), with one level of submenus. Classic skins
//! have no graphics for arbitrary menus, so they use the playlist font and colours.

use std::time::{Duration, Instant};

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
/// Rows scrolled by a click on a scroll arrow or a wheel notch.
const SCROLL_STEP: isize = 2;
/// While the pointer heads for an open submenu it may cross other items; one of them only
/// takes over once the pointer has rested on it this long.
const SWITCH_DELAY: Duration = Duration::from_millis(300);

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
    /// The tallest the panel may be (the window's height); longer lists scroll.
    max_h: i32,
    /// First entry shown when the list scrolls.
    scroll: usize,
}

impl<T: Copy> Panel<T> {
    fn new(entries: Vec<Entry<T>>, max_h: i32) -> Self {
        Panel {
            x: 0,
            y: 0,
            entries,
            hover: None,
            max_h,
            scroll: 0,
        }
    }

    fn full_h(&self) -> i32 {
        2 * EDGE + self.entries.iter().map(Entry::height).sum::<i32>()
    }

    /// Whether the list is too tall for the window and shows scroll arrows.
    fn scrolls(&self) -> bool {
        self.full_h() > self.max_h
    }

    /// Height available to entries between the scroll arrows.
    fn list_h(&self) -> i32 {
        self.max_h - 2 * EDGE - 2 * ROW_H
    }

    /// The entries shown: all of them, or those that fit from `scroll` on.
    fn visible(&self) -> std::ops::Range<usize> {
        if !self.scrolls() {
            return 0..self.entries.len();
        }
        let mut used = 0;
        let end = (self.scroll..self.entries.len())
            .find(|i| {
                used += self.entries[*i].height();
                used > self.list_h()
            })
            .unwrap_or(self.entries.len());
        self.scroll..end
    }

    /// The largest `scroll` that still fills the list.
    fn max_scroll(&self) -> usize {
        let mut used = 0;
        for i in (0..self.entries.len()).rev() {
            used += self.entries[i].height();
            if used > self.list_h() {
                return i + 1;
            }
        }
        0
    }

    fn scroll_by(&mut self, rows: isize) {
        let s = (self.scroll as isize + rows).max(0) as usize;
        self.scroll = s.min(self.max_scroll());
    }

    fn ensure_visible(&mut self, i: usize) {
        if !self.scrolls() {
            return;
        }
        if i < self.scroll {
            self.scroll = i;
        }
        while i >= self.visible().end && self.scroll < self.max_scroll() {
            self.scroll += 1;
        }
    }

    /// -1 or 1 for the up or down scroll arrow under (x, y).
    fn arrow_at(&self, x: i32, y: i32) -> Option<isize> {
        if !self.scrolls() || !self.bounds().contains(x, y) {
            return None;
        }
        let (_, h) = self.size();
        let y = y - self.y;
        if (EDGE..EDGE + ROW_H).contains(&y) {
            Some(-1)
        } else if (h - EDGE - ROW_H..h - EDGE).contains(&y) {
            Some(1)
        } else {
            None
        }
    }
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
        (w, self.full_h().min(self.max_h))
    }

    fn bounds(&self) -> Rect {
        let (w, h) = self.size();
        rect(self.x, self.y, w, h)
    }

    /// Top of entry `i`, relative to the panel (meaningful for visible entries).
    fn entry_y(&self, i: usize) -> i32 {
        let (top, first) = if self.scrolls() {
            (EDGE + ROW_H, self.scroll.min(i))
        } else {
            (EDGE, 0)
        };
        top + self.entries[first..i]
            .iter()
            .map(Entry::height)
            .sum::<i32>()
    }

    /// The item (not separator) under (x, y).
    fn item_at(&self, x: i32, y: i32) -> Option<usize> {
        if !self.bounds().contains(x, y) || self.arrow_at(x, y).is_some() {
            return None;
        }
        self.visible().find(|i| {
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
                self.ensure_visible(i as usize);
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
        if self.scrolls() {
            let x = (w - font::W) / 2;
            let up = if self.scroll > 0 { colors.normal } else { dim };
            let down = if self.scroll < self.max_scroll() {
                colors.normal
            } else {
                dim
            };
            let clip = rect(1, 1, w - 2, h - 2);
            font::draw(fb, "▲", x, EDGE, up, clip);
            font::draw(fb, "▼", x, h - EDGE - ROW_H, down, clip);
        }
        for i in self.visible() {
            let entry = &self.entries[i];
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
    /// The pointer's last position, to tell where it is heading.
    pointer: Option<(i32, i32)>,
    /// A root item the pointer crossed on its way to the open submenu, and since when; it
    /// opens if the pointer stays on it for `SWITCH_DELAY`.
    pending: Option<(usize, Instant)>,
}

impl<T: Copy> Menu<T> {
    /// Opens at (x, y), moved up or left as needed to fit a `bounds_w` x `bounds_h` window.
    pub fn new(entries: Vec<Entry<T>>, x: i32, y: i32, bounds_w: i32, bounds_h: i32) -> Self {
        let mut root = Panel::new(entries, bounds_h);
        let (w, h) = root.size();
        root.x = x.min(bounds_w - w).max(0);
        root.y = y.min(bounds_h - h).max(0);
        Menu {
            root,
            sub: None,
            in_sub: false,
            bounds: (bounds_w, bounds_h),
            pointer: None,
            pending: None,
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
        let mut sub = Panel::new(item.sub.clone(), self.bounds.1);
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
        self.mouse_move_at(x, y, Instant::now());
    }

    fn mouse_move_at(&mut self, x: i32, y: i32, now: Instant) {
        let from = self.pointer.replace((x, y));
        if let Some((parent, sub)) = &mut self.sub
            && sub.bounds().contains(x, y)
        {
            sub.hover = sub.item_at(x, y);
            self.root.hover = Some(*parent);
            self.in_sub = true;
            self.pending = None;
            return;
        }
        if !self.root.bounds().contains(x, y) {
            match &mut self.sub {
                Some((_, sub)) => sub.hover = None,
                None => self.root.hover = None,
            }
            self.pending = None;
            return;
        }
        self.in_sub = false;
        let open = self.sub.as_ref().map(|(parent, _)| *parent);
        match self.root.item_at(x, y) {
            // Borders, padding and separators leave whatever is open alone.
            None => {}
            Some(i) if open == Some(i) => {
                self.root.hover = Some(i);
                self.pending = None;
            }
            // Crossing other items on the way to the open submenu: wait and see.
            Some(i) if from.is_some_and(|from| self.heading_for_sub(from, (x, y))) => {
                if self.pending.is_none_or(|(p, _)| p != i) {
                    self.pending = Some((i, now));
                }
            }
            Some(i) => {
                self.root.hover = Some(i);
                self.pending = None;
                self.open_sub(i);
            }
        }
    }

    /// True if moving from `from` to `to` keeps the pointer within the triangle between `from`
    /// and the open submenu's near edge, i.e. it is on its way into the submenu.
    fn heading_for_sub(&self, from: (i32, i32), to: (i32, i32)) -> bool {
        let Some((_, sub)) = &self.sub else {
            return false;
        };
        if from == to {
            return false;
        }
        let s = sub.bounds();
        let edge_x = if s.x >= from.0 { s.x } else { s.x + s.w };
        // A little slack above and below, so a slightly shaky path still counts.
        let (top, bottom) = ((edge_x, s.y - 4), (edge_x, s.y + s.h + 4));
        let cross = |a: (i32, i32), b: (i32, i32), p: (i32, i32)| {
            (b.0 - a.0) as i64 * (p.1 - a.1) as i64 - (b.1 - a.1) as i64 * (p.0 - a.0) as i64
        };
        let d = [
            cross(from, top, to),
            cross(top, bottom, to),
            cross(bottom, from, to),
        ];
        d.iter().all(|v| *v >= 0) || d.iter().all(|v| *v <= 0)
    }

    /// Opens the submenu of an item the pointer has rested on long enough; call every frame.
    pub fn tick(&mut self, now: Instant) {
        let Some((i, since)) = self.pending else {
            return;
        };
        if now.duration_since(since) < SWITCH_DELAY {
            return;
        }
        self.pending = None;
        if let Some((x, y)) = self.pointer
            && self.root.item_at(x, y) == Some(i)
        {
            self.root.hover = Some(i);
            self.open_sub(i);
        }
    }

    /// The panel under (x, y): the submenu, which lies on top, or the menu.
    fn panel_at(&mut self, x: i32, y: i32) -> Option<&mut Panel<T>> {
        match &mut self.sub {
            Some((_, sub)) if sub.bounds().contains(x, y) => Some(sub),
            _ if self.root.bounds().contains(x, y) => Some(&mut self.root),
            _ => None,
        }
    }

    /// The action of the item the pointer was released on, if any. Releasing on a scroll
    /// arrow scrolls.
    pub fn mouse_up(&mut self, x: i32, y: i32) -> Option<T> {
        let panel = self.panel_at(x, y)?;
        if let Some(dir) = panel.arrow_at(x, y) {
            panel.scroll_by(dir * SCROLL_STEP);
            return None;
        }
        let i = panel.item_at(x, y)?;
        panel.item(i)?.action
    }

    /// Scrolls the list under the pointer, if it scrolls.
    pub fn wheel(&mut self, x: i32, y: i32, notches: i32) {
        if let Some(panel) = self.panel_at(x, y) {
            panel.scroll_by(-notches as isize * SCROLL_STEP);
        }
    }

    pub fn key(&mut self, key: Keycode) -> KeyResult<T> {
        self.pending = None;
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

    /// A menu whose first item has a tall submenu, with items and a separator below it.
    fn aim_menu() -> Menu<u8> {
        let skins = (0..8)
            .map(|i| Entry::item(format!("Skin {i}"), 10 + i))
            .collect();
        let entries = vec![
            Entry::submenu("Skin", skins),
            Entry::item("Other", 1),
            Entry::Separator,
            Entry::item("Quit", 2),
        ];
        Menu::new(entries, 0, 0, 275, 200)
    }

    #[test]
    fn gaps_and_separators_keep_the_submenu_open() {
        let t = Instant::now();
        let mut m = aim_menu();
        let r = m.root.bounds();
        m.mouse_move_at(r.x + 5, r.y + m.root.entry_y(0) + 2, t);
        assert!(m.sub.is_some());
        // The separator row, then the menu's own border.
        m.mouse_move_at(r.x + 5, r.y + m.root.entry_y(2) + 2, t);
        m.mouse_move_at(r.x, r.y + 1, t);
        assert_eq!(m.sub.as_ref().map(|(p, _)| *p), Some(0));
    }

    #[test]
    fn crossing_items_on_the_way_to_the_submenu_does_not_close_it() {
        let t = Instant::now();
        let mut m = aim_menu();
        let r = m.root.bounds();
        let skin_y = r.y + m.root.entry_y(0) + 6;
        m.mouse_move_at(r.x + 10, skin_y, t);
        let s = m.sub.as_ref().unwrap().1.bounds();
        // Heading down-right towards the submenu's lower part, crossing "Other".
        let other_y = r.y + m.root.entry_y(1) + 6;
        m.mouse_move_at(s.x - 10, other_y, t);
        assert_eq!(m.sub.as_ref().map(|(p, _)| *p), Some(0));
        assert_eq!(m.root.hover, Some(0));
        // Resting on "Other" for a moment lets it take over.
        m.tick(t + SWITCH_DELAY / 2);
        assert!(m.sub.is_some());
        m.tick(t + SWITCH_DELAY);
        assert!(m.sub.is_none());
        assert_eq!(m.root.hover, Some(1));
    }

    #[test]
    fn moving_away_from_the_submenu_switches_at_once() {
        let t = Instant::now();
        let mut m = aim_menu();
        let r = m.root.bounds();
        m.mouse_move_at(r.x + 40, r.y + m.root.entry_y(0) + 6, t);
        // Straight down and to the left: away from the submenu.
        m.mouse_move_at(r.x + 5, r.y + m.root.entry_y(3) + 6, t);
        assert!(m.sub.is_none());
        assert_eq!(m.root.hover, Some(3));
    }

    #[test]
    fn long_lists_scroll_within_the_window() {
        let entries: Vec<Entry<u8>> = (0..20)
            .map(|i| Entry::item(format!("Skin {i}"), i))
            .collect();
        let mut m = Menu::new(entries, 0, 0, 275, 116);
        let r = m.root.bounds();
        assert_eq!(r.h, 116);
        let shown = m.root.visible();
        assert_eq!(shown.start, 0);
        assert!(shown.end < 20);
        // The first row under the up arrow is item 0; the down arrow scrolls.
        assert_eq!(m.mouse_up(r.x + 5, r.y + EDGE + ROW_H + 1), Some(0));
        assert_eq!(m.mouse_up(r.x + 5, r.y + r.h - EDGE - 1), None);
        assert_eq!(m.root.scroll, SCROLL_STEP as usize);
        // The wheel scrolls, but never past the end.
        m.wheel(r.x + 5, r.y + 50, -100);
        assert_eq!(m.root.visible().end, 20);
        // Walking up with the keyboard brings the top back into view.
        for _ in 0..19 {
            m.key(Keycode::Up);
        }
        assert_eq!(m.root.hover, Some(1));
        assert!(m.root.visible().contains(&1));
    }

    #[test]
    fn stays_inside_the_window() {
        let m = Menu::new(vec![Entry::item("Something long", 0u8)], 270, 110, 275, 116);
        let r = m.root.bounds();
        assert!(r.x >= 0 && r.x + r.w <= 275 && r.y >= 0 && r.y + r.h <= 116);
    }
}
