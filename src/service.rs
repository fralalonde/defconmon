//! Systemd unit management for defconmon (`service install` / `service remove`).
//!
//! Installs GENERIC, public-safe units that point the display and the config
//! server at whatever real paths `current_exe()` and the `--config` value
//! resolve to, so it works wherever the binaries were actually installed. The
//! units deliberately carry no hostnames, no usernames and no hardcoded
//! timezone - an admin may add their own TZ or seat per box.
//!
//! `--dry-run` prints exactly what would be written and which commands would
//! run, touching nothing - that is how this code is verified off-box.

use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Where the generated display unit lives.
pub const DISPLAY_UNIT: &str = "/etc/systemd/system/defconmon.service";
/// Where the generated config-server unit lives (when that binary is present).
pub const CONFIG_UNIT: &str = "/etc/systemd/system/defconmon-config.service";
/// Port the generated config-server unit serves on. This is the documented
/// default for the homelab; edit the unit if yours differs.
const CONFIG_PORT: u16 = 8080;

/// Options gathered by the CLI layer for a service operation.
pub struct ServiceOpts {
    /// Resolved from the global `--config` (default /etc/defconmon/config.json).
    /// The generated units point at this file.
    pub config_path: PathBuf,
    /// `true` = print everything, change nothing. Works without root so it can
    /// be auditioned safely before a real install.
    pub dry_run: bool,
}

// ---------------------------------------------------------------------------

/// `service install`: write, enable and start the units.
pub fn install(opts: &ServiceOpts) -> Result<(), String> {
    require_root(opts.dry_run)?;
    let bin = current_bin()?;
    let cage = cage_on_path();

    let mode = if cage { "wrap (cage)" } else { "direct" };
    println!(
        "defconmon: service install  binary={}  config={}  display={mode}",
        bin.display(),
        opts.config_path.display()
    );

    write_unit(
        DISPLAY_UNIT,
        &display_unit_text(&bin, &opts.config_path, cage),
        opts.dry_run,
    )?;
    let mut units = vec!["defconmon.service".to_string()];

    // The config server is a separate binary. Only ship its unit when a real
    // defconmon-config sits next to the display binary - otherwise the unit
    // would point at a binary that does not exist and restart-loop forever.
    if let Some(cb) = config_server_bin(bin.parent().unwrap_or(bin.as_path())) {
        write_unit(
            CONFIG_UNIT,
            &config_unit_text(&cb, &opts.config_path),
            opts.dry_run,
        )?;
        units.push("defconmon-config.service".to_string());
        println!(
            "defconmon: config-server unit included (binary found: {})",
            cb.display()
        );
    } else {
        println!("defconmon: no defconmon-config binary beside the display; skipping its unit");
    }

    run(
        Command::new("systemctl").arg("daemon-reload"),
        opts.dry_run,
        "systemctl daemon-reload",
    )?;
    let enable = format!("systemctl enable --now {}", units.join(" "));
    run(
        Command::new("systemctl")
            .args(["enable", "--now"])
            .args(&units),
        opts.dry_run,
        &enable,
    )?;

    if opts.dry_run {
        println!("[dry-run] nothing changed.");
    }
    Ok(())
}

