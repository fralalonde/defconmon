# Releasing defconmon

Target platform is **x86_64 Linux only** — the display box for this homelab.
There is deliberately no macOS / Windows / arm64 / musl build target anywhere
in the toolchain.

## Versioning policy

The **git tag is the single source of truth** for the version. It is injected
at **compile time** by `build.rs` into `DEFCONMON_VERSION`, which
`main.rs` reads via `env!("DEFCONMON_VERSION")`. The version is **never
hardcoded in a source file**.

Versions only need to **increase monotonically**; aesthetics do not matter.
The wiring:

| Git state                              | Injected value          |
|----------------------------------------|-------------------------|
| Tagged `v0.5.5`                        | `0.5.5`                 |
| Dirty on tag                           | `0.5.5+dirty`           |
| Between tags (3 commits ahead of tag)  | `0.5.5-dev.3+gabcdef`   |
| No tags reachable / backend lacks git  | `CARGO_PKG_VERSION` (Cargo.toml fallback) |

Because of the fallback, a plain `cargo build` on an untagged checkout still
works — it just reports the crate version from `Cargo.toml` instead of a tag.

> **Changing the env var name** must be coordinated in *both* places:
> `build.rs` emits `cargo:rustc-env=DEFCONMON_VERSION=…` and
> `src/bin/defconmon.rs` reads `env!("DEFCONMON_VERSION")`.

## Build variants

Three things are built and released, all x86_64 Linux:

| Variant | Command                                        | Output binary        | Asset name                                    |
|---------|------------------------------------------------|----------------------|-----------------------------------------------|
| CPU     | `cargo build --release --bin defconmon --features live`    | `defconmon` | `defconmon-<ver>-x86_64-linux-cpu`  |
| GPU     | `cargo build --release --bin defconmon --features live,gpu`| `defconmon` | `defconmon-<ver>-x86_64-linux-gpu`  |
| Config  | `cargo build --release --bin defconmon-config --features web` | `defconmon-config` | `defconmon-config-<ver>-x86_64-linux` |

- **CPU** build is lean: pure-Rust Wayland + tiny-skia rasterization.
- **GPU** build links against **EGL/Wayland at runtime via `dlopen`** (wgpu),
  so it is a lightweight dynamic-glibc binary with no hard link-time
  dependency — EGL may even be absent and the CPU path is the fallback. Both
  display builds need `libwayland-dev` and the GPU build additionally needs
  `libegl-dev` **at build time** (headers only).
- **Config** server is std-only; it needs no system libraries.

## The release flow (order)

1. **Bump + tag** (run by the repo owner, on the branch to release):
   ```bash
   ./release.sh <major|minor|patch> [--push]
   ```
   The script runs `cargo check`, derives the next `vX.Y.Z` from the last
   `v[0-9]*.[0-9]*.[0-9]*` tag (default `v0.0.0`), syncs the version in
   `Cargo.toml` / `Cargo.lock` (git remains the source of
   truth; the crate files just carry it for the toolchain), commits, and
   creates an annotated tag. Refuses on a detached HEAD; on a dirty tree it
   offers to commit first. Without `--push` it prompts (default no).
   It does **not** write any release binaries.

2. **Push to trigger the pipeline** (or up-front with `--push`):
   ```bash
   git push origin <branch>
   git push origin <tag>        # e.g. v0.1.0
   ```

3. **GitHub Actions** (`.github/workflows/release.yml`) reacts to a `v*` tag
   push. The `build` job runs the three variant commands from the table above
   in parallel on `ubuntu-22.04`, sanity-checks each `--version`, packages
   each into a `.tar.gz`, and the `release` job attaches all three plus
   `scripts/install.sh` and a `checksums.txt` to a **GitHub Release**.

4. **Install** anywhere (x86_64 Linux):
   ```bash
   curl -fsSL https://github.com/fralalonde/defconmon/releases/latest/download/install.sh | sh
   # or pin cpu / gpu explicitly:
   curl -fsSL .../install.sh | sh -s -- --variant cpu
   ```
   `scripts/install.sh` fetches the chosen display variant plus the config
   server and installs both to `PREFIX/bin` (default `/usr/local/bin`, where
   `defconmon.service` expects them).

## Verifying a build here

Plain host gnu builds fail on this machine (no clang/mold, and the global
`~/.cargo/config.toml` pins the linker). Build in a Debian container instead
(`CARGO_TARGET_DIR` a scratch dir, not `target-debian`):

```bash
cd defconmon
podman run --rm -v "$PWD":/src:Z -v rd-cargo:/cargo -w /src \
  -e CARGO_HOME=/cargo -e RUSTUP_HOME=/cargo/rustup \
  -e CARGO_TARGET_DIR=/src/target-b -e DEBIAN_FRONTEND=noninteractive \
  debian:12 bash -c '
    apt-get update -qq >/dev/null 2>&1;
    apt-get install -y -qq gcc libc6-dev pkg-config libwayland-dev libegl-dev >/dev/null 2>&1;
    /cargo/bin/cargo build --release --bin defconmon --features live &&
    ./target-b/release/defconmon --version'
```

## Who releases

The repo owner reserves the actual release (running `release.sh`, tagging, and
pushing) to themselves. Contributors and automation are expected to **author
the machinery**, not invoke it. `release.sh` idempotently guards everything:
it never pushes without an explicit prompt/`--push`, so a release cannot be
triggered by accident.