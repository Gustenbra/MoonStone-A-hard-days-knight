//! Moonstone's packer, shared by every container in the game
//! (PIV, CMP, CEL, OB, F, FON, T, C, A).
//!
//! One control byte, then eight items, most significant bit first.
//!
//! * bit clear: one literal byte
//! * bit set:   a 16-bit big-endian back-reference
//!   - `count  = 0x22 - (word >> 11)`
//!   - `offset = word & 0x7ff`, measured back from the write head
//!
//! The copy may overlap the write head, which makes it double as a run encoder,
//! so it has to be done byte by byte rather than with a block copy.

pub fn unpack(src: &[u8], off: usize, packed_len: usize) -> Vec<u8> {
    // Headers are trusted only as far as the file actually goes. A few of the
    // game's stub files (F09.T, SW9.T) carry a length field that is plainly
    // garbage, so clamp rather than believe it.
    let available = src.len().saturating_sub(off);
    let packed_len = packed_len.min(available);
    let end = off + packed_len;
    let mut out: Vec<u8> = Vec::with_capacity(packed_len.saturating_mul(4).min(1 << 22));
    let mut p = off;

    while p < end {
        let control = src[p];
        p += 1;
        for bit in (0..8).rev() {
            if p >= end {
                break;
            }
            if control & (1 << bit) != 0 {
                if p + 1 >= end {
                    break;
                }
                let word = u16::from_be_bytes([src[p], src[p + 1]]) as usize;
                p += 2;
                let count = 0x22 - (word >> 11);
                let dist = word & 0x7ff;
                let from = out.len().wrapping_sub(dist);
                for i in 0..count {
                    let b = from
                        .checked_add(i)
                        .and_then(|k| out.get(k).copied())
                        .unwrap_or(0);
                    out.push(b);
                }
            } else {
                out.push(src[p]);
                p += 1;
            }
        }
    }
    out
}

/// Pads with zeroes or truncates to an exact length.
pub fn fit(mut v: Vec<u8>, len: usize) -> Vec<u8> {
    v.resize(len, 0);
    v
}
