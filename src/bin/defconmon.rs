//! The live display binary (`defconmon`).
//!
//! Every display task runs from here as a subcommand: headless preview
//! (`preview --out PNG`), benchmarking (`bench N`), `list` / `params`
//! introspection, systemd management (`service install` / `service remove`),
//! and — with no subcommand — the live full-screen loop. The config server
//! lives in its own binary (`defconmon-config`), so this artifact is never
//! clobbered by a `--features web` build — the exact collision that took the
//! family's screen down once.
//!
//! `defconmon --config /etc/defconmon/config.json` (no subcommand) still starts
//! the live display, exactly as the deployed box runs it. `--config` is a
//! global option, accepted before or after any subcommand.
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use defconmon::config::{self, Cfg};
use defconmon::data::Snap;
use defconmon::fonts::Fonts;
use defconmon::screens::Env;
use defconmon::service::ServiceOpts;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "defconmon",
    about = "Retro-styled, full-screen HDMI dashboard for a Proxmox homelab.",
    after_help = "With no subcommand, runs the live full-screen display loop (the \
        behaviour of the older flag-based CLI)."
)]
struct Cli {
    /// Path to the config file (default /etc/defconmon/config.json)
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Render one frame headlessly and write it to a PNG (no display needed).
    Preview {
        /// Screen to render
        #[arg(long, value_name = "NAME")]
        screen: String,
        /// Output PNG path
        #[arg(long, value_name = "PNG")]
        out: String,
        /// Render width in px
        #[arg(long, value_name = "W", default_value_t = 1920)]
        w: u32,
        /// Render height in px
        #[arg(long, value_name = "H", default_value_t = 1080)]
        h: u32,
        /// Animation time offset in seconds
        #[arg(long, value_name = "T", default_value_t = 0.0)]
        t: f32,
        /// Feed JSON to render instead of the live one (e.g. a sample)
        #[arg(long, value_name = "PATH")]
        feed: Option<String>,
    },
    /// Render N frames and report ms/frame against the CPU budget.
    Bench {
        /// Number of frames to render
        n: u32,
        /// Render scale factor (default 0.5)
        #[arg(long, value_name = "S")]
        scale: Option<f32>,
    },
    /// Print every registered screen (index, name, title).
    List,
    /// Print every setting (key, kind, default).
    Params,
    /// Install or remove the systemd units that run this program.
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
}

#[derive(Subcommand)]
enum ServiceAction {
    /// Write, enable and start the systemd units.
    Install {
        /// Print what would change without touching the system.
        #[arg(long)]
        dry_run: bool,
    },
    /// Stop, disable and remove the systemd units (config file and binaries
    /// are left in place).
    Remove {
        /// Print what would change without touching the system.
        #[arg(long)]
        dry_run: bool,
    },
}

fn main() {
    // clap handles --help/--version here: get_matches prints them and exits.
    // version() carries the injected DEFCONMON_VERSION (git tag) with the
    // CARGO_PKG_VERSION fallback, matching the old hand-rolled parser.
    let cli = Cli::from_arg_matches(&Cli::command().version(defconmon::version()).get_matches())
        .unwrap_or_else(|e| e.exit());

    let config_path: PathBuf = cli
        .config
        .clone()
        .unwrap_or_else(|| config::DEFAULT_PATH.into());

    match cli.command {
        Some(Command::Service { action }) => {
            let res = match action {
                ServiceAction::Install { dry_run } => defconmon::service::install(&ServiceOpts {
                    config_path,
                    dry_run,
                }),
                ServiceAction::Remove { dry_run } => defconmon::service::remove(&ServiceOpts {
                    config_path,
                    dry_run,
                }),
            };
            if let Err(e) = res {
                eprintln!("defconmon: {e}");
                std::process::exit(1);
            }
        }
        Some(Command::List) => {
            for (i, s) in defconmon::screens::SCREENS.iter().enumerate() {
                println!("{i}\t{}\t{}", s.name, s.title);
            }
        }
        Some(Command::Params) => {
            for p in config::global_params()
                .into_iter()
                .chain(defconmon::screens::all_params())
            {
                println!("{}\t{:?}\t{}", p.key, p.kind, p.default);
            }
        }
        Some(Command::Preview {
            screen,
            out,
            w,
            h,
            t,
            feed,
        }) => {
            let cfg = Cfg::load(&config_path);
            let fonts = Fonts::load();
            let mut snap = Snap::load_feed(feed.as_deref());
            if snap.hist.is_empty() {
                // no live trend off-box: seed a preview history so FIG2 has data
                snap.seed_history();
            }
            let mut cv = defconmon::canvas::Canvas::new(w, h);
            let env = Env {
                cfg: &cfg,
                snap: &snap,
                f: &fonts,
                gpu_crt: false,
            };
            defconmon::screens::render(&screen, &mut cv, &env, t);
            std::fs::write(&out, cv.png()).expect("write png");
            println!(
                "wrote {out}  {w}x{h}  screen={screen}  t={t:.1}s  feed_live={}",
                snap.live
            );
        }
        Some(Command::Bench { n, scale }) => {
            let cfg = Cfg::load(&config_path);
            let screen = "wopr";
            let fonts = Fonts::load();
            let mut snap = Snap::load_feed(None);
            if snap.hist.is_empty() {
                snap.seed_history();
            }
            // 0.5 matches the live path; vary it to see how cost scales with pixels
            let scale: f32 = scale.unwrap_or(0.5);
            let mut cv = defconmon::canvas::Canvas::with_scale(1920, 1080, scale);
            let env = Env {
                cfg: &cfg,
                snap: &snap,
                f: &fonts,
                gpu_crt: false,
            };
            for i in 0..3 {
                cv.clear();
                defconmon::screens::render(screen, &mut cv, &env, i as f32 * 0.1);
            }
            let t0 = std::time::Instant::now();
            for i in 0..n {
                cv.clear();
                defconmon::screens::render(screen, &mut cv, &env, i as f32 / 12.0);
            }
            let ms = t0.elapsed().as_secs_f64() * 1000.0 / n as f64;
            println!(
                "{screen:<10} {ms:>6.1} ms/frame   {:>5.1}% of one core at 12 fps",
                ms * 1.2
            );
        }
        None => {
            // ---- live: put it on the compositor (the no-subcommand path) ----
            let cfg = Cfg::load(&config_path);
            eprintln!("defconmon: live  config={}", cfg.path.display());
            let conn = match wayland_client::Connection::connect_to_env() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("defconmon: cannot connect to a Wayland display: {e}");
                    std::process::exit(1);
                }
            };
            defconmon::live::run(&conn, cfg);
        }
    }
}
