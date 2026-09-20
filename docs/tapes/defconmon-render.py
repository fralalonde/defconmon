#!/usr/bin/env python3
"""Show defconmon screens as ANSI 24-bit half-block art on stdout.

WHY: defconmon draws to a Wayland surface, so it can't run inside vhs's fake
terminal. But it has a headless preview (`--out png`). We render one PNG per
screen at a few t values, downscale each once with ffmpeg to the cell grid,
and store raw rgb24 in a working dir. This script replays those bytes as
upper-half-block (U+2580) art: fg = top source row, bg = bottom, so one char
cell shows two source rows.

vhs frame model (this drives the design):
    * vhs only writes a new GIF frame when the terminal *changes*, and holds
      each held/static frame for the real wall-clock it lasted. Continuously
      cycling frames makes every frame distinct, so vhs compresses them all to
      the minimum 40 ms delay and the whole reel blazes past.
    * So a screen's *dwell* must be a single static frame re-printed once and
      then left alone (vhs holds it). We give the first t of each screen its
      full `dwell`, and show the other t-steps only as a brief pre-"power-up"
      so the CRT sweep/grid motion is visible without eating the dwell.

Blueprint (tricky bits kept short):
    * Half-block + ~2:1 cell ratio maps a 16:9 frame into a terminal twice as
      tall in chars as wide (128 cols x 36 rows here).
    * Always downscale (never crop) so the whole screen fits the grid.

Usage:  render.py RAW_DIR dwell_ms
RAW_DIR holds <screen>_<i>.raw (see make-defconmon-gif.sh).
"""
import os, sys, time

WIDTH, HEIGHT = 128, 72                 # must match scale in make-defconmon-gif.sh
RAWDIR = sys.argv[1]
DWELL_MS = int(sys.argv[2]) if len(sys.argv) > 2 else 2600

def draw(buf):
    sys.stdout.write("\x1b[2J\x1b[H")            # clear + home
    for y in range(0, HEIGHT, 2):                # one cell per 2 source rows
        top = buf[y * WIDTH * 3:(y + 1) * WIDTH * 3]
        bot = buf[(y + 1) * WIDTH * 3:(y + 2) * WIDTH * 3] \
            if y + 1 < HEIGHT else top
        parts = []
        for x in range(WIDTH):
            i = x * 3
            parts.append("\x1b[38;2;%d;%d;%d;48;2;%d;%d;%dm\u2580" % (
                top[i], top[i + 1], top[i + 2],
                bot[i], bot[i + 1], bot[i + 2]))
        sys.stdout.write("".join(parts) + "\n")
    sys.stdout.flush()

def main():
    dwell = DWELL_MS / 1000.0
    raws = sorted(f for f in os.listdir(RAWDIR) if f.endswith(".raw"))
    order, seen = [], set()
    for f in raws:
        s = f.split("_")[0]
        if s not in seen:
            seen.add(s); order.append(s)
    by = {s: [open(os.path.join(RAWDIR, f), "rb").read()
              for f in raws if f.startswith(s + "_")] for s in order}
    while True:
        for s in order:
            bufs = by[s]
            # brief animation pre-roll (0.5s): CRT sweep / grid drift for a few
            # t-steps so the phosphor reads as "live", then hold a static frame
            # for the real dwell so vhs preserves the wall-clock time.
            for i in range(-5, 0):                # last 5 t-frames, quick
                draw(bufs[i])
                time.sleep(0.1)
            draw(bufs[-1])                        # dwell on the final t
            time.sleep(dwell)
        # never exits; the make script bounds this with `timeout`

main()