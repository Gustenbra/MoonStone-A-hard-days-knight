//! A run: what carries between fights.
//!
//! Without this the game is a fight simulator. Every bout starts fresh, dying
//! costs nothing, and there is no reason to avoid a fight or to break one off.
//!
//! The rule that makes travel matter: **wounds persist, and only travelling
//! mends them**. Walking is how you heal, and walking is also how you run into
//! trouble, so the same action that repairs you is the one that risks you. That
//! tension is the whole game loop.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub health: i32,
    pub max_health: i32,
    pub day: u32,
    pub victories: u32,
    pub fights: u32,
    pub over: bool,
    /// Steps of travel banked toward the next point of healing.
    progress: u32,
    /// How far you must walk to mend one point.
    pub steps_per_point: u32,
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
            progress: 0,
            steps_per_point: 12,
        }
    }

    pub fn alive(&self) -> bool {
        !self.over && self.health > 0
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

    /// The health to enter the next fight with. Whatever you have left.
    pub fn health_for_fight(&self) -> i32 {
        self.health.max(1)
    }

    /// Record how a fight ended. Returns whether the run continues.
    ///
    /// Winning restores nothing. A victory that healed you would remove the
    /// reason to ever avoid a fight, and turn the map into a corridor between
    /// free health.
    pub fn finished_fight(&mut self, health_left: i32, won: bool) -> bool {
        if self.over {
            return false;
        }
        self.fights += 1;
        self.health = health_left.max(0);
        if won {
            self.victories += 1;
        }
        if self.health <= 0 {
            self.over = true;
        }
        !self.over
    }

    /// Days spent in someone's care. Wounds close, and the price is time.
    ///
    /// Time is the only thing a run owns: there is no money in this game yet,
    /// and days are not free, because the calendar is what the map's ambushes
    /// and the moon are hung on. Returns the days actually spent, which is
    /// zero when there was nothing to mend: a healer does not take a week off
    /// you to look at an unmarked man.
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

    #[test]
    fn wounds_carry_into_the_next_fight() {
        let mut r = Run::new(100);
        assert_eq!(r.health_for_fight(), 100);
        r.finished_fight(40, true);
        assert_eq!(r.health_for_fight(), 40, "you do not get a fresh start");
        assert_eq!(r.victories, 1);
    }

    #[test]
    fn walking_mends_you_but_only_so_far() {
        let mut r = Run::new(100);
        r.finished_fight(90, true);
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
        r.finished_fight(25, true);
        assert_eq!(r.health, 25, "a victory is not a reward of health");
    }

    #[test]
    fn dying_ends_the_run_for_good() {
        let mut r = Run::new(100);
        assert!(!r.finished_fight(0, false));
        assert!(r.over);
        assert!(!r.alive());
        // Nothing continues afterwards: no healing, no further fights, no days.
        assert!(!r.travelled());
        assert!(!r.finished_fight(50, true));
        let before = r.day;
        r.new_day();
        assert_eq!(r.day, before);
    }

    #[test]
    fn a_restart_wipes_the_slate() {
        let mut r = Run::new(100);
        r.finished_fight(0, false);
        r.day = 9;
        r.restart();
        assert_eq!(r, Run::new(100));
    }

    #[test]
    fn the_tally_counts_fights_and_wins_separately() {
        let mut r = Run::new(100);
        r.finished_fight(80, true);
        r.finished_fight(60, false);
        r.finished_fight(30, true);
        assert_eq!(r.fights, 3);
        assert_eq!(r.victories, 2);
    }

    #[test]
    fn a_healer_trades_days_for_health() {
        let mut r = Run::new(100);
        r.finished_fight(20, true);
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
        r.finished_fight(0, false);
        assert_eq!(r.tended(4), 0);
    }

    #[test]
    fn a_run_survives_serialization() {
        let mut r = Run::new(100);
        r.finished_fight(55, true);
        r.new_day();
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<Run>(&json).unwrap(), r);
    }
}
