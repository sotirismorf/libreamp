//! The equalizer window (eqmain.bmp).

use crate::gfx::{Framebuffer, Rect, rect};
use crate::skin::Skin;

pub const H: i32 = 116;
pub const W: i32 = 275;

/// x of the preamp slider followed by the 10 band sliders.
const SLIDER_X: [i32; 11] = [21, 78, 96, 114, 132, 150, 168, 186, 204, 222, 240];
const SLIDER_Y: i32 = 38;
const SLIDER_W: i32 = 14;
/// The travel of a slider: 62 px track minus an 11 px thumb.
const SLIDER_TRAVEL: i32 = 51;
const THUMB: i32 = 11;

const CLOSE: Rect = rect(264, 3, 9, 9);
const SHADE: Rect = rect(254, 3, 9, 9);
const ON: Rect = rect(14, 18, 26, 12);
const AUTO: Rect = rect(40, 18, 32, 12);
const PRESETS: Rect = rect(217, 18, 44, 12);
const PLUS_12: Rect = rect(45, 36, 22, 8);
const ZERO: Rect = rect(45, 64, 22, 8);
const MINUS_12: Rect = rect(45, 95, 22, 8);
const GRAPH: Rect = rect(86, 17, 113, 19);

#[derive(Debug)]
pub enum EqCommand {
    /// Settings changed; push them to the audio engine.
    Changed,
    Close,
    /// Open the presets menu at this point in window coordinates.
    Presets {
        x: i32,
        y: i32,
    },
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Press {
    Close,
    Shade,
    On,
    Auto,
    Presets,
    Plus12,
    Zero,
    Minus12,
    /// 0 is the preamp, 1..=10 the bands.
    Slider(usize),
}

pub struct EqWindow {
    pub focused: bool,
    pub on: bool,
    /// Slider positions, 0 bottom (-12 dB) .. 1 top (+12 dB).
    pub preamp: f32,
    pub bands: [f32; 10],
    pressed: Option<Press>,
    over_pressed: bool,
}

impl EqWindow {
    pub fn new() -> Self {
        EqWindow {
            focused: true,
            on: false,
            preamp: 0.5,
            bands: [0.5; 10],
            pressed: None,
            over_pressed: false,
        }
    }

    /// True where dragging should move the window.
    pub fn is_draggable(x: i32, y: i32) -> bool {
        rect(0, 0, W, 14).contains(x, y) && !CLOSE.contains(x, y) && !SHADE.contains(x, y)
    }

    fn slider_rect(i: usize) -> Rect {
        rect(SLIDER_X[i], SLIDER_Y, SLIDER_W, 63)
    }

    fn slider_value(y: i32) -> f32 {
        let pos = y - SLIDER_Y - THUMB / 2;
        1.0 - (pos as f32 / SLIDER_TRAVEL as f32).clamp(0.0, 1.0)
    }

    fn set_slider(&mut self, i: usize, v: f32) {
        if i == 0 {
            self.preamp = v;
        } else {
            self.bands[i - 1] = v;
        }
    }

    fn slider(&self, i: usize) -> f32 {
        if i == 0 {
            self.preamp
        } else {
            self.bands[i - 1]
        }
    }

    fn button_rect(p: Press) -> Option<Rect> {
        Some(match p {
            Press::Close => CLOSE,
            Press::Shade => SHADE,
            Press::On => ON,
            Press::Auto => AUTO,
            Press::Presets => PRESETS,
            Press::Plus12 => PLUS_12,
            Press::Zero => ZERO,
            Press::Minus12 => MINUS_12,
            Press::Slider(_) => return None,
        })
    }

