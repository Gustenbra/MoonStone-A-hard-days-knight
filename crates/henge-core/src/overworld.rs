//! The overworld: travel, the day's distance, and the ground under your feet.
//!
//! **Recovered.** The original keeps the world in two byte grids inside
//! `MAIN.EXE`, both 40 columns wide over the 320x200 map picture, one cell to
//! every 8x8 block of it:
//!
//! * `MapType` says which of the four arena families the ground belongs to.
//! * `MapSLOW` says how hard that ground is to cross.
//!
//! `_MAP:FindLandscape` reads `MapType[index]` into the variable the arena
//! loader switches on, and `_MAP:CheckSLOW` reads `MapSLOW[index]` and uses it
//! as a bit mask against a counter. Both use the same index, built by
//! `_MAP:CalcKnGrid` and `_MAP:GetIndex`:
//!
//! ```text
//! grid_x = (x + w/2) >> 3        w, h = the map token's size, 8x10
//! grid_y = (y + h)   >> 3        x, y = the token's top-left on the map
//! index  = grid_y * 40 + grid_x
//! ```
//!
//! so the ground under a traveller is the ground under the middle of his feet.
//! With `y` bounded at 190 the row index reaches 25, which is why the terrain
//! grid is 26 rows and not 25.
//!
//! What `MapSLOW` holds, read out of the image at DS:`0xc42a`: every forest
//! cell but eight is mask 1, which refuses every other frame; the wastes are
//! 0, 2 and 3 (2 refuses two frames in four, 3 three in four); the swamp is
//! mostly open with patches of all three; and the map's own border, the top
//! row and the two side columns, is 3 and 2. There is no impassable ground.
//! The row past the table's end, which `CalcKnGrid` can index, is the first
//! row of `MapType`, and it reads 0 and 6.
//!
//! The day is a distance, not a clock: see [`Overworld::travel`].

use serde::{Deserialize, Serialize};

/// The map picture, and the token that walks over it.
///
/// `MAP.CMP` is one 320x200 picture and `MI.C` frame 0, the traveller's token,
/// is 8x10. `_MAP:HawkBorders` clamps the token's top-left corner with four
/// literal comparisons against 0, 0x136 and 0xbe, so the traveller lives in
/// `0..=310` across and `0..=190` down. Down, that is the token's height
/// exactly; across it stops two columns short of the right edge. Either way the
/// whole world fits one screen, which is what settles the scrolling question.
pub const MAP_W: i32 = 320;
pub const MAP_H: i32 = 200;
pub const TOKEN_W: i32 = 8;
pub const TOKEN_H: i32 = 10;
/// `cmp ax, 0x136` in `_MAP:HawkBorders`.
pub const MAX_X: i32 = 310;
/// `cmp bx, 0xbe` in `_MAP:HawkBorders`.
pub const MAX_Y: i32 = 190;

/// The terrain grid: 40 columns of 8 pixels, 26 rows of 8.
pub const GRID_COLS: usize = 40;
pub const GRID_ROWS: usize = 26;
pub const CELL: i32 = 8;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Terrain {
    Forest,
    Glade,
    Swamp,
    Waste,
}

impl Terrain {
    /// The code this terrain has in `MapType`.
    ///
    /// Recovered from `MOON:ColourBackdrop`, which compares the landscape
    /// variable against 0, 2, 4 and 6 and branches to `ColourPlains`,
    /// `ColourForest`, `ColourSwamp` and `ColourWastelands` in that order. The
    /// codes are even because the loader uses them as byte offsets into tables
    /// of words.
    pub fn code(self) -> u8 {
        match self {
            Terrain::Glade => 0,
            Terrain::Forest => 2,
            Terrain::Swamp => 4,
            Terrain::Waste => 6,
        }
    }

    /// Anything unrecognised reads as wasteland rather than panicking, because
    /// this is content and content can be wrong.
    pub fn from_code(code: u8) -> Terrain {
        match code {
            0 => Terrain::Glade,
            2 => Terrain::Forest,
            4 => Terrain::Swamp,
            _ => Terrain::Waste,
        }
    }

