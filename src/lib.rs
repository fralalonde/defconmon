//! defconmon - a retro-styled full-screen HDMI dashboard for a Proxmox homelab.
//!
//! One library shared by two binaries so their build artifacts never collide:
//!   - `defconmon` (src/bin/defconmon.rs)        the live display
//!   - `defconmon-config` (src/bin/defconmon-config.rs)  the web config server
//!
//! The display is CPU-only unless the `gpu` feature is added: tiny-skia + real
//! glyph outlines rasterise full-screen, and an optional wgpu/EGL path takes the
//! upscale and CRT finish. `live` supplies the pure-Rust Wayland present path.
pub mod canvas;
pub mod clock;
pub mod config;
pub mod data;
pub mod fonts;
#[cfg(feature = "gpu")]
pub mod gpu;
#[cfg(feature = "live")]
pub mod live;
pub mod screens;
#[cfg(feature = "web")]
pub mod web;

// Version contract: `DEFCONMON_VERSION` is stamped by build.rs when present;
// falling back to CARGO_PKG_VERSION means the binary reports a sane version even
// before build.rs has run (or if it is ever removed). A plain fn, not a const:
// `Option::unwrap_or` is not const-stable.
pub fn version() -> &'static str {
    option_env!("DEFCONMON_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
}

/// Find the value that follows `k` in `args` (e.g. `--config path`).
pub fn arg(args: &[String], k: &str) -> Option<String> {
    args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned()
}

/// Handle `--help`/`--version`; returns true when one was consumed so main
/// can bail out before touching a config file. Shared by both binaries.
pub fn handle_introspect(args: &[String], usage: &str) -> bool {
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("defconmon {}", version());
        return true;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{usage}");
        return true;
    }
    false
}