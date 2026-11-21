#[cfg(unix)]
use gentle_emote_desktop::appearance::{Completion, SaveResult};
use gentle_emote_desktop::appearance::{Patch, Snapshot, Status, Store, resolve_agent_dir};
use std::ffi::OsStr;
use std::{fs, path::PathBuf};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "emote-appearance-{}-{sequence}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        Self(fs::canonicalize(root).unwrap())
    }
    fn store(&self) -> Store {
        Store::new(self.0.join("agent")).unwrap()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn platform_status_contract_classifies_unsupported_without_changing_io_errors() {
    use std::io::{Error, ErrorKind};
    assert_eq!(
        Status::from_error(&Error::new(ErrorKind::Unsupported, "unsupported platform")).code(),
        "unsupported-platform"
    );
    for kind in [
        ErrorKind::PermissionDenied,
        ErrorKind::NotFound,
        ErrorKind::Other,
    ] {
        assert_eq!(
            Status::from_error(&Error::new(kind, "ordinary I/O failure")),
            Status::IoError
        );
    }
}

#[cfg(not(unix))]
#[test]
fn platform_status_contract_store_leaves_filesystem_unchanged() {
    let scratch = Scratch::new();
    let store = scratch.store();
    for report in [
        store.load_report(),
        store.patch_report(0, Patch::Rgb([1, 2, 3])),
    ] {
        assert_eq!(report.status, Status::UnsupportedPlatform);
        assert_eq!(
            report.error.unwrap().kind(),
            std::io::ErrorKind::Unsupported
        );
        assert!(report.snapshot.is_none());
        assert!(report.cleanup_error.is_none());
    }
    assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 0);
    fs::create_dir_all(store.path().parent().unwrap()).unwrap();
    let original = b"existing invalid source must not be parsed or overwritten";
    fs::write(store.path(), original).unwrap();
    for report in [
        store.load_report(),
        store.patch_report(0, Patch::Rgb([1, 2, 3])),
    ] {
        assert_eq!(report.status, Status::UnsupportedPlatform);
        assert_eq!(
            report.error.unwrap().kind(),
            std::io::ErrorKind::Unsupported
        );
        assert!(report.snapshot.is_none());
        assert!(report.cleanup_error.is_none());
    }
    assert_eq!(fs::read(store.path()).unwrap(), original);
    let entries: Vec<_> = fs::read_dir(store.path().parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(entries, [OsStr::new("appearance.json")]);
    assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 1);
    assert_eq!(fs::read_dir(scratch.0.join("agent")).unwrap().count(), 1);
    assert_eq!(
        fs::read_dir(scratch.0.join("agent/extensions"))
            .unwrap()
            .count(),
        1
    );
}

