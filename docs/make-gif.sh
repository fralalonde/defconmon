#!/usr/bin/env bash
# Regenerate docs/defconmon.gif.
#
# WHY: the hero GIF must show the dashboard's OWN headless renders at full
# resolution. No terminal, no vhs, no ANSI half-block art, no typed command,
# no window chrome - just the five screens cycling as the app really draws
# them. The loop is ~15 s (each of the five screens dwells ~3 s), and each
# screen carries real motion (CRT sweep/flicker, radar rotation, log churn,
# wireframe spin) by sweeping its `--t` animation-time across the dwell.
#
# WHY --feed: renders must use the committed GENERIC sample data
# (examples/sample-feed.json), never live homelab state.
#
# SIZE/LEGIBILITY TRADE: frames are rendered at the app's full 1920x1080 (so
# every glyph is drawn at its intended 1:1 resolution) then downscaled to
# 1440x810 for delivery - a 0.75x reduction that keeps on-screen text legible
# while more than halving the GIF's byte weight. A shared, small global palette
# (64 colours - plenty for phosphor greens/cyans plus the amber/red status
# ramp) with no dithering is what brings a 15 s loop in under ~4 MB; these are
# flat-colour CRT frames, so posterised banding is faithful, not a defect.
#
# PREREQUISITE: `target-gif/release/defconmon` must already be built. Host
# builds fail (the global ~/.cargo/config.toml pins a nonexistent /usr/bin/
# clang), so it is built in a Debian-12 podman container:
#   podman run --rm -v "$PWD":/src:Z -v rd-cargo:/cargo -w /src \
#     -e CARGO_HOME=/cargo -e RUSTUP_HOME=/cargo/rustup \
#     -e CARGO_TARGET_DIR=/src/target-gif -e DEBIAN_FRONTEND=noninteractive \
#     debian:12 bash -c 'export PATH=/cargo/bin:$PATH; \
#       apt-get update -qq && apt-get install -y -qq gcc libc6-dev pkg-config \
#         libwayland-dev libegl-dev; \
#       cargo build --release --bin defconmon'
# (CARGO_TARGET_DIR=/src/target-gif keeps it out of the concurrent agents'
# target dirs; bare `cargo` is NOT on the container PATH, export it first.)

set -euo pipefail
cd "$(dirname "$0")"

BIN=../target-gif/release/defconmon
FEED=../examples/sample-feed.json
OUT=./defconmon.gif
SCRATCH=../.defconmon-gif

RW=1920                 # render resolution (the app's native)
RH=1080
DW=1440                 # delivered resolution (0.75x, still legible)
DH=810
FPS=7
DWELL=3                 # seconds each screen is on screen
N_FRAMES=$((FPS * DWELL))   # frames per screen
COLORS=64               # global GIF palette size

# Screen order + how far each screen's own animation time (`--t`) advances
# over its dwell (seconds). Radar gets near a full rotation in its 3 s (~7 s
# at 8.6 rpm), the vectrex solids turn a good while, the others get grid,
# sweep and log churn.
declare -a SCREENS=(wopr defcon radar telemetry vectrex)
declare -a TSPAN=(4.5 4.5 7.0 4.5 6.0)

rm -rf "$SCRATCH/frames"
mkdir -p "$SCRATCH/frames"

k=0
for i in "${!SCREENS[@]}"; do
    sc=${SCREENS[$i]}
    span=${TSPAN[$i]}
    for f in $(seq 0 $((N_FRAMES - 1))); do
        # t advances linearly from 0 to span across the screen's N_FRAMES.
        t=$(awk "BEGIN{printf \"%.3f\", ${f} * ${span} / ${N_FRAMES}}")
        "$BIN" preview --screen "$sc" --out "$SCRATCH/frames/$(printf '%05d' "$k").png" \
            --w "$RW" --h "$RH" --t "$t" --feed "$FEED" \
            >/dev/null
        k=$((k + 1))
    done
done

# WHY palettegen/paletteuse: a shared global palette of flat phosphor shades
# compresses the loop far better than full-colour frames. Whole-sequence
# `palettegen` after `split` so the single palette covers every frame; `-loop 0`
# makes it repeat forever. Render-exact downscale happens in the same graph.
ffmpeg -y -loglevel error -framerate "$FPS" \
    -i "$SCRATCH/frames/%05d.png" \
    -filter_complex \
    "scale=$DW:$DH:flags=area,split[s0][s1];[s0]palettegen=max_colors=$COLORS:stats_mode=diff[p];[s1][p]paletteuse=dither=none" \
    -loop 0 "$OUT"

n=$(ls "$SCRATCH/frames" | wc -l)
secs=$(awk "BEGIN{printf \"%.1f\", ${n} / ${FPS}}")
ls -l "$OUT"
echo "frames=$n fps=$FPS render=$RW"x"$RH deliver=$DW"x"$DH loop_seconds=$secs"