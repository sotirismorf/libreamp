//! Winamp-style 10-band equalizer: settings shared with the audio thread, and the filters.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

pub const BANDS_HZ: [f32; 10] = [
    60.0, 170.0, 310.0, 600.0, 1000.0, 3000.0, 6000.0, 12000.0, 14000.0, 16000.0,
];
/// Slider range is ±12 dB around the middle.
const RANGE_DB: f32 = 12.0;

/// Slider position (0 bottom .. 1 top) to decibels.
pub fn slider_db(v: f32) -> f32 {
    (v.clamp(0.0, 1.0) * 2.0 - 1.0) * RANGE_DB
}

/// Built-in Winamp presets (winamp.q1): name, preamp, bands, as 1..64 with 33 at 0 dB.
pub const PRESETS: &[(&str, u8, [u8; 10])] = &[
    ("Classical", 33, [33, 33, 33, 33, 33, 33, 20, 20, 20, 16]),
    ("Club", 33, [33, 33, 38, 42, 42, 42, 38, 33, 33, 33]),
    ("Dance", 33, [48, 44, 36, 32, 32, 22, 20, 20, 32, 32]),
    (
        "Laptop speakers/headphones",
        33,
        [40, 50, 41, 26, 28, 35, 40, 48, 53, 56],
    ),
    ("Large hall", 33, [49, 49, 42, 42, 33, 24, 24, 24, 33, 33]),
    ("Party", 33, [44, 44, 33, 33, 33, 33, 33, 33, 44, 44]),
    ("Pop", 33, [29, 40, 44, 45, 41, 30, 28, 28, 29, 29]),
    ("Reggae", 33, [33, 33, 31, 22, 33, 43, 43, 33, 33, 33]),
    ("Rock", 33, [45, 40, 23, 19, 26, 39, 47, 50, 50, 50]),
    ("Soft", 33, [40, 35, 30, 28, 30, 39, 46, 48, 50, 52]),
    ("Ska", 33, [28, 24, 25, 31, 39, 42, 47, 48, 50, 48]),
    ("Full Bass", 33, [48, 48, 48, 42, 35, 25, 18, 15, 14, 14]),
    ("Soft Rock", 33, [39, 39, 36, 31, 25, 23, 26, 31, 37, 47]),
    ("Full Treble", 33, [16, 16, 16, 25, 37, 50, 58, 58, 58, 60]),
    (
        "Full Bass & Treble",
        33,
        [44, 42, 33, 20, 24, 35, 46, 50, 52, 52],
    ),
    ("Live", 33, [24, 33, 39, 41, 42, 42, 39, 37, 37, 36]),
    ("Techno", 33, [45, 42, 33, 23, 24, 33, 45, 48, 48, 47]),
];

/// A winamp.q1 value (1..64) as a slider position.
pub fn preset_value(v: u8) -> f32 {
    (v.clamp(1, 64) - 1) as f32 / 63.0
}

/// EQ settings as slider positions, written by the UI and read by the audio callback.
pub struct EqParams {
    enabled: AtomicBool,
    preamp: AtomicU32,
    bands: [AtomicU32; 10],
    /// Bumped on every change so the audio thread knows to recompute filters.
    version: AtomicU32,
}

impl Default for EqParams {
    fn default() -> Self {
        let mid = || AtomicU32::new(0.5f32.to_bits());
        EqParams {
            enabled: AtomicBool::new(false),
            preamp: mid(),
            bands: std::array::from_fn(|_| mid()),
            version: AtomicU32::new(1),
        }
    }
}

impl EqParams {
    pub fn set(&self, enabled: bool, preamp: f32, bands: &[f32; 10]) {
        self.enabled.store(enabled, Ordering::Relaxed);
        self.preamp.store(preamp.to_bits(), Ordering::Relaxed);
        for (a, v) in self.bands.iter().zip(bands) {
            a.store(v.to_bits(), Ordering::Relaxed);
        }
        self.version.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Clone, Copy, Default)]