    /// The arena family an encounter here uses.
    ///
    /// The original calls this family "plain": its counter is `PLAINCOUNT` and
    /// its table `PlainTable`. Every file in it is named `GL*`, which is what
    /// this project named the family after, so the two names are the same thing.
    pub fn family(self) -> &'static str {
        match self {
            Terrain::Forest => "forest",
            Terrain::Glade => "glade",
            Terrain::Swamp => "swamp",
            Terrain::Waste => "waste",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Terrain::Forest => "forest",
            Terrain::Glade => "open ground",
            Terrain::Swamp => "swamp",
            Terrain::Waste => "wasteland",
        }
    }
}

/// The two recovered grids, as the pack carries them.
///
/// `going` is 1040 bytes rather than the 1000 `MapSLOW` occupies, because the
/// original indexes it with the same index it uses for the terrain grid and so
/// reads a row past its end. That last row is baked in with the rest rather
/// than special-cased, which is both simpler and what the game does.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Landscape {
    /// `MapType`: one family code per cell.
    pub terrain: Vec<u8>,
    /// `MapSLOW`: one delay mask per cell. Zero is open going.
    pub going: Vec<u8>,
}

impl Landscape {
    /// Ground with nothing on it: everything open, everything glade. For tests
    /// and for a pack with no grid.
    pub fn open() -> Landscape {
        Landscape {
            terrain: vec![Terrain::Glade.code(); GRID_COLS * GRID_ROWS],
            going: vec![0; GRID_COLS * GRID_ROWS],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.terrain.len() < GRID_COLS * GRID_ROWS
    }

    /// `_MAP:CalcKnGrid` and `_MAP:GetIndex`, exactly.
    pub fn index(x: i32, y: i32) -> usize {
        let col = ((x + TOKEN_W / 2) >> 3).clamp(0, GRID_COLS as i32 - 1) as usize;
        let row = ((y + TOKEN_H) >> 3).clamp(0, GRID_ROWS as i32 - 1) as usize;
        row * GRID_COLS + col
    }

    pub fn terrain_at(&self, x: i32, y: i32) -> Terrain {
        self.terrain
            .get(Landscape::index(x, y))
            .map_or(Terrain::Waste, |c| Terrain::from_code(*c))
    }

    /// The delay mask for the ground under the traveller.
    pub fn going_at(&self, x: i32, y: i32) -> u32 {
        self.going.get(Landscape::index(x, y)).copied().unwrap_or(0) as u32
    }
}

/// What one tick of travel did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Step {
    /// The traveller actually covered ground.
    pub moved: bool,
    /// The ground refused the step this tick. The clock still moved.
    pub bogged: bool,
    /// The step spent the last of the turn's distance, and the day turned.
    /// `_MAP:GoTheDistance` (0xa422) into `NextWHICH` (0xa434): the step is
    /// counted and the day is over before `FOLLOW` would have walked it, so
    /// the token did not move on this tick.
    pub turn_over: bool,
}

/// Where the traveller stands, how far he has walked today and how far he
/// may. Serializable because a save is a serialization of the simulation,
/// and where the traveller stands and what day it is are as much of it as
/// the purse.
///
/// **Recovered, and nothing here is rolled.** The original has no ambush on
/// the road. The map loop (`_MAP:MapLOOP`, 0xa306, to `DistanceDONE`, 0xa4b2)
/// calls nothing that rolls, and every fight the map can start is a thing the
/// token is standing on: `_MAP:StackDecision` (0xae9f) hands a rival knight
/// or his grave to `Combat+102` (0x3b7), a lair to `ClearCombat+22` (0x574)
/// and every other icon to `TakingMoon+45` (0xc7f), and it is only reached
/// from `ScrollINPUT` with fire held (`test ax, 0x10` at 0xa3c9). The dragon
/// (`DragonEncounter`, 0xa3e2) and a rival's challenge (`BKCollision`,
/// 0xaab1) are the two things that come to you, and both are other tokens.
/// `CHECKENCOUNTERS` and `ENCOUNTERAREA` in the public list carry no
/// address; the only encounter test the map has is `MOON:CheckGROOC` (0x653)
/// and the walk at 0x6b5 that `FOLLOW` calls, which overlap the token with
/// the icons and push what it stands on, and neither rolls. The one in
/// ninety roll per step and the two hundred and twenty step day that stood
/// here were ours, and are gone.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Overworld {
    /// The traveller's token, by its top-left corner on the map picture. This
    /// is the original's own convention: `knight[0x5c]` and `knight[0x5e]` are
    /// what `_MAP:SHOW` passes straight to the sprite blitter.
    pub x: i32,
    pub y: i32,
    /// Which day of the quest it is. The original keeps no such number; it
    /// keeps `[0x898b]`, the day of the moon, and `MoonCount`, both of which
    /// the run's [`crate::moon::Moon`] carries. This is the count of times
    /// the routine at 0x1148 has run, for the trace and the save to read.
    pub day: u32,
    /// `[0xcc98]`: the distance walked this turn. `_MAP:MapMovement` (0xa37e)
    /// does `inc word [0xcc98]` on every frame a direction is held, and it
    /// does so *before* it looks at `SlowFLAG` and before `FOLLOW` looks at
    /// the border, so a refused step and a step into the edge both count.
    pub steps: u32,
    /// `[0xccac]`: how far this turn may go. `DistanceDONE+12` (0xa4be)
    /// writes it at the start of every turn and on every return to the map:
    /// `mov al, [di+0x3e]; shl ax, 1` four times, then `shl` once more when
    /// `[0xcca2]` (haste) is set. So it is the knight's stride byte times
    /// sixteen, which [`crate::run::Run::day_steps`] computes; the caller
    /// writes it here the way `DistanceDONE+12` does, and it is zero until
    /// that has happened, which in the original is before the first frame.
    pub steps_per_day: u32,
    /// `_MAP:SlowDELAY` (DS:`0xcd4a`). Advances only while standing on ground
    /// that is not open, which is what makes the mask a rhythm rather than a
    /// probability.
    pub going_counter: u32,
}

