//! Desktop-internal 64x64 idle clock, ported 1:1 from the canonical Lemmings
//! idle-clock renderer (`gentle-emotex/src/idle_clock_image.ts`, canonical
//! sprite extraction in `tests/fixtures/lemmings-walk.json`, golden pixels in
//! `tests/fixtures/idle-clock-golden.json`).
//!
//! The renderer is deterministic: the date/time glyphs come from the local
//! wall-clock components and the walking Lemmings animation from the approved
//! 500ms cadence `Math.floor(epoch_millis / 500)` (the canonical renderer uses
//! `Math.floor(nowMs / 1000)`; the desktop refreshes twice as fast, advancing
//! pose and movement on every 500ms tick while glyphs stay second-stable).
//! Ink pixels are opaque, background gaps stay transparent so the desktop's
//! own backdrop (opaque black or the transparency setting) shows through, the
//! same way transparent sender RGBA composites.
use crate::receiver::Frame;
use chrono::{DateTime, Datelike, FixedOffset, Timelike};

/// Approved animation tick: pose and movement advance every 500ms; the GUI
/// schedules its repaint with this period while any clock tile is visible.
pub const ANIMATION_PERIOD_MS: u64 = 500;

const WHITE: [u8; 3] = [242, 241, 220];
const MUTED: [u8; 3] = [147, 181, 160];
const BLUE: [u8; 3] = [85, 189, 232];
const GREEN: [u8; 3] = [184, 243, 91];
const AMBER: [u8; 3] = [241, 189, 101];
const PATH: [u8; 3] = [41, 75, 54];
const HAIR: [u8; 3] = [0, 179, 0];
const SKIN: [u8; 3] = [240, 208, 208];
const TUNIC: [u8; 3] = [64, 64, 224];

const GLYPHS: &[(char, [&str; 5])] = &[
    ('0', ["111", "101", "101", "101", "111"]),
    ('1', ["010", "110", "010", "010", "111"]),
    ('2', ["111", "001", "111", "100", "111"]),
    ('3', ["111", "001", "111", "001", "111"]),
    ('4', ["101", "101", "111", "001", "001"]),
    ('5', ["111", "100", "111", "001", "111"]),
    ('6', ["111", "100", "111", "101", "111"]),
    ('7', ["111", "001", "010", "010", "010"]),
    ('8', ["111", "101", "111", "101", "111"]),
    ('9', ["111", "101", "111", "001", "111"]),
    ('/', ["001", "001", "010", "100", "100"]),
    (':', ["000", "010", "000", "010", "000"]),
];

struct WalkFrame {
    x: i64,
    y: i64,
    rows: &'static [&'static str],
}

/// Native Lemmings walk cycle, extracted 1:1 (no resizing) from the canonical
/// fixture: every pose stands with its feet on cell row 12, faces right, in
/// sheet order. `WALK_FEET_ROW` is that canonical feet row.
const WALK_FEET_ROW: i64 = 12;
#[rustfmt::skip]
const WALK_FRAMES: [WalkFrame; 9] = [
    WalkFrame { x: 6, y: 4, rows: &["GGGG", "GGS.", ".SSS", ".SB.", ".SB.", ".SB.", ".BB.", "SBB.", ".SS."] },
    WalkFrame { x: 6, y: 3, rows: &[".G.G.", "GGG..", "GGS..", ".SSS.", ".SB..", "SBB..", "SBB.S", ".BB.S", "BB.S.", "SS..."] },
    WalkFrame { x: 5, y: 4, rows: &[".G.G..", ".GGG..", "..GS..", "..SSS.", ".SSB..", ".SBB..", "SSBBB.", ".BBBB.", "SS..SS"] },
    WalkFrame { x: 5, y: 4, rows: &["..GG.", ".GGSG", ".GSSS", "..SB.", "..SB.", ".SBB.", "..BB.", "SBBBB", "S..SS"] },
    WalkFrame { x: 6, y: 4, rows: &["GGGG", "GGS.", "GSSS", ".SB.", ".BS.", ".SB.", ".BB.", "SBB.", ".SS."] },
    WalkFrame { x: 6, y: 3, rows: &[".G.G.", "GGG..", "GGS..", ".SSS.", ".SB..", ".BS..", ".BS.S", ".BB.S", "BB.S.", "SS..."] },
    WalkFrame { x: 5, y: 4, rows: &[".G.G..", ".GGG..", "..GS..", "..SSS.", "..BS..", "..BS..", "..BBS.", ".BBBB.", "SS..SS"] },
    WalkFrame { x: 5, y: 4, rows: &["..GG.", ".GGSG", ".GSSS", "..SB.", "..SB.", "..BS.", "..BB.", "SBBBB", "S..SS"] },
    WalkFrame { x: 5, y: 3, rows: &[".GG.", "GGS.", "GSSS", ".SB.", ".BS.", ".BBS", ".BBS", ".BB.", "SB..", ".S.."] },
];

