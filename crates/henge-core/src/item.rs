//! Things you carry, and the purse you carry them beside.
//!
//! Until now a run accumulated nothing. You fought, you were wounded, you
//! walked it off, and the only number that ever moved was the day. Every door
//! in a town was shut because there was nothing to spend and nothing to spend
//! it on.
//!
//! Two ideas, both of them state on a [`Run`](crate::run::Run) and both of them
//! serializable:
//!
//! - **Gold.** Coin comes off the fallen and goes to whoever is left standing.
//! - **A pack.** It holds a bounded number of things, and, crucially, things
//!   leave it. The original names a routine `TAKEFROMKNIGHT`, so losing what
//!   you carry is a first-class operation here rather than an afterthought on a
//!   list that only ever grows. A potion drunk is a potion gone, and a cutpurse
//!   on the road takes something real.
//!
//! What an item *is* lives in the pack as data, exactly as places and actors
//! do, so a new item is a JSON entry and not a line of Rust.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What using an item does.
///
/// A closed set. `_STATUS` owns the original's magic: a knight's magic record
/// (`+0x44` on his record) is a row of byte counts indexed by slot, and
/// `MagicCast` (image `0xcab6`) is a chain of `cmp bx, slot` on that index,
/// so what each of the ten magic items does is read off the code rather than
/// invented. `MagicName` (DS:`0xe38d`) pairs each slot with its name:
///
/// ```text
/// slot 0x00  Potion of healing      health to full, or a life point       Restore
/// slot 0x02  Gem of seeing          fly the map and come back             Sight
/// slot 0x04  Sword of Sharpness     held: weapon 0x19, +5 a swing         Weapon
/// slot 0x06  Ring of protection     worn: +20 health a ring               Ward
/// slot 0x08  Talisman of the Wyrm   halves the dragon's fire, floor 5     Inert here
/// slot 0x0a  Scroll of Haste        doubles the day's travel              Haste
/// slot 0x0c  Scroll of the Hawk     fly; 16 in 128 it strands you         Sight
/// slot 0x0e  Scroll of Aquisition   take a thing off another knight       Seize
/// slot 0x10  Scroll of the Wyrm     sets the dragon on a rival            Inert here
/// slot 0x12  Scroll of Protection   turns a challenger away, or backfires Protection
/// ```
///
/// Three of these are worn or held rather than used. `CalcDamage` adds a
/// weapon's own number to every swing, the routine at 0x28d adds an armour's
/// to the health a knight can carry and twenty for every ring in the record,
/// so all three are properties of the thing and belong beside its price.
/// Using a worn thing puts it on.
///
/// The two marked inert are recovered and not built: `TalismanWrym` (image
/// `0x43f4`) shifts the dragon's fire right once per talisman held and floors
/// it at five, and the Scroll of the Wyrm sets `WyrmFLAG` and opens the
/// knight picker so `KnightWyrm` can send the dragon after the chosen rival.
/// Both need a dragon that flies, which is item 36's, so they are carried,
/// worth coin, and say so when used.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "does", rename_all = "kebab-case")]
pub enum Virtue {
    /// Mends wounds on the spot, by a fixed amount, up to your full health.
    /// Ours: the henge flask, which predates the recovered potion below.
    Heal { health: i32 },
    /// The original's potion, from the routine at image `0xcad0`: health is
    /// set to its maximum, and a man who is already whole gains a life point
    /// instead, `inc byte [si+0x31]`, capped at five.
    Restore,
    /// Held. What it adds to a swing, from `CalcDamage`: nothing for a long
    /// sword, two for a broad sword, three for a claymore, five for the sword of
    /// sharpness.
    Weapon { damage: i32 },
    /// Worn. What it adds to the health a knight can carry and to their stride,
    /// from the two derivation routines in `MOON`.
    Armour { health: i32, stride: i32 },
    /// Worn, and it stacks: the routine at 0x28d does `ax = [magic+6]; mul 20`
    /// and adds it into the maximum, so every ring carried is worth twenty
    /// more health.
    Ward { health: i32 },
    /// Cast. `CastHaste` sets one flag and `_MAP:DistanceDONE` doubles the
    /// day's step budget while it is up. It falls when the day turns.
    Haste,
    /// Cast or used. Fly the map from where you stand. The gem (`MagicCast`
    /// slot 2) saves the position and sets the gem flag, and the flight ends
    /// back where it began; the hawk (slot 0xc) rolls first, and on 16 of the
    /// generator's 128 outcomes drops the knight at a random spot instead
    /// (`(rnd & 0xff) + 0x20, (rnd & 0x7f) + 0x24`, image `0xa98a`).
    /// `astray` is that count out of 128: zero for the gem, sixteen for the
    /// hawk. `returns` is whether the flight ends where it began.
    Sight { astray: u32, returns: bool },
    /// Cast. `HGTakeMagic` moves one thing from another knight's record to
    /// yours. There is one knight on this map, so it is carried, understood,
    /// and has nobody to rob.
    Seize,
    /// Cast. A ward against the next challenge. `MOON:KnightProtection` asks
    /// the challenged knight to cast it and, unless the cast backfired, the
    /// fight is skipped; `MagicCast` slot 0x12 rolls and on 11 of 128
    /// outcomes sets the backfire flag, which `ControlKnight` reads to reverse
    /// the caster's joystick for that bout. `backfire` is that count.
    Protection { backfire: u32 },
    /// Carried and worth something, but it does nothing here.
    Inert,
}

