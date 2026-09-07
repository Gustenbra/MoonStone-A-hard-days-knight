//! Who a knight is, and what that is worth in a fight.
//!
//! Four knights ride out. Until now they were four copies of one figure in four
//! substituted colours, with no name, no history and nothing to choose between
//! them. This is the other half: an identity, a stat block, and the arithmetic
//! the original actually used to turn that block into health, reach and damage.
//!
//! **What is recovered.** `_STATUS` and the knight record in `MOON` give the
//! whole sheet. A knight record carries three ability bytes at `+0x2e`, `+0x2f`
//! and `+0x30`, and the status panel prints them against the labels `Strength`,
//! `Constitution` and `Endurance`; life points at `+0x31`, gold at `+0x32`,
//! daggers at `+0x34`, experience at `+0x36`, health and its maximum at `+0x38`
//! and `+0x3c`, the weapon at `+0x40` and the armour at `+0x42`.
//! `SetKnightEquipment` opens every knight at one of each ability, five life
//! points, ten daggers, ten gold, a long sword and padded armour. Two short
//! routines in `MOON` derive the rest:
//!
//! ```text
//! max health = 10 * constitution + armour + 10        (0x28d)
//! stride     =  2 * endurance    + armour +  4        (0x2e7)
//! damage     = swing + strength  + weapon             (CalcDamage, 0x2d67)
//! ```
//!
//! The armour terms are the original's own table, quirks included: chain mail
//! adds ten health, plate twenty, battle armour thirty, and only chain mail and
//! battle armour add to the stride. Weapons add nothing for a long sword, two
//! for a broad sword, three for a claymore and five for the sword of sharpness.
//!
//! **What is not.** `InitKnights` distinguishes the four only by name, by colour
//! and by which corner of the map they start in. Their stat blocks are
//! identical. The shape here allows them to differ, because the data allows it
//! and because the build order asks for it, but what ships is what the original
//! had, and anything else would be an invention wearing recovered clothes.

use crate::item::{Items, Virtue};
use serde::{Deserialize, Serialize};

/// One of the four, as the pack declares them.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct KnightDef {
    pub name: String,
    /// The knight's three shades, brightest first, as 0xRRGGBB.
    ///
    /// Recovered from `KnightGlowColours`, which holds three 12-bit values per
    /// knight: blue, gold, emerald and red, in that order. The initials on the
    /// original's four name buffers (`BNAME`, `GNAME`, `ENAME`, `RNAME`) are
    /// those colours, which is how the pairing is known rather than guessed.
    pub shades: Vec<u32>,
    /// Where on the overworld this knight begins. One corner each, from
    /// `InitKnights`.
    pub home: [i32; 2],
    pub strength: i32,
    pub constitution: i32,
    pub endurance: i32,
    /// Life points. Five, and the healer in `KnightHeal` treats fewer than five
    /// as a reason to open a flask.
    pub life: i32,
    pub daggers: u32,
    pub gold: u32,
    /// Item ids. What they are worth is on the item, not here.
    pub weapon: String,
    pub armour: String,
}

/// The four, in select order. A list rather than a map because the order is the
/// order they stand in on the select screen, and a map would lose it.
pub type Knights = Vec<KnightDef>;

/// A knight as a run carries them: the sheet, without the run's own progress.
///
/// Abilities do not yet change. Growing them is `AdjustLevel` and build order
/// item 45, and inventing a curve here would be worse than leaving the field
/// honest.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct Knight {
    pub name: String,
    /// Which of the four. Also the seat, and therefore the colour.
    pub seat: usize,
    pub strength: i32,
    pub constitution: i32,
    pub endurance: i32,
    pub daggers: u32,
    pub weapon: String,
    pub armour: String,
}

impl Knight {
    pub fn from_def(def: &KnightDef, seat: usize) -> Knight {
        Knight {
            name: def.name.clone(),
            seat,
            strength: def.strength,
            constitution: def.constitution,
            endurance: def.endurance,
            daggers: def.daggers,
            weapon: def.weapon.clone(),
            armour: def.armour.clone(),
        }
    }

    /// Has anyone been chosen yet? A default sheet belongs to nobody, which is
    /// what a run has before the select screen has been through.
    pub fn named(&self) -> bool {
        !self.name.is_empty()
    }

    /// `10 * constitution + armour + 10`, from the routine at 0x28d.
    pub fn max_health(&self, items: &Items) -> i32 {
        self.constitution * 10 + self.armour_worn(items).0 + 10
    }

