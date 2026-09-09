//! Drawing words.
//!
//! Which glyph draws which character was read off the artwork first and has
//! since been found in the executable: `GFX:TextASCII` at image 107,446 is 95
//! bytes indexed by `character - 32`, and it agrees entry for entry. The
//! mapping still lives in the pack as content, so a replacement font is a data
//! change, and the space is glyph 69 drawn like any other character, which is
//! why `space_width` is that glyph's own advance rather than a number.
//!
//! **The original never draws text in a colour, and neither does this any
//! more.** `GFX:TextP` looks a glyph up, puts its width and height in the
//! blitter's registers and calls the same cel blit every other sprite in the
//! game goes through; there is no ink anywhere in it. `BOLD.F`'s glyphs are
//! drawn in five indices, 5 the ring round the letter and 9 to 12 the bright
//! face inside it, and flattening all five to one colour closes every counter
//! and turns a line into a row of blobs.
//!
//! `Font::draw`, which took an ink and painted the glyph as a silhouette, is
//! gone and so is the `blit_mask` under it. What stood in its way was that the
//! five entries mean something else over an arena, and the fix is the one
//! `docs/ROADMAP.md` already named: **the manifest says which palette a sheet's
//! indices were authored against**, so the glyph's own shades can be translated
//! into whatever the screen loaded, by nearest colour. Over `MESSAGE.PIV`,
//! `CH.PIV` and the intro's panorama, which are the screens the original writes
//! on, the two palettes agree and the translation is the identity.

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

    /// Draw a line in the glyphs' own colours, which is the only way there is.
    ///
    /// Where the screen's palette is not the one the font bank was authored
    /// against, each of the glyph's indices is sent to the nearest colour in
    /// the palette that *is* loaded. That keeps the letter shapes, which a
    /// silhouette destroys.
    pub fn draw_own(
        &self,
        reg: &mut Registry,
        fb: &mut Framebuffer,
        s: &str,
        x: i32,
        y: i32,
    ) -> i32 {
        self.render(reg, fb, s, x, y)
    }

    /// The table that carries this sheet's indices into the loaded palette.
    ///
    /// The identity where the manifest names no palette for the sheet, and the
    /// identity again where the two palettes are the same, which is every
    /// screen the original itself writes on.
    fn translation(&self, reg: &Registry, fb: &Framebuffer) -> [u8; 32] {
        let mut map = [0u8; 32];
        for (i, m) in map.iter_mut().enumerate() {
            *m = i as u8;
        }
        let Some(id) = reg.sheet(&self.sheet).and_then(|r| r.value.palette.clone()) else {
            return map;
        };
        let Some(own) = reg.palette(&id).map(|r| r.value.clone()) else {
            return map;
        };
        let same = own
            .iter()
            .take(32)
            .enumerate()
            .all(|(i, c)| fb.palette[i] == *c);
        if same {
            return map;
        }
        let split = |c: u32| -> [i32; 3] {
            [
                ((c >> 16) & 0xff) as i32,
                ((c >> 8) & 0xff) as i32,
                (c & 0xff) as i32,
            ]
        };
        // Nearest by squared distance, weighted the way luminance is: a glyph
        // that has to move ends up on the entry a viewer would call the same
        // colour rather than the one with the smallest arithmetic difference.
        for (i, want) in own.iter().take(32).enumerate() {
            let [wr, wg, wb] = split(*want);
            let mut best = (i32::MAX, i as u8);
            for (j, have) in fb.palette.iter().enumerate() {
                let [hr, hg, hb] = split(*have);
                let d =
                    2 * (wr - hr) * (wr - hr) + 4 * (wg - hg) * (wg - hg) + (wb - hb) * (wb - hb);
                if d < best.0 {
                    best = (d, j as u8);
                }
            }
            map[i] = best.1;
        }
        map
    }

    fn render(&self, reg: &mut Registry, fb: &mut Framebuffer, s: &str, x: i32, y: i32) -> i32 {
        let map = self.translation(reg, fb);
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
            fb.blit_mapped(&px, w, h, cx, y, &map);
            cx += w as i32 + self.tracking;
        }
        cx - x
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
