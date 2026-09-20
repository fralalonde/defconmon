# defconmon — retro computing dashboard for the HDMI monitor

Custom Rust dashboard for **CT 107** (`monitor`, 192.168.1.4) that drives the
Proxmox host's physical HDMI output. Replaces/augments conky with a set of
animated, full-screen retro scenes, plus a web config UI.

Aesthetic brief: **WarGames (1983) WOPR**, **Vectrex vector display**,
**DEFCON**, **ATC radar**. Scientific, military, ominous. Phosphor green,
scanlines, bloom, flicker, CRT vignette, animated backgrounds.

## Stack

- **Rust**, rendering with **tiny-skia** (pure-Rust CPU vector rasteriser — no GPU
  needed; the Caicos Radeon has no Vulkan).
- Text is drawn from real glyph **outlines** (ab_glyph) so it scales cleanly and
  can be stroked for phosphor glow.
- Fonts are **embedded** in the binary: IBM 3270 (thin plotter headings),
  DSEG7 Classic (segmented numerics), Px VGA SquarePx, Terminus.
- Static **musl** binary, ~2 MB, **zero runtime dependencies** — which matters
  because the CT is Debian 12 (glibc 2.36) and the build host is Fedora (2.43).
- The config server adds **no dependencies either**: `std::net::TcpListener` and
  a hand-rolled HTTP/1.1 response, with htmx vendored into the binary.

## Rendering: CPU default, GPU proven

The dashboard renders with **tiny-skia (pure-CPU vector rasteriser)** by default.
That path stays: one **static musl binary, zero runtime dependencies**, verified
to build for `x86_64` and `aarch64` musl alike, no `cfg(target_arch)`, no SIMD
intrinsics, no vendor code. It runs on anything.

A **GPU path is proven working** on this hardware (see `gpu-spike/`), and is
being built behind a feature so the CPU path remains the fallback:

```
adapters visible to wgpu: 1
  AMD CAICOS (DRM 2.51.0 / 7.0.14-16-pve, LLVM 15.0.6) | backend=Gl | 4.5 (Core Profile) Mesa 22.3.6
GPU post-process 960x540: 0.492 ms/frame
```

### What was established, by measurement

- **No Vulkan here** — the Caicos PRO (Radeon HD 7450, TeraScale 2) has no ICD on
  either box, so wgpu's Vulkan backend is unavailable. That says nothing about GL.
- **GLES 3.1 / GL 4.5 is available** via Mesa r600. wgpu's **GL backend works**
  and gives a real hardware adapter. This is the portable baseline: EGL + GLES
  exists on Mali, Intel, AMD and NVIDIA alike.
- **The CRT post-process belongs on the GPU**: measured at **0.492 ms/frame**
  against ~1.5 ms for the optimised CPU pass and ~9 ms for the original vector
  version. Output verified from the pixels (scanline modulation at the 3-px
  pitch; corners at 0.23x centre luminance).

### The cost, which is real

**A fully static musl binary cannot `dlopen` anything** — musl's static runtime
answers "Dynamic loading not supported", and Mesa's libEGL is glibc-linked. So
the GPU variant must be a **dynamically-linked glibc build**, produced in a
Debian-12 container so its glibc matches the container's (2.36):

```sh
podman run --rm -v "$PWD":/src:Z -w /src -e CARGO_TARGET_DIR=/src/target-debian \
  debian:12 bash -c 'apt-get update -qq && apt-get install -y -qq curl gcc libc6-dev \
  && curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable \
  && /root/.cargo/bin/cargo build --release --target x86_64-unknown-linux-gnu'
```

Compile time is ~30 s once the toolchain is in the image. This is the trade: the
GPU path is distro-matched and dynamically linked; the CPU path stays portable
and static. Keeping both is the point.

### The backend split (why `gles` also switches Wayland backend)

EGL needs a real `wl_display`/`wl_surface` to attach to. The dashboard's default
client is the **pure-Rust** wayland backend, which deliberately has no libwayland
objects at all - it speaks the socket directly - so there is no pointer to hand
to libwayland-egl. `display_ptr()` only exists on the C backend.

So the `gles` feature also enables `wayland-client/system`, and the split falls
out cleanly:

| build | Wayland backend | links | deps |
|---|---|---|---|
| `--features live` (default) | pure-Rust | static musl | none |
| `--features live,gles` | C libwayland (`system`) | dynamic glibc | libwayland, libEGL, libGLESv2 |

