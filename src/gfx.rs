//! A 1x CPU framebuffer that skin sprites are blitted into.

use crate::skin::Sheet;

#[derive(Clone, Copy)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

pub const fn rect(x: i32, y: i32, w: i32, h: i32) -> Rect {
    Rect { x, y, w, h }
}

impl Rect {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
}

pub struct Framebuffer {
    pub w: i32,
    pub h: i32,
    pub px: Vec<u32>,
}

impl Framebuffer {
    pub fn new(w: i32, h: i32) -> Self {
        Framebuffer {
            w,
            h,
            px: vec![0xff00_0000; (w * h) as usize],
        }
    }

    /// Resizes, keeping nothing of the old contents.
    pub fn resize(&mut self, w: i32, h: i32) {
        if (w, h) != (self.w, self.h) {
            *self = Framebuffer::new(w, h);
        }
    }

    /// Fills `r`, clipped to the framebuffer, with an 0xRRGGBB colour.
    pub fn fill(&mut self, r: Rect, rgb: u32) {
        let x0 = r.x.max(0);
        let x1 = (r.x + r.w).min(self.w);
        for y in r.y.max(0)..(r.y + r.h).min(self.h) {
            let row = (y * self.w) as usize;
            if x0 < x1 {
                self.px[row + x0 as usize..row + x1 as usize].fill(0xff00_0000 | rgb);
            }
        }
    }

    /// Sets one pixel if it lies inside `clip` and the framebuffer.
    pub fn plot(&mut self, x: i32, y: i32, rgb: u32, clip: Rect) {
        if clip.contains(x, y) && x >= 0 && y >= 0 && x < self.w && y < self.h {
            self.px[(y * self.w + x) as usize] = 0xff00_0000 | rgb;
        }
    }

    /// Copies `src` from `sheet` to (dx, dy), clipped to both the sheet and the framebuffer.
    pub fn blit(&mut self, sheet: &Sheet, src: Rect, dx: i32, dy: i32) {
        self.blit_clipped(sheet, src, dx, dy, rect(0, 0, self.w, self.h));
    }

    /// Like `blit`, but also clipped to `clip` (in framebuffer coordinates).
    pub fn blit_clipped(&mut self, sheet: &Sheet, src: Rect, dx: i32, dy: i32, clip: Rect) {
        let (sw, sh) = (sheet.w as i32, sheet.h as i32);
        let x0 = dx.max(clip.x).max(0).max(dx - src.x);
        let y0 = dy.max(clip.y).max(0).max(dy - src.y);
        let x1 = (dx + src.w)
            .min(clip.x + clip.w)
            .min(self.w)
            .min(dx + sw - src.x);
        let y1 = (dy + src.h)
            .min(clip.y + clip.h)
            .min(self.h)
            .min(dy + sh - src.y);
        if x0 >= x1 {
            return;
        }
        let n = (x1 - x0) as usize;
        for y in y0..y1 {
            let s = ((src.y + y - dy) * sw + src.x + x0 - dx) as usize;
            let d = (y * self.w + x0) as usize;
            self.px[d..d + n].copy_from_slice(&sheet.px[s..s + n]);
        }
    }
}