impl Virtue {
    /// Is this something you wear or hold rather than something you spend?
    pub fn worn(&self) -> bool {
        matches!(self, Virtue::Weapon { .. } | Virtue::Armour { .. } | Virtue::Ward { .. })
    }
}

/// An item as it is authored in the pack.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ItemDef {
    /// What it is called on a menu line.
    pub name: String,
    /// What a merchant asks for one. The price lives with the item, so a menu
    /// never has to repeat it and the two can never disagree.
    pub price: u32,
    /// What using one does.
    pub virtue: Virtue,
    /// Whether using it uses it up. A flask is emptied; a key is not.
    #[serde(default)]
    pub consumed: bool,
}

impl ItemDef {
    /// The menu line for using this item, in the shape the original's status
    /// screen gives its own (`st1`..`st9`): `Cast scroll of Haste`, `Cast
    /// scroll of Protection`, with the scroll's own capital lowered after the
    /// verb. Seven of the nine come out letter for letter; the original's
    /// `Drink Healing potion` and `Use Gem of Seeing` are worded differently
    /// from their item names, and the name is kept here rather than a second
    /// string carried for two lines.
    pub fn action_line(&self) -> String {
        let verb = match &self.virtue {
            Virtue::Heal { .. } | Virtue::Restore => "Drink",
            Virtue::Weapon { .. } => "Wield",
            Virtue::Armour { .. } | Virtue::Ward { .. } => "Wear",
            Virtue::Sight { returns: true, .. } => "Use",
            Virtue::Sight { .. } | Virtue::Haste | Virtue::Seize | Virtue::Protection { .. } => {
                "Cast"
            }
            Virtue::Inert => "Use",
        };
        let mut name = self.name.clone();
        if name.starts_with("Scroll") {
            name.replace_range(0..1, "s");
        }
        format!("{verb} {name}")
    }
}

/// Every item the packs declare, keyed by id. A `BTreeMap` so iteration order
/// is defined and two machines pick the same thing out of a pack.
pub type Items = BTreeMap<String, ItemDef>;

/// What came of trying to buy something.
///
/// Three outcomes rather than a bool, because a merchant who says "you cannot
/// afford that" and a merchant who says "you cannot carry that" are telling you
/// two different things, and the player needs to know which.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purchase {
    Bought { paid: u32 },
    /// The purse is short.
    TooDear,
    /// The pack is full.
    NoRoom,
    /// No such item in the packs. A data error, not a game state.
    Unknown,
}

/// What a cutpurse got.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Loss {
    Gold(u32),
    /// The id of the item taken.
    Item(String),
}

