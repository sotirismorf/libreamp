//! The list of tracks: entries, selection, M3U/PLS files and background tag reading.

use std::collections::HashSet;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "ogg", "oga", "wav", "m4a", "mp4", "aac", "alac",
];

pub fn is_audio(path: &Path) -> bool {
    has_extension(path, AUDIO_EXTENSIONS)
}

pub fn is_list(path: &Path) -> bool {
    has_extension(path, &["m3u", "m3u8", "pls"])
}

fn has_extension(path: &Path, exts: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| exts.contains(&e.to_ascii_lowercase().as_str()))
}

pub struct Entry {
    /// Stable identity, so tag results can find the entry after edits.
    pub id: u64,
    pub path: PathBuf,
    pub title: Option<String>,
    pub duration: Option<f32>,
    pub selected: bool,
}

impl Entry {
    pub fn display_title(&self) -> String {
        match &self.title {
            Some(t) => t.clone(),
            None => self.path.file_stem().map_or_else(
                || self.path.display().to_string(),
                |s| s.to_string_lossy().into_owned(),
            ),
        }
    }
}

/// Tag info read in the background: (entry id, title, duration).
type TagResult = (u64, Option<String>, Option<f32>);

pub struct Playlist {
    pub entries: Vec<Entry>,
    /// Index of the playing (or next to play) entry.
    pub current: usize,
    next_id: u64,
    tag_requests: Sender<(u64, PathBuf)>,
    tag_results: Receiver<TagResult>,
}

impl Playlist {
    pub fn new() -> Self {
        let (req_tx, req_rx) = channel::<(u64, PathBuf)>();
        let (res_tx, res_rx) = channel();
        std::thread::spawn(move || {
            for (id, path) in req_rx {
                let result = match crate::audio::read_info(&path) {
                    Ok(info) => (id, Some(info.title), info.duration),
                    Err(_) => (id, None, None),
                };
                if res_tx.send(result).is_err() {
                    break;
                }
            }
        });
        Playlist {
            entries: Vec::new(),
            current: 0,
            next_id: 0,
            tag_requests: req_tx,
            tag_results: res_rx,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    fn push(&mut self, path: PathBuf, title: Option<String>, duration: Option<f32>) {
        let id = self.next_id;
        self.next_id += 1;
        if title.is_none() || duration.is_none() {
            let _ = self.tag_requests.send((id, path.clone()));
        }
        self.entries.push(Entry {
            id,
            path,
            title,
            duration,
            selected: false,
        });
    }

    /// Adds a file, a playlist file's contents, or every audio file under a directory.
    pub fn add(&mut self, path: &Path) {
        self.add_walk(path, &mut HashSet::new());
    }

    /// `add`, remembering directories already walked so symlink loops end.
    fn add_walk(&mut self, path: &Path, seen: &mut HashSet<PathBuf>) {
        if path.is_dir() {
            let Ok(real) = std::fs::canonicalize(path) else {
                return;
            };
            if !seen.insert(real) {
                return;
            }
            let Ok(entries) = std::fs::read_dir(path) else {
                return;
            };
            let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
            paths.sort();
            for p in paths {
                // Playlist files inside folders would duplicate their tracks.
                if !is_list(&p) {
                    self.add_walk(&p, seen);
                }
            }
        } else if is_list(path) {
            if let Err(e) = self.load_list(path) {
                eprintln!("libreamp: {}: {e}", path.display());
            }
        } else if is_audio(path) {
            self.push(path.to_path_buf(), None, None);
        }
    }

    /// Applies tag info read in the background.
    pub fn poll_tags(&mut self) {
        let results: Vec<TagResult> = self.tag_results.try_iter().collect();
        if results.is_empty() {
            return;
        }
        let index: std::collections::HashMap<u64, usize> = self
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.id, i))
            .collect();
        for (id, title, duration) in results {
            if let Some(&i) = index.get(&id) {
                let entry = &mut self.entries[i];
                entry.title = entry.title.take().or(title);
                entry.duration = entry.duration.or(duration);
            }
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.current = 0;
    }

    pub fn current(&self) -> Option<&Entry> {
        self.entries.get(self.current)
    }

    /// Moves by `delta` entries; wraps around when `wrap` is set, otherwise fails at the ends.
    pub fn step(&mut self, delta: isize, wrap: bool) -> bool {
        let len = self.len() as isize;
        if len == 0 {
            return false;
        }
        let next = self.current as isize + delta;
        if (0..len).contains(&next) {
            self.current = next as usize;
            true
        } else if wrap {
            self.current = next.rem_euclid(len) as usize;
            true
        } else {
            false
        }
    }

    /// Keeps entries matching `keep`, adjusting `current` to follow its entry where possible.
    fn retain(&mut self, keep: impl Fn(&Entry) -> bool) {
        let before_current = self.entries[..self.current.min(self.len())]
            .iter()
            .filter(|e| !keep(e))
            .count();
        self.entries.retain(|e| keep(e));
        self.current = self
            .current
            .saturating_sub(before_current)
            .min(self.len().saturating_sub(1));
    }

    pub fn remove_selected(&mut self) {
        self.retain(|e| !e.selected);
    }

    pub fn crop(&mut self) {
        self.retain(|e| e.selected);
    }

    pub fn select_all(&mut self, selected: bool) {
        self.entries.iter_mut().for_each(|e| e.selected = selected);
    }

    pub fn invert_selection(&mut self) {
        self.entries
            .iter_mut()
            .for_each(|e| e.selected = !e.selected);
    }

    /// Sorts by displayed title, keeping the current track current.
    pub fn sort_by_title(&mut self) {
        let current_id = self.current().map(|e| e.id);
        self.entries
            .sort_by_cached_key(|e| e.display_title().to_lowercase());
        if let Some(id) = current_id {
            self.current = self.entries.iter().position(|e| e.id == id).unwrap_or(0);
        }
    }

    /// Total and selected duration in seconds (known durations only).
    pub fn durations(&self) -> (f32, f32) {
        self.entries.iter().fold((0.0, 0.0), |(all, sel), e| {
            let d = e.duration.unwrap_or(0.0);
            (all + d, if e.selected { sel + d } else { sel })
        })
    }

    /// Appends the tracks listed in an M3U or PLS file. Relative paths are resolved against it.
    pub fn load_list(&mut self, path: &Path) -> std::io::Result<()> {
        let text = std::fs::read(path).map(|b| String::from_utf8_lossy(&b).into_owned())?;
        let base = path.parent().unwrap_or(Path::new("."));
        let resolve = |p: &str| {
            let p = p.trim();
            match p.strip_prefix("file://") {
                Some(url) => base.join(percent_decode(url)),
                None => base.join(p),
            }
        };
        if has_extension(path, &["pls"]) {
            for line in text.lines() {
                if let Some((key, value)) = line.split_once('=')
                    && key.trim().to_ascii_lowercase().starts_with("file")
                {
                    self.push(resolve(value), None, None);
                }
            }
            return Ok(());
        }
        // #EXTINF:<seconds>,<title> describes the next path line.
        let mut info: Option<(Option<f32>, Option<String>)> = None;
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
            if let Some(extinf) = line.strip_prefix("#EXTINF:") {
                let (secs, title) = extinf.split_once(',').unwrap_or((extinf, ""));
                let secs = secs.trim().parse::<f32>().ok().filter(|s| *s >= 0.0);
                let title = Some(title.trim().to_string()).filter(|t| !t.is_empty());
                info = Some((secs, title));
            } else if !line.starts_with('#') {
                let (duration, title) = info.take().unwrap_or((None, None));
                self.push(resolve(line), title, duration);
            }
        }
        Ok(())
    }

