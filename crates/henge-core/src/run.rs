//! A run: what carries between fights.
//!
//! Without this the game is a fight simulator. Every bout starts fresh, dying
//! costs nothing, and there is no reason to avoid a fight or to break one off.
//!
//! The rule that makes travel matter: **wounds persist, and only travelling
//! mends them**. Walking is how you heal, and walking is also how you run into
//! trouble, so the same action that repairs you is the one that risks you. That
//! tension is the whole game loop.
//!
//! A run also carries what it has won: a purse and a pack. Coin comes off the
//! fallen, so a fight is worth taking as well as worth avoiding, and the pack
//! is the only reason a town has anything to sell. Both are ordinary state,
//! serialized with everything else, so a run still round-trips.

use crate::item::{Inventory, ItemDef, Items, Loss, Purchase, Virtue};
use serde::{Deserialize, Serialize};

/// What came of using something you carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Used {
    /// It did what it does, and was spent if it is the spending kind.
    Did,
    /// You are carrying none.
    HaveNone,
    /// It would do nothing right now: a healing flask on an unmarked man is
    /// not opened, because opening it would waste it.
    Pointless,
    /// No such item in the packs. A data error, not a game state.
    Unknown,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub health: i32,
    pub max_health: i32,
    pub day: u32,
    pub victories: u32,
    pub fights: u32,
    pub over: bool,
    /// The purse. Coin off the fallen, spent at a town.
    pub gold: u32,
    /// What you carry.
    pub kit: Inventory,
    /// Steps of travel banked toward the next point of healing.
    progress: u32,
    /// How far you must walk to mend one point.
    pub steps_per_point: u32,
    /// One step of travel in this many meets a cutpurse. Zero is a safe road.
    pub theft_odds: u32,
    /// The run's own randomness, carried in the state and never taken from the
    /// system, so two machines walking the same road are robbed on the same
    /// step.
    seed: u32,
}

impl Run {
    pub fn new(max_health: i32) -> Run {
        Run {
            health: max_health,
            max_health,
            day: 1,
            victories: 0,
            fights: 0,
            over: false,
            gold: 0,
            kit: Inventory::default(),
            progress: 0,
            steps_per_point: 12,
            theft_odds: 700,
            seed: 0x51ed_270b,
        }
    }

    pub fn alive(&self) -> bool {
        !self.over && self.health > 0
    }

    /// Small xorshift, the same shape the overworld uses. Deterministic,
    /// seedable, and enough for deciding who meets a thief.
    fn next_random(&mut self) -> u32 {
        let mut s = self.seed;
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        self.seed = s;
        s
    }

    /// One step of travel. Returns true when a point of health was recovered,
    /// so a caller can make it audible or visible.
    pub fn travelled(&mut self) -> bool {
        if !self.alive() || self.health >= self.max_health {
            return false;
        }
        self.progress += 1;
        if self.progress < self.steps_per_point {
            return false;
        }
        self.progress = 0;
        self.health = (self.health + 1).min(self.max_health);
        true
    }

    /// A cutpurse on the road. Returns what was taken, if anything.
    ///
    /// This is the world's half of `TAKEFROMKNIGHT`: without it, a pack is a
    /// list that only ever grows and losing is a method nothing calls. A thief
    /// prefers coin and settles for goods, and someone carrying neither is not
    /// worth robbing.
    ///
    /// Not recovered from the original, which names the routine but not its
    /// trigger. Rolled per travelled step so the risk is in the walking, like
    /// every other risk on this map.
    pub fn waylaid(&mut self) -> Option<Loss> {
        if !self.alive() || self.theft_odds == 0 {
            return None;
        }
        // The roll is taken on every step of the road, whether or not there is
        // anything on you worth taking. Rolling only when you are carrying
        // something would tie the sequence to the moment you got it, and every
        // run in the world would then be robbed the same number of steps after
        // its first purse.
        let roll = self.next_random();
        if self.gold == 0 && self.kit.is_empty() {
            return None;
        }
        if roll % self.theft_odds != 0 {
            return None;
        }
        if self.gold > 0 {
            // A share of the purse rather than all of it: being cleaned out by
            // one unlucky step would make carrying coin pointless.
            let taken = (self.gold / 4).max(1).min(self.gold);
            self.gold -= taken;
            return Some(Loss::Gold(taken));
        }
        self.kit.take_one(roll).map(Loss::Item)
    }

    /// The health to enter the next fight with. Whatever you have left.
    pub fn health_for_fight(&self) -> i32 {
        self.health.max(1)
    }