/// What you are carrying.
///
/// Counts rather than a flat list of ids: four flasks is one line on a menu and
/// one entry here, and the ordering is the id ordering, which is defined.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Inventory {
    held: BTreeMap<String, u32>,
    /// How many things in total can be carried. A pack with no bottom would
    /// make [`Inventory::take`] incapable of ever refusing, and a merchant
    /// incapable of ever telling you why.
    pub capacity: u32,
}

impl Default for Inventory {
    fn default() -> Inventory {
        Inventory::new(8)
    }
}

impl Inventory {
    pub fn new(capacity: u32) -> Inventory {
        Inventory { held: BTreeMap::new(), capacity }
    }

    /// How many of one thing you have.
    pub fn count(&self, id: &str) -> u32 {
        self.held.get(id).copied().unwrap_or(0)
    }

    /// How many things in total.
    pub fn carried(&self) -> u32 {
        self.held.values().sum()
    }

    pub fn room(&self) -> u32 {
        self.capacity.saturating_sub(self.carried())
    }

    pub fn is_empty(&self) -> bool {
        self.carried() == 0
    }

    /// What you hold, in id order. Order is defined so a listing is the same on
    /// every machine.
    pub fn iter(&self) -> impl Iterator<Item = (&str, u32)> {
        self.held.iter().map(|(k, n)| (k.as_str(), *n))
    }

    /// Pick some up. Returns how many actually fitted, which may be fewer than
    /// asked for and may be none.
    pub fn take(&mut self, id: &str, n: u32) -> u32 {
        let fits = n.min(self.room());
        if fits > 0 {
            *self.held.entry(id.to_string()).or_insert(0) += fits;
        }
        fits
    }

    /// Have some taken off you. Returns how many actually went, which is the
    /// operation `TAKEFROMKNIGHT` names: losing is a real thing that can partly
    /// succeed, not a silent removal.
    ///
    /// An emptied entry is removed rather than left at zero, so `iter` never
    /// offers a thing you do not have.
    pub fn lose(&mut self, id: &str, n: u32) -> u32 {
        let have = self.count(id);
        let gone = n.min(have);
        if gone == 0 {
            return 0;
        }
        if gone == have {
            self.held.remove(id);
        } else {
            self.held.insert(id.to_string(), have - gone);
        }
        gone
    }

    /// Lose everything.
    pub fn clear(&mut self) {
        self.held.clear();
    }

