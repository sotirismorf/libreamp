//! Sprite source rectangles and on-window positions for classic skins.
//! Coordinates follow Webamp's skinSprites / Winamp 2.x layout.

use crate::gfx::{Rect, rect};

pub const MAIN_W: i32 = 275;
pub const MAIN_H: i32 = 116;

// titlebar.bmp
pub const TITLE_ACTIVE: Rect = rect(27, 0, 275, 14);
pub const TITLE_INACTIVE: Rect = rect(27, 15, 275, 14);

/// A two-state (normal / pressed) button: sheet, normal sprite, pressed sprite, position.
pub struct Button {
    pub sheet: &'static str,
    pub normal: Rect,
    pub pressed: Rect,
    pub at: Rect,
}

const fn button(sheet: &'static str, nx: i32, ny: i32, px: i32, py: i32, at: Rect) -> Button {
    Button {
        sheet,
        normal: rect(nx, ny, at.w, at.h),
        pressed: rect(px, py, at.w, at.h),
        at,
    }
}

pub const BTN_OPTIONS: Button = button("titlebar", 0, 0, 0, 9, rect(6, 3, 9, 9));
pub const BTN_MINIMIZE: Button = button("titlebar", 9, 0, 9, 9, rect(244, 3, 9, 9));
pub const BTN_SHADE: Button = button("titlebar", 0, 18, 9, 18, rect(254, 3, 9, 9));
pub const BTN_CLOSE: Button = button("titlebar", 18, 0, 18, 9, rect(264, 3, 9, 9));

pub const BTN_PREV: Button = button("cbuttons", 0, 0, 0, 18, rect(16, 88, 23, 18));
pub const BTN_PLAY: Button = button("cbuttons", 23, 0, 23, 18, rect(39, 88, 23, 18));
pub const BTN_PAUSE: Button = button("cbuttons", 46, 0, 46, 18, rect(62, 88, 23, 18));
pub const BTN_STOP: Button = button("cbuttons", 69, 0, 69, 18, rect(85, 88, 23, 18));
pub const BTN_NEXT: Button = button("cbuttons", 92, 0, 92, 18, rect(108, 88, 22, 18));
pub const BTN_EJECT: Button = button("cbuttons", 114, 0, 114, 16, rect(136, 89, 22, 16));

/// A toggle in shufrep.bmp: sprites are (off, off pressed, on, on pressed).
pub struct Toggle {
    pub sprites: [Rect; 4],
    pub at: Rect,
}

const fn toggle(xs: [i32; 2], ys: [i32; 4], at: Rect) -> Toggle {
    let (w, h) = (at.w, at.h);
    Toggle {
        sprites: [
            rect(xs[0], ys[0], w, h),
            rect(xs[1], ys[1], w, h),
            rect(xs[0], ys[2], w, h),
            rect(xs[1], ys[3], w, h),
        ],
        at,
    }
}

pub const TGL_REPEAT: Toggle = toggle([0, 0], [0, 15, 30, 45], rect(210, 89, 28, 15));
pub const TGL_SHUFFLE: Toggle = toggle([28, 28], [0, 15, 30, 45], rect(164, 89, 47, 15));
pub const TGL_EQ: Toggle = toggle([0, 46], [61, 61, 73, 73], rect(219, 58, 23, 12));
pub const TGL_PL: Toggle = toggle([23, 69], [61, 61, 73, 73], rect(242, 58, 23, 12));

// numbers.bmp: 9x13 digits, index 10 is blank. nums_ex.bmp adds a minus at index 11.
pub const DIGIT_W: i32 = 9;
pub const DIGIT_H: i32 = 13;
pub const TIME_DIGITS_X: [i32; 4] = [48, 60, 78, 90];
pub const TIME_Y: i32 = 26;

// text.bmp: 5x6 glyphs.
pub const GLYPH_W: i32 = 5;
pub const GLYPH_H: i32 = 6;
pub const MARQUEE: Rect = rect(111, 27, 154, 6);
pub const KBPS: Rect = rect(111, 43, 15, 6);
pub const KHZ: Rect = rect(156, 43, 10, 6);

// monoster.bmp: (active, inactive)
pub const STEREO: [Rect; 2] = [rect(0, 0, 29, 12), rect(0, 12, 29, 12)];
pub const MONO: [Rect; 2] = [rect(29, 0, 27, 12), rect(29, 12, 27, 12)];
pub const STEREO_AT: (i32, i32) = (239, 41);
pub const MONO_AT: (i32, i32) = (212, 41);

// playpaus.bmp: 9x9 playing / paused / stopped
pub const PLAYSTATE: [Rect; 3] = [rect(0, 0, 9, 9), rect(9, 0, 9, 9), rect(18, 0, 9, 9)];
pub const PLAYSTATE_AT: (i32, i32) = (26, 28);

// posbar.bmp
pub const POS_BG: Rect = rect(0, 0, 248, 10);
pub const POS_THUMB: [Rect; 2] = [rect(248, 0, 29, 10), rect(278, 0, 29, 10)];
pub const POS_AT: Rect = rect(16, 72, 248, 10);

// volume.bmp: 28 backgrounds, 15px apart; thumbs at the bottom.
pub const VOL_AT: Rect = rect(107, 57, 68, 13);
pub const VOL_THUMB: [Rect; 2] = [rect(15, 422, 14, 11), rect(0, 422, 14, 11)];
// balance.bmp: backgrounds start at x=9.
pub const BAL_AT: Rect = rect(177, 57, 38, 13);
pub const BAL_THUMB: [Rect; 2] = [rect(15, 422, 14, 11), rect(0, 422, 14, 11)];
pub const SLIDER_THUMB_W: i32 = 14;
