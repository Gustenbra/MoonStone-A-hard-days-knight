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
//! **Two prices, and they are different prices.** Time is what a service in the
//! wilds costs: days matter because the map keeps its ambushes, so a week under
//! a healer is a week the world moved without you. Coin is what a service in a
//! town costs, now that there is coin. A place may ask for either or both, and
//! which it asks for is authored in the pack rather than decided here.

use crate::item::{Items, Purchase};
use crate::overworld::{TOKEN_H, TOKEN_W};
use crate::run::{Run, Used};
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
    /// Someone tends your wounds. The price is days, and in a town also coin.
    Heal {
        days: u32,
        /// Coin as well as days. Zero for a hermit who wants only your time.
        #[serde(default)]
        gold: u32,
        /// Said when there was something to mend.
        said: String,
        /// Said when there was not. A healer does not take your time for nothing.
        refused: String,
        /// Said when the purse is short. Falls back to `refused` rather than
        /// saying nothing, so a refusal is never silent.
        #[serde(default)]
        too_poor: String,
    },
    /// A stall. One line, one item, at the price the item data names, so the
    /// menu and the goods can never disagree about what a thing costs.
    Buy {
        item: String,
        said: String,
        too_dear: String,
        no_room: String,
    },
    /// Use something you are carrying, here and now.
    Use {
        item: String,
        said: String,
        /// Said when you have none, or when using it would achieve nothing.
        refused: String,
    },
    /// Through a door inside this place: the merchant's stall is not the town
    /// square. The place it names is normally `hidden`, so it exists only as
    /// somewhere you are already standing can send you.
    Go { place: String },
    /// On the sign, not built yet. The game admits it rather than pretending.
    Closed { said: String },
    /// Back out onto the map.
    Leave,
}

impl Effect {
    /// Whether this option is one the game can honour at all. False only for a
    /// door that is not built yet; whether you can afford what is behind an
    /// open one is [`Effect::offered`].
    pub fn available(&self) -> bool {
        !matches!(self, Effect::Closed { .. })
    }

    /// Whether taking this option right now would do anything, given the purse
    /// and the pack. The renderer dims the rest, so a man with eight coins can
    /// see that the flask is out of reach before he tries for it.
    pub fn offered(&self, items: &Items, run: &Run) -> bool {
        match self {
            Effect::Closed { .. } => false,
            Effect::Heal { gold, .. } => run.gold >= *gold,
            Effect::Buy { item, .. } => items
                .get(item)
                .is_some_and(|d| run.gold >= d.price && run.kit.room() > 0),
            Effect::Use { item, .. } => run.kit.count(item) > 0,
            Effect::Go { .. } | Effect::Leave => true,
        }
    }

    /// What this option costs in coin, for a menu to show against its line. The
    /// number lives with the item or with the place, never in the label.
    pub fn cost(&self, items: &Items) -> Option<u32> {
        match self {
            Effect::Buy { item, .. } => items.get(item).map(|d| d.price),
            Effect::Heal { gold, .. } if *gold > 0 => Some(*gold),
            _ => None,
        }
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
    /// The rectangle it occupies on the map image: top-left corner, then size.
    ///
    /// **Recovered.** The original keeps a place as an entry in
    /// `MOON:MapIconsTABLE`, three words of icon number, x and y, and decides
    /// that you are there in `MOON:CheckGROOC`: it takes the icon's width and
    /// height out of the `MI.C` bank with `MOON:GetWIDTH`, does the same for
    /// the traveller's own 8x10 token, and overlaps the two rectangles one axis
    /// at a time. There is no radius and no centre. `x`, `y` are the icon's
    /// top-left, in the same coordinates the traveller's own position uses.
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// Where the renderer puts the menu: `[x, y, w, h]`.
    ///
    /// Layout rather than rules, but it belongs beside the place and not in
    /// code, because it depends entirely on where that particular backdrop has
    /// room for words.
    pub menu: [i32; 4],
    pub options: Vec<Choice>,
    /// Not on the map. A hidden place is a room inside another one, reached
    /// only through an [`Effect::Go`], so its coordinates mean nothing and
    /// walking can never stumble into it.
    #[serde(default)]
    pub hidden: bool,
}

impl PlaceDef {
    /// Whether the traveller's token overlaps this place's icon.
    ///
    /// `MOON:CheckGROOC` calls the same half-open range test twice, once per
    /// axis, and counts the passes; two passes is inside. That is a plain
    /// axis-aligned box overlap, so this is one.
    pub fn covers(&self, x: i32, y: i32) -> bool {
        if self.hidden {
            return false;
        }
        let spans = |a: i32, aw: i32, b: i32, bw: i32| a < b + bw && b < a + aw;
        spans(x, TOKEN_W, self.x, self.w) && spans(y, TOKEN_H, self.y, self.h)
    }

