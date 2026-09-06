//! A palette-indexed 320x200 framebuffer, the same shape the original drew into.
//!
//! Working in palette indices rather than RGB is not nostalgia. It keeps colour
//! cycling, palette fades and per-area recolouring cheap, all of which the genre
//! leans on heavily.

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

    /// Scales every palette entry, for fades in and out.
    pub fn faded_palette(&self, level: u8) -> [u32; 32] {
        let mut out = [0u32; 32];
        for (i, c) in self.palette.iter().enumerate() {
            let r = ((c >> 16) & 0xff) * level as u32 / 255;
            let g = ((c >> 8) & 0xff) * level as u32 / 255;
            let b = (c & 0xff) * level as u32 / 255;
            out[i] = (r << 16) | (g << 8) | b;
        }
        out
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
