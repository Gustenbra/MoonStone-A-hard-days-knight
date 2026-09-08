//! `BattlePal`: how the original colours the two sides of a bout.
//!
//! A fight is drawn in thirty two colours, and the original does not have a
//! knight sheet per colour or a substitution table. It rewrites palette
//! entries. Every fighter's artwork is authored against fixed indices, and a
//! bout begins by writing the right colours into those indices:
//!
//! * the knight's armour is pixel indices 6, 7 and 8, and `MOON:ColourKnight`
//!   (image 0x480e) writes his three shades there, branching on the colour
//!   index at `+0x20` of his record;
//! * a second knight is drawn from `HE1.OB`..`HE3.OB`, which are the same
//!   figure painted in indices 9, 10 and 11, and `Colour2ndKnight` (0x4691)
//!   writes his shades there;
//! * a creature is authored against 9 upwards, and `ColourBeast`,
//!   `ColourRatmen`, `ColourTroggAxe`, `ColourBalok`, `ColourTroll`,
//!   `ColourDemon`, `ColourMudmen` and `ColourDragon` (0x466c to 0x47e3)
//!   write its colours there: seven words from 9 for most, six for the troll,
//!   twenty three for the demon, and the dragon adds three more at 29;
//! * the ground's own colours come last, from `ColourBackdrop` (0x4879),
//!   which the demon alone skips; then entry 0 is made black and entry 15
//!   is made `0xc00`, the blood red, for everyone but the dragon.
//!
//! The base the writes go over is the backdrop picture's own palette, which
//! the picture loader leaves at `DS:0x80bb` and `ColourBackDrop` (0x460f)
//! copies into `BattlePal` before dispatching on the creature code. The
//! numbers themselves are content, baked out of the image; this module is the
//! order they are applied in, which is the part that is code.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Colours in a palette. Five bitplanes, so thirty two.
pub const ENTRIES: usize = 32;

/// Where `ColourMainKnight` puts the first knight: `BattlePal + 12`, entry 6.
pub const MAIN_KNIGHT_AT: usize = 6;
/// Where `Colour2ndKnight`, and every creature, begins: `BattlePal + 18`, entry 9.
pub const SECOND_AT: usize = 9;
/// The knight's three shades, brightest first.
pub const SHADES: usize = 3;
/// The entry made blood red at the end of `ColourMainKnight`, and what it is made.
pub const BLOOD_AT: usize = 15;
pub const BLOOD: u16 = 0xc00;

/// A run of the original's `0x0RGB` words written from one entry. The colour
/// routines are nothing but a handful of these.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Write {
    /// First palette entry written.
    pub at: u8,
    /// The words, in entry order.
    pub words: Vec<u16>,
}

impl Write {
    fn apply(&self, pal: &mut [u16; ENTRIES]) {
        for (k, w) in self.words.iter().enumerate() {
            if let Some(slot) = pal.get_mut(self.at as usize + k) {
                *slot = *w;
            }
        }
    }
}

/// What one creature's colour routine writes.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct CreatureColours {
    /// Written on any ground the creature has no entry for in `by_family`.
    #[serde(default)]
    pub writes: Vec<Write>,
    /// Written instead of `writes` on one family's ground. `ColourTroggAxe`
    /// is the one routine that looks at the landscape code.
    #[serde(default)]
    pub by_family: BTreeMap<String, Vec<Write>>,
    /// `ColourBackdrop` returns without writing the ground's colours when
    /// `COLOURS` is this creature's code. The demon brings its own ground.
    #[serde(default)]
    pub keeps_ground: bool,
    /// Entry 15 is left as this creature wrote it rather than made red. The
    /// dragon's fire lives there.
    #[serde(default)]
    pub keeps_blood: bool,
}

impl CreatureColours {
    /// The writes this creature makes on a family's ground.
    pub fn on(&self, family: &str) -> &[Write] {
        self.by_family.get(family).map_or(&self.writes, Vec::as_slice)
    }
}

