//! Live data feed + rolling history.
//!
//! Reads the collector's JSON (drop-in fragments merged by
//! /usr/local/bin/update-dashboard) and falls back to a synthetic homelab that
//! mirrors the real one, so previews render off-box.
//!
//! TYPE NOTES: the collector emits the health flags as *integers* (`1`/`0`),
//! not JSON booleans, and reports CPU as a *fraction* (0.086 = 8.6%). Accept
//! both spellings and normalise the maths here so screens never have to care.
use std::fs;
use std::ops::Deref;

pub struct Guest {
    pub name: String,
    pub up: bool,
    pub cpu: f32,
}

pub struct Feed {
    pub gateway: bool,
    pub internet: bool,
    pub dns: bool,
    pub opn_reach: bool,
    pub wan_online: bool,
    pub wan_rx: f32,
    pub wan_tx: f32,
    pub line_rate: String,
    /// current WAN round-trip time, milliseconds
    pub ping_ms: f32,
    /// percent packet loss on that probe
    pub ping_loss: f32,
    /// name resolution time, milliseconds
    pub dns_ms: f32,
    pub pve_online: bool,
    pub pve_node: String,
    pub pve_cpu: f32,
    pub pve_mem: f32,
    pub guests_running: u32,
    pub guests_total: u32,
    pub guests: Vec<Guest>,
    /// true when the numbers came from the real feed
    pub live: bool,
}

fn s(v: &serde_json::Value, k: &str) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
}

/// Truthiness that tolerates `true`, `1`, `"1"` and `"true"`.
fn truthy(v: &serde_json::Value, k: &str) -> bool {
    match v.get(k) {
        Some(serde_json::Value::Bool(b)) => *b,
        Some(serde_json::Value::Number(n)) => n.as_f64().unwrap_or(0.0) != 0.0,
        Some(serde_json::Value::String(t)) => t == "1" || t.eq_ignore_ascii_case("true"),
        _ => false,
    }
}

fn f(v: &serde_json::Value, k: &str) -> f32 {
    v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32
}

impl Feed {
    fn from_json(v: &serde_json::Value) -> Feed {
        let wan = v.get("wan").cloned().unwrap_or(serde_json::Value::Null);
        let pve = v.get("pve").cloned().unwrap_or(serde_json::Value::Null);
        let mut guests = Vec::new();
        if let Some(arr) = pve.get("guests").and_then(|x| x.as_array()) {
            for gv in arr {
                let st = s(gv, "status");
                guests.push(Guest {
                    name: s(gv, "name"),
                    up: st.eq_ignore_ascii_case("running") || st.eq_ignore_ascii_case("up"),
                    cpu: f(gv, "cpu") * 100.0, // fraction -> percent
                });
            }
        }
        Feed {
            gateway: truthy(v, "gateway"),
            internet: truthy(v, "internet"),
            dns: truthy(v, "dns"),
            opn_reach: truthy(v, "opnsense_reachable"),
            wan_online: truthy(&wan, "online"),
            wan_rx: f(&wan, "rx_mbps"),
            wan_tx: f(&wan, "tx_mbps"),
            line_rate: s(&wan, "line_rate"),
            ping_ms: f(v, "ping_ms"),
            ping_loss: f(v, "ping_loss"),
            dns_ms: f(v, "dns_ms"),
            pve_online: truthy(&pve, "online"),
            pve_node: s(&pve, "node"),
            pve_cpu: f(&pve, "cpu") * 100.0,
            pve_mem: f(&pve, "mempct"),
            guests_running: f(&pve, "guests_running") as u32,
            guests_total: f(&pve, "guests_total") as u32,
            guests,
            live: true,
        }
    }

    /// Mirror of the real homelab, for off-box previews.
    pub fn synthetic() -> Feed {
        let names = [
            ("opnsense", true, 4.0),
            ("adguard", true, 0.1),
            ("immich", true, 0.9),
            ("torrent", true, 0.5),
            ("music", true, 0.4),
            ("monitor", true, 20.1),
            ("debian-cloudinit", false, 0.0),
            ("buro", false, 0.0),
            ("agent", false, 0.0),
        ];
        Feed {
            gateway: true,
            internet: true,
            dns: true,
            opn_reach: true,
            wan_online: true,
            wan_rx: 12.4,
            wan_tx: 3.1,
            line_rate: "1000000000 bit/s".into(),
            ping_ms: 11.3,
            ping_loss: 0.0,
            dns_ms: 15.0,
            pve_online: true,
            pve_node: "streaker".into(),
            pve_cpu: 11.0,
            pve_mem: 61.0,
            guests_running: 6,
            guests_total: 9,
            guests: names
                .iter()
                .map(|(n, u, c)| Guest { name: n.to_string(), up: *u, cpu: *c })
                .collect(),
            live: false,
        }
    }

    /// Number of failed *services* -> drives the DEFCON badge.
    ///
    /// Deliberately excludes guests that are simply powered off: that is normal
    /// operation, and counting it would pin the house at DEFCON 3 forever and
    /// turn the badge into noise. Guest state is displayed separately.
    pub fn failures(&self) -> u32 {
        [!self.gateway, !self.internet, !self.dns, !self.wan_online, !self.pve_online]
            .iter()
            .filter(|x| **x)
            .count() as u32
    }

