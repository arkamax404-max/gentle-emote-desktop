//! Desktop background opacity and checkbox storage, independent of RGB/senders.
use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
};

/// Reuse the RGB store's absolute user-global directory and root policy.
pub fn global_path() -> Option<PathBuf> {
    crate::background_color::global_path().map(|path| path.with_file_name("background-opacity.txt"))
}

fn valid(opacity: u8, enabled: bool) -> bool {
    opacity <= 100 && (enabled || opacity == 100)
}

fn encode(opacity: u8, enabled: bool) -> String {
    format!("{opacity} {enabled}\n")
}

/// Canonical decimal opacity, one space, lowercase boolean, optional single LF.
fn decode(bytes: &[u8]) -> Option<(u8, bool)> {
    let text = std::str::from_utf8(bytes.strip_suffix(b"\n").unwrap_or(bytes)).ok()?;
    let (opacity, enabled) = text.split_once(' ')?;
    let opacity: u8 = opacity.parse().ok()?;
    let enabled: bool = enabled.parse().ok()?;
    (valid(opacity, enabled) && text == encode(opacity, enabled).trim_end_matches('\n'))
        .then_some((opacity, enabled))
}

/// Read at most eleven bytes (one beyond the maximum valid format).
/// Missing, invalid and inaccessible files default to (55, true), without repair.
pub fn load(path: &Path) -> (u8, bool) {
    if !path.is_absolute() {
        return (55, true);
    }
    let read = || -> io::Result<Vec<u8>> {
        let file = File::open(path)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "opacity store is not a file",
            ));
        }
        let mut bytes = Vec::with_capacity(11);
        file.take(11).read_to_end(&mut bytes)?;
        Ok(bytes)
    };
    read()
        .ok()
        .and_then(|bytes| decode(&bytes))
        .unwrap_or((55, true))
}

/// Validate before creating directories or overwriting this file. Writes are not
/// atomic; callers retain their live selection on any filesystem failure.
pub fn save(path: &Path, opacity: u8, enabled: bool) -> io::Result<()> {
    if !path.is_absolute() || !valid(opacity, enabled) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "opacity store requires an absolute path and opacity 0..=100 (100 when disabled)",
        ));
    }
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "opacity store needs a parent")
    })?;
    fs::create_dir_all(parent)?;
    fs::write(path, encode(opacity, enabled))
}
