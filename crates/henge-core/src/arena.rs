//! A combat arena: a backdrop, some scenery, and the ground the fighters may
//! stand on.
//!
//! # What a border actually is
//!
//! An arena's `.T` header is a **count and that many eight-byte rectangles**,
//! and each rectangle is a piece of ground you may not stand on: the tree line
//! across the top of the screen, and in four of the fifty six layouts one or
//! two more for whatever hangs below it. `left`/`right` are the columns it
//! covers and `bottom` is the row its lower edge falls on. The walkable ground
//! is everything *under* the rectangles, which is the opposite of how this
//! engine used to read the header.
//!
//! Two routines in the original decide, every frame, which of the four
//! directions a fighter may still take. Neither of them clamps a coordinate to
//! a box: they clear bits in a per-actor byte at `+0x26`, which the movement
//! step then reads.
//!
//! * **`CheckBorder`** (image `0x40d0`) is the global one, and it is the same
//!   in every arena. It probes the actor's own anchor twenty five pixels ahead
//!   in whichever way he faces and refuses the horizontal step that would take
//!   that probe outside 10 to 320, writing the anchor back to the limit it
//!   crossed; and it takes the anchor's depth plus nine and refuses down above
//!   155 and up below 30.
//! * **`SBORD`** (`0x4552`) walks the arena's own rectangles. For each one
//!   whose columns the actor's body box would overlap after the step, it
//!   refuses **up** when the rectangle's bottom is at or below the depth the
//!   step would reach, and refuses the sideways step when the body box's own
//!   bottom row is inside the band from 30 down to that same bottom.
//!
//! The depth both routines test is the **task anchor**, the point the original
//! places sprite parts against, not the feet: `SBORD` adds `0x2f` to it before
//! comparing it with a rectangle's bottom, `CheckBorder` adds `9` before its
//! own two limits, and `FindHalfBORD` and its two neighbours subtract the same
//! `0x2f` when they choose where to stand a knight. On the knight, which is the
//! only actor the original ever puts through either routine, the anchor sits
//! fifty two rows above his feet, so the two probes fall five and forty three
//! rows above them. This engine positions every actor by its feet, so the two
//! probes are written that way here: exactly the original for the knight, and
//! the same place on the figure for the creatures the original bordered not at
//! all.
//!
//! Scenery is still drawn over flat ground and depth is still decided purely by
//! feet position.

use serde::{Deserialize, Serialize};

/// The direction bits of the original's actor byte `+0x26`.
///
/// `GetInputDevice` builds it from the joystick, `ControlKnight` copies it
/// straight into the actor, and every check below clears bits out of it. Bit 4
/// is fire, which is not a direction and is not here.
pub mod dir {
    pub const RIGHT: u8 = 1;
    pub const LEFT: u8 = 2;
    pub const DOWN: u8 = 4;
    pub const UP: u8 = 8;
    /// All four, which is what a step starts with before anything refuses it.
    pub const ALL: u8 = RIGHT | LEFT | DOWN | UP;
}

