//! A palette-indexed 320x200 framebuffer, the same shape the original drew into.
//!
//! Working in palette indices rather than RGB is not nostalgia. It keeps colour
//! cycling, palette fades and per-area recolouring cheap, all of which the genre
//! leans on heavily. The fades and the cycling themselves live in
//! `henge_assets::palette`, because they are arithmetic on a palette and have
//! nothing to do with pixels; this holds the base palette a screen drew with,
//! and the composed one is what reaches the window.

use henge_core::{SCREEN_H, SCREEN_W};

pub struct Framebuffer {
    pub pixels: Vec<u8>,
    pub palette: [u32; 32],
}

impl Default for Framebuffer {
    fn default() -> Self {
        Framebuffer { pixels: vec![0; SCREEN_W * SCREEN_H], palette: [0; 32] }
    }
}

impl Framebuffer {
    pub fn set_palette(&mut self, p: &[u32]) {
        self.palette = [0; 32];
        for (i, c) in p.iter().take(32).enumerate() {
            self.palette[i] = *c;
        }
    }

    pub fn clear(&mut self, index: u8) {
        self.pixels.fill(index);
    }

    /// Draws an indexed sprite, treating index 0 as transparent.
    /// Draws a sprite through a colour substitution table, which is how four
    /// knights in identical armour are told apart without adding a colour.
    pub fn blit_lut(&mut self, src: &[u8], sw: usize, sh: usize, x: i32, y: i32,
                    flip: bool, lut: &[u8; 32]) {
        for sy in 0..sh {
            let dy = y + sy as i32;
            if dy < 0 || dy >= SCREEN_H as i32 {
                continue;
            }
            let row = sy * sw;
            let drow = dy as usize * SCREEN_W;
            for sx in 0..sw {
                let dx = if flip { x + (sw - 1 - sx) as i32 } else { x + sx as i32 };
                if dx < 0 || dx >= SCREEN_W as i32 {
                    continue;
                }
                let v = src[row + sx];
                if v != 0 {
                    self.pixels[drow + dx as usize] = lut[(v & 0x1f) as usize];
                }
            }
        }
    }

    /// Draws a sprite as a flat silhouette in one colour.
    ///
    /// Text is drawn this way rather than in its own colours: arena palettes
    /// differ, so a glyph's own shades are legible over one backdrop and invisible
    /// over the next. A silhouette in a chosen colour reads everywhere.
    pub fn blit_mask(&mut self, src: &[u8], sw: usize, sh: usize, x: i32, y: i32, colour: u8) {
        for sy in 0..sh {
            let dy = y + sy as i32;
            if dy < 0 || dy >= SCREEN_H as i32 {
                continue;
            }
            for sx in 0..sw {
                let dx = x + sx as i32;
                if dx < 0 || dx >= SCREEN_W as i32 {
                    continue;
                }
                if src[sy * sw + sx] != 0 {
                    self.pixels[dy as usize * SCREEN_W + dx as usize] = colour;
                }
            }
        }
    }

    pub fn blit(&mut self, src: &[u8], sw: usize, sh: usize, x: i32, y: i32, flip: bool) {
        for sy in 0..sh {
            let dy = y + sy as i32;
            if dy < 0 || dy >= SCREEN_H as i32 {
                continue;
            }
            let row = sy * sw;
            let drow = dy as usize * SCREEN_W;
            for sx in 0..sw {
                let dx = if flip { x + (sw - 1 - sx) as i32 } else { x + sx as i32 };
                if dx < 0 || dx >= SCREEN_W as i32 {
                    continue;
                }
                let v = src[row + sx];
                if v != 0 {
                    self.pixels[drow + dx as usize] = v;
                }
            }
        }
    }

    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, index: u8) {
        for yy in y..y + h {
            if yy < 0 || yy >= SCREEN_H as i32 { continue; }
            for xx in x..x + w {
                if xx < 0 || xx >= SCREEN_W as i32 { continue; }
                self.pixels[yy as usize * SCREEN_W + xx as usize] = index;
            }
        }
    }

    /// Expands to 0RGB for presentation, letterboxed and scaled by the caller.
    /// Where a window pixel lands on the 320x200 screen, or nothing when it is
    /// in the letterbox. The inverse of [`Framebuffer::present_into`]'s
    /// arithmetic, kept beside it so the two cannot drift: a pointer that
    /// disagrees with the picture by a few pixels is worse than no pointer.
    pub fn to_screen(dw: usize, dh: usize, px: f64, py: f64) -> Option<(i32, i32)> {
        if dw == 0 || dh == 0 {
            return None;
        }
        let target = 4.0 / 3.0;
        let (mut vw, mut vh) = (dw, (dw as f32 / target) as usize);
        if vh > dh {
            vh = dh;
            vw = (dh as f32 * target) as usize;
        }
        let (ox, oy) = ((dw - vw) / 2, (dh - vh) / 2);
        let (x, y) = (px as i64 - ox as i64, py as i64 - oy as i64);
        if x < 0 || y < 0 || x >= vw as i64 || y >= vh as i64 {
            return None;
        }
        Some(((x as usize * SCREEN_W / vw) as i32, (y as usize * SCREEN_H / vh) as i32))
    }

    pub fn present_into(&self, dst: &mut [u32], dw: usize, dh: usize, palette: &[u32; 32]) {
        if dw == 0 || dh == 0 {
            return;
        }
        // The original was shown on a 4:3 display, so 320x200 pixels were not
        // square. Presenting at 4:3 rather than 8:5 is what makes it look right.
        let target = 4.0 / 3.0;
        let (mut vw, mut vh) = (dw, (dw as f32 / target) as usize);
        if vh > dh {
            vh = dh;
            vw = (dh as f32 * target) as usize;
        }
        let (ox, oy) = ((dw - vw) / 2, (dh - vh) / 2);
        dst.fill(0);
        for y in 0..vh {
            let sy = y * SCREEN_H / vh;
            let srow = sy * SCREEN_W;
            let drow = (oy + y) * dw + ox;
            for x in 0..vw {
                let sx = x * SCREEN_W / vw;
                dst[drow + x] = palette[(self.pixels[srow + sx] & 0x1f) as usize];
            }
        }
    }
}
