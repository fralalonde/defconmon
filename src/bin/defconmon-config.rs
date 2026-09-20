//! The config web server binary (`defconmon-config`).
//!
//! A separate process with its own artifact name, so this build can never
//! overwrite the display binary (`defconmon`). Built with `--features web`; it
//! serves the shared config schema over std-only HTTP + htmx and writes the
//! config file the display watches by mtime.
use defconmon::config::Cfg;
use defconmon::{arg, handle_introspect};

const USAGE: &str = "\
defconmon-config - web server for the defconmon config file.

USAGE:
  defconmon-config [--config PATH] --serve PORT

OPTIONS:
  --config P   config file to read and write (default /etc/defconmon/config.json)
  --serve PORT bind the HTTP server on this port (required)";

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if handle_introspect(&args, USAGE) {
        return;
    }

    let path = arg(&args, "--config")
        .unwrap_or_else(|| defconmon::config::DEFAULT_PATH.to_string());

    // the server is useless without a port; fail loudly rather than serve nothing
    let Some(port) = arg(&args, "--serve").and_then(|s| s.parse::<u16>().ok()) else {
        eprintln!("defconmon-config: --serve PORT is required");
        eprintln!("{USAGE}");
        std::process::exit(2);
    };

    let cfg = Cfg::load(path);
    defconmon::web::serve(cfg, port);
}