//! Album art: the cover embedded in a track's tags, or an image saved next to it, decoded on a
//! background thread.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use image::RgbaImage;
use image::imageops::FilterType;

/// Larger covers are shrunk once on load; the panel never needs more than this.
const MAX_SIZE: u32 = 1024;
/// Image files used when a track has no embedded cover, most preferred first.
const FOLDER_NAMES: &[&str] = &["cover", "folder", "front", "album", "albumart"];
const FOLDER_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png"];

pub struct Art {
    /// Distinguishes one loaded image from the next, so drawing can cache its scaled copy.
    pub id: u64,
    pub image: RgbaImage,
}

pub struct ArtLoader {
    requests: Sender<(u64, PathBuf)>,
    results: Receiver<(u64, Option<RgbaImage>)>,
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
                if res_tx.send((id, load(&path))).is_err() {
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
        for (id, image) in self.results.try_iter() {
            if id == self.latest {
                out = Some(image.map(|image| Art { id, image }));
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