    pub fn defcon(&self) -> u32 {
        (5u32).saturating_sub(self.failures()).max(1)
    }
}

// ---------------------------------------------------------------------------
// Channels: the 12 real things this dashboard measures. One vertex per channel
// on the Vectrex solid, and the source for the persistent status plate.
// ---------------------------------------------------------------------------

pub const NCH: usize = 12;
/// short window (seconds) and medium window for volatility
pub const SHORT_W: usize = 15;
pub const MED_W: usize = 90;

const NAMES: [&str; NCH] = [
    "WAN RX", "WAN TX", "PING", "DNS", "PVE CPU", "PVE MEM", "GUESTS", "WAN LINK", "GATEWAY",
    "INTERNET", "PVE NODE", "OPNSENSE",
];
const UNITS: [&str; NCH] = [
    "Mb/s", "Mb/s", "ms", "ms", "%", "%", "up", "", "", "", "", "",
];

pub fn ch_name(i: usize) -> &'static str {
    NAMES.get(i).copied().unwrap_or("?")
}
pub fn ch_unit(i: usize) -> &'static str {
    UNITS.get(i).copied().unwrap_or("")
}

/// Severity colour: 0 nominal (green), 1 warning (amber), 2 critical (red).
pub fn level_rgb(l: u8) -> u32 {
    match l {
        0 => 0x33ff66,
        1 => 0xffb000,
        _ => 0xff3030,
    }
}

/// Full-scale for a channel, used both for radius normalisation and to give the
/// coefficient of variation a sane denominator.
fn ch_scale(i: usize) -> f32 {
    match i {
        0 | 1 => 100.0,
        2 => 150.0,
        3 => 200.0,
        4 | 5 => 100.0,
        _ => 1.0,
    }
}

/// One 1 Hz trend sample.
#[derive(Clone, Copy, Default)]
pub struct Sample {
    pub wan_rx: f32,
    pub wan_tx: f32,
    pub ping_ms: f32,
    pub dns_ms: f32,
    pub pve_cpu: f32,
    pub pve_mem: f32,
    pub guests: f32,
    pub wan_link: f32,
    pub gateway: f32,
    pub internet: f32,
    pub pve_node: f32,
    pub opnsense: f32,
}

impl Sample {
    pub fn ch(&self, i: usize) -> f32 {
        match i {
            0 => self.wan_rx,
            1 => self.wan_tx,
            2 => self.ping_ms,
            3 => self.dns_ms,
            4 => self.pve_cpu,
            5 => self.pve_mem,
            6 => self.guests,
            7 => self.wan_link,
            8 => self.gateway,
            9 => self.internet,
            10 => self.pve_node,
            11 => self.opnsense,
            _ => 0.0,
        }
    }
}

/// The feed plus its rolling history. Derefs to `Feed`, so screens keep reading
/// `feed.wan_rx` / `feed.defcon()` unchanged while trend data is reachable as
/// `feed.ch_now(i)` / `feed.ch_level(i)` / `feed.ch_vol(i, SHORT_W)`.
pub struct Snap {
    pub feed: Feed,
    /// 1 Hz samples, oldest first, newest last
    pub hist: Vec<Sample>,
}

impl Deref for Snap {
    type Target = Feed;
    fn deref(&self) -> &Feed {
        &self.feed
    }
}

