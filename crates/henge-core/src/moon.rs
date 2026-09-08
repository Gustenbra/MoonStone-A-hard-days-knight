//! The moon the game is named after: an eight step cycle, four days to a step,
//! and the things on the map that answer to it.
//!
//! **Recovered.** The whole calendar is `MOON:EncounterFini`, which `_MAP:NextWHICH`
//! calls once every fourth turn, that is once each time all four knights have
//! moved, and then puts up the `Next Day` screen:
//!
//! ```text
//! [0x898b] += 1                     ; days since the moon last moved
//! if [0x898b] <= 3: AdjustTIME      ; four days to a phase
//! [0x5b1] += 1                      ; how many times the moon has moved
//! [0x898b] = 0
//! MoonCount = (MoonCount + 1) & 7   ; eight steps to the cycle
//! GiveBK()                          ; the Black Knights take their turn
//! [0x8989] = Moons[MoonCount]       ; tonight's moon, as a cel number
//! AdjustTIME()                      ; everyone mends, and the wizard forgets
//! ```
//!
//! So a day is a round of the map, **four days move the moon one step and
//! eight steps close the cycle**: thirty two days. `MOON:InitGameStart` writes
//! 0x2d into the phase before anything else runs, so a quest opens on the full
//! moon.
//!
//! **The phases are pictures, and they were checked by looking.** `[0x8989]` is
//! handed straight to the blitter by the `Next Day` routine at image 0x8e5b,
//! which draws that cel of `KI.CEL` at (119, 12) over `CH.PIV`, the night sky
//! the select screen also uses. Cels 0x2d to 0x31 are five 57x44 moons; drawn
//! in that screen's own palette they are a full moon, a waning gibbous, a half,
//! a crescent and a thin sliver, the shadow on the right of each. The three
//! constants the rest of the game compares against, 0x2d, 0x2e and 0x31, are
//! three of those five.
//!
//! **What is not recovered is the table.** `MOON:Moons` sits at DS:05a9, inside
//! the 2,906 bytes of DGROUP the load image carries as a stale duplicate, so its
//! eight bytes cannot be read. [`CYCLE`] is therefore **ours**: five pictures
//! over eight steps, starting on the full moon, waning to the sliver and waxing
//! back, is the one arrangement that uses every picture and returns to where it
//! began. Everything else in this file was read out of the executable.
//!
//! **What the moon gates**, all of it recovered:
//!
//! * `SetRatmenTables` reads the phase before a ratman fight: five hit points
//!   and a slash of one on most nights, seven and three under 0x2d, twelve and
//!   five under 0x31. That lives on the ratman's own definition in the pack
//!   ([`crate::content::ActorDef::moon`]) rather than here.
//! * `MOON:CalcDamage` doubles a knight's blow while he carries the moonstone
//!   whose night it is. The moonstones are the quest's business and do not
//!   exist yet; [`Moonstone`] says which night each answers to so that when
//!   they do, nothing here has to change.
//! * `MOON:Henge` ends the game for a knight standing in the circle with the
//!   moonstone of the night.
//!
//! Also recovered, and worth writing down because the game contradicts itself:
//! the between-days hint says the ratmen "grow stronger as the moon gets
//! fuller", and the code makes them strongest on the 0x31 night, which the
//! picture shows as the thin sliver and the programmer's own label calls
//! `RatNewMoon`. The code is what runs, so the code is what is reproduced.

use serde::{Deserialize, Serialize};

/// A phase of the moon, named by the `KI.CEL` cel that draws it.
///
/// The numbers are the original's own: what `MOON:InitGameStart` writes, what
/// `SetRatmenTables` and `CalcDamage` compare against, and what the `Next Day`
/// screen hands to the blitter.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    Full = 0x2d,
    Gibbous = 0x2e,
    Half = 0x2f,
    Crescent = 0x30,
    New = 0x31,
}