    /// `2 * endurance + armour + 4`, from the routine at 0x2e7. The original
    /// calls this nothing; it is the term that grows with endurance and with
    /// what you are wearing, and it is used here as reach.
    pub fn stride(&self, items: &Items) -> i32 {
        self.endurance * 2 + self.armour_worn(items).1 + 4
    }

    /// What strength and a blade add to a swing, from `CalcDamage`.
    pub fn damage_bonus(&self, items: &Items) -> i32 {
        self.strength + self.weapon_held(items)
    }

    fn armour_worn(&self, items: &Items) -> (i32, i32) {
        match items.get(&self.armour).map(|d| &d.virtue) {
            Some(Virtue::Armour { health, stride }) => (*health, *stride),
            _ => (0, 0),
        }
    }

    fn weapon_held(&self, items: &Items) -> i32 {
        match items.get(&self.weapon).map(|d| &d.virtue) {
            Some(Virtue::Weapon { damage }) => *damage,
            _ => 0,
        }
    }

    /// What to call the weapon and armour on a status panel. Falls back to the
    /// id, so a pack that is missing an item still says something true.
    pub fn weapon_name(&self, items: &Items) -> String {
        items.get(&self.weapon).map_or_else(|| self.weapon.clone(), |d| d.name.clone())
    }

    pub fn armour_name(&self, items: &Items) -> String {
        items.get(&self.armour).map_or_else(|| self.armour.clone(), |d| d.name.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::ItemDef;

    fn kit() -> Items {
        let mut items = Items::new();
        let mut put = |id: &str, name: &str, price: u32, virtue: Virtue| {
            items.insert(
                id.into(),
                ItemDef { name: name.into(), price, virtue, consumed: false },
            );
        };
        put("long_sword", "Long sword", 0, Virtue::Weapon { damage: 0 });
        put("claymore", "Claymore sword", 25, Virtue::Weapon { damage: 3 });
        put("padded_armour", "Padded armour", 0, Virtue::Armour { health: 0, stride: 0 });
        put("chain_mail", "Chain mail", 30, Virtue::Armour { health: 10, stride: 2 });
        put("plate_armour", "Plate armour", 50, Virtue::Armour { health: 20, stride: 0 });
        items
    }

    fn opening() -> Knight {
        Knight {
            name: "Sir Banner".into(),
            seat: 0,
            strength: 1,
            constitution: 1,
            endurance: 1,
            daggers: 10,
            weapon: "long_sword".into(),
            armour: "padded_armour".into(),
        }
    }

    /// `SetKnightEquipment` opens at one of each ability in padded armour, and
    /// the routine at 0x28d then gives twenty health, not the ninety-nine the
    /// field is first written with.
    #[test]
    fn a_new_knight_starts_with_twenty_health() {
        let (k, items) = (opening(), kit());
        assert_eq!(k.max_health(&items), 20);
        assert_eq!(k.stride(&items), 6);
        assert_eq!(k.damage_bonus(&items), 1);
    }

    #[test]
    fn constitution_is_ten_health_a_point() {
        let (mut k, items) = (opening(), kit());
        k.constitution = 4;
        assert_eq!(k.max_health(&items), 50);
    }

    /// The armour table, quirks included: plate is worth more health than chain
    /// mail and yet adds nothing to the stride.
    #[test]
    fn armour_is_worth_health_and_only_sometimes_reach() {
        let (mut k, items) = (opening(), kit());
        k.armour = "chain_mail".into();
        assert_eq!(k.max_health(&items), 30);
        assert_eq!(k.stride(&items), 8);
        k.armour = "plate_armour".into();
        assert_eq!(k.max_health(&items), 40);
        assert_eq!(k.stride(&items), 6, "plate is heavy and buys no reach");
    }

    #[test]
    fn a_better_blade_hits_harder() {
        let (mut k, items) = (opening(), kit());
        k.strength = 3;
        assert_eq!(k.damage_bonus(&items), 3);
        k.weapon = "claymore".into();
        assert_eq!(k.damage_bonus(&items), 6);
    }

    /// A pack that does not declare the gear must not silently change the
    /// arithmetic. Missing kit is worth nothing, not worth a guess.
    #[test]
    fn missing_gear_is_worth_nothing_rather_than_something_invented() {
        let mut k = opening();
        k.weapon = "no_such_sword".into();
        k.armour = "no_such_armour".into();
        let items = kit();
        assert_eq!(k.max_health(&items), 20);
        assert_eq!(k.damage_bonus(&items), 1);
    }

    #[test]
    fn a_sheet_survives_serialization() {
        let k = opening();
        let json = serde_json::to_string(&k).unwrap();
        assert_eq!(serde_json::from_str::<Knight>(&json).unwrap(), k);
    }
}
