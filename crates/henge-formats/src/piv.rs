//! PIV and CMP: a full-screen 320x200 planar image with its own palette.
//!
//! ```text
//! u16 kind            4 = four bitplanes (16 colours), 5 = five (32 colours)
//! u16 _
//! u16 packed_len
//! u16 palette[16|32]  Amiga 0x0RGB, four bits per channel
//! u8  packed[]        unpacks to 8000 bytes per plane, planes stored consecutively
//! ```
//!
//! CMP files are PIVs used as tile sheets; the game stamps 32x25 cells out of them.

use crate::depack;

pub const W: usize = 320;
pub const H: usize = 200;
pub const CELL_W: usize = 32;
pub const CELL_H: usize = 25;

#[derive(Clone)]
pub struct Piv {
    pub kind: u16,
    pub planes: usize,
    /// 0xRRGGBB per entry. Index 0 is the background/transparent colour.
    pub palette: Vec<u32>,
    /// W * H palette indices.
    pub pixels: Vec<u8>,
}

impl Piv {
    pub fn parse(d: &[u8]) -> anyhow::Result<Piv> {
        anyhow::ensure!(d.len() > 8, "file too short to be a PIV");
        let kind = u16::from_be_bytes([d[0], d[1]]);
        let packed_len = u16::from_be_bytes([d[4], d[5]]) as usize;
        let (planes, n_colours) = match kind {
            5 => (5usize, 32usize),
            4 => (4, 16),
            other => anyhow::bail!("not a PIV: kind {other}"),
        };

        let mut palette = Vec::with_capacity(n_colours);
        for i in 0..n_colours {
            let o = 6 + i * 2;
            let w = u16::from_be_bytes([d[o], d[o + 1]]) & 0x7fff;
            let (r, g, b) = ((w >> 8) & 0xf, (w >> 4) & 0xf, w & 0xf);
            // four bits per channel widened to eight: 0x0..0xf -> 0x00..0xff
            palette.push(((r as u32 * 17) << 16) | ((g as u32 * 17) << 8) | (b as u32 * 17));
        }

        let data_start = 6 + n_colours * 2;
        let raw = depack::fit(depack::unpack(d, data_start, packed_len), 8000 * planes);
        Ok(Piv { kind, planes, palette, pixels: deplanar(&raw, planes) })
    }

    /// One cell of a CMP tile sheet.
    pub fn cell(&self, n: usize) -> Sprite {
        let per_row = W / CELL_W;
        let (sx, sy) = ((n % per_row) * CELL_W, (n / per_row) * CELL_H);
        let mut px = vec![0u8; CELL_W * CELL_H];
        for y in 0..CELL_H {
            let src = (sy + y) * W + sx;
            if src + CELL_W <= self.pixels.len() {
                px[y * CELL_W..(y + 1) * CELL_W].copy_from_slice(&self.pixels[src..src + CELL_W]);
            }
        }
        Sprite { width: CELL_W, height: CELL_H, real_width: CELL_W, pixels: px }
    }
}

/// Bitplanes are stored one after another. For each byte position, the same bit
/// of each plane contributes one bit of the pixel index, plane 0 lowest.
fn deplanar(raw: &[u8], planes: usize) -> Vec<u8> {
    let mem_width = 40 * H;
    let mut out = vec![0u8; W * H];
    let mut o = 0;
    for i in 0..mem_width {
        let mut b = [0u8; 5];
        for (p, slot) in b.iter_mut().enumerate().take(planes) {
            *slot = raw.get(p * mem_width + i).copied().unwrap_or(0);
        }
        for bit in (0..8).rev() {
            let mut v = 0u8;
            for p in 0..planes {
                v |= ((b[p] >> bit) & 1) << p;
            }
            out[o] = v;
            o += 1;
        }
    }
    out
}

/// One decoded image. Colour index 0 is transparent.
#[derive(Clone)]
pub struct Sprite {
    /// Storage width, padded up to a multiple of eight.
    pub width: usize,
    pub height: usize,
    /// The width the game actually draws.
    pub real_width: usize,
    pub pixels: Vec<u8>,
}
