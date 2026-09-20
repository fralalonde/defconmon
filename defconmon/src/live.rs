//! Live display path: a minimal Wayland client (pure-Rust backend, no
//! libwayland) that presents the rendered frames to whatever compositor cage
//! provides, full-screen, with double-buffered shared memory.
//!
//! Format note: tiny-skia's buffer is premultiplied RGBA8888 (byte order
//! R,G,B,A). wl_shm's XBGR8888 has the same byte order, so those frames can be
//! blitted with a straight memcpy. If the compositor only offers XRGB8888 we
//! fall back to that and swizzle R/B during the copy.
use std::os::unix::io::AsFd;
use std::time::{Duration, Instant};

use wayland_client::{
    globals::{registry_queue_init, GlobalListContents},
    protocol::{wl_buffer, wl_callback, wl_compositor, wl_registry, wl_shm, wl_shm_pool, wl_surface},
    Connection, Dispatch, QueueHandle,
};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

use crate::canvas::Canvas;
use crate::config::Cfg;
use crate::gpu;
use wayland_client::Proxy;
use crate::data::Snap;
use crate::fonts::Fonts;
use crate::screens;
use crate::screens::Env;

struct Buf {
    buffer: wl_buffer::WlBuffer,
    map: memmap2::MmapMut,
    busy: bool,
}

pub struct State {
    has_xbgr: bool,
    buffers: Vec<Buf>,
    configured: bool,
    size: (u32, u32),
    frame_done: bool,
    closed: bool,
}

impl State {
    fn new() -> Self {
        State {
            has_xbgr: false,
            buffers: Vec::new(),
            configured: false,
            size: (1920, 1080),
            frame_done: true,
            closed: false,
        }
    }
}

macro_rules! noop_dispatch {
    ($($t:ty),* $(,)?) => {$(
        impl Dispatch<$t, ()> for State {
            fn event(
                _: &mut Self,
                _: &$t,
                _: <$t as wayland_client::Proxy>::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {}
        }
    )*};
}

noop_dispatch!(wl_compositor::WlCompositor, wl_shm_pool::WlShmPool, wl_surface::WlSurface);

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_shm::WlShm, ()> for State {
    fn event(
        state: &mut Self,
        _: &wl_shm::WlShm,
        event: wl_shm::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_shm::Event::Format { format } = event {
            // 4 == XBGR8888 (byte order R,G,B,X): zero-copy for tiny-skia output
            if format == wayland_client::WEnum::Value(wl_shm::Format::Xbgr8888) {
                state.has_xbgr = true;
            }
        }
    }
}

impl Dispatch<wl_buffer::WlBuffer, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_buffer::Event::Release = event {
            for b in state.buffers.iter_mut() {
                if &b.buffer == proxy {
                    b.busy = false;
                }
            }
        }
    }
}

