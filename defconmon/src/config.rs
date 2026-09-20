//! Runtime configuration: a flat key/value store plus the schema describing it.
//!
//! Deliberately decoupled. The web UI never names a screen or a metric: it asks
//! for `schema()` and renders whatever comes back. Screens declare their own
//! knobs in their `params()`, next to the code that consumes them, so adding a
//! setting is one entry in one place - and when screens become plugins, the
//! config tool needs no changes at all.
//!
//! Values live in one JSON file. serde_json is already a dependency, so this
//! costs nothing new, and the display process picks up edits by watching the
//! file's mtime - which is the *only* channel between the display and the
//! config server, so neither can take the other down.
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

use serde_json::Value;

pub const DEFAULT_PATH: &str = "/etc/defconmon/config.json";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Bool,
    Int,
    Float,
    Text,
    Choice,
}

/// One configurable setting. Keys are dotted and namespaced:
/// `display.fps` for app-wide, `radar.sweep_rpm` for a screen's.
pub struct Param {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: Kind,
    pub default: Value,
    pub min: f32,
    pub max: f32,
    pub step: f32,
    pub choices: &'static [&'static str],
    pub help: &'static str,
}

impl Param {
    pub fn b(key: &'static str, label: &'static str, default: bool, help: &'static str) -> Param {
        Param {
            key,
            label,
            kind: Kind::Bool,
            default: Value::Bool(default),
            min: 0.0,
            max: 0.0,
            step: 0.0,
            choices: &[],
            help,
        }
    }
    pub fn i(
        key: &'static str,
        label: &'static str,
        default: i64,
        min: f32,
        max: f32,
        help: &'static str,
    ) -> Param {
        Param {
            key,
            label,
            kind: Kind::Int,
            default: Value::from(default),
            min,
            max,
            step: 1.0,
            choices: &[],
            help,
        }
    }
    pub fn f(
        key: &'static str,
        label: &'static str,
        default: f32,
        min: f32,
        max: f32,
        step: f32,
        help: &'static str,
    ) -> Param {
        Param {
            key,
            label,
            kind: Kind::Float,
            default: Value::from(default),
            min,
            max,
            step,
            choices: &[],
            help,
        }
    }
    pub fn c(
        key: &'static str,
        label: &'static str,
        default: &str,
        choices: &'static [&'static str],
        help: &'static str,
    ) -> Param {
        Param {
            key,
            label,
            kind: Kind::Choice,
            default: Value::from(default),
            min: 0.0,
            max: 0.0,
            step: 0.0,
            choices,
            help,
        }
    }
    pub fn t(key: &'static str, label: &'static str, default: &str, help: &'static str) -> Param {
        Param {
            key,
            label,
            kind: Kind::Text,
            default: Value::from(default),
            min: 0.0,
            max: 0.0,
            step: 0.0,
            choices: &[],
            help,
        }
    }
    /// The part of the key before the dot: the group this setting shows under.
    pub fn group(&self) -> &'static str {
        match self.key.find('.') {
            Some(i) => &self.key[..i],
            None => "general",
        }
    }
    /// The part after the dot, for the widget's name attribute.
    pub fn leaf(&self) -> &'static str {
        match self.key.find('.') {
            Some(i) => &self.key[i + 1..],
            None => self.key,
        }
    }
}

/// App-wide settings. Screen-specific ones come from the screen registry.
pub fn global_params() -> Vec<Param> {
    vec![
        Param::i(
            "display.fps",
            "Refresh rate",
            12,
            4.0,
            30.0,
            "Frames per second. The whole cost of the dashboard scales with this, \
             so drop it to 8 if the container is loaded.",
        ),
        Param::i(
            "display.dwell",
            "Seconds per screen",
            20,
            5.0,
            300.0,
            "How long each screen stays up before the rotation advances.",
        ),
        Param::t(
            "display.screens",
            "Screen rotation",
            "wopr,defcon,radar,telemetry,vectrex",
            "Comma-separated, in order. Drop one to take it out of the rotation.",
        ),
        Param::c(
            "gpu.mode",
            "GPU present path",
            "auto",
            &["auto", "on", "off"],
            "Whether the GPU takes the upscale and CRT finish. `auto` uses it only \
             where it is likely to win - a non-GL backend, or a discrete GL part. \
             On old integrated GL hardware the texture upload and GL submission \
             cost more than the CPU pass, and the look does not depend on them: \
             the fonts and the vector art carry it, and both stay on the CPU.",
        ),
        Param::b(
            "hud.visible",
            "Status plate",
            true,
            "The DEFCON/WAN/PING/DNS plate that rides on every screen. Kept on all \
             screens so the house state is readable without waiting for a screen.",
        ),
    ]
}