    /// Record how a fight ended. Returns whether the run continues.
    ///
    /// Winning restores nothing. A victory that healed you would remove the
    /// reason to ever avoid a fight, and turn the map into a corridor between
    /// free health.
    ///
    /// `purse` is what the fallen were carrying. A purse is picked up after the
    /// fight, so only a winner still on their feet collects it: coin is the
    /// reward for finishing a fight, not for being in one.
    pub fn finished_fight(&mut self, health_left: i32, won: bool, purse: u32) -> bool {
        if self.over {
            return false;
        }
        self.fights += 1;
        self.health = health_left.max(0);
        if self.health <= 0 {
            self.over = true;
        }
        if won {
            self.victories += 1;
            if !self.over {
                self.gold = self.gold.saturating_add(purse);
            }
        }
        !self.over
    }

    /// Coin from somewhere that is not a corpse: the wizard's `BESTOWGOLD`,
    /// eventually, and a dice game before that.
    pub fn earn(&mut self, gold: u32) {
        self.gold = self.gold.saturating_add(gold);
    }

    /// Pay out. Refuses rather than going into debt, so no caller has to
    /// remember to check first.
    pub fn spend(&mut self, cost: u32) -> bool {
        if self.gold < cost {
            return false;
        }
        self.gold -= cost;
        true
    }

    /// Buy one of something. The price lives with the item, so nowhere else has
    /// to know it and nowhere else can disagree about it.
    pub fn buy(&mut self, id: &str, items: &Items) -> Purchase {
        let Some(def) = items.get(id) else {
            return Purchase::Unknown;
        };
        if self.kit.room() == 0 {
            return Purchase::NoRoom;
        }
        if self.gold < def.price {
            return Purchase::TooDear;
        }
        self.gold -= def.price;
        self.kit.take(id, 1);
        Purchase::Bought { paid: def.price }
    }

    /// Use one of something you carry. A spent item leaves the pack, which is
    /// the everyday half of losing things.
    pub fn use_item(&mut self, id: &str, items: &Items) -> Used {
        let Some(def) = items.get(id) else {
            return Used::Unknown;
        };
        if self.kit.count(id) == 0 {
            return Used::HaveNone;
        }
        if !self.apply(def) {
            return Used::Pointless;
        }
        if def.consumed {
            self.kit.lose(id, 1);
        }
        Used::Did
    }

    /// Do what an item does. False when it would achieve nothing.
    fn apply(&mut self, def: &ItemDef) -> bool {
        match &def.virtue {
            // `DRINKPOTIONHEAL`.
            Virtue::Heal { health } => {
                if !self.alive() || self.health >= self.max_health {
                    return false;
                }
                self.health = (self.health + health).min(self.max_health);
                true
            }
            Virtue::Inert => false,
        }
    }

    /// Days spent in someone's care. Wounds close, and the price is time.
    ///
    /// Days are not free, because the calendar is what the map's ambushes and
    /// the moon are hung on. Returns the days actually spent, which is zero
    /// when there was nothing to mend: a healer does not take a week off you to
    /// look at an unmarked man.
    ///
    /// A town healer may also want coin; that price is charged by the caller
    /// through [`Run::spend`], because whether a place asks for money is a
    /// property of the place and not of being mended.
    pub fn tended(&mut self, days: u32) -> u32 {
        if !self.alive() || self.health >= self.max_health {
            return 0;
        }
        self.health = self.max_health;
        self.progress = 0;
        for _ in 0..days {
            self.new_day();
        }
        days
    }

    pub fn new_day(&mut self) {
        if self.alive() {
            self.day += 1;
        }
    }

