//! A pointer, and the boxes it can be over.
//!
//! **Recovered.** `_STATUS:MovePointer` is short enough to quote:
//!
//! ```text
//! PointerFLAG = 1
//! read the stick
//! bit 0x01 -> x += 2      bit 0x02 -> x -= 2
//! bit 0x04 -> y += 2      bit 0x08 -> y -= 2
//! bit 0x10 -> PointerFLAG = 0        ; fire
//! clamp x to 0..0x13a, y to 0..0xc2
//! ```
//!
//! So the original's pointer is **driven by the stick, not by a mouse**: two
//! pixels a frame in whichever direction is held, clamped to a box four pixels
//! narrower and six shorter than the screen. `SHOWPOINTER` then blits it at
//! `bx, cx` from the same pair, and `PO.CEL` is the art: one 16 by 18 frame,
//! an arrow with a tail, which the packs have carried decoded and unused since
//! the first day.
//!
//! **The gadgets are recovered too**, and they are not a general widget kit:
//! they are a list of rectangles with an id and a line of text apiece.
//!
//! - `GadgetSlot` walks 98 records of twenty bytes looking for one whose width
//!   is zero, so 98 is the table's size and a zero width is what "empty" means.
//! - `CLEARGADGETS` zeroes all 2,000 bytes of it and the twenty-byte record
//!   being built beside it.
//! - `AddIconGadget` fills that record: the width and height come out of the
//!   **icon's own cel header** (`es:[bx+si+0xe]` and `+0x10`, byte swapped),
//!   the position from `STX + StatsOffset` and `STY`, the id from `STID`, a
//!   payload word from `STRP`, and a pointer to a ten-byte text record from
//!   `RESP + STID/2 * 10`. So a gadget's size is the size of the thing drawn
//!   in it, and every gadget carries the line it says when the pointer is on
//!   it.
//! - `CHECKGADGET` walks the live slots and asks the same two-rectangle overlap
//!   helper at `0x9f0d` that `MOON:CheckGROOC` uses to decide whether a
//!   traveller has arrived somewhere: one axis at a time, two passes is inside.
//!   The pointer's rectangle is **one pixel by one pixel** (`mov bx, ax;
//!   inc bx`), so a gadget is hit by the pointer's own corner and not by a
//!   region around it.
//! - `GadgetHit` draws the gadget's text record if it has one, and returns
//!   "yes" either way. `HotGadget` is what fire does with the one under the
//!   pointer: id 7 leaves the screen, a gadget whose text record is `NEXT`
//!   moves to the next knight, and anything else is decoded out of the payload
//!   word's low nibble.
//!
//! **Ours:** what a gadget's payload means here. The original's nibbles name
//! its own trading screen's operations (5 cast, 1 take magic, 3 raise an
//! ability, 0xa buy). Here a gadget carries a `usize` id that the screen which
//! registered it interprets, because the screens are ours and their menus are
//! already lists with a highlight.

use serde::{Deserialize, Serialize};

/// Pixels the pointer moves in one tick. `add word ptr [PointerX], 2`.
pub const STEP: i32 = 2;
/// `cmp word ptr [PointerX], 0x13b / jl` then `mov ..., 0x13a`.
pub const MAX_X: i32 = 0x13a;
/// `cmp word ptr [PointerY], 0xc3 / jl` then `mov ..., 0xc2`.
pub const MAX_Y: i32 = 0xc2;
/// How many gadgets there is room for. `mov cx, 0x62` in `GadgetSlot`.
pub const SLOTS: usize = 0x62;

/// Where the pointer is, and whether fire is down.
///
/// Serializable like everything else in this crate: it is simulation state, so
/// a replay and a network peer see the same pointer.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pointer {
    pub x: i32,
    pub y: i32,
    /// `PointerFLAG`, which fire clears. Named for what it means rather than
    /// for the sense the original stores it in.
    pub fire: bool,
    /// Whether the pointer has been steered at all. A pointer nobody has moved
    /// is not drawn, so a player using only the keyboard never has an arrow
    /// sitting in the corner of every screen.
    pub woken: bool,
}

impl Pointer {
    /// Start in the middle, which is where a pointer with nothing to say
    /// belongs. The original leaves it wherever the last screen left it.
    pub fn centred() -> Pointer {
        Pointer { x: MAX_X / 2, y: MAX_Y / 2, fire: false, woken: false }
    }

    /// One tick of `MovePointer`: two pixels a direction, then the clamp.
    pub fn steer(&mut self, dx: i32, dy: i32, fire: bool) {
        if dx != 0 || dy != 0 {
            self.woken = true;
        }
        self.x = (self.x + dx.signum() * STEP).clamp(0, MAX_X);
        self.y = (self.y + dy.signum() * STEP).clamp(0, MAX_Y);
        self.fire = fire;
    }
}

/// One clickable box.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Gadget {
    /// What the screen that registered it calls this one. `STID`.
    pub id: usize,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// The line this gadget says while the pointer is on it. `RESP`, the ten
    /// byte text record `AddIconGadget` hangs off every gadget it makes.
    pub label: String,
}

impl Gadget {
    /// `CHECKGADGET`, exactly: two rectangles, one axis at a time, and the
    /// pointer's is one pixel square.
    pub fn covers(&self, x: i32, y: i32) -> bool {
        overlaps(x, x + 1, self.x, self.x + self.w) && overlaps(y, y + 1, self.y, self.y + self.h)
    }
}

