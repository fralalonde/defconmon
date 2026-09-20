#!/usr/bin/env bash
# Play the pre-rendered defconmon screens as ANSI art, bounded by `timeout`
# so vhs can end the recording. Kept tiny so the vhs preamble is short.
cd "$(dirname "$0")/../.."          # repo root
exec timeout 16 python3 docs/tapes/defconmon-render.py .defconmon-gif/raw 2600