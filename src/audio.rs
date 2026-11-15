//! Audio playback: symphonia decodes on SDL's audio thread; SDL resamples to the device.

use std::fs::File;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::eq::{EqParams, Equalizer};
use crate::vis;
use sdl3::audio::{AudioCallback, AudioFormat, AudioSpec, AudioStream, AudioStreamWithCallback};
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as DecodeError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, StandardTag};
use symphonia::core::units::{Time, TimeBase};

/// What the UI needs to know about a loaded track.
pub struct TrackInfo {
    pub title: String,
    pub duration: Option<f32>,
    pub rate: u32,
    pub channels: u32,
    pub kbps: Option<u32>,
}

/// An opened file ready to decode.
struct Source {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    time_base: Option<TimeBase>,
    rate: u32,
    channels: usize,
    /// Decoded samples not yet handed to SDL.
    pending: Vec<f32>,
    pending_pos: usize,
}

fn open(path: &Path) -> Result<(Source, TrackInfo), String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let file_bytes = file.metadata().map(|m| m.len()).ok();
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| e.to_string())?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or("no audio track")?;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or("unsupported codec")?;
    let decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|e| e.to_string())?;
    let rate = params.sample_rate.ok_or("unknown sample rate")?;
    let channels = params
        .channels
        .as_ref()
        .map_or(2, |c| c.count())
        .clamp(1, 8);
    let duration = match (track.time_base, track.duration) {
        (Some(tb), Some(d)) => tb.calc_duration(d).map(|t| t.as_secs_f64() as f32),
        _ => track.num_frames.map(|n| n as f32 / rate as f32),
    };
    let (track_id, time_base) = (track.id, track.time_base);

    let mut title = None;
    let mut artist = None;
    if let Some(rev) = format.metadata().skip_to_latest() {
        let per_track = rev.per_track.iter().flat_map(|t| &t.metadata.tags);
        for tag in rev.media.tags.iter().chain(per_track) {
            match &tag.std {
                Some(StandardTag::TrackTitle(s)) => title = Some(s.to_string()),
                Some(StandardTag::Artist(s)) => artist = Some(s.to_string()),
                _ => {}
            }
        }
    }
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned());
    let title = match (artist, title) {
        (Some(a), Some(t)) => format!("{a} - {t}"),
        (None, Some(t)) => t,
        _ => stem.unwrap_or_default(),
    };
    let kbps = match (file_bytes, duration) {
        (Some(b), Some(d)) if d > 0.0 => Some((b as f32 * 8.0 / d / 1000.0).round() as u32),
        _ => None,
    };

    let source = Source {
        format,
        decoder,
        track_id,
        time_base,
        rate,
        channels,
        pending: Vec::new(),
        pending_pos: 0,
    };
    let info = TrackInfo {
        title,
        duration,
        rate,
        channels: channels as u32,
        kbps,
    };
    Ok((source, info))
}

/// Reads a file's title and duration without playing it.
pub fn read_info(path: &Path) -> Result<TrackInfo, String> {
    open(path).map(|(_, info)| info)
}

impl Source {
    /// Decodes the next packet into `pending`. Returns false at end of stream or on fatal errors.
    fn decode_next(&mut self) -> bool {
        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return false,
                Err(e) => {
                    eprintln!("libreamp: read error: {e}");
                    return false;
                }
            };
            if packet.track_id != self.track_id {
                continue;
            }
            match self.decoder.decode(&packet) {
                Ok(buf) => {
                    buf.copy_to_vec_interleaved(&mut self.pending);
                    self.pending_pos = 0;
                    return true;
                }
                // Corrupt packets are skipped, like other players do.
                Err(DecodeError::DecodeError(_)) | Err(DecodeError::IoError(_)) => continue,
                Err(e) => {
                    eprintln!("libreamp: decode error: {e}");
                    return false;
                }
            }
        }
    }
}

/// The most recent mono samples, for the visualizer.
struct VisRing {
    samples: Vec<f32>,
    pos: usize,
}

