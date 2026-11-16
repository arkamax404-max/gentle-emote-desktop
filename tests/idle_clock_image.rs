use chrono::{DateTime, TimeDelta};
use gentle_emote_desktop::idle_clock_image::{animation_step, render, render_for_session};

const GOLDEN: &str = include_str!("fixtures/idle-clock-golden.json");

fn header(rgba: &[u8]) -> &[u8] {
    &rgba[64 * 4..6 * 64 * 4]
}

fn assert_only_header_changes(now: DateTime<chrono::FixedOffset>, id: &str) {
    let original = render(now).rgba;
    let session = render_for_session(now, id).rgba;
    assert_eq!(original.len(), session.len());
    assert_eq!(&original[..64 * 4], &session[..64 * 4]);
    assert_eq!(&original[6 * 64 * 4..], &session[6 * 64 * 4..]);
}

#[test]
fn session_header_alternates_at_absolute_five_second_boundaries() {
    let epoch = at("1970-01-01T00:00:00+00:00");
    for millis in [0, 4999, 5000, 9999, 10000, -1, -5000, -5001] {
        let now = epoch + TimeDelta::milliseconds(millis);
        let original = render(now).rgba;
        let session = render_for_session(now, "Ab_9:-.z").rgba;
        if millis.div_euclid(5000).rem_euclid(2) == 0 {
            assert_eq!(session, original, "date phase at {millis}ms");
        } else {
            assert_ne!(
                header(&session),
                header(&original),
                "ID phase at {millis}ms"
            );
            assert_ne!(
                header(&session),
                header(&render_for_session(now, "Other").rgba),
                "session identity must affect the header"
            );
        }
        assert_only_header_changes(now, "Ab_9:-.z");
        assert_eq!(render_for_session(now, "").rgba, original);
    }
}

#[test]
fn session_header_ellipsis_is_static_and_fits_sixteen_cells() {
    let now = at("1970-01-01T00:00:05+00:00");
    let sixteen = "AbcdefghijklmNOP";
    let seventeen = "AbcdefghijklmNOPq";
    let truncated = render_for_session(now, seventeen).rgba;
    let expected = render_for_session(now, "Abcdefghijklm...").rgba;
    assert_ne!(
        header(&render_for_session(now, sixteen).rgba),
        header(&truncated),
        "16 cells stay intact; 17 cells require an ellipsis"
    );
    assert_eq!(header(&truncated), header(&expected));
    let longest = format!("Abcdefghijklm{}", "X".repeat(115));
    assert_eq!(longest.len(), 128);
    assert_eq!(
        header(&render_for_session(now, &longest).rgba),
        header(&expected)
    );
    for millis in [500, 1000, 4999] {
        let later = now + TimeDelta::milliseconds(millis);
        let pixels = render_for_session(later, seventeen).rgba;
        assert_eq!(header(&pixels), header(&expected), "no scrolling");
        assert_only_header_changes(later, seventeen);
    }
    // Sixteen 3-pixel glyphs and fifteen 1-pixel gaps occupy exactly 63 pixels.
    // The three final period cells begin at x=52, 56 and 60; each dot is centered.
    for x in [53, 57, 61] {
        let index = (5 * 64 + x) * 4;
        assert_eq!(&truncated[index..index + 4], &[147, 181, 160, 255]);
    }
    for y in 1..=5 {
        assert_eq!(&truncated[(y * 64 + 63) * 4..(y * 64 + 64) * 4], &[0; 4]);
    }
    assert_only_header_changes(now, sixteen);
    assert_only_header_changes(now, &longest);
}

#[test]
fn session_header_supports_every_validated_ascii_id_character_and_case() {
    let now = at("1970-01-01T00:00:05+00:00");
    for ch in "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._:-".chars() {
        let pixels = render_for_session(now, &ch.to_string()).rgba;
        assert!(
            header(&pixels)
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p == &[147, 181, 160, 255])
        );
        // A single cell is centered at x=30; no date or neighboring cells remain.
        for y in 1..=5 {
            for x in 0..64 {
                if !(30..33).contains(&x) {
                    assert_eq!(&pixels[(y * 64 + x) * 4..(y * 64 + x + 1) * 4], &[0; 4]);
                }
            }
        }
    }
    for ch in 'A'..='Z' {
        assert_ne!(
            header(&render_for_session(now, &ch.to_string()).rgba),
            header(&render_for_session(now, &ch.to_ascii_lowercase().to_string()).rgba)
        );
    }
}

fn at(iso: &str) -> DateTime<chrono::FixedOffset> {
    DateTime::parse_from_rfc3339(iso).unwrap()
}

/// Minimal extraction of top-level string fields from the golden fixture. The
/// generator (canonical TypeScript renderer executed by Node) emits each case
/// as `{"label": ..., "iso": ..., "offsetSeconds": ..., "rgbaBase64": ...}` in
/// that key order, so same-index zipping is exact.
fn extract_strings(source: &str, key: &str) -> Vec<String> {
    let needle = format!("\"{key}\": \"");
    let mut out = Vec::new();
    let mut rest = source;
    while let Some(position) = rest.find(&needle) {
        let start = position + needle.len();
        let end = start + rest[start..].find('"').unwrap();
        out.push(rest[start..end].to_string());
        rest = &rest[end + 1..];
    }
    out
}

