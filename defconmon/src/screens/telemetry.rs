//! SCREEN 4 - TELEMETRY / SIGNAL ANALYSIS
//! Scientific scope: oscilloscope traces over the real WAN rate, spectrum
//! analyser and numeric readouts taken from the live feed. The envelope of the
//! traces responds to actual throughput - nothing here is invented, because a
//! monitoring screen showing plausible fiction is worse than no screen.
use super::{grid, hashf, sweep};
use super::Env;
use crate::config::Param;
use crate::canvas::Canvas;

const G: u32 = 0x33ff66;
const GD: u32 = 0x186b32;
const DIM: u32 = 0x0e4a22;
const AMBER: u32 = 0xffb000;
const CYAN: u32 = 0x38e8ff;
const RED: u32 = 0xff3030;
/// sweep period, shared by the animation and the footer caption
const SWEEP: f32 = 9.0;
/// full-scale for the WAN traces, Mb/s
const WAN_FS: f32 = 100.0;

pub fn render(cv: &mut Canvas, env: &Env, t: f32) {
    let f = env.f;
    let feed = env.snap;
    let (w, h) = (cv.w, cv.h);
    grid(cv, 80.0, DIM, 34, t * 3.0);
    sweep(cv, t, SWEEP, CYAN);

    let pad = 64.0;
    let blink = (t * 2.0) as i32 % 2 == 0;

    // ---------------- header ----------------
    let hy = pad + 32.0;
    cv.glow_text(&f.a3270, 44.0, pad, hy, G, "TELEMETRY  //  SIGNAL ANALYSIS");
    let live = if feed.live { "LIVE" } else { "SIM" };
    let lw = cv.text_w(&f.a3270, 34.0, live);
    let lc = if feed.live { RED } else { AMBER };
    cv.circle(w - pad - lw - 40.0, hy - 12.0, 9.0, 3.0, if blink { lc } else { 0x501010 }, 255, true);
    cv.text(&f.a3270, 34.0, w - pad - lw, hy, G, 235, live);
    cv.line(pad, hy + 24.0, w - pad, hy + 24.0, 2.0, GD, 210);

    // ---------------- scope ----------------
    let sx = 72.0;
    let sy = hy + 70.0;
    let sw = w - 144.0 - 520.0;
    let sh = 420.0;
    cv.rect_outline(sx, sy, sw, sh, 2.0, GD, 200);
    for i in 1..10 {
        let x = sx + sw * i as f32 / 10.0;
        cv.line(x, sy, x, sy + sh, 1.0, DIM, 90);
    }
    for i in 1..6 {
        let y = sy + sh * i as f32 / 6.0;
        cv.line(sx, y, sx + sw, y, 1.0, DIM, 90);
    }

    // ch0 = WAN receive (envelope follows the real rate), ch1 = PVE load
    let wan_fs = env.cfg.f("telemetry.full_scale", WAN_FS).max(1.0);
    let load = (feed.wan_rx / wan_fs).clamp(0.02, 1.0);
    let cpu = (feed.pve_cpu / 100.0).clamp(0.02, 1.0);
    let mid = sy + sh / 2.0;
    for ch in 0..2 {
        let (col, amp, freq, ph) = if ch == 0 {
            (G, sh * 0.34 * (0.30 + 0.70 * load), 2.3, 0.0)
        } else {
            (CYAN, sh * 0.30 * (0.25 + 0.75 * cpu), 5.7, 1.2)
        };
        let mut prev: Option<(f32, f32)> = None;
        let steps = (sw / 2.0) as usize;
        for i in 0..=steps {
            let fx = i as f32 / steps as f32;
            let x = sx + fx * sw;
            let env = (fx * std::f32::consts::PI).sin().abs();
            let n = hashf((i as u32) ^ (ch as u32 * 977)) - 0.5;
            let y = mid + amp * env * ((fx * freq * std::f32::consts::TAU + t * 3.0 + ph).sin() + n * 0.18);
            if let Some((px, py)) = prev {
                cv.glow_line(px, py, x, y, 1.6, col);
            }
            prev = Some((x, y));
        }
    }
    cv.text(&f.a3270, 20.0, sx + 6.0, sy + 24.0, 0x9fe870, 200, "CH0 WAN RX");
    cv.text(&f.a3270, 20.0, sx + 6.0, sy + 48.0, CYAN, 180, "CH1 PVE CPU");

    // ---------------- spectrum ----------------
    let sp_y = sy + sh + 56.0;
    cv.text(&f.a3270, 26.0, sx, sp_y - 14.0, GD, 230, "SPECTRUM");
    let bars = 56;
    let bw = sw / bars as f32;
    for i in 0..bars {
        let fx = i as f32 / bars as f32;
        let base = (1.0 - fx).powf(1.4);
        let n = hashf(i as u32 * 31 + (t * 6.0) as u32);
        let v = (base * 0.6 * (0.4 + 0.6 * load) + n * 0.5 * (0.3 + 0.7 * load)).min(1.0) * 150.0;
        let x = sx + i as f32 * bw;
        let col = if v > 120.0 { AMBER } else { G };
        cv.rect(x + 1.0, sp_y + 150.0 - v, bw - 2.0, v, col, 200);
    }
    cv.line(sx, sp_y + 151.0, sx + sw, sp_y + 151.0, 1.5, GD, 170);

    // ---------------- right rail: real readouts ----------------
    let rx = sx + sw + 60.0;
    let ry = sy + 10.0;
    cv.text(&f.a3270, 32.0, rx, ry, GD, 235, "READOUTS");
    let rows: &[(&str, String, &str, u32)] = &[
        ("WAN RX", format!("{:.1}", feed.wan_rx), "Mb/s", G),
        ("WAN TX", format!("{:.1}", feed.wan_tx), "Mb/s", AMBER),
        ("PVE CPU", format!("{:.0}", feed.pve_cpu), "%", G),
        ("PVE MEM", format!("{:.0}", feed.pve_mem), "%", CYAN),
    ];
    for (i, (lab, val, unit, col)) in rows.iter().enumerate() {
        let yy = ry + 60.0 + i as f32 * 88.0;
        cv.text(&f.a3270, 26.0, rx, yy, GD, 220, lab);
        cv.text(&f.dseg7, 46.0, rx + 130.0, yy + 6.0, *col, 240, val);
        cv.text(&f.a3270, 20.0, rx + 340.0, yy, GD, 200, unit);
    }

    // system box - the real state, not a fictional RF lock
    let by = ry + 60.0 + rows.len() as f32 * 88.0 + 16.0;
    cv.rect_outline(rx - 16.0, by, 460.0, 150.0, 2.0, GD, 190);
    cv.text(&f.a3270, 30.0, rx + 6.0, by + 46.0, G, 235, "SYSTEM");
    cv.text(
        &f.a3270,
        22.0,
        rx + 6.0,
        by + 88.0,
        GD,
        220,
        &format!("NODES {}/{}   NODE {}", feed.guests_running, feed.guests_total, feed.pve_node),
    );
    let (net, nc) = if feed.internet {
        ("INTERNET LINK UP", G)
    } else {
        ("INTERNET LINK DOWN", RED)
    };
    cv.text(&f.a3270, 22.0, rx + 6.0, by + 122.0, nc, 230, net);

    // ---------------- footer ----------------
    let fy = h - pad - 12.0;
    cv.line(pad, fy - 34.0, w - pad, fy - 34.0, 2.0, GD, 200);
    cv.text(
        &f.a3270,
        24.0,
        pad,
        fy,
        GD,
        220,
        &format!("STATION K-9  //  ARRAY 04  //  SWEEP {SWEEP:.1}s  //  FULL SCALE {wan_fs:.0} Mb/s"),
    );
    let st = "REC  \u{25CF}";
    let stw = cv.text_w(&f.a3270, 24.0, st);
    cv.text(&f.a3270, 24.0, w - pad - stw, fy, if blink { RED } else { GD }, 235, st);

}

/// Knobs this screen owns, surfaced by the config server.
pub fn params() -> Vec<Param> {
    vec![Param::f(
        "telemetry.full_scale",
        "WAN full scale",
        100.0,
        10.0,
        1000.0,
        10.0,
        "Mb/s that fills the scope. Lower it so small household traffic is visible.",
    )]
}