/// State shared between the UI thread and the audio callback. The visualizer ring is the only
/// lock, and the audio thread never waits for it.
#[derive(Default)]
struct Shared {
    vis: Mutex<Option<VisRing>>,
    /// Playback position in frames of the current source.
    frames: AtomicU64,
    finished: AtomicBool,
    /// f32 bits.
    volume: AtomicU32,
    balance: AtomicU32,
}

struct Player {
    source: Option<Source>,
    shared: Arc<Shared>,
    eq_params: Arc<EqParams>,
    eq: Equalizer,
    out: Vec<f32>,
}

impl AudioCallback<f32> for Player {
    fn callback(&mut self, stream: &mut AudioStream, requested: i32) {
        let Some(src) = &mut self.source else { return };
        // Whole frames only, so channels never get out of step.
        let wanted = requested.max(0) as usize / src.channels * src.channels;
        self.out.clear();
        let mut ended = false;
        while self.out.len() < wanted {
            if src.pending_pos >= src.pending.len() && !src.decode_next() {
                ended = true;
                break;
            }
            let take = (wanted - self.out.len()).min(src.pending.len() - src.pending_pos);
            self.out
                .extend_from_slice(&src.pending[src.pending_pos..src.pending_pos + take]);
            src.pending_pos += take;
        }

        self.eq
            .process(&self.eq_params, src.rate, src.channels, &mut self.out);

        if let Ok(mut vis) = self.shared.vis.try_lock() {
            let ring = vis.get_or_insert_with(|| VisRing {
                samples: vec![0.0; vis::WINDOW],
                pos: 0,
            });
            for frame in self.out.chunks_exact(src.channels) {
                ring.samples[ring.pos] = frame.iter().sum::<f32>() / src.channels as f32;
                ring.pos = (ring.pos + 1) % vis::WINDOW;
            }
        }

        let volume = f32::from_bits(self.shared.volume.load(Ordering::Relaxed));
        let balance = f32::from_bits(self.shared.balance.load(Ordering::Relaxed));
        // Perceptual volume curve.
        let gain = volume * volume;
        if src.channels == 2 {
            let (l, r) = (
                (1.0 - balance).min(1.0) * gain,
                (1.0 + balance).min(1.0) * gain,
            );
            for [left, right] in self.out.as_chunks_mut::<2>().0 {
                *left *= l;
                *right *= r;
            }
        } else {
            self.out.iter_mut().for_each(|s| *s *= gain);
        }

        let frames = (self.out.len() / src.channels) as u64;
        self.shared.frames.fetch_add(frames, Ordering::Relaxed);
        if !self.out.is_empty() {
            let _ = stream.put_data_f32(&self.out);
        }
        if ended && !self.shared.finished.swap(true, Ordering::Relaxed) {
            // Push out samples SDL's resampler is holding back, so the queue can drain.
            let _ = stream.flush();
        }
    }
}

pub struct Audio {
    stream: AudioStreamWithCallback<Player>,
    shared: Arc<Shared>,
    eq_params: Arc<EqParams>,
    /// Sample rate of the loaded source, 0 when none.
    rate: u32,
}

impl Audio {
    pub fn new(sdl: &sdl3::Sdl) -> Result<Audio, String> {
        let audio = sdl.audio().map_err(|e| e.to_string())?;
        let shared = Arc::new(Shared::default());
        shared.volume.store(1.0f32.to_bits(), Ordering::Relaxed);
        let spec = AudioSpec {
            freq: Some(44100),
            channels: Some(2),
            format: Some(AudioFormat::f32_sys()),
        };
        let eq_params = Arc::new(EqParams::default());
        let player = Player {
            source: None,
            shared: shared.clone(),
            eq_params: eq_params.clone(),
            eq: Equalizer::default(),
            out: Vec::new(),
        };
        let stream = audio
            .open_playback_stream(&spec, player)
            .map_err(|e| e.to_string())?;
        Ok(Audio {
            stream,
            shared,
            eq_params,
            rate: 0,
        })
    }