/// Shared literal-fixture compatibility, not independent verification or a
/// concurrent sender/Rust run. SHA-256 values describe the decoded input bytes.
#[cfg(unix)]
#[test]
fn peer_corpus_v1_public_store_contract() {
    use std::os::unix::fs::DirBuilderExt;
    let cases = include!("fixtures/appearance/peer-corpus/manifest.rs");
    assert_eq!(cases.len(), 12);
    for (name, raw, expected, sha256) in cases {
        let scratch = Scratch::new();
        let store = scratch.store();
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(store.path().parent().unwrap())
            .unwrap();
        fs::write(store.path(), raw).unwrap();
        if *name == "leading-bom" {
            assert!(raw.starts_with(b"\xef\xbb\xbf"));
        }
        let read = store.load_report();
        assert_eq!(read.status, *expected, "{name}: {:?}", read.error);
        assert!(read.cleanup_error.is_none(), "{name}");
        assert_eq!(fs::read(store.path()).unwrap(), *raw, "read changed {name}");
        let patch = store.patch_report(0, Patch::Rgb([32, 128, 224]));
        if *expected == Status::Ready {
            let original = read.snapshot.unwrap();
            assert_eq!(original.revision, 0);
            assert_eq!(patch.status, Status::Saved, "{name}: {:?}", patch.error);
            assert!(patch.error.is_none());
            let saved = patch.snapshot.as_ref().unwrap();
            assert_eq!((saved.revision, saved.rgb), (1, [32, 128, 224]));
            assert_eq!(
                (saved.opacity, saved.transparent),
                (original.opacity, original.transparent)
            );
            // Expected tree differs only in the requested RGB and revision.
            // Snapshot equality includes unknown nested keys/values semantically.
            // The original raw bytes already reached Store unchanged above.
            let expected_saved = std::str::from_utf8(raw)
                .unwrap()
                .replacen("\"revision\":0", "\"revision\":1", 1)
                .replacen(
                    "\"backgroundRgb\":[0,0,0]",
                    "\"backgroundRgb\":[32,128,224]",
                    1,
                );
            assert_eq!(
                *saved,
                Snapshot::parse(expected_saved.as_bytes()).unwrap(),
                "unknown subtree changed: {name}"
            );
            let confirmed = store.load_report();
            assert_eq!(confirmed.status, Status::Ready);
            assert_eq!(confirmed.snapshot.as_ref(), Some(saved));
            let committed = fs::read(store.path()).unwrap();
            let conflict = store.patch_report(0, Patch::Rgb([9, 8, 7]));
            assert_eq!(conflict.status, Status::Conflict);
            assert_eq!(conflict.snapshot.as_ref(), Some(saved));
            assert!(conflict.error.is_none());
            assert!(conflict.cleanup_error.is_none());
            assert_eq!(
                fs::read(store.path()).unwrap(),
                committed,
                "stale CAS changed {name}"
            );
            println!("{name}: ready -> saved(rev1) -> conflict; input-sha256={sha256}");
        } else {
            assert!(read.snapshot.is_none());
            assert!(read.error.is_some());
            assert_eq!(patch.status, Status::Invalid, "{name}: {:?}", patch.error);
            assert!(patch.snapshot.is_none());
            assert!(patch.error.is_some());
            assert_eq!(
                fs::read(store.path()).unwrap(),
                *raw,
                "invalid patch changed bytes/revision: {name}"
            );
            assert_eq!(store.load_report().status, Status::Invalid);
            println!("{name}: invalid -> invalid; input-sha256={sha256}");
        }
        assert!(patch.cleanup_error.is_none(), "{name}");
        let entries: Vec<_> = fs::read_dir(store.path().parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(
            entries,
            [OsStr::new("appearance.json")],
            "lock/temp/alternate store leak: {name}"
        );
        assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 1);
        assert_eq!(fs::read_dir(scratch.0.join("agent")).unwrap().count(), 1);
        assert_eq!(
            fs::read_dir(scratch.0.join("agent/extensions"))
                .unwrap()
                .count(),
            1
        );
    }
}

#[cfg(unix)]
#[test]
fn l17_required_desktop_fields_reject_missing_leaves_without_writes() {
    let scratch = Scratch::new();
    let store = scratch.store();
    store.patch(0, Patch::Rgb([0, 0, 0])).unwrap();
    for (field, source) in [
        (
            "backgroundRgb",
            include_bytes!("fixtures/appearance/missing-rgb.json").as_slice(),
        ),
        (
            "backgroundOpacity",
            include_bytes!("fixtures/appearance/missing-opacity.json").as_slice(),
        ),
        (
            "transparentBackground",
            include_bytes!("fixtures/appearance/missing-transparent.json").as_slice(),
        ),
    ] {
        assert!(
            Snapshot::parse(source).is_err(),
            "missing {field} must be invalid"
        );
        fs::write(store.path(), source).unwrap();
        assert_eq!(store.load_report().status, Status::Invalid);
        assert!(store.patch(7, Patch::Rgb([1, 2, 3])).is_err());
        assert_eq!(
            fs::read(store.path()).unwrap(),
            source,
            "reject preserves exact source and revision 7"
        );
    }
}

#[cfg(unix)]
#[test]
fn l17_ancestor_guards_reject_symlinks_and_unsafe_agent_directories() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let scratch = Scratch::new();
    let real = scratch.0.join("real");
    fs::create_dir(&real).unwrap();
    let alias = scratch.0.join("alias");
    symlink(&real, &alias).unwrap();
    let aliased = Store::new(alias.join("agent")).unwrap();
    assert_eq!(
        aliased.load_report().status,
        Status::UnsafePath,
        "ancestor symlink must not become missing defaults"
    );
    assert!(aliased.patch(0, Patch::Rgb([1, 2, 3])).is_err());
    assert_eq!(fs::read_dir(&real).unwrap().count(), 0);
    let agent = scratch.0.join("unsafe-agent");
    fs::create_dir(&agent).unwrap();
    fs::set_permissions(&agent, fs::Permissions::from_mode(0o770)).unwrap();
    let unsafe_store = Store::new(agent.clone()).unwrap();
    assert_eq!(unsafe_store.load_report().status, Status::UnsafePath);
    assert_eq!(
        unsafe_store.patch_report(0, Patch::Rgb([1, 2, 3])).status,
        Status::UnsafePath
    );
    assert_eq!(
        fs::metadata(&agent).unwrap().permissions().mode() & 0o777,
        0o770,
        "never chmod existing roots"
    );
    assert_eq!(fs::read_dir(&agent).unwrap().count(), 0);
    let blocker = scratch.0.join("file-ancestor");
    fs::write(&blocker, b"keep").unwrap();
    let blocked = Store::new(blocker.join("agent")).unwrap();
    assert!(blocked.load().is_err());
    assert!(blocked.patch(0, Patch::Rgb([1, 2, 3])).is_err());
    assert_eq!(fs::read(blocker).unwrap(), b"keep");
}

