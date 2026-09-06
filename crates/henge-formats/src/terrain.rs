//! `.T` files: one combat arena.
//!
//! ```text
//! u32 packed_len
//! u8  packed[]
//! ```
//!
//! and once unpacked:
//!
//! ```text
//! u16 _
//! u16 left, u16 right, u16 bottom, u16 top    walkable bounds
//! placement[]                                  six bytes each, until sheet == 0xff:
//!     u8  sheet    which CMP sheet of the area's set
//!     u8  cell     cell number within that sheet
//!     u16 x, u16 y screen position
//! ```
//!
//! Which sheets an arena uses is implied by its filename prefix: `WA*` draws from
//! `WA1.CMP` over the `WAB1.CMP` backdrop, `FO*` from `FO1`/`FO2` over `FOB1`,
//! `SW*` from `SW1` over `SWB1`, `GL*` over `GLB1`.

use crate::depack;
use serde::Serialize;

#[derive(Serialize, Clone, Copy, Debug)]
pub struct Placement {
    pub sheet: u8,
    pub cell: u8,
    /// Signed: scenery may start above or left of the screen, so a tree can be
    /// cut off by the top edge.
    pub x: i16,
    pub y: i16,
}

#[derive(Serialize, Debug)]
pub struct Terrain {
    pub left: u16,
    pub right: u16,
    pub bottom: u16,
    pub top: u16,
    pub placements: Vec<Placement>,
}

impl Terrain {
    pub fn parse(file: &[u8]) -> anyhow::Result<Terrain> {
        anyhow::ensure!(file.len() > 14, "file too short to be a .T");
        let packed_len = u32::from_be_bytes([file[0], file[1], file[2], file[3]]) as usize;
        let d = depack::unpack(file, 4, packed_len);
        anyhow::ensure!(d.len() >= 10, "unpacked .T is too short");

        let u16at = |o: usize| u16::from_be_bytes([d[o], d[o + 1]]);
        let i16at = |o: usize| i16::from_be_bytes([d[o], d[o + 1]]);
        let mut placements = Vec::new();
        let mut o = 10;
        while o + 6 <= d.len() {
            if d[o] == 0xff {
                break;
            }
            let p = Placement { sheet: d[o], cell: d[o + 1], x: i16at(o + 2), y: i16at(o + 4) };
            // A placement this far off screen means we have run past the real list,
            // which happens in the stub files (F09.T, SW9.T).
            if !(-200..520).contains(&p.x) || !(-200..400).contains(&p.y) {
                break;
            }
            placements.push(p);
            o += 6;
        }
        Ok(Terrain { left: u16at(2), right: u16at(4), bottom: u16at(6), top: u16at(8), placements })
    }
}
