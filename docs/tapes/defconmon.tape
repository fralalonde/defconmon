Output "docs/defconmon.gif"
Set Shell "bash"
Set FontFamily "Noto Sans Mono"
Set FontSize 16
Set Width 1360
Set Height 840
Set Padding 12
Set PlaybackSpeed 1

# defconmon renders to Wayland, so it can't show inside vhs's pty. The make
# script runs its headless preview (--out png) at a few t values, pre-writes
# scaled raw frames, and play-defconmon.sh plays them as ANSI half-block art.
# The wrapper's fixed `timeout 15` bounds the renderer so vhs moves on to Exit.
# (No Hide: vhs 0.11 records no frames while the cursor is hidden.)
Type "docs/tapes/play-defconmon.sh"
Enter
Sleep 16s