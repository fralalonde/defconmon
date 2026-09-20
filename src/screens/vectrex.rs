//! Vectrex-style vector display - two wireframe solids, both driven by the feed.
//!
//! This screen used to be pure homage: pretty, but it carried no information.
//! Both solids are now plots of the twelve real channels in `data.rs`, one
//! vertex per channel:
//!
//!   FIG1 STATE       vertex radius = magnitude   vertex colour = severity
//!   FIG2 VOLATILITY  vertex radius = short-window sigma (15 s)
//!                    vertex colour = medium-window sigma (90 s)
//!
//! A calm, healthy house is two small even spheres in green. A pegged CPU or a
//! flapping link pushes its own vertex out and colours it, so the shape is
//! readable at a glance and the legend tells you exactly which channel moved.
use super::hashf;
use super::Env;
use crate::canvas::Canvas;
use crate::config::Param;
use crate::clock;
use crate::data::{ch_name, ch_unit, level_rgb, Snap, MED_W, NCH, SHORT_W};

const G: u32 = 0x33ff66;
const GD: u32 = 0x2f7a45;
const DIM: u32 = 0x1d4a2c;
const AMBER: u32 = 0xffb000;

/// Icosahedron: 12 vertices = 12 channels, one each.
const PHI: f32 = 1.618_034;
const ICO_V: [(f32, f32, f32); 12] = [
    (-1.0, PHI, 0.0),
    (1.0, PHI, 0.0),
    (-1.0, -PHI, 0.0),
    (1.0, -PHI, 0.0),
    (0.0, -1.0, PHI),
    (0.0, 1.0, PHI),
    (0.0, -1.0, -PHI),
    (0.0, 1.0, -PHI),
    (PHI, 0.0, -1.0),
    (PHI, 0.0, 1.0),
    (-PHI, 0.0, -1.0),
    (-PHI, 0.0, 1.0),
];

/// Vertex distance from the origin, so `scale` can be a pixel radius.
const ICO_R: f32 = 1.902_113_5;

/// Edges derived from the vertices by distance, so the wireframe is correct by
/// construction (an icosahedron's 30 edges) rather than a hand-typed list.
fn ico_edges() -> Vec<(usize, usize)> {
    let mut e = Vec::new();
    for (i, a) in ICO_V.iter().enumerate() {
        for (j, b) in ICO_V.iter().enumerate().skip(i + 1) {
            let d = ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2) + (a.2 - b.2).powi(2)).sqrt();
            if d < 2.5 {
                e.push((i, j));
            }
        }
    }
    e
}

fn ok_str(b: bool) -> String {
    if b {
        "OK".into()
    } else {
        "DOWN".into()
    }
}

/// The channel's live value in its own units, for the legend.
fn value_str(f: &Snap, i: usize) -> String {
    match i {
        0 => format!("{:.1} {}", f.wan_rx, ch_unit(0)),
        1 => format!("{:.1} {}", f.wan_tx, ch_unit(1)),
        2 => format!("{:.0} {}", f.ping_ms, ch_unit(2)),
        3 => format!("{:.0} {}", f.dns_ms, ch_unit(3)),
        4 => format!("{:.0} {}", f.pve_cpu, ch_unit(4)),
        5 => format!("{:.0} {}", f.pve_mem, ch_unit(5)),
        6 => format!("{}/{}", f.guests_running, f.guests_total),
        7 => ok_str(f.wan_online),
        8 => ok_str(f.gateway),
        9 => ok_str(f.internet),
        10 => ok_str(f.pve_online),
        11 => ok_str(f.opn_reach),
        _ => String::new(),
    }
}

