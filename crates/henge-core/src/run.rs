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
use crate::knight::{Ability, Knight, KnightDef};
use crate::moon::Moon;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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

/// What a cast did, in more detail than [`Used`] gives: the magic items do
/// more than one thing, and a caller that cannot tell "you are flying" from
/// "you are lost" cannot draw either. [`Run::use_item`] folds every success
/// here into [`Used::Did`] for the callers that only want to know whether.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cast {
    /// Wounds mended.
    Healed,
    /// A potion opened by a man who was already whole. Not wasted: the
    /// routine at `0xcad0` turns it into a life point, up to five.
    LifePoint,
    /// The day's travel is doubled until the day turns.
    Hastened,
    /// Aloft. `returns` says whether the flight ends where it began.
    Aloft { returns: bool },
    /// The hawk dropped you somewhere else entirely.
    Astray { x: i32, y: i32 },
    /// A scroll of protection is up, and will answer the next challenge.
    Warded,
    /// Put on, and whatever it replaced is in the pack.
    Worn,
    /// You are carrying none.
    HaveNone,
    /// It would do nothing right now, so nothing was spent.
    Pointless,
    /// No such item in the packs.
    Unknown,
}

impl Cast {
    /// The short form, for a caller that only wants to know whether.
    pub fn used(self) -> Used {
        match self {
            Cast::HaveNone => Used::HaveNone,
            Cast::Pointless => Used::Pointless,
            Cast::Unknown => Used::Unknown,
            _ => Used::Did,
        }
    }
}

/// What a scroll of protection did to a challenge.
///
/// `MOON:KnightProtection`, called on the way into a knight against knight
/// fight: the challenged knight is offered his scroll, and if he casts it the
/// routine returns one, which skips the fight, unless the cast's backfire
/// flag is up, in which case it returns zero, the fight goes ahead, and the
/// caster is written into `KnightCursed`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Challenge {
    /// No ward, so the fight is on.
    Fight,
    /// The ward turned the challenger away.
    Averted,
    /// The ward backfired: the fight is on, and the caster's controls are
    /// reversed for it.
    Backfired,
}

/// Ours, not the original's: see [`Run::xp_per_level`].
fn default_xp_per_level() -> u32 {
    4
}

/// `mov byte ptr [di+0x3b], 0xff` in `SetKnightEquipment`: the grudge a knight
/// who has never rung the wizard's bell carries.
pub const NEVER_MET: u32 = 0xff;

fn never_met() -> u32 {
    NEVER_MET
}

/// No magic slot yet: `MAGIC_CNT` before the first bestowal.
pub const NO_SLOT: u8 = 0xff;

fn no_slot() -> u8 {
    NO_SLOT
}

/// `mov byte ptr [si+0x3b], 0x46` on the way out of the tower.
pub const WIZARD_GRUDGE: u32 = 0x46;

/// `sub byte ptr [si+0x3b], 0xa` in `AdjustTIME`.
pub const GRUDGE_PER_DAY: u32 = 10;

/// The life points a potion can raise you to: `cmp byte ptr [si+0x31], 6`
/// then `mov byte ptr [si+0x31], 5` in the routine at `0xcad0`.
pub const LIFE_CEILING: i32 = 5;

/// The generator behind every magic roll, `0xbd89`, is a sixteen-bit shift
/// register and the callers keep its low seven bits, so every chance in
/// `_STATUS` is a count out of 128. `MagicCast` compares the hawk's roll
/// against 15 and the protection scroll's against 10, both inclusive.
pub const ROLL_SPAN: u32 = 128;

