//! The classic main window: drawing and mouse interaction, in skin (1x) coordinates.

use crate::gfx::{Framebuffer, Rect, rect};
use crate::skin::Skin;
use crate::sprites::*;
use crate::ui::draw_string;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Widget {
    Options,
    Minimize,
    Shade,
    Close,
    Prev,
    Play,
    Pause,
    Stop,
    Next,
    Eject,
    Repeat,
    Shuffle,
    Eq,
    Pl,
    Position,
    Volume,
    Balance,
    Vis,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlayState {
    Playing,
    Paused,
    Stopped,
}

#[derive(Clone, Copy, Debug)]
pub enum Action {
    Close,
    Minimize,
    Prev,
    Play,
    Pause,
    Stop,
    Next,
    Eject,
    Seek(f32),
    Volume(f32),
    Balance(f32),
    CycleVis,
}

const BUTTONS: &[(Widget, &Button)] = &[
    (Widget::Options, &BTN_OPTIONS),
    (Widget::Minimize, &BTN_MINIMIZE),
    (Widget::Shade, &BTN_SHADE),
    (Widget::Close, &BTN_CLOSE),
    (Widget::Prev, &BTN_PREV),
    (Widget::Play, &BTN_PLAY),
    (Widget::Pause, &BTN_PAUSE),
    (Widget::Stop, &BTN_STOP),
    (Widget::Next, &BTN_NEXT),
    (Widget::Eject, &BTN_EJECT),
];

const TOGGLES: &[(Widget, &Toggle)] = &[
    (Widget::Repeat, &TGL_REPEAT),
    (Widget::Shuffle, &TGL_SHUFFLE),
    (Widget::Eq, &TGL_EQ),
    (Widget::Pl, &TGL_PL),
];

pub struct MainWindow {
    pub focused: bool,
    pub state: PlayState,
    pub title: String,
    pub kbps: Option<u32>,
    pub khz: Option<u32>,
    pub channels: u32,
    /// Elapsed and total track time in seconds.
    pub elapsed: f32,
    pub duration: Option<f32>,
    pub volume: f32,
    pub balance: f32,
    pub repeat: bool,
    pub shuffle: bool,
    pub eq: bool,
    pub pl: bool,
    marquee_offset: i32,
    /// Widget under an active mouse press, and whether the pointer is still over it.
    pressed: Option<Widget>,
    over_pressed: bool,
    /// Seek position while dragging the position bar.
    seek_drag: Option<f32>,
}

impl MainWindow {
    pub fn new() -> Self {
        MainWindow {
            focused: true,
            state: PlayState::Stopped,
            title: "LibreAmp - drop a file here".into(),
            kbps: None,
            khz: None,
            channels: 0,
            elapsed: 0.0,
            duration: None,
            volume: 0.75,
            balance: 0.0,
            repeat: false,
            shuffle: false,
            eq: false,
            pl: false,
            marquee_offset: 0,
            pressed: None,
            over_pressed: false,
            seek_drag: None,
        }
    }

    /// True where dragging should move the window (title bar outside its buttons).
    pub fn is_draggable(x: i32, y: i32) -> bool {
        rect(0, 0, MAIN_W, 14).contains(x, y)
            && ![&BTN_OPTIONS, &BTN_MINIMIZE, &BTN_SHADE, &BTN_CLOSE]
                .iter()
                .any(|b| b.at.contains(x, y))
    }

    /// Advances animations; call at a fixed rate (~30 Hz).
    pub fn tick(&mut self) {
        self.marquee_offset += 1;
    }

    fn widget_at(x: i32, y: i32) -> Option<Widget> {
        BUTTONS
            .iter()
            .map(|(w, b)| (*w, b.at))
            .chain(TOGGLES.iter().map(|(w, t)| (*w, t.at)))
            .chain([
                (Widget::Position, POS_AT),
                (Widget::Volume, VOL_AT),
                (Widget::Balance, BAL_AT),
                (Widget::Vis, crate::vis::AREA),
            ])
            .find(|(_, r)| r.contains(x, y))
            .map(|(w, _)| w)
    }

    fn widget_rect(w: Widget) -> Rect {
        match w {
            Widget::Position => POS_AT,
            Widget::Volume => VOL_AT,
            Widget::Balance => BAL_AT,
            Widget::Vis => crate::vis::AREA,
            _ => BUTTONS
                .iter()
                .find(|(b, _)| *b == w)
                .map(|(_, b)| b.at)
                .or_else(|| TOGGLES.iter().find(|(t, _)| *t == w).map(|(_, t)| t.at))
                .expect("every widget has a rect"),
        }
    }

    /// Maps x inside a slider to 0..1, keeping the thumb centred under the pointer.
    fn slider_value(r: Rect, thumb_w: i32, x: i32) -> f32 {
        let v = (x - r.x - thumb_w / 2) as f32 / (r.w - thumb_w) as f32;
        v.clamp(0.0, 1.0)
    }

    fn drag(&mut self, w: Widget, x: i32) -> Option<Action> {
        match w {
            Widget::Position if self.duration.is_some() => {
                self.seek_drag = Some(Self::slider_value(POS_AT, POS_THUMB[0].w, x));
                None
            }
            Widget::Volume => {
                self.volume = Self::slider_value(VOL_AT, SLIDER_THUMB_W, x);
                Some(Action::Volume(self.volume))
            }
            Widget::Balance => {
                let v = Self::slider_value(BAL_AT, SLIDER_THUMB_W, x) * 2.0 - 1.0;
                // Snap to centre like Winamp does.
                self.balance = if v.abs() < 0.08 { 0.0 } else { v };
                Some(Action::Balance(self.balance))
            }
            _ => None,
        }
    }

    pub fn mouse_down(&mut self, x: i32, y: i32) -> Option<Action> {
        let w = Self::widget_at(x, y)?;
        self.pressed = Some(w);
        self.over_pressed = true;
        self.drag(w, x)
    }

    pub fn mouse_move(&mut self, x: i32, y: i32) -> Option<Action> {
        let w = self.pressed?;
        self.over_pressed = Self::widget_rect(w).contains(x, y);
        self.drag(w, x)
    }

    pub fn mouse_up(&mut self, x: i32, y: i32) -> Option<Action> {
        let w = self.pressed.take()?;
        if w == Widget::Position {
            return self.seek_drag.take().map(Action::Seek);
        }
        if !Self::widget_rect(w).contains(x, y) {
            return None;
        }
        match w {
            Widget::Close => Some(Action::Close),
            Widget::Minimize => Some(Action::Minimize),
            Widget::Prev => Some(Action::Prev),
            Widget::Play => Some(Action::Play),
            Widget::Pause => Some(Action::Pause),
            Widget::Stop => Some(Action::Stop),
            Widget::Next => Some(Action::Next),
            Widget::Eject => Some(Action::Eject),
            Widget::Vis => Some(Action::CycleVis),
            Widget::Repeat => {
                self.repeat = !self.repeat;
                None
            }
            Widget::Shuffle => {
                self.shuffle = !self.shuffle;
                None
            }
            Widget::Eq => {
                self.eq = !self.eq;
                None
            }
            Widget::Pl => {
                self.pl = !self.pl;
                None
            }
            Widget::Options
            | Widget::Shade
            | Widget::Position
            | Widget::Volume
            | Widget::Balance => None,
        }
    }

    fn is_down(&self, w: Widget) -> bool {
        self.pressed == Some(w) && self.over_pressed
    }

    pub fn draw(&self, fb: &mut Framebuffer, skin: &Skin) {
        fb.blit(skin.sheet("main"), rect(0, 0, MAIN_W, MAIN_H), 0, 0);

        let titlebar = skin.sheet("titlebar");
        let title = if self.focused {
            TITLE_ACTIVE
        } else {
            TITLE_INACTIVE
        };
        fb.blit(titlebar, title, 0, 0);

        for (w, b) in BUTTONS {
            let src = if self.is_down(*w) {
                b.pressed
            } else {
                b.normal
            };
            fb.blit(skin.sheet(b.sheet), src, b.at.x, b.at.y);
        }

        let shufrep = skin.sheet("shufrep");
        for (w, t) in TOGGLES {
            let on = match w {
                Widget::Repeat => self.repeat,
                Widget::Shuffle => self.shuffle,
                Widget::Eq => self.eq,
                _ => self.pl,
            };
            let src = t.sprites[on as usize * 2 + self.is_down(*w) as usize];
            fb.blit(shufrep, src, t.at.x, t.at.y);
        }

        let playpaus = skin.sheet("playpaus");
        let ps = PLAYSTATE[match self.state {
            PlayState::Playing => 0,
            PlayState::Paused => 1,
            PlayState::Stopped => 2,
        }];
        fb.blit(playpaus, ps, PLAYSTATE_AT.0, PLAYSTATE_AT.1);

        let monoster = skin.sheet("monoster");
        fb.blit(
            monoster,
            STEREO[(self.channels < 2) as usize],
            STEREO_AT.0,
            STEREO_AT.1,
        );
        fb.blit(
            monoster,
            MONO[(self.channels != 1) as usize],
            MONO_AT.0,
            MONO_AT.1,
        );

        self.draw_time(fb, skin);
        self.draw_text(fb, skin);
        self.draw_sliders(fb, skin);
    }

    fn draw_time(&self, fb: &mut Framebuffer, skin: &Skin) {
        if self.state == PlayState::Stopped {
            return;
        }
        // Prefer nums_ex.bmp when the skin has it.
        let nums = match skin.sheet("nums_ex") {
            s if s.w > 0 => s,
            _ => skin.sheet("numbers"),
        };
        let secs = self.elapsed as u32;
        let (m, s) = ((secs / 60).min(99), secs % 60);
        for (x, d) in TIME_DIGITS_X.iter().zip([m / 10, m % 10, s / 10, s % 10]) {
            fb.blit(
                nums,
                rect(d as i32 * DIGIT_W, 0, DIGIT_W, DIGIT_H),
                *x,
                TIME_Y,
            );
        }
    }

    fn draw_text(&self, fb: &mut Framebuffer, skin: &Skin) {
        let text = skin.sheet("text");
        let title = self.title.to_lowercase();
        let text_w = title.chars().count() as i32 * GLYPH_W;
        if text_w <= MARQUEE.w {
            draw_string(fb, text, &title, MARQUEE.x, MARQUEE.y, MARQUEE);
        } else {
            let looped = format!("{title}  ***  ");
            let loop_w = looped.chars().count() as i32 * GLYPH_W;
            let x = MARQUEE.x - self.marquee_offset.rem_euclid(loop_w);
            draw_string(fb, text, &looped, x, MARQUEE.y, MARQUEE);
            draw_string(fb, text, &looped, x + loop_w, MARQUEE.y, MARQUEE);
        }
        if let Some(kbps) = self.kbps {
            let s = format!("{:>3}", kbps.min(999));
            draw_string(fb, text, &s, KBPS.x, KBPS.y, KBPS);
        }
        if let Some(khz) = self.khz {
            let s = format!("{:>2}", khz.min(99));
            draw_string(fb, text, &s, KHZ.x, KHZ.y, KHZ);
        }
    }

    fn draw_sliders(&self, fb: &mut Framebuffer, skin: &Skin) {
        let volume = skin.sheet("volume");
        let vi = (self.volume * 27.0).round() as i32;
        fb.blit(
            volume,
            rect(0, vi * 15, VOL_AT.w, VOL_AT.h),
            VOL_AT.x,
            VOL_AT.y,
        );
        let tx = VOL_AT.x + ((VOL_AT.w - SLIDER_THUMB_W) as f32 * self.volume).round() as i32;
        let thumb = VOL_THUMB[self.is_down(Widget::Volume) as usize];
        fb.blit(volume, thumb, tx, VOL_AT.y + 1);

        let balance = skin.sheet("balance");
        let bi = (self.balance.abs() * 27.0).round() as i32;
        fb.blit(
            balance,
            rect(9, bi * 15, BAL_AT.w, BAL_AT.h),
            BAL_AT.x,
            BAL_AT.y,
        );
        let frac = (self.balance + 1.0) / 2.0;
        let tx = BAL_AT.x + ((BAL_AT.w - SLIDER_THUMB_W) as f32 * frac).round() as i32;
        let thumb = BAL_THUMB[self.is_down(Widget::Balance) as usize];
        fb.blit(balance, thumb, tx, BAL_AT.y + 1);

        if let Some(duration) = self.duration {
            let posbar = skin.sheet("posbar");
            fb.blit(posbar, POS_BG, POS_AT.x, POS_AT.y);
            let frac = self.seek_drag.unwrap_or(if duration > 0.0 {
                self.elapsed / duration
            } else {
                0.0
            });
            let tw = POS_THUMB[0].w;
            let tx = POS_AT.x + ((POS_AT.w - tw) as f32 * frac.clamp(0.0, 1.0)).round() as i32;
            let thumb = POS_THUMB[self.is_down(Widget::Position) as usize];
            fb.blit(posbar, thumb, tx, POS_AT.y);
        }
    }
}
