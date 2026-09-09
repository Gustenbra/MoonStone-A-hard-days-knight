//! Lairs: twenty four dangerous places, a guardian in each, and the four keys.
//!
//! A lair is the only place on the map worth going to for what is in it rather
//! than for what is sold there, and it is where the quest begins: one key is
//! hidden in one lair of each terrain family, and four keys open the Valley.
//!
//! **What is recovered**, from `MOON` and `_MAP`. The table is 24 records of
//! 18 bytes at DS:0002, built by the initialiser at image 0x1e00:
//!
//! ```text
//! +0x00  the lair's own item record, 24 bytes of counts at fmem_LairMagic
//! +0x02  which entry of CombatTable sets up the guardian fight
//! +0x04  how many of them: TotalMonsters going in, what is left coming out
//! +0x06  gold
//! +0x08  set to 1 the first time the guardian is beaten (LairWon)
//! +0x0a  x on the map, 0xffff once the lair is stripped bare
//! +0x0c  y
//! +0x0e  the landscape code, which picks the arena family
//! +0x10  the arena layout: fol1..6, wal1..6, swl1..6, gll1..6
//! ```
//!
//! **The keys.** The initialiser calls `NUM16` four times, which is `rnd & 7`
//! rolled again while it exceeds five, and writes 8, 4, 2 and 1 into byte
//! `+0x14` of the chosen lair's item record, stepping 0x6c (six records) on
//! between each. So each family of six hides exactly one key, in a lair chosen
//! uniformly at the start of the quest.
//!
//! **The contents.** `LairFill` rolls 0..=100 against `MOON:LairRND`, four
//! records of a threshold and a kind: 50 gold, 70 magic, 90 both, 100 both
//! again, because the dispatcher's last comparison is dead code and falls
//! through to the same call. Gold is the wizard's own gift routine called with
//! `dx` set, which puts ten to thirty one in the lair; magic is the wizard's
//! own bestowal called twice, so a lair with magic holds two items.
//!
//! **Arriving.** `MOON:CheckLairEncounter` overlaps the traveller's 8x10 token
//! with icon frame 0x1f of `MI.C`, which is 9x5, exactly the way a town is
//! decided, and puts `Enter Lair` on the map's own list. `_MAP:DisplayLairs`
//! draws frame 0x14 at every lair whose x is not negative, so lairs are on the
//! map from the start. A stripped one leaves it: `MOON:CheckLairClear` writes
//! 0xffff over the coordinates when the gold is zero *and* all 24 item counts
//! are zero, so a lair you have beaten but not emptied is still there to go
//! back to.
//!
//! **Leaving.** `MOON:LairWon` sets the cleared flag and adds one to the
//! knight's experience the first time only, then opens the status panel's lair
//! page for the taking.
//!
//! **Where they are, and what is in them, is recovered.** All four of the
//! tables the initialiser copies from are readable, and the baker reads them:
//! `ForestLairs` is 24 pairs of words at DS:0a6a, a `CombatTable` byte offset
//! and a head count; `LairLocation` is 24 pairs at DS:0aca, the corner the map
//! draws the lair at; `LairType` is 24 words at DS:0b2a, the landscape the
//! fight happens on; and `LairFile` is the 24 arena layouts. The copy loop at
//! image 0x1ea0 is what says which is which, field by field. What the guardian
//! can be is recovered too, from `InitGameStart` filling `CombatTable` with
//! the thirteen `InitKnightvs*` routines.
//!
//! Twenty three of the twenty four coordinates land on a cell of `MapType`
//! whose code is that lair's own `LairType`, which is two tables agreeing that
//! were read out of different places; the odd one, lair 15, stands a cell into
//! the treeline and is still fought in the marsh, because `InitLair` hands
//! `ColourBackdrop` the record's landscape and never asks the map.
//!
//! `ForestLairs`'s head count is `TotalMonsters`, everything that comes at you
//! before a lair is clear, and it runs from three to fourteen. **The waves are
//! built**, and they are [`crate::wave`]: `AdjustLevel` (image `0x2824`) writes
//! this number over `TotalMonsters` at `0x287e` and then takes the creature's
//! own row of `lev_adjust` off it, `SetMonsterCombat` (`0x27e4`) stands
//! `MaxMonsters` of them up, which is one for everything but the ratmen, and
//! `CountTheDead` (`0x213`) sends the next one in on the frame the last one's
//! death script reaches its `TASKGOSUB`. So a lair of fourteen is fourteen
//! fights one after another, not a crowd, and the pack carries the number
//! unrounded because the arena no longer has to hold it all at once.