fn extract_numbers(source: &str, key: &str) -> Vec<i32> {
    let needle = format!("\"{key}\": ");
    let mut out = Vec::new();
    let mut rest = source;
    while let Some(position) = rest.find(&needle) {
        let start = position + needle.len();
        let end = rest[start..]
            .find(|c: char| !(c.is_ascii_digit() || c == '-'))
            .map(|end| start + end)
            .unwrap_or(rest.len());
        out.push(rest[start..end].parse().unwrap());
        rest = &rest[end..];
    }
    out
}

fn base64_decode(value: &str) -> Vec<u8> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for byte in value.bytes() {
        if byte == b'\n' || byte == b'\r' {
            continue;
        }
        if byte == b'=' {
            continue;
        }
        let decoded = TABLE
            .iter()
            .position(|&entry| entry == byte)
            .unwrap_or_else(|| panic!("invalid base64 byte {byte}"));
        buffer = (buffer << 6) | decoded as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    out
}

#[test]
fn internal_clock_matches_canonical_golden_pixels() {
    let labels = extract_strings(GOLDEN, "label");
    let isos = extract_strings(GOLDEN, "iso");
    let offsets = extract_numbers(GOLDEN, "offsetSeconds");
    let pixels = extract_strings(GOLDEN, "rgbaBase64");
    assert_eq!(labels.len(), isos.len());
    assert_eq!(labels.len(), offsets.len());
    assert_eq!(labels.len(), pixels.len());
    assert!(
        labels.len() >= 4,
        "golden must cover rollover, offsets and digit coverage"
    );
    let cases: Vec<_> = labels
        .iter()
        .zip(&isos)
        .zip(&offsets)
        .zip(&pixels)
        .map(|(((label, iso), offset), encoded)| (label, iso, offset, encoded))
        .collect();
    for (label, iso, offset, encoded) in cases {
        let golden = base64_decode(encoded);
        assert_eq!(golden.len(), 64 * 64 * 4, "{label}");
        let offset = chrono::FixedOffset::east_opt(*offset).unwrap();
        let now = at(iso).with_timezone(&offset);
        let rendered = render(now).rgba;
        if rendered != golden {
            let index = rendered
                .iter()
                .zip(&golden)
                .position(|(a, b)| a != b)
                .expect("vectors differ");
            panic!(
                "{label}: first differing byte at pixel ({}, {}), channel {}: rendered {:?} vs golden {:?}",
                (index / 4) % 64,
                index / 4 / 64,
                index % 4,
                &rendered[index..index + 4.min(rendered.len() - index)],
                &golden[index..index + 4.min(golden.len() - index)],
            );
        }
        assert_eq!(rendered, golden);
    }
}

#[test]
fn half_second_tick_advances_walkers_while_second_glyphs_hold() {
    let base = at("2025-07-04T09:41:56+05:00");
    let half = base + TimeDelta::try_milliseconds(500).unwrap();
    let next = base + TimeDelta::try_seconds(1).unwrap();
    let first = render(base).rgba;
    let second = render(half).rgba;
    // Walkers occupy rows 53..64 only; every glyph row is byte-identical
    // within the same wall-clock second.
    assert_eq!(
        &first[..52 * 64 * 4],
        &second[..52 * 64 * 4],
        "glyph rows must stay byte-identical 500ms apart inside one second"
    );
    assert_ne!(
        first, second,
        "walker pose and movement must advance every 500ms"
    );
    let third = render(next).rgba;
    assert_ne!(
        &first[..52 * 64 * 4],
        &third[..52 * 64 * 4],
        "the seconds glyph must advance on the whole second"
    );
    // The advance is confined to the seconds digits (rows 33..42): hour,
    // date and minute glyph rows above and below are unchanged.
    assert_eq!(&first[..33 * 64 * 4], &third[..33 * 64 * 4]);
    assert_eq!(
        &first[43 * 64 * 4..52 * 64 * 4],
        &third[43 * 64 * 4..52 * 64 * 4]
    );
}

#[test]
fn animation_step_and_pixels_advance_on_every_half_second_tick() {
    let base = at("2030-01-01T00:00:00+00:00");
    assert_eq!(
        animation_step(base),
        base.timestamp_millis().div_euclid(500)
    );
    let mut previous = render(base);
    for tick in 1..=10 {
        let now = base + TimeDelta::try_milliseconds(tick * 500).unwrap();
        assert_eq!(
            animation_step(now),
            animation_step(base) + tick,
            "the animation step must count exact 500ms ticks"
        );
        let current = render(now);
        assert_ne!(
            previous.rgba, current.rgba,
            "tick {tick}: walkers must change every 500ms"
        );
        // The same instant renders identically, so the app can key texture
        // uploads on the animation step alone.
        assert_eq!(current.rgba, render(now).rgba);
        previous = current;
    }
}

#[test]
fn internal_clock_uses_transparent_gaps_with_opaque_ink() {
    let rgba = render(at("2026-01-01T01:02:03+00:00")).rgba;
    let pixels = rgba.as_chunks::<4>().0;
    let opaque = pixels.iter().filter(|p| p[3] == 255).count();
    let clear = pixels.iter().filter(|p| p[3] == 0).count();
    assert!(opaque > 500, "ink pixels must stay opaque");
    assert!(clear > 1000, "background gaps must stay transparent");
    assert!(
        pixels.iter().all(|p| p[3] == 0 || p[3] == 255),
        "alpha must stay binary for unpremultiplied desktop compositing"
    );
}
