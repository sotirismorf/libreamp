//! The album art panel, framed with the skin's generic window graphics (gen.bmp), as classic
//! Winamp did for plugin windows. The art itself is drawn by the caller over `inner()`, at
//! full screen resolution.

use crate::font;
use crate::gfx::{Framebuffer, Rect, rect};
use crate::skin::{Sheet, Skin};

pub const W: i32 = 275;
const TOP_H: i32 = 20;
const BOTTOM_H: i32 = 14;
const TILE_H: i32 = 29;
const LEFT_W: i32 = 11;
const RIGHT_W: i32 = 8;
pub const MIN_TILES: u32 = 2;
/// 8 tiles make a 256 x 232 area for the art.
pub const DEFAULT_TILES: u32 = 8;
pub const MAX_TILES: u32 = 30;

const CLOSE: Rect = rect(W - 11, 3, 9, 9);
const TITLE: &str = "ALBUM ART";
/// Space between the title and the bars either side of it.
const TITLE_PAD: i32 = 4;
const TITLE_Y: i32 = 4;

// gen.bmp. Title bar pieces are 25 x 20, active at y 0 and inactive at y 21.
const TOP_LEFT: i32 = 0;
const TOP_LEFT_END: i32 = 26;
const TOP_CENTER_FILL: i32 = 52;
const TOP_RIGHT_END: i32 = 78;
const TOP_FILL: i32 = 104;
const TOP_RIGHT: i32 = 130;
const BOTTOM_LEFT: Rect = rect(0, 42, 125, 14);
const BOTTOM_RIGHT: Rect = rect(0, 57, 125, 14);
const BOTTOM_FILL: Rect = rect(127, 72, 25, 14);
const MIDDLE_LEFT: Rect = rect(127, 42, LEFT_W, TILE_H);
const MIDDLE_RIGHT: Rect = rect(139, 42, RIGHT_W, TILE_H);
const CLOSE_PRESSED: Rect = rect(148, 42, 9, 9);
/// The title font: A to Z, 7 px tall, separated by marker-coloured columns.
const LETTERS_ACTIVE_Y: i32 = 88;
const LETTERS_INACTIVE_Y: i32 = 96;
const LETTER_H: i32 = 7;
const SPACE_W: i32 = 5;

#[derive(Debug)]
pub enum ArtCommand {
    Close,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Press {
    Close,
    Resize { from_bottom: i32 },
}

pub struct ArtWindow {
    pub open: bool,
    pub tiles: u32,
    pub focused: bool,
    pressed: Option<Press>,
    over_pressed: bool,
}

impl ArtWindow {
    pub fn new(open: bool, tiles: u32) -> Self {
        ArtWindow {
            open,
            tiles: tiles.clamp(MIN_TILES, MAX_TILES),
            focused: true,
            pressed: None,
            over_pressed: false,
        }
    }

    pub fn height(&self) -> i32 {
        TOP_H + self.tiles as i32 * TILE_H + BOTTOM_H
    }

    /// Where the art goes, relative to the panel's top-left.
    pub fn inner(&self) -> Rect {
        rect(
            LEFT_W,
            TOP_H,
            W - LEFT_W - RIGHT_W,
            self.tiles as i32 * TILE_H,
        )
    }

    fn resize_grip(&self) -> Rect {
        rect(W - 20, self.height() - 20, 20, 20)
    }

    /// True where dragging should move the window.
    pub fn is_draggable(x: i32, y: i32) -> bool {
        rect(0, 0, W, TOP_H).contains(x, y) && !CLOSE.contains(x, y)
    }

    pub fn mouse_down(&mut self, x: i32, y: i32) {
        self.pressed = if CLOSE.contains(x, y) {
            Some(Press::Close)
        } else if self.resize_grip().contains(x, y) {
            Some(Press::Resize {
                from_bottom: self.height() - y,
            })
        } else {
            None
        };
        self.over_pressed = true;
    }

    pub fn mouse_move(&mut self, x: i32, y: i32) {
        match self.pressed {
            Some(Press::Resize { from_bottom }) => {
                let inner = y + from_bottom - TOP_H - BOTTOM_H;
                let tiles = (inner as f32 / TILE_H as f32).round().max(0.0) as u32;
                self.tiles = tiles.clamp(MIN_TILES, MAX_TILES);
            }
            Some(Press::Close) => self.over_pressed = CLOSE.contains(x, y),
            None => {}
        }
    }

    pub fn mouse_up(&mut self, x: i32, y: i32) -> Option<ArtCommand> {
        match self.pressed.take()? {
            Press::Close if CLOSE.contains(x, y) => Some(ArtCommand::Close),
            _ => None,
        }
    }

