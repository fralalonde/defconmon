//! Embedded fonts. The retro set: IBM 3270 (thin technical) for all text and
//! headings, DSEG7 (segmented) for the numeric readouts. Px VGA SquarePx and
//! Terminus were dropped because both clash chronologically with IBM 3270's
//! 1980s look.
use ab_glyph::FontRef;

pub const F_3270: &[u8] = include_bytes!("../assets/fonts/3270-Regular.ttf");
pub const F_DSEG7: &[u8] = include_bytes!("../assets/fonts/DSEG7Classic-Regular.ttf");

pub struct Fonts {
    pub a3270: FontRef<'static>,
    pub dseg7: FontRef<'static>,
}

impl Fonts {
    pub fn load() -> Self {
        Self {
            a3270: FontRef::try_from_slice(F_3270).expect("3270 font"),
            dseg7: FontRef::try_from_slice(F_DSEG7).expect("dseg7 font"),
        }
    }
}