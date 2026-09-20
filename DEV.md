# defconmon — developer notes

Build/verify notes and the hardware background behind the GPU decision. This is
for the person building the binary, not the person wall-mounting the screen.

## Hardware / GPU

Honest numbers, measured on this box's integrated Radeon HD 7450 / Caicos,
TeraScale 2, Mesa r600 GL backend:

- **GPU present path: 14.5% of one core** vs **~10% for the tuned CPU pass**.
  The per-frame scene (texture) upload plus GL submission costs more than the
  CPU mask pass.
- Hence `gpu.mode` defaults to `auto`: the GPU is used only where it is likely
  to win — a non-GL backend (Vulkan/Metal/DX12) or a discrete GL card.
  Integrated + GL falls back to the CPU.
- Build sizes / RSS: CPU-only display ~2 MB bin (~12 MB RSS); GPU display
  ~5.4 MB bin (~53 MB RSS).

## GPU present path

- wgpu (GL/EGL backend) reaches EGL/Wayland via **dlopen at runtime** — the GPU
  binary carries no hard link-time dependency on them.
- The CPU path is a **runtime fallback in the same binary**: EGL absent, it just
  draws. The look never depends on the GPU — tiny-skia rasterises everything on
  the CPU; the GPU only upscales and adds the CRT finish.

## Portability rule

Assume a **cheap old GPU — possibly ARM/Mali; never assume a vendor, a
desktop-class part, or anything newer than ~2010**. The CPU path is pure Rust,
no `cfg(target_arch)`, no SIMD intrinsics — it runs anywhere. Nothing
user-visible may depend on the GPU.

## Design rule

**Decorative is fine; contradictory is not.** A screen may be pure fiction (the
WOPR log is NORAD chatter), but no screen may display a value that contradicts
measurable state. Real on-box signal loss came from violating this: `DEFCON 3`
hardcoded while the board computed the real level, and two screens disagreeing
on the time.

## Container build

Plain host gnu builds fail on the box (no clang/mold; the global
`~/.cargo/config.toml` pins the linker). Build in a Debian container instead,
with `CARGO_TARGET_DIR` a scratch dir (not `target-debian`):

```sh
cd defconmon
podman run --rm -v "$PWD":/src:Z -v rd-cargo:/cargo -w /src \
  -e CARGO_HOME=/cargo -e RUSTUP_HOME=/cargo/rustup \
  -e CARGO_TARGET_DIR=/src/target-b -e DEBIAN_FRONTEND=noninteractive \
  debian:12 bash -c '
    apt-get update -qq >/dev/null 2>&1;
    apt-get install -y -qq gcc libc6-dev pkg-config libwayland-dev libegl-dev >/dev/null 2>&1;
    /cargo/bin/cargo build --release --no-default-features &&
    /cargo/bin/cargo build --release &&
    ./target-b/release/defconmon --version'
```

The GPU build needs `libwayland-dev` and `libegl-dev` at build time (headers
only; EGL resolves via dlopen at runtime). `rd-cargo` is the shared cargo/rustup
volume carrying the toolchain.

## Measuring

```sh
./defconmon --bench 40 --screen radar --scale 0.5   # ms/frame for one screen
./defconmon --list                                 # screen registry
./defconmon --params                               # full schema
./defconmon --screen wopr --t 7.3 --out wopr.png   # headless frame
./defconmon --feed examples/sample-feed.json --screen defcon --out defcon.png
```

`--t SECONDS` is the scene clock, so any frame of the animation can be
inspected. `--feed PATH` renders from a feed JSON instead of the live one; pass
`--feed examples/sample-feed.json` to draw previews from the bundled sample
data (works off-box, no live feed required). Without `--feed`, the live feed is
read from `/run/dashboard/dashboard.json` (falling back to `./dashboard.json`).
Stop the live service before benchmarking — single-core container.