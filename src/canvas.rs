//! Canvas: a thin drawing layer over tiny-skia (CPU/vector, no GPU needed),
//! plus the CRT post-effects (scanlines, glow, vignette, flicker, noise) that
//! give the whole thing its phosphor look. Text is rendered as real glyph
//! outlines so it scales cleanly and can be stroked for glow.
//!
//! RESOLUTION: screens lay out in *design units* (1920x1080) via `cv.w`/`cv.h`.
//! The backing pixmap may be smaller -- `with_scale(w, h, 0.5)` rasterises at
//! 960x540 for the live path, which cuts rasteriser work 4x; the caller
//! upscales on blit. Previews use scale 1.0 so the PNGs stay full quality.
use ab_glyph::{Font, FontRef, OutlineCurve, PxScale, ScaleFont};
use tiny_skia::{
    FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Rect, Shader,
    Stroke, Transform,
};

pub fn rgba(rgb: u32, a: u8) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(
        ((rgb >> 16) & 0xff) as u8,
        ((rgb >> 8) & 0xff) as u8,
        (rgb & 0xff) as u8,
        a,
    )
}

fn paint(rgb: u32, a: u8) -> Paint<'static> {
    Paint {
        shader: Shader::SolidColor(rgba(rgb, a)),
        anti_alias: true,
        ..Default::default()
    }
}

// The CRT finish, in one place: these four numbers are the whole look.
const SCAN_STEP: f32 = 3.0; // design units between scanlines
const SCAN_STRENGTH: f32 = 40.0; // how far a scanline darkens
const VIG_STRENGTH: f32 = 196.0; // how far the corners darken
const FLICKER_STRENGTH: f32 = 9.0; // global brightness wobble
const GRAIN_COUNT: usize = 320; // speckles per frame
const GRAIN_ALPHA: u8 = 18;
/// Scanline pitch in *device* pixels. The GPU path works in destination space
/// (so scanlines land on real display rows), and reads it from here.
pub const SCAN_STEP_DEVICE: u32 = 3;

pub struct Canvas {
    pub pm: Pixmap,
    /// design width  (what screens lay out against)
    pub w: f32,
    /// design height
    pub h: f32,
    /// device pixels per design unit
    scale: f32,
    seed: u32,
    /// static CRT mask (scanlines x vignette), one byte per device pixel
    keep: Vec<u8>,
}

// The drawing API is deliberately positional (x, y, size, colour, alpha) to
// match tiny-skia's style; bundling them into a struct would churn every screen.
#[allow(clippy::too_many_arguments)]
impl Canvas {
    pub fn new(w: u32, h: u32) -> Self {
        Self::with_scale(w, h, 1.0)
    }

    pub fn with_scale(w: u32, h: u32, scale: f32) -> Self {
        let dw = ((w as f32 * scale).round() as u32).max(1);
        let dh = ((h as f32 * scale).round() as u32).max(1);
        let mut pm = Pixmap::new(dw, dh).expect("pixmap");
        pm.fill(rgba(0x000000, 255));
        Self {
            pm,
            w: w as f32,
            h: h as f32,
            scale,
            seed: 0x1234_5678,
            keep: Vec::new(),
        }
    }

    /// Reset to black between frames (used by the live loop).
    pub fn clear(&mut self) {
        self.pm.fill(rgba(0x000000, 255));
    }

    fn rnd(&mut self) -> f32 {
        // xorshift for cheap deterministic noise
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        (x & 0xffffff) as f32 / 0xffffff as f32
    }

    /// Device-space rectangle. Everything else goes through here.
    fn drect(&mut self, x: f32, y: f32, w: f32, h: f32, rgb: u32, a: u8) {
        if let Some(r) = Rect::from_xywh(x, y, w, h) {
            self.pm.fill_rect(r, &paint(rgb, a), Transform::identity(), None);
        }
    }

    pub fn fill(&mut self, rgb: u32, a: u8) {
        self.pm.fill(rgba(rgb, a));
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, rgb: u32, a: u8) {
        let s = self.scale;
        self.drect(x * s, y * s, w * s, h * s, rgb, a);
    }

