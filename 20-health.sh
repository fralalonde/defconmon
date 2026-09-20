#!/bin/bash
# LAN internet-access health -> GREEN / YELLOW / RED for PING and DNS.
# Emits { gateway, internet, dns } booleans, the raw timings behind them
# (ping_ms / ping_loss / dns_ms) and sets traffic-light markers
# /run/dashboard/tl-{ping,dns}-{green,yellow,red}.
#
# The dashboard shows the millisecond figures, so they must be exported, not
# just the derived colour.
set -eu
D="/run/dashboard"

mark() { # mark <base> <level>
    local c
    for c in green yellow red; do rm -f "$D/tl-$1-$c"; done
    touch "$D/tl-$1-$2"
}

# ---- PING: 3 probes to a public anycast target ------------------------------
gw=0; inet=0; dns=0; ms=0; prtt=0; ploss=100
ping -c1 -W2 192.168.1.1 >/dev/null 2>&1 && gw=1

pout="$(ping -c3 -W2 1.1.1.1 2>/dev/null)" || true
ploss="$(printf '%s\n' "$pout" | grep -o '[0-9]*% packet loss' | grep -o '^[0-9]*')"
prtt="$(printf '%s\n' "$pout" | grep -o '[0-9.]*/[0-9.]*/[0-9.]*/[0-9.]*' | cut -d/ -f2)"
[[ -z "${ploss:-}" ]] && ploss=100
[[ -z "${prtt:-}" ]] && prtt=0

if   [[ "$ploss" -ge 100 ]]; then plev=red;  inet=0
elif [[ "$ploss" -gt 0 ]] || awk -v r="${prtt:-0}" 'BEGIN{exit !(r+0 > 150)}'; then plev=yellow; inet=1
else plev=green; inet=1
fi
mark ping "$plev"

# ---- DNS: resolve timing ---------------------------------------------------
dnslev=red
t0="$(date +%s%N)"
if getent ahostsv4 one.one.one.one >/dev/null 2>&1; then
    dns=1
    ms=$(( ($(date +%s%N) - t0) / 1000000 ))
    if (( ms > 500 )); then dnslev=yellow; else dnslev=green; fi
fi
mark dns "$dnslev"

# ---- convenience on/off markers (kept for other panels) --------------------
[[ "$gw"   -eq 1 ]] && touch "$D/ok.gateway"  || rm -f "$D/ok.gateway"
[[ "$inet" -eq 1 ]] && touch "$D/ok.internet" || rm -f "$D/ok.internet"
[[ "$dns"  -eq 1 ]] && touch "$D/ok.dns"      || rm -f "$D/ok.dns"

jq -n --argjson gateway "$gw" --argjson internet "$inet" --argjson dns "$dns" \
      --arg ping "$plev" --arg dns_lvl "$dnslev" \
      --argjson ping_ms "${prtt:-0}" --argjson ping_loss "$ploss" --argjson dns_ms "$ms" \
  '{gateway:$gateway, internet:$internet, dns:$dns,
    ping_level:$ping, dns_level:$dns_lvl,
    ping_ms:$ping_ms, ping_loss:$ping_loss, dns_ms:$dns_ms}'
