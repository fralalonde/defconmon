//! Screen registry + shared scene helpers (grid, sweep, hash, condition colour).
//!
//! Every screen is one row in `SCREENS`: its name, a title, a one-line blurb,
//! the settings it exposes, and its render function. The CLI, the rotation and
//! the config server all walk this table and never hardcode a screen name -
//! which is what lets screens become plugins later without either of them
//! changing.
pub mod defcon;
pub mod hud;
pub mod radar;
pub mod telemetry;
pub mod vectrex;
pub mod wopr;

use crate::canvas::Canvas;
use crate::config::Param;
use crate::data::Snap;
use crate::fonts::Fonts;

/// Everything a screen needs to draw a frame. Bundled so that adding a new
/// shared input later is one field here, not five signatures.
pub struct Env<'a> {
    pub cfg: &'a crate::config::Cfg,
    pub snap: &'a Snap,
    pub f: &'a Fonts,
    /// True when the GPU present path owns the CRT finish (see `gpu.rs`). The
    /// CPU pass is then skipped, or the frame would be darkened twice.
    pub gpu_crt: bool,
}

pub struct ScreenDesc {
    pub name: &'static str,
    pub title: &'static str,
    pub blurb: &'static str,
    /// Settings this screen owns. Declared next to the code that reads them.
    pub params: fn() -> Vec<Param>,
    pub render: fn(&mut Canvas, &Env, f32),
}

pub const SCREENS: &[ScreenDesc] = &[
    ScreenDesc {
        name: "wopr",
        title: "WOPR COMMAND CONSOLE",
        blurb: "WarGames homage: the house condition plus a NORAD chatter log.",
        params: wopr::params,
        render: wopr::render,
    },
    ScreenDesc {
        name: "defcon",
        title: "THREAT BOARD",
        blurb: "Topology from the WAN boundary down to every guest, with the defence condition.",
        params: defcon::params,
        render: defcon::render,
    },
    ScreenDesc {
        name: "radar",
        title: "APPROACH CONTROL",
        blurb: "Proxmox guests plotted as radar contacts on a plan-position scope.",
        params: radar::params,
        render: radar::render,
    },
    ScreenDesc {
        name: "telemetry",
        title: "SIGNAL ANALYSIS",
        blurb: "WAN and CPU waveforms, a spectrum, and the numeric readouts.",
        params: telemetry::params,
        render: telemetry::render,
    },
    ScreenDesc {
        name: "vectrex",
        title: "METRIC SOLIDS",
        blurb: "Twelve channels as two wireframe solids: current state and volatility.",
        params: vectrex::params,
        render: vectrex::render,
    },
];

pub fn names() -> Vec<&'static str> {
    SCREENS.iter().map(|s| s.name).collect()
}

pub fn find(name: &str) -> Option<&'static ScreenDesc> {
    SCREENS.iter().find(|s| s.name == name)
}

/// Every screen-contributed setting, in registry order. The config server calls
/// this and never needs to know what a "wopr" is.
pub fn all_params() -> Vec<Param> {
    SCREENS.iter().flat_map(|s| (s.params)()).collect()
}

/// Draw one frame, in three layers:
///   1. the screen's own content,
///   2. the persistent status plate (so every screen reports the house state),
///   3. the CRT finish over both, so the plate reads as part of the glass.
pub fn render(name: &str, cv: &mut Canvas, env: &Env, t: f32) {
    if let Some(s) = find(name).or_else(|| SCREENS.first()) {
        (s.render)(cv, env, t);
    }
    if env.cfg.b("hud.visible", true) {
        hud::draw(cv, env.f, name, env.snap);
    }
    if !env.gpu_crt {
        cv.crt_finish(t);
    }
}

/// Cold-war condition colour for a DEFCON level (1 = war, 5 = peace).
/// Shared so every screen that shows a condition agrees on its colour.
pub fn cond_color(d: u32) -> u32 {
    match d {
        1 => 0xff3030,
        2 => 0xff6020,
        3 => 0xffb000,
        4 => 0x9fe870,
        _ => 0x38e8ff,
    }
}

/// Faint animated grid background (military plot table).
pub fn grid(cv: &mut Canvas, step: f32, rgb: u32, a: u8, drift: f32) {
    let (w, h) = (cv.w, cv.h);
    let mut x = (drift % step) - step;
    while x < w + step {
        cv.line(x, 0.0, x, h, 1.0, rgb, a);
        x += step;
    }
    let mut y = (drift * 0.5 % step) - step;
    while y < h + step {
        cv.line(0.0, y, w, y, 1.0, rgb, a);
        y += step;
    }
}

/// A bright horizontal sweep bar travelling down the screen (CRT refresh).
pub fn sweep(cv: &mut Canvas, t: f32, period: f32, rgb: u32) {
    let h = cv.h;
    let y = ((t % period) / period) * (h + 120.0) - 60.0;
    cv.rect(0.0, y, cv.w, 3.0, rgb, 26);
    cv.rect(0.0, y + 3.0, cv.w, 10.0, rgb, 9);
}

/// Deterministic hash -> 0..1 for pseudo-random scene content.
pub fn hashf(n: u32) -> f32 {
    let mut x = n.wrapping_mul(2654435761);
    x ^= x >> 15;
    x = x.wrapping_mul(2246822519);
    x ^= x >> 13;
    (x & 0xffffff) as f32 / 0xffffff as f32
}