/// `CheckBorder`'s literals, and `SBORD`'s.
pub mod limit {
    /// How far ahead of himself an actor is probed, in whichever way he faces.
    /// `mov ax, 0x19`.
    pub const REACH: i32 = 25;
    /// The horizontal limits the probe may not cross, `0x0a` and `0x140`. The
    /// routine writes the anchor back to whichever it crossed, so these are
    /// also where a fighter comes to rest.
    pub const X_LOW: i32 = 10;
    pub const X_HIGH: i32 = 320;
    /// `mov bx, 9`, added to the anchor's depth before the two depth tests.
    pub const DEPTH_PROBE: i32 = 9;
    /// The depth limits, `0x1e` and `0x9b`.
    pub const DEPTH_LOW: i32 = 30;
    pub const DEPTH_HIGH: i32 = 155;
    /// `SBORD`'s own proxy for where an actor's feet are, `0x2f`, added to the
    /// anchor before it is compared with a rectangle's bottom. The same number
    /// is subtracted by `FindHalfBORD`, which is why the standing places are
    /// anchor depths.
    pub const FEET: i32 = 47;
    /// Where the knight's anchor sits above his feet, out of his own standing
    /// frame: the lowest pixel of `Knight_SwStance` is `+52`.
    pub const ANCHOR: i32 = 52;
    /// `SBORD`'s feet proxy, as rows above the feet: `ANCHOR - FEET`.
    pub const FEET_LEAD: i32 = ANCHOR - FEET;
    /// `CheckBorder`'s depth probe, the same way: `ANCHOR - DEPTH_PROBE`.
    pub const DEPTH_LEAD: i32 = ANCHOR - DEPTH_PROBE;
    /// The top of the band `SBORD` considers a fighter to be *inside* a
    /// rectangle for the purpose of refusing him a sideways step.
    pub const BAND_TOP: i32 = 30;
    /// The screen row the three standing places are measured down to,
    /// `sub ax, 0xc8` in `FindHalfBORD`.
    pub const FLOOR: i32 = 200;
}

/// `AddCNT` at DS:`0xa68`, and what `AddKnight` does with it.
///
/// **Recovered**, `AddKnight` at image `0x298c`:
///
/// ```text
/// 0298c  add  word [AddCNT], 1
/// 02991  and  word [AddCNT], 3
/// 02996  je   AddKnight              ; zero is skipped: round again
/// 02998  cmp  word [AddCNT], 3  / jne +3 / call FindHalfBORD
/// 029a2  cmp  word [AddCNT], 2  / jne +3 / call FindQuarterBORD
/// 029ac  cmp  word [AddCNT], 1  / jne +3 / call Find3QuarterBORD
/// 029b6  mov  word [di+6], ax        ; whichever of the three ran
/// ```
///
/// So the counter runs 1, 2, 3, 1, 2, 3 and never rests on 0, and the places it
/// hands out are three quarters, one quarter, one half, over and over. Two
/// things follow. **Every arrival gets one of the three**, because the store at
/// `0x29b6` is unconditional, which is why the `z` word of a creature's seat
/// table and the `0x64` `SetKnightCombat` writes are both dead. And **the
/// counter is never reset**: it is a word of BSS that only `AddKnight` touches,
/// so the rotation carries on across a whole session rather than starting again
/// each bout, and which depth the player's knight gets depends on how many
/// fighters have stood up before him.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Arrivals(pub i32);

impl Arrivals {
    /// One arrival: step the counter as `AddKnight` does, and say which of the
    /// three standing places this one takes, in quarters.
    pub fn next_place(&mut self) -> i32 {
        loop {
            self.0 = (self.0 + 1) & 3;
            if self.0 != 0 {
                break;
            }
        }
        match self.0 {
            3 => 2,
            2 => 1,
            _ => 3,
        }
    }
}

/// One impassable rectangle out of an arena's `.T` header, or the one
/// `SETDEMONBORD` writes over the list.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Border {
    pub left: i32,
    pub right: i32,
    pub bottom: i32,
    pub top: i32,
}

impl Border {
    /// Whether this looks like a rectangle at all. The two stub layouts that
    /// ship three times over decode to nonsense and are caught here.
    pub fn is_sane(&self) -> bool {
        self.left < self.right && self.top < self.bottom && self.bottom < 400 && self.left > -640
    }
}

/// The overlap test at image `0x9f0d`, which both `SBORD` and the arrival
/// check share: it counts a pass in `bp` rather than returning a flag.
///
/// It is not symmetric, and the asymmetry is reproduced rather than tidied:
/// the first interval is half open and the second is tested by its low end
/// first.
pub fn overlaps(a: i32, b: i32, c: i32, d: i32) -> bool {
    if c >= a {
        c < b
    } else {
        d > a
    }
}

/// A rectangle. Still here because a fight has half a dozen incidental places
/// that shove somebody about, and every one of them wants somewhere to stop.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    pub left: i32,
    pub right: i32,
    pub top: i32,
    pub bottom: i32,
}

