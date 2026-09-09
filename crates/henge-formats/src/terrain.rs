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
//! u16be count                                  how many border rectangles follow
//! border[count]                                eight bytes each:
//!     i16be left, right, bottom, top
//! placement[]                                  six bytes each, until sheet == 0xff:
//!     u8  sheet    which CMP sheet of the area's set, or 0xfe for no sheet
//!     u8  cell     cell number within that sheet
//!     i16be x, y   screen position
//! ```
//!
//! **The header is a list, not one rectangle.** The loader at image `0x8d93`
//! reads the count, skips that many eight-byte records to reach the placements,
//! and walks the records again taking the deepest `bottom` into DS:`0x80b5`,
//! which is where the knights are stood. `SBORD` (`0x4552`) walks the same list
//! every frame and clears the walk bits that would carry a fighter across one.
//! Fifty two of the fifty six shipped layouts hold exactly one record, which is
//! why reading the header as `u16 _; u16 left, right, bottom, top` looked right;
//! `FO7`, `SW6` and `SWL2` hold two and `GLL4` holds three, and reading those
//! four as one record ran the placement walk two or four bytes out of step and
//! threw most of their scenery away.
//!
//! A rectangle is **impassable ground**, not the walkable box: `left`/`right`
//! span the part of the screen it covers and `bottom` is the row its lower edge
//! sits on, so the first record of every arena is the tree line and the extra
//! ones are whatever hangs below it.
//!
//! Which sheets an arena uses is implied by its filename prefix: `WA*` draws from
//! `WA1.CMP` over the `WAB1.CMP` backdrop, `FO*` from `FO1`/`FO2` over `FOB1`,
//! `SW*` from `SW1` over `SWB1`, `GL*` over `GLB1`.
//!
//! **`sheet` is kept as the file writes it and read by the renderer**, because
//! it says more than which sheet: `Sholoop` (image `0x7ca9`) ends the list on
//! 0xff, skips 0xfe without drawing anything, sends 3 to the family's own sheet
//! and forces every other value to 4, which is `FO2`. The release carries 3
//! (3,338 times), 4 (1,606), 0xfe (168) and 1 (six times). See
//! `henge_core::content::Family::tile_sheet`.

use crate::depack;
use serde::Serialize;

#[derive(Serialize, Clone, Copy, Debug)]
pub struct Placement {
    pub sheet: u8,
    pub cell: u8,
    /// Signed and big-endian: scenery may start above or left of the screen.
    /// `ClipTile` (image `0x7d5a`) does not cut what hangs off either edge, it
    /// clamps: a negative `x` puts the whole cell at column zero, and a
    /// negative `y` takes `-y - 1` rows off it rather than `-y`.
    pub x: i16,
    pub y: i16,
}

/// One impassable rectangle out of the header.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Border {
    pub left: i32,
    pub right: i32,
    pub bottom: i32,
    pub top: i32,
}

#[derive(Serialize, Debug)]
pub struct Terrain {
    pub borders: Vec<Border>,
    pub placements: Vec<Placement>,
}

/// More records than this in a `.T` header means the file is not one: the two
/// stub layouts that ship three times over declare 19,342 of them.
const MAX_BORDERS: usize = 16;

impl Terrain {
    pub fn parse(file: &[u8]) -> anyhow::Result<Terrain> {
        anyhow::ensure!(file.len() > 14, "file too short to be a .T");
        let packed_len = u32::from_be_bytes([file[0], file[1], file[2], file[3]]) as usize;
        let d = depack::unpack(file, 4, packed_len);
        anyhow::ensure!(d.len() >= 10, "unpacked .T is too short");

        let i16at = |o: usize| i32::from(i16::from_be_bytes([d[o], d[o + 1]]));
        let count = u16::from_be_bytes([d[0], d[1]]) as usize;
        anyhow::ensure!(
            count <= MAX_BORDERS,
            "{count} border records is not a layout"
        );
        anyhow::ensure!(2 + count * 8 <= d.len(), "border list runs past the end");

        let borders: Vec<Border> = (0..count)
            .map(|i| {
                let o = 2 + i * 8;
                Border {
                    left: i16at(o),
                    right: i16at(o + 2),
                    bottom: i16at(o + 4),
                    top: i16at(o + 6),
                }
            })
            .collect();

        let mut placements = Vec::new();
        let mut o = 2 + count * 8;
        while o + 6 <= d.len() {
            if d[o] == 0xff {
                break;
            }
            let p = Placement {
                sheet: d[o],
                cell: d[o + 1],
                x: i16::from_be_bytes([d[o + 2], d[o + 3]]),
                y: i16::from_be_bytes([d[o + 4], d[o + 5]]),
            };
            // A placement this far off screen means we have run past the real
            // list, which is what the stub files look like.
            if !(-200..520).contains(&p.x) || !(-200..400).contains(&p.y) {
                break;
            }
            placements.push(p);
            o += 6;
        }
        Ok(Terrain {
            borders,
            placements,
        })
    }
}