impl Dispatch<wl_callback::WlCallback, ()> for State {
    fn event(
        state: &mut Self,
        _: &wl_callback::WlCallback,
        _: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        state.frame_done = true;
    }
}

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for State {
    fn event(
        _: &mut Self,
        proxy: &xdg_wm_base::XdgWmBase,
        event: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            proxy.pong(serial);
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for State {
    fn event(
        _: &mut Self,
        proxy: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            proxy.ack_configure(serial);
        }
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, ()> for State {
    fn event(
        state: &mut Self,
        _: &xdg_toplevel::XdgToplevel,
        event: xdg_toplevel::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            xdg_toplevel::Event::Configure { width, height, .. } => {
                if width > 0 && height > 0 {
                    state.size = (width as u32, height as u32);
                }
                state.configured = true;
            }
            xdg_toplevel::Event::Close => state.closed = true,
            _ => {}
        }
    }
}

fn make_buffer(
    shm: &wl_shm::WlShm,
    qh: &QueueHandle<State>,
    w: u32,
    h: u32,
    format: wl_shm::Format,
) -> Buf {
    let stride = w * 4;
    let size = (stride * h) as usize;
    // Back the pool with tmpfs and unlink immediately: the mapping outlives the
    // name, so nothing is left behind even on a hard kill.
    let path = format!("/dev/shm/defconmon-{}-{}", std::process::id(), size);
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)
        .expect("shm file");
    file.set_len(size as u64).expect("truncate");
    let map = unsafe { memmap2::MmapMut::map_mut(&file).expect("mmap") };
    let _ = std::fs::remove_file(&path);

    let pool = shm.create_pool(file.as_fd(), size as i32, qh, ());
    let buffer = pool.create_buffer(0, w as i32, h as i32, stride as i32, format, qh, ());
    pool.destroy();
    Buf { buffer, map, busy: false }
}

/// Nearest-neighbour upscale (integer factor) plus optional R/B swizzle.
/// Word-at-a-time: the fast path walks *source* rows and emits two destination
/// rows per iteration, which halves the loop count versus walking the output.
fn blit(src: &[u8], sw: u32, dw: u32, dh: u32, dst: &mut [u8], swizzle: bool, _row: &mut Vec<u8>) {
    let inner = dw as usize;
    let swn = sw as usize;
    let factor = (dw / sw.max(1)).max(1) as usize;
    // the mmap backing the shm pool is page-aligned, so a u32 view is sound
    let dst32: &mut [u32] =
        unsafe { std::slice::from_raw_parts_mut(dst.as_mut_ptr() as *mut u32, inner * dh as usize) };

    if factor == 2 && !swizzle {
        for sy in 0..(dh as usize / 2) {
            let srow = sy * swn * 4;
            let o1 = sy * 2 * inner;
            let o2 = o1 + inner;
            for x in 0..inner {
                let sp = srow + (x >> 1) * 4;
                match src[sp..sp + 4].try_into() {
                    Ok(b) => {
                        let px = u32::from_ne_bytes(b);
                        dst32[o1 + x] = px;
                        dst32[o2 + x] = px;
                    }
                    Err(_) => {}
                }
            }
        }
        return;
    }

    for y in 0..dh as usize {
        let srow = (y / factor) * swn * 4;
        let o = y * inner;
        for x in 0..inner {
            let sp = srow + (x / factor) * 4;
            let mut px = match src[sp..sp + 4].try_into() {
                Ok(b) => u32::from_ne_bytes(b),
                Err(_) => 0,
            };
            if swizzle {
                px = (px & 0xff00_ff00) | ((px & 0xff) << 16) | ((px >> 16) & 0xff);
            }
            dst32[o + x] = px;
        }
    }
}

/// Parse the rotation list, dropping anything the registry does not know and
/// falling back to every registered screen if that leaves nothing.
fn parse_screens(cfg: &Cfg) -> Vec<String> {
    let v: Vec<String> = cfg
        .s("display.screens", "")
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| screens::find(s).is_some())
        .collect();
    if v.is_empty() {
        screens::names().iter().map(|s| s.to_string()).collect()
    } else {
        v
    }
}

