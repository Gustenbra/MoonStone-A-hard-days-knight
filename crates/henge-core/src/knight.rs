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
//! **What each one is for**, read off every routine that touches the three
//! bytes. Strength is read by `CalcDamage` and nowhere else in a fight.
//! Constitution is read by the routine at 0x28d and nowhere else. Endurance is
//! read by the routine at 0x2e7, whose stride byte at `+0x3e` has exactly one
//! reader, `_MAP:DistanceDONE`: `shl ax, 1` four times, into the day's step
//! budget. So endurance is how far you get in a day, sixteen steps to the
//! point, and nothing about reach; the one other reader is `RatLeapHit`,
//! which sets how many shakes a ratman on your head takes to `endurance + 6`.
//!
//! **What makes them grow.** `HGAbility` in `_STATUS` is the status screen's
//! `Increase Strength`, `Increase Endurance` and `Increase Constitution`
//! lines: `inc byte [bx+si]` on the chosen byte, ten health on the spot for
//! constitution, and `[0x718]` experience taken off. `_MAP:KnightXP` does the
//! same for a computer knight with the ability picked by the wizard's own
//! roll, and `WIZBestowAbility` hands one out free. `CheckMaxAbility` counts
//! the three against five and the picker refuses a sixth point, so five is
//! the ceiling; `MysticAbility` takes one off, never below one.
//!
//! **What is not.** `InitKnights` distinguishes the four only by name, by colour
//! and by which corner of the map they start in. Their stat blocks are
//! identical. The shape here allows them to differ, because the data allows it
//! and because the build order asks for it, but what ships is what the original
//! had, and anything else would be an invention wearing recovered clothes.

use crate::item::{Items, Virtue};
use serde::{Deserialize, Serialize};

/// `shl ax, 1` four times in `_MAP:DistanceDONE`: one point of stride is
/// sixteen steps of the day's travel, and a step there is one pixel.
pub const STEPS_PER_STRIDE: u32 = 16;

/// The ceiling on every ability. `_WIZARD:CheckMaxAbility` compares all three
/// against five and the picker below it (`cmp byte [bx+di], 5; je retry`)
/// will not hand a point to one already there.
pub const MAX_ABILITY: i32 = 5;

/// The floor. `MysticAbility` opens with `cmp byte [bx+di], 1; je ExitMystic`.
pub const MIN_ABILITY: i32 = 1;

/// One of the three things a knight can get better at.
///
/// The set and the order are the knight record's own: `+0x2e`, `+0x2f`,
/// `+0x30`, and the labels `_STATUS` prints beside them are `ab4` Strength,
/// `ab6` Constitution and `ab5` Endurance.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum Ability {
    Strength,
    Constitution,
    Endurance,
}

impl Ability {
    pub const ALL: [Ability; 3] = [Ability::Strength, Ability::Constitution, Ability::Endurance];

    /// The record offset, which is how every routine names the ability.
    pub fn offset(self) -> u8 {
        match self {
            Ability::Strength => 0x2e,
            Ability::Constitution => 0x2f,
            Ability::Endurance => 0x30,
        }
    }

    /// `ab4`, `ab6`, `ab5`.
    pub fn name(self) -> &'static str {
        match self {
            Ability::Strength => "Strength",
            Ability::Constitution => "Constitution",
            Ability::Endurance => "Endurance",
        }
    }

    /// `ab1`, `ab3`, `ab2`: the status screen's three gadgets, verbatim.
    pub fn increase_line(self) -> &'static str {
        match self {
            Ability::Strength => "Increase Strength",
            Ability::Constitution => "Increase Constitution",
            Ability::Endurance => "Increase Endurance",
        }
    }
}

