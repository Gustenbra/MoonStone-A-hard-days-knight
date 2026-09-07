//! Places on the map: where you can go, and what you can do once you are there.
//!
//! Until now the overworld was terrain and ambushes, which meant walking had
//! exactly one outcome. A place is somewhere walking can take you *on purpose*.
//!
//! Everything here is logic and data with no drawing in it: which coordinates a
//! place occupies, what its menu offers, and what choosing an option does to a
//! [`Run`]. The renderer is handed a [`PlaceDef`] and a [`Visit`] and decides
//! nothing.
//!
//! **The price of a service is time.** There is no money in this game yet and
//! inventing some would be inventing a whole economy, so the healer charges the
//! only thing a run actually owns: days. Days matter because the map keeps its
//! ambushes, so a week under a healer is a week the world moved without you.

use crate::run::Run;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What choosing a menu option does.
///
/// Deliberately a closed set. An option the game cannot honour yet is
/// [`Effect::Closed`], which says so, rather than a live-looking option that
/// silently does nothing.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "do", rename_all = "kebab-case")]
pub enum Effect {
    /// Someone tends your wounds, and the price is days.
    Heal {
        days: u32,
        /// Said when there was something to mend.
        said: String,
        /// Said when there was not. A healer does not take your time for nothing.
        refused: String,
    },
    /// On the sign, not built yet. The game admits it rather than pretending.
    Closed { said: String },
    /// Back out onto the map.
    Leave,
}

impl Effect {
    /// Whether this option can actually be taken. The renderer dims the rest.
    pub fn available(&self) -> bool {
        !matches!(self, Effect::Closed { .. })
    }
}

/// One line of a place's menu.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub label: String,
    pub effect: Effect,
}

/// A place, as it is authored in the pack.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PlaceDef {
    pub name: String,
    /// Asset id of the full-screen backdrop; its palette is `palette.<scene>`.
    pub scene: String,
    /// Where it sits on the map image.
    pub x: i32,
    pub y: i32,
    /// How close you have to walk before you are inside.
    pub radius: i32,
    /// Where the renderer puts the menu: `[x, y, w, h]`.
    ///
    /// Layout rather than rules, but it belongs beside the place and not in
    /// code, because it depends entirely on where that particular backdrop has
    /// room for words.
    pub menu: [i32; 4],
    pub options: Vec<Choice>,
}

impl PlaceDef {
    pub fn covers(&self, x: i32, y: i32) -> bool {
        let (dx, dy) = (x - self.x, y - self.y);
        dx * dx + dy * dy <= self.radius * self.radius
    }

    /// Distance squared, for picking the closest of several. Squared so the
    /// simulation never needs a square root, and so never needs a float.
    pub fn distance2(&self, x: i32, y: i32) -> i32 {
        let (dx, dy) = (x - self.x, y - self.y);
        dx * dx + dy * dy
    }
}

/// Every place in the world, keyed by id. A `BTreeMap` so that iteration order
/// is defined and two machines pick the same place out of an overlap.
pub type Places = BTreeMap<String, PlaceDef>;

/// The place you are nearest, within `range` pixels. Used to name what you are
/// walking towards before you get there.
pub fn nearest(places: &Places, x: i32, y: i32, range: i32) -> Option<(&str, &PlaceDef)> {
    places
        .iter()
        .filter(|(_, d)| d.distance2(x, y) <= range * range)
        .min_by_key(|(id, d)| (d.distance2(x, y), id.as_str()))
        .map(|(id, d)| (id.as_str(), d))
}

/// Whether the traveller is standing in a place, and whether they just walked in.
///
/// Arriving has to be an edge rather than a state: leaving a town puts you back
/// on top of it, so a state test would walk you straight back inside on the
/// very next tick and you could never get out.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Approach {
    inside: Option<String>,
}