    /// Opens `path` and starts playing it from the beginning.
    pub fn play(&mut self, path: &Path) -> Result<TrackInfo, String> {
        let (source, info) = open(path)?;
        let spec = sdl3::sys::audio::SDL_AudioSpec {
            format: AudioFormat::f32_sys().into(),
            channels: source.channels as i32,
            freq: source.rate as i32,
        };
        let raw = self.stream.raw();
        {
            let mut player = self.stream.lock().ok_or("could not lock audio stream")?;
            player.source = Some(source);
            player.eq.reset();
            // The stream's input format follows the file; SDL converts to whatever the device
            // wants. Done under the (recursive) stream lock so no callback sees a mismatch.
            unsafe {
                if !sdl3::sys::audio::SDL_SetAudioStreamFormat(raw, &spec, std::ptr::null()) {
                    return Err(sdl3::get_error().to_string());
                }
                sdl3::sys::audio::SDL_ClearAudioStream(raw);
            }
        }
        self.rate = info.rate;
        self.shared.frames.store(0, Ordering::Relaxed);
        self.shared.finished.store(false, Ordering::Relaxed);
        self.stream.resume().map_err(|e| e.to_string())?;
        Ok(info)
    }

    pub fn stop(&mut self) {
        let _ = self.stream.pause();
        if let Some(mut player) = self.stream.lock() {
            player.source = None;
        }
        let _ = self.stream.clear();
        self.rate = 0;
        self.shared.frames.store(0, Ordering::Relaxed);
    }

    pub fn set_paused(&mut self, paused: bool) {
        let _ = if paused {
            self.stream.pause()
        } else {
            self.stream.resume()
        };
    }

    pub fn seek(&mut self, secs: f32) {
        let Some(mut player) = self.stream.lock() else {
            return;
        };
        let Some(src) = &mut player.source else {
            return;
        };
        let Some(time) = Time::try_from_secs_f64(secs.max(0.0) as f64) else {
            return;
        };
        let to = SeekTo::Time {
            time,
            track_id: Some(src.track_id),
        };
        match src.format.seek(SeekMode::Accurate, to) {
            Ok(seeked) => {
                src.decoder.reset();
                src.pending.clear();
                src.pending_pos = 0;
                let actual = src
                    .time_base
                    .and_then(|tb| tb.calc_time(seeked.actual_ts))
                    .map_or(secs as f64, |t| t.as_secs_f64());
                let frames = (actual.max(0.0) * src.rate as f64) as u64;
                player.eq.reset();
                drop(player);
                self.shared.frames.store(frames, Ordering::Relaxed);
                self.shared.finished.store(false, Ordering::Relaxed);
                let _ = self.stream.clear();
            }
            Err(e) => eprintln!("libreamp: seek failed: {e}"),
        }
    }

    /// Copies the latest `vis::WINDOW` mono samples, oldest first, into `out`.
    pub fn vis_samples(&self, out: &mut Vec<f32>) -> bool {
        let Ok(vis) = self.shared.vis.lock() else {
            return false;
        };
        let Some(ring) = vis.as_ref() else {
            return false;
        };
        out.clear();
        out.extend_from_slice(&ring.samples[ring.pos..]);
        out.extend_from_slice(&ring.samples[..ring.pos]);
        true
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Seconds played of the current track.
    pub fn position(&self) -> f32 {
        if self.rate == 0 {
            return 0.0;
        }
        self.shared.frames.load(Ordering::Relaxed) as f32 / self.rate as f32
    }

    /// True once the current track has been fully decoded and drained.
    pub fn finished(&self) -> bool {
        self.rate != 0
            && self.shared.finished.load(Ordering::Relaxed)
            && self.stream.queued_bytes().unwrap_or(0) == 0
    }

    pub fn set_volume(&self, v: f32) {
        self.shared.volume.store(v.to_bits(), Ordering::Relaxed);
    }

    /// EQ settings as slider positions (0..1, 0.5 is flat).
    pub fn set_eq(&self, enabled: bool, preamp: f32, bands: &[f32; 10]) {
        self.eq_params.set(enabled, preamp, bands);
    }

    pub fn set_balance(&self, b: f32) {
        self.shared.balance.store(b.to_bits(), Ordering::Relaxed);
    }
}
