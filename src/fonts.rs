//! Embedded fonts. The retro set: IBM 3270 (thin technical), DSEG7 (segmented
//! numerics), Px VGA SquarePx (chunky small text), Terminus (fallback).
use ab_glyph::FontRef;

pub const F_3270: &[u8] = include_bytes!("../assets/fonts/3270-Regular.ttf");
pub const F_DSEG7: &[u8] = include_bytes!("../assets/fonts/DSEG7Classic-Regular.ttf");
pub const F_VGA: &[u8] = include_bytes!("../assets/fonts/Px_VGA_SquarePx.ttf");
pub const F_TERM: &[u8] = include_bytes!("../assets/fonts/TerminusTTF-4.46.0.ttf");

pub struct Fonts {
    pub a3270: FontRef<'static>,
    pub dseg7: FontRef<'static>,
    pub vga: FontRef<'static>,
    pub term: FontRef<'static>,
}

impl Fonts {
    pub fn load() -> Self {
        Self {
            a3270: FontRef::try_from_slice(F_3270).expect("3270 font"),
            dseg7: FontRef::try_from_slice(F_DSEG7).expect("dseg7 font"),
            vga: FontRef::try_from_slice(F_VGA).expect("vga font"),
            term: FontRef::try_from_slice(F_TERM).expect("terminus font"),
        }
    }
}
