//! defconmon - a retro-styled full-screen HDMI dashboard for a Proxmox homelab.
//!
//! One library shared by two binaries so their build artifacts never collide:
//!   - `defconmon` (src/bin/defconmon.rs)        the live display
//!   - `defconmon-config` (src/bin/defconmon-config.rs)  the web config server
//!
//! The display is CPU-only unless the `gpu` feature is added: tiny-skia + real
//! glyph outlines rasterise full-screen, and an optional wgpu/EGL path takes the
//! upscale and CRT finish. The present path itself (pure-Rust Wayland) is the
//! core of the display and is always compiled.
pub mod canvas;
pub mod clock;
pub mod config;
pub mod data;
pub mod fonts;
#[cfg(feature = "gpu")]
pub mod gpu;
pub mod live;
pub mod screens;
pub mod service;
#[cfg(feature = "web")]
pub mod web;

// Version contract: `DEFCONMON_VERSION` is stamped by build.rs when present;
// falling back to CARGO_PKG_VERSION means the binary reports a sane version even
// before build.rs has run (or if it is ever removed). A plain fn, not a const:
// `Option::unwrap_or` is not const-stable. clap reads this via
// `Cli::command().version(defconmon::version())` so `--version` always prints
// the injected value.
pub fn version() -> &'static str {
    option_env!("DEFCONMON_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
}