    /// Writes the playlist as extended M3U with absolute paths.
    pub fn save_list(&self, path: &Path) -> std::io::Result<()> {
        std::fs::write(path, self.to_m3u())
    }

    pub fn to_m3u(&self) -> String {
        let mut out = String::from("#EXTM3U\n");
        for e in &self.entries {
            let secs = e.duration.map_or(-1, |d| d.round() as i64);
            // A title can't span lines without breaking the file.
            let title = e.display_title().replace(['\r', '\n'], " ");
            out += &format!("#EXTINF:{secs},{title}\n{}\n", e.path.display());
        }
        out
    }
}

/// Decodes %XX escapes in the path part of a file:// URL (after the "file://").
fn percent_decode(url: &str) -> PathBuf {
    // "file://host/path": only local files are meaningful, so drop any host.
    let path = url.find('/').map_or(url, |i| &url[i..]);
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    PathBuf::from(OsString::from_vec(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_file_urls() {
        assert_eq!(
            percent_decode("/home/me/My%20Music/%CE%B1.mp3"),
            Path::new("/home/me/My Music/α.mp3")
        );
        assert_eq!(percent_decode("localhost/a%2"), Path::new("/a%2"));
    }

    #[test]
    fn reads_extended_m3u() {
        let dir = std::env::temp_dir().join(format!("libreamp-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let list = dir.join("list.m3u");
        std::fs::write(
            &list,
            "#EXTM3U\n#EXTINF:61,Artist - Song\nsong.mp3\nfile:///abs/x%20y.flac\n",
        )
        .unwrap();
        let mut pl = Playlist::new();
        pl.load_list(&list).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(pl.len(), 2);
        assert_eq!(pl.entries[0].path, dir.join("song.mp3"));
        assert_eq!(pl.entries[0].title.as_deref(), Some("Artist - Song"));
        assert_eq!(pl.entries[0].duration, Some(61.0));
        assert_eq!(pl.entries[1].path, Path::new("/abs/x y.flac"));
    }

    #[test]
    fn symlink_loops_end() {
        let dir = std::env::temp_dir().join(format!("libreamp-loop-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.mp3"), b"").unwrap();
        let _ = std::os::unix::fs::symlink(&dir, dir.join("again"));
        let mut pl = Playlist::new();
        pl.add(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(pl.len(), 1);
    }
}
