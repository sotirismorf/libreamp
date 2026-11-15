//! The classic main-window visualizer: spectrum analyzer or oscilloscope, in skin colours.

use crate::gfx::{Framebuffer, Rect, rect};

pub const AREA: Rect = rect(24, 43, 76, 16);
/// Samples the audio thread keeps for the visualizer (a power of two for the FFT).
pub const WINDOW: usize = 1024;
const BARS: usize = 19;
const BAR_W: i32 = 3;
/// Height lost per frame (at ~30 fps) by bars and peaks.
const BAR_FALL: f32 = 1.1;
const PEAK_FALL: f32 = 0.35;
/// Frames a peak dot holds before falling.
const PEAK_HOLD: u8 = 12;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mode {
    #[default]
    Spectrum,
    Oscilloscope,
    Off,
}

impl Mode {
    pub fn next(self) -> Mode {
        match self {
            Mode::Spectrum => Mode::Oscilloscope,
            Mode::Oscilloscope => Mode::Off,
            Mode::Off => Mode::Spectrum,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Mode::Spectrum => "spectrum",
            Mode::Oscilloscope => "oscilloscope",
            Mode::Off => "off",
        }
    }

    pub fn from_name(name: &str) -> Option<Mode> {
        [Mode::Spectrum, Mode::Oscilloscope, Mode::Off]
            .into_iter()
            .find(|m| m.name() == name)
    }
}

pub struct Visualizer {
    pub mode: Mode,
    bars: [f32; BARS],
    peaks: [f32; BARS],
    hold: [u8; BARS],
    /// The latest samples, oldest first, mono in -1..1.
    samples: Vec<f32>,
    spectrum: Vec<f32>,
    window: Vec<f32>,
}

impl Visualizer {
    pub fn new(mode: Mode) -> Self {
        let window = (0..WINDOW)
            .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / WINDOW as f32).cos())
            .collect();
        Visualizer {
            mode,
            bars: [0.0; BARS],
            peaks: [0.0; BARS],
            hold: [0; BARS],
            samples: vec![0.0; WINDOW],
            spectrum: vec![0.0; WINDOW / 2],
            window,
        }
    }

    /// Feeds the newest `WINDOW` samples (silence when not playing) and advances one frame.
    pub fn update(&mut self, samples: Option<&[f32]>, rate: u32) {
        match samples {
            Some(s) => self.samples.copy_from_slice(&s[s.len() - WINDOW..]),
            None => self.samples.fill(0.0),
        }
        let target = if self.mode == Mode::Spectrum && samples.is_some() {
            self.analyze(rate.max(1))
        } else {
            [0.0; BARS]
        };
        let bars = self
            .bars
            .iter_mut()
            .zip(&mut self.peaks)
            .zip(&mut self.hold);
        for (((bar, peak), hold), target) in bars.zip(target) {
            *bar = target.max(*bar - BAR_FALL);
            if *bar >= *peak {
                *peak = *bar;
                *hold = PEAK_HOLD;
            } else if *hold > 0 {
                *hold -= 1;
            } else {
                *peak = (*peak - PEAK_FALL).max(0.0);
            }
        }
    }

    /// Bar heights (0..16) from log-spaced bands between 40 Hz and 16 kHz.
    fn analyze(&mut self, rate: u32) -> [f32; BARS] {
        let mut re: Vec<f32> = self
            .samples
            .iter()
            .zip(&self.window)
            .map(|(s, w)| s * w)
            .collect();
        let mut im = vec![0.0f32; WINDOW];
        fft(&mut re, &mut im);
        // A full-scale sine peaks at N/4 with a Hann window.
        let norm = 4.0 / WINDOW as f32;
        for (i, m) in self.spectrum.iter_mut().enumerate() {
            *m = (re[i] * re[i] + im[i] * im[i]).sqrt() * norm;
        }

        let bin_hz = rate as f32 / WINDOW as f32;
        let (lo, hi) = (40.0f32, 16000.0f32.min(rate as f32 / 2.0));
        let mut out = [0.0; BARS];
        for (i, bar) in out.iter_mut().enumerate() {
            let f0 = lo * (hi / lo).powf(i as f32 / BARS as f32);
            let f1 = lo * (hi / lo).powf((i + 1) as f32 / BARS as f32);
            let b0 = ((f0 / bin_hz) as usize).clamp(1, WINDOW / 2 - 1);
            let b1 = ((f1 / bin_hz).ceil() as usize).clamp(b0 + 1, WINDOW / 2);
            let peak = self.spectrum[b0..b1].iter().fold(0.0f32, |a, m| a.max(*m));
            // Music falls off towards the treble; tilt by 3 dB per octave so bars look even.
            let centre = (f0 * f1).sqrt();
            let db = 20.0 * peak.max(1e-6).log10() + 3.0 * (centre / 1000.0).log2();
            *bar = ((db + 54.0) / 54.0 * AREA.h as f32).clamp(0.0, AREA.h as f32);
        }
        out
    }

    /// Draws into the main window's framebuffer. `colors` is viscolor.txt's 24 entries.
    pub fn draw(&self, fb: &mut Framebuffer, colors: &[u32; 24]) {
        if self.mode == Mode::Off {
            return;
        }
        fb.fill(AREA, colors[0]);
        for y in (1..AREA.h).step_by(2) {
            for x in (1..AREA.w).step_by(2) {
                fb.plot(AREA.x + x, AREA.y + y, colors[1], AREA);
            }
        }
        match self.mode {
            Mode::Spectrum => self.draw_spectrum(fb, colors),
            Mode::Oscilloscope => self.draw_oscilloscope(fb, colors),
            Mode::Off => {}
        }
    }

    fn draw_spectrum(&self, fb: &mut Framebuffer, colors: &[u32; 24]) {
        let bottom = AREA.y + AREA.h;
        for (i, (bar, peak)) in self.bars.iter().zip(&self.peaks).enumerate() {
            let x = AREA.x + i as i32 * (BAR_W + 1);
            let h = bar.round() as i32;
            // Colours 2..=17 run from the top of the area to the bottom.
            for row in bottom - h..bottom {
                let color = colors[2 + (row - AREA.y) as usize];
                fb.fill(rect(x, row, BAR_W, 1), color);
            }
            let p = peak.round() as i32;
            if p > 0 {
                fb.fill(rect(x, bottom - p, BAR_W, 1), colors[23]);
            }
        }
    }

    fn draw_oscilloscope(&self, fb: &mut Framebuffer, colors: &[u32; 24]) {
        // Colours 18..=22, brightest in the middle, fading towards the edges.
        let color_at = |y: i32| match (y - AREA.h / 2).abs() {
            0..=1 => colors[18],
            2..=3 => colors[19],
            4..=5 => colors[20],
            6 => colors[21],
            _ => colors[22],
        };
        // About 12 ms of the newest samples across the width, like Winamp's 576-sample window.
        let step = 7;
        let start = WINDOW - AREA.w as usize * step;
        let mut last = None;
        for x in 0..AREA.w {
            let s = self.samples[start + x as usize * step];
            let y = ((1.0 - s) * AREA.h as f32 / 2.0).floor() as i32;
            let y = y.clamp(0, AREA.h - 1);
            let (a, b) = last.map_or((y, y), |l: i32| (l.min(y), l.max(y)));
            for row in a..=b {
                fb.plot(AREA.x + x, AREA.y + row, color_at(row), AREA);
            }
            last = Some(y);
        }
    }
}

