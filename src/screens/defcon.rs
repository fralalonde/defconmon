//! SCREEN 5 - DEFCON / THREAT BOARD
//! Cold-war style defence board driven by the real homelab feed: WAN boundary,
//! gateway, guest constellation, and a DEFCON badge derived from live failures.
use super::{hashf, sweep};
use super::Env;
use crate::config::Param;
use crate::canvas::Canvas;

const G: u32 = 0x33ff66;
const GD: u32 = 0x186b32;
const DIM: u32 = 0x0e4a22;
const AMBER: u32 = 0xffb000;
const RED: u32 = 0xff3030;
const CYAN: u32 = 0x38e8ff;

fn cond_color(d: u32) -> u32 {
    match d {
        1 => RED,
        2 => 0xff6020,
        3 => AMBER,
        4 => 0x9fe870,
        _ => CYAN,
    }
}

pub fn render(cv: &mut Canvas, env: &Env, t: f32) {
    let f = env.f;
    let feed = env.snap;
    let (w, h) = (cv.w, cv.h);
    let pad = 64.0;
    let blink = (t * 1.9) as i32 % 2 == 0;

    // ---------------- header ----------------
    let hy = pad + 32.0;
    cv.glow_text(&f.a3270, 44.0, pad, hy, G, "NORAD  //  HOME SECTOR DEFENSE");
    // real local time, same as every other screen
    let (hh, mm, ss) = crate::clock::now_hms();
    let clock = format!("{hh:02}:{mm:02}:{ss:02}");
    let cw = cv.text_w(&f.a3270, 38.0, &clock);
    cv.text(&f.a3270, 38.0, w - pad - cw, hy, G, 235, &clock);

    // DEFCON badge
    let d = feed.defcon();
    let dc = cond_color(d);
    let ba = if d <= 3 && !blink { 150 } else { 255 };
    let bx = w - pad - cw - 400.0;
    cv.rect_outline(bx, hy - 60.0, 360.0, 80.0, 3.0, dc, ba);
    cv.text(&f.a3270, 26.0, bx + 24.0, hy + 2.0, dc, ba, "DEFCON");
    cv.text(&f.dseg7, 56.0, bx + 214.0, hy + 16.0, dc, ba, &format!("{d}"));
    cv.line(pad, hy + 28.0, w - pad, hy + 28.0, 2.0, GD, 210);

    // ---------------- topology ----------------
    let mx0 = pad;
    let mx1 = w - 620.0;
    let top = hy + 120.0;
    let gx = (mx0 + mx1) / 2.0;
    let gy = top + 150.0;
    let y_bus = gy + 140.0;

    // WAN boundary
    cv.line(mx0, top, mx1, top, 2.0, GD, 220);
    cv.text(&f.a3270, 22.0, mx0, top - 16.0, GD, 220, "WAN BOUNDARY   re0");
    let wan_c = if feed.wan_online { G } else { RED };
    let wr = format!("{:.1} Mb/s", feed.wan_rx);
    cv.text(&f.a3270, 22.0, mx1 - cv.text_w(&f.a3270, 22.0, &wr), top - 16.0, wan_c, 230, &wr);

    // traffic pulses along the boundary (speed scales with real throughput)
    let span = mx1 - mx0;
    let inbound = 18;
    for i in 0..inbound {
        let sp = 60.0 + feed.wan_rx.min(80.0) * 8.0;
        let x = mx1 - ((t * sp + i as f32 * span / inbound as f32) % span);
        cv.circle(x, top, 3.5, 2.0, G, 190, false);
    }
    let outbound = 12;
    for i in 0..outbound {
        let sp = 40.0 + feed.wan_tx.min(80.0) * 8.0;
        let x = mx0 + ((t * sp + i as f32 * span / outbound as f32) % span);
        cv.circle(x, top, 3.5, 2.0, AMBER, 180, false);
    }

    // WAN -> gateway trunk
    let trunk_c = if feed.wan_online && feed.gateway { G } else { RED };
    cv.line(gx, top, gx, gy - 36.0, 3.5, trunk_c, 230);
    let pulse = env.cfg.f("defcon.pulse_rate", 0.9);
    for i in 0..8 {
        let fr = (t * pulse + i as f32 / 8.0) % 1.0;
        let y = top + (gy - 36.0 - top) * fr;
        cv.circle(gx, y, 3.0, 2.0, trunk_c, (140.0 + 100.0 * (1.0 - fr)) as u8, false);
    }

    // gateway node (diamond)
    let gc = if feed.gateway { G } else { RED };
    cv.poly(
        &[(gx, gy - 36.0), (gx + 54.0, gy), (gx, gy + 36.0), (gx - 54.0, gy)],
        2.5,
        gc,
        240,
        true,
        true,
    );
    cv.text_c(&f.a3270, 22.0, gx, gy + 8.0, gc, 255, "GATEWAY");
    // RFC 5737 documentation range: the feed carries no address, and a real
    // gateway IP has no business in a public repo.
    cv.text_c(&f.a3270, 18.0, gx, gy + 66.0, GD, 220, "192.0.2.1");

    // bus + guest constellation
    cv.line(mx0 + 40.0, y_bus, mx1 - 40.0, y_bus, 1.6, DIM, 190);
    cv.line(gx, gy + 36.0, gx, y_bus, 1.6, DIM, 190);
    let n = feed.guests.len().max(1) as f32;
    let gw = (mx1 - 40.0 - (mx0 + 40.0)) / n;
    for (i, g) in feed.guests.iter().enumerate() {
        // nodes sit at cell left edges so none lines up with the centre trunk
        let x = mx0 + 40.0 + gw * (i as f32);
        let y = y_bus + 130.0;
        let c = if g.up { G } else { RED };
        cv.line(x, y_bus, x, y - 26.0, 1.4, c, 150);
        cv.circle(x, y, 13.0, 2.2, c, 235, true);
        if g.up {
            cv.circle(x, y, 5.0, 2.0, c, if blink { 255 } else { 150 }, true);
        } else if blink {
            cv.line(x - 8.0, y - 8.0, x + 8.0, y + 8.0, 2.2, RED, 255);
            cv.line(x + 8.0, y - 8.0, x - 8.0, y + 8.0, 2.2, RED, 255);
        }
        let name = if g.name.len() > 13 { &g.name[..13] } else { &g.name };
        cv.text_c(&f.a3270, 17.0, x, y + 34.0, c, 230, name);
        if g.up {
            cv.text_c(&f.a3270, 17.0, x, y + 52.0, G, 220, &format!("{:.0}%", g.cpu));
        } else {
            cv.text_c(&f.a3270, 15.0, x, y + 52.0, RED, if blink { 255 } else { 130 }, "LOST");
        }
    }

    // ---------------- right rail ----------------
    let rx = w - pad - 560.0;
    cv.text(&f.a3270, 30.0, rx, hy + 70.0, GD, 235, "DEFENSE CONDITION");
    let rows: [(&str, bool); 5] = [
        ("GATEWAY", feed.gateway),
        ("INTERNET", feed.internet),
        ("DNS", feed.dns),
        ("WAN LINK", feed.wan_online),
        ("PVE NODE", feed.pve_online),
    ];
    let mut y = hy + 118.0;
    for (lab, ok) in rows.iter() {
        let c = if *ok { G } else { RED };
        cv.rect_outline(rx - 8.0, y - 20.0, 500.0, 30.0, 1.4, if *ok { DIM } else { 0x5a1010 }, 200);
        cv.circle(rx + 12.0, y - 8.0, 7.0, 2.0, c, if !*ok && !blink { 110 } else { 255 }, true);
        cv.text(&f.a3270, 21.0, rx + 32.0, y, c, 235, lab);
        let st = if *ok { "OK" } else { "FAIL" };
        cv.text(&f.a3270, 21.0, rx + 430.0, y, c, 235, st);
        y += 36.0;
    }

    // WAN throughput instruments
    y += 24.0;
    cv.text(&f.a3270, 26.0, rx, y, GD, 235, "WAN THROUGHPUT");
    y += 44.0;
    cv.text(&f.a3270, 20.0, rx, y, GD, 220, "RX");
    cv.text(&f.dseg7, 34.0, rx + 70.0, y + 4.0, G, 240, &format!("{:.1}", feed.wan_rx));
    cv.text(&f.a3270, 20.0, rx + 200.0, y, GD, 220, "Mb/s");
    y += 52.0;
    cv.text(&f.a3270, 20.0, rx, y, GD, 220, "TX");
    cv.text(&f.dseg7, 34.0, rx + 70.0, y + 4.0, AMBER, 240, &format!("{:.1}", feed.wan_tx));
    cv.text(&f.a3270, 20.0, rx + 200.0, y, GD, 220, "Mb/s");

    // sparkline (values centred on the live rate)
    y += 60.0;
    cv.rect_outline(rx - 8.0, y, 500.0, 110.0, 1.4, DIM, 190);
    let mut prev: Option<(f32, f32)> = None;
    for i in 0..=60 {
        let fx = i as f32 / 60.0;
        let n = hashf((i as u32).wrapping_add((t * 2.0) as u32 * 31)) - 0.5;
        let v = (feed.wan_rx / 40.0).clamp(0.05, 0.95) + n * 0.25;
        let px = rx - 8.0 + fx * 500.0;
        let py = y + 110.0 - (v.clamp(0.0, 1.0)) * 100.0 - 5.0;
        if let Some((ox, oy)) = prev {
            cv.glow_line(ox, oy, px, py, 1.4, CYAN);
        }
        prev = Some((px, py));
    }

    // ---------------- footer ----------------
    let fy = h - pad - 12.0;
    cv.line(pad, fy - 34.0, w - pad, fy - 34.0, 2.0, GD, 200);
    cv.text(&f.a3270, 24.0, pad, fy, GD, 220, &format!(
        "NODES {}/{}   PVE {} {:.0}% CPU / {:.0}% MEM   LINE {}",
        feed.guests_running, feed.guests_total, feed.pve_node, feed.pve_cpu, feed.pve_mem,
        if feed.line_rate.is_empty() { "n/a" } else { feed.line_rate.as_str() }
    ));
    let src = if feed.live { "FEED LIVE" } else { "FEED SIM" };
    let sw = cv.text_w(&f.a3270, 24.0, src);
    cv.text(&f.a3270, 24.0, w - pad - sw, fy, if feed.live { G } else { AMBER }, 235, src);

    sweep(cv, t, 7.5, G);
}

/// Knobs this screen owns, surfaced by the config server.
pub fn params() -> Vec<Param> {
    vec![Param::f(
        "defcon.pulse_rate",
        "Traffic pulse rate",
        0.9,
        0.0,
        4.0,
        0.1,
        "Speed of the packets crawling along the WAN trunk. 0 freezes them.",
    )]
}