/// The helper at image 0x9f0d, which `CheckGROOC` also asks: do two spans on
/// one axis touch. Reproduced as written, because it is not symmetric:
///
/// ```text
/// cmp cx, ax        ; b0 against a0
/// jl  L1
/// cmp cx, bx        ; b0 >= a0, so hit when b0 <  a1
/// jl  hit
/// ret
/// L1: cmp dx, ax    ; b0 <  a0, so hit when b1 >  a0
///     jle ret
/// hit: inc bp
/// ```
///
/// With the pointer's own one-pixel span as `a`, that comes out as
/// `b0 <= x < b1`: closed on the left, open on the right, which is why a
/// gadget's own top-left corner is inside it and the pixel past its width is
/// not.
fn overlaps(a0: i32, a1: i32, b0: i32, b1: i32) -> bool {
    if b0 >= a0 {
        b0 < a1
    } else {
        b1 > a0
    }
}

/// The live gadget table. `CLEARGADGETS` empties it, `ADDGADGET` fills a slot,
/// `CHECKGADGET` asks which one a point is in.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Gadgets {
    list: Vec<Gadget>,
}

impl Gadgets {
    /// `CLEARGADGETS`. Every screen does this before it lays its own out, which
    /// is why the table can be small and flat.
    pub fn clear(&mut self) {
        self.list.clear();
    }

    /// `ADDGADGET`, through `GadgetSlot`: refused when the table is full,
    /// rather than growing it. The original has 98 slots and no way to make
    /// more, and silently dropping the 99th is what it does.
    pub fn add(&mut self, g: Gadget) -> bool {
        if self.list.len() >= SLOTS {
            return false;
        }
        self.list.push(g);
        true
    }

    /// Convenience for a screen laying out a list of rows.
    pub fn add_box(&mut self, id: usize, x: i32, y: i32, w: i32, h: i32, label: &str) -> bool {
        self.add(Gadget { id, x, y, w, h, label: label.to_string() })
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Gadget> {
        self.list.iter()
    }

    /// `CHECKGADGET`. The first slot the point is inside wins, because the
    /// original stops walking the table the moment `bp` reaches two.
    pub fn hit(&self, x: i32, y: i32) -> Option<&Gadget> {
        self.list.iter().find(|g| g.covers(x, y))
    }

    /// What the pointer is over, as the index into whatever list the screen
    /// laid out. Screens here are menus with a highlight, so this is the shape
    /// they actually want.
    pub fn hit_id(&self, p: &Pointer) -> Option<usize> {
        self.hit(p.x, p.y).map(|g| g.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pointer_moves_two_pixels_a_tick() {
        let mut p = Pointer { x: 10, y: 10, fire: false, woken: false };
        p.steer(1, 0, false);
        assert_eq!((p.x, p.y), (12, 10));
        p.steer(0, -1, false);
        assert_eq!((p.x, p.y), (12, 8));
        assert!(p.woken, "steering it is what wakes it");
    }

    /// The clamps are literals in `MovePointer` and worth pinning.
    #[test]
    fn the_pointer_stops_at_the_recovered_bounds() {
        let mut p = Pointer { x: 0, y: 0, fire: false, woken: false };
        for _ in 0..400 {
            p.steer(-1, -1, false);
        }
        assert_eq!((p.x, p.y), (0, 0));
        for _ in 0..400 {
            p.steer(1, 1, false);
        }
        assert_eq!((p.x, p.y), (MAX_X, MAX_Y), "0x13a by 0xc2");
    }

    #[test]
    fn fire_rides_on_the_pointer() {
        let mut p = Pointer::centred();
        assert!(!p.fire);
        p.steer(0, 0, true);
        assert!(p.fire);
        assert!(!p.woken, "fire alone does not wake it");
    }

    #[test]
    fn a_gadget_is_hit_by_the_pointers_own_corner() {
        let g = Gadget { id: 3, x: 10, y: 20, w: 30, h: 8, label: "Buy".into() };
        assert!(g.covers(10, 20), "the top left corner is inside");
        assert!(g.covers(39, 27), "and the last pixel of it");
        assert!(!g.covers(40, 27), "the pixel past its width is not");
        assert!(!g.covers(9, 20));
        assert!(!g.covers(10, 19));
        assert!(!g.covers(10, 28));
    }

    #[test]
    fn the_first_matching_slot_wins() {
        let mut g = Gadgets::default();
        g.add_box(0, 0, 0, 100, 100, "under");
        g.add_box(1, 10, 10, 10, 10, "over");
        assert_eq!(g.hit(15, 15).map(|h| h.id), Some(0), "the table is walked in order");
    }

    #[test]
    fn the_table_is_the_original_size_and_refuses_the_next() {
        let mut g = Gadgets::default();
        for i in 0..SLOTS {
            assert!(g.add_box(i, 0, 0, 1, 1, ""));
        }
        assert!(!g.add_box(SLOTS, 0, 0, 1, 1, ""), "98 slots and no more");
        g.clear();
        assert!(g.is_empty());
    }

    #[test]
    fn a_pointer_over_a_row_names_that_row() {
        let mut g = Gadgets::default();
        for i in 0..4 {
            g.add_box(i, 20, 100 + i as i32 * 9, 120, 9, "row");
        }
        let p = Pointer { x: 30, y: 118, fire: false, woken: true };
        assert_eq!(g.hit_id(&p), Some(2));
        let off = Pointer { x: 300, y: 10, fire: false, woken: true };
        assert_eq!(g.hit_id(&off), None);
    }
}
