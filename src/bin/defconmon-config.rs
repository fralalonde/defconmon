//! The config web server binary (`defconmon-config`).
//!
//! A separate process with its own artifact name, so this build can never
//! overwrite the display binary (`defconmon`). Built with `--features web`; it
//! serves the shared config schema over std-only HTTP + htmx and writes the
//! config file the display watches by mtime.
use clap::{CommandFactory, FromArgMatches, Parser};
use defconmon::config::Cfg;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "defconmon-config",
    about = "Web server for the defconmon config file (HTTP on the LAN)."
)]
struct ConfigCli {
    /// Config file to read and write (default /etc/defconmon/config.json)
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
    /// Bind the HTTP server on this port (required)
    #[arg(long, value_name = "PORT")]
    serve: Option<u16>,
}

fn main() {
    let cli = ConfigCli::from_arg_matches(
        &ConfigCli::command()
            .version(defconmon::version())
            .get_matches(),
    )
    .unwrap_or_else(|e| e.exit());

    let path = cli
        .config
        .clone()
        .unwrap_or_else(|| defconmon::config::DEFAULT_PATH.into());

    // the server is useless without a port; fail loudly rather than serve nothing
    let Some(port) = cli.serve else {
        eprintln!("defconmon-config: --serve PORT is required");
        eprintln!("{}", ConfigCli::command().render_help());
        std::process::exit(2);
    };

    let cfg = Cfg::load(path);
    defconmon::web::serve(cfg, port);
}