/// What `CheckBorder` allows, in feet coordinates: columns 10 to 320, and
/// rows 73 to 198.
///
/// It is the same in every arena, which is the point of it: the arena's own
/// rectangles do the rest, and they are a list rather than a box.
pub const GLOBAL: Bounds = Bounds {
    left: limit::X_LOW,
    right: limit::X_HIGH,
    top: limit::DEPTH_LOW + limit::DEPTH_LEAD,
    bottom: limit::DEPTH_HIGH + limit::DEPTH_LEAD,
};

impl Bounds {
    /// Tolerates inverted bounds rather than panicking, since content is data and
    /// data can be wrong.
    pub fn clamp(&self, x: i32, y: i32) -> (i32, i32) {
        let (l, r) = (self.left.min(self.right), self.left.max(self.right));
        let (t, b) = (self.top.min(self.bottom), self.top.max(self.bottom));
        (x.clamp(l, r), y.clamp(t, b))
    }

    pub fn is_sane(&self) -> bool {
        self.left < self.right
            && self.top < self.bottom
            && (0..640).contains(&self.right)
            && (0..400).contains(&self.bottom)
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && x <= self.right && y >= self.top && y <= self.bottom
    }
}

/// Everything one actor is tested with on the frame it wants to move.
///
/// The fields are the original's actor record: `+2` is the anchor's column,
/// `+8` the facing, `+0x22`/`+0x24` the body box's columns and `+0x50` its
/// lowest row. `y` is where this engine keeps an actor, which is its feet.
#[derive(Clone, Copy, Debug)]
pub struct Step {
    pub x: i32,
    pub y: i32,
    /// 1 facing right, -1 facing left.
    pub facing: i32,
    pub dx: i32,
    pub dy: i32,
    pub box_left: i32,
    pub box_right: i32,
    pub box_bottom: i32,
}

/// The ground of one arena: the rectangles nobody may stand on.
///
/// Held as a list because the header is a list. `SETDEMONBORD` replaces the
/// whole of it with one record rather than adding to it, and so does
/// [`Field::narrow_to`].
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Field {
    pub borders: Vec<Border>,
}

impl Field {
    pub fn new(borders: Vec<Border>) -> Field {
        Field { borders }
    }

    /// `SETDEMONBORD`: one actor's own rectangle written over the whole list.
    pub fn narrow_to(&mut self, border: Border) {
        self.borders = vec![border];
    }

    /// The deepest bottom in the list, floored at 30 the way the loader at
    /// image `0x8d93` floors DS:`0x80b5`. It is the row the ground begins at,
    /// and the three standing places are measured down from it.
    pub fn floor(&self) -> i32 {
        self.borders
            .iter()
            .map(|b| b.bottom)
            .fold(limit::DEPTH_LOW, i32::max)
    }

    /// `FindHalfBORD` (image `0x29cb`), `FindQuarterBORD` (`0x29e0`) and
    /// `Find3QuarterBORD` (`0x29f7`): where an arrival is stood, as an anchor
    /// depth, one quarter, one half or three quarters of the way from the
    /// deepest border down to the foot of the screen.
    ///
    /// All three are the same five instructions on DS:`0x80b5`, the deepest
    /// border row, which the layout loader at image `0x8d93` fills: subtract
    /// 200, negate, shift, add the row back, take `0x2f` off. The three quarter
    /// one takes the half and adds half of that again (`mov bx, ax; shr bx, 1;
    /// add ax, bx`) rather than shifting twice, which is the same number for
    /// every shipped layout and is reproduced as written.
    pub fn standing_depth(&self, quarters: i32) -> i32 {
        let floor = self.floor();
        let gap = limit::FLOOR - floor;
        // The original shifts rather than divides, on a value that is positive
        // in every shipped layout.
        let down = match quarters {
            1 => gap >> 2,
            3 => (gap >> 1) + (gap >> 2),
            _ => gap >> 1,
        };
        floor + down - limit::FEET
    }

