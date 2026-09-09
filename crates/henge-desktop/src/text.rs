//! Drawing words.
//!
//! Which glyph draws which character was read off the artwork first and has
//! since been found in the executable: `GFX:TextASCII` at image 107,446 is 95
//! bytes indexed by `character - 32`, and it agrees entry for entry. The
//! mapping still lives in the pack as content, so a replacement font is a data
//! change, and the space is glyph 69 drawn like any other character, which is
//! why `space_width` is that glyph's own advance rather than a number.
//!
//! **The original never draws text in a colour.** `GFX:TextP` looks a glyph up,
//! puts its width and height in the blitter's registers and calls the same cel
//! blit every other sprite in the game goes through; there is no ink anywhere in
//! it. So [`Font::draw_own`] is the original's way and [`Font::draw`] is this
//! project's, for the screens whose palette is an arena's rather than one of the
//! three plates that reserve the face's five entries.

use crate::framebuffer::Framebuffer;
use henge_assets::Registry;
use henge_core::content::FontDef;
use std::collections::BTreeMap;

pub struct Font {
    sheet: String,
    /// Character to glyph index. Sparse on purpose: a character the font has no
    /// glyph for is skipped rather than drawn as something wrong.
    glyph: BTreeMap<char, usize>,
    space_width: i32,
    tracking: i32,
    /// Carried from the pack for a caller that wants to stack lines itself.
    /// Nothing here does: every screen in the game places its lines at the y
    /// the original's own text records give them.
    #[allow(dead_code)]
    pub line_height: i32,
}

impl Font {
    pub fn new(def: &FontDef) -> Font {
        let mut glyph: BTreeMap<char, usize> = def
            .glyphs
            .chars()
            .enumerate()
            .map(|(i, c)| (c, i))
            .collect();
        // `TextASCII` maps characters to glyphs and `FontDef::glyphs` is its
        // inverse, so the two characters the table sends to a glyph another
        // character already names cannot be written in it. Both are added here:
        // 0x5f, the underscore, shares glyph 69 with the space, and 0x5c, the
        // backslash, shares glyph 71 with the slash.
        //
        // The underscore is not a curiosity. `ASCIIT[0x39]` is 0x5f, so the
        // space bar types an underscore, which is why the four default knight
        // names are stored with one. The backslash is `CURSOR`, the caret the
        // name typing writes into the buffer.
        for (alias, same_as) in [('_', ' '), ('\\', '/')] {
            if let Some(g) = glyph.get(&same_as).copied() {
                glyph.insert(alias, g);
            }
        }
        Font {
            sheet: def.sheet.clone(),
            glyph,
            space_width: def.space_width,
            tracking: def.tracking,
            line_height: def.line_height,
        }
    }

    fn advance(&self, reg: &Registry, c: char) -> i32 {
        if c == ' ' {
            return self.space_width;
        }
        let Some(g) = self.glyph.get(&c) else {
            return 0;
        };
        reg.sheet(&self.sheet)
            .and_then(|r| r.value.frames.get(*g).map(|f| f.w as i32 + self.tracking))
            .unwrap_or(0)
    }

    pub fn width(&self, reg: &Registry, s: &str) -> i32 {
        s.chars().map(|c| self.advance(reg, c)).sum()
    }

    pub fn draw(
        &self,
        reg: &mut Registry,
        fb: &mut Framebuffer,
        s: &str,
        x: i32,
        y: i32,
        colour: u8,
    ) -> i32 {
        self.render(reg, fb, s, x, y, Some(colour))
    }

    /// Draw a line in the glyphs' own colours rather than as a silhouette.
    ///
    /// `BOLD.F`'s glyphs carry five indices: 5 rings the letter and fills its
    /// counters, and 9 to 12 are the bright face inside that ring. A
    /// silhouette paints both in one colour, which closes every counter and
    /// turns the line into a row of blobs. Blitting the indices keeps the
    /// letter shapes, and it is legible wherever the screen's palette carries
    /// the font's own five entries: `MESSAGE.PIV` has them already, and
    /// `henge_core::intro::CAPTION_INK` is the intro writing them itself.
    pub fn draw_own(
        &self,
        reg: &mut Registry,
        fb: &mut Framebuffer,
        s: &str,
        x: i32,
        y: i32,
    ) -> i32 {
        self.render(reg, fb, s, x, y, None)
    }

    fn render(
        &self,
        reg: &mut Registry,
        fb: &mut Framebuffer,
        s: &str,
        x: i32,
        y: i32,
        colour: Option<u8>,
    ) -> i32 {
        let mut cx = x;
        for c in s.chars() {
            if c == ' ' {
                cx += self.space_width;
                continue;
            }
            let Some(g) = self.glyph.get(&c).copied() else {
                continue;
            };
            let Some(rect) = reg
                .sheet(&self.sheet)
                .and_then(|r| r.value.frames.get(g).copied())
            else {
                continue;
            };
            let Ok(img) = reg.image(&self.sheet) else {
                continue;
            };

            let (w, h) = (rect.w as usize, rect.h as usize);
            let mut px = vec![0u8; w * h];
            for row in 0..h {
                let src = (rect.y as usize + row) * img.width + rect.x as usize;
                if src + w <= img.pixels.len() {
                    px[row * w..(row + 1) * w].copy_from_slice(&img.pixels[src..src + w]);
                }
            }
            match colour {
                Some(ink) => fb.blit_mask(&px, w, h, cx, y, ink),
                None => fb.blit(&px, w, h, cx, y, false),
            }
            cx += w as i32 + self.tracking;
        }
        cx - x
    }

    pub fn draw_centred(
        &self,
        reg: &mut Registry,
        fb: &mut Framebuffer,
        s: &str,
        y: i32,
        colour: u8,
    ) {
        let w = self.width(reg, s);
        self.draw(reg, fb, s, (henge_core::SCREEN_W as i32 - w) / 2, y, colour);
    }

    /// [`Font::draw_own`], centred the way `TextPTop` centres a line whose
    /// record sets bit 0 of its flag word.
    pub fn draw_own_centred(&self, reg: &mut Registry, fb: &mut Framebuffer, s: &str, y: i32) {
        let w = self.width(reg, s);
        self.draw_own(reg, fb, s, (henge_core::SCREEN_W as i32 - w) / 2, y);
    }
}

/// Loads whatever fonts the packs provide. A pack without fonts is not an error;
/// the game simply says nothing, as it did before.
pub fn load(reg: &Registry) -> BTreeMap<String, Font> {
    let defs: henge_core::content::Fonts = match reg.read_data("data.fonts") {
        Ok(d) => d,
        Err(_) => return BTreeMap::new(),
    };
    defs.iter()
        .map(|(k, d)| (k.clone(), Font::new(d)))
        .collect()
}
