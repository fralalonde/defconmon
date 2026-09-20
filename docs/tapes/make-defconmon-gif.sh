#!/usr/bin/env bash
# Regenerate docs/defconmon.gif in one shot. Requires vhs, ffmpeg, and a
# defconmon binary with the headless preview mode (`--out png`).
#
# WHY this pipeline exists: defconmon draws to a Wayland surface, so it cannot
# run inside vhs's terminal. So we drive the app's headless preview instead --
# one PNG per screen at a few t values (t animates the CRT sweep + grid drift) --
# pre-downscale them to raw RGB once, and have vhs record a terminal that plays
# those frames back as ANSI half-block art. The recorded GIF is the GIF we ship.
set -euo pipefail
# repo root is two dirs up from this script (docs/tapes/).
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

# 1. Resolve the defconmon binary: prefer an existing release build, build one
#    in a Debian container as a fallback (host Rust needs clang/mold we lack).
BIN="${DEFCONMON_BIN:-}"
if [[ -z "$BIN" ]]; then
    for cand in target-debian/release/defconmon target-debian/release/defconmon target/release/defconmon; do
        if [[ -x "$ROOT/$cand" ]]; then BIN="$ROOT/$cand"; break; fi
    done
fi
if [[ -z "$BIN" || ! -x "$BIN" ]]; then
    echo "no defconmon binary; building in a Debian 12 container (needs podman)..." >&2
    ( cd "$ROOT" && podman run --rm -v "$PWD":/src:Z -v rd-cargo:/cargo -w /src \
        -e CARGO_HOME=/cargo -e RUSTUP_HOME=/cargo/rustup \
        -e CARGO_TARGET_DIR=/src/target-c -e DEBIAN_FRONTEND=noninteractive \
        debian:12 bash -c 'apt-get update -qq >/dev/null 2>&1; \
          apt-get install -y -qq gcc libc6-dev pkg-config >/dev/null 2>&1; \
          /cargo/bin/cargo build --release --bin defconmon' >/dev/null )
    BIN="$ROOT/target-debian/release/defconmon"
fi

# 2. Render headless preview frames into the (git-ignored) working dir, then
#    downscale each to the 128x72 grid the ANSI renderer reads back.
#    WHY 30 t-steps per screen: the CRT sweep is a faint alpha-26 bar that
#    vanishes at the 128x72 grid, so a few t-values look ~static and vhs would
#    dedupe the dwell away. 30 dense steps across a full sweep period (7-8s)
#    keep the frame animating, so vhs keeps every dwell second.
WORK="$ROOT/.defconmon-gif"
rm -rf "$WORK"; mkdir -p "$WORK/png" "$WORK/raw"
for s in wopr defcon radar telemetry vectrex; do
    for i in $(seq 0 29); do
        t=$(awk "BEGIN{print $i*0.25}")
        "$BIN" --screen "$s" --t "$t" --out "$WORK/png/${s}_${i}.png" --w 1920 --h 1080 >/dev/null
    done
done
for png in "$WORK/png"/*.png; do
    ffmpeg -loglevel error -i "$png" -vf scale=128:72 -f rawvideo \
        -pix_fmt rgb24 "${png%.png}.raw"
done
mv "$WORK"/png/*.raw "$WORK/raw/"

# 3. Record the terminal. Use vhs from the repo root so the tape's relative
#    paths and `Output docs/defconmon.gif` match. Keep the Chromium cache on a
#    stable path so repeated runs don't re-download it (see vhs docs).
export XDG_CACHE_HOME="${XDG_CACHE_HOME:-$HOME/.cache}"
vhs docs/tapes/defconmon.tape

# 4. The GIF is done; report size for sanity. (vhs writes docs/defconmon.gif.)
ls -l docs/defconmon.gif