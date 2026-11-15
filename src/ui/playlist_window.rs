//! The playlist editor window (pledit.bmp), drawn below the main window.

use crate::font;
use crate::gfx::{Framebuffer, Rect, rect};
use crate::playlist::Playlist;
use crate::skin::Skin;
use crate::ui::{Action, draw_string};
use sdl3::keyboard::Keycode;

pub const W: i32 = 275;
const TOP_H: i32 = 20;
const BOTTOM_H: i32 = 38;
const TILE_H: i32 = 29;
pub const MIN_TILES: u32 = 2;
pub const DEFAULT_TILES: u32 = 6;
pub const MAX_TILES: u32 = 40;
const ROW_H: i32 = 13;
const HANDLE: (i32, i32) = (8, 18);

/// Things only the app can do, requested by the playlist window.
#[derive(Debug)]
pub enum PlCommand {
    Play(usize),
    Close,
    Transport(Action),
    AddFiles,
    AddDir,
    SaveList,
    LoadList,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Menu {
    Add,
    Remove,
    Select,
    Misc,
    List,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Item {
    AddUrl,
    AddDir,
    AddFile,
    RemoveMisc,
    RemoveAll,
    Crop,
    RemoveSelected,
    InvertSelection,
    SelectZero,
    SelectAll,
    SortList,
    FileInfo,
    MiscOptions,
    NewList,
    SaveList,
    LoadList,
}

impl Menu {
    const ALL: [Menu; 5] = [
        Menu::Add,
        Menu::Remove,
        Menu::Select,
        Menu::Misc,
        Menu::List,
    ];

    /// Items from top to bottom, with the x of their sprite column in pledit.bmp and its y.
    fn items(self) -> &'static [(Item, i32, i32)] {
        match self {
            Menu::Add => &[
                (Item::AddUrl, 0, 111),
                (Item::AddDir, 0, 130),
                (Item::AddFile, 0, 149),
            ],
            Menu::Remove => &[
                (Item::RemoveMisc, 54, 168),
                (Item::RemoveAll, 54, 111),
                (Item::Crop, 54, 130),
                (Item::RemoveSelected, 54, 149),
            ],
            Menu::Select => &[
                (Item::InvertSelection, 104, 111),
                (Item::SelectZero, 104, 130),
                (Item::SelectAll, 104, 149),
            ],
            Menu::Misc => &[
                (Item::SortList, 154, 111),
                (Item::FileInfo, 154, 130),
                (Item::MiscOptions, 154, 149),
            ],
            Menu::List => &[
                (Item::NewList, 204, 111),
                (Item::SaveList, 204, 130),
                (Item::LoadList, 204, 149),
            ],
        }
    }

    /// The thin bar drawn left of an open menu.
    fn bar(self) -> Rect {
        match self {
            Menu::Add => rect(48, 111, 3, 54),
            Menu::Remove => rect(100, 111, 3, 72),
            Menu::Select => rect(150, 111, 3, 54),
            Menu::Misc => rect(200, 111, 3, 54),
            Menu::List => rect(250, 111, 3, 54),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Press {
    Close,
    Shade,
    Resize { from_bottom: i32 },
    ScrollHandle { grab: i32 },
    ScrollUp,
    ScrollDown,
    Mini(usize),
    MenuItem,
    Rows,
}

pub struct PlaylistWindow {
    pub tiles: u32,
    pub focused: bool,
    scroll: usize,
    /// Selection anchor for shift-clicks.
    anchor: usize,
    pressed: Option<Press>,
    over_pressed: bool,
    menu: Option<Menu>,
    menu_hover: Option<usize>,
}

const MINI_ACTIONS: [Action; 6] = [
    Action::Prev,
    Action::Play,
    Action::Pause,
    Action::Stop,
    Action::Next,
    Action::Eject,
];

impl PlaylistWindow {
    pub fn new(tiles: u32) -> Self {
        PlaylistWindow {
            tiles: tiles.clamp(MIN_TILES, MAX_TILES),
            focused: true,
            scroll: 0,
            anchor: 0,
            pressed: None,
            over_pressed: false,
            menu: None,
            menu_hover: None,
        }
    }

    pub fn height(&self) -> i32 {
        TOP_H + self.tiles as i32 * TILE_H + BOTTOM_H
    }

    // Geometry, all relative to the window's top-left.

    fn list_area(&self) -> Rect {
        rect(12, TOP_H, W - 32, self.height() - TOP_H - BOTTOM_H)
    }

    fn visible_rows(&self) -> usize {
        ((self.list_area().h - 6) / ROW_H).max(0) as usize
    }

    fn row_at(&self, y: i32) -> Option<usize> {
        let rel = y - (TOP_H + 3);
        (rel >= 0).then(|| self.scroll + (rel / ROW_H) as usize)
    }

    fn close_button() -> Rect {
        rect(W - 11, 3, 9, 9)
    }

    fn shade_button() -> Rect {
        rect(W - 21, 3, 9, 9)
    }

    fn resize_grip(&self) -> Rect {
        rect(W - 20, self.height() - 20, 20, 20)
    }

    fn scroll_track(&self) -> Rect {
        rect(W - 15, TOP_H, HANDLE.0, self.list_area().h)
    }

    fn scroll_up(&self) -> Rect {
        rect(W - 15, self.height() - BOTTOM_H + 2, 8, 5)
    }

    fn scroll_down(&self) -> Rect {
        rect(W - 15, self.height() - BOTTOM_H + 8, 8, 5)
    }

    fn mini_button(&self, i: usize) -> Rect {
        rect(
            W - 150 + 3 + 10 * i as i32,
            self.height() - BOTTOM_H + 22,
            10,
            10,
        )
    }

    fn menu_button(&self, m: Menu) -> Rect {
        let x = match m {
            Menu::Add => 14,
            Menu::Remove => 43,
            Menu::Select => 72,
            Menu::Misc => 101,
            Menu::List => W - 44,
        };
        rect(x, self.height() - 30, 22, 18)
    }

    fn menu_item_rect(&self, m: Menu, i: usize) -> Rect {
        let b = self.menu_button(m);
        let from_bottom = (m.items().len() - 1 - i) as i32;
        rect(b.x, b.y - from_bottom * 18, 22, 18)
    }

    fn max_scroll(&self, playlist: &Playlist) -> usize {
        playlist.len().saturating_sub(self.visible_rows())
    }

    fn handle_y(&self, playlist: &Playlist) -> i32 {
        let track = self.scroll_track();
        let max = self.max_scroll(playlist);
        let frac = if max == 0 {
            0.0
        } else {
            self.scroll as f32 / max as f32
        };
        track.y + ((track.h - HANDLE.1) as f32 * frac).round() as i32
    }

    /// True where dragging should move the window.
    pub fn is_draggable(x: i32, y: i32) -> bool {
        rect(0, 0, W, TOP_H).contains(x, y)
            && !Self::close_button().contains(x, y)
            && !Self::shade_button().contains(x, y)
    }

    pub fn ensure_visible(&mut self, index: usize) {
        let rows = self.visible_rows().max(1);
        if index < self.scroll {
            self.scroll = index;
        } else if index >= self.scroll + rows {
            self.scroll = index + 1 - rows;
        }
    }

    pub fn wheel(&mut self, rows: i32, playlist: &Playlist) {
        let s = (self.scroll as i32 - rows).max(0) as usize;
        self.scroll = s.min(self.max_scroll(playlist));
    }

    // Mouse handling.

    pub fn mouse_down(
        &mut self,
        x: i32,
        y: i32,
        clicks: u8,
        ctrl: bool,
        shift: bool,
        playlist: &mut Playlist,
    ) -> Option<PlCommand> {
        if let Some(m) = self.menu {
            if let Some(i) = self.menu_item_at(m, x, y) {
                self.menu_hover = Some(i);
                self.pressed = Some(Press::MenuItem);
                return None;
            }
            self.menu = None;
            if self.menu_button(m).contains(x, y) {
                return None;
            }
        }
        if let Some(&m) = Menu::ALL
            .iter()
            .find(|m| self.menu_button(**m).contains(x, y))
        {
            self.menu = Some(m);
            self.menu_hover = None;
            self.pressed = Some(Press::MenuItem);
            return None;
        }

        self.over_pressed = true;
        let press = if Self::close_button().contains(x, y) {
            Press::Close
        } else if Self::shade_button().contains(x, y) {
            Press::Shade
        } else if self.resize_grip().contains(x, y) {
            Press::Resize {
                from_bottom: self.height() - y,
            }
        } else if self.scroll_up().contains(x, y) {
            Press::ScrollUp
        } else if self.scroll_down().contains(x, y) {
            Press::ScrollDown
        } else if let Some(i) =
            (0..MINI_ACTIONS.len()).find(|i| self.mini_button(*i).contains(x, y))
        {
            Press::Mini(i)
        } else if self.scroll_track().contains(x, y) {
            let handle = self.handle_y(playlist);
            let grab = if (handle..handle + HANDLE.1).contains(&y) {
                y - handle
            } else {
                HANDLE.1 / 2
            };
            Press::ScrollHandle { grab }
        } else if self.list_area().contains(x, y) {
            return self.click_rows(y, clicks, ctrl, shift, playlist);
        } else {
            return None;
        };
        self.pressed = Some(press);
        self.drag(x, y, playlist);
        None
    }

    fn click_rows(
        &mut self,
        y: i32,
        clicks: u8,
        ctrl: bool,
        shift: bool,
        playlist: &mut Playlist,
    ) -> Option<PlCommand> {
        self.pressed = Some(Press::Rows);
        let Some(i) = self.row_at(y).filter(|i| *i < playlist.len()) else {
            playlist.select_all(false);
            return None;
        };
        if clicks >= 2 {
            return Some(PlCommand::Play(i));
        }
        if shift {
            let (a, b) = (self.anchor.min(i), self.anchor.max(i));
            for (j, e) in playlist.entries.iter_mut().enumerate() {
                e.selected = (a..=b).contains(&j) || (ctrl && e.selected);
            }
        } else if ctrl {
            playlist.entries[i].selected ^= true;
            self.anchor = i;
        } else {
            playlist.select_all(false);
            playlist.entries[i].selected = true;
            self.anchor = i;
        }
        None
    }

    fn menu_item_at(&self, m: Menu, x: i32, y: i32) -> Option<usize> {
        (0..m.items().len()).find(|i| self.menu_item_rect(m, *i).contains(x, y))
    }

    /// Updates whatever is being dragged: resize grip, scroll handle, menu hover, button state.
    fn drag(&mut self, x: i32, y: i32, playlist: &Playlist) {
        match self.pressed {
            Some(Press::Resize { from_bottom }) => {
                let target = y + from_bottom;
                let tiles = ((target - TOP_H - BOTTOM_H) as f32 / TILE_H as f32).round();
                self.tiles = (tiles.max(0.0) as u32).clamp(MIN_TILES, MAX_TILES);
                self.wheel(0, playlist);
            }
            Some(Press::ScrollHandle { grab }) => {
                let track = self.scroll_track();
                let span = (track.h - HANDLE.1).max(1) as f32;
                let frac = ((y - grab - track.y) as f32 / span).clamp(0.0, 1.0);
                self.scroll = (frac * self.max_scroll(playlist) as f32).round() as usize;
            }
            Some(Press::MenuItem) => {
                if let Some(m) = self.menu {
                    self.menu_hover = self.menu_item_at(m, x, y);
                }
            }
            Some(p) => {
                self.over_pressed = self.press_rect(p).is_none_or(|r| r.contains(x, y));
            }
            None => {}
        }
    }

    fn press_rect(&self, p: Press) -> Option<Rect> {
        match p {
            Press::Close => Some(Self::close_button()),
            Press::Shade => Some(Self::shade_button()),
            Press::ScrollUp => Some(self.scroll_up()),
            Press::ScrollDown => Some(self.scroll_down()),
            Press::Mini(i) => Some(self.mini_button(i)),
            _ => None,
        }
    }

    pub fn mouse_move(&mut self, x: i32, y: i32, playlist: &Playlist) {
        if self.pressed.is_some() {
            self.drag(x, y, playlist);
        } else if let Some(m) = self.menu {
            self.menu_hover = self.menu_item_at(m, x, y);
        }
    }

    pub fn mouse_up(&mut self, x: i32, y: i32, playlist: &mut Playlist) -> Option<PlCommand> {
        let press = self.pressed.take()?;
        if press == Press::MenuItem {
            let m = self.menu?;
            if let Some(i) = self.menu_item_at(m, x, y) {
                self.menu = None;
                return self.run_item(m.items()[i].0, playlist);
            }
            // Releasing on the menu's own button leaves it open for a second click.
            if !self.menu_button(m).contains(x, y) {
                self.menu = None;
            }
            return None;
        }
        if !self.press_rect(press).is_some_and(|r| r.contains(x, y)) {
            return None;
        }
        match press {
            Press::Close => Some(PlCommand::Close),
            Press::ScrollUp => {
                self.wheel(1, playlist);
                None
            }
            Press::ScrollDown => {
                self.wheel(-1, playlist);
                None
            }
            Press::Mini(i) => Some(PlCommand::Transport(MINI_ACTIONS[i])),
            _ => None,
        }
    }

    fn run_item(&mut self, item: Item, playlist: &mut Playlist) -> Option<PlCommand> {
        match item {
            Item::AddDir => return Some(PlCommand::AddDir),
            Item::AddFile => return Some(PlCommand::AddFiles),
            Item::SaveList => return Some(PlCommand::SaveList),
            Item::LoadList => return Some(PlCommand::LoadList),
            Item::RemoveAll | Item::NewList => playlist.clear(),
            Item::Crop => playlist.crop(),
            Item::RemoveSelected => playlist.remove_selected(),
            Item::InvertSelection => playlist.invert_selection(),
            Item::SelectZero => playlist.select_all(false),
            Item::SelectAll => playlist.select_all(true),
            Item::SortList => playlist.sort_by_title(),
            // Not implemented: URLs, file info, misc options.
            Item::AddUrl | Item::RemoveMisc | Item::FileInfo | Item::MiscOptions => {}
        }
        self.wheel(0, playlist);
        None
    }

    // Keyboard.

    pub fn key(
        &mut self,
        key: Keycode,
        ctrl: bool,
        shift: bool,
        playlist: &mut Playlist,
    ) -> Option<PlCommand> {
        let len = playlist.len();
        let focus = playlist
            .entries
            .iter()
            .position(|e| e.selected)
            .unwrap_or(playlist.current);
        let page = self.visible_rows().max(1);
        let target = match key {
            Keycode::A if ctrl => {
                playlist.select_all(true);
                return None;
            }
            Keycode::Delete => {
                playlist.remove_selected();
                self.wheel(0, playlist);
                return None;
            }
            Keycode::Return | Keycode::KpEnter => {
                return (focus < len).then_some(PlCommand::Play(focus));
            }
            Keycode::Up => focus.saturating_sub(1),
            Keycode::Down => focus + 1,
            Keycode::PageUp => focus.saturating_sub(page),
            Keycode::PageDown => focus + page,
            Keycode::Home => 0,
            Keycode::End => len.saturating_sub(1),
            _ => return None,
        };
        if len == 0 {
            return None;
        }
        let target = target.min(len - 1);
        if !shift {
            playlist.select_all(false);
            self.anchor = target;
        }
        playlist.entries[target].selected = true;
        self.ensure_visible(target);
        None
    }

    // Drawing.

    pub fn draw(
        &self,
        fb: &mut Framebuffer,
        skin: &Skin,
        playlist: &Playlist,
        elapsed: Option<f32>,
    ) {
        let h = self.height();
        fb.resize(W, h);
        let s = skin.sheet("pledit");
        let sy = if self.focused { 0 } else { 21 };

        // Frame: top, sides, bottom.
        for x in (0..W).step_by(25) {
            fb.blit(s, rect(127, sy, 25, 20), x, 0);
        }
        fb.blit(s, rect(0, sy, 25, 20), 0, 0);
        fb.blit(s, rect(26, sy, 100, 20), (W - 100) / 2, 0);
        fb.blit(s, rect(153, sy, 25, 20), W - 25, 0);
        let middle = rect(0, TOP_H, W, h - TOP_H - BOTTOM_H);
        for y in (TOP_H..h - BOTTOM_H).step_by(TILE_H as usize) {
            fb.blit_clipped(s, rect(0, 42, 12, 29), 0, y, middle);
            fb.blit_clipped(s, rect(31, 42, 20, 29), W - 20, y, middle);
        }
        let by = h - BOTTOM_H;
        for x in (0..W).step_by(25) {
            fb.blit(s, rect(179, 0, 25, 38), x, by);
        }
        fb.blit(s, rect(0, 72, 125, 38), 0, by);
        fb.blit(s, rect(126, 72, 150, 38), W - 150, by);

        if self.pressed == Some(Press::Close) && self.over_pressed {
            let r = Self::close_button();
            fb.blit(s, rect(52, 42, 9, 9), r.x, r.y);
        }
        if self.pressed == Some(Press::Shade) && self.over_pressed {
            let r = Self::shade_button();
            fb.blit(s, rect(62, 42, 9, 9), r.x, r.y);
        }

        self.draw_rows(fb, skin, playlist);

        let handle = if matches!(self.pressed, Some(Press::ScrollHandle { .. })) {
            61
        } else {
            52
        };
        fb.blit(
            s,
            rect(handle, 53, HANDLE.0, HANDLE.1),
            W - 15,
            self.handle_y(playlist),
        );

        // Running time "selected/total" and the mini time display, in the skin's font.
        let text = skin.sheet("text");
        let (total, selected) = playlist.durations();
        let mut running = format!("{}/{}", format_long(selected), format_long(total));
        if font_chars(&running) > 13 {
            running = format_long(total);
        }
        let area = rect(W - 150 + 7, by + 10, 67, 6);
        draw_string(fb, text, &running, area.x, area.y, area);
        if let Some(t) = elapsed {
            let secs = t as u32;
            let mini = format!("{:>2}:{:02}", (secs / 60).min(99), secs % 60);
            let area = rect(W - 150 + 66, by + 23, 25, 6);
            draw_string(fb, text, &mini, area.x, area.y, area);
        }

        if let Some(m) = self.menu {
            let bar = m.bar();
            let bottom = self.menu_button(m);
            fb.blit(s, bar, bottom.x - 3, bottom.y + bottom.h - bar.h);
            for (i, (_, sx, sy)) in m.items().iter().enumerate() {
                let r = self.menu_item_rect(m, i);
                let sx = if self.menu_hover == Some(i) {
                    sx + 23
                } else {
                    *sx
                };
                fb.blit(s, rect(sx, *sy, 22, 18), r.x, r.y);
            }
        }
    }

    fn draw_rows(&self, fb: &mut Framebuffer, skin: &Skin, playlist: &Playlist) {
        let colors = skin.playlist_colors;
        let area = self.list_area();
        fb.fill(area, colors.normal_bg);
        let digits = playlist.len().max(1).to_string().len();
        for row in 0..self.visible_rows() {
            let i = self.scroll + row;
            let Some(entry) = playlist.entries.get(i) else {
                break;
            };
            let y = TOP_H + 3 + row as i32 * ROW_H;
            let row_rect = rect(area.x, y, area.w, ROW_H);
            if entry.selected {
                fb.fill(row_rect, colors.selected_bg);
            }
            let color = if i == playlist.current {
                colors.current
            } else {
                colors.normal
            };

            let duration = entry.duration.map(format_short).unwrap_or_default();
            let dur_w = font::width(&duration);
            let dur_x = area.x + area.w - 3 - dur_w;
            font::draw(fb, &duration, dur_x, y + ROW_H - font::H, color, row_rect);

            let title = format!("{:>digits$}. {}", i + 1, entry.display_title());
            let title_clip = rect(area.x, y, dur_x - area.x - font::W, ROW_H);
            font::draw(
                fb,
                &title,
                area.x + 2,
                y + ROW_H - font::H,
                color,
                title_clip,
            );
        }
    }
}

fn font_chars(s: &str) -> usize {
    s.chars().count()
}

/// m:ss
fn format_short(secs: f32) -> String {
    let s = secs.round() as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

/// m:ss, or h:mm:ss past an hour.
fn format_long(secs: f32) -> String {
    let s = secs.round() as u32;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format_short(secs)
    }
}