impl Phase {
    pub const ALL: [Phase; 5] =
        [Phase::Full, Phase::Gibbous, Phase::Half, Phase::Crescent, Phase::New];

    /// The cel of `KI.CEL` that draws this phase, which is also the value the
    /// original stores and compares.
    pub fn cel(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            Phase::Full => "Full moon",
            Phase::Gibbous => "Gibbous moon",
            Phase::Half => "Half moon",
            Phase::Crescent => "Crescent moon",
            Phase::New => "New moon",
        }
    }

    /// The key an actor definition's moon table is written under.
    pub fn key(self) -> &'static str {
        match self {
            Phase::Full => "full",
            Phase::Gibbous => "gibbous",
            Phase::Half => "half",
            Phase::Crescent => "crescent",
            Phase::New => "new",
        }
    }

    pub fn from_cel(cel: usize) -> Option<Phase> {
        Phase::ALL.into_iter().find(|p| p.cel() == cel)
    }
}

/// How many days the moon stays on one step. `cmp word ptr [0x898b], 3` then
/// `jle`, so the fourth day is the one that moves it.
pub const DAYS_PER_PHASE: u32 = 4;

/// The eight steps of the cycle. `and word ptr [MoonCount], 7`.
///
/// **Ours, not recovered.** See the module docs: the eight bytes live in the
/// part of DGROUP the load image does not carry. What is recovered is that
/// there are eight of them, that they are cel numbers, that five such cels
/// exist, and that the quest opens on [`Phase::Full`].
pub const CYCLE: [Phase; 8] = [
    Phase::Full,
    Phase::Gibbous,
    Phase::Half,
    Phase::Crescent,
    Phase::New,
    Phase::Crescent,
    Phase::Half,
    Phase::Gibbous,
];

/// Where the calendar has got to.
///
/// Two counters and nothing else, which is all the original keeps: the days
/// since the moon last moved, and its place in the eight.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Moon {
    /// Days on this step so far, `0..DAYS_PER_PHASE`. The original's `[0x898b]`.
    pub day: u32,
    /// `MoonCount`, masked to three bits.
    pub count: u32,
}

impl Moon {
    pub fn new() -> Moon {
        Moon::default()
    }

    /// Tonight's moon.
    pub fn phase(self) -> Phase {
        CYCLE[(self.count & 7) as usize]
    }

    /// A day turned over. Returns whether the moon moved with it, which is
    /// what the `Next Day` screen has to show a new picture for.
    pub fn new_day(&mut self) -> bool {
        self.day += 1;
        if self.day < DAYS_PER_PHASE {
            return false;
        }
        self.day = 0;
        self.count = (self.count + 1) & 7;
        true
    }

    /// Days until the moon next moves.
    pub fn days_left(self) -> u32 {
        DAYS_PER_PHASE - self.day.min(DAYS_PER_PHASE)
    }
}

/// One of the four lair keys.
///
/// **Recovered.** The keys are bits in byte `+0x14` of a knight's item record,
/// the one `_STATUS:StatCheckKeys` draws four icons for and `MOON:Valley`
/// wants all four of (`cmp byte ptr [si+0x14], 0xf`). The lair initialiser
/// plants them one per family, walking the 24 lair records six at a time in
/// `LairFile` order: forest, waste, swamp, glade, with the bits 8, 4, 2, 1.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum Key {
    Forest,
    Waste,
    Swamp,
    Glade,
}

impl Key {
    /// In the order the initialiser plants them, which is `LairFile` order.
    pub const ALL: [Key; 4] = [Key::Forest, Key::Waste, Key::Swamp, Key::Glade];

    /// The bit the original sets in the item record.
    pub fn bit(self) -> u8 {
        match self {
            Key::Forest => 8,
            Key::Waste => 4,
            Key::Swamp => 2,
            Key::Glade => 1,
        }
    }

