//! Skinned windows. All coordinates are skin pixels, relative to each window's top-left.

pub mod art_window;
pub mod eq_window;
mod main_window;
pub mod menu;
pub mod playlist_window;

pub use art_window::{ArtCommand, ArtWindow};
pub use eq_window::{EqCommand, EqWindow};
pub use main_window::{Action, MainWindow, PlayState};
pub use playlist_window::{PlCommand, PlaylistWindow};

use crate::gfx::{Framebuffer, Rect, rect};
use crate::sprites::{GLYPH_H, GLYPH_W};

/// Position of `c` in text.bmp as (column, row).
fn glyph(c: char) -> (i32, i32) {
    const ROW1: &str = "0123456789….:()-'!_+\\/[]^&%,=$#";
    match c {
        'a'..='z' => (c as i32 - 'a' as i32, 0),
        '"' => (26, 0),
        '@' => (27, 0),
        'å' => (0, 2),
        'ö' => (1, 2),
        'ä' => (2, 2),
        '?' => (3, 2),
        '*' => (4, 2),
        '<' | '{' => (22, 1),
        '>' | '}' => (23, 1),
        _ => ROW1
            .chars()
            .position(|r| r == c)
            .map(|i| (i as i32, 1))
            .unwrap_or((30, 0)), // space
    }
}

pub fn draw_string(
    fb: &mut Framebuffer,
    text: &crate::skin::Sheet,
    s: &str,
    x: i32,
    y: i32,
    clip: Rect,
) {
    for (i, c) in s.chars().enumerate() {
        let gx = x + i as i32 * GLYPH_W;
        if gx >= clip.x + clip.w {
            break;
        }
        if gx + GLYPH_W <= clip.x {
            continue;
        }
        let (col, row) = glyph(c);
        let src = rect(col * GLYPH_W, row * GLYPH_H, GLYPH_W, GLYPH_H);
        fb.blit_clipped(text, src, gx, y, clip);
    }
}
