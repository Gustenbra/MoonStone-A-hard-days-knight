//! The overworld: travel, the day cycle, and where a fight comes from.
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
//! The colour classifier below it is what this used before the tables were
//! recovered. It is kept only as a fallback for a pack that has no grid in it,
//! and it is never used when one is present.

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

/// Classify a map pixel by colour.
///
/// **Fallback, not the original's rule.** Used only when a pack carries no
/// recovered grid. Deliberately crude and deliberately readable: dense green is
/// forest, pale green is open ground, anything blue is swamp or water, and
/// everything else, which is rock and scree, is wasteland.
pub fn terrain_of(rgb: u32) -> Terrain {
    let (r, g, b) = ((rgb >> 16) & 0xff, (rgb >> 8) & 0xff, rgb & 0xff);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);

    if b > r && b >= g {
        return Terrain::Swamp;
    }
    if g > r && g > b {
        // Green. Dark and saturated reads as canopy; light reads as clearing.
        return if max < 130 { Terrain::Forest } else { Terrain::Glade };
    }
    if max - min < 30 && max > 150 {
        return Terrain::Glade; // pale, washed out ground
    }
    Terrain::Waste
}

/// Pick the terrain a patch of map reads as. Fallback, as above.
pub fn terrain_of_patch(samples: impl IntoIterator<Item = u32>) -> Terrain {
    let mut counts = [0u32; 4];
    for rgb in samples {
        counts[match terrain_of(rgb) {
            Terrain::Forest => 0,
            Terrain::Glade => 1,
            Terrain::Swamp => 2,
            Terrain::Waste => 3,
        }] += 1;
    }
    let best = counts
        .iter()
        .enumerate()
        .max_by_key(|(_, n)| **n)
        .map(|(i, _)| i)
        .unwrap_or(3);
    [Terrain::Forest, Terrain::Glade, Terrain::Swamp, Terrain::Waste][best]
}

/// What one tick of travel did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Step {
    /// The traveller actually covered ground.
    pub moved: bool,
    /// The ground refused the step this tick. The clock still moved.
    pub bogged: bool,
    /// Something jumped out.
    pub encounter: bool,
}

/// Serializable because a save is a serialization of the simulation, and where
/// the traveller stands and what day it is are as much of it as the purse:
/// `seed` included, so a reloaded run is robbed on the same step a continued
/// one would have been.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Overworld {
    /// The traveller's token, by its top-left corner on the map picture. This
    /// is the original's own convention: `knight[0x5c]` and `knight[0x5e]` are
    /// what `_MAP:SHOW` passes straight to the sprite blitter.
    pub x: i32,
    pub y: i32,
    pub day: u32,
    /// Steps taken today. The original ties encounters and the moon to a day
    /// cycle; this keeps the same shape without guessing at its numbers.
    pub steps: u32,
    pub steps_per_day: u32,
    /// One in this many steps triggers an encounter.
    pub encounter_odds: u32,
    /// `_MAP:SlowDELAY`. Advances only while standing on ground that is not
    /// open, which is what makes the mask a rhythm rather than a probability.
    pub going_counter: u32,
    seed: u32,
}

impl Overworld {
    pub fn new(x: i32, y: i32) -> Overworld {
        Overworld {
            x, y, day: 1, steps: 0,
            steps_per_day: 220,
            encounter_odds: 90,
            going_counter: 0,
            seed: 0x1a2b_3c4d,
        }
    }

    /// Small xorshift. Deterministic, seedable, and enough for encounter rolls.
    fn next_random(&mut self) -> u32 {
        let mut s = self.seed;
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        self.seed = s;
        s
    }

    pub fn set_seed(&mut self, seed: u32) {
        self.seed = seed | 1;
    }