/// Monotonic animation step: advances on every 500ms tick and fully determines
/// the rendered pixels (glyph seconds change only on whole-second boundaries,
/// which coincide with even steps), so GUI uploads can be keyed on it.
pub fn animation_step(now: DateTime<FixedOffset>) -> i64 {
    now.timestamp_millis()
        .div_euclid(ANIMATION_PERIOD_MS as i64)
}

fn glyph(ch: char) -> Option<&'static [&'static str; 5]> {
    GLYPHS
        .iter()
        .find(|(candidate, _)| *candidate == ch)
        .map(|(_, rows)| rows)
}

struct Canvas([u8; 64 * 64 * 4]);

impl Canvas {
    fn rect(&mut self, left: i64, top: i64, width: i64, height: i64, ink: [u8; 3]) {
        for y in top.max(0)..(top + height).min(64) {
            for x in left.max(0)..(left + width).min(64) {
                let index = (y as usize * 64 + x as usize) * 4;
                self.0[index..index + 3].copy_from_slice(&ink);
                self.0[index + 3] = 255;
            }
        }
    }

    fn text(&mut self, value: &str, left: i64, top: i64, scale: i64, ink: [u8; 3]) {
        for (i, ch) in value.chars().enumerate() {
            let Some(rows) = glyph(ch) else { continue };
            for (y, row) in rows.iter().enumerate() {
                for (x, cell) in row.bytes().enumerate() {
                    if cell == b'1' {
                        self.rect(
                            left + (i as i64 * 4 + x as i64) * scale,
                            top + y as i64 * scale,
                            scale,
                            scale,
                            ink,
                        );
                    }
                }
            }
        }
    }
}

/// Selected bottom-promenade design: native pixels, local DD/MM/YYYY and 24h
/// time, transparent background gaps.
pub fn render(now: DateTime<FixedOffset>) -> Frame {
    let local = now.naive_local();
    let date = local.date();
    let time = local.time();
    let mut canvas = Canvas([0; 64 * 64 * 4]);
    let pad = |n: u32| format!("{n:02}");
    let date_text = format!("{:02}/{:02}/{:04}", date.day(), date.month(), date.year());
    canvas.text(
        &date_text,
        (64 - (date_text.len() as i64 * 4 - 1)) / 2,
        1,
        1,
        MUTED,
    );
    canvas.text(&pad(time.hour()), 12, 9, 4, WHITE);
    canvas.text(&pad(time.minute()), 12, 31, 4, BLUE);
    canvas.text(":", 43, 35, 1, AMBER);
    canvas.text(&pad(time.second()), 46, 33, 2, AMBER);
    canvas.rect(0, 63, 64, 1, PATH);
    for x in (0..64i64).step_by(8) {
        canvas.rect(x, 63, 3, 1, GREEN);
    }
    let step = animation_step(now);
    let count = WALK_FRAMES.len() as i64;
    let frame = &WALK_FRAMES[(((step % count) + count) % count) as usize];
    let cell_top = 62 - WALK_FEET_ROW;
    for offset in [0i64, 40] {
        let left = ((step * 2 + offset) % 80 + 80) % 80 - 8;
        for (y, row) in frame.rows.iter().enumerate() {
            for (x, cell) in row.bytes().enumerate() {
                let ink = match cell {
                    b'G' => HAIR,
                    b'S' => SKIN,
                    b'B' => TUNIC,
                    _ => continue,
                };
                canvas.rect(
                    left + 1 + frame.x + x as i64,
                    cell_top + frame.y + y as i64,
                    1,
                    1,
                    ink,
                );
            }
        }
    }
    Frame {
        rgba: canvas.0.to_vec(),
    }
}
