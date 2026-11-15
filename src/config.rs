//! Remembered state between runs, in $XDG_CONFIG_HOME/libreamp (or ~/.config/libreamp).
//! `state` is plain key=value lines; the playlist is kept as `playlist.m3u`.

use std::path::{Path, PathBuf};

use crate::ui::playlist_window::DEFAULT_TILES;
use crate::vis;

pub struct State {
    pub volume: f32,
    pub balance: f32,
    pub repeat: bool,
    pub shuffle: bool,
    pub playlist_open: bool,
    pub eq_open: bool,
    pub vis_mode: vis::Mode,
    pub eq_on: bool,
    pub eq_preamp: f32,
    pub eq_bands: [f32; 10],
    pub playlist_tiles: u32,
    pub art_open: bool,
    /// Height of the album art area in skin pixels; None keeps it square.
    pub art_height: Option<i32>,
    pub current: usize,
    pub skin: Option<PathBuf>,
    /// Size in percent.
    pub scale: u32,
    /// Seconds into the current track, when it was playing or paused at exit.
    pub position: Option<f32>,
}

impl Default for State {
    fn default() -> Self {
        State {
            volume: 0.75,
            balance: 0.0,
            repeat: false,
            shuffle: false,
            playlist_open: true,
            eq_open: false,
            vis_mode: vis::Mode::Spectrum,
            eq_on: false,
            eq_preamp: 0.5,
            eq_bands: [0.5; 10],
            playlist_tiles: DEFAULT_TILES,
            art_open: false,
            art_height: None,
            current: 0,
            skin: None,
            scale: crate::DEFAULT_SCALE,
            position: None,
        }
    }
}

pub fn dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("libreamp"))
}

pub fn playlist_path() -> Option<PathBuf> {
    dir().map(|d| d.join("playlist.m3u"))
}

pub fn load() -> State {
    let mut state = State::default();
    let Some(text) = dir().and_then(|d| std::fs::read_to_string(d.join("state")).ok()) else {
        return state;
    };
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        let flag = value == "1";
        match key.trim() {
            "volume" => state.volume = value.parse().unwrap_or(state.volume).clamp(0.0, 1.0),
            "balance" => state.balance = value.parse().unwrap_or(state.balance).clamp(-1.0, 1.0),
            "repeat" => state.repeat = flag,
            "shuffle" => state.shuffle = flag,
            "playlist_open" => state.playlist_open = flag,
            "eq_open" => state.eq_open = flag,
            "vis" => state.vis_mode = vis::Mode::from_name(value).unwrap_or_default(),
            "eq_on" => state.eq_on = flag,
            "eq_preamp" => state.eq_preamp = value.parse().unwrap_or(0.5f32).clamp(0.0, 1.0),
            "eq_bands" => {
                let values: Vec<f32> = value
                    .split(',')
                    .filter_map(|v| v.trim().parse().ok())
                    .collect();
                if let Ok(bands) = <[f32; 10]>::try_from(values) {
                    state.eq_bands = bands.map(|v| v.clamp(0.0, 1.0));
                }
            }
            "playlist_tiles" => {
                state.playlist_tiles = value.parse().unwrap_or(state.playlist_tiles)
            }
            "art_open" => state.art_open = flag,
            "art_height" => state.art_height = value.parse().ok(),
            "position" => state.position = value.parse().ok().filter(|p: &f32| p.is_finite()),
            "current" => state.current = value.parse().unwrap_or(0),
            "skin" if !value.is_empty() => state.skin = Some(value.into()),
            // Older versions saved a pixel multiple (1, 2, …), which falls back to the default.
            "scale" => state.scale = crate::parse_scale(value).unwrap_or(crate::DEFAULT_SCALE),
            _ => {}
        }
    }
    state
}

pub fn save(state: &State) -> std::io::Result<()> {
    let dir = dir().ok_or_else(|| std::io::Error::other("no config directory"))?;
    std::fs::create_dir_all(&dir)?;
    let b = |v: bool| if v { 1 } else { 0 };
    let bands: Vec<String> = state.eq_bands.iter().map(f32::to_string).collect();
    let text = format!(
        "volume={}\nbalance={}\nrepeat={}\nshuffle={}\nplaylist_open={}\nvis={}\neq_open={}\neq_on={}\neq_preamp={}\neq_bands={}\nplaylist_tiles={}\nart_open={}\nart_height={}\ncurrent={}\nposition={}\nskin={}\nscale={}\n",
        state.volume,
        state.balance,
        b(state.repeat),
        b(state.shuffle),
        b(state.playlist_open),
        state.vis_mode.name(),
        b(state.eq_open),
        b(state.eq_on),
        state.eq_preamp,
        bands.join(","),
        state.playlist_tiles,
        b(state.art_open),
        state.art_height.map(|h| h.to_string()).unwrap_or_default(),
        state.current,
        state.position.map(|p| p.to_string()).unwrap_or_default(),
        state
            .skin
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        state.scale,
    );
    write_atomic(&dir.join("state"), &text)
}

/// Writes via a temporary file and a rename, so a crash mid-write can't leave half a file.
pub fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)
}