`src/gpu.rs` implements the present path (scene texture + one fragment shader
doing the nearest-neighbour upscale, scanlines and vignette, presented with
`Queue::present`). Both feature sets type-check against wgpu 30.

### GPU path: integrated, and where it currently stops

`src/gpu.rs` is wired into `live.rs`: the present path is chosen once at startup,
the GPU takes the upscale + CRT finish and presents via `Queue::present`, and the
CPU/shm path remains the fallback. The fallback is **proven under real failure** -
on the CT the GPU init failed and the dashboard carried on with:

```
defconmon: raw handles display=true surface=true (non-null)
defconmon: GPU path unavailable (no adapter: ... gl not compatible with
  provided surface); using the CPU/shm path
defconmon: surface 1920x1080, shm format Xbgr8888 (zero-copy)
```

So the plumbing is right (both raw handles are valid) and the blocker is inside
wgpu's GL backend: it refuses a surface built from those handles. Ruled out so
far, by measurement:

- *Driver support* - GLES 3.1 / GL 4.5 is present (`tools/egl_probe.py`), and the
  headless spike gets a real hardware adapter and renders correctly.
- *Missing Wayland support in wgpu* - `cargo tree` shows wgpu-hal's `gles`
  feature pulling `wayland-sys` with `client,dlopen,egl`.
- *EGL unable to initialise on the Wayland platform* - it does, once the platform
  is named: `EGL_PLATFORM=wayland` → `eglInitialize OK` (without it, Mesa reports
  `0x3001`, which is what misled an earlier probe).

Next diagnostics, in order: (1) check whether wgpu wants
`InstanceDescriptor::display` set to the same `RawDisplayHandle` it is given at
surface creation; (2) instrument wgpu-hal's gles surface path to see whether
`wl_egl_window_create` or the EGL config match is what fails; (3) try the same
surface creation from the spike, so defconmon is out of the picture.

**Cost note:** the wgpu-linked build costs ~33 MB RSS (11.8 → 44.5 MB) *even when
it falls back to the CPU path*, because the graphics stack is compiled in and EGL
is loaded during the attempt. On a 512 MB container that is a poor trade for a
path that does not activate yet, so the deployed binary is currently the lean
CPU-only build; the GPU build stays in the tree behind the `live` + wgpu deps.

### Still open on the GPU path

- **Presentation.** The spike renders to a texture and reads it back on purpose,
  so the answer did not depend on surface integration. Real use needs a wgpu
  surface on the Wayland surface (`libwayland-egl.so.1` is present).
- **The drawing itself**, which is the larger half: wgpu has no path renderer, so
  strokes and glyph outlines need tessellation (or SDF text). The CPU rasteriser
  remains the right tool for thin antialiased strokes until that is measured.
- Grain in the spike's shader came out empty — the hash is precision-degenerate
  on this GPU. Fix the hash, or generate grain on the CPU.

## Screens

Every screen is one row in `screens::SCREENS` — name, title, blurb, the settings
it exposes, and its render function. The CLI, the rotation and the config UI all
walk that table, so nothing outside a screen's own module names it.

| # | name | what it is | settings it owns |
|---|------|-----------|------------------|
| 1 | `wopr` | NORAD/WOPR console: typewriter `GREETINGS PROFESSOR FALKEN.`, the game menu, real DEFCON badge, scrolling system log. | `wopr.log_rows` |
| 2 | `defcon` | Threat board driven by **live infra state**: WAN boundary with traffic pulses, gateway diamond, guest constellation, WAN throughput instruments + sparkline. | `defcon.pulse_rate` |
| 3 | `radar` | ATC approach control: PPI scope, range rings, rotating sweep, contacts = **PVE guests** (blips, trails, data blocks, contact strips). | `radar.sweep_rpm` |
| 4 | `telemetry` | Oscilloscope + spectrum analyser + readouts driven by the real WAN rate and CPU. | `telemetry.full_scale` |
| 5 | `vectrex` | **Metric solids**: two wireframe icosahedra, one vertex per channel. FIG1 = state (radius = magnitude, colour = severity); FIG2 = volatility (radius = 15 s σ, colour = 90 s σ), with a channel legend. | `vectrex.spin` |

All five carry the **status plate** (`hud.rs`): DEFCON, WAN, PING and DNS, colour
coded green/amber/red. It is at a *different position on every screen* so no
pixels are lit twice in a row (panel burn-in), with the DEFCON cell as the anchor
that makes it recognisable wherever it lands.