/// How the original picks an ability when nobody chose one.
///
/// The picker at image `0xb7e9` rolls, keeps four bits, folds 9..15 down to
/// 2..8, and indexes `WIZABL`, nine entries of a record offset and a message:
/// strength, endurance, constitution, endurance, strength, endurance,
/// constitution, constitution, strength. The fold is why the entries are not
/// equally likely: 0 and 1 come up once in sixteen and the rest twice, which
/// makes it five sixteenths strength, six constitution and five endurance.
///
/// One slip is worth recording rather than reproducing: the picker scales the
/// index by eight where the entries are four bytes long, so as shipped only
/// the even entries can be reached and the odd folds read past the table. The
/// table's own contents are taken as the intent.
pub const ABILITY_WEIGHTS: [(Ability, u32); 3] =
    [(Ability::Strength, 5), (Ability::Constitution, 6), (Ability::Endurance, 5)];

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
/// The abilities change: [`Knight::raise`] is what `HGAbility` and
/// `WIZBestowAbility` do to the record, [`Knight::lower`] what `MysticAbility`
/// does. What the run pays for a point is the run's business.
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

    pub fn ability(&self, which: Ability) -> i32 {
        match which {
            Ability::Strength => self.strength,
            Ability::Constitution => self.constitution,
            Ability::Endurance => self.endurance,
        }
    }

    fn ability_mut(&mut self, which: Ability) -> &mut i32 {
        match which {
            Ability::Strength => &mut self.strength,
            Ability::Constitution => &mut self.constitution,
            Ability::Endurance => &mut self.endurance,
        }
    }

    /// Put a point into one ability: `inc byte [bx+si]`. False at the
    /// ceiling, which is what `CheckMaxAbility` exists to say.
    pub fn raise(&mut self, which: Ability) -> bool {
        let slot = self.ability_mut(which);
        if *slot >= MAX_ABILITY {
            return false;
        }
        *slot += 1;
        true
    }

    /// Take a point off one ability: `MysticAbility`'s `dec byte [bx+di]`,
    /// refused at one so a knight is never hollowed out entirely.
    pub fn lower(&mut self, which: Ability) -> bool {
        let slot = self.ability_mut(which);
        if *slot <= MIN_ABILITY {
            return false;
        }
        *slot -= 1;
        true
    }

    /// Nothing left to buy: `CheckMaxAbility` counting three.
    pub fn maxed(&self) -> bool {
        Ability::ALL.iter().all(|a| self.ability(*a) >= MAX_ABILITY)
    }

    /// The ability a roll picks, weighted as `WIZABL` is and skipping any
    /// already at the ceiling, the way the picker at `0xb7e9` rolls again.
    /// None only when all three are there, its `sub bx, bx; ret`.
    pub fn rolled_ability(&self, roll: u32) -> Option<Ability> {
        let open: Vec<(Ability, u32)> = ABILITY_WEIGHTS
            .iter()
            .copied()
            .filter(|(a, _)| self.ability(*a) < MAX_ABILITY)
            .collect();
        let total: u32 = open.iter().map(|(_, w)| *w).sum();
        if total == 0 {
            return None;
        }
        let mut n = roll % total;
        for (a, w) in open {
            if n < w {
                return Some(a);
            }
            n -= w;
        }
        None
    }

    /// `10 * constitution + armour + 10`, from the routine at 0x28d. The
    /// rings that routine also counts are on the run, which holds the pack.
    pub fn max_health(&self, items: &Items) -> i32 {
        self.constitution * 10 + self.armour_worn(items).0 + 10
    }

    /// `2 * endurance + armour + 4`, from the routine at 0x2e7, into `+0x3e`.
    pub fn stride(&self, items: &Items) -> i32 {
        self.endurance * 2 + self.armour_worn(items).1 + 4
    }

    /// How far this knight travels in a day, in map steps.
    ///
    /// **Recovered, and it is what endurance is for.** `_MAP:DistanceDONE`
    /// reads the stride byte, shifts it left four and stores it as the step
    /// budget the map loop counts against before the turn passes on.
    pub fn steps_per_day(&self, items: &Items) -> u32 {
        self.stride(items).max(1) as u32 * STEPS_PER_STRIDE
    }

    /// What strength and a blade add to a swing, from `CalcDamage`: `cl =
    /// [si+0x2e]; add ax, cx`, then two, three or five for the three better
    /// blades. The base of the sum is the swing's own entry in the `*Dam`
    /// table, which is the arena's business; this is the sheet's part.
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
            name: "SIR GODBER".into(),
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

    /// `CheckMaxAbility` counts how many of the three have reached five and
    /// the picker refuses a point to one that has. Five is the ceiling.
    #[test]
    fn an_ability_stops_at_five() {
        let mut k = opening();
        for _ in 0..4 {
            assert!(k.raise(Ability::Strength));
        }
        assert_eq!(k.strength, 5);
        assert!(!k.raise(Ability::Strength), "nothing above five");
        assert_eq!(k.strength, 5);
        assert!(!k.maxed(), "the other two are still at one");
        for a in Ability::ALL {
            while k.raise(a) {}
        }
        assert!(k.maxed());
        assert_eq!(k.rolled_ability(3), None, "and the roll has nothing to pick");
    }

    /// `MysticAbility`: a point comes off, and never the last one.
    #[test]
    fn an_ability_is_never_lowered_below_one() {
        let mut k = opening();
        assert!(!k.lower(Ability::Constitution), "already at one");
        k.raise(Ability::Constitution);
        assert!(k.lower(Ability::Constitution));
        assert_eq!(k.constitution, 1);
    }

    /// `WIZABL`'s nine entries under the picker's fold: five sixteenths
    /// strength, six constitution, five endurance, and a maxed ability is
    /// rolled again rather than handed a sixth point.
    #[test]
    fn a_rolled_ability_follows_the_recovered_weights() {
        let k = opening();
        let mut seen = [0u32; 3];
        for roll in 0..16u32 {
            seen[k.rolled_ability(roll).unwrap() as usize] += 1;
        }
        assert_eq!(seen, [5, 6, 5]);
        let mut full = opening();
        while full.raise(Ability::Constitution) {}
        for roll in 0..64u32 {
            assert_ne!(full.rolled_ability(roll), Some(Ability::Constitution));
        }
    }

    /// `shl ax, 1` four times in `DistanceDONE`: sixteen steps of road a
    /// point of stride. This is what endurance buys.
    #[test]
    fn endurance_is_sixteen_steps_of_road_a_point() {
        let (mut k, items) = (opening(), kit());
        assert_eq!(k.steps_per_day(&items), 96);
        k.endurance = 5;
        assert_eq!(k.steps_per_day(&items), 224);
        k.armour = "chain_mail".into();
        assert_eq!(k.steps_per_day(&items), 256, "and mail is worth another two");
    }

    #[test]
    fn a_sheet_survives_serialization() {
        let k = opening();
        let json = serde_json::to_string(&k).unwrap();
        assert_eq!(serde_json::from_str::<Knight>(&json).unwrap(), k);
    }
}
