//! The skin collection: every skin loaded is copied into $XDG_DATA_HOME/libreamp/skins (or
//! ~/.local/share/libreamp/skins), so it stays available from the menu even if the original
//! moves, and inside Flatpak doesn't depend on the file dialog's temporary access.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::skin::is_skin;

pub fn dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(base.join("libreamp").join("skins"))
}

/// The collected skins as (name, path), sorted by name.
pub fn list() -> Vec<(String, PathBuf)> {
    let Some(entries) = dir().and_then(|d| fs::read_dir(d).ok()) else {
        return Vec::new();
    };
    let mut skins: Vec<(String, PathBuf)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| is_skin(p))
        .map(|p| (display_name(&p), fs::canonicalize(&p).unwrap_or(p)))
        .collect();
    skins.sort_by_cached_key(|(name, _)| name.to_lowercase());
    skins
}

/// The file name without a .wsz/.zip extension.
fn display_name(path: &Path) -> String {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let lower = name.to_ascii_lowercase();
    match lower.strip_suffix(".wsz").or(lower.strip_suffix(".zip")) {
        Some(stem) => name[..stem.len()].to_string(),
        None => name.into_owned(),
    }
}

/// Adds a just-loaded skin to the collection and returns the path to remember it by: the
/// copy, or the original if copying failed (with a warning).
pub fn keep(path: &Path) -> PathBuf {
    import(path).unwrap_or_else(|e| {
        eprintln!(
            "libreamp: couldn't add {} to the skin collection: {e}",
            path.display()
        );
        path.to_path_buf()
    })
}

/// Copies the skin at `path` into the collection and returns the copy's path; a skin already
/// there, or an identical copy of one, is returned as is.
pub fn import(path: &Path) -> io::Result<PathBuf> {
    let dir = dir().ok_or_else(|| io::Error::other("no data directory"))?;
    import_into(path, &dir)
}

fn import_into(path: &Path, dir: &Path) -> io::Result<PathBuf> {
    let src = fs::canonicalize(path)?;
    fs::create_dir_all(dir)?;
    let dir = fs::canonicalize(dir)?;
    if src.parent() == Some(dir.as_path()) {
        return Ok(src);
    }
    let name = src
        .file_name()
        .ok_or_else(|| io::Error::other("skin has no file name"))?;
    // "Skin.wsz", then "Skin (2).wsz" and so on, unless one of them is this skin already.
    let (stem, ext) = match Path::new(name).extension() {
        Some(ext) if src.is_file() => (
            Path::new(name).file_stem().unwrap_or(name).to_os_string(),
            format!(".{}", ext.to_string_lossy()),
        ),
        _ => (name.to_os_string(), String::new()),
    };
    for n in 1.. {
        let mut candidate = stem.clone();
        if n > 1 {
            candidate.push(format!(" ({n})"));
        }
        candidate.push(&ext);
        let dest = dir.join(candidate);
        if !dest.exists() {
            copy(&src, &dest)?;
            return Ok(dest);
        }
        if same(&src, &dest)? {
            return Ok(dest);
        }
    }
    unreachable!()
}

/// Copies a file, or a directory recursively.
fn copy(src: &Path, dest: &Path) -> io::Result<()> {
    if src.is_dir() {
        fs::create_dir(dest)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            copy(&entry.path(), &dest.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        fs::copy(src, dest).map(|_| ())
    }
}

/// True if two files, or two directory trees, have the same contents.
fn same(a: &Path, b: &Path) -> io::Result<bool> {
    match (a.is_dir(), b.is_dir()) {
        (false, false) => {
            Ok(fs::metadata(a)?.len() == fs::metadata(b)?.len() && fs::read(a)? == fs::read(b)?)
        }
        (true, true) => {
            let names = |d: &Path| -> io::Result<Vec<_>> {
                let mut v: Vec<_> = (fs::read_dir(d)?.map(|e| e.map(|e| e.file_name())))
                    .collect::<io::Result<_>>()?;
                v.sort();
                Ok(v)
            };
            let (na, nb) = (names(a)?, names(b)?);
            if na != nb {
                return Ok(false);
            }
            for n in na {
                if !same(&a.join(&n), &b.join(&n))? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_once_and_keeps_different_skins_apart() {
        let root = std::env::temp_dir().join(format!("libreamp-skins-{}", std::process::id()));
        let (outside, lib) = (root.join("downloads"), root.join("skins"));
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("Cool.wsz"), b"one").unwrap();
        let first = import_into(&outside.join("Cool.wsz"), &lib).unwrap();
        // Importing the same skin again, or the copy itself, reuses the copy.
        let again = import_into(&outside.join("Cool.wsz"), &lib).unwrap();
        let itself = import_into(&first, &lib).unwrap();
        // A different skin with the same name gets its own copy.
        fs::write(outside.join("Cool.wsz"), b"two").unwrap();
        let other = import_into(&outside.join("Cool.wsz"), &lib).unwrap();
        // Unpacked skins are copied as folders.
        fs::create_dir_all(outside.join("Folder skin")).unwrap();
        fs::write(outside.join("Folder skin/main.bmp"), b"x").unwrap();
        let folder = import_into(&outside.join("Folder skin"), &lib).unwrap();
        let folder_again = import_into(&outside.join("Folder skin"), &lib).unwrap();
        let names: Vec<_> = (fs::read_dir(&lib).unwrap())
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        fs::remove_dir_all(&root).unwrap();

        assert_eq!(first.file_name().unwrap(), "Cool.wsz");
        assert_eq!((&again, &itself), (&first, &first));
        assert_eq!(other.file_name().unwrap(), "Cool (2).wsz");
        assert_eq!(folder, folder_again);
        let mut names = names;
        names.sort();
        assert_eq!(names, ["Cool (2).wsz", "Cool.wsz", "Folder skin"]);
    }

    #[test]
    fn names_skins_without_their_extension() {
        assert_eq!(
            display_name(Path::new("/x/Blame - Wired.wsz")),
            "Blame - Wired"
        );
        assert_eq!(display_name(Path::new("/x/Old.ZIP")), "Old");
        assert_eq!(display_name(Path::new("/x/Folder skin")), "Folder skin");
    }
}
