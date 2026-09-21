//! Config server: std-only HTTP + htmx.
//!
//! A separate process (`defconmon-config --serve PORT`) behind the `web` feature, so
//! the display binary carries none of it. No framework, no new dependencies:
//! std::net::TcpListener and a hand-rolled HTTP/1.1 response. htmx is vendored
//! into the binary, so the page works with no internet access either.
//!
//! It knows nothing about screens or metrics. It walks the schema - the global
//! settings plus whatever the screen registry contributes - and renders a widget
//! per entry, so a new screen shows up here for free, and the whole thing can be
//! pointed at a different set of screens later without edits.
//!
//! It writes config.json and nothing else. The display notices via the file's
//! mtime, which is the only channel between the two processes: no IPC, no
//! sockets, and no way for this process to disturb the screen.
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use crate::config::{self, Cfg, Kind, Param};
use crate::screens;

const CSS: &str = include_str!("../assets/theme.css");
const HTMX: &[u8] = include_bytes!("../assets/htmx.min.js");

pub fn serve(cfg: Cfg, port: u16) -> ! {
    let shared = Arc::new(Mutex::new(cfg));
    let listener = match TcpListener::bind(("0.0.0.0", port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("defconmon: cannot bind port {port}: {e}");
            std::process::exit(1);
        }
    };
    let count = settings().len();
    eprintln!(
        "defconmon: config server on http://0.0.0.0:{port}  ({count} settings, \
         file {})",
        shared.lock().map(|c| c.path.display().to_string()).unwrap_or_default()
    );
    for stream in listener.incoming() {
        let Ok(s) = stream else { continue };
        let cfg = Arc::clone(&shared);
        std::thread::spawn(move || {
            if let Err(e) = handle(s, cfg) {
                eprintln!("defconmon: request failed: {e}");
            }
        });
    }
    unreachable!()
}

fn handle(mut s: TcpStream, cfg: Arc<Mutex<Cfg>>) -> std::io::Result<()> {
    let mut r = BufReader::new(s.try_clone()?);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let mut it = line.split_whitespace();
    let method = it.next().unwrap_or("GET").to_string();
    let path = it.next().unwrap_or("/").to_string();

    let mut len = 0usize;
    let mut is_hx = false;
    loop {
        let mut h = String::new();
        if r.read_line(&mut h)? == 0 || h == "\r\n" || h == "\n" {
            break;
        }
        let l = h.to_ascii_lowercase();
        if let Some(v) = l.strip_prefix("content-length:") {
            len = v.trim().parse().unwrap_or(0);
        }
        if l.starts_with("hx-request:") {
            is_hx = true;
        }
    }
    let mut raw = vec![0u8; len];
    if len > 0 {
        r.read_exact(&mut raw)?;
    }
    let body = String::from_utf8_lossy(&raw).to_string();

    let (status, ctype, payload) = route(&method, &path, &body, &cfg, is_hx);
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    s.write_all(head.as_bytes())?;
    s.write_all(&payload)?;
    s.flush()
}

type Reply = (&'static str, &'static str, Vec<u8>);

fn route(method: &str, path: &str, body: &str, cfg: &Mutex<Cfg>, is_hx: bool) -> Reply {
    let path = path.split('?').next().unwrap_or("/");
    match (method, path) {
        ("POST", "/save") => {
            let (flash, ok) = save(body, cfg);
            let mut c = cfg.lock().unwrap();
            let html = if ok && is_hx {
                fragment(&c, &flash)
            } else {
                // no htmx (or a rejected save): full page so it still works
                page(&c, &flash)
            };
            // keep the borrow short; page() takes &Cfg only
            let _ = &mut c;
            ("200 OK", "text/html; charset=utf-8", html.into_bytes())
        }
        (_, "/form") => {
            let c = cfg.lock().unwrap();
            let html = fragment(&c, "");
            ("200 OK", "text/html; charset=utf-8", html.into_bytes())
        }
        (_, "/theme.css") => ("200 OK", "text/css", CSS.as_bytes().to_vec()),
        (_, "/htmx.min.js") => (
            "200 OK",
            "application/javascript",
            HTMX.to_vec(),
        ),
        (_, "/fonts/3270.ttf") => font(crate::fonts::F_3270),
        (_, "/fonts/dseg7.ttf") => font(crate::fonts::F_DSEG7),
        (_, "/") => {
            let c = cfg.lock().unwrap();
            let html = page(&c, "");
            ("200 OK", "text/html; charset=utf-8", html.into_bytes())
        }
        _ => (
            "404 Not Found",
            "text/plain",
            b"no such route\n".to_vec(),
        ),
    }
}

fn font(bytes: &'static [u8]) -> Reply {
    ("200 OK", "font/ttf", bytes.to_vec())
}

/// Every setting in the app, grouped into sections. Global first, then one per
/// registered screen - in registry order, with no screen names hardcoded here.
fn sections() -> Vec<(&'static str, &'static str, Vec<Param>)> {
    let mut v = vec![(
        "GENERAL",
        "Applies to the whole display.",
        config::global_params(),
    )];
    for s in screens::SCREENS {
        v.push((s.title, s.blurb, (s.params)()));
    }
    v
}

fn settings() -> Vec<Param> {
    sections().into_iter().flat_map(|(_, _, p)| p).collect()
}