struct Coeffs {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum Shape {
    LowShelf,
    Peaking,
    HighShelf,
}

/// RBJ audio EQ cookbook biquads (shelves with slope 1, peaking with Q 1, as Web Audio uses).
fn coeffs(shape: Shape, freq: f32, db: f32, rate: f32) -> Coeffs {
    let a = 10f32.powf(db / 40.0);
    let w0 = std::f32::consts::TAU * freq / rate;
    let (sin, cos) = (w0.sin(), w0.cos());
    let (b0, b1, b2, a0, a1, a2) = match shape {
        Shape::Peaking => {
            let alpha = sin / 2.0;
            (
                1.0 + alpha * a,
                -2.0 * cos,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cos,
                1.0 - alpha / a,
            )
        }
        Shape::LowShelf | Shape::HighShelf => {
            let k = 2.0 * a.sqrt() * sin / 2.0 * std::f32::consts::SQRT_2;
            let sign = if shape == Shape::LowShelf { 1.0 } else { -1.0 };
            let (ap, am) = (a + 1.0, a - 1.0);
            (
                a * (ap - sign * am * cos + k),
                sign * 2.0 * a * (am - sign * ap * cos),
                a * (ap - sign * am * cos - k),
                ap + sign * am * cos + k,
                -sign * 2.0 * (am + sign * ap * cos),
                ap + sign * am * cos - k,
            )
        }
    };
    Coeffs {
        b0: b0 / a0,
        b1: b1 / a0,
        b2: b2 / a0,
        a1: a1 / a0,
        a2: a2 / a0,
    }
}

/// Filter state for the audio thread.
#[derive(Default)]
pub struct Equalizer {
    version: u32,
    rate: u32,
    channels: usize,
    enabled: bool,
    preamp: f32,
    /// Bands that currently do something: (band index, coefficients).
    active: Vec<(usize, Coeffs)>,
    /// Transposed direct form II state, [band][channel] -> (z1, z2).
    state: Vec<Vec<[f32; 2]>>,
}

impl Equalizer {
    fn update(&mut self, params: &EqParams, rate: u32, channels: usize) {
        let version = params.version.load(Ordering::Relaxed);
        let format_changed = rate != self.rate || channels != self.channels;
        if version == self.version && !format_changed {
            return;
        }
        if format_changed {
            self.rate = rate;
            self.channels = channels;
            self.state = vec![vec![[0.0; 2]; channels]; BANDS_HZ.len()];
        }
        self.version = version;
        self.enabled = params.enabled.load(Ordering::Relaxed);
        let preamp = f32::from_bits(params.preamp.load(Ordering::Relaxed));
        self.preamp = 10f32.powf(slider_db(preamp) / 20.0);
        self.active.clear();
        for (i, (freq, band)) in BANDS_HZ.iter().zip(&params.bands).enumerate() {
            let db = slider_db(f32::from_bits(band.load(Ordering::Relaxed)));
            // Flat bands and bands above what the sample rate can carry are skipped.
            if db.abs() < 0.05 || *freq >= rate as f32 * 0.45 {
                self.state[i].iter_mut().for_each(|s| *s = [0.0; 2]);
                continue;
            }
            let shape = match i {
                0 => Shape::LowShelf,
                9 => Shape::HighShelf,
                _ => Shape::Peaking,
            };
            self.active.push((i, coeffs(shape, *freq, db, rate as f32)));
        }
    }

    /// Filters interleaved samples in place.
    pub fn process(&mut self, params: &EqParams, rate: u32, channels: usize, samples: &mut [f32]) {
        self.update(params, rate, channels);
        if !self.enabled {
            return;
        }
        for s in samples.iter_mut() {
            *s *= self.preamp;
        }
        for (band, c) in &self.active {
            let state = &mut self.state[*band];
            for frame in samples.chunks_exact_mut(channels) {
                for (x, z) in frame.iter_mut().zip(state.iter_mut()) {
                    let y = c.b0 * *x + z[0];
                    z[0] = c.b1 * *x - c.a1 * y + z[1];
                    z[1] = c.b2 * *x - c.a2 * y;
                    *x = y;
                }
            }
        }
    }

    /// Clears filter memory, e.g. after a seek or track change.
    pub fn reset(&mut self) {
        self.state.iter_mut().flatten().for_each(|s| *s = [0.0; 2]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Magnitude response in dB of `c` at `freq`.
    fn response_db(c: Coeffs, freq: f32, rate: f32) -> f32 {
        let w = std::f64::consts::TAU * freq as f64 / rate as f64;
        let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
        let (b0, b1, b2, a1, a2) = (
            c.b0 as f64,
            c.b1 as f64,
            c.b2 as f64,
            c.a1 as f64,
            c.a2 as f64,
        );
        let num = (b0 + b1 * c1 + b2 * c2).powi(2) + (b1 * s1 + b2 * s2).powi(2);
        let den = (1.0 + a1 * c1 + a2 * c2).powi(2) + (a1 * s1 + a2 * s2).powi(2);
        (10.0 * (num / den).log10()) as f32
    }

    #[test]
    fn filters_hit_their_gain() {
        let rate = 44100.0;
        let peak = coeffs(Shape::Peaking, 1000.0, 9.0, rate);
        assert!((response_db(peak, 1000.0, rate) - 9.0).abs() < 0.05);
        assert!(response_db(peak, 20.0, rate).abs() < 0.2);

        let low = coeffs(Shape::LowShelf, 60.0, -12.0, rate);
        assert!((response_db(low, 5.0, rate) + 12.0).abs() < 0.3);
        assert!(response_db(low, 5000.0, rate).abs() < 0.1);

        let high = coeffs(Shape::HighShelf, 16000.0, 6.0, rate);
        assert!((response_db(high, 21000.0, rate) - 6.0).abs() < 1.0);
        assert!(response_db(high, 100.0, rate).abs() < 0.1);
    }

    #[test]
    fn flat_eq_is_transparent_and_preamp_scales() {
        let params = EqParams::default();
        let mut eq = Equalizer::default();
        params.set(true, 0.5, &[0.5; 10]);
        let mut samples = vec![0.25f32, -0.5, 0.75, 1.0];
        eq.process(&params, 44100, 2, &mut samples);
        assert_eq!(samples, [0.25, -0.5, 0.75, 1.0]);

        params.set(true, 0.0, &[0.5; 10]);
        eq.process(&params, 44100, 2, &mut samples);
        let expected = 0.25 * 10f32.powf(-12.0 / 20.0);
        assert!((samples[0] - expected).abs() < 1e-5);
    }
}
