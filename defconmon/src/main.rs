mod canvas;
mod clock;
mod config;
mod data;
mod fonts;
#[cfg(feature = "live")]
mod gpu;
#[cfg(feature = "live")]
mod live;
mod screens;
#[cfg(feature = "web")]
mod web;

use canvas::Canvas;
use config::Cfg;
use data::Snap;
use fonts::Fonts;
use screens::Env;

fn arg(args: &[String], k: &str) -> Option<String> {
    args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // ---- introspection: the registry is the single source of truth --------
    if args.iter().any(|a| a == "--list") {
        for (i, s) in screens::SCREENS.iter().enumerate() {
            println!("{i}\t{}\t{}", s.name, s.title);
        }
        return;
    }
    if args.iter().any(|a| a == "--params") {
        for p in config::global_params().into_iter().chain(screens::all_params()) {
            println!("{}\t{:?}\t{}", p.key, p.kind, p.default);
        }
        return;
    }

    let path = arg(&args, "--config").unwrap_or_else(|| config::DEFAULT_PATH.to_string());
    let cfg = Cfg::load(path);

    // ---- config server: a separate process, so it cannot disturb the display
    #[cfg(feature = "web")]
    if let Some(port) = arg(&args, "--serve").and_then(|s| s.parse::<u16>().ok()) {
        web::serve(cfg, port);
        return;
    }
    #[cfg(not(feature = "web"))]
    if args.iter().any(|a| a == "--serve") {
        eprintln!("defconmon: built without the `web` feature - rebuild with --features web");
        std::process::exit(2);
    }

    let w: u32 = arg(&args, "--w").and_then(|s| s.parse().ok()).unwrap_or(1920);
    let h: u32 = arg(&args, "--h").and_then(|s| s.parse().ok()).unwrap_or(1080);
    let t: f32 = arg(&args, "--t").and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let screen = arg(&args, "--screen").unwrap_or_else(|| "wopr".into());
    let out = arg(&args, "--out");

    let fonts = Fonts::load();
    let mut snap = Snap::load();

    // ---- headless preview: render one frame to a PNG ----------------------
    if let Some(out) = out {
        if snap.hist.is_empty() {
            // no live trend off-box: seed a preview history so FIG2 has data
            snap.seed_history();
        }
        let mut cv = Canvas::new(w, h);
        let env = Env { cfg: &cfg, snap: &snap, f: &fonts, gpu_crt: false };
        screens::render(&screen, &mut cv, &env, t);
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
        let scale: f32 = arg(&args, "--scale").and_then(|s| s.parse().ok()).unwrap_or(0.5);
        let mut cv = Canvas::with_scale(w, h, scale);
        let env = Env { cfg: &cfg, snap: &snap, f: &fonts, gpu_crt: false };
        for i in 0..3 {
            cv.clear();
            screens::render(&screen, &mut cv, &env, i as f32 * 0.1);
        }
        let t0 = std::time::Instant::now();
        for i in 0..n {
            cv.clear();
            screens::render(&screen, &mut cv, &env, i as f32 / 12.0);
        }
        let ms = t0.elapsed().as_secs_f64() * 1000.0 / n as f64;
        println!(
            "{screen:<10} {ms:>6.1} ms/frame   {:>5.1}% of one core at 12 fps",
            ms * 1.2
        );
        return;
    }

    // ---- live: put it on the compositor ----------------------------------
    #[cfg(feature = "live")]
    {
        eprintln!("defconmon: live  config={}", cfg.path.display());
        let conn = match wayland_client::Connection::connect_to_env() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("defconmon: cannot connect to a Wayland display: {e}");
                std::process::exit(1);
            }
        };
        live::run(&conn, cfg);
    }

    #[cfg(not(feature = "live"))]
    {
        let _ = (w, h, screen, cfg);
        eprintln!("defconmon: built without the `live` feature; use --out FILE to render a PNG");
        std::process::exit(2);
    }
}