    pub fn mouse_down(&mut self, x: i32, y: i32) -> Option<EqCommand> {
        const BUTTONS: [Press; 8] = [
            Press::Close,
            Press::Shade,
            Press::On,
            Press::Auto,
            Press::Presets,
            Press::Plus12,
            Press::Zero,
            Press::Minus12,
        ];
        let press = BUTTONS
            .into_iter()
            .find(|p| Self::button_rect(*p).is_some_and(|r| r.contains(x, y)))
            .or_else(|| {
                (0..11)
                    .find(|i| Self::slider_rect(*i).contains(x, y))
                    .map(Press::Slider)
            })?;
        self.pressed = Some(press);
        self.over_pressed = true;
        if let Press::Slider(i) = press {
            self.set_slider(i, Self::slider_value(y));
            return Some(EqCommand::Changed);
        }
        None
    }

    pub fn mouse_move(&mut self, x: i32, y: i32) -> Option<EqCommand> {
        match self.pressed? {
            Press::Slider(i) => {
                // Dragging sideways across the bands sweeps them, like Winamp.
                let i = if i > 0 {
                    let col = (x - SLIDER_X[1] + 2) / (SLIDER_X[2] - SLIDER_X[1]);
                    col.clamp(0, 9) as usize + 1
                } else {
                    0
                };
                self.pressed = Some(Press::Slider(i));
                self.set_slider(i, Self::slider_value(y));
                Some(EqCommand::Changed)
            }
            p => {
                self.over_pressed = Self::button_rect(p).is_some_and(|r| r.contains(x, y));
                None
            }
        }
    }

    pub fn mouse_up(&mut self, x: i32, y: i32) -> Option<EqCommand> {
        let press = self.pressed.take()?;
        if !Self::button_rect(press).is_some_and(|r| r.contains(x, y)) {
            return None;
        }
        let all = |eq: &mut Self, v| {
            eq.bands = [v; 10];
            Some(EqCommand::Changed)
        };
        match press {
            Press::Close => Some(EqCommand::Close),
            Press::On => {
                self.on = !self.on;
                Some(EqCommand::Changed)
            }
            Press::Presets => Some(EqCommand::Presets {
                x: PRESETS.x,
                y: PRESETS.y + PRESETS.h,
            }),
            Press::Plus12 => all(self, 1.0),
            Press::Zero => all(self, 0.5),
            Press::Minus12 => all(self, 0.0),
            // Shade mode and auto-loading presets per file aren't implemented.
            Press::Shade | Press::Auto | Press::Slider(_) => None,
        }
    }

    /// Mouse wheel over a slider nudges it by half a dB per notch.
    pub fn wheel(&mut self, x: i32, y: i32, notches: i32) -> Option<EqCommand> {
        let i = (0..11).find(|i| Self::slider_rect(*i).contains(x, y))?;
        let v = self.slider(i) + notches as f32 / 48.0;
        self.set_slider(i, v.clamp(0.0, 1.0));
        Some(EqCommand::Changed)
    }

    fn is_down(&self, p: Press) -> bool {
        self.pressed == Some(p) && self.over_pressed
    }

    pub fn draw(&self, fb: &mut Framebuffer, skin: &Skin) {
        fb.resize(W, H);
        let s = skin.sheet("eqmain");
        fb.blit(s, rect(0, 0, W, H), 0, 0);
        fb.blit(
            s,
            rect(0, if self.focused { 134 } else { 149 }, W, 14),
            0,
            0,
        );
        let close_y = if self.is_down(Press::Close) { 125 } else { 116 };
        fb.blit(s, rect(0, close_y, 9, 9), CLOSE.x, CLOSE.y);

        // ON and AUTO: normal, pressed, lit, lit and pressed columns.
        let on_x = match (self.on, self.is_down(Press::On)) {
            (false, false) => 10,
            (false, true) => 128,
            (true, false) => 69,
            (true, true) => 187,
        };
        fb.blit(s, rect(on_x, 119, ON.w, ON.h), ON.x, ON.y);
        let auto_x = if self.is_down(Press::Auto) { 154 } else { 36 };
        fb.blit(s, rect(auto_x, 119, AUTO.w, AUTO.h), AUTO.x, AUTO.y);
        let presets_y = if self.is_down(Press::Presets) {
            176
        } else {
            164
        };
        fb.blit(
            s,
            rect(224, presets_y, PRESETS.w, PRESETS.h),
            PRESETS.x,
            PRESETS.y,
        );

        for i in 0..11 {
            let v = self.slider(i);
            let frame = (v * 27.0).round() as i32;
            let src = rect(13 + frame % 14 * 15, 164 + frame / 14 * 65, SLIDER_W, 63);
            let r = Self::slider_rect(i);
            fb.blit(s, src, r.x, r.y);
            let thumb_y = if self.is_down(Press::Slider(i)) {
                176
            } else {
                164
            };
            let ty = r.y + ((1.0 - v) * SLIDER_TRAVEL as f32).round() as i32;
            fb.blit(s, rect(0, thumb_y, THUMB, THUMB), r.x + 1, ty);
        }

        self.draw_graph(fb, skin);
    }

