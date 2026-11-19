//! Dedicated desktop background RGB storage, independent of sender settings.
use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
};

/// Resolve only a standard absolute user configuration root, never the project.
/// An explicitly set invalid root disables persistence rather than falling back.
pub fn global_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let root = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let root = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|root| root.is_absolute())
        .map(|root| root.join("Library/Application Support"));
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let root = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(root) => Some(PathBuf::from(root)),
        None => std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|root| root.is_absolute())
            .map(|root| root.join(".config")),
    };
    root.and_then(|root| path_from_root(&root))
}

pub fn path_from_root(root: &Path) -> Option<PathBuf> {
    root.is_absolute()
        .then(|| root.join("gentle-emote-desktop/background-color.txt"))
}

/// Strict lowercase #rrggbb with at most one trailing LF; no other whitespace.
pub fn decode(bytes: &[u8]) -> Option<[u8; 3]> {
    let bytes = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    if bytes.len() != 7 || bytes[0] != b'#' {
        return None;
    }
    fn nibble(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            _ => None,
        }
    }
    let mut rgb = [0; 3];
    for (channel, pair) in rgb.iter_mut().zip(bytes[1..].as_chunks::<2>().0) {
        *channel = nibble(pair[0])? * 16 + nibble(pair[1])?;
    }
    Some(rgb)
}

pub fn encode(rgb: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}\n", rgb[0], rgb[1], rgb[2])
}

/// Read at most nine bytes (one beyond the maximum valid format).
/// Missing, invalid and inaccessible files are black and are never repaired here.
pub fn load(path: &Path) -> [u8; 3] {
    if !path.is_absolute() {
        return [0; 3];
    }
    let read = || -> io::Result<Vec<u8>> {
        let file = File::open(path)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "RGB store is not a file",
            ));
        }
        let mut bytes = Vec::with_capacity(9);
        file.take(9).read_to_end(&mut bytes)?;
        Ok(bytes)
    };
    read()
        .ok()
        .and_then(|bytes| decode(&bytes))
        .unwrap_or([0; 3])
}

/// Write only this RGB file. This simple overwrite is not an atomic replace.
/// Callers keep their live color if any filesystem operation fails.
pub fn save(path: &Path, rgb: [u8; 3]) -> io::Result<()> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "RGB store path must be absolute",
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "RGB store needs a parent"))?;
    fs::create_dir_all(parent)?;
    fs::write(path, encode(rgb))
}