#[cfg(unix)]
#[test]
fn l17_posix_guards_allow_reads_but_reject_group_or_other_writes() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new();
    let store = scratch.store();
    store.patch(0, Patch::Rgb([1, 2, 3])).unwrap();
    let agent = scratch.0.join("agent");
    fs::set_permissions(&agent, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(store.path(), fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        store.load().unwrap().revision,
        1,
        "same-user readable source is allowed"
    );
    let before = fs::read(store.path()).unwrap();
    let extensions = agent.join("extensions");
    fs::set_permissions(&extensions, fs::Permissions::from_mode(0o775)).unwrap();
    assert!(store.load().is_err());
    assert!(store.patch(1, Patch::Rgb([4, 5, 6])).is_err());
    assert_eq!(fs::read(store.path()).unwrap(), before);
}

#[test]
fn container_depth_contract_accepts_64_rejects_65() {
    for location in ["unknown", "desktop", "image"] {
        for shape in ["array", "object", "mixed"] {
            for depth in [65, 64] {
                let ancestors = if location == "desktop" { 2 } else { 1 };
                let mut nested = if shape == "array" { "[]" } else { "{}" }.to_owned();
                for level in 1..(depth - ancestors) {
                    nested = if shape == "array" || (shape == "mixed" && level % 2 == 0) {
                        format!("[{nested}]")
                    } else {
                        format!(r#"{{"next":{nested}}}"#)
                    };
                }
                let leaves = r#""backgroundRgb":[0,0,0],"backgroundOpacity":65,"transparentBackground":true"#;
                let desktop_extra = if location == "desktop" {
                    format!(",\"future\":{nested}")
                } else {
                    String::new()
                };
                let root_extra = if location != "desktop" {
                    format!(",\"{location}\":{nested}")
                } else {
                    String::new()
                };
                let source = format!(
                    r#"{{"version":1,"revision":7,"desktop":{{{leaves}{desktop_extra}}}{root_extra}}}"#
                );
                let parsed = Snapshot::parse(source.as_bytes());
                if depth == 65 {
                    let error =
                        parsed.expect_err(&format!("must reject depth65: {location}/{shape}"));
                    assert_eq!(error.to_string(), "JSON container depth exceeds 64");
                } else {
                    assert_eq!(
                        parsed.unwrap().revision,
                        7,
                        "accept depth64: {location}/{shape}"
                    );
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    let scratch = Scratch::new();
                    let store = scratch.store();
                    fs::DirBuilder::new()
                        .recursive(true)
                        .mode(0o700)
                        .create(store.path().parent().unwrap())
                        .unwrap();
                    fs::write(store.path(), source.as_bytes()).unwrap();
                    let read = store.load_report();
                    let patch = store.patch_report(7, Patch::Rgb([12, 34, 56]));
                    if depth == 65 {
                        assert_eq!(read.status, Status::Invalid);
                        assert_eq!(patch.status, Status::Invalid);
                        assert!(patch.snapshot.is_none());
                        assert_eq!(
                            patch.error.as_ref().unwrap().to_string(),
                            "JSON container depth exceeds 64"
                        );
                        assert_eq!(
                            fs::read(store.path()).unwrap(),
                            source.as_bytes(),
                            "reject preserves bytes/revision7"
                        );
                    } else {
                        assert_eq!(read.status, Status::Ready);
                        assert_eq!(patch.status, Status::Saved);
                        assert!(patch.error.is_none());
                        let expected = source
                            .replace("\"revision\":7", "\"revision\":8")
                            .replace("\"backgroundRgb\":[0,0,0]", "\"backgroundRgb\":[12,34,56]");
                        assert_eq!(
                            patch.snapshot.unwrap(),
                            Snapshot::parse(expected.as_bytes()).unwrap()
                        );
                        assert_eq!(
                            store.load().unwrap(),
                            Snapshot::parse(expected.as_bytes()).unwrap()
                        );
                        let transparency = store.patch_report(
                            8,
                            Patch::Transparency {
                                opacity: 100,
                                enabled: false,
                            },
                        );
                        assert_eq!(transparency.status, Status::Saved);
                        assert!(
                            transparency.error.is_none() && transparency.cleanup_error.is_none()
                        );
                        let expected = expected
                            .replace("\"revision\":8", "\"revision\":9")
                            .replace("\"backgroundOpacity\":65", "\"backgroundOpacity\":100")
                            .replace(
                                "\"transparentBackground\":true",
                                "\"transparentBackground\":false",
                            );
                        assert_eq!(
                            store.load().unwrap(),
                            Snapshot::parse(expected.as_bytes()).unwrap()
                        );
                    }
                    assert!(patch.cleanup_error.is_none());
                    let entries: Vec<_> = fs::read_dir(store.path().parent().unwrap())
                        .unwrap()
                        .map(|entry| entry.unwrap().file_name())
                        .collect();
                    assert_eq!(
                        entries,
                        [OsStr::new("appearance.json")],
                        "no lock/temp after depth {depth}"
                    );
                }
            }
        }
    }
}

#[test]
fn strict_whole_document_validation() {
    for extra in [
        r#""x":{"a":1,"a":2}"#,
        r#""x":[1.0]"#,
        r#""x":{"n":1e0}"#,
        r#""x":9007199254740992"#,
        r#""x":-9007199254740992"#,
        r#""desktop":{}"#,
    ] {
        let text = format!(
            r#"{{"version":1,"revision":0,"desktop":{{"backgroundRgb":[0,0,0],"backgroundOpacity":55,"transparentBackground":true}},{extra}}}"#
        );
        assert!(Snapshot::parse(text.as_bytes()).is_err(), "{text}");
    }
    for n in ["-9007199254740991", "9007199254740991", "-0"] {
        let text = format!(
            r#"{{"version":1,"revision":0,"desktop":{{"backgroundRgb":[0,255,0],"backgroundOpacity":55,"transparentBackground":true}},"image":{{"n":{n}}}}}"#
        );
        let s = Snapshot::parse(text.as_bytes()).unwrap();
        assert_eq!(s.revision, 0);
        assert_eq!((s.opacity, s.transparent), (55, true));
    }
    for text in [
        r#"{"version":2,"revision":0,"desktop":{"backgroundRgb":[0,0,0],"backgroundOpacity":55,"transparentBackground":true}}"#,
        r#"{"version":1,"revision":0,"desktop":{"backgroundRgb":[0,0,0],"backgroundOpacity":55,"transparentBackground":false}}"#,
        r#"{"version":1,"revision":-1,"desktop":{"backgroundRgb":[0,0,0],"backgroundOpacity":55,"transparentBackground":true}}"#,
    ] {
        assert!(Snapshot::parse(text.as_bytes()).is_err());
    }
    assert!(Snapshot::parse(&vec![b' '; 65537]).is_err());
    assert!(Snapshot::parse(b"\xff").is_err());
    for n in [
        "1.0",
        "1e0",
        "1E+0",
        "01",
        "+1",
        "-",
        "9007199254740992",
        "-9007199254740992",
    ] {
        let text = format!(
            r#"{{"version":1,"revision":0,"desktop":{{"backgroundRgb":[0,0,0],"backgroundOpacity":55,"transparentBackground":true}},"image":[{{"n":{n}}}]}}"#
        );
        assert!(Snapshot::parse(text.as_bytes()).is_err(), "{text}");
    }
    for duplicate in [r#""x":1,"\u0078":2"#, r#""a":[{"x":1,"\u0078":2}]"#] {
        let text = format!(
            r#"{{"version":1,"revision":0,"desktop":{{"backgroundRgb":[0,0,0],"backgroundOpacity":55,"transparentBackground":true}},{duplicate}}}"#
        );
        assert!(Snapshot::parse(text.as_bytes()).is_err());
    }
    for (fixture, valid) in [
        (
            include_bytes!("fixtures/appearance/valid-unknown.json").as_slice(),
            true,
        ),
        (
            include_bytes!("fixtures/appearance/duplicate-nested.json").as_slice(),
            false,
        ),
        (
            include_bytes!("fixtures/appearance/fraction-nested.json").as_slice(),
            false,
        ),
    ] {
        assert_eq!(Snapshot::parse(fixture).is_ok(), valid);
    }
}

#[cfg(unix)]
#[test]
fn contract_revision_unicode_rejects_bom_and_unpaired_surrogates() {
    let scratch = Scratch::new();
    let store = scratch.store();
    store.patch(0, Patch::Rgb([0, 0, 0])).unwrap();
    for source in [
        include_bytes!("fixtures/appearance/invalid-bom.json").as_slice(),
        include_bytes!("fixtures/appearance/invalid-surrogate-value.json").as_slice(),
        include_bytes!("fixtures/appearance/invalid-surrogate-key.json").as_slice(),
        br#"{"version":1,"revision":7,"desktop":{"backgroundRgb":[0,0,0],"backgroundOpacity":55,"transparentBackground":true},"\ud800":1}"#,
        br#"{"version":1,"revision":7,"desktop":{"backgroundRgb":[0,0,0],"backgroundOpacity":55,"transparentBackground":true},"x":"\udfff"}"#,
        br#"{"version":1,"revision":7,"desktop":{"backgroundRgb":[0,0,0],"backgroundOpacity":55,"transparentBackground":true},"x":"\ud800x\udc00"}"#,
        br#"{"version":1,"revision":7,"desktop":{"backgroundRgb":[0,0,0],"backgroundOpacity":55,"transparentBackground":true},"x":"\ud800\ud800"}"#,
        br#"{"version":1,"revision":7,"desktop":{"backgroundRgb":[0,0,0],"backgroundOpacity":55,"transparentBackground":true},"x":"\udc00\ud800"}"#,
    ] {
        let error=Snapshot::parse(source).unwrap_err().to_string();
        assert!(error.contains(if source.starts_with(b"\xef\xbb\xbf") {"UTF-8 BOM"} else {"unpaired UTF-16"}), "wrong rejection: {error}");
        fs::write(store.path(), source).unwrap();
        assert!(store.load().is_err());
        assert!(store.patch(7, Patch::Rgb([1, 2, 3])).is_err());
        assert_eq!(
            fs::read(store.path()).unwrap(),
            source,
            "rejection preserves bytes and revision 7"
        );
    }
    let valid = include_bytes!("fixtures/appearance/valid-surrogate-pairs.json");
    assert_eq!(Snapshot::parse(valid).unwrap().revision, 7);
    fs::write(store.path(), valid).unwrap();
    store.patch(7, Patch::Rgb([1, 2, 3])).unwrap();
    assert_eq!(store.load().unwrap().revision, 8);
    assert!(
        fs::read_to_string(store.path())
            .unwrap()
            .contains("pair 𐀀 and 😀")
    );
}

#[cfg(unix)]
#[test]
fn contract_revision_raw_dot_components_fail_without_normalization_or_fallback() {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let scratch = Scratch::new();
    let valid = scratch.0.join("valid-agent");
    let home = scratch.0.as_os_str();
    for suffix in [
        "/./agent",
        "/../agent",
        "/agent/.",
        "/agent/..",
        "/agent//./nested",
    ] {
        let mut raw = scratch.0.as_os_str().to_os_string();
        raw.push(suffix);
        assert!(
            resolve_agent_dir(Some(&raw), Some(valid.as_os_str()), Some(home)).is_err(),
            "CLI must reject raw {suffix}"
        );
        assert!(
            resolve_agent_dir(None, Some(&raw), Some(home)).is_err(),
            "environment must reject raw {suffix}"
        );
        assert!(Store::new(PathBuf::from(&raw)).is_err());
        for cli in [true, false] {
            let mut command =
                std::process::Command::new(env!("CARGO_BIN_EXE_gentle-emote-desktop"));
            if cli {
                command.arg("--agent-dir").arg(&raw);
            }
            let out = command
                .env(
                    "PI_CODING_AGENT_DIR",
                    if cli {
                        valid.as_os_str()
                    } else {
                        raw.as_os_str()
                    },
                )
                .output()
                .unwrap();
            assert_eq!(out.status.code(), Some(2));
            assert_eq!(out.stdout, b"");
            assert_eq!(
                out.stderr,
                b"Error: agent directory must not contain '.' or '..' components\n"
            );
        }
    }
    let mut non_utf8 = scratch.0.as_os_str().as_bytes().to_vec();
    non_utf8.extend_from_slice(b"/\xff/./agent");
    assert!(Store::new(PathBuf::from(std::ffi::OsString::from_vec(non_utf8))).is_err());
    let ordinary = scratch.0.join(".pi/.../agent../back\\slash");
    assert_eq!(
        resolve_agent_dir(Some(ordinary.as_os_str()), None, None).unwrap(),
        ordinary
    );
    assert!(Store::new(ordinary).is_ok());
    assert_eq!(
        fs::read_dir(&scratch.0).unwrap().count(),
        0,
        "invalid paths never create storage"
    );
}

#[test]
fn resolver_is_absolute_and_never_falls_back_from_explicit_invalid_input() {
    let home_path = std::env::temp_dir().join("appearance-home");
    let env_path = std::env::temp_dir().join("appearance-env");
    let cli_path = std::env::temp_dir().join("appearance-cli");
    let home = home_path.as_os_str();
    assert_eq!(
        resolve_agent_dir(None, None, Some(home)).unwrap(),
        home_path.join(".pi/agent")
    );
    assert_eq!(
        resolve_agent_dir(None, Some(env_path.as_os_str()), Some(home)).unwrap(),
        env_path
    );
    assert_eq!(
        resolve_agent_dir(
            Some(cli_path.as_os_str()),
            Some(OsStr::new("bad")),
            Some(home)
        )
        .unwrap(),
        cli_path
    );
    for value in ["", "relative"] {
        assert!(
            resolve_agent_dir(
                Some(OsStr::new(value)),
                Some(env_path.as_os_str()),
                Some(home)
            )
            .is_err()
        );
        assert!(resolve_agent_dir(None, Some(OsStr::new(value)), Some(home)).is_err());
    }
    assert!(resolve_agent_dir(None, None, None).is_err());
}

#[cfg(unix)]
#[test]
fn first_save_cas_and_unknown_fields_are_preserved() {
    let scratch = Scratch::new();
    let store = scratch.store();
    assert_eq!(store.load().unwrap().revision, 0);
    assert!(
        !store.path().parent().unwrap().exists(),
        "load never creates directories"
    );
    assert!(matches!(
        store.patch(0, Patch::Rgb([32, 128, 224])).unwrap(),
        SaveResult::Committed(_)
    ));
    let before = fs::read(store.path()).unwrap();
    let conflict = store
        .patch(
            0,
            Patch::Transparency {
                opacity: 100,
                enabled: false,
            },
        )
        .unwrap();
    assert!(
        matches!(conflict, SaveResult::Conflict(s) if s.revision == 1 && s.rgb == [32,128,224])
    );
    assert_eq!(fs::read(store.path()).unwrap(), before);
    assert_eq!(store.load().unwrap().revision, 1);
    let text = String::from_utf8(before).unwrap().replace(
        "\"version\":1",
        "\"image\":{\"opaque\":[1,true,null]},\"version\":1",
    );
    fs::write(store.path(), text).unwrap();
    store
        .patch(
            1,
            Patch::Transparency {
                opacity: 100,
                enabled: true,
            },
        )
        .unwrap();
    let after = fs::read_to_string(store.path()).unwrap();
    assert!(after.contains("\"image\":{\"opaque\":[1,true,null]}"));
    assert_eq!(store.load().unwrap().revision, 2);
    assert_eq!(store.load().unwrap().rgb, [32, 128, 224]);
    let before = fs::read(store.path()).unwrap();
    assert!(
        store
            .patch(
                2,
                Patch::Transparency {
                    opacity: 50,
                    enabled: false
                }
            )
            .is_err()
    );
    assert_eq!(fs::read(store.path()).unwrap(), before);
    assert_eq!(store.load().unwrap().revision, 2);
}

#[cfg(unix)]
#[test]
fn contention_has_one_cas_winner_and_never_replays() {
    let scratch = Scratch::new();
    let store = scratch.store();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let store = store.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.patch(0, Patch::Rgb([i, 0, 0])).unwrap()
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, SaveResult::Committed(_)))
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, SaveResult::Conflict(_)))
            .count(),
        7
    );
    assert_eq!(store.load().unwrap().revision, 1);
    assert!(!store.path().with_file_name("appearance.json.lock").exists());
}

