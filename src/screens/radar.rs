//! SCREEN 4 - ATC APPROACH CONTROL
//! Primary surveillance radar: rotating sweep, range rings, contacts with
//! trails and data blocks. The homelab's guests are the contacts.
use super::{hashf, sweep};
use super::Env;
use crate::config::Param;
use crate::canvas::Canvas;

const G: u32 = 0x33ff66;
const GD: u32 = 0x186b32;
const DIM: u32 = 0x0e4a22;
const AMBER: u32 = 0xffb000;
const RED: u32 = 0xff3030;

pub fn render(cv: &mut Canvas, env: &Env, t: f32) {
    let f = env.f;
    let feed = env.snap;
    let (w, h) = (cv.w, cv.h);
    let pad = 64.0;
    let blink = (t * 2.0) as i32 % 2 == 0;

    // ---------------- header ----------------
    let hy = pad + 32.0;
    cv.glow_text(&f.a3270, 44.0, pad, hy, G, "APPROACH CONTROL  //  PRIMARY SURVEILLANCE");
    let mode = if feed.live { "MODE 3/A  LIVE" } else { "MODE 3/A  SIM" };
    let mw = cv.text_w(&f.a3270, 30.0, mode);
    cv.text(&f.a3270, 30.0, w - pad - mw, hy, if feed.live { G } else { AMBER }, 235, mode);
    cv.line(pad, hy + 24.0, w - pad, hy + 24.0, 2.0, GD, 210);

    // ---------------- scope ----------------
    // Size the scope to the screen: the radius must fit the *shorter* of the
    // available width and height, otherwise the bottom arc is clipped.
    let scope_w = w - 144.0 - 560.0;
    let top = hy + 70.0;
    // Reserve the band the persistent status plate occupies on this screen
    // (hud::slot): the plate is opaque and drawn over the scope, so without
    // this reservation it covers the lower arc and the contact labels near it.
    let bottom = (h - 228.0) - 24.0;
    let r = (scope_w / 2.0).min((bottom - top) / 2.0);
    let cx = pad + 20.0 + r;
    let cy = (top + bottom) / 2.0;

    // rings
    for i in 1..=4 {
        cv.circle(cx, cy, r * i as f32 / 4.0, if i == 4 { 2.5 } else { 1.4 }, DIM, if i == 4 { 190 } else { 130 }, false);
    }
    // crosshairs + bearing ticks
    cv.line(cx - r, cy, cx + r, cy, 1.2, DIM, 120);
    cv.line(cx, cy - r, cx, cy + r, 1.2, DIM, 120);
    for b in 0..36 {
        let a = b as f32 * std::f32::consts::TAU / 36.0;
        let l = if b % 3 == 0 { 18.0 } else { 9.0 };
        cv.line(cx + (r - l) * a.cos(), cy + (r - l) * a.sin(), cx + r * a.cos(), cy + r * a.sin(), 1.2, DIM, 150);
    }
    // labels on the cardinal ring
    let ring: [(f32, &str); 4] = [(0.0, "360"), (90.0, "090"), (180.0, "180"), (270.0, "270")];
    for (deg, lab) in ring.iter() {
        let a = deg.to_radians();
        cv.text(&f.a3270, 18.0, cx + (r + 10.0) * a.sin() - 16.0, cy - (r + 10.0) * a.cos() + 6.0, GD, 200, lab);
    }

    // ---------------- sweep + contacts ----------------
    let rpm = env.cfg.f("radar.sweep_rpm", 8.6).max(0.0);
    let sweep_a = (t * rpm * std::f32::consts::TAU / 60.0) % std::f32::consts::TAU;
    let sdir = (sweep_a.sin(), -sweep_a.cos());
    cv.glow_line(cx, cy, cx + r * sdir.0, cy + r * sdir.1, 2.0, G);
    // trailing fade arc
    for k in 1..14 {
        let a = sweep_a - k as f32 * 0.045;
        let alpha = (60 - k * 4).max(0) as u8;
        cv.line(cx, cy, cx + r * a.sin(), cy - r * a.cos(), 1.0, G, alpha);
    }
    cv.circle(cx, cy, 5.0, 2.0, G, 255, true);

    // contacts
    for (i, g) in feed.guests.iter().enumerate() {
        let b = hashf(i as u32 * 17 + 5) * std::f32::consts::TAU;
        let rr = 0.22 + hashf(i as u32 * 29 + 11) * 0.72;
        let drift = (t * 0.02 * (1.0 + hashf(i as u32))).sin() * 0.06;
        let bz = b + drift;
        let px = cx + r * rr * bz.sin();
        let py = cy - r * rr * bz.cos();
        let col = if g.up { G } else { RED };
        // blip fades as the sweep moves away from its bearing
        let mut d = (bz - sweep_a).rem_euclid(std::f32::consts::TAU);
        if d > std::f32::consts::TAU { d -= std::f32::consts::TAU; }
        let fade = (1.0 - d / std::f32::consts::TAU).powf(1.6);
        let a = (60.0 + fade * 195.0) as u8;
        // trail
        if g.up && fade > 0.35 {
            let tb = bz - 0.06 - 0.05 * hashf(i as u32 * 3);
            let tr = rr;
            cv.line(cx + r * tr * tb.sin(), cy - r * tr * tb.cos(), px, py, 1.2, col, 90);
        }
        cv.circle(px, py, 6.0, if g.up { 2.5 } else { 3.5 }, col, a, true);
        if !g.up {
            // struck-through = lost contact
            cv.line(px - 8.0, py - 8.0, px + 8.0, py + 8.0, 2.0, col, if blink { 255 } else { 90 });
            cv.line(px + 8.0, py - 8.0, px - 8.0, py + 8.0, 2.0, col, if blink { 255 } else { 90 });
        }
        // data block - kept inside the scope, flipped left near the right edge
        let label = format!("{}  {:.0}%", g.name, g.cpu);
        let tw = cv.text_w(&f.a3270, 19.0, &label);
        let ly = py.clamp(top + 26.0, bottom - 10.0) - 8.0;
        let flip = px + 14.0 + tw > cx + r - 6.0;
        let (lx, ax) = if flip { (px - 14.0 - tw, px - 5.0) } else { (px + 14.0, px + 5.0) };
        let (ex, ey) = if flip { (lx + tw + 2.0, ly + 2.0) } else { (lx - 2.0, ly + 2.0) };
        cv.line(ax, py - 5.0, ex, ey, 1.0, col, a / 2);
        cv.text(&f.a3270, 19.0, lx, ly, col, a, &label);
    }

    // ---------------- right rail: flight strips ----------------
    let rx = cx + r + 90.0;
    cv.text(&f.a3270, 30.0, rx, hy + 70.0, GD, 235, "CONTACT STRIPS");
    let mut y = hy + 116.0;
    for (i, g) in feed.guests.iter().enumerate().take(10) {
        let col = if g.up { G } else { RED };
        cv.rect_outline(rx - 8.0, y - 22.0, 470.0, 30.0, 1.2, if g.up { DIM } else { 0x5a1010 }, 200);
        let cs = format!("{}{:02}", (b'A' + (i as u8 % 26)) as char, i + 1);
        cv.text(&f.a3270, 20.0, rx, y, col, 235, &format!("{cs}  {:<14} {:>6.1}%  {}", g.name, g.cpu, if g.up { "UP" } else { "LOST" }));
        y += 34.0;
    }

    // ---------------- footer ----------------
    let fy = h - pad - 12.0;
    cv.line(pad, fy - 34.0, w - pad, fy - 34.0, 2.0, GD, 200);
    cv.text(&f.a3270, 24.0, pad, fy, GD, 220, &format!(
        "TARGETS {}/{}   RANGE 60 NM   SWEEP 7.0s   PVE {} {:.0}% CPU / {:.0}% MEM",
        feed.guests_running, feed.guests_total, feed.pve_node, feed.pve_cpu, feed.pve_mem
    ));
    let st = format!("ACFT {}", feed.guests_running);
    let stw = cv.text_w(&f.a3270, 24.0, &st);
    cv.text(&f.a3270, 24.0, w - pad - stw, fy, if blink { G } else { GD }, 235, &st);

    sweep(cv, t, 8.0, G);
}

/// Knobs this screen owns, surfaced by the config server.
pub fn params() -> Vec<Param> {
    vec![Param::f(
        "radar.sweep_rpm",
        "Antenna rotation",
        8.6,
        0.5,
        30.0,
        0.1,
        "Sweep rotation in RPM. 8.6 is one full turn every 7 seconds.",
    )]
}
