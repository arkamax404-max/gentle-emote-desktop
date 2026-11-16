use chrono::{DateTime, TimeDelta};
use gentle_emote_desktop::idle_clock_image::{animation_step, render};

const GOLDEN: &str = include_str!("fixtures/idle-clock-golden.json");

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