/// Volatility colour: stable green, moving amber, thrashing red.
fn vol_rgb(v: f32) -> u32 {
    if v < 0.15 {
        level_rgb(0)
    } else if v < 0.40 {
        level_rgb(1)
    } else {
        level_rgb(2)
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_solid(
    cv: &mut Canvas,
    radius: &[f32; NCH],
    cols: &[u32; NCH],
    edges: &[(usize, usize)],
    t: f32,
    cx: f32,
    cy: f32,
    scale: f32,
) {
    let (rx, ry) = (t * 0.31, t * 0.47);
    let (crx, srx) = (rx.cos(), rx.sin());
    let (cry, sry) = (ry.cos(), ry.sin());

    let mut pts = [(0.0f32, 0.0f32); NCH];
    let mut dep = [0.0f32; NCH];
    for i in 0..NCH {
        let (x, y, z) = ICO_V[i];
        // rotate about Y, then X
        let x1 = x * cry + z * sry;
        let z1 = -x * sry + z * cry;
        let y1 = y * crx - z1 * srx;
        let z2 = y * srx + z1 * crx;
        let k = 4.2 / (4.2 + z2 * 0.5); // slight perspective
        let r = radius[i];
        pts[i] = (
            cx + x1 / ICO_R * k * scale * r,
            cy - y1 / ICO_R * k * scale * r,
        );
        dep[i] = z2;
    }

    // wireframe: nearer edges brighter
    for (a, b) in edges.iter() {
        let d = (dep[*a] + dep[*b]) * 0.5;
        let al = (118.0 - d * 26.0).clamp(42.0, 190.0) as u8;
        cv.line(pts[*a].0, pts[*a].1, pts[*b].0, pts[*b].1, 1.5, GD, al);
    }
    // vertices: bright, coloured by the channel they stand for
    for i in 0..NCH {
        let r = (3.2 - dep[i] * 0.5).max(2.2);
        cv.circle(pts[i].0, pts[i].1, r, 2.2, cols[i], 245, true);
    }
}

pub fn render(cv: &mut Canvas, env: &Env, t: f32) {
    let f = env.f;
    let snap = env.snap;
    let (w, h) = (cv.w, cv.h);
    let pad = 64.0;
    let edges = ico_edges();

    // ---------------- per-channel plot values ----------------
    let mut r_state = [0.0f32; NCH];
    let mut c_state = [0u32; NCH];
    let mut r_vol = [0.0f32; NCH];
    let mut c_vol = [0u32; NCH];
    for i in 0..NCH {
        r_state[i] = 0.72 + 0.50 * snap.ch_norm(i);
        c_state[i] = level_rgb(snap.ch_level(i));
        r_vol[i] = 0.70 + 0.55 * (snap.ch_vol(i, SHORT_W) * 1.8).min(1.0);
        c_vol[i] = vol_rgb(snap.ch_vol(i, MED_W));
    }

    // ---------------- HUD ----------------
    cv.glow_text(&f.a3270, 30.0, pad, pad + 20.0, G, "VECTREX  //  METRIC SOLIDS");
    let (hh, mm, ss) = clock::now_hms();
    let clock = format!("{hh:02}:{mm:02}:{ss:02}");
    let cw = cv.text_w(&f.dseg7, 34.0, &clock);
    cv.text(&f.dseg7, 34.0, w - pad - cw, pad + 14.0, G, 240, &clock);
    let date = clock::now_date();
    let dw = cv.text_w(&f.a3270, 20.0, &date);
    cv.text(&f.a3270, 20.0, w - pad - dw, pad + 40.0, GD, 210, &date);

    // ---------------- the two solids ----------------
    let cy = h * 0.54;
    let scale = h * 0.155;
    let cx1 = w * 0.27;
    let cx2 = w * 0.585;

    let spin = env.cfg.i("vectrex.spin", 100) as f32 / 100.0;
    draw_solid(cv, &r_state, &c_state, &edges, t * spin, cx1, cy, scale);
    draw_solid(cv, &r_vol, &c_vol, &edges, (t * 0.7 + 11.0) * spin, cx2, cy, scale);

    let cap_y = cy + scale * 1.32 + 26.0;
    cv.text_c(&f.a3270, 21.0, cx1, cap_y, G, 235, "FIG1  STATE  //  R = MAGNITUDE   C = SEVERITY");
    cv.text_c(
        &f.a3270,
        21.0,
        cx2,
        cap_y,
        G,
        235,
        "FIG2  VOLATILITY  //  R = SHORT 15s   C = MID 90s",
    );

    // ---------------- legend: which vertex is which ----------------
    let lx = w * 0.75;
    cv.text(&f.a3270, 22.0, lx, 178.0, GD, 235, "CHANNELS  /  VERTEX MAP");
    cv.line(lx, 192.0, w - pad, 192.0, 1.4, DIM, 200);
    let mut y = 226.0;
    for i in 0..NCH {
        let lvl = snap.ch_level(i);
        let c = level_rgb(lvl);
        cv.rect(lx, y - 12.0, 14.0, 14.0, c, 235);
        cv.text(&f.a3270, 19.0, lx + 26.0, y, GD, 225, ch_name(i));
        let v = value_str(snap, i);
        let vw = cv.text_w(&f.a3270, 19.0, &v);
        cv.text(&f.a3270, 19.0, lx + 280.0 - vw, y, c, 235, &v);
        // medium-window volatility, the same thing FIG2 colours by
        let vol = snap.ch_vol(i, MED_W);
        cv.rect(lx + 300.0, y - 9.0, 116.0, 10.0, DIM, 160);
        cv.rect(lx + 300.0, y - 9.0, 116.0 * vol, 10.0, vol_rgb(vol), 220);
        y += 40.0;
    }
    let keys = [
        "RADIUS 0.72 IDLE .. 1.22 PEGGED",
        "BAR = MED 90s DEVIATION",
        "FIG1 COLOUR  NOMINAL / WARNING / CRITICAL",
        "FIG2 COLOUR  STEADY / MOVING / THRASHING",
    ];
    for (i, k) in keys.iter().enumerate() {
        cv.text(&f.a3270, 16.0, lx, y + 18.0 + i as f32 * 26.0, GD, 205, k);
    }

    // ---------------- footer ----------------
    let fy = h - pad - 20.0;
    cv.line(pad, fy - 30.0, w - pad, fy - 30.0, 2.0, GD, 200);

    // the channel that most deserves attention: worst severity, then worst jitter
    let mut worst = 0usize;
    for i in 1..NCH {
        let (a, b) = (snap.ch_level(i), snap.ch_vol(i, MED_W));
        let (wa, wb) = (snap.ch_level(worst), snap.ch_vol(worst, MED_W));
        if a > wa || (a == wa && b > wb) {
            worst = i;
        }
    }
    let wl = snap.ch_level(worst);
    let status = if wl == 0 {
        "ALL SYSTEMS NOMINAL".to_string()
    } else {
        format!("LIMITING FACTOR: {} {}", ch_name(worst), value_str(snap, worst))
    };
    let sc = if wl == 0 { G } else { level_rgb(wl) };
    let foot = format!(
        "SOLIDS 02   VERTS {}   EDGES {}",
        NCH * 2,
        edges.len() * 2
    );
    cv.text(&f.a3270, 22.0, pad, fy, GD, 220, &foot);
    cv.text(&f.a3270, 22.0, pad + 400.0, fy, sc, 235, &status);

    let mode = if snap.live { "MODE: LIVE" } else { "MODE: SIM" };
    let mw = cv.text_w(&f.a3270, 22.0, mode);
    cv.text(
        &f.a3270,
        22.0,
        w - pad - mw,
        fy,
        if snap.live { G } else { AMBER },
        235,
        mode,
    );

    // a little beam chatter so it still feels like a vector tube
    for i in 0..40u32 {
        let x = hashf(i * 31 + (t * 8.0) as u32) * w;
        let y = hashf(i * 17 + 99) * h;
        cv.rect(x, y, 2.0, 2.0, G, 22);
    }
}

/// Knobs this screen owns, surfaced by the config server.
pub fn params() -> Vec<Param> {
    vec![Param::i(
        "vectrex.spin",
        "Solid spin",
        100,
        0.0,
        400.0,
        "Rotation rate of both solids, as a percentage of nominal. 0 freezes them.",
    )]
}