use crate::item::Items;
use crate::moon::Key;
use crate::run::Run;
use crate::service::{gold_from, magic_item};
use serde::{Deserialize, Serialize};

/// How many lairs there are. `mov cx, 0x18` in the initialiser,
/// `CheckLairEncounter` and `DisplayLairs`.
pub const LAIRS: usize = 24;

/// How many each family holds: the initialiser steps 0x6c bytes, six records,
/// between one key and the next.
pub const PER_FAMILY: usize = 6;

/// How many magic items a lair with magic in it holds: `MagicLair` calls the
/// bestowal twice.
pub const MAGIC_PER_LAIR: usize = 2;

/// What a lair holds, by the roll `LairFill` takes: `MOON:LairRND`, four
/// records of a threshold and a kind, walked for the first threshold the roll
/// does not exceed. 1 is gold, 2 magic, 3 and 4 both.
pub const CONTENTS: [(u32, u8); 4] = [(50, 1), (70, 2), (90, 3), (100, 4)];

/// What is actually in one lair on this run.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Lair {
    /// `+0x06`. Ten to thirty one, or none.
    pub gold: u32,
    /// The item ids on the floor. Two, or none, less whatever was carried out.
    pub magic: Vec<String>,
    /// The key hidden here, if this is the family's one.
    pub key: Option<Key>,
    /// `+0x08`. Whether the guardian has been beaten.
    pub cleared: bool,
}

impl Lair {
    /// Whether anything is left to carry out. `CheckLairClear` asks exactly
    /// this: no gold, and every one of the 24 item counts zero.
    pub fn empty(&self) -> bool {
        self.gold == 0 && self.magic.is_empty() && self.key.is_none()
    }
}

/// What came of walking in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Raid {
    /// The guardian is waiting. The caller starts the bout; the floor is only
    /// yours once it is won.
    Guardian,
    /// Already beaten, and there was something on the floor.
    Spoils(Spoils),
    /// Beaten and stripped. The original takes such a lair off the map; this
    /// is what it says if a pack keeps one there.
    Bare,
}

/// What was carried out of a lair.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Spoils {
    pub gold: u32,
    /// Item ids taken. Fewer than were on the floor if the pack filled up.
    pub magic: Vec<String>,
    pub key: Option<Key>,
    /// Left behind because there was no room.
    pub left: usize,
}

impl Spoils {
    pub fn anything(&self) -> bool {
        self.gold > 0 || !self.magic.is_empty() || self.key.is_some()
    }

    /// What to say about it. The original's lair page is a picture of the
    /// floor; a menu has to say it in words, so the words are ours.
    pub fn describe(&self, items: &Items) -> String {
        if !self.anything() {
            return "Nothing but bones.".into();
        }
        let mut parts: Vec<String> = Vec::new();
        if self.gold > 0 {
            parts.push(format!("{} gold", self.gold));
        }
        for id in &self.magic {
            parts.push(items.get(id).map_or_else(|| id.clone(), |d| d.name.clone()));
        }
        if let Some(k) = self.key {
            parts.push(format!("the {}", k.name()));
        }
        let mut said = format!("You take {}.", parts.join(", "));
        if self.left > 0 {
            said.push_str(" More lies here than you can carry.");
        }
        said
    }
}

impl Run {
    /// Stock every lair, once, at the start of a quest.
    ///
    /// `families` is the terrain family of each lair in table order, which is
    /// what decides where the keys go: the initialiser plants them six records
    /// apart, so lair `i` of family `f` is the one at index `f * 6 + i`. The
    /// rolls are taken in the initialiser's order, the four keys first and then
    /// the 24 fills, so two machines with the same seed lay the same board.
    ///
    /// Magic that the pack does not declare is left out rather than put on a
    /// floor nobody can pick it up from: a lair is stocked from what exists.
    pub fn stock_lairs(&mut self, families: &[String], items: &Items) {
        self.lairs = vec![Lair::default(); families.len()];
        // `call NUM16; mul 0x12; ... mov byte ptr [di+0x14], 8`, then
        // `add si, 0x6c` and again, for 8, 4, 2 and 1.
        for key in Key::ALL {
            let mine: Vec<usize> = (0..families.len())
                .filter(|i| families[*i] == key.family())
                .collect();
            if mine.is_empty() {
                continue;
            }
            let pick = self.num16() as usize % mine.len();
            self.lairs[mine[pick]].key = Some(key);
        }
        for i in 0..self.lairs.len() {
            self.fill_lair(i, items);
        }
    }

