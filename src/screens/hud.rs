//! The persistent status plate: DEFCON / WAN / PING / DNS, on every screen.
//!
//! The family should be able to read the state of the house without waiting for
//! a particular screen to come around, so this rides on all five. To avoid
//! burning the panel it is placed at a *different* position on each screen -
//! all five slots differ in both axes, so consecutive screens never light the
//! same pixels. The DEFCON cell is the leftmost, largest, brightest element, so
//! the plate is recognisable at a glance wherever it lands.
use super::cond_color;
use crate::canvas::Canvas;
use crate::data::{level_rgb, Snap};
use crate::fonts::Fonts;

pub const PW: f32 = 900.0;
pub const PH: f32 = 104.0;

const FRAME: u32 = 0x2f7a45;
const LABEL: u32 = 0x4fbf6a;

/// Top-left corner of the plate for a given screen.
pub fn slot(screen: &str, w: f32, h: f32) -> (f32, f32) {
    let pad = 64.0;
    match screen {
        "wopr" => (pad, h - 272.0),                 // lower left
        "defcon" => (w - pad - PW, h - 376.0),      // right, high
        "radar" => (w / 2.0 - PW / 2.0, h - 228.0), // bottom centre
        "telemetry" => (pad, h - 192.0),            // lower left, low
        "vectrex" => (w / 2.0 - PW / 2.0, 136.0),   // top centre
        _ => (w / 2.0 - PW / 2.0, h - 228.0),
    }
}

pub fn draw(cv: &mut Canvas, f: &Fonts, screen: &str, snap: &Snap) {
    let (x, y) = slot(screen, cv.w, cv.h);

    // opaque backing so the plate reads over grids, starfields and traces
    cv.rect(x, y, PW, PH, 0x000000, 242);
    cv.rect_outline(x, y, PW, PH, 2.5, FRAME, 235);
    cv.rect(x, y, PW, 3.0, FRAME, 200);

    // ---- DEFCON cell (the anchor) -------------------------------------
    let level = snap.defcon();
    let dc = cond_color(level);
    cv.rect_outline(x + 14.0, y + 14.0, 226.0, PH - 28.0, 3.0, dc, 240);
    cv.text(&f.a3270, 20.0, x + 30.0, y + 44.0, LABEL, 235, "DEFCON");
    cv.text(&f.dseg7, 48.0, x + 152.0, y + 74.0, dc, 255, &format!("{level}"));

    // ---- WAN ----------------------------------------------------------
    let cw = 212.0;
    let c1 = x + 264.0;
    let c2 = c1 + cw;
    let c3 = c2 + cw;
    for cx in [c1, c2, c3] {
        cv.line(cx - 12.0, y + 16.0, cx - 12.0, y + PH - 16.0, 1.5, FRAME, 170);
    }

    let wc = if snap.wan_online { level_rgb(0) } else { level_rgb(2) };
    cell(cv, f, c1, y, "WAN", &format!("{:.1}", snap.wan_rx), "Mb/s", wc);

    let pc = level_rgb(snap.ch_level(2));
    cell(cv, f, c2, y, "PING", &format!("{:.0}", snap.ping_ms), "ms", pc);

    let dcol = level_rgb(snap.ch_level(3));
    cell(cv, f, c3, y, "DNS", &format!("{:.0}", snap.dns_ms), "ms", dcol);
}

// positional on purpose, like the Canvas primitives it calls
#[allow(clippy::too_many_arguments)]
fn cell(cv: &mut Canvas, f: &Fonts, cx: f32, y: f32, label: &str, value: &str, unit: &str, col: u32) {
    cv.text(&f.a3270, 20.0, cx + 4.0, y + 44.0, LABEL, 235, label);
    let vw = cv.text(&f.dseg7, 34.0, cx + 4.0, y + 76.0, col, 245, value);
    cv.text(&f.a3270, 18.0, cx + 12.0 + vw, y + 76.0, col, 190, unit);
}
