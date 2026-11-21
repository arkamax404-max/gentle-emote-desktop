use gentle_emote_desktop::background_opacity::{load, save};
use std::fs;

#[test]
fn opacity_and_checkbox_survive_reload_without_touching_rgb() {
    let root = std::env::temp_dir().join(format!("gentle-opacity-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let path = root.join("nested/background-opacity.txt");
    assert_eq!(load(&path), (55, true));
    assert!(!path.exists());
    for (opacity, enabled) in [(0, true), (37, true), (100, true), (100, false)] {
        save(&path, opacity, enabled).unwrap();
        assert_eq!(load(&path), (opacity, enabled));
    }
    let rgb = path.with_file_name("background-color.txt");
    fs::write(&rgb, "#123456\n").unwrap();
    save(&path, 20, true).unwrap();
    assert_eq!(fs::read(&rgb).unwrap(), b"#123456\n");
    for invalid in [
        "101 true\n",
        "50 false\n",
        "garbage",
        "55 true\nextra",
        "055 true\n",
        "+55 true\n",
        "55  true\n",
        "55 True\n",
        "55 true\r\n",
        "55 true\n\n",
        " 55 true\n",
        "55 true ",
    ] {
        fs::write(&path, invalid).unwrap();
        assert_eq!(load(&path), (55, true));
        assert_eq!(fs::read_to_string(&path).unwrap(), invalid);
    }
    let oversized = vec![b'x'; 65536];
    fs::write(&path, &oversized).unwrap();
    assert_eq!(load(&path), (55, true));
    assert_eq!(fs::read(&path).unwrap(), oversized);
    for text in ["0 true", "100 true", "100 false"] {
        fs::write(&path, text).unwrap();
        assert_ne!(load(&path), (55, true), "single LF is optional");
    }
    let absent = root.join("must-not-create/background-opacity.txt");
    assert!(save(&absent, 101, true).is_err());
    assert!(save(&absent, 55, false).is_err());
    assert!(!absent.parent().unwrap().exists());
    assert_eq!(
        gentle_emote_desktop::background_opacity::global_path(),
        gentle_emote_desktop::background_color::global_path()
            .map(|path| path.with_file_name("background-opacity.txt"))
    );
    fs::write(&path, "55 true\n").unwrap();
    for (opacity, enabled) in [(101, true), (50, false)] {
        assert!(save(&path, opacity, enabled).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "55 true\n");
    }
    assert_eq!(load(&root), (55, true));
    assert!(save(&root, 55, true).is_err());
    assert_eq!(load(std::path::Path::new("relative")), (55, true));
    assert!(save(std::path::Path::new("relative"), 55, true).is_err());
    fs::remove_dir_all(root).unwrap();
}