    /// One thing, chosen by a roll, taken off you. Returns what went.
    ///
    /// The roll picks across everything held rather than across kinds, so a man
    /// carrying four flasks and one key most often loses a flask. Deterministic
    /// given the roll, because the simulation's randomness is carried in its
    /// state and never taken from the system.
    pub fn take_one(&mut self, roll: u32) -> Option<String> {
        let total = self.carried();
        if total == 0 {
            return None;
        }
        let mut nth = roll % total;
        let mut chosen = None;
        for (id, n) in self.held.iter() {
            if nth < *n {
                chosen = Some(id.clone());
                break;
            }
            nth -= *n;
        }
        let id = chosen?;
        self.lose(&id, 1);
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn potion() -> ItemDef {
        ItemDef {
            name: "Flask of healing".into(),
            price: 25,
            virtue: Virtue::Heal { health: 40 },
            consumed: true,
        }
    }

    #[test]
    fn a_pack_holds_what_you_put_in_it() {
        let mut kit = Inventory::new(4);
        assert_eq!(kit.take("potion", 2), 2);
        assert_eq!(kit.count("potion"), 2);
        assert_eq!(kit.carried(), 2);
        assert_eq!(kit.room(), 2);
    }

    #[test]
    fn a_pack_has_a_bottom() {
        let mut kit = Inventory::new(3);
        assert_eq!(kit.take("potion", 5), 3, "only what fits goes in");
        assert_eq!(kit.take("key", 1), 0, "and then nothing does");
        assert_eq!(kit.carried(), 3);
    }

    /// `TAKEFROMKNIGHT`: things leave the pack, and the pack says how many
    /// actually went rather than pretending it lost what it never had.
    #[test]
    fn things_can_be_taken_off_you() {
        let mut kit = Inventory::default();
        kit.take("potion", 2);
        assert_eq!(kit.lose("potion", 1), 1);
        assert_eq!(kit.count("potion"), 1);
        assert_eq!(kit.lose("potion", 5), 1, "you cannot lose what you have not got");
        assert_eq!(kit.count("potion"), 0);
        assert_eq!(kit.lose("key", 1), 0, "nor something you never carried");
    }

    #[test]
    fn an_emptied_line_leaves_the_pack_entirely() {
        let mut kit = Inventory::default();
        kit.take("potion", 1);
        kit.lose("potion", 1);
        assert!(kit.is_empty());
        assert_eq!(kit.iter().count(), 0, "no ghost entry sitting at zero");
    }

    #[test]
    fn a_cutpurse_takes_one_thing_and_the_roll_decides_which() {
        let mut kit = Inventory::default();
        kit.take("key", 1);
        kit.take("potion", 3);
        // Four things held: the key is first in id order, then three flasks.
        let mut a = kit.clone();
        assert_eq!(a.take_one(0).as_deref(), Some("key"));
        let mut b = kit.clone();
        assert_eq!(b.take_one(1).as_deref(), Some("potion"));
        assert_eq!(b.carried(), 3, "exactly one thing went");
        // The roll wraps, so any number is a legal roll.
        let mut c = kit.clone();
        assert_eq!(c.take_one(4).as_deref(), Some("key"));
    }

    #[test]
    fn an_empty_pack_has_nothing_to_take() {
        let mut kit = Inventory::default();
        assert_eq!(kit.take_one(7), None);
    }

    #[test]
    fn an_item_survives_serialization() {
        let p = potion();
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<ItemDef>(&json).unwrap(), p);
        assert!(json.contains("\"does\":\"heal\""), "virtues are tagged in the data");
    }

    /// The original's own menu lines, `st5` and `st9`, come out of the name
    /// and the virtue; what is worn is put on rather than spent.
    #[test]
    fn the_line_that_uses_a_thing_is_the_status_screens_own() {
        let scroll = |name: &str, virtue| ItemDef {
            name: name.into(),
            price: 0,
            virtue,
            consumed: true,
        };
        assert_eq!(scroll("Scroll of Haste", Virtue::Haste).action_line(), "Cast scroll of Haste");
        assert_eq!(
            scroll("Scroll of Protection", Virtue::Protection { backfire: 11 }).action_line(),
            "Cast scroll of Protection"
        );
        assert_eq!(
            scroll("Scroll of the Hawk", Virtue::Sight { astray: 16, returns: false }).action_line(),
            "Cast scroll of the Hawk"
        );
        assert_eq!(
            scroll("Gem of seeing", Virtue::Sight { astray: 0, returns: true }).action_line(),
            "Use Gem of seeing"
        );
        assert_eq!(scroll("Ring of protection", Virtue::Ward { health: 20 }).action_line(), "Wear Ring of protection");
        assert!(Virtue::Ward { health: 20 }.worn());
        assert!(!Virtue::Haste.worn());
    }

    #[test]
    fn every_virtue_survives_serialization_under_its_own_tag() {
        for (v, tag) in [
            (Virtue::Restore, "restore"),
            (Virtue::Ward { health: 20 }, "ward"),
            (Virtue::Haste, "haste"),
            (Virtue::Sight { astray: 16, returns: false }, "sight"),
            (Virtue::Seize, "seize"),
            (Virtue::Protection { backfire: 11 }, "protection"),
        ] {
            let json = serde_json::to_string(&v).unwrap();
            assert!(json.contains(&format!("\"does\":\"{tag}\"")), "{json}");
            assert_eq!(serde_json::from_str::<Virtue>(&json).unwrap(), v);
        }
    }

    #[test]
    fn a_pack_survives_serialization() {
        let mut kit = Inventory::new(6);
        kit.take("potion", 2);
        let json = serde_json::to_string(&kit).unwrap();
        assert_eq!(serde_json::from_str::<Inventory>(&json).unwrap(), kit);
    }
}