    /// `MOON:NUM16`: `rnd & 7`, rolled again while it is over five. Bounded
    /// here, because a simulation two machines keep in step must not be able
    /// to hang; eight refusals in a row is one chance in sixty five thousand.
    fn num16(&mut self) -> u32 {
        for _ in 0..8 {
            let n = self.next_roll() & 7;
            if n <= 5 {
                return n;
            }
        }
        self.next_roll() % 6
    }

    /// `LairFill`, for one lair.
    fn fill_lair(&mut self, index: usize, items: &Items) {
        let roll = self.roll(101);
        let kind = CONTENTS
            .iter()
            .find(|(threshold, _)| roll <= *threshold)
            .map_or(4, |(_, kind)| *kind);
        if kind != 2 {
            let gold = gold_from(self.next_roll());
            if let Some(lair) = self.lairs.get_mut(index) {
                lair.gold += gold;
            }
        }
        if kind != 1 {
            for _ in 0..MAGIC_PER_LAIR {
                let Some(slot) = self.known_magic_slot(items) else { continue };
                let Some(id) = magic_item(slot) else { continue };
                if let Some(lair) = self.lairs.get_mut(index) {
                    lair.magic.push(id.to_string());
                }
            }
        }
    }

    /// Whether a lair is still on the map: beaten *and* stripped takes it off,
    /// which is `CheckLairClear` writing 0xffff over its coordinates. A lair
    /// the run has not stocked is on the map, so a pack works before a quest
    /// has begun.
    pub fn lair_on_the_map(&self, index: usize) -> bool {
        self.lairs.get(index).map_or(true, |l| !(l.cleared && l.empty()))
    }

    /// Walk in. Either the guardian is up, or the floor is yours.
    pub fn raid(&mut self, index: usize, items: &Items) -> Raid {
        if index >= self.lairs.len() {
            return Raid::Bare;
        }
        if !self.lairs[index].cleared {
            return Raid::Guardian;
        }
        let spoils = self.strip_lair(index, items);
        if spoils.anything() {
            Raid::Spoils(spoils)
        } else {
            Raid::Bare
        }
    }

    /// The guardian is down. `MOON:LairWon`: the first win marks the lair and
    /// is worth a point of experience; the floor is yours either way.
    pub fn lair_won(&mut self, index: usize, items: &Items) -> Spoils {
        let first = match self.lairs.get_mut(index) {
            Some(lair) if !lair.cleared => {
                lair.cleared = true;
                true
            }
            _ => false,
        };
        if first {
            // `add word ptr [di+0x36], 1`, the same field a won bout pays into.
            self.earned_experience(1);
        }
        self.strip_lair(index, items)
    }

    /// Carry out what will fit. What will not stays on the floor, which is why
    /// a beaten lair can still be worth coming back to. The key is small and
    /// goes first, so the quest is never blocked by a pack full of potions.
    fn strip_lair(&mut self, index: usize, items: &Items) -> Spoils {
        let Some(lair) = self.lairs.get(index).cloned() else { return Spoils::default() };
        let mut got = Spoils { gold: lair.gold, ..Spoils::default() };
        if got.gold > 0 {
            self.earn(got.gold);
            self.lairs[index].gold = 0;
        }
        if let Some(key) = lair.key {
            if self.kit.take(key.item(), 1) > 0 {
                got.key = Some(key);
                self.lairs[index].key = None;
            } else {
                got.left += 1;
            }
        }
        let mut left_behind = Vec::new();
        for id in lair.magic {
            if self.kit.take(&id, 1) > 0 {
                got.magic.push(id);
            } else {
                got.left += 1;
                left_behind.push(id);
            }
        }
        self.lairs[index].magic = left_behind;
        self.refresh(items);
        got
    }

