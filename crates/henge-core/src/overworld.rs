//! The overworld: travel, the day cycle, and where a fight comes from.
//!
//! The original's map is a node graph living inside `MAIN.EXE`, which is not
//! recovered. Rather than invent a graph and pretend, this reads the terrain
//! straight off the map image: whatever you are standing on decides which kind
//! of arena an encounter drops you into. Walk into the swamp and you fight in
//! the swamp.
//!
//! That is a different mechanism from the original's, but it produces the same
//! thing the player experiences, and it needs no reverse engineering at all.

use crate::arena::Bounds;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Terrain {
    Forest,
    Glade,
    Swamp,
    Waste,
}

impl Terrain {
    /// The arena family an encounter here uses.
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

/// Classify a map pixel by colour.
///
/// Deliberately crude and deliberately readable: dense green is forest, pale
/// green is open ground, anything blue is swamp or water, and everything else,
/// which is rock and scree, is wasteland.
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

/// Pick the terrain a patch of map reads as.
///
/// The art is heavily dithered, so neighbouring pixels alternate between two or
/// three colours and any single sample is noise. Taking the majority over a
/// small patch recovers what the eye actually sees.
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

#[derive(Clone, Debug)]
pub struct Overworld {
    pub x: i32,
    pub y: i32,
    pub day: u32,
    /// Steps taken today. The original ties encounters and the moon to a day
    /// cycle; this keeps the same shape without guessing at its numbers.
    pub steps: u32,
    pub steps_per_day: u32,
    /// One in this many steps triggers an encounter.
    pub encounter_odds: u32,
    seed: u32,
}

impl Overworld {
    pub fn new(x: i32, y: i32) -> Overworld {
        Overworld {
            x, y, day: 1, steps: 0,
            steps_per_day: 220,
            encounter_odds: 90,
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

    /// Move, advance the clock, and report an encounter if one is triggered.
    pub fn travel(&mut self, dx: i32, dy: i32, bounds: Bounds) -> bool {
        if dx == 0 && dy == 0 {
            return false;
        }
        let (x, y) = bounds.clamp(self.x + dx, self.y + dy);
        let moved = x != self.x || y != self.y;
        self.x = x;
        self.y = y;
        if !moved {
            return false;
        }

        self.steps += 1;
        if self.steps >= self.steps_per_day {
            self.steps = 0;
            self.day += 1;
        }
        self.next_random() % self.encounter_odds == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn bounds() -> Bounds {
        Bounds { left: 0, right: 319, top: 0, bottom: 199 }
    }

    #[test]
    fn standing_still_costs_nothing() {
        let mut w = Overworld::new(10, 10);
        assert!(!w.travel(0, 0, bounds()));
        assert_eq!(w.steps, 0);
    }

    #[test]
    fn walking_into_the_edge_costs_nothing_either() {
        let mut w = Overworld::new(0, 10);
        assert!(!w.travel(-1, 0, bounds()));
        assert_eq!(w.steps, 0, "a blocked step must not advance the clock");
    }

    #[test]
    fn days_roll_over_after_enough_travel() {
        let mut w = Overworld::new(0, 100);
        w.steps_per_day = 5;
        for _ in 0..5 {
            w.travel(1, 0, bounds());
        }
        assert_eq!(w.day, 2);
        assert_eq!(w.steps, 0);
    }

    #[test]
    fn encounters_happen_but_not_constantly() {
        let mut w = Overworld::new(0, 100);
        w.encounter_odds = 20;
        let mut hits = 0;
        for i in 0..2000 {
            let dx = if (i / 100) % 2 == 0 { 1 } else { -1 };
            if w.travel(dx, 0, bounds()) {
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
            (0..500).filter(|i| w.travel(if (i / 50) % 2 == 0 { 1 } else { -1 }, 0, bounds())).count()
        };
        assert_eq!(run(), run());
    }
}