/// `service remove`: stop, disable, delete the unit files, re-read systemd.
/// Deliberately does NOT touch the config file or the binaries.
pub fn remove(opts: &ServiceOpts) -> Result<(), String> {
    require_root(opts.dry_run)?;
    println!("defconmon: service remove");
    println!(
        "  leaving the config file ({}) and the binaries untouched",
        opts.config_path.display()
    );

    for (name, path) in ["defconmon.service", "defconmon-config.service"]
        .iter()
        .zip([DISPLAY_UNIT, CONFIG_UNIT].iter())
    {
        if Path::new(path).exists() {
            let op = format!("systemctl disable --now {name}");
            if opts.dry_run {
                println!("[dry-run] would run: {op}");
            } else {
                println!("running: {op}");
                match Command::new("systemctl")
                    .args(["disable", "--now"])
                    .arg(name)
                    .status()
                {
                    Ok(s) if s.success() => {}
                    Ok(s) => println!("note: {op} exited {s}; continuing with removal"),
                    Err(e) => println!("note: cannot {op}: {e}"),
                }
            }
        }
        if opts.dry_run {
            println!("[dry-run] would remove {path}");
        } else {
            match std::fs::remove_file(path) {
                Ok(_) => println!("removed {path}"),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("cannot remove {path}: {e}")),
            }
        }
    }

    run(
        Command::new("systemctl").arg("daemon-reload"),
        opts.dry_run,
        "systemctl daemon-reload",
    )?;

    if opts.dry_run {
        println!("[dry-run] nothing changed.");
    } else {
        println!("defconmon: units removed. Left the config file and binaries in place - remove those by hand if you really want them gone.");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// unit text
// ---------------------------------------------------------------------------

/// The display unit. `cage` (a fullscreen Wayland kiosk compositor) wraps the
/// binary; without `cage` we run the display directly against whatever
/// compositor owns the seat.
fn display_unit_text(bin: &Path, cfg: &Path, cage: bool) -> String {
    let exec = if cage {
        // /usr/bin/cage is the canonical path on the distros this targets;
        // PATH is only used to decide whether to wrap at all.
        format!("/usr/bin/cage -- {} --config {}", quote(bin), quote(cfg))
    } else {
        format!("{} --config {}", quote(bin), quote(cfg))
    };
    format!(
        r#"# defconmon.service - generated by `defconmon service install`. Rerun to refresh.
#
# WHY a compositor is required: defconmon is a pure Wayland client - it draws
# into a wl_surface and has no DRM/KMS path of its own, so it can only present
# inside a running compositor. `cage` is a minimal kiosk compositor that owns
# the console seat and hands the display a single fullscreen surface.
#
# Seat/TTY: as a fullscreen GUI this unit expects to run on an active local
# seat (e.g. loginctl seat0). It is started with systemd, not a display manager,
# so make sure something is logged in on the console/VT, or the compositor has
# nothing to present to.
#
# On systems without `cage` the binary runs directly against the compositor
# that is already on the seat.

[Unit]
Description=defconmon - retro-styled full-screen HDMI dashboard
After=network.target

[Service]
Type=simple
ExecStart={exec}
Restart=always
RestartSec=3
# Timezone is deliberately unset: the display shows a wall clock in local time,
# so it follows the environment. If your system clock is UTC and this is a
# local-time wall display, set it here (only affects this service):
# Environment=TZ=

[Install]
WantedBy=multi-user.target
"#,
        exec = exec
    )
}

/// The config-server unit. Generic and public-safe: no username is hardcoded -
/// see the comment in the unit about running it unprivileged.
fn config_unit_text(bin: &Path, cfg: &Path) -> String {
    let exec = format!(
        "{} --config {} --serve {CONFIG_PORT}",
        quote(bin),
        quote(cfg)
    );
    format!(
        r#"# defconmon-config.service - generated by `defconmon service install`.
#
# The separate process that edits the config file over the LAN. It is a companion
# to the display unit, not a dependency: the display only ever reads the config
# file, so a crash or a config edit can never take the display down.
#
# No User=/Group= is set here on purpose: the unit must run on boxes with no
# dedicated service account. systemd runs it as root unless you add a
# `User=`/`Group=` of your choice below. Unprivileged running is recommended
# when you trust the box.

[Unit]
Description=defconmon config web server (HTTP, LAN only)
After=network.target

[Service]
Type=simple
ExecStart={exec}
Restart=always
RestartSec=3

[Install]
WantedBy=multi-user.target
"#,
        exec = exec
    )
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn require_root(dry_run: bool) -> Result<(), String> {
    if dry_run {
        return Ok(());
    }
    // A real install/remove must not half-write as a user. --dry-run is the
    // non-root path: it changes nothing, so it is safe for anyone to inspect.
    #[allow(unsafe_code)] // trivial single syscall; libc is already a dep
    if unsafe { libc::geteuid() } != 0 {
        return Err("service install/remove must run as root (use `sudo`). \
             --dry-run works without root so you can audition it first"
            .into());
    }
    Ok(())
}

/// The real path of the running `defconmon` binary. The units point at this.
fn current_bin() -> Result<PathBuf, String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("cannot resolve my own executable path: {e}"))?;
    Ok(exe.canonicalize().unwrap_or(exe))
}

/// Is a `cage` binary on PATH? (Used to decide whether to wrap under cage.)
fn cage_on_path() -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|dir| dir.join("cage").is_file()))
        .unwrap_or(false)
}

/// A `defconmon-config` binary sitting beside ours means the config server is
/// installed too; return its path when usable.
fn config_server_bin(dir: &Path) -> Option<PathBuf> {
    let p = dir.join("defconmon-config");
    is_executable(&p).then_some(p)
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = CString::new(p.as_os_str().as_bytes()) else {
        return false;
    };
    // X_OK: exists and is searchable/executable. Root can traverse anything,
    // so access() is a lower bound, not a guarantee - good enough to decide
    // whether to write the config-server unit.
    unsafe { libc::access(c.as_ptr(), libc::X_OK) == 0 }
}

fn write_unit(path: &str, text: &str, dry_run: bool) -> Result<(), String> {
    if dry_run {
        println!("[dry-run] would write {path}:");
        println!("{text}");
        return Ok(());
    }
    std::fs::write(path, text).map_err(|e| format!("cannot write {path}: {e}"))?;
    println!("wrote {path}");
    Ok(())
}

fn run(cmd: &mut Command, dry_run: bool, label: &str) -> Result<(), String> {
    if dry_run {
        println!("[dry-run] would run: {label}");
        return Ok(());
    }
    println!("running: {label}");
    let status = cmd
        .status()
        .map_err(|e| format!("cannot run {label}: {e}"))?;
    if !status.success() {
        return Err(format!("`{label}` exited with {status}"));
    }
    Ok(())
}

/// Quote a path for ExecStart. systemd splits the line on whitespace unless a
/// token is quoted; /usr paths have no spaces, but quoting keeps the unit
/// correct if the binaries were installed somewhere with spaces.
fn quote(p: &Path) -> String {
    format!("\"{}\"", p.display())
}