impl Approach {
    /// Advance to a map position. Returns the place walked into, once.
    pub fn step(&mut self, places: &Places, x: i32, y: i32) -> Option<String> {
        let here = places
            .iter()
            .filter(|(_, d)| d.covers(x, y))
            .min_by_key(|(id, d)| (d.distance2(x, y), id.as_str()))
            .map(|(id, _)| id.clone());
        let arrived = match (&self.inside, &here) {
            (Some(was), Some(id)) if was == id => None,
            (_, Some(id)) => Some(id.clone()),
            _ => None,
        };
        self.inside = here;
        arrived
    }

    pub fn inside(&self) -> Option<&str> {
        self.inside.as_deref()
    }
}

/// Being in a place: which one, where the highlight sits, and what was last
/// said to you.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Visit {
    pub place: String,
    pub cursor: usize,
    /// The last thing that happened here, for the renderer to show.
    pub said: String,
}

/// What a choice did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// Still here. `days` is what the choice cost in time.
    Stayed { days: u32 },
    /// Out onto the map.
    Left,
}

impl Visit {
    pub fn open(place: &str) -> Visit {
        Visit { place: place.to_string(), cursor: 0, said: String::new() }
    }

    /// Move the highlight, wrapping. A menu this short reads better as a ring
    /// than as a list with ends you can jam against.
    pub fn move_by(&mut self, def: &PlaceDef, dy: i32) {
        let n = def.options.len() as i32;
        if n == 0 || dy == 0 {
            return;
        }
        self.cursor = (((self.cursor as i32 + dy) % n + n) % n) as usize;
        // What a place last said to you belongs to the option you chose.
        // Leaving it up while the highlight moves puts a refusal under a
        // live option and reads as though that option refused you.
        self.said.clear();
    }