    /// Which keys the run carries, as `Valley` reads them: all four bits set
    /// is `0xf`, and the Valley wants all four.
    pub fn keys_held(&self) -> Vec<Key> {
        Key::ALL.into_iter().filter(|k| self.kit.count(k.item()) > 0).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{ItemDef, Virtue};
    use crate::knight::KnightDef;

    fn goods() -> Items {
        let mut items = Items::new();
        let mut add = |id: &str, virtue: Virtue| {
            items.insert(id.into(), ItemDef { name: id.into(), price: 0, virtue, consumed: false });
        };
        for id in ["potion", "gem_of_seeing", "ring_of_protection", "talisman", "scroll_of_haste"] {
            add(id, Virtue::Inert);
        }
        add("sword_of_sharpness", Virtue::Weapon { damage: 5 });
        add("long_sword", Virtue::Weapon { damage: 0 });
        add("padded_armour", Virtue::Armour { health: 0, stride: 0 });
        for k in Key::ALL {
            add(k.item(), Virtue::Inert);
        }
        items
    }

    fn families() -> Vec<String> {
        ["forest", "waste", "swamp", "glade"]
            .iter()
            .flat_map(|f| std::iter::repeat(f.to_string()).take(PER_FAMILY))
            .collect()
    }

    fn run(seed: u32) -> (Run, Items) {
        let items = goods();
        let def = KnightDef {
            name: "SIR GODBER".into(),
            shades: vec![0],
            home: [16, 16],
            strength: 1,
            constitution: 1,
            endurance: 1,
            life: 5,
            daggers: 10,
            gold: 10,
            weapon: "long_sword".into(),
            armour: "padded_armour".into(),
        };
        let mut r = Run::for_knight(&def, 0, &items);
        r.reseed(seed);
        r.kit.capacity = 200;
        r.stock_lairs(&families(), &items);
        (r, items)
    }

    #[test]
    fn there_are_twenty_four_lairs_six_to_a_family() {
        let (r, _) = run(1);
        assert_eq!(families().len(), LAIRS);
        assert_eq!(r.lairs.len(), LAIRS);
    }

    /// One key per family, in a lair of that family, and never two in one.
    #[test]
    fn each_family_hides_exactly_one_key() {
        let fams = families();
        for seed in 1..60u32 {
            let (r, _) = run(seed.wrapping_mul(2_654_435_761) | 1);
            for key in Key::ALL {
                let here: Vec<usize> = (0..LAIRS)
                    .filter(|n| r.lairs[*n].key == Some(key))
                    .collect();
                assert_eq!(here.len(), 1, "{key:?} at seed {seed}: {here:?}");
                assert_eq!(fams[here[0]], key.family(), "and in its own family's ground");
            }
        }
    }

    /// And which lair it is in moves with the seed, or the quest would be the
    /// same walk every time.
    #[test]
    fn where_a_key_is_depends_on_the_seed() {
        let mut seen = std::collections::BTreeSet::new();
        for seed in 1..80u32 {
            let (r, _) = run(seed.wrapping_mul(0x9e37_79b9) | 1);
            seen.insert(r.lairs[..PER_FAMILY].iter().position(|l| l.key.is_some()).unwrap());
        }
        assert!(seen.len() >= 5, "the forest key moves about: {seen:?}");
    }

    /// `LairRND`: half gold, a fifth magic, the rest both. Nothing is empty,
    /// because the fourth kind falls through to the same call as the third.
    #[test]
    fn every_lair_holds_something() {
        let (mut only_gold, mut only_magic, mut both) = (0, 0, 0);
        for seed in 1..40u32 {
            let (r, _) = run(seed.wrapping_mul(48271) | 1);
            for lair in &r.lairs {
                assert!(!lair.empty(), "a lair with nothing in it");
                match (lair.gold > 0, !lair.magic.is_empty()) {
                    (true, false) => only_gold += 1,
                    (false, true) => only_magic += 1,
                    (true, true) => both += 1,
                    (false, false) => panic!("neither, and no key either"),
                }
                if !lair.magic.is_empty() {
                    assert_eq!(lair.magic.len(), MAGIC_PER_LAIR, "two items, or none");
                }
                if lair.gold > 0 {
                    assert!((10..=31).contains(&lair.gold), "{}", lair.gold);
                }
            }
        }
        assert!(only_gold > both && both > only_magic, "{only_gold} {both} {only_magic}");
    }

    /// A lair is stocked from what the pack declares. A slot the pack has no
    /// item for is left off the floor rather than put there as an id nobody
    /// can pick up.
    #[test]
    fn a_lair_only_holds_what_the_pack_knows() {
        let mut items = goods();
        items.retain(|id, _| id == "potion" || id.starts_with("key.") || id.ends_with("sword") || id.ends_with("armour"));
        let mut r = run(9).0;
        r.stock_lairs(&families(), &items);
        for lair in &r.lairs {
            assert!(lair.magic.iter().all(|id| id == "potion"), "{:?}", lair.magic);
        }
    }

    #[test]
    fn the_guardian_is_up_until_it_is_beaten() {
        let (mut r, items) = run(7);
        let before = r.gold;
        assert_eq!(r.raid(0, &items), Raid::Guardian);
        assert_eq!(r.gold, before, "walking in takes nothing off the floor");
        let floor = r.lairs[0].clone();
        let spoils = r.lair_won(0, &items);
        assert!(spoils.anything());
        assert_eq!(spoils.gold, floor.gold);
        assert_eq!(r.gold, before + floor.gold);
        assert!(r.lairs[0].cleared);
        assert_eq!(r.experience, 1, "the first kill is worth a point");
        r.lair_won(0, &items);
        assert_eq!(r.experience, 1, "and only the first");
    }

    #[test]
    fn a_lair_leaves_the_map_only_when_it_is_beaten_and_stripped() {
        let (mut r, items) = run(11);
        assert!(r.lair_on_the_map(0));
        r.lair_won(0, &items);
        assert!(r.lairs[0].empty());
        assert!(!r.lair_on_the_map(0), "beaten and stripped is off the map");
        assert_eq!(r.raid(0, &items), Raid::Bare);
        assert!(r.lair_on_the_map(99), "a lair the run has not stocked is still on it");
    }

    #[test]
    fn a_full_pack_leaves_the_magic_on_the_floor_but_never_the_key() {
        let (mut r, items) = run(3);
        let key_at = r.lairs[..PER_FAMILY].iter().position(|l| l.key.is_some()).unwrap();
        // Room for the key and nothing else.
        r.kit.capacity = r.kit.carried() + 1;
        let floor = r.lairs[key_at].clone();
        let spoils = r.lair_won(key_at, &items);
        assert_eq!(spoils.key, floor.key, "the key came out");
        assert!(r.lairs[key_at].key.is_none());
        assert_eq!(r.keys_held(), vec![floor.key.unwrap()]);
        if !floor.magic.is_empty() {
            assert_eq!(spoils.left, floor.magic.len(), "and what would not fit is still there");
            assert_eq!(r.lairs[key_at].magic, floor.magic);
            assert!(r.lair_on_the_map(key_at));
        }
    }

    /// A beaten lair you could not empty is worth coming back to, and coming
    /// back does not fight the guardian again.
    #[test]
    fn coming_back_to_a_beaten_lair_picks_up_what_was_left() {
        let (mut r, items) = run(5);
        let at = (0..LAIRS).find(|i| !r.lairs[*i].magic.is_empty()).unwrap();
        r.kit.capacity = r.kit.carried();
        r.lair_won(at, &items);
        let left = r.lairs[at].magic.len();
        assert!(left > 0, "nothing fitted");
        r.kit.capacity = 200;
        match r.raid(at, &items) {
            Raid::Spoils(s) => assert_eq!(s.magic.len(), left),
            other => panic!("the guardian should stay down: {other:?}"),
        }
        assert!(r.lairs[at].empty());
        assert!(!r.lair_on_the_map(at));
    }

    #[test]
    fn a_stocked_board_survives_serialization_and_two_seeds_agree() {
        let (a, _) = run(99);
        let (b, _) = run(99);
        assert_eq!(a.lairs, b.lairs, "the same seed stocks the same board");
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(serde_json::from_str::<Run>(&json).unwrap().lairs, a.lairs);
        let (c, _) = run(100);
        assert_ne!(a.lairs, c.lairs, "and a different seed a different one");
    }

    #[test]
    fn spoils_are_said_in_words() {
        let items = goods();
        let s = Spoils { gold: 12, magic: vec!["potion".into()], key: Some(Key::Swamp), left: 1 };
        assert_eq!(
            s.describe(&items),
            "You take 12 gold, potion, the Key of the marsh. More lies here than you can carry."
        );
        assert_eq!(Spoils::default().describe(&items), "Nothing but bones.");
    }
}