#[cfg(unix)]
#[test]
fn invalid_sources_permissions_and_symlinks_are_never_overwritten() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let scratch = Scratch::new();
    let store = scratch.store();
    store.patch(0, Patch::Rgb([1, 2, 3])).unwrap();
    for text in [
        r#"{"version":2}"#,
        r#"{"version":1,"revision":9007199254740991,"desktop":{"backgroundRgb":[1,2,3],"backgroundOpacity":55,"transparentBackground":true}}"#,
        r#"{"version":1,"revision":1,"desktop":{"backgroundRgb":[1,2,3],"backgroundOpacity":55,"transparentBackground":true},"image":{"x":1.5}}"#,
    ] {
        fs::write(store.path(), text).unwrap();
        assert!(
            store
                .patch(9007199254740991, Patch::Rgb([0, 0, 0]))
                .is_err()
        );
        assert_eq!(fs::read_to_string(store.path()).unwrap(), text);
    }
    fs::set_permissions(store.path(), fs::Permissions::from_mode(0o664)).unwrap();
    assert!(store.load().is_err());
    assert!(store.patch(0, Patch::Rgb([0, 0, 0])).is_err());
    let other = scratch.0.join("other");
    fs::write(&other, b"keep").unwrap();
    let link_store = Store::new(scratch.0.join("links")).unwrap();
    fs::create_dir_all(link_store.path().parent().unwrap()).unwrap();
    symlink(&other, link_store.path()).unwrap();
    assert!(link_store.load().is_err());
    assert!(link_store.patch(0, Patch::Rgb([0, 0, 0])).is_err());
    assert_eq!(fs::read(other).unwrap(), b"keep");
}