    pub fn rect_outline(&mut self, x: f32, y: f32, w: f32, h: f32, lw: f32, rgb: u32, a: u8) {
        let s = self.scale;
        let (x, y, w, h) = (x * s, y * s, w * s, h * s);
        let mut pb = PathBuilder::new();
        pb.move_to(x, y);
        pb.line_to(x + w, y);
        pb.line_to(x + w, y + h);
        pb.line_to(x, y + h);
        pb.close();
        if let Some(p) = pb.finish() {
            self.pm.stroke_path(&p, &paint(rgb, a), &stroke(lw * s), Transform::identity(), None);
        }
    }

    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, lw: f32, rgb: u32, a: u8) {
        let s = self.scale;
        if let Some(p) = line_path(x0 * s, y0 * s, x1 * s, y1 * s) {
            self.pm.stroke_path(&p, &paint(rgb, a), &stroke(lw * s), Transform::identity(), None);
        }
    }

    /// A glowing vector line: wide dim halo + bright core (Vectrex/DEFCON vibe).
    pub fn glow_line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, lw: f32, rgb: u32) {
        self.line(x0, y0, x1, y1, lw * 3.2, rgb, 30);
        self.line(x0, y0, x1, y1, lw, rgb, 235);
    }

    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, lw: f32, rgb: u32, a: u8, glow: bool) {
        let s = self.scale;
        let (dcx, dcy, dr) = (cx * s, cy * s, r * s);
        let mut pb = PathBuilder::new();
        pb.push_circle(dcx, dcy, dr);
        if let Some(p) = pb.finish() {
            if glow {
                self.pm.stroke_path(&p, &paint(rgb, 34), &stroke(lw * s * 2.6), Transform::identity(), None);
            }
            self.pm.stroke_path(&p, &paint(rgb, a), &stroke(lw * s), Transform::identity(), None);
        }
    }

    pub fn arc(&mut self, cx: f32, cy: f32, r: f32, a0: f32, a1: f32, lw: f32, rgb: u32, a: u8) {
        let s = self.scale;
        let (cx, cy, r) = (cx * s, cy * s, r * s);
        let steps = (((a1 - a0).abs() * r / 6.0) as usize).max(8);
        let mut pb = PathBuilder::new();
        for i in 0..=steps {
            let t = a0 + (a1 - a0) * (i as f32 / steps as f32);
            let (x, y) = (cx + r * t.cos(), cy + r * t.sin());
            if i == 0 {
                pb.move_to(x, y);
            } else {
                pb.line_to(x, y);
            }
        }
        if let Some(p) = pb.finish() {
            self.pm.stroke_path(&p, &paint(rgb, a), &stroke(lw * s), Transform::identity(), None);
        }
    }

    pub fn poly(&mut self, pts: &[(f32, f32)], lw: f32, rgb: u32, a: u8, close: bool, glow: bool) {
        let s = self.scale;
        let mut pb = PathBuilder::new();
        for (i, (x, y)) in pts.iter().enumerate() {
            if i == 0 {
                pb.move_to(x * s, y * s);
            } else {
                pb.line_to(x * s, y * s);
            }
        }
        if close {
            pb.close();
        }
        if let Some(p) = pb.finish() {
            if glow {
                self.pm.stroke_path(&p, &paint(rgb, 32), &stroke(lw * s * 2.6), Transform::identity(), None);
            }
            self.pm.stroke_path(&p, &paint(rgb, a), &stroke(lw * s), Transform::identity(), None);
        }
    }

    /// Text width in *design* units for a given font/size.
    pub fn text_w(&self, font: &FontRef, size: f32, s: &str) -> f32 {
        let sf = font.as_scaled(PxScale::from(size * self.scale));
        let adv: f32 = s.chars().map(|c| sf.h_advance(font.glyph_id(c))).sum();
        adv / self.scale
    }

    /// Draw text using glyph outlines. Returns the advance width (design units).
    pub fn text(&mut self, font: &FontRef, size: f32, x: f32, y: f32, rgb: u32, a: u8, s: &str) -> f32 {
        let scale = self.scale;
        let dsize = size * scale;
        let sf = font.as_scaled(PxScale::from(dsize));
        let x0 = x * scale;
        let mut caret = x0;
        let yd = y * scale;
        let mut path = PathBuilder::new();
        let mut any = false;
        // `Outline.curves` comes back in *unscaled* font units, so scale to
        // pixels exactly the way ab_glyph's own rasterizer does (y flips).
        let sfc = sf.scale_factor();
        let hf = sfc.horizontal;
        let vf = -sfc.vertical;
        for ch in s.chars() {
            let gid = font.glyph_id(ch);
            let adv = sf.h_advance(gid);
            let pos = ab_glyph::point(caret, yd);
            let tx = |p: ab_glyph::Point| (p.x * hf + pos.x, p.y * vf + pos.y);
            if let Some(ol) = font.outline(gid) {
                let mut cur: Option<(f32, f32)> = None;
                for c in ol.curves.iter() {
                    let (a, b, cc, d) = match c {
                        OutlineCurve::Line(p0, p1) => (tx(*p0), tx(*p1), tx(*p1), tx(*p1)),
                        OutlineCurve::Quad(p0, p1, p2) => (tx(*p0), tx(*p1), tx(*p2), tx(*p2)),
                        OutlineCurve::Cubic(p0, p1, p2, p3) => (tx(*p0), tx(*p1), tx(*p2), tx(*p3)),
                    };
                    let cont = matches!(cur, Some((px, py)) if (px - a.0).abs() < 0.05 && (py - a.1).abs() < 0.05);
                    if !cont {
                        path.move_to(a.0, a.1);
                    }
                    match c {
                        OutlineCurve::Line(..) => path.line_to(b.0, b.1),
                        OutlineCurve::Quad(..) => path.quad_to(b.0, b.1, cc.0, cc.1),
                        OutlineCurve::Cubic(..) => path.cubic_to(b.0, b.1, cc.0, cc.1, d.0, d.1),
                    }
                    cur = Some(d);
                    any = true;
                }
            }
            caret += adv;
        }
        if any {
            if let Some(p) = path.finish() {
                self.pm.fill_path(&p, &paint(rgb, a), FillRule::Winding, Transform::identity(), None);
            }
        }
        (caret - x0) / scale
    }

    /// Glowing text: two offset halo passes then a bright core. (Six passes
    /// looked marginally softer but cost twice as much on a 1-core box.)
    pub fn glow_text(&mut self, font: &FontRef, size: f32, x: f32, y: f32, rgb: u32, s: &str) -> f32 {
        let d = 1.6 / self.scale; // ~1.6 device px in design units
        self.text(font, size, x + d, y + d, rgb, 62, s);
        self.text(font, size, x - d, y - d, rgb, 62, s);
        self.text(font, size, x, y, rgb, 255, s)
    }

    /// Centre text horizontally at x (design units).
    pub fn text_c(&mut self, font: &FontRef, size: f32, cx: f32, y: f32, rgb: u32, a: u8, s: &str) {
        let w = self.text_w(font, size, s);
        self.text(font, size, cx - w / 2.0, y, rgb, a, s);
    }

    pub fn glow_text_c(&mut self, font: &FontRef, size: f32, cx: f32, y: f32, rgb: u32, s: &str) {
        let w = self.text_w(font, size, s);
        self.glow_text(font, size, cx - w / 2.0, y, rgb, s);
    }

    // ---------------- CRT post effects (device space) ----------------

    // ---------------- CRT post effects (device space) ----------------
    //
    // Scanlines, vignette and flicker are all "darken this pixel by X". Drawing
    // them as ~360 one-pixel vector fills plus a per-pixel radial-gradient
    // shader cost more than all five screens' content put together, and every
    // frame they produced the identical result. So the static part is baked
    // into one byte-per-pixel mask at startup, and the per-frame cost collapses
    // to a single linear pass. The pass is a "multiply by this/255": each piece
    // is folded with the exact-division identity in `mul255` (below), which is
    // branchless shifter math that LLVM auto-vectorises - no gather table.

    /// Build the static mask: scanlines x vignette, "multiply by this/255".
    fn prep_crt(&mut self) {
        if !self.keep.is_empty() {
            return;
        }
        let (dw, dh) = (self.pm.width(), self.pm.height());
        let (dwu, dhu) = (dw as usize, dh as usize);

        // scanline coverage per device row, replicating 1px fills at y = k*step
        // (partial overlaps accumulate, exactly as the vector version did)
        let step = SCAN_STEP * self.scale;
        let mut row = vec![0f32; dhu];
        let mut y0 = 0.0f32;
        while y0 < dh as f32 {
            let (lo, hi) = (y0, y0 + 1.0);
            let mut r = lo.floor() as usize;
            let rmax = (hi.floor() as usize).min(dhu - 1);
            while r <= rmax {
                let ov = (hi.min(r as f32 + 1.0) - lo.max(r as f32)).max(0.0);
                row[r] = (row[r] + ov).min(1.0);
                r += 1;
            }
            y0 += step;
        }

        let (cx, cy) = (dwu as f32 / 2.0, dhu as f32 / 2.0);
        let rad = dw.max(dh) as f32 * 0.72;
        let inner = 0.45 * rad;

        let mut keep = Vec::with_capacity(dwu * dhu);
        for (y, acc) in row.iter().enumerate() {
            let s = SCAN_STRENGTH * acc;
            let dy = y as f32 - cy;
            for x in 0..dwu {
                let dx = x as f32 - cx;
                let d = (dx * dx + dy * dy).sqrt().min(rad);
                let tt = ((d - inner) / (rad - inner)).clamp(0.0, 1.0);
                let v = VIG_STRENGTH * tt;
                // both passes darken, so the survivors multiply
                keep.push((((255.0 - s) * (255.0 - v) / 255.0).round()).clamp(0.0, 255.0) as u8);
            }
        }
        self.keep = keep;
    }

    /// Scanlines + vignette + flicker in one pass.
    ///
    /// Branchless by design. Every pixel gets the *same* folding: fold the
    /// static mask by the flicker wobble, then darken each channel by that.
    /// There is deliberately no `if k == 255 { continue }` shortcut: a
    /// data-dependent branch forces LLVM to scalarise the loop (and dropping it
    /// costs nothing, because the k==255 "skip" is exactly what the arithmetic
    /// does anyway - (p * 255 / 255) == p). The result is a straight load /
    /// widen / multiply / narrow stream that compiles to one tight SIMD pass.
    fn apply_darken(&mut self, flicker: u8) {
        // flicker is global, so it is folded into the mask as a second multiply
        let fm = 255u16 - flicker as u16;
        let keep = &self.keep;
        let data = self.pm.data_mut();
        // iterate over the mask bytes (`needless_range_loop` would flag a
        // `for i in 0..n` that indexes `keep`), while `base = i*4` lets the
        // 3-of-4-byte RGBA stride stay visible to the vectoriser.
        for (i, &mk) in keep.iter().enumerate() {
            let base = i * 4;
            let k = mul255(mk, fm as u8);
            data[base] = mul255(data[base], k);
            data[base + 1] = mul255(data[base + 1], k);
            data[base + 2] = mul255(data[base + 2], k);
        }
    }

    /// Tube grain, written straight into the buffer (2x2 speckles, as before).
    fn apply_grain(&mut self, n: usize, rgb: u32, a: u8) {
        let (dw, dh) = (self.pm.width(), self.pm.height());
        let (cr, cg, cb) = ((rgb >> 16) as u8, ((rgb >> 8) & 0xff) as u8, (rgb & 0xff) as u8);
        // positions first: rnd() needs &mut self and the pixel writes need
        // &mut self.pm, so the two borrows cannot overlap
        let mut pts = Vec::with_capacity(n);
        for _ in 0..n {
            pts.push((
                (self.rnd() * dw as f32) as u32,
                (self.rnd() * dh as f32) as u32,
            ));
        }
        let keep_a = 255u8 - a; // keep = blend this much of the existing pixel
        let data = self.pm.data_mut();
        for (x, y) in pts {
            for (ox, oy) in [(0u32, 0u32), (1, 0), (0, 1), (1, 1)] {
                let (xx, yy) = (x + ox, y + oy);
                if xx >= dw || yy >= dh {
                    continue;
                }
                let i = ((yy * dw + xx) as usize) * 4;
                data[i] = mul255(data[i], keep_a).saturating_add(mul255(cr, a));
                data[i + 1] = mul255(data[i + 1], keep_a).saturating_add(mul255(cg, a));
                data[i + 2] = mul255(data[i + 2], keep_a).saturating_add(mul255(cb, a));
            }
        }
    }

    pub fn png(&self) -> Vec<u8> {
        self.pm.encode_png().expect("png encode")
    }

    /// The CRT finish, applied once per frame to the whole image by
    /// `screens::render` - after the content and the status plate, so the plate
    /// sits *under* the glass rather than pasted on top of it.
    pub fn crt_finish(&mut self, t: f32) {
        fn on(k: &str) -> bool {
            std::env::var_os(k).is_some()
        }
        static GATES: std::sync::OnceLock<(bool, bool, bool)> = std::sync::OnceLock::new();
        let (off, no_darken, no_grain) =
            *GATES.get_or_init(|| (on("DEFCONMON_NO_CRT"), on("DEFCONMON_NO_DARKEN"), on("DEFCONMON_NO_GRAIN")));
        if off {
            return;
        }
        self.prep_crt();
        let f = (t * 11.0).sin() * 0.5 + (t * 27.3).sin() * 0.5;
        let flick = (FLICKER_STRENGTH * (0.5 + 0.5 * f)) as u8;
        if !no_grain {
            self.apply_grain(GRAIN_COUNT, 0x33ff66, GRAIN_ALPHA);
        }
        if !no_darken {
            self.apply_darken(flick);
        }
    }

}

/// Exact `(a * b) / 255`, truncated, for two 8-bit factors.
///
/// `a*b <= 255*255 = 65025`, and for every t < 65535 the shifter identity
/// `((t + 1 + (t >> 8)) >> 8) == t / 255` holds exactly. Written this way - a
/// multiply plus two shifts and an add - it is expression-shape pure code with
/// no table and no division, so LLVM can vectorise the loops that call it
/// (a per-pixel gather table could not be). Kept `#[inline(always)]` so it
/// never forms a call barrier between the loads and the stores.
#[inline(always)]
fn mul255(a: u8, b: u8) -> u8 {
    let t = (a as u16) * (b as u16);
    ((t + 1 + (t >> 8)) >> 8) as u8
}

fn stroke(lw: f32) -> Stroke {
    Stroke { width: lw, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() }
}

fn line_path(x0: f32, y0: f32, x1: f32, y1: f32) -> Option<Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(x0, y0);
    pb.line_to(x1, y1);
    pb.finish()
}
