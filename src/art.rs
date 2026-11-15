//! Album art: the cover embedded in a track's tags, or an image saved next to it, decoded on a
//! background thread.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use image::RgbaImage;
use image::imageops::FilterType;

/// Larger covers are shrunk once on load; the panel never needs more than this.
const MAX_SIZE: u32 = 2048;
/// Halved copies stop once the longer side would drop below this.
const MIN_LEVEL_SIZE: u32 = 64;
/// Image files used when a track has no embedded cover, most preferred first.
const FOLDER_NAMES: &[&str] = &["cover", "folder", "front", "album", "albumart"];
const FOLDER_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png"];

pub struct Art {
    /// Distinguishes one loaded image from the next, so drawing can cache its textures.
    pub id: u64,
    /// The cover at full size, then halved again and again (like mipmaps), all made on the
    /// loading thread. Drawing picks the copy just larger than needed and lets the GPU scale
    /// it the rest of the way (by at most half, which stays smooth), so resizing the panel
    /// never resamples anything on the UI thread.
    pub levels: Vec<RgbaImage>,
}

impl Art {
    /// The smallest copy at least `w` x `h`, or the full-size one when even that is smaller.
    pub fn level_for(&self, w: u32, h: u32) -> usize {
        (self.levels.iter())
            .rposition(|l| l.width() >= w && l.height() >= h)
            .unwrap_or(0)
    }
}

/// The halving chain for `image`, starting with `image` itself.
fn levels(image: RgbaImage) -> Vec<RgbaImage> {
    let mut levels = vec![image];
    loop {
        let last = &levels[levels.len() - 1];
        let (w, h) = (last.width() / 2, last.height() / 2);
        if w.max(h) < MIN_LEVEL_SIZE || w == 0 || h == 0 {
            return levels;
        }
        let half = image::imageops::resize(last, w, h, FilterType::Triangle);
        levels.push(half);
    }
}

pub struct ArtLoader {
    requests: Sender<(u64, PathBuf)>,
    results: Receiver<(u64, Option<Vec<RgbaImage>>)>,
    /// The track whose art is shown or loading, and that request's id.
    path: Option<PathBuf>,
    latest: u64,
    /// Set when the track went away; the next poll reports "no art".
    cleared: bool,
}

impl ArtLoader {
    pub fn new() -> Self {
        let (req_tx, req_rx) = channel::<(u64, PathBuf)>();
        let (res_tx, res_rx) = channel();
        std::thread::spawn(move || {
            while let Ok(mut job) = req_rx.recv() {
                // Skipping through tracks queues requests; only the newest matters.
                while let Ok(newer) = req_rx.try_recv() {
                    job = newer;
                }
                let (id, path) = job;
                if res_tx.send((id, load(&path).map(levels))).is_err() {
                    break;
                }
            }
        });
        ArtLoader {
            requests: req_tx,
            results: res_rx,
            path: None,
            latest: 0,
            cleared: false,
        }
    }

    /// Starts loading the art for `path`, unless it is already shown or loading.
    pub fn request(&mut self, path: Option<&Path>) {
        if self.path.as_deref() == path {
            return;
        }
        self.path = path.map(Path::to_path_buf);
        self.latest += 1;
        match path {
            Some(p) => {
                let _ = self.requests.send((self.latest, p.to_path_buf()));
            }
            None => self.cleared = true,
        }
    }

    /// The outcome of the latest request once it is known: `Some(None)` means no art.
    pub fn poll(&mut self) -> Option<Option<Art>> {
        if std::mem::take(&mut self.cleared) {
            return Some(None);
        }
        let mut out = None;
        for (id, levels) in self.results.try_iter() {
            if id == self.latest {
                out = Some(levels.map(|levels| Art { id, levels }));
            }
        }
        out
    }
}

/// Finds and decodes the art for the track at `path`.
fn load(path: &Path) -> Option<RgbaImage> {
    let decoded = crate::audio::read_cover(path)
        .and_then(|bytes| image::load_from_memory(&bytes).ok())
        .or_else(|| folder_image(path).and_then(|p| image::open(p).ok()))?;
    let image = decoded.to_rgba8();
    let (w, h) = image.dimensions();
    if w.max(h) <= MAX_SIZE {
        return Some(image);
    }
    let (w, h) = fit(w, h, MAX_SIZE, MAX_SIZE);
    Some(image::imageops::resize(&image, w, h, FilterType::Triangle))
}

/// cover.jpg, folder.png and the like in the track's directory, matched case-insensitively.
fn folder_image(track: &Path) -> Option<PathBuf> {
    let candidates: Vec<(usize, PathBuf)> = std::fs::read_dir(track.parent()?)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let ext = p.extension()?.to_str()?.to_ascii_lowercase();
            let stem = p.file_stem()?.to_str()?.to_ascii_lowercase();
            let rank = FOLDER_NAMES.iter().position(|n| *n == stem)?;
            FOLDER_EXTENSIONS
                .contains(&ext.as_str())
                .then_some((rank, p))
        })
        .collect();
    candidates.into_iter().min().map(|(_, p)| p)
}

/// The largest size with `w:h`'s aspect ratio that fits in `max_w` x `max_h`.
pub fn fit(w: u32, h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    let s = (max_w as f32 / w.max(1) as f32).min(max_h as f32 / h.max(1) as f32);
    let size = |v: u32, max: u32| ((v as f32 * s).round() as u32).clamp(1, max);
    (size(w, max_w), size(h, max_h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_keeps_aspect() {
        assert_eq!(fit(1000, 1000, 256, 232), (232, 232));
        assert_eq!(fit(1600, 900, 256, 232), (256, 144));
        assert_eq!(fit(100, 400, 256, 232), (58, 232));
    }

    #[test]
    fn halves_down_to_a_small_size_and_picks_the_next_larger() {
        let art = Art {
            id: 0,
            levels: levels(RgbaImage::new(1000, 600)),
        };
        let sizes: Vec<_> = art.levels.iter().map(|l| l.dimensions()).collect();
        assert_eq!(sizes, [(1000, 600), (500, 300), (250, 150), (125, 75)]);
        assert_eq!(art.level_for(400, 240), 1);
        assert_eq!(art.level_for(500, 300), 1);
        assert_eq!(art.level_for(501, 300), 0);
        assert_eq!(art.level_for(2000, 1200), 0);
        assert_eq!(art.level_for(10, 6), 3);
    }

    #[test]
    fn finds_folder_art() {
        let dir = std::env::temp_dir().join(format!("libreamp-art-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["Folder.JPG", "cover.png", "notes.txt", "song.mp3"] {
            std::fs::write(dir.join(name), b"").unwrap();
        }
        let found = folder_image(&dir.join("song.mp3"));
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(found, Some(dir.join("cover.png")));
    }
}