    /// Start again. A finished run is read, then cleared.
    pub fn restart(&mut self) {
        *self = Run::new(self.max_health);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::Virtue;

    fn shop() -> Items {
        let mut items = Items::new();
        items.insert(
            "potion".into(),
            ItemDef {
                name: "Flask of healing".into(),
                price: 25,
                virtue: Virtue::Heal { health: 40 },
                consumed: true,
            },
        );
        items.insert(
            "key".into(),
            ItemDef {
                name: "Iron key".into(),
                price: 60,
                virtue: Virtue::Inert,
                consumed: false,
            },
        );
        items
    }

    #[test]
    fn wounds_carry_into_the_next_fight() {
        let mut r = Run::new(100);
        assert_eq!(r.health_for_fight(), 100);
        r.finished_fight(40, true, 0);
        assert_eq!(r.health_for_fight(), 40, "you do not get a fresh start");
        assert_eq!(r.victories, 1);
    }

    #[test]
    fn walking_mends_you_but_only_so_far() {
        let mut r = Run::new(100);
        r.finished_fight(90, true, 0);
        let mut healed = 0;
        for _ in 0..(r.steps_per_point * 30) {
            if r.travelled() {
                healed += 1;
            }
        }
        assert_eq!(r.health, 100);
        assert_eq!(healed, 10, "exactly the missing points, no more");
        assert!(!r.travelled(), "already whole, so nothing to mend");
    }

    #[test]
    fn winning_heals_nothing() {
        let mut r = Run::new(100);
        r.finished_fight(25, true, 0);
        assert_eq!(r.health, 25, "a victory is not a reward of health");
    }

    #[test]
    fn dying_ends_the_run_for_good() {
        let mut r = Run::new(100);
        assert!(!r.finished_fight(0, false, 0));
        assert!(r.over);
        assert!(!r.alive());
        // Nothing continues afterwards: no healing, no further fights, no days.
        assert!(!r.travelled());
        assert!(!r.finished_fight(50, true, 0));
        let before = r.day;
        r.new_day();
        assert_eq!(r.day, before);
    }

    #[test]
    fn a_restart_wipes_the_slate() {
        let mut r = Run::new(100);
        r.finished_fight(0, false, 0);
        r.day = 9;
        r.gold = 400;
        r.kit.take("potion", 3);
        r.restart();
        assert_eq!(r, Run::new(100), "purse and pack go with everything else");
    }

    #[test]
    fn the_tally_counts_fights_and_wins_separately() {
        let mut r = Run::new(100);
        r.finished_fight(80, true, 0);
        r.finished_fight(60, false, 0);
        r.finished_fight(30, true, 0);
        assert_eq!(r.fights, 3);
        assert_eq!(r.victories, 2);
    }

    #[test]
    fn a_healer_trades_days_for_health() {
        let mut r = Run::new(100);
        r.finished_fight(20, true, 0);
        assert_eq!(r.tended(4), 4);
        assert_eq!(r.health, 100);
        assert_eq!(r.day, 5, "four days passed while you lay there");
    }

    #[test]
    fn nobody_charges_a_whole_man() {
        let mut r = Run::new(100);
        assert_eq!(r.tended(4), 0, "nothing to mend, so no time spent");
        assert_eq!(r.day, 1);
        // And a dead man is past helping.
        r.finished_fight(0, false, 0);
        assert_eq!(r.tended(4), 0);
    }

    #[test]
    fn a_run_survives_serialization() {
        let mut r = Run::new(100);
        r.finished_fight(55, true, 30);
        r.kit.take("potion", 2);
        r.new_day();
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<Run>(&json).unwrap(), r);
    }

    // Gold.

    #[test]
    fn a_won_fight_pays_and_a_lost_one_does_not() {
        let mut r = Run::new(100);
        r.finished_fight(60, true, 40);
        assert_eq!(r.gold, 40, "the fallen were carrying it");
        r.finished_fight(30, false, 40);
        assert_eq!(r.gold, 40, "and a fight you did not win pays nothing");
    }

    /// A purse is picked up after the fight. A man face down in the mud picks
    /// up nothing, however many he took with him.
    #[test]
    fn the_dead_collect_no_purse() {
        let mut r = Run::new(100);
        r.finished_fight(0, true, 200);
        assert!(r.over);
        assert_eq!(r.gold, 0);
    }

    #[test]
    fn you_cannot_spend_what_you_have_not_got() {
        let mut r = Run::new(100);
        r.earn(30);
        assert!(!r.spend(31), "and the attempt changes nothing");
        assert_eq!(r.gold, 30);
        assert!(r.spend(30));
        assert_eq!(r.gold, 0);
    }

    #[test]
    fn buying_moves_coin_one_way_and_goods_the_other() {
        let (mut r, items) = (Run::new(100), shop());
        r.earn(60);
        assert_eq!(r.buy("potion", &items), Purchase::Bought { paid: 25 });
        assert_eq!(r.gold, 35);
        assert_eq!(r.kit.count("potion"), 1);
    }

    #[test]
    fn a_merchant_says_which_of_the_two_reasons_he_refused() {
        let (mut r, items) = (Run::new(100), shop());
        assert_eq!(r.buy("potion", &items), Purchase::TooDear, "an empty purse");
        r.earn(1000);
        r.kit.capacity = 1;
        r.buy("potion", &items);
        assert_eq!(r.buy("potion", &items), Purchase::NoRoom, "a full pack");
        assert_eq!(r.gold, 975, "and a refused sale takes nothing");
        assert_eq!(r.buy("moonstone", &items), Purchase::Unknown);
    }