#[cfg(unix)]
#[test]
fn existing_lock_is_never_stolen_and_has_exact_private_protocol() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new();
    let store = scratch.store();
    store.patch(0, Patch::Rgb([1, 2, 3])).unwrap();
    let before = fs::read(store.path()).unwrap();
    assert_eq!(
        fs::metadata(store.path()).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let lock = store.path().with_file_name("appearance.json.lock");
    fs::create_dir(&lock).unwrap();
    fs::set_permissions(&lock, fs::Permissions::from_mode(0o700)).unwrap();
    let marker = b"v1:0123456789abcdef0123456789abcdef\n";
    fs::write(lock.join("owner"), marker).unwrap();
    fs::set_permissions(lock.join("owner"), fs::Permissions::from_mode(0o600)).unwrap();
    let start = std::time::Instant::now();
    let report = store.patch_report(1, Patch::Rgb([0, 0, 0]));
    assert_eq!(report.status, Status::Busy);
    assert!(report.cleanup_error.is_none());
    assert_eq!(report.error.unwrap().kind(), std::io::ErrorKind::TimedOut);
    assert!(start.elapsed() >= std::time::Duration::from_millis(1000));
    assert_eq!(fs::read(store.path()).unwrap(), before);
    assert_eq!(store.load().unwrap().revision, 1);
    assert_eq!(fs::read(lock.join("owner")).unwrap(), marker);
}