impl Overworld {
    pub fn new(x: i32, y: i32) -> Overworld {
        Overworld {
            x,
            y,
            day: 1,
            steps: 0,
            steps_per_day: 0,
            going_counter: 0,
        }
    }

    /// A fingerprint of where the traveller is and when, for a save to check
    /// itself against.
    pub fn state_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for v in [
            self.x as i64,
            self.y as i64,
            self.day as i64,
            self.steps as i64,
            self.steps_per_day as i64,
            self.going_counter as i64,
        ] {
            h ^= v as u64;
            h = h.wrapping_mul(0x1000_0000_01b3);
        }
        h
    }

    /// Days that pass without a step: the turn a toad does not get.
    /// `_MAP:NextWHICH+89` (0xa48d) is `cmp byte [si+0x3a], 0; jg NextWHICH`,
    /// so a knight under the wizard's curse is passed over and the day turns
    /// without him.
    pub fn pass_days(&mut self, days: u32) {
        self.day += days;
        self.steps = 0;
    }

    /// `MOON:EncounterAllDone` (0x113e): `mov ax, [0xccac]; mov [0xcc98], ax`.
    ///
    /// Everything the map can send you into comes back through it: the end of
    /// `Combat` (0x38d, the same two moves), a village (`EncounterDone`,
    /// 0x1138, falls into it), a town (`CEXIT+6`, 0xe11), the wizard
    /// (`Wizard+9`, 0xcf0), the circle (`Henge+122`, 0x10cd), the Valley
    /// (`FightDemon+47` and `+108`) and the dragon (`_dragon_won`, 0xd32 and
    /// 0xd56). So whatever you did, the rest of the day's distance is spent,
    /// and the next frame's `GoTheDistance` turns the day.
    pub fn end_turn(&mut self) {
        self.steps = self.steps_per_day;
    }

    /// The terrain the traveller is standing on.
    pub fn terrain(&self, land: &Landscape) -> Terrain {
        land.terrain_at(self.x, self.y)
    }

    /// One frame of the map loop for the player's knight, in the original's
    /// order, which is `MapLOOP` (0xa306) from `PlayerKnight` (0xa355) round
    /// to `DistanceDONE` (0xa4b2).
    ///
    /// ```text
    /// PlayerKnight:
    /// 0a355  call CheckSLOW             ; every frame, held or not
    /// 0a358  sub bx, bx
    /// 0a35a  call 0x81ec                ; the stick, into bx
    /// MapMovement:
    /// 0a35d  mov [JOYS], bx
    /// 0a361  cmp word [0xcc9e], 0; jne  ; aloft: no step is counted
    /// 0a368  cmp word [0xcca0], 0; jne
    /// 0a36f  and bx, 0xf; je            ; no direction held: no step
    /// 0a374  mov si, [0x77e8]; cmp word [si+0x20], 4; je   ; not for a rival
    /// 0a37e  inc word [0xcc98]          ; the step is counted here...
    /// 0a382  cmp word [SlowFLAG], 0; je
    /// 0a389  mov word [JOYS], 0         ; ...and only then thrown away
    /// GoTheDistance:
    /// 0a422  mov ax, [0xcc98]
    /// 0a425  cmp ax, [0xccac]
    /// 0a429  jge NextWHICH              ; the day is over; nothing is walked
    /// DistanceDONE:
    /// 0a4b2  call FOLLOW                ; HawkBorders, then the move
    /// 0a4b5  call SHOW
    /// ```
    ///
    /// and `FOLLOW` (0xa29f) is `call HawkBorders` and then one `inc` or `dec`
    /// of `[si+0x5c]` and `[si+0x5e]` per direction bit still set, so a step
    /// into the edge is counted by `MapMovement` and then not walked.
    ///
    /// `CheckSLOW` (0xa728):
    ///
    /// ```text
    /// 0a728  mov word [SlowFLAG], 0
    /// 0a72e  cmp word [0xcca0], 0; jne ret     ; aloft: no slow ground
    /// 0a735  cmp word [0xcc9e], 0; jne ret
    /// 0a73c  call GetIndex                    ; CalcKnGrid, row * 40 + col
    /// 0a73f  mov si, MapSLOW; mov al, [bx+si]
    /// 0a744  or al, al; je ret                ; open going
    /// 0a749  inc word [SlowDELAY]
    /// 0a74d  mov dx, [SlowDELAY]; and dx, ax
    /// 0a753  je ret
    /// 0a755  mov word [SlowFLAG], 1
    /// ```
    ///
    /// Aloft is the caller's: both flags live with the flight, and a flight
    /// does not come through here.
    pub fn travel(&mut self, dx: i32, dy: i32, land: &Landscape) -> Step {
        // CheckSLOW, 0xa728.
        let mask = land.going_at(self.x, self.y);
        let slow = if mask != 0 {
            self.going_counter = self.going_counter.wrapping_add(1);
            self.going_counter & mask != 0
        } else {
            false
        };

        // MapMovement, 0xa36f: `and bx, 0xf; je`, then `inc word [0xcc98]`.
        let held = dx != 0 || dy != 0;
        if held {
            self.steps += 1;
        }

        // GoTheDistance, 0xa422: `cmp ax, [0xccac]; jge NextWHICH`.
        if self.steps >= self.steps_per_day {
            self.next_turn();
            return Step {
                moved: false,
                bogged: held && slow,
                turn_over: true,
            };
        }

        // 0a382: the held direction is dropped when SlowFLAG is up.
        if !held {
            return Step::default();
        }
        if slow {
            return Step {
                moved: false,
                bogged: true,
                turn_over: false,
            };
        }

        // FOLLOW, 0xa29f: HawkBorders clears the bit that would leave the
        // rectangle, and what is left moves the token one pixel.
        let (x, y) = ((self.x + dx).clamp(0, MAX_X), (self.y + dy).clamp(0, MAX_Y));
        let moved = x != self.x || y != self.y;
        self.x = x;
        self.y = y;
        Step {
            moved,
            bogged: false,
            turn_over: false,
        }
    }

    /// `_MAP:NextWHICH` (0xa434) for a board with one knight on it.
    ///
    /// ```text
    /// 0a434  call 0xa962                ; the three effect flags off
    /// 0a437  inc word [WHICH]
    /// 0a43b  mov word [NextFLAG], 0
    /// 0a441  mov word [0xcc98], 0       ; the distance walked
    /// 0a447  and word [WHICH], 3
    /// 0a44c  jne 0a463                  ; another knight's turn
    /// 0a44e  call 0x1148                ; the day turns
    /// ```
    ///
    /// With one seat the `and` always lands on zero, so every turn's end is
    /// a day's end. The routine at 0x1148 is the run's: `[0x898b]` and
    /// `MoonCount`, `GiveBK` and `AdjustTIME` are [`crate::run::Run::new_day`]
    /// and [`crate::moon::Moon::new_day`]; the haste flag `0xa962` clears is
    /// there too. Here only the count of days and the distance change.
    fn next_turn(&mut self) {
        self.steps = 0;
        self.day += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glade() -> Landscape {
        Landscape::open()
    }

    #[test]
    fn terrain_codes_are_the_ones_the_original_branches_on() {
        // MOON:ColourBackdrop compares against 0, 2, 4, 6 in this order.
        assert_eq!(Terrain::Glade.code(), 0);
        assert_eq!(Terrain::Forest.code(), 2);
        assert_eq!(Terrain::Swamp.code(), 4);
        assert_eq!(Terrain::Waste.code(), 6);
        for t in [
            Terrain::Glade,
            Terrain::Forest,
            Terrain::Swamp,
            Terrain::Waste,
        ] {
            assert_eq!(Terrain::from_code(t.code()), t);
        }
    }

    // The `1 *` is kept: every line below reads `row * GRID_COLS + col`, so the
    // row and the column of each expected cell can be checked against the
    // comment beside it. Folding the first two to a bare `GRID_COLS` would hide
    // which row they are in.
    #[allow(clippy::identity_op)]
    #[test]
    fn the_grid_index_is_the_middle_of_the_feet() {
        // CalcKnGrid: (x + 8/2) >> 3 and (y + 10) >> 3.
        assert_eq!(Landscape::index(0, 0), 1 * GRID_COLS);
        assert_eq!(Landscape::index(4, 0), 1 * GRID_COLS + 1);
        // Highwood sits at map (94, 47) and the original computes its cell as
        // (12, 7) in KnightGoesToTown. Ours has to agree.
        assert_eq!(Landscape::index(94, 47), 7 * GRID_COLS + 12);
        // Waterdeep at (297, 157) computes as (37, 20).
        assert_eq!(Landscape::index(297, 157), 20 * GRID_COLS + 37);
    }

    #[test]
    fn the_bottom_of_the_map_needs_the_twenty_sixth_row() {
        // HawkBorders lets y reach 190, and (190 + 10) >> 3 is 25.
        assert_eq!(Landscape::index(0, MAX_Y) / GRID_COLS, 25);
        // And row 25 has to be a row the grid really allocates, which is asked
        // of the grid rather than of the constant it was built from.
        let rows = Landscape::open().terrain.len() / GRID_COLS;
        assert!(rows > 25, "a {rows} row grid could not hold that cell");
    }

    #[test]
    fn terrain_comes_off_the_grid_when_there_is_one() {
        let mut land = Landscape::open();
        land.terrain[Landscape::index(100, 100)] = Terrain::Swamp.code();
        assert_eq!(land.terrain_at(100, 100), Terrain::Swamp);
        assert_eq!(land.terrain_at(0, 0), Terrain::Glade);
    }

    #[test]
    fn every_terrain_names_a_real_arena_family() {
        for t in [
            Terrain::Forest,
            Terrain::Glade,
            Terrain::Swamp,
            Terrain::Waste,
        ] {
            assert!(["forest", "glade", "swamp", "waste"].contains(&t.family()));
        }
    }

    /// A turn long enough that no test below runs into its end by accident.
    fn walker(x: i32, y: i32) -> Overworld {
        let mut w = Overworld::new(x, y);
        w.steps_per_day = 10_000;
        w
    }

    #[test]
    fn standing_still_costs_nothing() {
        let mut w = walker(10, 10);
        assert!(!w.travel(0, 0, &glade()).moved);
        assert_eq!(w.steps, 0);
    }

    /// `MapMovement` counts the step at 0xa37e and `FOLLOW` only then calls
    /// `HawkBorders`, so a step into the edge is paid for and not walked. This
    /// replaces a test that said the edge was free, which was ours.
    #[test]
    fn walking_into_the_edge_costs_a_step_and_goes_nowhere() {
        let mut w = walker(0, 10);
        let step = w.travel(-1, 0, &glade());
        assert!(!step.moved);
        assert!(!step.bogged);
        assert_eq!((w.x, w.y), (0, 10));
        assert_eq!(w.steps, 1, "inc word [0xcc98] comes before HawkBorders");
    }

    #[test]
    fn the_map_is_one_screen_and_the_token_stays_on_it() {
        let mut w = walker(0, 0);
        for _ in 0..500 {
            w.travel(1, 1, &glade());
        }
        assert_eq!((w.x, w.y), (MAX_X, MAX_Y));
        assert!(w.x + TOKEN_W <= MAP_W && w.y + TOKEN_H <= MAP_H);
    }

    #[test]
    fn days_roll_over_after_enough_travel() {
        let mut w = Overworld::new(0, 100);
        w.steps_per_day = 5;
        for _ in 0..5 {
            w.travel(1, 0, &glade());
        }
        assert_eq!(w.day, 2);
        assert_eq!(w.steps, 0);
    }

    /// `GoTheDistance` (0xa422) compares after `MapMovement` has counted and
    /// before `FOLLOW` has moved, so the step that reaches `[0xccac]` ends
    /// the day with the token where it was: a budget of five walks four.
    #[test]
    fn the_step_that_spends_the_distance_is_not_walked() {
        let mut w = Overworld::new(0, 100);
        w.steps_per_day = 5;
        let mut steps = Vec::new();
        for _ in 0..5 {
            steps.push(w.travel(1, 0, &glade()));
        }
        assert!(steps[..4].iter().all(|s| s.moved && !s.turn_over));
        assert!(!steps[4].moved && steps[4].turn_over);
        assert_eq!(w.x, 4);
        assert_eq!(w.day, 2);
    }

    /// `DistanceDONE+12` makes the budget `[di+0x3e] << 4`, so the opening
    /// knight's six is ninety six frames of held stick, and every refused or
    /// edge step is one of them.
    #[test]
    fn a_refused_step_and_an_edge_step_both_spend_the_distance() {
        let mut land = Landscape::open();
        for cell in land.going.iter_mut() {
            *cell = 1;
        }
        let mut w = Overworld::new(0, 100);
        w.steps_per_day = 96;
        let mut days = 0;
        for _ in 0..96 {
            if w.travel(-1, 0, &land).turn_over {
                days += 1;
            }
        }
        assert_eq!(days, 1, "ninety six held frames are one day");
        assert_eq!(w.x, 0, "and none of them went anywhere");
    }

    /// `EncounterAllDone` (0x113e) writes `[0xccac]` into `[0xcc98]`, and
    /// `GoTheDistance` runs on the next frame whether or not the stick is
    /// held, so coming back from anywhere ends the day on the spot.
    #[test]
    fn coming_back_from_an_encounter_ends_the_day() {
        let mut w = Overworld::new(50, 100);
        w.steps_per_day = 96;
        w.travel(1, 0, &glade());
        w.end_turn();
        assert_eq!(w.steps, 96);
        let step = w.travel(0, 0, &glade());
        assert!(step.turn_over);
        assert!(!step.moved);
        assert_eq!((w.day, w.steps), (2, 0));
        assert_eq!(w.x, 51, "the encounter cost the day, not the ground");
    }

    #[test]
    fn time_can_pass_without_walking() {
        let mut w = walker(10, 10);
        w.travel(1, 0, &glade());
        w.pass_days(3);
        assert_eq!(w.day, 4);
        assert_eq!(w.steps, 0, "a day spent indoors starts the next one fresh");
    }

    /// CheckSLOW: `SlowDELAY & mask` non-zero refuses the step. Mask 3 lets one
    /// tick in four through, mask 1 one in two.
    #[test]
    fn hard_going_costs_steps_without_covering_ground() {
        for (mask, want) in [(0u8, 40usize), (1, 20), (2, 20), (3, 10)] {
            let mut land = Landscape::open();
            for cell in land.going.iter_mut() {
                *cell = mask;
            }
            let mut w = walker(0, 100);
            let mut moved = 0;
            for _ in 0..40 {
                if w.travel(1, 0, &land).moved {
                    moved += 1;
                }
            }
            assert_eq!(moved, want, "mask {mask}");
            assert_eq!(w.steps, 40, "mask {mask}: every attempt still costs a step");
        }
    }

    #[test]
    fn open_ground_never_bogs_and_never_ticks_the_counter() {
        let mut w = walker(0, 100);
        for _ in 0..50 {
            assert!(!w.travel(1, 0, &glade()).bogged);
        }
        assert_eq!(w.going_counter, 0);
    }

    /// Nothing on the map is rolled, so two walks are the same walk. This
    /// replaces two tests of a seeded roll the original does not make.
    #[test]
    fn the_same_walk_is_the_same_journey() {
        let run = || {
            let mut w = Overworld::new(0, 100);
            w.steps_per_day = 96;
            for i in 0..500 {
                w.travel(if (i / 50) % 2 == 0 { 1 } else { -1 }, 0, &glade());
            }
            (w.x, w.y, w.day, w.steps, w.state_hash())
        };
        assert_eq!(run(), run());
    }
}
