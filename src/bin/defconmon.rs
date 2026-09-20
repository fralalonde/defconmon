//! The live display binary (`defconmon`).
//!
//! Every mode except the configured server run from here: headless preview
//! (`--out PNG`), benchmarking (`--bench`), and the live full-screen loop. The
//! config server lives in its own binary (`defconmon-config`) so this artifact
//! is never clobbered by a `--features web` build - the exact collision that
//! took the family's screen down once.
use defconmon::config::Cfg;
use defconmon::data::Snap;
use defconmon::fonts::Fonts;
use defconmon::screens::Env;
use defconmon::{arg, handle_introspect};

const USAGE: &str = "\
defconmon - retro-styled HDMI dashboard for a Proxmox homelab.

USAGE:
  defconmon [--config PATH] --list | --params | --help | --version
  defconmon [--config PATH] [--w W --h H --t T --feed PATH] --screen NAME --out PNG
  defconmon [--config PATH] [--w W --h H] --bench N [--scale S]
  defconmon [--config PATH]                      # live display

OPTIONS:
  --list       print every registered screen (index, name, title)
  --params     print every setting (key, kind, default) to stdout
  --config P   config file (default /etc/defconmon/config.json)
  --w W --h H  preview/bench resolution (default 1920x1080)
  --t T        preview time offset in seconds
  --feed PATH  feed JSON to render instead of the live one (e.g. a sample);
               default reads /run/dashboard/dashboard.json then ./dashboard.json
  --screen N   preview screen name (default wopr)
  --out PNG    headless preview: render one frame and write it
  --bench N    render N frames and report ms/frame against the CPU budget
  --scale S    bench scale factor (default 0.5)
  --serve PORT (web build only) run the config server - see defconmon-config";

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if handle_introspect(&args, USAGE) {
        return;
    }

    // ---- introspection: the registry is the single source of truth --------
    if args.iter().any(|a| a == "--list") {
        for (i, s) in defconmon::screens::SCREENS.iter().enumerate() {
            println!("{i}\t{}\t{}", s.name, s.title);
        }
        return;
    }
    if args.iter().any(|a| a == "--params") {
        for p in defconmon::config::global_params()
            .into_iter()
            .chain(defconmon::screens::all_params())
        {
            println!("{}\t{:?}\t{}", p.key, p.kind, p.default);
        }
        return;
    }

    let path = arg(&args, "--config")
        .unwrap_or_else(|| defconmon::config::DEFAULT_PATH.to_string());
    let cfg = Cfg::load(path);

    // ---- config server: a separate process, so it cannot disturb the display
    #[cfg(feature = "web")]
    if let Some(port) = arg(&args, "--serve").and_then(|s| s.parse::<u16>().ok()) {
        // serve() is `-> !`; the display loop below never runs on this path
        defconmon::web::serve(cfg, port);
    }
    #[cfg(not(feature = "web"))]
    if args.iter().any(|a| a == "--serve") {
        eprintln!(
            "defconmon: built without the `web` feature; run defconmon-config instead \
             (or rebuild without --no-default-features)"
        );
        std::process::exit(2);
    }

    let w: u32 = arg(&args, "--w").and_then(|s| s.parse().ok()).unwrap_or(1920);
    let h: u32 = arg(&args, "--h").and_then(|s| s.parse().ok()).unwrap_or(1080);
    let t: f32 = arg(&args, "--t").and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let screen = arg(&args, "--screen").unwrap_or_else(|| "wopr".into());
    let out = arg(&args, "--out");

    let fonts = Fonts::load();
    let mut snap = Snap::load_feed(arg(&args, "--feed").as_deref());

    // ---- headless preview: render one frame to a PNG ----------------------
    if let Some(out) = out {
        if snap.hist.is_empty() {
            // no live trend off-box: seed a preview history so FIG2 has data
            snap.seed_history();
        }
        let mut cv = defconmon::canvas::Canvas::new(w, h);
        let env = Env { cfg: &cfg, snap: &snap, f: &fonts, gpu_crt: false };
        defconmon::screens::render(&screen, &mut cv, &env, t);
        std::fs::write(&out, cv.png()).expect("write png");
        println!(
            "wrote {out}  {w}x{h}  screen={screen}  t={t:.1}s  feed_live={}",
            snap.live
        );
        return;
    }

    // ---- benchmark: what one screen costs, against the resource budget ----
    if let Some(n) = arg(&args, "--bench").and_then(|s| s.parse::<u32>().ok()) {
        if snap.hist.is_empty() {
            snap.seed_history();
        }
        // 0.5 matches the live path; vary it to see how cost scales with pixels
        let scale: f32 =
            arg(&args, "--scale").and_then(|s| s.parse().ok()).unwrap_or(0.5);
        let mut cv = defconmon::canvas::Canvas::with_scale(w, h, scale);
        let env = Env { cfg: &cfg, snap: &snap, f: &fonts, gpu_crt: false };
        for i in 0..3 {
            cv.clear();
            defconmon::screens::render(&screen, &mut cv, &env, i as f32 * 0.1);
        }
        let t0 = std::time::Instant::now();
        for i in 0..n {
            cv.clear();
            defconmon::screens::render(&screen, &mut cv, &env, i as f32 / 12.0);
        }
        let ms = t0.elapsed().as_secs_f64() * 1000.0 / n as f64;
        println!(
            "{screen:<10} {ms:>6.1} ms/frame   {:>5.1}% of one core at 12 fps",
            ms * 1.2
        );
        return;
    }

    // ---- live: put it on the compositor ----------------------------------
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