    /// Distance squared between the middle of this place and the middle of the
    /// token, for picking the closest of several and for naming what you are
    /// walking towards. Squared so the simulation never needs a square root,
    /// and so never needs a float.
    pub fn distance2(&self, x: i32, y: i32) -> i32 {
        let dx = (x * 2 + TOKEN_W) - (self.x * 2 + self.w);
        let dy = (y * 2 + TOKEN_H) - (self.y * 2 + self.h);
        (dx * dx + dy * dy) / 4
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
        .filter(|(_, d)| !d.hidden && d.distance2(x, y) <= range * range)
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    /// Still here. `days` is what the choice cost in time.
    Stayed { days: u32 },
    /// Through a door into another place, without going back out to the map.
    Went { place: String },
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
    ///
    /// Every branch either changes the run and says what it did, or changes
    /// nothing and says why. There is no third case, which is what stops a
    /// menu quietly eating a choice.
    pub fn choose(&mut self, def: &PlaceDef, items: &Items, run: &mut Run) -> Answer {
        let Some(choice) = def.options.get(self.cursor) else {
            return Answer::Left;
        };
        match &choice.effect {
            Effect::Leave => Answer::Left,
            Effect::Go { place } => Answer::Went { place: place.clone() },
            Effect::Closed { said } => {
                self.said = said.clone();
                Answer::Stayed { days: 0 }
            }
            Effect::Heal { days, gold, said, refused, too_poor } => {
                // Coin is asked for at the door, before the days are spent, so
                // a man who cannot pay does not lose a week finding out.
                if run.gold < *gold {
                    self.said =
                        if too_poor.is_empty() { refused.clone() } else { too_poor.clone() };
                    return Answer::Stayed { days: 0 };
                }
                let spent = run.tended(*days);
                if spent > 0 {
                    run.spend(*gold);
                    self.said = said.clone();
                } else {
                    self.said = refused.clone();
                }
                Answer::Stayed { days: spent }
            }
            Effect::Buy { item, said, too_dear, no_room } => {
                self.said = match run.buy(item, items) {
                    Purchase::Bought { .. } => said.clone(),
                    Purchase::TooDear => too_dear.clone(),
                    Purchase::NoRoom => no_room.clone(),
                    Purchase::Unknown => format!("No {item} here."),
                };
                Answer::Stayed { days: 0 }
            }
            Effect::Use { item, said, refused } => {
                self.said = match run.use_item(item, items) {
                    Used::Did => said.clone(),
                    _ => refused.clone(),
                };
                Answer::Stayed { days: 0 }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{ItemDef, Virtue};

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
        items
    }

    fn healer() -> PlaceDef {
        PlaceDef {
            name: "The Healer".into(),
            scene: "scene.hea".into(),
            x: 100,
            y: 100,
            w: 10,
            h: 10,
            hidden: false,
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
                        gold: 0,
                        said: "You are made whole.".into(),
                        refused: "You have no need of me.".into(),
                        too_poor: String::new(),
                    },
                },
                Choice { label: "Leave".into(), effect: Effect::Leave },
            ],
        }
    }