    /// The terrain family whose six lairs hide this key.
    pub fn family(self) -> &'static str {
        match self {
            Key::Forest => "forest",
            Key::Waste => "waste",
            Key::Swamp => "swamp",
            Key::Glade => "glade",
        }
    }

    pub fn from_family(family: &str) -> Option<Key> {
        Key::ALL.into_iter().find(|k| k.family() == family)
    }

    /// The item id a carried key has in the pack. The original keeps one
    /// slot, `Key to the Valley`, with four bits in it; a pack that carries
    /// items by id needs four ids.
    pub fn item(self) -> &'static str {
        match self {
            Key::Forest => "key.forest",
            Key::Waste => "key.waste",
            Key::Swamp => "key.swamp",
            Key::Glade => "key.glade",
        }
    }

    pub fn from_item(id: &str) -> Option<Key> {
        Key::ALL.into_iter().find(|k| k.item() == id)
    }

    pub fn name(self) -> &'static str {
        match self {
            Key::Forest => "Key of the forest",
            Key::Waste => "Key of the wastes",
            Key::Swamp => "Key of the marsh",
            Key::Glade => "Key of the glades",
        }
    }
}

/// One of the four moonstones, and the night it answers to.
///
/// **Recovered, and the original disagrees with itself.** A moonstone is a
/// bit in byte `+0x16` of the item record; `MOON:Valley` hands one out for the
/// four keys (`1 << (rnd & 3)`), and two routines then pair a bit with a phase:
///
/// ```text
/// MOON:Henge        4 -> 0x2e   8 -> 0x2e   2 -> 0x2d   1 -> 0x31
/// MOON:CalcDamage   1 -> 0x2e   8 -> 0x2e   2 -> 0x2d   4 -> 0x31
/// ```
///
/// Both agree that bit 2 is the full moon's stone and bit 8 the gibbous
/// moon's; they swap bits 1 and 4 between the gibbous and the new moon. The
/// status panel names only three, `New moon Moonstone`, `Full Moonstone` and
/// `Half Moonstone`, for four bits. `henge` takes `Henge`'s pairing, because
/// the stone circle is where a moonstone matters, and uses it for the blow as
/// well so the two can never disagree here.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum Moonstone {
    /// Bit 1.
    New,
    /// Bit 2.
    Full,
    /// Bit 4.
    Half,
    /// Bit 8.
    Gibbous,
}

impl Moonstone {
    pub const ALL: [Moonstone; 4] =
        [Moonstone::New, Moonstone::Full, Moonstone::Half, Moonstone::Gibbous];

    pub fn bit(self) -> u8 {
        match self {
            Moonstone::New => 1,
            Moonstone::Full => 2,
            Moonstone::Half => 4,
            Moonstone::Gibbous => 8,
        }
    }

    /// The night this stone opens the circle. `MOON:Henge`.
    pub fn phase(self) -> Phase {
        match self {
            Moonstone::New => Phase::New,
            Moonstone::Full => Phase::Full,
            Moonstone::Half | Moonstone::Gibbous => Phase::Gibbous,
        }
    }

    pub fn item(self) -> &'static str {
        match self {
            Moonstone::New => "moonstone.new",
            Moonstone::Full => "moonstone.full",
            Moonstone::Half => "moonstone.half",
            Moonstone::Gibbous => "moonstone.gibbous",
        }
    }

    pub fn from_item(id: &str) -> Option<Moonstone> {
        Moonstone::ALL.into_iter().find(|m| m.item() == id)
    }

    /// The stone a single bit of `+0x16` names. `MOON:Valley` hands one out as
    /// `al = 1; al <<= rnd & 3`, so a granted stone arrives as a bit and has
    /// to be read back as one.
    pub fn from_bit(bit: u8) -> Option<Moonstone> {
        Moonstone::ALL.into_iter().find(|m| m.bit() == bit)
    }

    /// The panel's own words where it has them, `st10`..`st12`.
    pub fn name(self) -> &'static str {
        match self {
            Moonstone::New => "New moon Moonstone",
            Moonstone::Full => "Full Moonstone",
            Moonstone::Half => "Half Moonstone",
            Moonstone::Gibbous => "Gibbous Moonstone",
        }
    }
}