Screens read the real collector feed (`/run/dashboard/dashboard.json`) and fall
back to a synthetic homelab that mirrors it, so previews render off-box. The
status plate is honest about which: `FEED LIVE` vs `FEED SIM`.

## Config server

```sh
defconmon --serve 8080          # built with --features web
```

A **separate process** behind the `web` feature, so the display binary carries
none of it and disabling it is a build switch (or just `systemctl stop
defconmon-config`). Design decisions worth keeping:

- **The config file is the only channel between the two.** The server writes
  `config.json`; the display polls its mtime once a second and reloads. No IPC,
  no sockets between them, so the UI can be restarted, rebuilt or absent without
  the display noticing — and a crashed web server cannot take the TV down.
- **The UI never names a screen or a metric.** It walks the schema (global
  settings + whatever the registry contributes) and renders a widget per entry.
  Adding a setting is one line in the screen that consumes it
  (`Param::i("radar.sweep_rpm", …)`), and adding a *screen* needs no UI change at
  all — which is the groundwork for making screens plugins later.
- **Validation is atomic.** Coercion and range checks happen before anything is
  written; a bad submission is rejected with per-field reasons and nothing is
  saved. Unchecked checkboxes simply arrive absent, which is how a `false`
  boolean comes through.
- **Same machine, same theme.** The palette is the display's exactly
  (`#33ff66` phosphor, `#38e8ff` values, `#1d4a2c` hairlines) and the fonts are
  served out of the binary, so the tool reads as part of the same equipment.
  Controls get IBM 3270 rather than the segmented face: a field you type into
  wants legibility, and segments are a readout idiom.

Plain HTTP on the LAN, no auth — deliberate, per the brief.

## Cost

Measured on the CT with the live process stopped (`--bench 40 --scale 0.5`, which
matches the live path). The CRT finish used to be the single most expensive thing
in the program:

| screen | before | after | with CRT off |
|---|---|---|---|
| `wopr` | 15.9 ms | 7.9 | 7.8 |
| `telemetry` | 21.5 ms | 10.9 | 10.1 |
| `radar` | 17.2 ms | 6.2 | 4.9 |
| `defcon` | 14.9 ms | 6.8 | 4.8 |
| `vectrex` | 12.9 ms | 4.7 | 4.0 |
| **average** | **16.4 ms** | **7.3 ms** | **6.3 ms** |

Live CPU went from **~27% to ~10% of the container's single core** (measured over
a 40 s window from `/proc/<pid>/stat`, 11.8 MB RSS, 1 thread). `display.fps` is a
config knob and the cost scales with it, so there is headroom to trade back.

### How the CRT finish got 56% cheaper

It was ~360 one-pixel vector fills (scanlines), a per-pixel radial-gradient
shader (vignette), a full-canvas fill (flicker) and 320 tiny rects (grain) - per
frame, producing the identical result every time. Scanlines, vignette and flicker
are all just "darken this pixel", so they are now **baked into one byte-per-pixel
mask at startup** and applied in a single linear pass with an exact 8-bit
multiply table; the grain is written straight into the buffer. Same look -
verified by pixel diff, not by eye: **mean |delta| 0.08-0.14 of 255**, with only
0.08-0.64% of pixels differing by more than 3.

### Measuring it

```sh
defconmon --bench 40 --screen radar --scale 0.5    # ms/frame for one screen
DEFCONMON_NO_CRT=1 defconmon --bench 40 --screen radar    # with the tube effects off
DEFCONMON_NO_DARKEN=1 ...  DEFCONMON_NO_GRAIN=1 ...              # one effect at a time
```

Stop the live service first: this is a single-core container and a competing
process makes the numbers meaningless.

## CLI

```sh
cargo build --release --target x86_64-unknown-linux-musl --features live   # display
cargo build --release --target x86_64-unknown-linux-musl --features web    # config server

./defconmon --list                    # the registry: index, name, title
./defconmon --params                  # the full schema (needs the web build)
./defconmon --screen radar --t 7.3 --out radar.png    # headless frame
./defconmon --screen wopr --bench 40                  # cost of one screen
```

`--t SECONDS` is the scene clock, so any frame of the animation can be inspected.

**Build note:** both features share one output path, so build, copy, then build
the other — otherwise the second build silently replaces the first.

`~/.cargo/config.toml` on this host pins `linker = /usr/bin/clang` +
`--ld-path=/usr/bin/mold`, neither of which exists here. Side-step it by building
for the **musl** target (the global config only pins the gnu triple), which also
yields the portable static binary.

## Deployed