    /// The same three places as a row to stand feet on, which is what this
    /// engine positions a fighter by.
    pub fn standing_row(&self, quarters: i32) -> i32 {
        self.standing_depth(quarters) + limit::ANCHOR
    }

    /// Which of the four directions a step may still take, out of the ones it
    /// asked for.
    ///
    /// `CheckBorder` first, which may also move the anchor to the column limit
    /// it crossed, then `SBORD` over every rectangle.
    pub fn allow(&self, step: &mut Step, wanted: u8) -> u8 {
        let mut ok = wanted;
        // ---- CheckBorder, image 0x40d0.
        let probe = step.x + limit::REACH * step.facing;
        if probe < limit::X_LOW {
            ok &= !dir::LEFT;
            step.x = limit::X_LOW;
        }
        if probe > limit::X_HIGH {
            ok &= !dir::RIGHT;
            step.x = limit::X_HIGH;
        }
        let depth = step.y - limit::DEPTH_LEAD;
        if depth > limit::DEPTH_HIGH {
            ok &= !dir::DOWN;
        }
        if depth < limit::DEPTH_LOW {
            ok &= !dir::UP;
        }

        // ---- SBORD, image 0x4552.
        let (bl, br) = (step.box_left + step.dx, step.box_right + step.dx);
        let reach = step.y + step.dy - limit::FEET_LEAD;
        for b in &self.borders {
            if !overlaps(b.left, b.right, bl, br) {
                continue;
            }
            // Inside the rectangle's own depth band: refuse the sideways step
            // that would carry the box further into it. Both comparisons are
            // the original's, and both are all but always true, which is why a
            // fighter on the ground is never stopped sideways by the tree line
            // that runs across the whole screen.
            if overlaps(
                limit::BAND_TOP,
                b.bottom,
                step.box_bottom,
                step.box_bottom + 1,
            ) {
                if step.facing < 0 {
                    if br >= step.box_left {
                        ok &= !dir::LEFT;
                    }
                } else if bl <= step.box_right {
                    ok &= !dir::RIGHT;
                }
            }
            // The one that matters: you may not walk up into it.
            if b.bottom >= reach {
                ok &= !dir::UP;
            }
        }
        ok
    }
}

/// One piece of scenery stamped onto the arena.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Prop {
    pub sheet: u8,
    pub cell: u8,
    pub x: i16,
    pub y: i16,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Arena {
    pub name: String,
    pub backdrop: String,
    pub sheets: Vec<String>,
    pub borders: Vec<Border>,
    pub props: Vec<Prop>,
}

