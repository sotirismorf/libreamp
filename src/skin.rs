//! Loading classic Winamp skins (.wsz zip archives or unpacked directories).

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::Path;

/// A .wsz/.zip archive, or an unpacked skin directory.
pub fn is_skin(path: &Path) -> bool {
    (path.is_dir() && (path.join("main.bmp").exists() || path.join("MAIN.BMP").exists()))
        || path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("wsz") || e.eq_ignore_ascii_case("zip"))
}

/// A decoded sprite sheet, pixels as 0xAARRGGBB.
pub struct Sheet {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u32>,
}

impl Sheet {
    fn empty() -> Self {
        Sheet {
            w: 0,
            h: 0,
            px: Vec::new(),
        }
    }

    /// Colour at (x, y) as 0xRRGGBB, if inside the sheet.
    pub fn pixel(&self, x: i32, y: i32) -> Option<u32> {
        let inside = x >= 0 && y >= 0 && x < self.w as i32 && y < self.h as i32;
        inside.then(|| self.px[(y * self.w as i32 + x) as usize] & 0xff_ffff)
    }

    fn decode(bytes: &[u8]) -> Option<Sheet> {
        let img = image::load_from_memory(bytes).ok()?.to_rgba8();
        let (w, h) = img.dimensions();
        let px = img
            .pixels()
            .map(|p| u32::from_be_bytes([0xff, p[0], p[1], p[2]]))
            .collect();
        Some(Sheet { w, h, px })
    }
}

pub struct Skin {
    sheets: HashMap<String, Sheet>,
    empty: Sheet,
    pub playlist_colors: PlaylistColors,
    pub vis_colors: [u32; 24],
}

/// Playlist text colours from pledit.txt, as 0xRRGGBB.
#[derive(Clone, Copy)]
pub struct PlaylistColors {
    pub normal: u32,
    pub current: u32,
    pub normal_bg: u32,
    pub selected_bg: u32,
}

impl Default for PlaylistColors {
    fn default() -> Self {
        PlaylistColors {
            normal: 0x00ff00,
            current: 0xffffff,
            normal_bg: 0x000000,
            selected_bg: 0x0000c6,
        }
    }
}

impl PlaylistColors {
    /// Parses the [Text] section of pledit.txt; unknown or malformed keys keep `self`'s value.
    fn parse(mut self, text: &str) -> Self {
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let hex = value.trim().trim_start_matches('#');
            let Some(rgb) = hex.get(..6).and_then(|h| u32::from_str_radix(h, 16).ok()) else {
                continue;
            };
            match key.trim().to_ascii_lowercase().as_str() {
                "normal" => self.normal = rgb,
                "current" => self.current = rgb,
                "normalbg" => self.normal_bg = rgb,
                "selectedbg" => self.selected_bg = rgb,
                _ => {}
            }
        }
        self
    }

    fn from_files(files: &Files) -> Option<Self> {
        let (_, bytes) = files.iter().find(|(name, _)| name == "pledit.txt")?;
        Some(PlaylistColors::default().parse(&String::from_utf8_lossy(bytes)))
    }
}

/// LibreAmp's default skin (Winamp's classic base skin, rebranded by tools/rebrand_skin.py),
/// also used to fill in sheets other skins lack.
const BASE_SKIN: &[u8] = include_bytes!("../skins/libreamp-base.wsz");

/// Sheet names we care about, lowercase without extension.
const SHEETS: &[&str] = &[
    "main", "titlebar", "cbuttons", "numbers", "nums_ex", "text", "posbar", "volume", "balance",
    "shufrep", "monoster", "playpaus", "eqmain", "pledit",
];

impl Skin {
    pub fn base() -> Skin {
        let files = read_zip(std::io::Cursor::new(BASE_SKIN)).expect("bundled base skin is valid");
        Skin {
            sheets: decode_sheets(&files),
            empty: Sheet::empty(),
            playlist_colors: PlaylistColors::from_files(&files).unwrap_or_default(),
            vis_colors: vis_colors(&files).expect("bundled base skin has viscolor.txt"),
        }
    }

    /// Loads a skin; sheets it lacks are taken from the base skin.
    pub fn load(path: &Path) -> Result<Skin, String> {
        let files = if path.is_dir() {
            read_dir(path)?
        } else {
            let file = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
            read_zip(file).map_err(|e| format!("{}: {e}", path.display()))?
        };
        let sheets = decode_sheets(&files);
        if !sheets.contains_key("main") {
            return Err(format!("{} has no main.bmp", path.display()));
        }
        let mut skin = Skin::base();
        skin.sheets.extend(sheets);
        if let Some(colors) = PlaylistColors::from_files(&files) {
            skin.playlist_colors = colors;
        }
        if let Some(colors) = vis_colors(&files) {
            skin.vis_colors = colors;
        }
        Ok(skin)
    }

    /// Returns the named sheet, or an empty one when the skin lacks it.
    pub fn sheet(&self, name: &str) -> &Sheet {
        self.sheets.get(name).unwrap_or(&self.empty)
    }
}

fn vis_colors(files: &Files) -> Option<[u32; 24]> {
    let (_, bytes) = files.iter().find(|(name, _)| name == "viscolor.txt")?;
    crate::vis::parse_colors(&String::from_utf8_lossy(bytes))
}

fn decode_sheets(files: &Files) -> HashMap<String, Sheet> {
    let mut sheets = HashMap::new();
    for (name, bytes) in files {
        if let Some((stem, ext)) = name.rsplit_once('.')
            && matches!(ext, "bmp" | "png")
            && SHEETS.contains(&stem)
        {
            match Sheet::decode(bytes) {
                Some(sheet) => {
                    sheets.insert(stem.to_string(), sheet);
                }
                None => eprintln!("libreamp: could not decode {name}"),
            }
        }
    }
    sheets
}

/// File name (lowercased, directories stripped) -> contents.
type Files = Vec<(String, Vec<u8>)>;

fn base_name(name: &str) -> String {
    name.rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .to_lowercase()
}

fn read_zip(reader: impl Read + std::io::Seek) -> Result<Files, String> {
    let mut zip = zip::ZipArchive::new(reader).map_err(|e| e.to_string())?;
    let mut files = Vec::new();
    for i in 0..zip.len() {
        let Ok(mut entry) = zip.by_index(i) else {
            continue;
        };
        if entry.is_dir() {
            continue;
        }
        let Ok(name) = entry.name().map(|n| base_name(&n)) else {
            continue;
        };
        let mut bytes = Vec::new();
        if entry.read_to_end(&mut bytes).is_ok() {
            files.push((name, bytes));
        }
    }
    Ok(files)
}

fn read_dir(path: &Path) -> Result<Files, String> {
    let entries = fs::read_dir(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(entries
        .flatten()
        .filter_map(|e| {
            let name = base_name(&e.file_name().to_string_lossy());
            Some((name, fs::read(e.path()).ok()?))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_skin_has_every_sheet() {
        let skin = Skin::base();
        // nums_ex is optional: skins without it use numbers.
        for name in SHEETS.iter().filter(|n| **n != "nums_ex") {
            assert!(
                skin.sheet(name).w > 0,
                "{name} missing from the bundled skin"
            );
        }
        assert_eq!((skin.sheet("main").w, skin.sheet("main").h), (275, 116));
    }
}
