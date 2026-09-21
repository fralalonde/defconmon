# defconmon

Retro-styled, full-screen local dashboard for a Proxmox homelab.
Renders a "home sector defence" wall display, cycling five full-screen scenes
with a persistent status plate (DEFCON / WAN throughput / PING / DNS) on every
screen. A standalone dashboard — its own thing, not a wrapper over another
monitor.

![defconmon screens](docs/defconmon.gif)

## Screens

Five scenes cycle, 20 s each by default. The status plate is relocated screen to
screen so no pixels are lit twice in a row (panel burn-in).

| name | shows |
|---|---|
| `wopr` | WOPR console: typewriter log, game menu, real DEFCON badge, scrolling system log |
| `defcon` | Threat board from live infra state: WAN boundary, traffic pulses, WAN throughput |
| `radar` | ATC PPI scope: rotating sweep, contacts = running PVE guests (blips, trails, data blocks) |
| `telemetry` | Oscilloscope + spectrum + readouts driven by real WAN rate and CPU |
| `vectrex` | Wireframe icosahedra, one vertex per channel (magnitude/volatility) |

All five draw the **status plate** (`hud.rs`): DEFCON, WAN, PING, DNS, colour
coded green/amber/red. The DEFCON cell anchors it so it stays recognisable at
whichever position.

## Build

x86_64 Linux only. Two features, both **on by default**:

- `gpu` — wgpu present path (EGL/Wayland via dlopen; see DEV.md)
- `web` — the config server binary `defconmon-config`

```sh
cargo build --release                                        # default: GPU present path + config server
cargo build --release --no-default-features                   # CPU-only display, no wgpu (~2 MB vs ~5.4 MB)
cargo build --release --no-default-features --features web    # CPU-only display + config server
cargo build --release --bin defconmon-config                  # the config server binary
```

Rendering pipeline:

- **tiny-skia** — pure-CPU vector rasteriser; **ab_glyph** — real glyph outlines
  for text. Fonts (IBM 3270, DSEG7, Px VGA, Terminus) are embedded in the binary.
- The GPU path (wgpu 30, GL/EGL backend) only does the nearest-neighbour upscale
  and the CRT finish; the fonts and vector art **always** rasterise on the CPU.
  The look survives with the GPU off.

## Command line

`defconmon` is a subcommand CLI (clap). `--config` is a **global** option, accepted
before or after a subcommand, and always defaults to `/etc/defconmon/config.json`.

```sh
defconmon --config /etc/defconmon/config.json      # live display (no subcommand)
defconmon [--config P] list                        # registered screens
defconmon [--config P] params                      # every setting (key, kind, default)
defconmon [--config P] preview --screen wopr --out frame.png \
        [--w 1920 --h 1080] [--t 0] [--feed path]  # headless one-frame render
defconmon [--config P] bench 30 [--scale 0.5]      # ms/frame against the CPU budget
defconmon [--config P] service install [--dry-run] # write + enable systemd units
defconmon [--config P] service remove  [--dry-run] # stop, disable, delete units
defconmon-config --config P --serve 8080           # the config web server (separate binary)
```

The no-subcommand form is the one the deployed box uses (`defconmon --config
/etc/defconmon/config.json` under `cage`); the old `--list` / `--params` /
`--bench` / `--screen` / `--out` flags moved to the subcommands above.

## Run / deploy

Display runs under cage (Wayland) on the compositor's HDMI output via the stock
`dashboard.service` plus a **drop-in override** — the stock unit is never
rewritten. Pacing and rotation live in `/etc/defconmon/config.json`, not the
unit, so tuning needs no restart.

| unit | runs |
|---|---|
| `dashboard.service` + `dashboard.service.d/defconmon.conf` | `cage -- /usr/local/bin/defconmon --config /etc/defconmon/config.json` |
| `defconmon-config.service` | `/usr/local/bin/defconmon-config --config /etc/defconmon/config.json --serve 8080` |

For a stock install (no existing `dashboard.service` binding), `defconmon
service install` writes generic, public-safe units at
`/etc/systemd/system/defconmon.service` (+ `defconmon-config.service` when the
config-server binary sits beside the display) and runs `systemctl daemon-reload`
and `systemctl enable --now` on them. It wraps the display in `cage` when `cage`
is on `PATH`, else runs the binary directly, and prints every path it writes and
every command it runs. `service remove` stops/disables and deletes the units but
never touches the config file or the binaries. Both accept `--dry-run` to show
what would change without touching the system (and work without root).

## Config

Flat key/value JSON at `/etc/defconmon/config.json`. The display polls its mtime
once a second and reloads — that file is the **only** channel between the config
server and the display, so a crashed web server cannot take the display down.

```json
{
  "defcon.pulse_rate": 0.9,
  "display.dwell": 20,
  "display.fps": 15,
  "display.screens": "wopr,defcon,radar,telemetry,vectrex",
  "hud.visible": true,
  "radar.sweep_rpm": 8.6,
  "telemetry.full_scale": 100,
  "vectrex.spin": 100,
  "wopr.log_rows": 14,
  "wopr.user": "PROFESSOR FALKEN"
}
```

Authoritative schema in `src/config.rs`. Sections: `display`, `hud`, `wopr`,
`defcon`, `radar`, `telemetry`, `vectrex`, `gpu`.

| key | kind | default | note |
|---|---|---|---|
| `display.fps` | int 4..30 | 12 | whole cost scales with this |
| `display.dwell` | int 5..300 | 20 | seconds per screen |
| `display.screens` | text | `wopr,defcon,radar,telemetry,vectrex` | comma order; drop one to take it out |
| `hud.visible` | bool | true | status plate on all screens |
| `gpu.mode` | choice | `auto` | `auto` / `on` / `off` (see DEV.md) |
| `wopr.log_rows` | int | 14 | |
| `wopr.user` | text | `PROFESSOR FALKEN` | addressee shown on the WOPR screen |
| `defcon.pulse_rate` | float | 0.9 | |
| `radar.sweep_rpm` | float | 8.6 | |
| `telemetry.full_scale` | float | 100 | |
| `vectrex.spin` | int | 100 | |

**Config web server** — separate process (the `defconmon-config` binary),
std-only HTTP + htmx (no web framework, no dependencies). Plain HTTP on the LAN,
no auth, by design. Edits `config.json`; validation is atomic (coercion and
range checks before anything is written). The UI walks the schema and never
names a screen or metric, so a new setting is one line in the screen that
consumes it and a new screen needs no UI change.

## Inspiration

WarGames' 1983 WOPR, the Vectrex vector console, the DEFCON strategy game, and
air-traffic-control approach displays: phosphor green, scanlines, range rings,
threat boards. The wall reads like the house is running its own NORAD — which,
on a home network, it more or less is.