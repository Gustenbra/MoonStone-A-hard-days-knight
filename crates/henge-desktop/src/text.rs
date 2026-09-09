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
//! gone and so is the `blit_mask` under it. **So is the translation that
//! replaced it**, which sent each of a glyph's indices to the nearest colour in
//! whatever palette was loaded: the original has nothing of the kind. `TextP`
//! at image 0x7aee is, in full,
//!
//! ```text
//! 07af4  lodsb; or al, al; jne / jmp TextPDone      ; the NUL ends the line
//! 07b00  sub al, 0x20; mov di, 0x8006; add di, ax   ; TextASCII[c - 0x20]
//! 07b09  mov al, [di]                               ; the cel number
//! 07b0b  les si, [0x8981]                           ; the current font bank
//! 07b21  mov dx, es:[bx+0xe]; xchg; mov [textwidth], dx    ; the cel's width
//! 07b2b  mov dx, es:[bx+0x10]; xchg; mov [texthieght], dx  ; and height
//! 07b37  mov bx, [TextX]; mov cx, [TextY]
//! 07b47  test byte ptr [bp+6], 8; je / sub word ptr [textwidth], 3
//! 07b53  call 0x5d7f                                ; the cel blit
//! 07b56  add bx, [textwidth]; mov [TextX], bx       ; advance
//! ```
//!
//! and 0x5d7f is the blit every other cel in the game goes through, which
//! writes the glyph's indices and nothing else. Nothing in `MAIN.EXE` writes
//! the bold face's five entries either: the only immediates `0xfed`, `0xdc9`,
//! `0xb95` and `0x842` in the code are creature colours (`ColourBalok+14`).
//! So a line means whatever the loaded palette says its indices mean, and the
//! original chooses where it writes: the bold face over `MESSAGE.PIV` and
//! `CH.PIV`, which reserve those five, and `SMALL.FON`, which is drawn in
//! index 1 alone, over the map, where index 1 is white. The place-entry
//! dispatch at 0xc7f sets the small face (`mov ax, [0x8903+2]; mov [0x8981],
//! ax` is `SMALL.FON`'s slot) before it jumps to any place; the three message
//! routines, the title and the between-days screen set the bold one.

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

    /// Draw a line in the glyphs' own indices, which is the only way there is:
    /// `TextP` puts each cel through the ordinary blit at 0x5d7f.
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

    fn render(&self, reg: &mut Registry, fb: &mut Framebuffer, s: &str, x: i32, y: i32) -> i32 {
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
            fb.blit(&px, w, h, cx, y, false);
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