/// Format a float for a form field: f32 -> f64 widening turns 0.9 into
/// 0.8999999761581421, which must not reach the input.
fn fnum(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" {
        "0".to_string()
    } else {
        s.to_string()
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn page(cfg: &Cfg, flash: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="en"><head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>DEFCONMON // CONFIG</title>
<link rel="stylesheet" href="/theme.css">
<script src="/htmx.min.js"></script>
</head><body>
<div class="bar">
  <span class="brand">DEFCONMON</span><span class="sep">//</span>
  <span class="sub">CONFIGURATION</span>
  <span class="right"><a href="/">RELOAD</a></span>
</div>
<main id="form">{}</main>
<div class="bar bottom">
  <span class="note">The display re-reads this file when it changes &mdash; no restart.</span>
  <span class="right">{}</span>
</div>
</body></html>"#,
        fragment(cfg, flash),
        esc(&cfg.path.display().to_string())
    )
}

fn fragment(cfg: &Cfg, flash: &str) -> String {
    let mut o = String::new();
    o.push_str("<form hx-post=\"/save\" hx-target=\"#form\" hx-swap=\"innerHTML\">");
    for (title, blurb, params) in sections() {
        o.push_str("<section><header>");
        o.push_str(&format!("<h2>{}</h2>", esc(title)));
        if !blurb.is_empty() {
            o.push_str(&format!("<span class=\"blurb\">{}</span>", esc(blurb)));
        }
        o.push_str("</header><div class=\"body\">");
        for p in params {
            o.push_str(&row(cfg, &p));
        }
        o.push_str("</div></section>");
    }
    let cls = if flash.starts_with("SAVED") { "ok" } else { "err" };
    o.push_str("<div class=\"actions\">");
    o.push_str("<button class=\"btn\" type=\"submit\">Apply</button>");
    if !flash.is_empty() {
        o.push_str(&format!("<span class=\"flash {cls}\">{flash}</span>"));
    }
    o.push_str("<span class=\"right note\">changes apply within a second</span>");
    o.push_str("</div></form>");
    o
}

fn row(cfg: &Cfg, p: &Param) -> String {
    let name = esc(p.key);
    let cur = cfg.raw(p.key).cloned().unwrap_or_else(|| p.default.clone());
    let widget = match p.kind {
        Kind::Bool => {
            let on = cfg.b(p.key, p.default.as_bool().unwrap_or(false));
            format!(
                "<label class=\"tog\"><input type=\"checkbox\" name=\"{name}\"{}> \
                 <span>{}</span></label>",
                if on { " checked" } else { "" },
                if on { "ENABLED" } else { "DISABLED" }
            )
        }
        Kind::Int | Kind::Float => {
            let v = cur.as_f64().unwrap_or(0.0);
            let shown = if p.kind == Kind::Int {
                format!("{}", v as i64)
            } else {
                fnum(v)
            };
            format!(
                "<input type=\"number\" name=\"{name}\" value=\"{shown}\" min=\"{}\" max=\"{}\" \
                 step=\"{}\">",
                p.min, p.max, p.step
            )
        }
        Kind::Text => format!(
            "<input type=\"text\" name=\"{name}\" value=\"{}\">",
            esc(cur.as_str().unwrap_or(""))
        ),
        Kind::Choice => {
            let sel = cur.as_str().unwrap_or("");
            let mut s = format!("<select name=\"{name}\">");
            for c in p.choices {
                s.push_str(&format!(
                    "<option value=\"{c}\"{}>{c}</option>",
                    if *c == sel { " selected" } else { "" }
                ));
            }
            s.push_str("</select>");
            s
        }
    };
    let def = match p.kind {
        Kind::Bool => String::new(),
        Kind::Text => format!("default {}", esc(p.default.as_str().unwrap_or(""))),
        Kind::Float => format!("default {}", fnum(p.default.as_f64().unwrap_or(0.0))),
        _ => format!("default {} &middot; {} to {}", p.default, p.min, p.max),
    };
    format!(
        "<div class=\"row\" id=\"row-{name}\">\
           <label class=\"lbl\" for=\"{name}\">{label}<span class=\"key\">{name}</span></label>\
           <div class=\"wdg\">{widget}</div>\
           <div class=\"help\">{help} <span class=\"def\">{def}</span></div>\
         </div>",
        label = esc(p.label),
        help = esc(p.help)
    )
}

/// Parse the urlencoded body, coerce every field against the schema, save, and
/// report. Returns (flash html, saved?). Unchecked checkboxes are absent from
/// the body, which is exactly how a false boolean arrives.
fn save(body: &str, cfg: &Mutex<Cfg>) -> (String, bool) {
    let mut posted: Vec<(String, String)> = Vec::new();
    for pair in body.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        posted.push((urldecode(k), urldecode(v)));
    }
    let get = |k: &str| posted.iter().find(|(a, _)| a == k).map(|(_, v)| v.clone());

    let mut errs: Vec<String> = Vec::new();
    let mut fresh: Vec<(String, serde_json::Value)> = Vec::new();
    for p in settings() {
        match p.kind {
            Kind::Bool => {
                fresh.push((p.key.into(), serde_json::Value::Bool(get(p.key).is_some())));
            }
            _ => {
                let Some(raw) = get(p.key) else { continue };
                match config::coerce(&p, &raw) {
                    Ok(v) => fresh.push((p.key.into(), v)),
                    Err(e) => errs.push(format!("{}: {e}", p.label)),
                }
            }
        }
    }
    if !errs.is_empty() {
        let items: String = errs.iter().map(|e| format!("<li>{}</li>", esc(e))).collect();
        return (
            format!("REJECTED &mdash; nothing saved<ul>{items}</ul>"),
            false,
        );
    }
    let mut c = cfg.lock().unwrap();
    for (k, v) in fresh {
        c.vals.insert(k, v);
    }
    match c.save() {
        Ok(()) => (
            format!("SAVED &mdash; {} settings written", settings().len()),
            true,
        ),
        Err(e) => (format!("SAVE FAILED: {}", esc(&e.to_string())), false),
    }
}

fn urldecode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < b.len() => {
                let hex = std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(b[i]);
                        i += 1;
                    }
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}