#[cfg(unix)]
#[test]
fn l17_reports_preserve_primary_status_snapshot_and_cleanup_warning() {
    let scratch = Scratch::new();
    let store = scratch.store();
    let missing = store.load_report();
    assert_eq!(missing.status, Status::Missing);
    let defaults = missing.snapshot.unwrap();
    assert_eq!(
        (
            defaults.revision,
            defaults.rgb,
            defaults.opacity,
            defaults.transparent
        ),
        (0, [0, 0, 0], 55, true)
    );
    assert!(!store.path().parent().unwrap().exists());
    let saved = store.patch_report(0, Patch::Rgb([1, 2, 3]));
    assert_eq!(saved.status, Status::Saved);
    assert!(saved.cleanup_error.is_none());
    let snapshot = saved.snapshot.unwrap();
    assert_eq!(snapshot.revision, 1);
    assert_eq!(store.load_report().status, Status::Ready);
    let before = fs::read(store.path()).unwrap();
    let conflict = store.patch_report(0, Patch::Rgb([4, 5, 6]));
    assert_eq!(conflict.status, Status::Conflict);
    assert_eq!(conflict.snapshot.unwrap(), snapshot);
    assert_eq!(
        store
            .patch_report(
                1,
                Patch::Transparency {
                    opacity: 55,
                    enabled: false
                }
            )
            .status,
        Status::Invalid
    );
    assert_eq!(fs::read(store.path()).unwrap(), before);
    let warning = Completion {
        status: Status::Saved,
        snapshot: Some(snapshot),
        error: None,
        cleanup_error: Some("owned cleanup denied; manual recovery required".into()),
    };
    assert!(
        warning
            .warning()
            .unwrap()
            .contains("Appearance cleanupError:")
    );
    assert_eq!(warning.status, Status::Saved);
    assert_eq!(warning.snapshot.unwrap().revision, 1);
    let exhausted = String::from_utf8(before)
        .unwrap()
        .replace("\"revision\":1", "\"revision\":9007199254740991");
    fs::write(store.path(), &exhausted).unwrap();
    assert_eq!(
        store
            .patch_report(9007199254740991, Patch::Rgb([4, 5, 6]))
            .status,
        Status::RevisionExhausted
    );
    assert_eq!(fs::read_to_string(store.path()).unwrap(), exhausted);
    for (status, code) in [
        (Status::Ready, "ready"),
        (Status::Missing, "missing"),
        (Status::Saved, "saved"),
        (Status::Conflict, "conflict"),
        (Status::RevisionExhausted, "revision-exhausted"),
        (Status::Busy, "busy"),
        (Status::Invalid, "invalid"),
        (Status::UnsafePath, "unsafe-path"),
        (Status::IoError, "io-error"),
        (
            Status::CommittedDurabilityUncertain,
            "committed-durability-uncertain",
        ),
    ] {
        assert_eq!(status.code(), code);
    }
}

