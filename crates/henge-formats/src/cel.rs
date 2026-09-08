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
//!     u8  blit_mask      which output bits the stored planes feed
//! u8  packed[]
//! ```
//!
//! Row stride is `((width + 15) / 16) * 2` bytes, so decoded width rounds up to a
//! multiple of eight.
//!
//! **`blit_mask` is a bit set, not a bit length.** `GFX` has one hand-written
//! routine per mask value, named `do_1` through `do_20` after the byte itself,
//! and each one loads exactly as many planes as its mask has bits set and rolls
//! them into the mask's set bits in order. `do_17` is the clearest: mask `0x17`,
//! four planes read through `si`, `di`, `bx` and `bp` into output bits 0, 1, 2
//! and 4, with `xor cl, cl` supplying a zero for bit 3 because nothing is
//! stored for it. So a frame stores `blit_mask.count_ones()` planes, and plane
//! *i* is the *i*th set bit of the mask.
//!
//! Reading the mask as a bit length instead is right for `0x01`, `0x03`, `0x07`,
//! `0x0f` and `0x1f`, which is most of the release; for `0x17`, `0x1b`, `0x13`
//! and the rest of the gapped masks it reads one plane too many, takes the next
//! frame's data as the top plane and paints the sprite in stripes.
//!
//! `blit_mask == 32` is the one routine that is not its own set bits. `do_20`
//! loads two planes into bits 0 and 1 and ors them together into bit 4, which is
//! the game's cheap outline blit; only `PO.CEL`, the pointer, uses it.
//!
//! The routines are reached through `plane_tab`, thirty-three words indexed by
//! the mask byte. Twenty-two of its slots name a routine and are exactly the
//! twenty-two `do_` symbols; the other eleven hold the address of a bare `ret`,
//! so those masks draw nothing. See [`NO_ROUTINE`].

use crate::depack;
use crate::piv::Sprite;

/// Mask bytes `plane_tab` sends to the bare `ret` instead of to a routine.
///
/// The table is thirty-three words indexed by the mask, and eleven of its
/// slots hold the stub. Nine of the eleven never occur in the release; the two
/// that do are `0x00`, which is the forty-two placeholder entries, and `0x11`,
/// which is `TROLL1.CEL` frame 46 and `MUDMEN2.CEL` frame 15. Those two frames
/// draw nothing in the original, so they draw nothing here.
const NO_ROUTINE: [u8; 11] = [0x00, 0x02, 0x08, 0x11, 0x14, 0x15, 0x16, 0x18, 0x19, 0x1a, 0x1d];

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
    // Which output bit each stored plane feeds, lowest set bit of the mask
    // first. `do_17` loads four planes and rolls them into bits 0, 1, 2 and 4,
    // which is the mask's set bits in order; every other routine does the same
    // with its own mask.
    let mut bits = [0u32; 5];
    let mut n_planes = 0;
    for b in 0..5 {
        if blit >> b & 1 != 0 {
            bits[n_planes] = b;
            n_planes += 1;
        }
    }
    // `do_20` is the one routine that is not its mask's set bits: it loads two
    // planes into bits 0 and 1 and ors them together into bit 4.
    if blit == 32 {
        n_planes = 2;
    }
    // A mask the dispatch table has no routine for draws nothing at all.
    if NO_ROUTINE.contains(&blit) {
        n_planes = 0;
    }
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
            if blit == 32 {
                v = ((b[0] >> bit) & 1) | (((b[1] >> bit) & 1) << 1);
                v |= u8::from(v != 0) << 4;
            } else {
                for p in 0..n_planes {
                    v |= ((b[p] >> bit) & 1) << bits[p];
                }
            }
            if o < px.len() {
                px[o] = v;
            }
            o += 1;
        }
    }
    Sprite { width: uw, height: h, real_width: w, pixels: px }
}

#[cfg(test)]
mod tests {
    use super::decode;

    /// One plane of one byte, so a frame is eight pixels wide on paper and
    /// sixteen after the stride rounds up.
    fn planes(bits: &[u8]) -> Vec<u8> {
        // Stride for a frame up to sixteen wide is two bytes, so each plane is
        // two bytes and the second is empty.
        bits.iter().flat_map(|b| [*b, 0]).collect()
    }

    /// `do_17` reads four planes and rolls them into bits 0, 1, 2 and 4. The
    /// mask is a bit set, not a bit length: reading it as a length takes five
    /// planes, which runs into whatever follows the frame.
    #[test]
    fn a_gapped_mask_stores_one_plane_per_set_bit() {
        // Four planes, each with a single pixel lit, and a fifth block of
        // 0xff standing for the next frame's data.
        let data = planes(&[0x80, 0x40, 0x20, 0x10, 0xff]);
        let s = decode(&data, 0, 8, 1, 0x17);
        assert_eq!(s.pixels[0], 1, "plane 0 is bit 0");
        assert_eq!(s.pixels[1], 2, "plane 1 is bit 1");
        assert_eq!(s.pixels[2], 4, "plane 2 is bit 2");
        assert_eq!(s.pixels[3], 16, "plane 3 is bit 4, the mask's fourth set bit");
        assert!(
            s.pixels[4..8].iter().all(|p| *p == 0),
            "nothing past the four stored planes is read: {:?}",
            &s.pixels[..8]
        );
    }

    /// The masks that are their own bit length decode the same either way,
    /// which is why most of the release never showed the fault.
    #[test]
    fn a_solid_mask_is_planes_in_order() {
        let data = planes(&[0x80, 0x40, 0x20, 0x10, 0x08]);
        let s = decode(&data, 0, 8, 1, 0x1f);
        assert_eq!(&s.pixels[..5], &[1, 2, 4, 8, 16]);
    }

    /// `do_20` is the exception: two planes into bits 0 and 1, ored together
    /// into bit 4. Only the pointer uses it.
    #[test]
    fn the_outline_blit_ors_its_two_planes_into_bit_four() {
        let data = planes(&[0b1010_0000, 0b1100_0000]);
        let s = decode(&data, 0, 8, 1, 32);
        assert_eq!(&s.pixels[..3], &[0b1_0011, 0b1_0010, 0b1_0001]);
        assert_eq!(s.pixels[3], 0, "an empty pixel stays transparent");
    }

    /// A mask of zero stores nothing and the frame is blank, which is what the
    /// forty-two placeholder entries in the release are.
    #[test]
    fn an_empty_mask_draws_nothing() {
        let s = decode(&planes(&[0xff]), 0, 8, 1, 0);
        assert!(s.pixels.iter().all(|p| *p == 0));
    }
}