    pub fn selected<'a>(&self, def: &'a PlaceDef) -> Option<&'a Choice> {
        def.options.get(self.cursor)
    }

    /// Take the highlighted option.
    pub fn choose(&mut self, def: &PlaceDef, run: &mut Run) -> Answer {
        let Some(choice) = def.options.get(self.cursor) else {
            return Answer::Left;
        };
        match &choice.effect {
            Effect::Leave => Answer::Left,
            Effect::Closed { said } => {
                self.said = said.clone();
                Answer::Stayed { days: 0 }
            }
            Effect::Heal { days, said, refused } => {
                let spent = run.tended(*days);
                self.said = if spent > 0 { said.clone() } else { refused.clone() };
                Answer::Stayed { days: spent }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn healer() -> PlaceDef {
        PlaceDef {
            name: "The Healer".into(),
            scene: "scene.hea".into(),
            x: 100,
            y: 100,
            radius: 5,
            menu: [8, 8, 100, 100],
            options: vec![
                Choice {
                    label: "Merchant".into(),
                    effect: Effect::Closed { said: "Nothing to sell you.".into() },
                },
                Choice {
                    label: "Tend my wounds".into(),
                    effect: Effect::Heal {
                        days: 3,
                        said: "You are made whole.".into(),
                        refused: "You have no need of me.".into(),
                    },
                },
                Choice { label: "Leave".into(), effect: Effect::Leave },
            ],
        }
    }

    fn world() -> Places {
        let mut p = Places::new();
        p.insert("healer".into(), healer());
        let mut far = healer();
        far.name = "Highwood".into();
        far.x = 200;
        far.y = 40;
        p.insert("highwood".into(), far);
        p
    }

    /// A refusal belongs to the option that refused you. Left on screen while
    /// the highlight moves, it sits under a live option and reads as though
    /// that option had refused you.
    #[test]
    fn the_last_message_does_not_follow_the_highlight() {
        let def = healer();
        let mut run = Run::new(100);
        let mut v = Visit::open("healer");
        assert!(matches!(def.options[v.cursor].effect, Effect::Closed { .. }));
        v.choose(&def, &mut run);
        assert!(!v.said.is_empty(), "a shut option should say why");
        v.move_by(&def, 1);
        assert!(v.said.is_empty(), "the refusal must not sit under the next option");
    }

    #[test]
    fn a_place_is_entered_by_walking_onto_it() {
        let mut a = Approach::default();
        assert_eq!(a.step(&world(), 60, 60), None, "nowhere near");
        assert_eq!(a.step(&world(), 102, 101).as_deref(), Some("healer"));
    }

    #[test]
    fn walking_out_of_a_place_does_not_walk_you_back_in() {
        let places = world();
        let mut a = Approach::default();
        assert!(a.step(&places, 100, 100).is_some());
        // Leaving the menu leaves you standing on the town. Standing still, or
        // shuffling about inside it, must not re-open it.
        assert_eq!(a.step(&places, 100, 100), None);
        assert_eq!(a.step(&places, 102, 102), None);
        assert_eq!(a.inside(), Some("healer"));
        // Step off, and it becomes enterable again.
        assert_eq!(a.step(&places, 140, 140), None);
        assert_eq!(a.inside(), None);
        assert_eq!(a.step(&places, 100, 100).as_deref(), Some("healer"));
    }

    #[test]
    fn stepping_straight_from_one_place_into_another_enters_the_second() {
        let places = world();
        let mut a = Approach::default();
        a.step(&places, 100, 100);
        assert_eq!(a.step(&places, 200, 40).as_deref(), Some("highwood"));
    }

    #[test]
    fn the_healer_mends_you_and_charges_days() {
        let def = healer();
        let mut run = Run::new(100);
        run.finished_fight(30, true);
        let mut v = Visit::open("healer");
        v.move_by(&def, 1);
        let day = run.day;
        assert_eq!(v.choose(&def, &mut run), Answer::Stayed { days: 3 });
        assert_eq!(run.health, 100, "wounds close");
        assert_eq!(run.day, day + 3, "and time is what it cost");
        assert_eq!(v.said, "You are made whole.");
    }

    #[test]
    fn a_healer_will_not_take_days_for_nothing() {
        let (def, mut run) = (healer(), Run::new(100));
        let mut v = Visit::open("healer");
        v.move_by(&def, 1);
        let day = run.day;
        assert_eq!(v.choose(&def, &mut run), Answer::Stayed { days: 0 });
        assert_eq!(run.day, day, "unwounded, so no time passes");
        assert_eq!(v.said, "You have no need of me.");
    }

    #[test]
    fn an_option_that_is_not_built_yet_says_so_and_costs_nothing() {
        let (def, mut run) = (healer(), Run::new(100));
        let mut v = Visit::open("healer");
        assert!(!def.options[0].effect.available());
        assert_eq!(v.choose(&def, &mut run), Answer::Stayed { days: 0 });
        assert_eq!(run, Run::new(100), "the run is untouched");
    }

    #[test]
    fn leaving_answers_that_you_left() {
        let (def, mut run) = (healer(), Run::new(100));
        let mut v = Visit::open("healer");
        v.move_by(&def, -1);
        assert_eq!(v.cursor, 2, "up from the top wraps to the bottom");
        assert_eq!(v.choose(&def, &mut run), Answer::Left);
    }

    #[test]
    fn the_highlight_is_a_ring() {
        let (def, mut v) = (healer(), Visit::open("healer"));
        for expect in [1, 2, 0, 1] {
            v.move_by(&def, 1);
            assert_eq!(v.cursor, expect);
        }
        v.move_by(&def, 0);
        assert_eq!(v.cursor, 1, "standing still moves nothing");
    }

    #[test]
    fn the_nearest_place_is_the_one_you_are_walking_towards() {
        let places = world();
        assert_eq!(nearest(&places, 110, 105, 20).map(|(id, _)| id), Some("healer"));
        assert_eq!(nearest(&places, 110, 105, 4), None, "out of range");
    }

    #[test]
    fn a_place_survives_serialization() {
        let def = healer();
        let json = serde_json::to_string(&def).unwrap();
        assert_eq!(serde_json::from_str::<PlaceDef>(&json).unwrap(), def);
        assert!(json.contains("\"do\":\"heal\""), "effects are tagged in the data");
    }

}