    /// Draws the frame, and a note in the art area when there is no art.
    pub fn draw(&self, fb: &mut Framebuffer, skin: &Skin, has_art: bool) {
        let h = self.height();
        fb.resize(W, h);
        let s = skin.sheet("gen");
        let sy = if self.focused { 0 } else { 21 };
        let piece = |x| rect(x, sy, 25, TOP_H);

        // Title bar: bars either side of the title, which sits on plain fill.
        for x in (0..W).step_by(25) {
            fb.blit(s, piece(TOP_FILL), x, 0);
        }
        let letters = letters(s);
        let title_w = letters.as_ref().map_or(0, |l| text_width(l, TITLE));
        let cw = title_w + 2 * TITLE_PAD;
        let cx = (W - cw) / 2;
        let centre = rect(cx, 0, cw, TOP_H);
        for x in (cx..cx + cw).step_by(25) {
            fb.blit_clipped(s, piece(TOP_CENTER_FILL), x, 0, centre);
        }
        fb.blit(s, piece(TOP_LEFT_END), cx - 25, 0);
        fb.blit(s, piece(TOP_RIGHT_END), cx + cw, 0);
        fb.blit(s, piece(TOP_LEFT), 0, 0);
        fb.blit(s, piece(TOP_RIGHT), W - 25, 0);
        if self.pressed == Some(Press::Close) && self.over_pressed {
            fb.blit(s, CLOSE_PRESSED, CLOSE.x, CLOSE.y);
        }
        if let Some(letters) = &letters {
            let y = if self.focused {
                LETTERS_ACTIVE_Y
            } else {
                LETTERS_INACTIVE_Y
            };
            draw_text(fb, s, letters, TITLE, cx + TITLE_PAD, TITLE_Y, y);
        }

        // Sides and bottom.
        let middle = rect(0, TOP_H, W, h - TOP_H - BOTTOM_H);
        for y in (TOP_H..h - BOTTOM_H).step_by(TILE_H as usize) {
            fb.blit_clipped(s, MIDDLE_LEFT, 0, y, middle);
            fb.blit_clipped(s, MIDDLE_RIGHT, W - RIGHT_W, y, middle);
        }
        for x in (0..W).step_by(BOTTOM_FILL.w as usize) {
            fb.blit(s, BOTTOM_FILL, x, h - BOTTOM_H);
        }
        fb.blit(s, BOTTOM_LEFT, 0, h - BOTTOM_H);
        fb.blit(s, BOTTOM_RIGHT, W - BOTTOM_RIGHT.w, h - BOTTOM_H);

        let colors = skin.playlist_colors;
        let inner = self.inner();
        fb.fill(inner, colors.normal_bg);
        if !has_art {
            let note = "no album art";
            let x = inner.x + (inner.w - font::width(note)) / 2;
            let y = inner.y + (inner.h - font::H) / 2;
            font::draw(fb, note, x, y, colors.normal, inner);
        }
    }
}

/// Columns (x, width) of A to Z in gen.bmp's title font, found from the marker columns that
/// separate them; None if the sheet doesn't have the font.
fn letters(s: &Sheet) -> Option<Vec<(i32, i32)>> {
    let row = LETTERS_ACTIVE_Y + 2;
    let marker = s.pixel(0, row)?;
    let mut bounds: Vec<i32> = (0..s.w as i32)
        .filter(|x| s.pixel(*x, row) == Some(marker))
        .collect();
    // The last letter may run on to the sheet's edge without a closing marker; it ends where
    // the unused (black) part of the sheet begins.
    if bounds.len() == 26 {
        let blank = |x: i32| (0..LETTER_H).all(|dy| s.pixel(x, LETTERS_ACTIVE_Y + dy) == Some(0));
        let start = bounds[25] + 1;
        let end = (start..s.w as i32)
            .find(|x| blank(*x))
            .unwrap_or(s.w as i32);
        bounds.push(end);
    }
    if bounds.len() < 27 {
        return None;
    }
    Some(
        bounds
            .windows(2)
            .take(26)
            .map(|b| (b[0] + 1, b[1] - b[0] - 1))
            .collect(),
    )
}

fn text_width(letters: &[(i32, i32)], text: &str) -> i32 {
    text.chars()
        .map(|c| match c {
            'A'..='Z' => letters[(c as u8 - b'A') as usize].1,
            _ => SPACE_W,
        })
        .sum()
}

fn draw_text(
    fb: &mut Framebuffer,
    s: &Sheet,
    letters: &[(i32, i32)],
    text: &str,
    x: i32,
    y: i32,
    row: i32,
) {
    let mut x = x;
    for c in text.chars() {
        if let 'A'..='Z' = c {
            let (lx, lw) = letters[(c as u8 - b'A') as usize];
            fb.blit(s, rect(lx, row, lw, LETTER_H), x, y);
            x += lw;
        } else {
            x += SPACE_W;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_title_font() {
        let skin = Skin::base();
        let l = letters(skin.sheet("gen")).expect("base skin has the gen font");
        assert_eq!(l.len(), 26);
        assert!(l.iter().all(|(_, w)| (3..=10).contains(w)), "{l:?}");
    }
}
