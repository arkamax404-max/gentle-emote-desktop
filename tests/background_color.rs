use gentle_emote_desktop::background_color::{decode, encode, load, path_from_root, save};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "gentle-rgb-store-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn strict_codec_and_roundtrip() {
    for rgb in [[0; 3], [255; 3], [32, 128, 224]] {
        let text = encode(rgb);
        assert_eq!(decode(text.as_bytes()), Some(rgb));
        assert_eq!(decode(text.trim_end_matches('\n').as_bytes()), Some(rgb));
    }
    assert_eq!(encode([32, 128, 224]), "#2080e0\n");
    for bad in [
        "",
        "#ABCDEF",
        "2080e0",
        " #2080e0",
        "#2080e0\r\n",
        "#2080e0\n\n",
        "#2080eg",
        "#2080e0 ",
        "#2080e00",
        "#é80e0",
    ] {
        assert_eq!(decode(bad.as_bytes()), None, "{bad:?}");
    }
    let scratch = Scratch::new();
    let path = scratch.0.join("nested/background-color.txt");
    save(&path, [32, 128, 224]).unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"#2080e0\n");
    assert_eq!(load(&path), [32, 128, 224]);
    save(&path, [255; 3]).unwrap();
    assert_eq!(load(&path), [255; 3]);
}

#[test]
fn invalid_missing_oversized_and_unreadable_are_black_without_repair() {
    let scratch = Scratch::new();
    let path = scratch.0.join("background-color.txt");
    assert_eq!(load(&path), [0; 3]);
    assert!(!path.exists());
    for bytes in [
        b"invalid".to_vec(),
        [b"#ffffff\n".as_slice(), &vec![b'x'; 65536]].concat(),
    ] {
        fs::write(&path, &bytes).unwrap();
        assert_eq!(load(&path), [0; 3]);
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
    assert_eq!(
        load(&scratch.0),
        [0; 3],
        "directory cannot be read as a store"
    );
    assert!(save(&scratch.0, [32, 128, 224]).is_err());
    let blocked = path.join("background-color.txt");
    assert_eq!(load(&blocked), [0; 3]);
    assert!(save(&blocked, [32, 128, 224]).is_err());
}

#[test]
fn config_root_must_be_absolute_and_never_uses_project_fallback() {
    for root in ["", ".", "relative/config"] {
        assert_eq!(path_from_root(std::path::Path::new(root)), None);
    }
    let root = std::env::temp_dir();
    assert_eq!(
        path_from_root(&root),
        Some(root.join("gentle-emote-desktop/background-color.txt"))
    );
    assert_eq!(
        load(std::path::Path::new("relative/background-color.txt")),
        [0; 3]
    );
    assert!(
        save(
            std::path::Path::new("relative/background-color.txt"),
            [255; 3]
        )
        .is_err()
    );
}