/// Everything `ColourBackDrop` and the routines it dispatches to know.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct BattleColours {
    /// `ColourKnight`: three shades per colour index, blue, gold, emerald,
    /// red and the fifth the computer's knights wear.
    #[serde(default)]
    pub knights: Vec<[u16; SHADES]>,
    /// `KnightGlowColours`: the brighter three each knight's entries pulse
    /// towards when he is down to ten health. Same order.
    #[serde(default)]
    pub glow: Vec<[u16; SHADES]>,
    /// `ColourBackdrop`: what each family's ground writes, by family id.
    #[serde(default)]
    pub ground: BTreeMap<String, Vec<Write>>,
    /// The creature routines, by actor id.
    #[serde(default)]
    pub creatures: BTreeMap<String, CreatureColours>,
}

/// Who is in the bout, as far as the palette cares.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sides<'a> {
    /// Colour index of the knight drawn in entries 6 to 8.
    pub main_knight: usize,
    /// Colour index of a knight drawn from the second knight's banks, in 9 to 11.
    pub second_knight: Option<usize>,
    /// Actor id of the creature in the bout, if there is one. A creature's
    /// block wins over a second knight, as `COLOURS` can only hold one code.
    pub creature: Option<&'a str>,
    /// The family whose ground the fight is on.
    pub family: &'a str,
}

impl BattleColours {
    /// The shades for a colour index. An index past the table wears the last
    /// entry, which is what `ColourKnight`'s final unguarded branch does with
    /// anything that is not 0 to 3.
    pub fn knight(&self, index: usize) -> Option<[u16; SHADES]> {
        self.knights.get(index).or_else(|| self.knights.last()).copied()
    }

    pub fn glow_for(&self, index: usize) -> Option<[u16; SHADES]> {
        self.glow.get(index).or_else(|| self.glow.last()).copied()
    }

    /// `BattlePal` for a bout, over the backdrop's own thirty two words.
    ///
    /// The order is `ColourBackDrop`'s: the creature's block, or the second
    /// knight's, then `ColourMainKnight`, which writes the first knight,
    /// the ground, black at 0 and red at 15.
    pub fn compose(&self, base: &[u16; ENTRIES], sides: &Sides) -> [u16; ENTRIES] {
        let mut pal = *base;
        let creature = sides.creature.and_then(|c| self.creatures.get(c));
        match (creature, sides.second_knight) {
            (Some(c), _) => {
                for w in c.on(sides.family) {
                    w.apply(&mut pal);
                }
            }
            (None, Some(index)) => {
                if let Some(shades) = self.knight(index) {
                    Write { at: SECOND_AT as u8, words: shades.to_vec() }.apply(&mut pal);
                }
            }
            (None, None) => {}
        }
        if let Some(shades) = self.knight(sides.main_knight) {
            Write { at: MAIN_KNIGHT_AT as u8, words: shades.to_vec() }.apply(&mut pal);
        }
        if !creature.is_some_and(|c| c.keeps_ground) {
            if let Some(ground) = self.ground.get(sides.family) {
                for w in ground {
                    w.apply(&mut pal);
                }
            }
        }
        pal[0] = 0;
        if !creature.is_some_and(|c| c.keeps_blood) {
            pal[BLOOD_AT] = BLOOD;
        }
        pal
    }