/// In-place iterative radix-2 FFT; `re.len()` must be a power of two.
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = -std::f32::consts::TAU / len as f32;
        let (wr, wi) = (angle.cos(), angle.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * cr - im[b] * ci;
                let ti = re[b] * ci + im[b] * cr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                (cr, ci) = (cr * wr - ci * wi, cr * wi + ci * wr);
            }
        }
        len <<= 1;
    }
}

/// Parses viscolor.txt: one "r,g,b" per line, anything after the numbers ignored.
pub fn parse_colors(text: &str) -> Option<[u32; 24]> {
    let mut colors = Vec::with_capacity(24);
    for line in text.lines() {
        let nums: Vec<u32> = line
            .split(|c: char| !c.is_ascii_digit())
            .filter(|s| !s.is_empty())
            .take(3)
            .filter_map(|s| s.parse().ok())
            .collect();
        if let [r, g, b] = nums[..] {
            colors.push((r.min(255) << 16) | (g.min(255) << 8) | b.min(255));
        }
        if colors.len() == 24 {
            break;
        }
    }
    colors.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_lights_the_right_bar() {
        let rate = 44100;
        let freq = 1000.0;
        let samples: Vec<f32> = (0..WINDOW)
            .map(|i| (std::f32::consts::TAU * freq * i as f32 / rate as f32).sin())
            .collect();
        let mut vis = Visualizer::new(Mode::Spectrum);
        vis.update(Some(&samples), rate);
        let loudest = (0..BARS)
            .max_by(|a, b| vis.bars[*a].total_cmp(&vis.bars[*b]))
            .unwrap();
        // 1 kHz sits in bar 10 of 19 log bands from 40 Hz to 16 kHz.
        assert_eq!(loudest, 10);
        assert!(vis.bars[loudest] > 14.0);
        assert!(vis.bars[0] < 4.0);
    }

    #[test]
    fn parses_viscolor_variants() {
        let mut text = String::from("0,0,0, // color 0 = black\n24,33,41 // dots\n");
        for i in 2..24 {
            text += &format!("{i}\t{i}\t{i}\n");
        }
        let colors = parse_colors(&text).unwrap();
        assert_eq!(colors[1], 0x182129);
        assert_eq!(colors[23], 0x171717);
        assert!(parse_colors("1,2,3").is_none());
    }
}