/// Whether an item id is one of the quest's tokens, which no service will buy,
/// sell or take as an offering.
pub fn is_token(id: &str) -> bool {
    Key::from_item(id).is_some() || Moonstone::from_item(id).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `[0x898b] += 1; cmp 3; jle` then `and MoonCount, 7`: four days to a
    /// step, eight steps to the cycle, thirty two days to come round.
    #[test]
    fn four_days_move_the_moon_and_eight_steps_close_the_cycle() {
        let mut m = Moon::new();
        assert_eq!(m.phase(), Phase::Full, "InitGameStart writes 0x2d");
        for _ in 0..3 {
            assert!(!m.new_day(), "the first three days leave it where it is");
        }
        assert!(m.new_day(), "the fourth moves it");
        assert_eq!(m.phase(), Phase::Gibbous);
        let mut moved = 1;
        for _ in 0..(DAYS_PER_PHASE * 7) {
            if m.new_day() {
                moved += 1;
            }
        }
        assert_eq!(moved, 8);
        assert_eq!(m.phase(), Phase::Full, "and the cycle is closed");
        assert_eq!(m.days_left(), 4);
    }

    /// The cycle waxes back to where it started, so every stone's night comes
    /// round and no phase is a dead end.
    #[test]
    fn every_night_a_moonstone_wants_comes_round() {
        for stone in Moonstone::ALL {
            assert!(CYCLE.contains(&stone.phase()), "{stone:?} waits for a moon that never rises");
        }
        assert_eq!(CYCLE.iter().filter(|p| **p == Phase::Full).count(), 1);
        assert_eq!(CYCLE.iter().filter(|p| **p == Phase::New).count(), 1);
        assert_eq!(CYCLE.iter().filter(|p| **p == Phase::Gibbous).count(), 2);
    }

    /// The cel numbers are the original's own constants, and a picture exists
    /// for each: `KI.CEL` has fifty frames and 0x31 is the last of them.
    #[test]
    fn a_phase_is_a_cel_of_the_bank() {
        assert_eq!(Phase::Full.cel(), 0x2d);
        assert_eq!(Phase::New.cel(), 0x31);
        for p in CYCLE {
            assert!((0x2d..=0x31).contains(&p.cel()));
            assert_eq!(Phase::from_cel(p.cel()), Some(p));
        }
        assert_eq!(Phase::from_cel(0x2c), None);
    }

    #[test]
    fn keys_are_planted_in_lair_file_order() {
        let bits: Vec<u8> = Key::ALL.iter().map(|k| k.bit()).collect();
        assert_eq!(bits, vec![8, 4, 2, 1]);
        let families: Vec<&str> = Key::ALL.iter().map(|k| k.family()).collect();
        assert_eq!(families, vec!["forest", "waste", "swamp", "glade"]);
        assert_eq!(Key::from_family("swamp"), Some(Key::Swamp));
        assert_eq!(Key::from_item("key.swamp"), Some(Key::Swamp));
        assert_eq!(Key::from_item("potion"), None);
    }

    /// `MOON:Henge`: bit 2 on the full moon, bit 1 on the new, bits 4 and 8
    /// both on the gibbous.
    #[test]
    fn a_moonstone_answers_to_the_night_henge_tests() {
        assert_eq!(Moonstone::Full.phase(), Phase::Full);
        assert_eq!(Moonstone::New.phase(), Phase::New);
        assert_eq!(Moonstone::Half.phase(), Phase::Gibbous);
        assert_eq!(Moonstone::Gibbous.phase(), Phase::Gibbous);
        assert!(is_token("moonstone.full") && is_token("key.glade") && !is_token("potion"));
    }

    #[test]
    fn the_moon_survives_serialization() {
        let mut m = Moon::new();
        for _ in 0..5 {
            m.new_day();
        }
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<Moon>(&json).unwrap(), m);
    }
}