impl Arena {
    /// Scenery and actors are drawn together, sorted by feet, so a fighter walking
    /// behind a rock is occluded by it and one walking in front covers it.
    pub fn draw_order(&self) -> Vec<usize> {
        let mut idx: Vec<usize> = (0..self.props.len()).collect();
        idx.sort_by_key(|&i| self.props[i].y);
        idx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `FO1.T`: one rectangle across the whole screen, the tree line at 119.
    fn fo1() -> Field {
        Field::new(vec![Border {
            left: 0,
            right: 319,
            bottom: 119,
            top: 10,
        }])
    }

    /// A knight, whose body box is eighteen wide and stands on his feet.
    fn knight_step(x: i32, y: i32, facing: i32, dx: i32, dy: i32) -> Step {
        Step {
            x,
            y,
            facing,
            dx,
            dy,
            box_left: x - 9,
            box_right: x + 9,
            box_bottom: y,
        }
    }

    #[test]
    fn the_tree_line_refuses_up_and_nothing_else() {
        let f = fo1();
        // Six rows below the tree line, which is where a knight walking up
        // comes to rest: the step up is still on.
        let mut s = knight_step(160, 126, 1, 0, -1);
        assert_eq!(f.allow(&mut s, dir::ALL), dir::ALL);
        // One row higher and up goes, and the other three stay: the tree line
        // is not a wall you slide along, it is a ceiling.
        let mut s = knight_step(160, 125, 1, 0, -1);
        assert_eq!(f.allow(&mut s, dir::ALL), dir::ALL & !dir::UP);
        // Deep in the canopy, where the old clamp rectangle used to put him:
        // up goes, and so does the way he is facing, because his body box is
        // inside the rectangle's own band and `SBORD` will not carry it
        // further in.
        let mut s = knight_step(160, 90, 1, 0, -1);
        assert_eq!(f.allow(&mut s, dir::ALL), dir::DOWN | dir::LEFT);
    }

    #[test]
    fn the_global_depth_limits_are_the_ones_check_border_has() {
        let f = Field::default();
        // Anchor depth 146, feet at 198: the last row that may still go down.
        let mut s = knight_step(160, 198, 1, 0, 1);
        assert_eq!(f.allow(&mut s, dir::ALL), dir::ALL);
        let mut s = knight_step(160, 199, 1, 0, 1);
        assert_eq!(f.allow(&mut s, dir::ALL), dir::ALL & !dir::DOWN);
        // And 73 is the last that may still go up, which no arena ever
        // reaches because its own tree line stops him first.
        let mut s = knight_step(160, 73, 1, 0, -1);
        assert_eq!(f.allow(&mut s, dir::ALL), dir::ALL);
        let mut s = knight_step(160, 72, 1, 0, -1);
        assert_eq!(f.allow(&mut s, dir::ALL), dir::ALL & !dir::UP);
        assert_eq!((GLOBAL.top, GLOBAL.bottom), (73, 198));
    }

    #[test]
    fn the_twenty_five_pixel_probe_follows_the_facing() {
        let f = Field::default();
        // Facing left at 35 the probe is exactly on the limit and left stands.
        let mut s = knight_step(35, 150, -1, -2, 0);
        assert_eq!(f.allow(&mut s, dir::ALL), dir::ALL);
        assert_eq!(s.x, 35);
        // One pixel further and left goes, and the anchor is written back to
        // the limit itself rather than to where the probe stopped: that write
        // is the original's, and it is what makes 10 the resting column
        // rather than 35.
        let mut s = knight_step(34, 150, -1, -2, 0);
        assert_eq!(f.allow(&mut s, dir::ALL), dir::ALL & !dir::LEFT);
        assert_eq!(s.x, limit::X_LOW);
        // The same actor in the same column facing the other way is not
        // stopped at all: the probe is 34 + 25.
        let mut s = knight_step(34, 150, 1, 2, 0);
        assert_eq!(f.allow(&mut s, dir::ALL), dir::ALL);
        assert_eq!(s.x, 34);
        // Right, at the other end.
        let mut s = knight_step(295, 150, 1, 2, 0);
        assert_eq!(f.allow(&mut s, dir::ALL), dir::ALL);
        let mut s = knight_step(296, 150, 1, 2, 0);
        assert_eq!(f.allow(&mut s, dir::ALL), dir::ALL & !dir::RIGHT);
        assert_eq!(s.x, limit::X_HIGH);
    }

    /// `FO7.T` is one of the four layouts with more than one rectangle: the
    /// tree line at 91, and something between columns 66 and 164 that hangs
    /// down to 103.
    #[test]
    fn a_second_rectangle_bites_only_in_its_own_columns() {
        let f = Field::new(vec![
            Border {
                left: 0,
                right: 319,
                bottom: 91,
                top: 10,
            },
            Border {
                left: 66,
                right: 164,
                bottom: 103,
                top: 10,
            },
        ]);
        // Out on the right, on ground only the tree line covers: free, and
        // free to walk up as far as row 97.
        let mut s = knight_step(250, 105, 1, 0, -1);
        assert_eq!(f.allow(&mut s, dir::ALL), dir::ALL);
        // The same row under the deeper rectangle: up is gone and the other
        // three stay.
        let mut s = knight_step(120, 105, 1, 0, -1);
        assert_eq!(f.allow(&mut s, dir::ALL), dir::ALL & !dir::UP);
        // Two rows lower and up comes back.
        let mut s = knight_step(120, 110, 1, 0, -1);
        assert_eq!(f.allow(&mut s, dir::ALL), dir::ALL);
        assert_eq!(f.floor(), 103);
    }

    /// The sideways refusal, which only fires on a fighter whose body box is
    /// inside the rectangle's own band. Nothing on the ground ever is.
    #[test]
    fn a_rectangle_refuses_the_step_that_would_carry_a_box_further_into_it() {
        let f = Field::new(vec![Border {
            left: 66,
            right: 164,
            bottom: 103,
            top: 10,
        }]);
        // Standing in the canopy at the deeper rectangle's left edge, walking
        // left, box bottom inside the band 30..103.
        let mut s = knight_step(70, 80, -1, -2, 0);
        assert_eq!(f.allow(&mut s, dir::ALL) & dir::LEFT, 0);
        // Down on the grass the same column is free to walk either way.
        let mut s = knight_step(70, 130, -1, -2, 0);
        assert_eq!(f.allow(&mut s, dir::ALL) & dir::LEFT, dir::LEFT);
    }

    #[test]
    fn the_three_standing_places_are_measured_down_from_the_deepest_border() {
        let f = fo1();
        assert_eq!(f.floor(), 119);
        // 119 + (200 - 119) / 4 - 47, and its two neighbours, as anchors.
        assert_eq!(f.standing_depth(1), 92);
        assert_eq!(f.standing_depth(2), 112);
        assert_eq!(f.standing_depth(3), 132);
        // And the feet that go with them.
        assert_eq!(f.standing_row(2), 112 + limit::ANCHOR);
        // The three quarter place is the deepest of the three and its feet are
        // on row 184, which is inside the thirty two rows this engine used to
        // cover with a status strip of its own. That is what settles that the
        // strip was standing on ground a fighter arrives on.
        assert_eq!(f.standing_row(3), 184);
        assert!(f.standing_row(3) > 168);
    }

    #[test]
    fn add_knight_rotates_three_quarters_one_quarter_one_half_and_never_rests_on_zero() {
        let mut a = Arrivals::default();
        let picked: Vec<i32> = (0..7).map(|_| a.next_place()).collect();
        assert_eq!(picked, vec![3, 1, 2, 3, 1, 2, 3]);
        // `and [AddCNT], 3` then `je AddKnight`: the counter itself is only
        // ever 1, 2 or 3, so no arrival keeps the depth it was built with.
        assert_eq!(a.0, 1);
    }

    #[test]
    fn the_demon_narrows_the_list_rather_than_adding_to_it() {
        let mut f = fo1();
        f.narrow_to(Border {
            left: 0,
            right: 309,
            bottom: 99,
            top: 10,
        });
        assert_eq!(f.borders.len(), 1);
        assert_eq!(f.floor(), 99);
    }

    #[test]
    fn clamping_keeps_actors_inside_the_walkable_band() {
        let b = Bounds {
            left: 0,
            right: 319,
            top: 10,
            bottom: 114,
        };
        assert_eq!(b.clamp(-40, 200), (0, 114));
        assert_eq!(b.clamp(400, 0), (319, 10));
        assert!(b.contains(160, 60));
        assert!(!b.contains(160, 150));
    }

    #[test]
    fn inverted_bounds_do_not_panic() {
        let b = Bounds {
            left: 35584,
            right: 0,
            top: 0,
            bottom: 0,
        };
        assert_eq!(b.clamp(100, 100), (100, 0));
        assert!(!b.is_sane());
    }

    #[test]
    fn props_sort_back_to_front() {
        let a = Arena {
            name: "t".into(),
            backdrop: "b".into(),
            sheets: vec![],
            borders: vec![Border {
                left: 0,
                right: 319,
                bottom: 114,
                top: 10,
            }],
            props: vec![
                Prop {
                    sheet: 0,
                    cell: 0,
                    x: 0,
                    y: 90,
                },
                Prop {
                    sheet: 0,
                    cell: 1,
                    x: 0,
                    y: 30,
                },
            ],
        };
        assert_eq!(a.draw_order(), vec![1, 0]);
    }
}
