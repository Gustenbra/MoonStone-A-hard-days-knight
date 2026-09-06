//! CEL, OB, F and FON: a bank of sprites sharing one packed blob.
//!
//! ```text
//! u16 image_count
//! u16 _
//! u16 packed_len
//! entry[image_count]     ten bytes each:
//!     u16 pad
//!     u16 data_offset    into the unpacked blob
//!     u16 width
//!     u16 height
//!     u8  plane_count
//!     u8  blit_mask      colour mask; its bit length is how many planes are stored
//! u8  packed[]
//! ```
//!
//! Row stride is `((width + 15) / 16) * 2` bytes, so decoded width rounds up to a
//! multiple of eight. `blit_mask == 32` is a special case: two planes, with bit 4
//! of the output set wherever either plane bit is set. That is the game's cheap
//! outline blit, used for shadows and silhouettes.

use crate::depack;
use crate::piv::Sprite;

pub struct Cel {
    pub images: Vec<Sprite>,
    pub blit_mask: Vec<u8>,
}

impl Cel {
    pub fn parse(d: &[u8]) -> anyhow::Result<Cel> {
        anyhow::ensure!(d.len() > 10, "file too short to be a CEL");
        let count = u16::from_be_bytes([d[0], d[1]]) as usize;
        let header_len = count * 10 + 10;
        anyhow::ensure!(d.len() > header_len, "CEL header overruns the file");
        let packed_len = u16::from_be_bytes([d[4], d[5]]) as usize;
        let data = depack::unpack(d, header_len, packed_len);

        let mut images = Vec::with_capacity(count);
        let mut blit_mask = Vec::with_capacity(count);
        for i in 0..count {
            let b = 10 + i * 10;
            let addr = u16::from_be_bytes([d[b + 2], d[b + 3]]) as usize;
            let w = u16::from_be_bytes([d[b + 4], d[b + 5]]) as usize;
            let h = u16::from_be_bytes([d[b + 6], d[b + 7]]) as usize;
            let blit = d[b + 9];
            blit_mask.push(blit);
            images.push(decode(&data, addr, w, h, blit));
        }
        Ok(Cel { images, blit_mask })
    }
}

fn decode(data: &[u8], addr: usize, w: usize, h: usize, blit: u8) -> Sprite {
    let packed_w = (w + 15) / 16 * 2;
    let n_planes = if blit == 32 { 2 } else { 8 - blit.leading_zeros() as usize };
    let plane_len = packed_w * h;
    let uw = packed_w * 8;
    let mut px = vec![0u8; uw * h];
    if uw == 0 || h == 0 {
        return Sprite { width: uw, height: h, real_width: w, pixels: px };
    }

    let at = |i: usize| -> u8 { data.get(i).copied().unwrap_or(0) };
    for i in 0..plane_len {
        let mut b = [0u8; 5];
        for (p, slot) in b.iter_mut().enumerate().take(n_planes) {
            *slot = at(addr + p * plane_len + i);
        }
        let mut o = i * 8;
        for bit in (0..8).rev() {
            let mut v = 0u8;
            for p in 0..n_planes {
                v |= ((b[p] >> bit) & 1) << p;
            }
            v = if blit == 32 {
                (v & 3) | (u8::from(v != 0) << 4)
            } else {
                v & blit
            };
            if o < px.len() {
                px[o] = v;
            }
            o += 1;
        }
    }
    Sprite { width: uw, height: h, real_width: w, pixels: px }
}
