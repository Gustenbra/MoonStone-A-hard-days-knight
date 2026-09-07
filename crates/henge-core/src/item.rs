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
/// A closed set, and deliberately small. The original's `DRINKPOTIONHEAL` is
/// the one virtue we can name with confidence, so it is the one that is real;
/// everything else a merchant might stock is [`Virtue::Inert`] until the system
/// that gives it meaning exists. An inert item is honest: it is carried, it is
/// worth coin, and using it says so.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "does", rename_all = "kebab-case")]
pub enum Virtue {
    /// Mends wounds on the spot, up to your full health.
    Heal { health: i32 },
    /// Carried and worth something, but it does nothing yet.
    Inert,
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

    #[test]
    fn a_pack_survives_serialization() {
        let mut kit = Inventory::new(6);
        kit.take("potion", 2);
        let json = serde_json::to_string(&kit).unwrap();
        assert_eq!(serde_json::from_str::<Inventory>(&json).unwrap(), kit);
    }
}