impl Snap {
    pub fn load() -> Snap {
        let txt = fs::read_to_string("/run/dashboard/dashboard.json")
            .or_else(|_| fs::read_to_string("dashboard.json"))
            .ok();
        match txt.and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) {
            Some(v) => Snap { feed: Feed::from_json(&v), hist: Vec::new() },
            None => Snap::synthetic(),
        }
    }

    /// Off-box preview: the synthetic feed plus a plausible trend.
    pub fn synthetic() -> Snap {
        let mut s = Snap { feed: Feed::synthetic(), hist: Vec::new() };
        s.seed_history();
        s
    }

    /// Fill the trend buffer from the current values. Only for off-box previews,
    /// which have no history of their own - the live loop pushes real samples
    /// once a second, and a real history must never be fabricated.
    pub fn seed_history(&mut self) {
        let now = self.sample();
        let mut hist = Vec::with_capacity(MED_W);
        for k in 0..MED_W {
            let phase = k as f32 * 0.14;
            let mut s = now;
            s.wan_rx = (now.wan_rx * (1.0 + 0.45 * phase.sin()) + noise(3, k) * now.wan_rx * 0.8)
                .max(0.0);
            s.wan_tx = (now.wan_tx + noise(5, k) * now.wan_tx * 0.5).max(0.0);
            s.ping_ms = (now.ping_ms + noise(7, k) * (now.ping_ms * 0.35 + 1.0)).max(0.0);
            s.dns_ms = (now.dns_ms + noise(11, k) * (now.dns_ms * 0.3 + 1.0)).max(0.0);
            s.pve_cpu = (now.pve_cpu + noise(13, k) * 6.0).clamp(0.0, 100.0);
            s.pve_mem = (now.pve_mem + noise(17, k) * 0.8).clamp(0.0, 100.0);
            hist.push(s);
        }
        self.hist = hist;
    }

    pub fn push(&mut self, s: Sample) {
        self.hist.push(s);
        if self.hist.len() > MED_W {
            let drop = self.hist.len() - MED_W;
            self.hist.drain(0..drop);
        }
    }

    /// Sample the current state of the feed for the trend.
    pub fn sample(&self) -> Sample {
        let f = &self.feed;
        Sample {
            wan_rx: f.wan_rx,
            wan_tx: f.wan_tx,
            ping_ms: f.ping_ms,
            dns_ms: f.dns_ms,
            pve_cpu: f.pve_cpu,
            pve_mem: f.pve_mem,
            guests: if f.guests_total > 0 {
                f.guests_running as f32 / f.guests_total as f32
            } else {
                0.0
            },
            wan_link: f.wan_online as u8 as f32,
            gateway: f.gateway as u8 as f32,
            internet: f.internet as u8 as f32,
            pve_node: f.pve_online as u8 as f32,
            opnsense: f.opn_reach as u8 as f32,
        }
    }

    /// Severity of a channel right now, from the real feed.
    pub fn ch_level(&self, i: usize) -> u8 {
        let f = &self.feed;
        match i {
            0 | 1 => {
                if f.wan_online {
                    0
                } else {
                    2
                }
            }
            2 => {
                if !f.internet || f.ping_loss >= 100.0 {
                    2
                } else if f.ping_ms < 50.0 && f.ping_loss == 0.0 {
                    0
                } else if f.ping_ms < 150.0 {
                    1
                } else {
                    2
                }
            }
            3 => {
                if !f.dns || f.dns_ms >= 200.0 {
                    2
                } else if f.dns_ms >= 50.0 {
                    1
                } else {
                    0
                }
            }
            4 => band(f.pve_cpu, 70.0, 90.0),
            5 => band(f.pve_mem, 80.0, 92.0),
            6 => {
                if f.pve_online {
                    0
                } else {
                    2
                }
            }
            7 => {
                if f.wan_online {
                    0
                } else {
                    2
                }
            }
            8 => bool_level(f.gateway),
            9 => bool_level(f.internet),
            10 => bool_level(f.pve_online),
            11 => bool_level(f.opn_reach),
            _ => 0,
        }
    }

    /// Current value of a channel, in its own units.
    pub fn ch_now(&self, i: usize) -> f32 {
        self.hist.last().map(|s| s.ch(i)).unwrap_or(0.0)
    }

    /// Magnitude of a channel normalised to 0..1, for the solid's vertex radius.
    pub fn ch_norm(&self, i: usize) -> f32 {
        (self.ch_now(i).abs() / ch_scale(i)).clamp(0.0, 1.0).sqrt()
    }

    /// Volatility over the last `window` samples: coefficient of variation,
    /// clamped to 0..1. A flapping link scores high; a quiet one scores ~0.
    pub fn ch_vol(&self, i: usize, window: usize) -> f32 {
        let n = self.hist.len().min(window);
        if n < 3 {
            return 0.0;
        }
        let tail = &self.hist[self.hist.len() - n..];
        let mean = tail.iter().map(|s| s.ch(i)).sum::<f32>() / n as f32;
        let var = tail.iter().map(|s| (s.ch(i) - mean).powi(2)).sum::<f32>() / n as f32;
        let sd = var.sqrt();
        // Coefficient of variation against a *small* per-channel floor, not the
        // full scale. With a full-scale denominator a low-rate channel's floor
        // swamps its mean, so a link swinging from 0.1 to 1.1 Mb/s - genuinely
        // erratic - scored as perfectly steady on the live screen. The floor is
        // only there to stop an idle channel dividing by ~0 and reading as
        // permanently thrashing.
        (sd / (mean.abs() + ch_vol_floor(i))).clamp(0.0, 1.0)
    }
}

/// Absolute floor for the volatility denominator, in the channel's own units.
fn ch_vol_floor(i: usize) -> f32 {
    match i {
        0 | 1 => 1.0, // Mb/s
        2 | 3 => 5.0, // ms
        4 | 5 => 3.0, // percent
        _ => 0.1,     // ratio / boolean
    }
}

fn band(v: f32, warn: f32, crit: f32) -> u8 {
    if v >= crit {
        2
    } else if v >= warn {
        1
    } else {
        0
    }
}

fn bool_level(ok: bool) -> u8 {
    if ok {
        0
    } else {
        2
    }
}

/// Deterministic pseudo-noise in -0.5..0.5, for preview histories only.
fn noise(seed: u32, k: usize) -> f32 {
    let mut x = seed.wrapping_mul(2654435761).wrapping_add(k as u32 * 7919);
    x ^= x >> 15;
    x = x.wrapping_mul(2246822519);
    x ^= x >> 13;
    (x & 0xffff) as f32 / 65535.0 - 0.5
}
