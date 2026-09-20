//! SCREEN 1 - WOPR / NORAD  (WarGames, 1983)
//! Phosphor-green command console: typewriter greeting, blinking cursor, the
//! game menu, DEFCON badge and a scrolling system log.
//!
//! The clock is the container's real local time and the DEFCON level comes from
//! the live feed - the same source the threat board uses. A wall display that
//! showed a different condition on two screens would be worse than useless.
use super::{cond_color, grid, sweep};
use super::Env;
use crate::config::Param;
use crate::canvas::Canvas;

const G: u32 = 0x33ff66;
const GD: u32 = 0x186b32;
const AMBER: u32 = 0xffb000;
const DIM: u32 = 0x0e4a22;

const GAMES: &[&str] = &[
    "FALKEN'S MAZE",
    "BLACK JACK",
    "GIN RUMMY",
    "HEARTS",
    "BRIDGE",
    "CHECKERS",
    "CHESS",
    "POKER",
    "FIGHTER COMBAT",
    "GUERRILLA ENGAGEMENT",
    "DESERT WARFARE",
    "AIR-TO-GROUND ACTIONS",
    "THEATERWIDE TACTICAL WARFARE",
    "THEATERWIDE BIOTOXIC AND CHEMICAL WARFARE",
    "GLOBAL THERMONUCLEAR WAR",
];

const LOG: &[&str] = &[
    "TRACKING STATION 4 ONLINE",
    "SATELLITE UPLINK NOMINAL",
    "NORAD STATUS POLL",
    "BALLISTIC EARLY WARNING OK",
    "SECURE VOICE LINK 443",
    "RADAR PICKET SWEEP 12",
    "WEATHER BALLOON FALSE RETURN",
    "AUTH CODE ROTATION",
    "KANSAS SILO SITES REPORTING",
    "TACAMO AIRBORNE",
];

pub fn render(cv: &mut Canvas, env: &Env, t: f32) {
    let f = env.f;
    let feed = env.snap;
    let (w, h) = (cv.w, cv.h);
    grid(cv, 96.0, DIM, 40, t * 6.0);
    sweep(cv, t, 7.0, G);

    let pad = 64.0;
    let blink = (t * 1.6) as i32 % 2 == 0;

    // ---------------- header ----------------
    let hy = pad + 34.0;
    cv.glow_text(&f.a3270, 46.0, pad, hy, G, "NORAD  //  WOPR");

    let (hh, mm, ss) = crate::clock::now_hms();
    let clock = format!("{hh:02}:{mm:02}:{ss:02}");
    let cw = cv.text_w(&f.a3270, 40.0, &clock);
    cv.text(&f.a3270, 40.0, w - pad - cw, hy, G, 235, &clock);

    // condition comes from the feed, never hardcoded
    let level = feed.defcon();
    let dc = cond_color(level);
    let defc = format!("DEFCON {level}");
    let dw = cv.text_w(&f.a3270, 40.0, &defc);
    cv.text(
        &f.a3270,
        40.0,
        w - pad - cw - dw - 48.0,
        hy,
        if blink { dc } else { (dc >> 1) & 0x007f_7f7f },
        255,
        &defc,
    );
    cv.line(pad, hy + 24.0, w - pad, hy + 24.0, 2.0, GD, 210);

    // ---------------- centre typewriter ----------------
    let m1 = "GREETINGS PROFESSOR FALKEN.";
    let m2 = "SHALL WE PLAY A GAME?";
    let cps = 15.0f32;
    let n1 = ((t * cps) as usize).min(m1.len());
    let n2 = (((t - m1.len() as f32 / cps - 0.7).max(0.0) * cps) as usize).min(m2.len());

    let cx = pad + 30.0;
    let y1 = h * 0.27;
    cv.glow_text(&f.a3270, 62.0, cx, y1, G, &m1[..n1]);
    if n1 < m1.len() && blink {
        let x = cx + cv.text_w(&f.a3270, 62.0, &m1[..n1]);
        cv.rect(x + 4.0, y1 - 52.0, 34.0, 8.0, G, 255);
    }
    let y2 = y1 + 132.0;
    cv.glow_text(&f.a3270, 86.0, cx, y2, G, &m2[..n2]);
    if n2 < m2.len() && blink {
        let x = cx + cv.text_w(&f.a3270, 86.0, &m2[..n2]);
        cv.rect(x + 6.0, y2 - 72.0, 46.0, 10.0, G, 255);
    }

    // ---------------- game menu (2 col) ----------------
    let my = y2 + 96.0;
    cv.text(&f.a3270, 30.0, cx, my, GD, 230, "GAMES:");
    let sel = ((t / 1.5) as usize) % GAMES.len();
    let col_x = [cx + 40.0, cx + 760.0];
    let rows = GAMES.len().div_ceil(2);
    for (i, game) in GAMES.iter().enumerate() {
        let c = i / rows;
        let r = i % rows;
        let x = col_x[c.min(1)];
        let yy = my + 44.0 + r as f32 * 34.0;
        let is_sel = i == sel;
        let rgb = if is_sel { if blink { 0xffffff } else { AMBER } } else { G };
        let mark = if is_sel { ">" } else { " " };
        let label = format!("{mark} {}. {game}", i + 1);
        cv.text(&f.a3270, 28.0, x, yy, rgb, if is_sel { 255 } else { 210 }, &label);
    }
    let gsel = format!("GAME SELECTED: {}", GAMES[sel]);
    cv.text(&f.a3270, 30.0, cx, my + 44.0 + rows as f32 * 34.0 + 30.0, AMBER, 235, &gsel);

    // ---------------- right rail: system log ----------------
    let rx = w * 0.70;
    cv.line(rx - 30.0, hy + 40.0, rx - 30.0, h - pad - 40.0, 1.5, GD, 150);
    cv.text(&f.a3270, 28.0, rx, hy + 70.0, GD, 235, "SYSTEM LOG");
    let base = (t / 1.4) as u32;
    let log_rows = env.cfg.i("wopr.log_rows", 14).clamp(1, 24) as u32;
    for i in 0..log_rows {
        let ln = base + i;
        let item = LOG[(ln as usize) % LOG.len()];
        let yy = hy + 112.0 + i as f32 * 30.0;
        if yy > h - pad - 30.0 {
            break;
        }
        cv.text(&f.a3270, 21.0, rx, yy, GD, 160, &format!("{ln:05}  {item}"));
    }

    // ---------------- footer ----------------
    let fy = h - pad - 18.0;
    cv.line(pad, fy - 34.0, w - pad, fy - 34.0, 2.0, GD, 200);
    cv.text(&f.a3270, 26.0, pad, fy, GD, 220, "CHEYENNE MOUNTAIN COMPLEX  //  USER: FALKEN  //  TTY-33");
    let hint = "AWAITING INPUT_";
    let hw = cv.text_w(&f.a3270, 26.0, hint);
    cv.text(&f.a3270, 26.0, w - pad - hw, fy, if blink { G } else { GD }, 235, hint);

    // ---------------- CRT finish ----------------
}

/// Knobs this screen owns, surfaced by the config server.
pub fn params() -> Vec<Param> {
    vec![Param::i(
        "wopr.log_rows",
        "System log rows",
        14,
        4.0,
        16.0,
        "Lines of NORAD chatter in the right-hand log. Longer logs scroll faster.",
    )]
}