#[cfg(unix)]
#[test]
fn cleanup_permission_fixture() {
    let Some(root) = std::env::var_os("EMOTE_APPEARANCE_PERMISSION_ROOT") else {
        return;
    };
    let mode = std::env::var("EMOTE_APPEARANCE_PERMISSION_CASE").unwrap();
    let store = Store::new(PathBuf::from(root)).unwrap();
    let before = fs::read(store.path()).unwrap();
    // This process runs only this exact helper test. No process-wide umask is
    // changed in the parallel parent test process, nor is this a product flag.
    unsafe {
        libc::umask(if mode == "marker-create" {
            0o777
        } else {
            0o400
        });
    }
    let report = store.patch_report(1, Patch::Rgb([4, 5, 6]));
    assert_eq!(report.status, Status::IoError);
    assert!(report.snapshot.is_none());
    assert_eq!(
        report.error.as_ref().unwrap().kind(),
        std::io::ErrorKind::PermissionDenied
    );
    assert_eq!(fs::read(store.path()).unwrap(), before);
    let lock = store.path().with_file_name("appearance.json.lock");
    if mode == "marker-create" {
        assert!(report.cleanup_error.is_none());
        assert!(
            !lock.exists(),
            "partial acquisition cleans the proven owned directory"
        );
    } else {
        assert!(report.cleanup_error.as_ref().unwrap().contains("lock:"));
        assert!(
            report
                .warning()
                .unwrap()
                .contains("Appearance cleanupError:")
        );
        assert!(lock.is_dir());
        assert!(
            fs::read_dir(store.path().parent().unwrap())
                .unwrap()
                .all(|entry| !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".tmp")),
            "temp cleanup must succeed independently of lock cleanup failure"
        );
    }
    println!(
        "permission-case={mode};status={};cleanup={}",
        report.status.code(),
        report.cleanup_error.is_some()
    );
}