/// What the wizard's toad curse lasts, `mov byte ptr [si+0x3a], 3`, and
/// `AdjustTIME` takes one off a day.
pub const TOAD_DAYS: u32 = 3;

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
    /// Whose run this is: the sheet the status panel reads. A default sheet
    /// belongs to nobody, which is what a run has before a knight is chosen.
    #[serde(default)]
    pub knight: Knight,
    /// Life points. `+0x31` on the knight record; five to begin with, and the
    /// thing a healer looks at first in `KnightHeal`.
    #[serde(default)]
    pub lives: i32,
    #[serde(default)]
    pub max_lives: i32,
    /// `+0x36`. What the original pays: one for a knight put down (`BKwon`),
    /// one for a lair cleared the first time (`LairWon`), two for the dragon,
    /// three for the Valley. A creature on the road pays nothing there; here
    /// it pays what its definition says, because here the road is where the
    /// fights are. Spending it is [`Run::spend_experience`].
    #[serde(default)]
    pub experience: u32,
    /// What one point of ability costs.
    ///
    /// **Half recovered.** `HGAbility` and `_MAP:KnightXP` both take exactly
    /// `[0x718]` off the experience for one point, and `MOON:Adjplayers` fills
    /// that word from `XPlevels` indexed by the player count. `XPlevels` sits
    /// at DS:`0x4d4`, inside the first 2,906 bytes of `DGROUP`, which the load
    /// image used to carry as a stale copy of another region, so its four
    /// values were not recoverable and this one is ours: four won bouts a
    /// point, which makes five of everything forty-eight wins away. That span
    /// is readable now (`docs/REVERSING.md`) and `XPlevels` has not been read
    /// out of it.
    #[serde(default = "default_xp_per_level")]
    pub xp_per_level: u32,
    /// The day's travel is doubled. `CastHaste`'s flag, `[0xcca2]`, which the
    /// map loop clears when the turn passes on.
    #[serde(default)]
    pub hasted: bool,
    /// A scroll of protection cast and waiting for the next challenge.
    ///
    /// Ours in shape: the original offers the scroll at the moment another
    /// knight attacks, inside `KnightProtection`, and there is no such prompt
    /// on a road where the ambusher is a troll. Cast ahead, it hangs until
    /// something does attack, and then does what the original's does.
    #[serde(default)]
    pub warded: bool,
    /// The roll `MagicCast` makes when the scroll is cast, kept until the
    /// challenge reads it: on 11 of 128 outcomes the ward will backfire.
    #[serde(default)]
    ward_backfires: bool,
    /// Controls reversed for the next bout: `[0x930]`, the backfire flag
    /// `MagicCast` sets and `ControlKnight` reads, cleared at the end of
    /// `Combat`.
    #[serde(default)]
    pub cursed: bool,
    /// Days left as a toad: `[si+0x3a]`, the wizard's curse. `MOON:Combat`
    /// hands a fight to the other knight when the challenged one is a toad,
    /// without a blow struck.
    #[serde(default)]
    pub toad: u32,
    /// The moon: where the eight step cycle has got to, and how many days it
    /// has sat there. Stepped from [`Run::new_day`], as `EncounterFini` does.
    #[serde(default)]
    pub moon: Moon,
    /// The wizard's opinion of you, `[si+0x3b]`. It is added to his roll, so
    /// it pushes every visit towards the toad. `SetKnightEquipment` writes
    /// 0xff, which `AdjustTIME` never decays and the wizard reads as "never
    /// met", so a first visit cannot end in a toad; leaving his tower writes
    /// 0x46, and every day takes ten off that until it is nothing.
    #[serde(default = "never_met")]
    pub grudge: u32,
    /// Bitten by a ratman: `+0x60`, set by `Ratman_KnightBit` and read once
    /// a day by `GiveBK`, which takes a life point off a bitten knight every
    /// day until the healer's donation or the stone circle clears it. Nothing
    /// sets it yet; the bite is the ratman's own behaviour, item 37.
    #[serde(default)]
    pub bitten: bool,
    /// The twenty four lairs on this run: what each holds and whether its
    /// guardian is down. Empty until [`Run::stock_lairs`] fills it.
    #[serde(default)]
    pub lairs: Vec<crate::lair::Lair>,
    /// The quest is done: the stone circle accepted a moonstone on its own
    /// night. The ending that follows is item 72's; this is the flag it reads.
    #[serde(default)]
    pub won: bool,
    /// `MAGIC_CNT`: the last magic slot the wizard's bestowal handed out, to
    /// anybody, so the next is never the same. 0xff before the first.
    #[serde(default = "no_slot")]
    pub magic_last: u8,
    /// `[0x6f06]`: the one Sword of Sharpness has been given, by the wizard or
    /// by a lair, and the bestowal rolls past that slot for good.
    #[serde(default)]
    pub sword_out: bool,
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
    /// Where each arena family's rotation has got to.
    ///
    /// **Recovered.** The original keeps one counter per family beside its
    /// table of eight arenas (`PLAINCOUNT`, `FORESTCOUNT`, `SWAMPCOUNT`,
    /// `WASTECOUNT` in the source names; four adjacent words in `_LOADER`), and
    /// generating an arena does `inc counter` then `and counter, 7`. So this is
    /// part of the run's state, not a roll, and it serializes with the rest of
    /// it. A `BTreeMap` so the order is defined on every machine.
    #[serde(default)]
    pub arena_turn: BTreeMap<String, u32>,
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
            knight: Knight::default(),
            lives: 0,
            max_lives: 0,
            experience: 0,
            xp_per_level: default_xp_per_level(),
            hasted: false,
            warded: false,
            ward_backfires: false,
            cursed: false,
            toad: 0,
            moon: Moon::new(),
            grudge: NEVER_MET,
            bitten: false,
            lairs: Vec::new(),
            won: false,
            magic_last: NO_SLOT,
            sword_out: false,
            progress: 0,
            steps_per_point: 12,
            theft_odds: 700,
            seed: 0x51ed_270b,
            arena_turn: BTreeMap::new(),
        }
    }

    /// Which arena of a family the next fight in it uses, and advance the
    /// rotation. `and counter, 7` in the original, so eight and then round
    /// again; `len` is here only so a family with fewer than eight arenas in
    /// the pack cannot index off the end of it.
    pub fn next_arena(&mut self, family: &str, len: usize) -> usize {
        if len == 0 {
            return 0;
        }
        let turn = self.arena_turn.entry(family.to_string()).or_insert(0);
        let pick = *turn as usize;
        *turn = (*turn + 1) & 7;
        pick.min(len - 1)
    }

    /// Begin a run as one of the four.
    ///
    /// Health is not chosen here. `SetKnightEquipment` writes ninety-nine into
    /// the health field and the routine at 0x28d immediately overwrites it with
    /// `10 * constitution + armour + 10`, so a knight who has taken nothing off
    /// anybody rides out with twenty. Doing the same means the number on the
    /// panel is the number the arithmetic produces rather than one written
    /// beside it.
    pub fn for_knight(def: &KnightDef, seat: usize, items: &Items) -> Run {
        let knight = Knight::from_def(def, seat);
        let max_health = knight.max_health(items);
        Run {
            gold: def.gold,
            lives: def.life,
            max_lives: def.life,
            knight,
            ..Run::new(max_health)
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
        // `BKwon` adds one for a knight put down, which is what stood in
        // every seat before the bestiary.
        self.finished_fight_worth(health_left, won, purse, 1)
    }

    /// As [`Run::finished_fight`], with what the fallen were worth in
    /// experience as well as coin. Both come off the actor definitions, so a
    /// dragon can be worth the original's two and a trogg whatever the pack
    /// says, without a table here.
    pub fn finished_fight_worth(&mut self, health_left: i32, won: bool, purse: u32, xp: u32) -> bool {
        if self.over {
            return false;
        }
        self.fights += 1;
        // The end of `Combat` clears the backfire flag: a curse is one bout.
        self.cursed = false;
        self.health = health_left.max(0);
        if self.health <= 0 {
            self.spend_life();
        }
        if won {
            self.victories += 1;
            if !self.over {
                self.gold = self.gold.saturating_add(purse);
                self.experience = self.experience.saturating_add(xp);
            }
        }
        !self.over
    }

    /// Put down, and back up again if there is a life left in you.
    ///
    /// **`MOON:WhoLived`, recovered.** A knight on nothing is not finished; he
    /// is a life point poorer and back on his feet with a full sheet:
    ///
    /// ```text
    /// cmp word ptr [si+0x38], 0        ; health
    /// jg  ..                           ; still up
    /// or  byte ptr [KnightDeath], 1
    /// mov ax, word ptr [si+0x3c]       ; the maximum
    /// mov word ptr [si+0x38], ax       ; whole again
    /// sub byte ptr [si+0x31], 1        ; and one life the poorer
    /// ```
    ///
    /// The run itself ends when the last one goes, which is what
    /// `_MAP:CheckEncounterDone` tests when it decides that a knight on the map
    /// is a grave rather than a rider (`cmp byte ptr [si+0x31], 0; jg`), and
    /// what the routine at image 0x617 answers with `GameOverMes` and
    /// `jmp StartAgain`.
    ///
    /// Returns whether the run is over. A run whose knight was never given
    /// life points has none to spend, so for one of those this is still
    /// death first time, which is what every test written before the quest
    /// assumes.
    pub fn spend_life(&mut self) -> bool {
        self.health = self.max_health;
        self.lives -= 1;
        if self.lives <= 0 {
            self.lives = 0;
            self.over = true;
        }
        self.over
    }

    /// Experience from somewhere that is not a bout on the road: `LairWon`'s
    /// one for a lair cleared the first time, the three for the Valley.
    pub fn earned_experience(&mut self, xp: u32) {
        if self.alive() {
            self.experience = self.experience.saturating_add(xp);
        }
    }

    /// Something is about to attack. A scroll of protection hanging over the
    /// run answers it the way `KnightProtection` does: the challenger is
    /// turned away, or the cast backfires and the fight goes ahead with the
    /// caster's controls reversed. The scroll is spent either way.
    pub fn challenged(&mut self) -> Challenge {
        if !self.warded {
            return Challenge::Fight;
        }
        self.warded = false;
        if self.ward_backfires {
            self.ward_backfires = false;
            self.cursed = true;
            Challenge::Backfired
        } else {
            Challenge::Averted
        }
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
        self.cast(id, items).used()
    }

    /// Use one of something you carry, and say what it did. `MagicCast`:
    /// `dec byte [bx+si]` on the record, then the slot's own routine.
    pub fn cast(&mut self, id: &str, items: &Items) -> Cast {
        let Some(def) = items.get(id) else {
            return Cast::Unknown;
        };
        if self.kit.count(id) == 0 {
            return Cast::HaveNone;
        }
        if !self.alive() {
            return Cast::Pointless;
        }
        let outcome = if def.virtue.worn() {
            self.wear(id, def, items)
        } else {
            self.apply(def)
        };
        if outcome == Cast::Pointless {
            return outcome;
        }
        if def.consumed {
            self.kit.lose(id, 1);
        }
        // A ring off the pack and a suit on the back both change what a
        // knight can bear.
        self.refresh(items);
        outcome
    }

    /// Do what a spent item does. [`Cast::Pointless`] when it would achieve
    /// nothing, in which case nothing is spent.
    fn apply(&mut self, def: &ItemDef) -> Cast {
        match &def.virtue {
            // `DRINKPOTIONHEAL`, the henge flask: a fixed amount, and not
            // opened on a whole man.
            Virtue::Heal { health } => {
                if self.health >= self.max_health {
                    return Cast::Pointless;
                }
                self.health = (self.health + health).min(self.max_health);
                Cast::Healed
            }
            // The routine at 0xcad0: compare the two health words; when they
            // differ, health becomes the maximum; when they are equal, a life
            // point instead, and five is the most you can hold.
            Virtue::Restore => {
                if self.health < self.max_health {
                    self.health = self.max_health;
                    return Cast::Healed;
                }
                if self.lives >= LIFE_CEILING {
                    return Cast::Pointless;
                }
                self.lives += 1;
                Cast::LifePoint
            }
            // `CastHaste`: one flag, read by `DistanceDONE`.
            Virtue::Haste => {
                if self.hasted {
                    return Cast::Pointless;
                }
                self.hasted = true;
                Cast::Hastened
            }
            // The gem and the hawk. The gem never misses; the hawk misses
            // on 16 of 128 and puts you down inside the recovered rectangle.
            Virtue::Sight { astray, returns } => {
                if *astray > 0 && self.roll(ROLL_SPAN) < *astray {
                    let (x, y) = self.lost_and_found();
                    return Cast::Astray { x, y };
                }
                Cast::Aloft { returns: *returns }
            }
            // `MagicCast` slot 0x12: the roll is made now and the flag kept
            // for `KnightProtection` to read when a challenge comes.
            Virtue::Protection { backfire } => {
                if self.warded {
                    return Cast::Pointless;
                }
                self.warded = true;
                self.ward_backfires = self.roll(ROLL_SPAN) < *backfire;
                Cast::Warded
            }
            // Taking off another knight needs another knight. There is one
            // traveller on this map, so the scroll has nobody to rob.
            Virtue::Seize => Cast::Pointless,
            // Worn things are handled by `wear`; anything inert says so.
            Virtue::Weapon { .. } | Virtue::Armour { .. } | Virtue::Ward { .. } | Virtue::Inert => {
                Cast::Pointless
            }
        }
    }

    /// Put something on. The sword in hand or the armour on the back goes
    /// into the pack if there is room for it, and is dropped if there is not;
    /// a ring is worn by being carried, so wearing one is nothing to do.
    ///
    /// Ours: the original keeps one weapon and one armour field per knight
    /// and swaps them through `TakeSword` and its like; how a bought sword
    /// gets into the hand is not a routine that survives.
    fn wear(&mut self, id: &str, def: &ItemDef, items: &Items) -> Cast {
        let slot = match &def.virtue {
            Virtue::Weapon { .. } => &mut self.knight.weapon,
            Virtue::Armour { .. } => &mut self.knight.armour,
            _ => return Cast::Pointless,
        };
        if *slot == id {
            return Cast::Pointless;
        }
        let old = std::mem::replace(slot, id.to_string());
        self.kit.lose(id, 1);
        if items.contains_key(&old) {
            self.kit.take(&old, 1);
        }
        Cast::Worn
    }

    /// Where a botched flight puts you down: `(rnd & 0xff) + 0x20` across
    /// and `(rnd & 0x7f) + 0x24` down, from the routine at `0xa98a`, which
    /// is inside the rectangle `HawkBorders` allows and clear of its edges.
    fn lost_and_found(&mut self) -> (i32, i32) {
        let x = (self.next_random() & 0xff) as i32 + 0x20;
        let y = (self.next_random() & 0x7f) as i32 + 0x24;
        (x, y)
    }

    /// A roll in `0..n`, taken from the run's own state.
    pub fn roll(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        self.next_random() % n
    }

    /// What the rings in the pack are worth: `ax = [magic+6]; mul 20` in the
    /// routine at 0x28d, so every one counts.
    pub fn ward_health(&self, items: &Items) -> i32 {
        self.kit
            .iter()
            .filter_map(|(id, n)| match items.get(id).map(|d| &d.virtue) {
                Some(Virtue::Ward { health }) => Some(health * n as i32),
                _ => None,
            })
            .sum()
    }

    /// Run the two derivation routines again.
    ///
    /// The original calls both from one loop over the four knights and again
    /// the moment an ability or the equipment changes (`HGAbility`,
    /// `WIZBestowAbility`, `MysticAbility`). Health follows the ceiling down
    /// but never up, the `cmp bx, [si+0x38]` at the end of 0x28d.
    pub fn refresh(&mut self, items: &Items) {
        if !self.knight.named() {
            return;
        }
        self.max_health = self.knight.max_health(items) + self.ward_health(items);
        if self.health > self.max_health {
            self.health = self.max_health;
        }
    }

    /// How far this run travels in a day: the stride, sixteen steps to the
    /// point, doubled while hasted. `DistanceDONE` in full.
    pub fn day_steps(&self, items: &Items) -> u32 {
        let base = self.knight.steps_per_day(items).max(1);
        if self.hasted { base * 2 } else { base }
    }

    /// The cost of a point, `[0x718]`.
    pub fn level_cost(&self) -> u32 {
        self.xp_per_level
    }

    /// Is there a point to be bought right now? The status screen at
    /// `0xd3a7` shows its three gadgets only when the experience covers the
    /// cost, and each only while its ability is under five.
    pub fn can_level(&self) -> bool {
        self.alive() && self.experience >= self.xp_per_level && !self.knight.maxed()
    }

    /// A point into an ability, free. `WIZBestowAbility` and the tail of
    /// `HGAbility`: `inc byte [bx+si]`, ten health on the spot for
    /// constitution as well as ten on the ceiling, then both derivations.
    pub fn raise_ability(&mut self, which: Ability, items: &Items) -> bool {
        if !self.alive() || !self.knight.raise(which) {
            return false;
        }
        if which == Ability::Constitution {
            self.health += 10;
        }
        self.refresh(items);
        true
    }

    /// A point off an ability: `MysticAbility`. Constitution takes ten
    /// health with it, and leaves at least one.
    pub fn lower_ability(&mut self, which: Ability, items: &Items) -> bool {
        if !self.alive() || !self.knight.lower(which) {
            return false;
        }
        if which == Ability::Constitution {
            self.health = (self.health - 10).max(1);
        }
        self.refresh(items);
        true
    }

    /// Buy a point with experience: `HGAbility` and `_MAP:KnightXP`, which
    /// refuse when the cost is more than you have, raise the ability, and
    /// then take the cost off.
    pub fn spend_experience(&mut self, which: Ability, items: &Items) -> bool {
        if self.experience < self.xp_per_level || !self.raise_ability(which, items) {
            return false;
        }
        self.experience -= self.xp_per_level;
        true
    }

    /// The same, with the ability picked by the original's own weighting
    /// rather than by the player: what `KnightXP` does for a computer knight.
    pub fn spend_experience_rolled(&mut self, items: &Items) -> Option<Ability> {
        if self.experience < self.xp_per_level {
            return None;
        }
        let roll = self.next_random();
        let which = self.knight.rolled_ability(roll)?;
        self.spend_experience(which, items).then_some(which)
    }

    /// A point handed out free, picked by the roll: `WIZBestowAbility`.
    pub fn bestow_ability(&mut self, items: &Items) -> Option<Ability> {
        let roll = self.next_random();
        let which = self.knight.rolled_ability(roll)?;
        self.raise_ability(which, items).then_some(which)
    }

    /// The wizard's other curse: three days as a toad.
    pub fn turned_to_toad(&mut self) {
        self.toad = self.toad.max(TOAD_DAYS);
    }

    pub fn is_toad(&self) -> bool {
        self.toad > 0
    }

    pub fn is_cursed(&self) -> bool {
        self.cursed
    }

    /// Set the run's own seed, for a host that wants two machines to agree
    /// and for tests. Never taken from the clock inside this crate.
    pub fn reseed(&mut self, seed: u32) {
        self.seed = seed | 1;
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

    /// A day turns over. Haste goes with it, since `DistanceDONE` reads the
    /// flag when it refills the budget and `NextWHICH` clears it; and
    /// `AdjustTIME` takes a day off a toad.
    /// A day turns over.
    ///
    /// **`MOON:EncounterFini` and `AdjustTIME`, recovered.** The moon counts
    /// the day and moves every fourth; then, for every knight on the board,
    /// the wizard's grudge comes down by ten unless it is the 0xff of a
    /// knight he has never met, a toad is a day less of a toad, and a quarter
    /// of whatever health is missing comes back, never less than a point:
    /// `bx = max - health; if bx: bx = (bx >> 2) | 1; health += bx`, clamped.
    /// That is a second way to mend besides walking, and the slower the wound
    /// the slower it closes. `GiveBK`, in the same pass, takes a life point
    /// off a knight a ratman has bitten.
    pub fn new_day(&mut self) {
        if !self.alive() {
            return;
        }
        self.day += 1;
        self.hasted = false;
        self.toad = self.toad.saturating_sub(1);
        self.moon.new_day();
        if self.grudge != NEVER_MET {
            self.grudge = self.grudge.saturating_sub(GRUDGE_PER_DAY);
        }
        let missing = self.max_health - self.health;
        if missing > 0 {
            self.health = (self.health + ((missing >> 2) | 1)).min(self.max_health);
        }
        if self.bitten && self.lives > 0 {
            self.lives -= 1;
        }
    }

    /// One number straight off the run's generator, for a caller that weights
    /// it itself the way the original's tables do.
    pub fn next_roll(&mut self) -> u32 {
        self.next_random()
    }

    /// Start again. A finished run is read, then cleared.
    ///
    /// Who you are survives. The tally, the purse and the pack do not: a new run
    /// is the same knight riding out again, not a different one, so the select
    /// screen is not made to run twice for the same decision.
    pub fn restart(&mut self) {
        let (knight, lives) = (self.knight.clone(), self.max_lives);
        *self = Run::new(self.max_health);
        self.knight = knight;
        self.lives = lives;
        self.max_lives = lives;
    }

    /// A fingerprint of the whole run, the way [`crate::bout::Bout::state_hash`]
    /// is one of a fight.
    ///
    /// Every field goes in, the private ones included: the generator's seed
    /// and the steps banked toward the next point of healing are as much of
    /// the state as the purse is, and a save that dropped them would reload
    /// into a run that mended and was robbed on different steps. This is what
    /// a save is checked against, and what proves a reloaded run is the same
    /// run rather than one that merely looks like it.
    pub fn state_hash(&self) -> u64 {
        fn mix(h: &mut u64, v: i64) {
            *h ^= v as u64;
            *h = h.wrapping_mul(0x1000_0000_01b3);
        }
        fn text(h: &mut u64, s: &str) {
            for b in s.as_bytes() {
                mix(h, *b as i64);
            }
            mix(h, -1);
        }
        let h = &mut 0xcbf2_9ce4_8422_2325u64;
        for v in [
            self.health as i64, self.max_health as i64, self.day as i64,
            self.victories as i64, self.fights as i64, self.over as i64,
            self.gold as i64, self.lives as i64, self.max_lives as i64,
            self.experience as i64, self.xp_per_level as i64, self.hasted as i64,
            self.warded as i64, self.ward_backfires as i64, self.cursed as i64,
            self.toad as i64, self.moon.day as i64, self.moon.count as i64,
            self.grudge as i64, self.bitten as i64, self.won as i64,
            self.magic_last as i64, self.sword_out as i64, self.progress as i64,
            self.steps_per_point as i64, self.theft_odds as i64, self.seed as i64,
        ] {
            mix(h, v);
        }
        text(h, &self.knight.name);
        for v in [
            self.knight.seat as i64, self.knight.strength as i64,
            self.knight.constitution as i64, self.knight.endurance as i64,
            self.knight.daggers as i64,
        ] {
            mix(h, v);
        }
        text(h, &self.knight.weapon);
        text(h, &self.knight.armour);
        mix(h, self.kit.capacity as i64);
        for (id, n) in self.kit.iter() {
            text(h, id);
            mix(h, n as i64);
        }
        for lair in &self.lairs {
            mix(h, lair.gold as i64);
            mix(h, lair.cleared as i64);
            mix(h, lair.key.map_or(0, |k| k.bit()) as i64);
            for id in &lair.magic {
                text(h, id);
            }
            mix(h, -2);
        }
        for (family, turn) in &self.arena_turn {
            text(h, family);
            mix(h, *turn as i64);
        }
        *h
    }
}

#[cfg(test)]
mod arena_rotation_tests {
    use super::Run;

    /// `Table[counter]`, `inc counter`, `and counter, 7`: eight in order, then
    /// round again. Not a roll, so no seed comes into it.
    #[test]
    fn a_family_rotates_through_its_eight_arenas_in_order() {
        let mut run = Run::new(100);
        let picks: Vec<usize> = (0..10).map(|_| run.next_arena("swamp", 8)).collect();
        assert_eq!(picks, vec![0, 1, 2, 3, 4, 5, 6, 7, 0, 1]);
    }

    #[test]
    fn each_family_keeps_its_own_place_in_the_rotation() {
        let mut run = Run::new(100);
        run.next_arena("swamp", 8);
        run.next_arena("swamp", 8);
        assert_eq!(run.next_arena("forest", 8), 0, "the forest has not been visited");
        assert_eq!(run.next_arena("swamp", 8), 2, "and the swamp is where it was");
    }

    #[test]
    fn a_short_rotation_cannot_index_off_the_end_of_the_pack() {
        let mut run = Run::new(100);
        for _ in 0..8 {
            assert!(run.next_arena("moors", 3) < 3);
        }
        assert_eq!(run.next_arena("nothing", 0), 0, "an empty family answers rather than panics");
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

    /// `MOON:WhoLived`, which is what makes losing a thing you can survive.
    /// Five life points, one a death, whole again each time, and the run ends
    /// on the fifth.
    #[test]
    fn a_life_point_is_spent_on_a_death_and_the_run_ends_on_the_last() {
        let mut items = shop();
        items.insert(
            "padded_armour".into(),
            ItemDef {
                name: "Padded armour".into(),
                price: 0,
                virtue: Virtue::Armour { health: 0, stride: 0 },
                consumed: false,
            },
        );
        let def = KnightDef {
            name: "SIR GODBER".into(),
            shades: vec![0],
            home: [10, 10],
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
        assert_eq!(r.lives, 5);
        for left in (1..5).rev() {
            assert!(r.finished_fight(0, false, 0), "still up with {left} to come");
            assert_eq!(r.lives, left);
            assert_eq!(r.health, r.max_health, "and whole again: WhoLived restores it");
            assert!(r.alive());
        }
        assert!(!r.finished_fight(0, false, 0), "and the last one ends it");
        assert_eq!(r.lives, 0);
        assert!(!r.alive());
        assert_eq!(r.fights, 5);
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

    /// A knight brings their own health, and the arithmetic decides it: a
    /// starting knight is worth twenty, not the ninety-nine the original writes
    /// into the field a moment before overwriting it.
    #[test]
    fn a_run_takes_its_health_from_the_knight_who_starts_it() {
        let mut items = shop();
        items.insert(
            "padded_armour".into(),
            ItemDef {
                name: "Padded armour".into(),
                price: 0,
                virtue: Virtue::Armour { health: 0, stride: 0 },
                consumed: false,
            },
        );
        let def = KnightDef {
            name: "SIR GODBER".into(),
            shades: vec![0x0000cc],
            home: [10, 10],
            strength: 1,
            constitution: 1,
            endurance: 1,
            life: 5,
            daggers: 10,
            gold: 10,
            weapon: "long_sword".into(),
            armour: "padded_armour".into(),
        };
        let r = Run::for_knight(&def, 0, &items);
        assert_eq!(r.max_health, 20);
        assert_eq!(r.health, 20);
        assert_eq!(r.gold, 10, "and the ten they set out with");
        assert_eq!(r.lives, 5);
        assert_eq!(r.knight.name, "SIR GODBER");
        assert_eq!(r.knight.seat, 0);
    }

    /// Who you are is not part of the slate. Wiping it would mean choosing a
    /// knight again every time a run ends.
    #[test]
    fn a_restart_keeps_the_knight_and_clears_everything_else() {
        let mut r = Run::new(40);
        r.knight.name = "SIR RICHARD".into();
        r.knight.seat = 1;
        r.max_lives = 5;
        r.lives = 2;
        r.gold = 200;
        r.experience = 7;
        r.finished_fight(0, false, 0);
        r.restart();
        assert_eq!(r.knight.name, "SIR RICHARD");
        assert_eq!(r.knight.seat, 1);
        assert_eq!(r.lives, 5, "back to full");
        assert_eq!(r.gold, 0);
        assert_eq!(r.experience, 0);
        assert_eq!(r.health, 40);
        assert!(r.alive());
    }

    /// `BKwon`: a knight put down is worth a point, and a fight you lost is
    /// worth nothing.
    #[test]
    fn winning_is_worth_experience_and_losing_is_not() {
        let mut r = Run::new(100);
        r.finished_fight(50, true, 0);
        assert_eq!(r.experience, 1);
        r.finished_fight(20, false, 0);
        assert_eq!(r.experience, 1);
        r.finished_fight(0, true, 0);
        assert_eq!(r.experience, 1, "and a corpse collects nothing, points included");
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

#[cfg(test)]
mod magic_tests {
    //! Items 40 to 45: the abilities in a run, the magic items, the curse
    //! and what experience buys.
    use super::*;
    use crate::item::Virtue;
    use crate::knight::Ability;

    /// Everything `_STATUS` stocks, at the prices its own `pu*` lines carry.
    fn magic() -> Items {
        let mut items = Items::new();
        let mut put = |id: &str, name: &str, price: u32, consumed: bool, virtue: Virtue| {
            items.insert(id.into(), ItemDef { name: name.into(), price, virtue, consumed });
        };
        put("long_sword", "Long sword", 0, false, Virtue::Weapon { damage: 0 });
        put("claymore", "Claymore sword", 25, false, Virtue::Weapon { damage: 3 });
        put("padded_armour", "Padded armour", 0, false, Virtue::Armour { health: 0, stride: 0 });
        put("chain_mail", "Chain mail", 30, false, Virtue::Armour { health: 10, stride: 2 });
        put("healing_potion", "Potion of healing", 20, true, Virtue::Restore);
        put("gem", "Gem of seeing", 32, true, Virtue::Sight { astray: 0, returns: true });
        put("ring", "Ring of protection", 50, false, Virtue::Ward { health: 20 });
        put("haste", "Scroll of Haste", 36, true, Virtue::Haste);
        put("aquisition", "Scroll of Aquisition", 52, true, Virtue::Seize);
        put("hawk", "Scroll of the Hawk", 52, true, Virtue::Sight { astray: 16, returns: false });
        put("protection", "Scroll of Protection", 24, true, Virtue::Protection { backfire: 11 });
        put("talisman", "Talisman of the Wyrm", 52, false, Virtue::Inert);
        items
    }

    fn knight_run(items: &Items) -> Run {
        let def = KnightDef {
            name: "SIR GODBER".into(),
            shades: vec![0x0000cc],
            home: [10, 10],
            strength: 1,
            constitution: 1,
            endurance: 1,
            life: 5,
            daggers: 10,
            gold: 10,
            weapon: "long_sword".into(),
            armour: "padded_armour".into(),
        };
        let mut r = Run::for_knight(&def, 0, items);
        r.kit.capacity = 1000;
        r
    }

    /// Put a knight down for good. He has the five life points
    /// `SetKnightEquipment` gives him, and `WhoLived` spends one a death, so
    /// one lost fight is not the end of him any more.
    fn kill(r: &mut Run) {
        for _ in 0..r.max_lives.max(1) {
            r.finished_fight(0, false, 0);
        }
        assert!(!r.alive(), "the knight should be out of lives");
    }

    // Item 41: the potion, as the routine at 0xcad0 has it.

    #[test]
    fn the_potion_mends_to_full_and_a_whole_man_gains_a_life_point() {
        let items = magic();
        let mut r = knight_run(&items);
        r.lives = 3;
        r.kit.take("healing_potion", 3);
        r.finished_fight(5, true, 0);
        assert_eq!(r.cast("healing_potion", &items), Cast::Healed);
        assert_eq!(r.health, r.max_health, "to the maximum, not by an amount");
        assert_eq!(r.cast("healing_potion", &items), Cast::LifePoint);
        assert_eq!(r.lives, 4);
        assert_eq!(r.kit.count("healing_potion"), 1, "both drunk");
        r.lives = LIFE_CEILING;
        assert_eq!(r.cast("healing_potion", &items), Cast::Pointless, "five and no more");
        assert_eq!(r.kit.count("healing_potion"), 1, "so it stays corked");
        assert_eq!(r.use_item("healing_potion", &items), Used::Pointless);
    }

    /// The henge flask is unchanged by the recovered potion beside it: a
    /// fixed amount, and not opened on a whole man.
    #[test]
    fn the_flask_still_mends_by_its_amount() {
        let mut items = magic();
        items.insert(
            "potion".into(),
            ItemDef { name: "Flask of healing".into(), price: 25, virtue: Virtue::Heal { health: 4 }, consumed: true },
        );
        let mut r = knight_run(&items);
        r.kit.take("potion", 2);
        r.finished_fight(10, true, 0);
        assert_eq!(r.cast("potion", &items), Cast::Healed);
        assert_eq!(r.health, 14);
    }

    // Item 40: the ring, the sword and the armour, on the run.

    /// `ax = [magic+6]; mul 20` in the routine at 0x28d: every ring counts.
    #[test]
    fn a_ring_is_worn_by_carrying_it_and_every_one_is_worth_twenty_health() {
        let items = magic();
        let mut r = knight_run(&items);
        assert_eq!(r.max_health, 20);
        r.earn(200);
        r.buy("ring", &items);
        r.refresh(&items);
        assert_eq!(r.max_health, 40);
        r.buy("ring", &items);
        r.refresh(&items);
        assert_eq!(r.max_health, 60, "two rings, forty health");
        assert_eq!(r.cast("ring", &items), Cast::Pointless, "already worn, by being carried");
        assert_eq!(r.kit.count("ring"), 2, "and none of them is spent");
        // Losing one takes its health with it, and cannot leave a man on
        // more than he can carry.
        r.health = 60;
        r.kit.lose("ring", 1);
        r.refresh(&items);
        assert_eq!((r.max_health, r.health), (40, 40), "health follows the ceiling down");
    }

    /// A bought sword goes into the hand and the old one into the pack, and
    /// `CalcDamage`'s bonus follows: strength plus the blade.
    #[test]
    fn wielding_a_sword_swaps_it_for_the_one_in_hand() {
        let items = magic();
        let mut r = knight_run(&items);
        r.earn(100);
        assert_eq!(r.buy("claymore", &items), Purchase::Bought { paid: 25 });
        assert_eq!(r.knight.damage_bonus(&items), 1, "still the long sword");
        assert_eq!(r.cast("claymore", &items), Cast::Worn);
        assert_eq!(r.knight.weapon, "claymore");
        assert_eq!(r.knight.damage_bonus(&items), 4, "strength one and the claymore's three");
        assert_eq!(r.kit.count("claymore"), 0);
        assert_eq!(r.kit.count("long_sword"), 1, "the old blade is carried, not lost");
        assert_eq!(r.cast("claymore", &items), Cast::HaveNone);
        // Armour the same way, and the ceiling moves with it.
        r.kit.take("chain_mail", 1);
        assert_eq!(r.cast("chain_mail", &items), Cast::Worn);
        assert_eq!(r.max_health, 30);
        assert_eq!(r.day_steps(&items), 128, "and mail adds two to the stride");
    }

    // Item 42: the scrolls, the gem and what they cost.

    /// `CastHaste` sets one flag; `DistanceDONE` doubles the budget while it
    /// is up; `NextWHICH` clears it when the day turns. The budget is the
    /// stride, sixteen steps to the point.
    #[test]
    fn haste_doubles_the_day_and_lasts_until_the_day_turns() {
        let items = magic();
        let mut r = knight_run(&items);
        assert_eq!(r.day_steps(&items), 96);
        r.earn(100);
        assert_eq!(r.buy("haste", &items), Purchase::Bought { paid: 36 }, "the scroll costs what pu13 says");
        r.kit.take("haste", 1);
        assert_eq!(r.cast("haste", &items), Cast::Hastened);
        assert_eq!(r.day_steps(&items), 192);
        assert_eq!(r.kit.count("haste"), 1, "one scroll spent");
        assert_eq!(r.cast("haste", &items), Cast::Pointless, "already hastened");
        assert_eq!(r.kit.count("haste"), 1, "so the second is not");
        r.new_day();
        assert!(!r.hasted);
        assert_eq!(r.day_steps(&items), 96, "a new day is a new budget");
    }

    /// The gem never misses and comes back; the hawk misses on 16 of 128 and
    /// puts you down inside the rectangle the routine at 0xa98a allows.
    #[test]
    fn the_gem_always_flies_and_the_hawk_sometimes_strands_you() {
        let items = magic();
        let mut r = knight_run(&items);
        r.kit.take("gem", 40);
        for _ in 0..40 {
            assert_eq!(r.cast("gem", &items), Cast::Aloft { returns: true });
        }
        assert_eq!(r.kit.count("gem"), 0);
        r.kit.take("hawk", 512);
        let mut astray = 0;
        for _ in 0..512 {
            match r.cast("hawk", &items) {
                Cast::Aloft { returns: false } => {}
                Cast::Astray { x, y } => {
                    astray += 1;
                    assert!((0x20..=0x11f).contains(&x), "off the map at {x}");
                    assert!((0x24..=0xa3).contains(&y), "off the map at {y}");
                }
                other => panic!("a hawk did {other:?}"),
            }
        }
        assert!((30..100).contains(&astray), "one in eight, got {astray} of 512");
    }

    /// A scroll of protection: the roll is made when it is cast and read
    /// when a challenge comes. Most casts turn the challenger away; on 11 of
    /// 128 the fight goes ahead with the caster cursed for it.
    #[test]
    fn a_ward_answers_the_next_challenge_and_sometimes_backfires() {
        let items = magic();
        let mut r = knight_run(&items);
        assert_eq!(r.challenged(), Challenge::Fight, "no ward, no answer");
        let (mut averted, mut backfired) = (0, 0);
        for _ in 0..512 {
            // The pack is bounded, so scrolls go in each time round rather
            // than five hundred of them up front. Two of them, because the
            // second cast is meant to be refused for already being warded,
            // and a refusal for an empty pack would prove nothing.
            assert_eq!(r.kit.take("protection", 2), 2, "room for two scrolls");
            assert_eq!(r.cast("protection", &items), Cast::Warded);
            assert_eq!(r.cast("protection", &items), Cast::Pointless, "one ward at a time");
            assert_eq!(r.kit.count("protection"), 1, "and a refused cast spends nothing");
            r.kit.lose("protection", 1);
            match r.challenged() {
                Challenge::Averted => averted += 1,
                Challenge::Backfired => {
                    backfired += 1;
                    assert!(r.is_cursed());
                    r.finished_fight(10, true, 0);
                    assert!(!r.is_cursed(), "a curse is one bout");
                }
                Challenge::Fight => panic!("the ward went unread"),
            }
            assert_eq!(r.challenged(), Challenge::Fight, "and the ward is spent");
        }
        assert_eq!(averted + backfired, 512);
        assert!((20..80).contains(&backfired), "eleven in 128, got {backfired} of 512");
    }

    #[test]
    fn a_scroll_of_aquisition_has_nobody_to_rob_here() {
        let items = magic();
        let mut r = knight_run(&items);
        r.kit.take("aquisition", 1);
        assert_eq!(r.cast("aquisition", &items), Cast::Pointless);
        assert_eq!(r.kit.count("aquisition"), 1, "and is kept");
        r.kit.take("talisman", 1);
        assert_eq!(r.cast("talisman", &items), Cast::Pointless);
    }

    #[test]
    fn the_dead_cast_nothing() {
        let items = magic();
        let mut r = knight_run(&items);
        r.kit.take("haste", 1);
        kill(&mut r);
        assert_eq!(r.cast("haste", &items), Cast::Pointless);
    }

    // Item 43: curses.

    #[test]
    fn a_toad_is_a_toad_for_three_days() {
        let mut r = Run::new(100);
        assert!(!r.is_toad());
        r.turned_to_toad();
        for day in 0..TOAD_DAYS {
            assert!(r.is_toad(), "day {day}");
            r.new_day();
        }
        assert!(!r.is_toad());
    }

    /// `MysticAbility`: a point off, ten health off for constitution, never
    /// below one of either.
    #[test]
    fn the_mystic_takes_a_point_and_the_health_that_went_with_it() {
        let items = magic();
        let mut r = knight_run(&items);
        assert!(!r.lower_ability(Ability::Strength, &items), "already at one");
        r.experience = 8;
        assert!(r.spend_experience(Ability::Constitution, &items));
        assert_eq!((r.max_health, r.health), (30, 30));
        assert!(r.lower_ability(Ability::Constitution, &items));
        assert_eq!((r.max_health, r.health), (20, 20));
        r.health = 5;
        r.experience = 8;
        r.spend_experience(Ability::Constitution, &items);
        assert_eq!(r.health, 15);
        r.lower_ability(Ability::Constitution, &items);
        assert_eq!(r.health, 5);
        r.health = 3;
        r.raise_ability(Ability::Constitution, &items);
        r.lower_ability(Ability::Constitution, &items);
        assert_eq!(r.health, 3, "ten off and ten on");
        r.health = 4;
        r.raise_ability(Ability::Constitution, &items);
        r.health = 4;
        r.lower_ability(Ability::Constitution, &items);
        assert_eq!(r.health, 1, "and never hollowed out");
    }

    // Item 45: experience, and what it buys.

    /// `HGAbility` and `KnightXP`: refuse when the cost is more than you
    /// have, raise the ability, take the cost off.
    #[test]
    fn a_point_of_ability_costs_experience_and_is_refused_without_it() {
        let items = magic();
        let mut r = knight_run(&items);
        r.xp_per_level = 4;
        assert!(!r.can_level());
        assert!(!r.spend_experience(Ability::Strength, &items));
        assert_eq!(r.knight.strength, 1, "and nothing changed");
        r.finished_fight(20, true, 0);
        r.finished_fight(20, true, 0);
        r.finished_fight(20, true, 0);
        r.finished_fight_worth(20, true, 0, 2);
        assert_eq!(r.experience, 5, "three bouts at one and a dragon's two");
        assert!(r.can_level());
        assert!(r.spend_experience(Ability::Strength, &items));
        assert_eq!(r.knight.strength, 2);
        assert_eq!(r.knight.damage_bonus(&items), 2, "and the swing hits harder for it");
        assert_eq!(r.experience, 1);
        assert!(!r.spend_experience(Ability::Strength, &items), "one point is not four");
        r.earned_experience(3);
        assert!(r.spend_experience(Ability::Endurance, &items));
        assert_eq!(r.day_steps(&items), 128, "endurance bought is road bought");
    }

    /// `HGAbility`: constitution is worth ten health where you stand as well
    /// as ten on the ceiling.
    #[test]
    fn constitution_bought_is_health_on_the_spot_as_well_as_a_higher_ceiling() {
        let items = magic();
        let mut r = knight_run(&items);
        r.experience = 40;
        r.health = 12;
        assert!(r.spend_experience(Ability::Constitution, &items));
        assert_eq!(r.max_health, 30);
        assert_eq!(r.health, 22, "the ten arrives in the man, not only in the number");
    }

    #[test]
    fn experience_cannot_be_spent_past_five() {
        let items = magic();
        let mut r = knight_run(&items);
        r.experience = 1000;
        let mut bought = 0;
        while r.spend_experience_rolled(&items).is_some() {
            bought += 1;
            assert!(bought <= 12, "the ceiling never arrived");
        }
        assert!(r.knight.maxed());
        assert_eq!(bought, 12, "four points each into three abilities");
        assert!(!r.can_level(), "nothing left to buy, whatever the purse says");
        assert_eq!(r.experience, 1000 - 48);
        assert_eq!(r.bestow_ability(&items), None, "and the wizard has nothing to give");
    }

    #[test]
    fn the_wizard_gives_a_point_for_nothing() {
        let items = magic();
        let mut r = knight_run(&items);
        let which = r.bestow_ability(&items).expect("a point");
        assert_eq!(r.knight.ability(which), 2);
        assert_eq!(r.experience, 0, "and it cost no experience");
    }

    #[test]
    fn a_dead_man_buys_nothing() {
        let items = magic();
        let mut r = knight_run(&items);
        r.experience = 40;
        kill(&mut r);
        assert!(!r.spend_experience(Ability::Strength, &items));
        assert_eq!(r.spend_experience_rolled(&items), None);
        assert_eq!(r.bestow_ability(&items), None);
    }

    #[test]
    fn a_run_with_magic_on_it_survives_serialization() {
        let items = magic();
        let mut r = knight_run(&items);
        r.kit.take("protection", 1);
        r.kit.take("haste", 1);
        r.cast("protection", &items);
        r.cast("haste", &items);
        r.turned_to_toad();
        r.experience = 9;
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<Run>(&json).unwrap(), r);
    }
}