    /// A fingerprint of where the traveller is and when, for a save to check
    /// itself against. The seed goes in with the rest, because two travellers
    /// standing on the same square with different seeds are not in the same
    /// place in the same game.
    pub fn state_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for v in [
            self.x as i64, self.y as i64, self.day as i64, self.steps as i64,
            self.steps_per_day as i64, self.encounter_odds as i64,
            self.going_counter as i64, self.seed as i64,
        ] {
            h ^= v as u64;
            h = h.wrapping_mul(0x1000_0000_01b3);
        }
        h
    }

    /// Days spent standing still: under a healer, or waiting somewhere out of
    /// the rain. Travel is not the only thing that moves the calendar.
    pub fn pass_days(&mut self, days: u32) {
        self.day += days;
        self.steps = 0;
    }

    /// The terrain the traveller is standing on.
    pub fn terrain(&self, land: &Landscape) -> Terrain {
        land.terrain_at(self.x, self.y)
    }

    /// Move, advance the clock, and report what happened.
    ///
    /// The order is the original's. `_MAP:CheckSLOW` runs every tick, whether
    /// or not a direction is held, so the delay counter keeps its rhythm while
    /// you stand still in a bog. `_MAP:HawkBorders` cancels a direction that
    /// would leave the map *before* the step is counted, which is why walking
    /// into the edge costs nothing at all. A step that the ground refuses still
    /// costs the day: `_MAP:MapMovement` increments its step counter and only
    /// then throws the direction away.
    pub fn travel(&mut self, dx: i32, dy: i32, land: &Landscape) -> Step {
        let mask = land.going_at(self.x, self.y);
        let bogged = if mask != 0 {
            self.going_counter = self.going_counter.wrapping_add(1);
            self.going_counter & mask != 0
        } else {
            false
        };

        if dx == 0 && dy == 0 {
            return Step::default();
        }
        let (x, y) = (
            (self.x + dx).clamp(0, MAX_X),
            (self.y + dy).clamp(0, MAX_Y),
        );
        if x == self.x && y == self.y {
            return Step::default();
        }

        self.steps += 1;
        if self.steps >= self.steps_per_day {
            self.steps = 0;
            self.day += 1;
        }
        if bogged {
            return Step { moved: false, bogged: true, encounter: false };
        }
        self.x = x;
        self.y = y;
        Step {
            moved: true,
            bogged: false,
            encounter: self.next_random() % self.encounter_odds == 0,
        }
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
        for t in [Terrain::Glade, Terrain::Forest, Terrain::Swamp, Terrain::Waste] {
            assert_eq!(Terrain::from_code(t.code()), t);
        }
    }

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
        assert!(GRID_ROWS > 25, "a 25 row grid could not hold that cell");
    }

    #[test]
    fn terrain_comes_off_the_grid_when_there_is_one() {
        let mut land = Landscape::open();
        land.terrain[Landscape::index(100, 100)] = Terrain::Swamp.code();
        assert_eq!(land.terrain_at(100, 100), Terrain::Swamp);
        assert_eq!(land.terrain_at(0, 0), Terrain::Glade);
    }

    #[test]
    fn colours_classify_the_way_the_map_reads() {
        assert_eq!(terrain_of(0x1e5a22), Terrain::Forest, "dark canopy green");
        assert_eq!(terrain_of(0x8fd45a), Terrain::Glade, "pale open green");
        assert_eq!(terrain_of(0x3a6fa8), Terrain::Swamp, "water blue");
        assert_eq!(terrain_of(0x8a5a34), Terrain::Waste, "rock brown");
        assert_eq!(terrain_of(0x000000), Terrain::Waste, "anything else");
    }

    #[test]
    fn a_dithered_patch_reads_as_its_majority() {
        // Canopy dithered with a few pale and rocky pixels is still forest.
        let patch = [0x1e5a22, 0x1e5a22, 0x8fd45a, 0x1e5a22, 0x8a5a34, 0x1e5a22];
        assert_eq!(terrain_of_patch(patch), Terrain::Forest);
        // An even mix of water and canopy still has to answer with one of them.
        let mixed = [0x3a6fa8, 0x3a6fa8, 0x3a6fa8, 0x1e5a22];
        assert_eq!(terrain_of_patch(mixed), Terrain::Swamp);
    }

    #[test]
    fn every_terrain_names_a_real_arena_family() {
        for t in [Terrain::Forest, Terrain::Glade, Terrain::Swamp, Terrain::Waste] {
            assert!(["forest", "glade", "swamp", "waste"].contains(&t.family()));
        }
    }

    #[test]
    fn standing_still_costs_nothing() {
        let mut w = Overworld::new(10, 10);
        assert!(!w.travel(0, 0, &glade()).moved);
        assert_eq!(w.steps, 0);
    }

    #[test]
    fn walking_into_the_edge_costs_nothing_either() {
        let mut w = Overworld::new(0, 10);
        assert!(!w.travel(-1, 0, &glade()).moved);
        assert_eq!(w.steps, 0, "a blocked step must not advance the clock");
    }

    #[test]
    fn the_map_is_one_screen_and_the_token_stays_on_it() {
        let mut w = Overworld::new(0, 0);
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

    #[test]
    fn time_can_pass_without_walking() {
        let mut w = Overworld::new(10, 10);
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
            let mut w = Overworld::new(0, 100);
            w.steps_per_day = 10_000;
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
        let mut w = Overworld::new(0, 100);
        for _ in 0..50 {
            assert!(!w.travel(1, 0, &glade()).bogged);
        }
        assert_eq!(w.going_counter, 0);
    }

    #[test]
    fn encounters_happen_but_not_constantly() {
        let mut w = Overworld::new(0, 100);
        w.encounter_odds = 20;
        let mut hits = 0;
        for i in 0..2000 {
            let dx = if (i / 100) % 2 == 0 { 1 } else { -1 };
            if w.travel(dx, 0, &glade()).encounter {
                hits += 1;
            }
        }
        assert!(hits > 40 && hits < 160, "expected roughly 1 in 20, got {hits} in 2000");
    }

    #[test]
    fn the_same_seed_gives_the_same_journey() {
        let run = || {
            let mut w = Overworld::new(0, 100);
            w.set_seed(7);
            (0..500)
                .filter(|i| w.travel(if (i / 50) % 2 == 0 { 1 } else { -1 }, 0, &glade()).encounter)
                .count()
        };
        assert_eq!(run(), run());
    }
}