pub struct Cfg {
    pub path: PathBuf,
    pub vals: BTreeMap<String, Value>,
    mtime: Option<SystemTime>,
}

impl Cfg {
    pub fn load(path: impl Into<PathBuf>) -> Cfg {
        let path = path.into();
        let (vals, mtime) = Self::read(&path);
        Cfg { path, vals, mtime }
    }

    fn read(path: &PathBuf) -> (BTreeMap<String, Value>, Option<SystemTime>) {
        let mtime = fs::metadata(path).ok().and_then(|m| m.modified().ok());
        let vals = fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Map<String, Value>>(&t).ok())
            .map(|m| m.into_iter().collect())
            .unwrap_or_default();
        (vals, mtime)
    }

    /// Re-read if the file changed on disk. Returns true when values moved, so
    /// the caller can log it. Cheap enough to call once a second.
    pub fn reload_if_changed(&mut self) -> bool {
        let now = fs::metadata(&self.path).ok().and_then(|m| m.modified().ok());
        if now == self.mtime {
            return false;
        }
        let (vals, mtime) = Self::read(&self.path);
        self.vals = vals;
        self.mtime = mtime;
        true
    }

    pub fn save(&self) -> std::io::Result<()> {
        let obj: serde_json::Map<String, Value> =
            self.vals.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let txt = serde_json::to_string_pretty(&Value::Object(obj)).unwrap_or_default();
        if let Some(dir) = self.path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        fs::write(&self.path, txt + "\n")
    }

    pub fn raw(&self, key: &str) -> Option<&Value> {
        self.vals.get(key)
    }

    pub fn i(&self, key: &str, def: i64) -> i64 {
        self.vals
            .get(key)
            .and_then(|v| match v {
                Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
                Value::String(s) => s.trim().parse().ok(),
                Value::Bool(b) => Some(*b as i64),
                _ => None,
            })
            .unwrap_or(def)
    }

    pub fn f(&self, key: &str, def: f32) -> f32 {
        self.vals
            .get(key)
            .and_then(|v| match v {
                Value::Number(n) => n.as_f64().map(|x| x as f32),
                Value::String(s) => s.trim().parse().ok(),
                _ => None,
            })
            .unwrap_or(def)
    }

    pub fn b(&self, key: &str, def: bool) -> bool {
        self.vals
            .get(key)
            .and_then(|v| match v {
                Value::Bool(x) => Some(*x),
                Value::Number(n) => n.as_f64().map(|x| x != 0.0),
                Value::String(s) => Some(s == "1" || s.eq_ignore_ascii_case("true")),
                _ => None,
            })
            .unwrap_or(def)
    }

    pub fn s(&self, key: &str, def: &str) -> String {
        self.vals
            .get(key)
            .and_then(|v| match v {
                Value::String(t) => Some(t.clone()),
                Value::Number(n) => Some(n.to_string()),
                Value::Bool(x) => Some(x.to_string()),
                _ => None,
            })
            .unwrap_or_else(|| def.to_string())
    }
}

/// Coerce one raw form string into the type the schema says the key has, and
/// clamp it to its declared range. Returns Err with a human-readable reason so
/// the UI can show which field it rejected.
pub fn coerce(p: &Param, raw: &str) -> Result<Value, String> {
    let raw = raw.trim();
    match p.kind {
        Kind::Bool => Ok(Value::Bool(raw == "on" || raw == "1" || raw == "true")),
        Kind::Text => Ok(Value::from(raw)),
        Kind::Choice => {
            if p.choices.iter().any(|c| *c == raw) {
                Ok(Value::from(raw))
            } else {
                Err(format!("'{}' is not one of the allowed values", raw))
            }
        }
        Kind::Int => {
            let v: f32 = raw.parse().map_err(|_| format!("'{raw}' is not a number"))?;
            if v < p.min || v > p.max {
                return Err(format!("{v} is outside {}..{}", p.min, p.max));
            }
            Ok(Value::from(v.round() as i64))
        }
        Kind::Float => {
            let v: f32 = raw.parse().map_err(|_| format!("'{raw}' is not a number"))?;
            if v < p.min || v > p.max {
                return Err(format!("{v} is outside {}..{}", p.min, p.max));
            }
            // store at f32 precision: Value::from(f32) widens to f64 and would
            // write 0.4000000059604645 into an otherwise hand-readable file
            Ok(Value::from((v as f64 * 10_000.0).round() / 10_000.0))
        }
    }
}