/// Run the live loop forever, cycling the configured screens.
///
/// Every knob here - rotation, pace, per-screen settings - comes from the config
/// file, which is re-read whenever it changes. That file is the *entire* link
/// between this process and the config server, so the UI can be restarted,
/// rebuilt, or absent altogether without the display noticing.
pub fn run(conn: &Connection, mut cfg: Cfg) -> ! {
    let (globals, mut queue) = registry_queue_init::<State>(conn).expect("wayland globals");
    let qh = queue.handle();

    let compositor = globals
        .bind::<wl_compositor::WlCompositor, _, _>(&qh, 1..=6, ())
        .expect("wl_compositor");
    let shm = globals.bind::<wl_shm::WlShm, _, _>(&qh, 1..=1, ()).expect("wl_shm");
    let wm_base = globals
        .bind::<xdg_wm_base::XdgWmBase, _, _>(&qh, 1..=4, ())
        .expect("xdg_wm_base");

    let mut state = State::new();
    // one roundtrip so the shm format list arrives before we pick a format
    queue.roundtrip(&mut state).expect("roundtrip");

    let surface = compositor.create_surface(&qh, ());
    let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
    let toplevel = xdg.get_toplevel(&qh, ());
    toplevel.set_title("defconmon".into());
    toplevel.set_app_id("defconmon".into());
    surface.commit();

    // the first configure gives us the real surface size
    while !state.configured {
        if queue.blocking_dispatch(&mut state).is_err() || state.closed {
            std::process::exit(0);
        }
    }

    let (w, h) = state.size;
    // Half-resolution rasterisation on a big screen, upscaled on present: 4x less
    // rasteriser work, and the chunkier pixels suit a CRT anyway.
    let scale = if w >= 1600 { 0.5 } else { 1.0 };
    let rsw = ((w as f32 * scale).round() as u32).max(1);
    let rsh = ((h as f32 * scale).round() as u32).max(1);

    // ---- present path ------------------------------------------------------
    // The scene is rasterised on the CPU either way. If a GPU is available it
    // takes the upscale and the whole CRT finish and presents straight to the
    // Wayland surface; otherwise we keep the CPU pass and blit into shared
    // memory. Decided once: both paths own the surface and cannot interleave.
    let mut gpu: Option<gpu::Gpu> = None;
    let gpu_mode = cfg.s("gpu.mode", "auto");
    if std::env::var_os("DEFCONMON_NO_GPU").is_some() || gpu_mode == "off" {
        eprintln!("defconmon: GPU path off ({gpu_mode}); using the CPU/shm path");
    } else {
        let dpy = conn.backend().display_ptr() as *mut std::ffi::c_void;
        let surf = surface.id().as_ptr() as *mut std::ffi::c_void;
        eprintln!(
            "defconmon: raw handles display={} surface={} (non-null)",
            !dpy.is_null(),
            !surf.is_null()
        );
        // catch_unwind as well as Err: wgpu panics on validation errors (a bad
        // shader, an unsupported format), and a panic here must degrade to the
        // CPU path rather than crash-loop the family's screen.
        let mode = gpu_mode.clone();
        let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            gpu::Gpu::new(dpy, surf, w, h, rsw, rsh, &mode)
        }));
        match attempt {
            Ok(Ok(g)) => gpu = Some(g),
            Ok(Err(e)) => {
                eprintln!("defconmon: GPU path unavailable ({e}); using the CPU/shm path")
            }
            Err(_) => eprintln!(
                "defconmon: GPU path panicked during init; using the CPU/shm path (set \
                 RUST_BACKTRACE=1 to see it)"
            ),
        }
    }
    let format = if state.has_xbgr {
        wl_shm::Format::Xbgr8888
    } else {
        wl_shm::Format::Xrgb8888
    };
    let swizzle = format == wl_shm::Format::Xrgb8888;
    if gpu.is_none() {
        eprintln!(
            "defconmon: surface {}x{}, shm format {:?}{}",
            w,
            h,
            format,
            if swizzle { " (swizzling R/B)" } else { " (zero-copy)" }
        );
        state.buffers = vec![
            make_buffer(&shm, &qh, w, h, format),
            make_buffer(&shm, &qh, w, h, format),
        ];
    }

    let fonts = Fonts::load();
    let mut canvas = Canvas::with_scale(w, h, scale);
    let mut row_buf: Vec<u8> = Vec::new();
    let mut feed = Snap::load();
    let start = Instant::now();
    let mut last_feed = Instant::now();

    let mut names = parse_screens(&cfg);
    let mut dwell = cfg.i("display.dwell", 20).max(1) as f32;
    let mut fps = cfg.i("display.fps", 12).clamp(4, 30) as f32;
    let mut frame_budget = Duration::from_secs_f32(1.0 / fps);
    let mut next = Instant::now();
    let mut last_cfg = Instant::now();
    // DEFCONMON_STATS=1 prints a render/blit split every 5 s: the only way to know which
    // half of the frame to optimise, and whether a GPU is worth involving at all.
    let stats = std::env::var_os("DEFCONMON_STATS").is_some();
    let mut acc_render = 0f64;
    let mut acc_blit = 0f64;
    let mut acc_n = 0u32;
    let mut last_stats = Instant::now();
    eprintln!("defconmon: screens={names:?}  dwell={dwell:.0}s  fps={fps:.0}");

    loop {
        if state.closed {
            std::process::exit(0);
        }
        // refresh the data feed once a second, off the render path
        if last_feed.elapsed() >= Duration::from_secs(1) {
            // take the fresh values but KEEP the trend buffer: one sample per
            // second is what the volatility channel is computed from
            let fresh = Snap::load();
            feed.feed = fresh.feed;
            let s = feed.sample();
            feed.push(s);
            last_feed = Instant::now();
        }

        // re-read the config file when the config server rewrites it
        if last_cfg.elapsed() >= Duration::from_secs(1) {
            if cfg.reload_if_changed() {
                names = parse_screens(&cfg);
                dwell = cfg.i("display.dwell", 20).max(1) as f32;
                fps = cfg.i("display.fps", 12).clamp(4, 30) as f32;
                frame_budget = Duration::from_secs_f32(1.0 / fps);
                eprintln!(
                    "defconmon: config reloaded  screens={names:?}  dwell={dwell:.0}s  fps={fps:.0}"
                );
            }
            last_cfg = Instant::now();
        }

        // pick a free buffer (the GPU path owns the surface, no shm buffers)
        let mut idx = 0usize;
        if gpu.is_none() {
            match state.buffers.iter().position(|b| !b.busy) {
                Some(i) => idx = i,
                None => {
                    if queue.blocking_dispatch(&mut state).is_err() {
                        std::process::exit(0);
                    }
                    continue;
                }
            }
        }

        let t = start.elapsed().as_secs_f32();
        let pick = names[((t / dwell.max(0.1)) as usize) % names.len()].as_str();
        let t_render = Instant::now();
        canvas.clear();
        let env = Env { cfg: &cfg, snap: &feed, f: &fonts, gpu_crt: gpu.is_some() };
        screens::render(pick, &mut canvas, &env, t);
        let render_ms = t_render.elapsed().as_secs_f64() * 1000.0;

        let t_blit = Instant::now();
        if let Some(g) = gpu.as_mut() {
            // upload the scene; the shader does the upscale and the CRT finish
            if let Err(e) = g.present(canvas.pm.data()) {
                eprintln!("defconmon: present failed: {e}");
            }
        } else {
            // blit into the shared buffer (upscaling if we rendered small; the
            // swizzle only happens when the compositor lacks XBGR8888)
            let dst = &mut state.buffers[idx].map;
            let src = canvas.pm.data();
            let sw = canvas.pm.width();
            if sw == w && !swizzle {
                dst.copy_from_slice(src);
            } else {
                blit(src, sw, w, h, dst, swizzle, &mut row_buf);
            }
        }

        let blit_ms = t_blit.elapsed().as_secs_f64() * 1000.0;
        if stats {
            acc_render += render_ms;
            acc_blit += blit_ms;
            acc_n += 1;
            if last_stats.elapsed() >= Duration::from_secs(5) {
                let secs = last_stats.elapsed().as_secs_f64();
                let n = acc_n as f64;
                eprintln!(
                    "defconmon: {acc_n} frames | render {:.2} ms | blit {:.2} ms | {:.1}% of a core",
                    acc_render / n,
                    acc_blit / n,
                    (acc_render + acc_blit) / secs / 10.0
                );
                acc_render = 0.0;
                acc_blit = 0.0;
                acc_n = 0;
                last_stats = Instant::now();
            }
        }

        if gpu.is_none() {
            let buf = &mut state.buffers[idx];
            buf.busy = true;
            surface.attach(Some(&buf.buffer), 0, 0);
            surface.damage(0, 0, w as i32, h as i32);
            surface.frame(&qh, ());
            state.frame_done = false;
            surface.commit();
        }
        queue.flush().ok();

        // pace: don't spin faster than the requested fps
        next += frame_budget;
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        } else {
            next = now;
        }
        // drain events without blocking so buffer releases are processed
        while queue.dispatch_pending(&mut state).unwrap_or(0) > 0 {}
        if !state.frame_done {
            let _ = queue.blocking_dispatch(&mut state);
        }
    }
}
