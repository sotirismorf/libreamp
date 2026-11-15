//! Loading classic Winamp skins (.wsz zip archives or unpacked directories).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use crate::gfx::Framebuffer;

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
    /// Sheets the skin itself provides, as opposed to ones filled in from the default skin.
    own: HashSet<String>,
    empty: Sheet,
    pub playlist_colors: PlaylistColors,
    pub vis_colors: [u32; 24],
    /// Window shapes from the skin's own region.txt.
    pub regions: Arc<Regions>,
}

/// Which pixels of a window belong to it, from region.txt; the rest are see-through.
#[derive(Clone, Debug)]
pub struct Region {
    w: i32,
    h: i32,
    inside: Vec<bool>,
}

impl Region {
    /// Rasterises polygons with corners on pixel boundaries ("outside" coordinates, as
    /// region.txt uses): a pixel is in if its centre is inside any polygon (even-odd).
    fn from_polygons(polygons: &[Vec<(f32, f32)>], w: i32, h: i32) -> Region {
        let mut inside = vec![false; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                inside[(y * w + x) as usize] = polygons.iter().any(|poly| {
                    let mut c = false;
                    for (i, &(x1, y1)) in poly.iter().enumerate() {
                        let (x2, y2) = poly[(i + 1) % poly.len()];
                        if (y1 > py) != (y2 > py) && px < x1 + (py - y1) * (x2 - x1) / (y2 - y1) {
                            c = !c;
                        }
                    }
                    c
                });
            }
        }
        Region { w, h, inside }
    }

    /// True if (x, y) is part of the window. Beyond the shape's size counts as inside.
    pub fn contains(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return true;
        }
        self.inside[(y * self.w + x) as usize]
    }

    /// Makes the pixels outside the shape transparent.
    pub fn apply(&self, fb: &mut Framebuffer) {
        for y in 0..fb.h.min(self.h) {
            for x in 0..fb.w.min(self.w) {
                if !self.inside[(y * self.w + x) as usize] {
                    fb.px[(y * fb.w + x) as usize] &= 0x00ff_ffff;
                }
            }
        }
    }
}

/// The shaped windows region.txt can describe.
#[derive(Clone, Debug, Default)]
pub struct Regions {
    pub main: Option<Region>,
    pub eq: Option<Region>,
}

impl Regions {
    /// Parses region.txt: sections like [Normal] with NumPoints= (points per polygon) and
    /// PointList= (x,y pairs). As in Winamp, only the first definition of each counts.
    fn parse(text: &str) -> Regions {
        /// NumPoints and PointList of one section, as read.
        #[derive(Default)]
        struct Section {
            counts: Option<Vec<f32>>,
            coords: Option<Vec<f32>>,
        }
        let mut sections: HashMap<String, Section> = HashMap::new();
        let mut current = None;
        for line in text.lines() {
            let line = line.split(';').next().unwrap_or("").trim();
            if let Some(name) = line.strip_prefix('[') {
                current = Some(name.trim_end_matches(']').trim().to_ascii_lowercase());
                continue;
            }
            let (Some(section), Some((key, value))) = (&current, line.split_once('=')) else {
                continue;
            };
            let numbers: Vec<f32> = value
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter_map(|n| n.parse().ok())
                .collect();
            let entry = sections.entry(section.clone()).or_default();
            match key.trim().to_ascii_lowercase().as_str() {
                "numpoints" if entry.counts.is_none() => entry.counts = Some(numbers),
                "pointlist" if entry.coords.is_none() => entry.coords = Some(numbers),
                _ => {}
            }
        }
        let region = |name: &str, w: i32, h: i32| {
            let section = sections.get(name)?;
            let (Some(counts), Some(coords)) = (&section.counts, &section.coords) else {
                return None;
            };
            let mut points = coords.as_chunks::<2>().0.iter().map(|&[x, y]| (x, y));
            let polygons: Vec<Vec<(f32, f32)>> = counts
                .iter()
                .map(|n| points.by_ref().take(*n as usize).collect::<Vec<_>>())
                .filter(|p| p.len() >= 3)
                .collect();
            (!polygons.is_empty()).then(|| Region::from_polygons(&polygons, w, h))
        };
        Regions {
            main: region("normal", crate::sprites::MAIN_W, crate::sprites::MAIN_H),
            eq: region(
                "equalizer",
                crate::ui::eq_window::W,
                crate::ui::eq_window::H,
            ),
        }
    }

    fn from_files(files: &Files) -> Regions {
        files
            .iter()
            .find(|(name, _)| name == "region.txt")
            .map(|(_, bytes)| Regions::parse(&String::from_utf8_lossy(bytes)))
            .unwrap_or_default()
    }
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
    "shufrep", "monoster", "playpaus", "eqmain", "pledit", "gen",
];

impl Skin {
    pub fn base() -> Skin {
        let files = read_zip(std::io::Cursor::new(BASE_SKIN)).expect("bundled base skin is valid");
        let sheets = decode_sheets(&files);
        Skin {
            own: sheets.keys().cloned().collect(),
            sheets,
            empty: Sheet::empty(),
            playlist_colors: PlaylistColors::from_files(&files).unwrap_or_default(),
            vis_colors: vis_colors(&files).expect("bundled base skin has viscolor.txt"),
            regions: Arc::new(Regions::from_files(&files)),
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
        skin.own = sheets.keys().cloned().collect();
        skin.sheets.extend(sheets);
        if let Some(colors) = PlaylistColors::from_files(&files) {
            skin.playlist_colors = colors;
        }
        if let Some(colors) = vis_colors(&files) {
            skin.vis_colors = colors;
        }
        // A shape belongs to the skin that drew it; never borrowed from the default skin.
        skin.regions = Arc::new(Regions::from_files(&files));
        Ok(skin)
    }

    /// Whether the skin itself has the named sheet (rather than borrowing the default skin's).
    pub fn has_own(&self, name: &str) -> bool {
        self.own.contains(name)
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

    #[test]
    fn parses_region_txt() {
        let text = "; a comment\r\n[Normal]\r\nNumPoints=4, 4 ; two squares\r\n\
            PointList=0,0, 10,0, 10,10, 0,10,  20,0 30,0 30,10 20,10\r\n\
            [Normal]\r\nNumPoints=4\r\nPointList=0,0, 275,0, 275,116, 0,116\r\n";
        let regions = Regions::parse(text);
        let main = regions.main.expect("main region");
        assert!(main.contains(0, 0) && main.contains(9, 9) && main.contains(25, 5));
        assert!(!main.contains(10, 0) && !main.contains(15, 5) && !main.contains(30, 5));
        assert!(!main.contains(5, 10));
        assert!(regions.eq.is_none());
        // The default skin's region.txt is all comments.
        assert!(Skin::base().regions.main.is_none());
    }

    #[test]
    fn knows_which_sheets_a_skin_brings() {
        let dir = std::env::temp_dir().join(format!("libreamp-skin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        image::RgbImage::new(275, 116)
            .save(dir.join("MAIN.BMP"))
            .unwrap();
        let skin = Skin::load(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        let skin = skin.unwrap();
        assert!(skin.has_own("main"));
        // Missing sheets are filled in from the default skin, but aren't the skin's own.
        assert!(!skin.has_own("gen"));
        assert!(skin.sheet("gen").w > 0);
        assert!(Skin::base().has_own("gen"));
    }
}