#[cfg(unix)]
#[test]
fn l17_permission_failures_preserve_source_and_report_independent_cleanup() {
    use std::os::unix::fs::PermissionsExt;
    // Root bypasses permission denials, so the real EACCES fixtures do not apply.
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("EACCES fixtures unavailable under root");
        return;
    }
    let scratch = Scratch::new();
    let store = scratch.store();
    store.patch(0, Patch::Rgb([1, 2, 3])).unwrap();
    let before = fs::read(store.path()).unwrap();
    for mode in ["marker-create", "token-read"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "cleanup_permission_fixture", "--nocapture"])
            .env("EMOTE_APPEARANCE_PERMISSION_ROOT", scratch.0.join("agent"))
            .env("EMOTE_APPEARANCE_PERMISSION_CASE", mode)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        assert!(String::from_utf8_lossy(&output.stdout).contains(&format!(
            "permission-case={mode};status=io-error;cleanup={}",
            mode == "token-read"
        )));
        assert_eq!(fs::read(store.path()).unwrap(), before);
        assert_eq!(store.load().unwrap().revision, 1);
        if mode == "token-read" {
            let lock = store.path().with_file_name("appearance.json.lock");
            // Manual recovery of this test's own artifact, after its writer exits.
            fs::set_permissions(&lock, fs::Permissions::from_mode(0o700)).unwrap();
            fs::set_permissions(lock.join("owner"), fs::Permissions::from_mode(0o600)).unwrap();
            let marker = fs::read(lock.join("owner")).unwrap();
            assert_eq!(marker.len(), 36);
            assert!(marker.starts_with(b"v1:"));
            assert_eq!(marker[35], b'\n');
            assert!(
                marker[3..35]
                    .iter()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn appearance_process_writer() {
    let Some(root) = std::env::var_os("EMOTE_APPEARANCE_TEST_ROOT") else {
        return;
    };
    let store = Store::new(PathBuf::from(root)).unwrap();
    match store.patch(0, Patch::Rgb([7, 8, 9])).unwrap() {
        SaveResult::Committed(_) => println!("CAS-WINNER"),
        SaveResult::Conflict(s) => {
            assert_eq!(s.revision, 1);
            println!("CAS-CONFLICT");
        }
        other => panic!("unexpected outcome: {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn independent_processes_share_the_same_cas_and_lock() {
    let scratch = Scratch::new();
    let store = scratch.store();
    let children: Vec<_> = (0..4)
        .map(|_| {
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "appearance_process_writer", "--nocapture"])
                .env("EMOTE_APPEARANCE_TEST_ROOT", scratch.0.join("agent"))
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    let outputs: Vec<_> = children
        .into_iter()
        .map(|c| c.wait_with_output().unwrap())
        .collect();
    for out in &outputs {
        assert!(
            out.status.success(),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stderr.is_empty());
    }
    assert_eq!(
        outputs
            .iter()
            .filter(|out| String::from_utf8_lossy(&out.stdout).contains("CAS-WINNER"))
            .count(),
        1
    );
    assert_eq!(
        outputs
            .iter()
            .filter(|out| String::from_utf8_lossy(&out.stdout).contains("CAS-CONFLICT"))
            .count(),
        3
    );
    assert_eq!(store.load().unwrap().revision, 1);
    assert_eq!(store.load().unwrap().rgb, [7, 8, 9]);
    assert!(!store.path().with_file_name("appearance.json.lock").exists());
}