    /// The little response curve: a spline through the bands, coloured per row by the skin.
    fn draw_graph(&self, fb: &mut Framebuffer, skin: &Skin) {
        let s = skin.sheet("eqmain");
        fb.blit(s, rect(0, 294, GRAPH.w, GRAPH.h), GRAPH.x, GRAPH.y);
        let max_y = (GRAPH.h - 1) as f32;
        let preamp_y = ((1.0 - self.preamp) * max_y).round() as i32;
        fb.blit(s, rect(0, 314, GRAPH.w, 1), GRAPH.x, GRAPH.y + preamp_y);

        let ys: Vec<f32> = self.bands.iter().map(|v| (1.0 - v) * max_y).collect();
        let curve = natural_spline(&ys, 12);
        let mut last = curve[0].round() as i32;
        for (x, y) in curve.iter().enumerate() {
            let y = (y.round() as i32).clamp(0, GRAPH.h - 1);
            for row in y.min(last)..=y.max(last) {
                // One-pixel column of per-row colours at (115, 294).
                if let Some(rgb) = s.pixel(115, 294 + row) {
                    fb.plot(GRAPH.x + 2 + x as i32, GRAPH.y + row, rgb, GRAPH);
                }
            }
            last = y;
        }
    }
}

/// Natural cubic spline through points spaced `step` apart, sampled at every integer x.
fn natural_spline(ys: &[f32], step: usize) -> Vec<f32> {
    let n = ys.len();
    let h = step as f32;
    // Second derivatives from the tridiagonal system (Thomas algorithm); ends are zero.
    let mut m = vec![0.0f32; n];
    if n > 2 {
        let mut c = vec![0.0f32; n];
        let mut d = vec![0.0f32; n];
        for i in 1..n - 1 {
            let rhs = 6.0 * (ys[i + 1] - 2.0 * ys[i] + ys[i - 1]) / (h * h);
            let denom = 4.0 - c[i - 1];
            c[i] = 1.0 / denom;
            d[i] = (rhs - d[i - 1]) / denom;
        }
        for i in (1..n - 1).rev() {
            m[i] = d[i] - c[i] * m[i + 1];
        }
    }
    let mut out = Vec::with_capacity((n - 1) * step + 1);
    for i in 0..n - 1 {
        for k in 0..step {
            let t = k as f32 / h;
            let (a, b) = (1.0 - t, t);
            let y = a * ys[i]
                + b * ys[i + 1]
                + ((a * a * a - a) * m[i] + (b * b * b - b) * m[i + 1]) * h * h / 6.0;
            out.push(y);
        }
    }
    out.push(ys[n - 1]);
    out
}

#[cfg(test)]
mod tests {
    use super::natural_spline;

    #[test]
    fn spline_passes_through_points_and_is_flat_for_flat_input() {
        let ys = [3.0, 9.0, 1.0, 4.0];
        let curve = natural_spline(&ys, 12);
        assert_eq!(curve.len(), 37);
        for (i, y) in ys.iter().enumerate() {
            assert!((curve[i * 12] - y).abs() < 1e-4);
        }
        assert!(
            natural_spline(&[5.0; 10], 12)
                .iter()
                .all(|y| (y - 5.0).abs() < 1e-4)
        );
    }
}