Live on CT 107's HDMI, cycling all five screens (20 s each, 15 fps) under the
existing `dashboard.service`, with the config UI on port 8080.

| unit | what it runs |
|---|---|
| `dashboard.service` (stock) + drop-in `.d/defconmon.conf` | `cage -- /usr/local/bin/defconmon --config /etc/defconmon/config.json` |
| `defconmon-config.service` | `/usr/local/bin/defconmon-config --config /etc/defconmon/config.json --serve 8080` |

Pacing and the rotation live in `/etc/defconmon/config.json`, not in the unit —
tuning them needs no restart. The drop-in only says where the file is, and sets
`TZ` (see below).

- **Display**: ~10% of the container's single core, 12 MB RSS, 1 thread. (It was
  78% before half-res rendering + a word-a-time blit, 27% before the CRT pass was
  rewritten.)
- **Config server**: ~0% CPU, ~800 KB RSS, idle until a request arrives.

### Reverting to conky

```sh
rm /etc/systemd/system/dashboard.service.d/defconmon.conf
systemctl daemon-reload && systemctl restart dashboard.service
```

Or copy the preserved config back over `/home/dashboard/.config/conky/conky.conf`.

## Gotchas worth remembering

- **Decorative is fine; contradictory is not.** The rule this dashboard is held
  to: a screen may be pure fiction (the WOPR log is NORAD chatter), but no screen
  may display a value that *contradicts* measurable state. Real defects came from
  violating it — a hardcoded `DEFCON 3` while the threat board computed the real
  level, two screens disagreeing about the time, and a footer advertising a 6.0 s
  sweep while the animation ran at 9.0 s. They are the same bug as a false alarm:
  they teach you to distrust the display.
- **Condition levels must be computed from failures, not from normal state.**
  `defcon()` derives from the five *service* flags only. Counting powered-off
  guests pinned the house at DEFCON 3 permanently (three dev VMs are stopped by
  design), which is noise. Guest state is shown in its own row instead.
- **A volatility metric needs a small denominator, not the full scale.**
  Volatility is a coefficient of variation. Dividing by full scale means a
  low-rate channel's floor swamps its mean, so a link swinging 0.1→1.1 Mb/s
  scored "perfectly steady" on the live screen. The floor only exists to stop an
  idle channel dividing by ~0 and reading as permanently thrashing.
- **Feed booleans are integers.** `gateway`/`internet`/`dns` come out of the
  collector as `1`/`0`, not `true`/`false` — `as_bool()` silently yields `None`,
  so the board claimed GATEWAY/INTERNET/DNS FAIL while the network was healthy.
  Accept bool | number | string.
- **CPU values are fractions.** `pve.cpu` and per-guest `cpu` are 0.086 = 8.6%,
  so they need ×100 or they render as 0%.
- **Clock/timezone.** The container's system clock is UTC; a household wall
  display wants the house timezone. `clock.rs` uses `localtime_r` (so it honours
  `TZ`/DST) and the service drop-in sets `Environment=TZ=America/New_York` — no
  system-wide timezone change needed.
- **Float defaults widen.** `serde_json::Value::from(0.9f32)` becomes
  `0.8999999761581421`, which reaches a form field verbatim and lands in the
  config file. Round on the way in and format on the way out.
- **`~/.cargo/config.toml` pins a linker that isn't installed** (`/usr/bin/clang`
  + mold), and a project-local override does *not* win — cargo concatenates the
  rustflags. Build for the musl target: the pin is scoped to the gnu triple, and
  you get a portable static binary for free.
- **Screenshot review can lie at small sizes.** Downscaled captures produced
  confident reports of a clipped clock, a missing decimal point and a wrong date,
  all of which were fine at full resolution; a crop at 1:1 is the only way to
  settle a small-text question.

- **A darkening effect is invisible over black.** The scanlines are a 16%
  darkening, which measures cleanly across content (15-24% modulation at the
  3-px pitch) but does nothing at all on the black background - so "I can't see
  scanlines" is not evidence they are broken, and a downscaled screenshot cannot
  show them either. Measure the modulation instead.
- **Measure CPU from `/proc/<pid>/stat`, not `ps %CPU`.** `ps` reports a lifetime
  average, so a freshly restarted process still carries its startup cost and
  reads ~3x high. Sample the tick counters over a window.

## Fallback

The previous conky dashboard is preserved on the CT at
`/usr/local/etc/dashboard/conky.classic.conf` (plus a timestamped copy). Revert
by copying it back over `/home/dashboard/.config/conky/conky.conf` and
restarting `dashboard.service`.