    /// A stall, as the pack authors one: a way in, a thing to buy, a flask to
    /// drink, and a way back to the room you came from.
    fn merchant() -> PlaceDef {
        PlaceDef {
            name: "The Merchant".into(),
            scene: "scene.highwood".into(),
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            hidden: true,
            menu: [8, 8, 100, 100],
            options: vec![
                Choice {
                    label: "Flask of healing".into(),
                    effect: Effect::Buy {
                        item: "potion".into(),
                        said: "The flask is yours.".into(),
                        too_dear: "Come back with coin.".into(),
                        no_room: "You cannot carry another.".into(),
                    },
                },
                Choice {
                    label: "Drink a flask".into(),
                    effect: Effect::Use {
                        item: "potion".into(),
                        said: "You drain it.".into(),
                        refused: "You have none, or no need.".into(),
                    },
                },
                Choice {
                    label: "Back".into(),
                    effect: Effect::Go { place: "highwood".into() },
                },
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
        v.choose(&def, &shop(), &mut run);
        assert!(!v.said.is_empty(), "a shut option should say why");
        v.move_by(&def, 1);
        assert!(v.said.is_empty(), "the refusal must not sit under the next option");
    }

    /// A box, not a circle. `MOON:CheckGROOC` overlaps two rectangles, so a
    /// corner counts: standing at the bottom-right of a place's icon puts you
    /// inside it even though the middles are further apart than either half.
    #[test]
    fn a_place_is_a_box_and_its_corners_count() {
        let d = healer(); // 10 x 10 at (100, 100); the token is 8 x 10.
        assert!(d.covers(109, 109), "a single overlapping pixel is inside");
        assert!(!d.covers(110, 100), "one further and the boxes only touch");
        assert!(!d.covers(100, 110));
        assert!(d.covers(93, 91), "and the same on the other two edges");
        assert!(!d.covers(92, 91));
    }

    /// The two towns, at the coordinates `_MAP:KnightGoesToTown` walks a knight
    /// to, in boxes the size of their own `MI.C` icons. Whatever else moves,
    /// standing on the recovered spot has to put you in the town.
    #[test]
    fn the_towns_hold_the_spots_the_original_sends_a_knight_to() {
        let town = |x, y, w, h| PlaceDef {
            name: "t".into(), scene: "s".into(), x, y, w, h,
            hidden: false, menu: [0, 0, 0, 0], options: vec![],
        };
        // Highwood: icon 0x19 is 25x32, hung so that (94, 47) is in the middle.
        let highwood = town(86, 36, 25, 32);
        assert!(highwood.covers(94, 47));
        // Waterdeep: icon 0x1a is 32x28, hung around (297, 157).
        let waterdeep = town(285, 148, 32, 28);
        assert!(waterdeep.covers(297, 157));
        // And they are nowhere near each other, so a walk between them is a walk.
        assert!(!highwood.covers(297, 157));
        assert!(!waterdeep.covers(94, 47));
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
        run.finished_fight(30, true, 0);
        let mut v = Visit::open("healer");
        v.move_by(&def, 1);
        let day = run.day;
        assert_eq!(v.choose(&def, &shop(), &mut run), Answer::Stayed { days: 3 });
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
        assert_eq!(v.choose(&def, &shop(), &mut run), Answer::Stayed { days: 0 });
        assert_eq!(run.day, day, "unwounded, so no time passes");
        assert_eq!(v.said, "You have no need of me.");
    }

    /// A hermit in the woods takes only days. A healer inside a town wants coin
    /// as well, and asks for it at the door rather than after the week.
    #[test]
    fn a_town_healer_takes_coin_as_well_as_days() {
        let mut def = healer();
        def.options[1].effect = Effect::Heal {
            days: 3,
            gold: 10,
            said: "You are made whole.".into(),
            refused: "You have no need of me.".into(),
            too_poor: "I do not work for nothing.".into(),
        };
        let mut run = Run::new(100);
        run.finished_fight(30, true, 0);
        let mut v = Visit::open("healer");
        v.move_by(&def, 1);

        let day = run.day;
        assert_eq!(v.choose(&def, &shop(), &mut run), Answer::Stayed { days: 0 });
        assert_eq!(v.said, "I do not work for nothing.");
        assert_eq!(run.health, 30, "and an empty purse buys nothing");
        assert_eq!(run.day, day, "nor costs a week to be turned away");

        run.earn(25);
        assert_eq!(v.choose(&def, &shop(), &mut run), Answer::Stayed { days: 3 });
        assert_eq!(run.gold, 15, "the fee changes hands");
        assert_eq!(run.health, 100);
    }

    #[test]
    fn an_option_that_is_not_built_yet_says_so_and_costs_nothing() {
        let (def, mut run) = (healer(), Run::new(100));
        let mut v = Visit::open("healer");
        assert!(!def.options[0].effect.available());
        assert_eq!(v.choose(&def, &shop(), &mut run), Answer::Stayed { days: 0 });
        assert_eq!(run, Run::new(100), "the run is untouched");
    }

    #[test]
    fn leaving_answers_that_you_left() {
        let (def, mut run) = (healer(), Run::new(100));
        let mut v = Visit::open("healer");
        v.move_by(&def, -1);
        assert_eq!(v.cursor, 2, "up from the top wraps to the bottom");
        assert_eq!(v.choose(&def, &shop(), &mut run), Answer::Left);
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

    // The merchant.

    #[test]
    fn the_merchant_sells_and_the_coin_actually_moves() {
        let (def, items) = (merchant(), shop());
        let mut run = Run::new(100);
        run.earn(60);
        let mut v = Visit::open("highwood.merchant");
        assert_eq!(v.choose(&def, &items, &mut run), Answer::Stayed { days: 0 });
        assert_eq!(run.gold, 35, "twenty five went across the counter");
        assert_eq!(run.kit.count("potion"), 1, "and a flask came back");
        assert_eq!(v.said, "The flask is yours.");
    }

    #[test]
    fn a_merchant_turns_away_an_empty_purse_and_says_which_reason() {
        let (def, items) = (merchant(), shop());
        let mut run = Run::new(100);
        let mut v = Visit::open("m");
        v.choose(&def, &items, &mut run);
        assert_eq!(v.said, "Come back with coin.");
        assert!(run.kit.is_empty(), "nothing changed hands");

        run.earn(1000);
        run.kit.capacity = 0;
        v.choose(&def, &items, &mut run);
        assert_eq!(v.said, "You cannot carry another.");
        assert_eq!(run.gold, 1000, "and a refused sale takes no coin");
    }

    /// The price is a property of the goods, so a menu can show it without the
    /// label ever repeating it and the two can never drift apart.
    #[test]
    fn a_stall_line_carries_the_price_of_what_it_sells() {
        let (def, items) = (merchant(), shop());
        assert_eq!(def.options[0].effect.cost(&items), Some(25));
        assert_eq!(def.options[2].effect.cost(&items), None, "a door is free");
    }

    /// A man with eight coins should be able to see that the flask is out of
    /// reach before he chooses it.
    #[test]
    fn what_you_cannot_afford_is_not_offered() {
        let (def, items) = (merchant(), shop());
        let mut run = Run::new(100);
        assert!(def.options[0].effect.available(), "the stall is open");
        assert!(!def.options[0].effect.offered(&items, &run), "but not to a pauper");
        run.earn(25);
        assert!(def.options[0].effect.offered(&items, &run));
        // Nor is a flask you are not carrying.
        assert!(!def.options[1].effect.offered(&items, &run));
        run.kit.take("potion", 1);
        assert!(def.options[1].effect.offered(&items, &run));
    }

    #[test]
    fn drinking_at_the_stall_mends_you_and_empties_the_flask() {
        let (def, items) = (merchant(), shop());
        let mut run = Run::new(100);
        run.kit.take("potion", 1);
        run.finished_fight(40, true, 0);
        let mut v = Visit::open("m");
        v.move_by(&def, 1);
        v.choose(&def, &items, &mut run);
        assert_eq!(run.health, 80);
        assert!(run.kit.is_empty(), "the flask is gone");
        assert_eq!(v.said, "You drain it.");
    }

    #[test]
    fn a_door_inside_a_place_leads_to_the_other_room_and_not_to_the_map() {
        let (def, items) = (merchant(), shop());
        let mut run = Run::new(100);
        let mut v = Visit::open("m");
        v.move_by(&def, -1);
        assert_eq!(
            v.choose(&def, &items, &mut run),
            Answer::Went { place: "highwood".into() }
        );
    }

    /// A stall is a room inside a town, not a landmark. Walking must never find
    /// it, however close to the map's origin its unused coordinates happen to
    /// put it.
    #[test]
    fn a_hidden_place_is_not_on_the_map() {
        let mut places = world();
        places.insert("highwood.merchant".into(), merchant());
        let mut a = Approach::default();
        assert_eq!(a.step(&places, 0, 0), None, "standing on its coordinates finds nothing");
        assert_eq!(nearest(&places, 0, 0, 40).map(|(id, _)| id), None);
    }
}