    // Using and losing.

    /// `DRINKPOTIONHEAL`, and the everyday half of `TAKEFROMKNIGHT`: the flask
    /// is gone afterwards.
    #[test]
    fn drinking_a_flask_mends_you_and_empties_it() {
        let (mut r, items) = (Run::new(100), shop());
        r.kit.take("potion", 2);
        r.finished_fight(30, true, 0);
        assert_eq!(r.use_item("potion", &items), Used::Did);
        assert_eq!(r.health, 70);
        assert_eq!(r.kit.count("potion"), 1, "one flask emptied, not both");
    }

    #[test]
    fn a_flask_never_mends_past_whole() {
        let (mut r, items) = (Run::new(100), shop());
        r.kit.take("potion", 1);
        r.finished_fight(80, true, 0);
        r.use_item("potion", &items);
        assert_eq!(r.health, 100, "forty points offered, twenty taken");
    }

    #[test]
    fn an_unmarked_man_does_not_waste_a_flask() {
        let (mut r, items) = (Run::new(100), shop());
        r.kit.take("potion", 1);
        assert_eq!(r.use_item("potion", &items), Used::Pointless);
        assert_eq!(r.kit.count("potion"), 1, "still corked");
    }

    #[test]
    fn you_cannot_drink_a_flask_you_do_not_have() {
        let (mut r, items) = (Run::new(100), shop());
        r.finished_fight(30, true, 0);
        assert_eq!(r.use_item("potion", &items), Used::HaveNone);
        assert_eq!(r.health, 30);
    }

    #[test]
    fn a_thing_with_no_virtue_yet_is_kept_rather_than_spent() {
        let (mut r, items) = (Run::new(100), shop());
        r.kit.take("key", 1);
        assert_eq!(r.use_item("key", &items), Used::Pointless);
        assert_eq!(r.kit.count("key"), 1, "and it is still in the pack");
    }

    /// The world's half of `TAKEFROMKNIGHT`.
    #[test]
    fn a_cutpurse_takes_coin_from_a_man_who_has_it() {
        let mut r = Run::new(100);
        r.earn(80);
        r.theft_odds = 1; // certain, so the test is about the loss and not the odds
        assert_eq!(r.waylaid(), Some(Loss::Gold(20)));
        assert_eq!(r.gold, 60, "a share, not the lot");
    }

    #[test]
    fn a_cutpurse_settles_for_goods_when_the_purse_is_empty() {
        let mut r = Run::new(100);
        r.kit.take("potion", 1);
        r.theft_odds = 1;
        assert_eq!(r.waylaid(), Some(Loss::Item("potion".into())));
        assert!(r.kit.is_empty());
    }

    #[test]
    fn nobody_bothers_robbing_a_pauper() {
        let mut r = Run::new(100);
        r.theft_odds = 1;
        assert_eq!(r.waylaid(), None, "nothing to take");
        r.earn(40);
        r.theft_odds = 0;
        assert_eq!(r.waylaid(), None, "and a safe road takes nothing");
        assert_eq!(r.gold, 40);
    }

    /// Rolling only for a man worth robbing would phase the sequence to the
    /// moment he first had coin, and every run in the world would then be
    /// robbed the same number of steps after its first purse.
    #[test]
    fn the_road_rolls_whether_or_not_you_are_worth_robbing() {
        let mut walked = Run::new(100);
        for _ in 0..30 {
            walked.waylaid();
        }
        walked.earn(400);
        let mut straight = Run::new(100);
        straight.earn(400);
        let take = |r: &mut Run| (0..30).find(|_| r.waylaid().is_some());
        assert_ne!(
            take(&mut walked),
            take(&mut straight),
            "thirty steps of empty road are still thirty steps"
        );
    }

    #[test]
    fn the_road_is_robbed_the_same_way_twice() {
        let mut a = Run::new(100);
        a.earn(500);
        let mut b = a.clone();
        let walk = |r: &mut Run| (0..4000).filter_map(|_| r.waylaid()).collect::<Vec<_>>();
        let (first, second) = (walk(&mut a), walk(&mut b));
        assert_eq!(first, second, "the roll is state, not the clock");
        assert!(!first.is_empty(), "and a long enough road does get robbed");
    }
}