    /// The entries a creature's block covers on a family's ground, for
    /// whoever wants to name the creature by one of its own colours.
    pub fn creature_entries(&self, actor: &str, family: &str) -> Vec<usize> {
        let mut out = Vec::new();
        if let Some(c) = self.creatures.get(actor) {
            for w in c.on(family) {
                out.extend((0..w.words.len()).map(|k| w.at as usize + k));
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}

/// The original's `0x0RGB` to 0xRRGGBB, by the nibble times seventeen the
/// picture decoders use, so a word from the executable and a word from a
/// picture come out the same.
pub fn widen(w: u16) -> u32 {
    let ch = |v: u16| ((v & 0xf) as u32) * 17;
    (ch(w >> 8) << 16) | (ch(w >> 4) << 8) | ch(w)
}

/// 0xRRGGBB back to `0x0RGB`. Exact for anything `widen` produced.
pub fn narrow(c: u32) -> u16 {
    let ch = |v: u32| (((v & 0xff) * 15 + 127) / 255) as u16;
    (ch(c >> 16) << 8) | (ch(c >> 8) << 4) | ch(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The recovered tables, as the baker writes them, enough of them to
    /// exercise every branch of the order.
    fn colours() -> BattleColours {
        let w = |at: u8, words: &[u16]| Write { at, words: words.to_vec() };
        let mut creatures = BTreeMap::new();
        creatures.insert(
            "trogg_axe".to_string(),
            CreatureColours {
                writes: vec![w(9, &[0x500, 0x200, 0x000, 0xb40, 0x610, 0x895, 0xc00])],
                by_family: BTreeMap::from([
                    ("waste".to_string(), vec![w(9, &[0x025, 0x004, 0x001, 0x830, 0x400, 0xf80, 0xc00])]),
                    ("glade".to_string(), vec![w(9, &[0x104, 0x102, 0x000, 0x600, 0x300, 0x693, 0xc00])]),
                ]),
                ..Default::default()
            },
        );
        creatures.insert(
            "troll".to_string(),
            CreatureColours {
                writes: vec![w(9, &[0x55a, 0x347, 0x123, 0x001, 0xf00, 0x800])],
                ..Default::default()
            },
        );
        creatures.insert(
            "dragon".to_string(),
            CreatureColours {
                writes: vec![
                    w(9, &[0xc00, 0x976, 0x700, 0x500, 0x754, 0xc30, 0xa00]),
                    w(29, &[0xfc0, 0xf80, 0xc50]),
                ],
                keeps_blood: true,
                ..Default::default()
            },
        );
        creatures.insert(
            "demon".to_string(),
            CreatureColours {
                writes: vec![w(9, &[0x0ef; 23])],
                keeps_ground: true,
                ..Default::default()
            },
        );
        BattleColours {
            knights: vec![
                [0x00a, 0x007, 0x004],
                [0xf80, 0xc50, 0xa30],
                [0x8c6, 0x593, 0x251],
                [0xf22, 0xb22, 0x700],
                [0x206, 0x103, 0x001],
            ],
            glow: vec![
                [0x00c, 0x009, 0x006],
                [0xfa0, 0xe70, 0xc50],
                [0xae8, 0x6b5, 0x473],
                [0xd00, 0x900, 0x500],
                [0x408, 0x305, 0x003],
            ],
            ground: BTreeMap::from([
                ("forest".to_string(), vec![w(16, &[0x210, 0x321, 0x532])]),
                ("waste".to_string(), vec![w(1, &[0xffd, 0x998, 0x776, 0x443]), w(16, &[0x322, 0x432])]),
            ]),
            creatures,
        }
    }

    /// A backdrop palette with a recognisable word in every entry, so any
    /// entry the composition ought to leave alone can be seen to be left.
    fn backdrop() -> [u16; ENTRIES] {
        let mut b = [0u16; ENTRIES];
        for (i, e) in b.iter_mut().enumerate() {
            *e = 0x111 * (i as u16 % 15 + 1);
        }
        b
    }

    fn sides<'a>(main: usize, second: Option<usize>, creature: Option<&'a str>, family: &'a str) -> Sides<'a> {
        Sides { main_knight: main, second_knight: second, creature, family }
    }

    /// The gold knight is `0xf80`, `0xc50`, `0xa30` at 6, 7 and 8 on every
    /// ground, whatever the backdrop had there. That is the whole point.
    #[test]
    fn the_knight_lives_in_entries_six_to_eight() {
        let c = colours();
        for family in ["forest", "waste", "glade", "swamp"] {
            for creature in [None, Some("trogg_axe"), Some("troll")] {
                let pal = c.compose(&backdrop(), &sides(1, None, creature, family));
                assert_eq!(&pal[6..9], &[0xf80, 0xc50, 0xa30], "{family} {creature:?}");
            }
        }
        let pal = c.compose(&backdrop(), &sides(0, None, None, "forest"));
        assert_eq!(&pal[6..9], &[0x00a, 0x007, 0x004]);
        let pal = c.compose(&backdrop(), &sides(3, None, None, "forest"));
        assert_eq!(&pal[6..9], &[0xf22, 0xb22, 0x700]);
    }

    /// `ColourKnight`'s last branch is unguarded, so a computer knight and
    /// anything else past the four wears the fifth triple.
    #[test]
    fn a_fifth_or_later_index_wears_the_computers_purple() {
        let c = colours();
        for index in [4, 5, 40] {
            let pal = c.compose(&backdrop(), &sides(index, None, None, "forest"));
            assert_eq!(&pal[6..9], &[0x206, 0x103, 0x001]);
        }
    }

    /// A second knight goes to 9, 10 and 11 and nowhere else; entries 12 to
    /// 14 keep the backdrop's words.
    #[test]
    fn a_second_knight_lives_in_nine_to_eleven() {
        let c = colours();
        let base = backdrop();
        let pal = c.compose(&base, &sides(1, Some(2), None, "forest"));
        assert_eq!(&pal[6..9], &[0xf80, 0xc50, 0xa30]);
        assert_eq!(&pal[9..12], &[0x8c6, 0x593, 0x251]);
        assert_eq!(&pal[12..15], &base[12..15]);
    }

    /// The trogg is the one creature coloured by the ground: one block on the
    /// waste, another on the glade, a third everywhere else.
    #[test]
    fn a_trogg_is_coloured_by_the_ground_it_stands_on() {
        let c = colours();
        let on = |family| c.compose(&backdrop(), &sides(1, None, Some("trogg_axe"), family));
        assert_eq!(&on("forest")[9..16], &[0x500, 0x200, 0x000, 0xb40, 0x610, 0x895, 0xc00]);
        assert_eq!(&on("swamp")[9..16], &[0x500, 0x200, 0x000, 0xb40, 0x610, 0x895, 0xc00]);
        assert_eq!(&on("waste")[9..16], &[0x025, 0x004, 0x001, 0x830, 0x400, 0xf80, 0xc00]);
        assert_eq!(&on("glade")[9..16], &[0x104, 0x102, 0x000, 0x600, 0x300, 0x693, 0xc00]);
    }

    /// The creature's block wins over a second knight: `COLOURS` holds one
    /// code, and a creature fight is never a knight fight.
    #[test]
    fn a_creature_takes_the_second_knights_entries() {
        let c = colours();
        let pal = c.compose(&backdrop(), &sides(1, Some(0), Some("troll"), "forest"));
        assert_eq!(&pal[9..12], &[0x55a, 0x347, 0x123]);
    }

    /// `ColourTroll` writes six words, so entry 15 is the red every fight
    /// ends on and not something of the troll's.
    #[test]
    fn the_troll_leaves_fifteen_to_the_blood() {
        let c = colours();
        let pal = c.compose(&backdrop(), &sides(1, None, Some("troll"), "forest"));
        assert_eq!(&pal[9..15], &[0x55a, 0x347, 0x123, 0x001, 0xf00, 0x800]);
        assert_eq!(pal[15], BLOOD);
    }

    /// Entry 0 is black and 15 is `0xc00` whoever is fighting, except that
    /// the dragon keeps its own 15, and it alone writes 29 to 31.
    #[test]
    fn black_at_zero_and_red_at_fifteen_except_for_the_dragon() {
        let c = colours();
        let base = backdrop();
        for creature in [None, Some("trogg_axe"), Some("troll"), Some("demon")] {
            let pal = c.compose(&base, &sides(1, None, creature, "forest"));
            assert_eq!(pal[0], 0, "{creature:?}");
            assert_eq!(pal[15], BLOOD, "{creature:?}");
        }
        for creature in [None, Some("trogg_axe"), Some("troll")] {
            let pal = c.compose(&base, &sides(1, None, creature, "forest"));
            assert_eq!(&pal[29..32], &base[29..32], "{creature:?} touched the top three");
        }
        let pal = c.compose(&base, &sides(1, None, Some("dragon"), "forest"));
        assert_eq!(pal[0], 0);
        assert_eq!(pal[15], 0xa00);
        assert_eq!(&pal[29..32], &[0xfc0, 0xf80, 0xc50]);
    }

    /// The ground's colours go in after the creature's, so a mudman's twenty
    /// three words would be trimmed to the ground's; the demon is the one
    /// creature `ColourBackdrop` steps aside for.
    #[test]
    fn the_ground_is_written_last_and_the_demon_keeps_its_own() {
        let c = colours();
        let base = backdrop();
        let pal = c.compose(&base, &sides(1, None, None, "waste"));
        assert_eq!(&pal[1..5], &[0xffd, 0x998, 0x776, 0x443]);
        assert_eq!(&pal[16..18], &[0x322, 0x432]);
        assert_eq!(pal[18], base[18], "the waste writes two words here, not three");
        let pal = c.compose(&base, &sides(1, None, Some("demon"), "waste"));
        assert_eq!(&pal[9..15], &[0x0ef; 6]);
        assert_eq!(pal[15], BLOOD, "the red at 15 is written even over the demon's own");
        assert_eq!(&pal[16..32], &[0x0ef; 16], "the demon's ground reaches to the top entry");
        assert_eq!(&pal[1..5], &base[1..5], "the demon's fight skips ColourBackdrop");
    }

    /// A family the table does not know writes no ground, and the backdrop's
    /// own colours stand. A pack with a fifth family is not a crash.
    #[test]
    fn an_unknown_family_keeps_the_backdrops_ground() {
        let c = colours();
        let base = backdrop();
        let pal = c.compose(&base, &sides(1, None, None, "moon"));
        assert_eq!(&pal[16..29], &base[16..29]);
        assert_eq!(&pal[6..9], &[0xf80, 0xc50, 0xa30]);
    }

    #[test]
    fn the_creatures_own_entries_are_named_by_ground() {
        let c = colours();
        assert_eq!(c.creature_entries("trogg_axe", "forest"), (9..16).collect::<Vec<_>>());
        assert_eq!(c.creature_entries("troll", "forest"), (9..15).collect::<Vec<_>>());
        let mut dragon = (9..16).collect::<Vec<_>>();
        dragon.extend(29..32);
        assert_eq!(c.creature_entries("dragon", "forest"), dragon);
        assert!(c.creature_entries("knight", "forest").is_empty());
    }

    /// The twelve bit words and the pictures' widened bytes are one and the
    /// same colour space, both ways round.
    #[test]
    fn widening_round_trips() {
        for w in [0x000u16, 0xf80, 0x00a, 0xfff, 0x8c6, 0xc00] {
            assert_eq!(narrow(widen(w)), w);
        }
        assert_eq!(widen(0xf80), 0xff8800);
        assert_eq!(widen(0x00a), 0x0000aa);
    }

    /// The table survives a trip through JSON as the pack stores it.
    #[test]
    fn the_table_is_plain_data() {
        let c = colours();
        let text = serde_json::to_string(&c).unwrap();
        let back: BattleColours = serde_json::from_str(&text).unwrap();
        assert_eq!(back, c);
        let empty: BattleColours = serde_json::from_str("{}").unwrap();
        let pal = empty.compose(&backdrop(), &sides(1, None, None, "forest"));
        assert_eq!(pal[0], 0);
        assert_eq!(pal[15], BLOOD);
        assert_eq!(&pal[6..9], &backdrop()[6..9], "no table, no knight colours");
    }
}
