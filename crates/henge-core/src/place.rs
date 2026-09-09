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
use crate::lair::Raid;
use crate::overworld::{TOKEN_H, TOKEN_W};
use crate::quest::Gate;
use crate::run::{Run, Used};
use crate::service::{Gift, Rite, Sale, Wager};
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
    /// Your own home village: one life point, and never past three.
    ///
    /// **Recovered.** `ForestVillage`, `MooresVillage` and `WasteVillage` are
    /// three public names on one routine at image `0x112a`, which is all four
    /// villages: `cmp byte [si+0x31], 3 / jge / add byte [si+0x31], 1`, then
    /// `ColourStatus 9` and back out to the map. Nothing is bought, nothing is
    /// sold and no day passes.
    ///
    /// Which village is whose is `MOON:CheckGROOC` at `0x732`: icon frames
    /// 0x15 to 0x18 are each gated on `[di+0x20]` being 0, 1, 2 or 3, the
    /// knight's own colour index, so a village another knight owns is not even
    /// offered. That gate is [`PlaceDef::knight`].
    Village {
        /// Said when there was a life point to give.
        said: String,
        /// Said when there was not: three is the ceiling.
        refused: String,
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
    /// The tavern's table: a stake on three dice. `_TAVERN`, and the five
    /// painted bets of `TAV.PIV`. The throw is shown in `room`, the dice
    /// screen, which is a hidden place whose one option leads back.
    Wager { stake: u32, room: String },
    /// Open the bowl: the coin-at-a-time donation panel both the healer and the
    /// mystic take their fee through.
    ///
    /// **Recovered.** `_WIZARD:InitDonation` at image `0xbb34` copies the purse
    /// into `GOLDP`, clears `DONATION`, and adds four gadgets whose `[si+0x10]`
    /// is 4, 5, 3 and 2; `DonateLoop` at `0xbbe6` reads that word itself and
    /// `AddDonation`/`SubDonation` move exactly one coin each way, refusing at
    /// an empty purse and an empty bowl. So there is no list of sums anywhere
    /// and never was; the three this project offered were its own. See
    /// [`crate::status::Donation`].
    Bowl {
        /// `false` is the healer and `HealDon`, `true` the mystic and
        /// `MysticJudge`.
        #[serde(default)]
        consult: bool,
    },
    /// A donation to the town healer, `HEA.PIV`: ten mends, fifteen buys a
    /// life point, and the pot is his whatever it bought. What the bowl commits
    /// to when `OkDonation` is taken.
    Donate { gold: u32 },
    /// A donation to the mystic, `MYS.PIV`: a point of an ability given or
    /// taken, on a roll the size of the donation shifts.
    Consult { gold: u32 },
    /// Sell something to the temple for half its price.
    Sell { item: String },
    /// Ring the bell at the wizard's tower and take what Math gives.
    Wizard,
    /// Stand in the stone circle: the moonstone of the night ends the quest,
    /// and short of that an offering buys a life point and a mending.
    Offer,
    /// Walk into a lair. The guardian is fought in the lair's own arena, and
    /// its floor is yours once it is down. `lair` indexes the run's table.
    Raid {
        lair: usize,
        arena: String,
        family: String,
        guardian: String,
        count: u32,
    },
    /// The gate of the Valley of the Gods. `MOON:Valley`: four keys or
    /// nothing, and beyond it the Guardian, which `InitKnightvsDemon` builds
    /// with 250 health, one of it, and `ColourBackDrop` 4, the marsh.
    Valley {
        arena: String,
        family: String,
        guardian: String,
        count: u32,
    },
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
    /// see that the potion is out of reach before he tries for it.
    pub fn offered(&self, items: &Items, run: &Run) -> bool {
        match self {
            Effect::Closed { .. } => false,
            Effect::Buy { item, .. } => items
                .get(item)
                .is_some_and(|d| run.gold >= d.price && run.kit.room() > 0),
            Effect::Use { item, .. } => run.kit.count(item) > 0,
            Effect::Wager { stake, .. } => run.gold >= *stake,
            Effect::Donate { gold } | Effect::Consult { gold } => run.gold >= *gold,
            // `InitDonation` opens with whatever is in the purse, and an empty
            // one opens a bowl that can only be closed again.
            Effect::Bowl { .. } => run.gold > 0,
            Effect::Sell { item } => run.kit.count(item) > 0,
            Effect::Go { .. }
            | Effect::Village { .. }
            | Effect::Wizard
            | Effect::Offer
            | Effect::Raid { .. }
            | Effect::Valley { .. }
            | Effect::Leave => true,
        }
    }

    /// What this option costs in coin, for a menu to show against its line. The
    /// number lives with the item or with the place, never in the label.
    pub fn cost(&self, items: &Items) -> Option<u32> {
        match self {
            Effect::Buy { item, .. } => items.get(item).map(|d| d.price),
            // Not the wager. The tavern's five gadgets are painted `1 gold` to
            // `5 gold`, so the stake is already the label and a price column
            // beside it would print the same number twice.
            Effect::Donate { gold } | Effect::Consult { gold } => Some(*gold),
            Effect::Bowl { .. } => None,
            // `GoldSell`: `shr ax, 1` on the price.
            Effect::Sell { item } => items.get(item).map(|d| d.price / 2),
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
    /// Where what the place says goes, if not inside the menu box.
    ///
    /// Several of the original's own screens paint a panel for words and
    /// nothing else: the healer's slate, the mystic's parchment, the plank on
    /// the dice table. A menu drawn over the picture and a paragraph dropped
    /// into that panel is what those screens were built for, so a place may
    /// name a second box and keep the picture between the two.
    #[serde(default)]
    pub text: Option<[i32; 4]>,
    /// One rectangle per option, where the picture already carries the words.
    ///
    /// **Recovered, and it is what a town front door is.** `MOON:InitHighWood`
    /// at image `0xec9` and `MOON:InitWaterDeep` at `0xf4a` clear the gadget
    /// table and add five gadgets each, all 64 wide, all with `+8` zero so they
    /// say nothing, and `+0xe` 1 to 5. `MOON:HWLOOP` at `0xe35` is then
    /// `MovePointer`, `CHECKGADGET` and a `cmp word ptr es:[si + 0xe]` ladder
    /// straight into `MERC`, `TAV`, `HEAL`, `HTEM` and `CEXIT`.
    ///
    /// ```text
    /// Highwood   x 0x100   y 0x1e 0x42 0x6a 0x8c 0xb7   h 16 16 16 26 12
    /// Waterdeep  x 0       y 0x1a 0x3f 0x65 0x86 0xb6   h 16 16 16 31 12
    /// ```
    ///
    /// `HIGHWOOD.PIV` has `Visit / Merchant / Tavern / Healer / High Temple /
    /// Exit` painted on the parchment down its right hand edge and those five
    /// boxes sit exactly on those words, so a screen with boxes draws **no
    /// panel, no title and no labels of its own**: the picture already says all
    /// of it, and a panel over it is a panel over the art.
    #[serde(default)]
    pub boxes: Option<Vec<[i32; 4]>>,
    /// The dice table: three faces drawn where `_TAVERN:RollDice` blits them.
    ///
    /// The screen behind them is `DICE.PIV`, which is a picture of three dice
    /// already on the wood; the faces go on top of it. Nothing else in the game
    /// draws a prop over a place, so this is a flag rather than a prop list.
    #[serde(default)]
    pub dice: bool,
    pub options: Vec<Choice>,
    /// Not on the map. A hidden place is a room inside another one, reached
    /// only through an [`Effect::Go`], so its coordinates mean nothing and
    /// walking can never stumble into it.
    #[serde(default)]
    pub hidden: bool,
    /// What the place says before anything is chosen: the wizard's bell, the
    /// healer's greeting. Empty for a place that waits to be asked.
    #[serde(default)]
    pub intro: String,
    /// The `MI.C` frame the map draws for this place, if the map draws one
    /// at all: the towns and the stones are painted into `MAP.CMP`, a lair
    /// is `DisplayLairs` blitting frame 0x14 wherever the table puts it.
    #[serde(default)]
    pub icon: Option<usize>,
    /// The line this place puts on the map's paper when you are standing on it.
    ///
    /// **Recovered.** `_MAP:OrderOpt` (image 0xaf5e) turns a stack entry's kind
    /// into one of these: kind 2 takes `_MAP:knlair` (`Enter Lair`), kind 1
    /// takes `_MAP:knkn` (`Battle with `) with the other knight's name after
    /// it, and everything else indexes `_MAP:StackMessages` at `DS:0xc404` by
    /// `kind - 0x15`. The kind is the `MI.C` frame `MOON:MapIconsTABLE` names,
    /// so the line belongs to the place and is baked beside it.
    ///
    /// Empty for a place the original does not have, which then shows its
    /// [`PlaceDef::name`] instead.
    #[serde(default)]
    pub line: String,
    /// Whose place this is, by the knight's own colour index, or none for a
    /// place anybody may walk into.
    ///
    /// **Recovered.** `MOON:CheckGROOC` at image `0x732` tests the icon frame
    /// it has just matched and, for the four villages, refuses the entry unless
    /// `[di+0x20]` agrees:
    ///
    /// ```text
    /// 00732  cmp ax, 0x15 / jne +6 / cmp word [di+0x20], 0 / jne (drop it)
    /// 0073d  cmp ax, 0x16 / jne +6 / cmp word [di+0x20], 1 / jne (drop it)
    /// 00748  cmp ax, 0x17 / jne +6 / cmp word [di+0x20], 2 / jne (drop it)
    /// 00753  cmp ax, 0x18 / jne +6 / cmp word [di+0x20], 3 / jne (drop it)
    /// ```
    ///
    /// So the village is not refused at the door, it never reaches the paper:
    /// the knight it does not belong to cannot see that it is there.
    #[serde(default)]
    pub knight: Option<usize>,
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

    /// The same, for one knight: `CheckGROOC`'s `[di+0x20]` gate on top of the
    /// overlap. A place that belongs to somebody else is not there at all.
    pub fn covers_for(&self, x: i32, y: i32, knight: usize) -> bool {
        self.knight.is_none_or(|whose| whose == knight) && self.covers(x, y)
    }

    /// The line the paper carries for this place: the recovered one if there is
    /// one, and the place's own name if the original has no such place.
    pub fn paper_line(&self) -> &str {
        if self.line.is_empty() {
            &self.name
        } else {
            &self.line
        }
    }
}

/// Every place in the world, keyed by id. A `BTreeMap` so that iteration order
/// is defined and two machines pick the same place out of an overlap.
pub type Places = BTreeMap<String, PlaceDef>;

/// The terrain family of every lair the pack declares, by the lair number its
/// [`Effect::Raid`] names, which is what [`Run::stock_lairs`] wants.
///
/// A pack decides how many lairs there are and where they sit, so the run's
/// table has to be built from the pack rather than from a constant here. A
/// number no place claims is a hole, and comes back as an empty family, so a
/// pack that skips one cannot shift every lair after it onto the wrong ground.
///
/// [`Run::stock_lairs`]: crate::run::Run::stock_lairs
pub fn lair_families(places: &Places) -> Vec<String> {
    let mut families: Vec<String> = Vec::new();
    for def in places.values() {
        for choice in &def.options {
            if let Effect::Raid { lair, family, .. } = &choice.effect {
                if families.len() <= *lair {
                    families.resize(*lair + 1, String::new());
                }
                families[*lair] = family.clone();
            }
        }
    }
    families
}

/// How many entries the overlap stack holds: `MOON:CheckGROOC`'s walker clears
/// five eight-byte slots at `DS:043c` before it starts
/// (`mov bx, 0x43c; mov cx, 5`, image 0x6b5), so five is what a traveller can be
/// standing on at once.
pub const OVERLAP_STACK: usize = 5;

/// Everything the traveller is standing on, as the original keeps it.
///
/// **Recovered, and it replaces an invention.** Walking onto a place used to
/// open it there and then, on an edge. The original does nothing of the kind.
/// The walker at image 0x6b5, which `_MAP:FOLLOW` calls once a frame
/// (`call 0x04dc` at 0xa2d3), clears the five slots at `DS:043c`, walks
/// `MOON:MapIconsTABLE` three words at a pass, hands each record to
/// `MOON:CheckGROOC` and pushes `[x][y][kind]` for every one that overlaps;
/// `MOON:CheckEncounterDone` then pushes the rival knights and
/// `MOON:CheckLairEncounter` the lairs. Nothing is opened. The stack is read
/// only when fire is pressed: `_MAP:ScrollINPUT` tests `JOYS` bit 0x10 at
/// 0xa3c9 and calls `_MAP:DisplayStack`, which counts the live slots and
///
/// * **none**: returns zero and the map carries on (`_MAP:NoEncounter`);
/// * **one**: falls straight into `_MAP:StackDecision` and enters it;
/// * **more**: draws `_MAP:CreatePaper` and waits for a number key.
///
/// So the stack is rebuilt from scratch every frame and holds no memory of
/// where you were, which is why standing still after leaving a town does not
/// walk you back in: nothing reads the stack until fire is pressed again.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Overlaps {
    on: Vec<String>,
}

impl Overlaps {
    /// Rebuild the stack for a map position, which is what one frame does.
    ///
    /// The original walks `MapIconsTABLE` in table order and the lairs after
    /// it. A pack is a map and not a table, so the order here is the pack's
    /// own, which a [`Places`] makes the id order. It is only ever visible when
    /// two boxes overlap at once.
    /// `knight` is the traveller's own colour index, which is what
    /// `CheckGROOC` gates the four villages on: a village another knight owns
    /// never reaches the stack.
    pub fn gather(&mut self, places: &Places, x: i32, y: i32, knight: usize) {
        self.on.clear();
        for (id, def) in places {
            if self.on.len() == OVERLAP_STACK {
                break;
            }
            if def.covers_for(x, y, knight) {
                self.on.push(id.clone());
            }
        }
    }

    pub fn ids(&self) -> &[String] {
        &self.on
    }

    pub fn len(&self) -> usize {
        self.on.len()
    }

    pub fn is_empty(&self) -> bool {
        self.on.is_empty()
    }

    /// The only entry, when there is exactly one: `DisplayStack`'s `cmp ax, 1`
    /// at 0xae40 goes straight to `StackDecision` without drawing anything.
    pub fn only(&self) -> Option<&str> {
        match self.on.as_slice() {
            [one] => Some(one.as_str()),
            _ => None,
        }
    }

    /// What a number key answers with. `DisplayStack`'s reader at 0xae58 keeps
    /// asking until the scan code is between 2 and 0x0a, which is the top row
    /// `1` to `9`, and subtracts 2 for the slot; an empty slot is refused and
    /// it waits again. So `1` is the first line and a number past the end of
    /// the list does nothing at all.
    pub fn answer(&self, number: u32) -> Option<&str> {
        if !(1..=9).contains(&number) {
            return None;
        }
        self.on.get(number as usize - 1).map(String::as_str)
    }

    /// The paper's numbered lines, as `_MAP:CreatePaper` composes them.
    ///
    /// Each pass writes `KEYNUM` and then a space into the line buffer
    /// (`mov al, [KEYNUM]; mov [bp], al; inc bp; mov byte [bp], 0x20`, 0xaf24),
    /// calls `OrderOpt` to append the words, draws the buffer and steps
    /// `KEYNUM`, which starts at 0x31, the character `1`.
    pub fn paper(&self, places: &Places) -> Vec<String> {
        self.on
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let what = places.get(id).map_or(id.as_str(), |d| d.paper_line());
                format!("{} {what}", i + 1)
            })
            .collect()
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
    /// The last throw of the dice, faces zero based, for the dice screen to
    /// draw from `DICE.CEL`. Carried through the door into that screen.
    #[serde(default)]
    pub dice: Option<[u8; 3]>,
    /// Whether the last choice was an offering the druids took.
    ///
    /// `MOON:Henge` (0x1053) does not hand a line back and leave it there: it
    /// runs the stone circle's own set piece, `0xb35e`, and only then gives the
    /// life point. `[0xf378]` holding something other than `0xffff` is what
    /// decides, and that is exactly a [`crate::service::Rite`] that was not
    /// `NothingToOffer`. See `crate::stones`.
    #[serde(default)]
    pub rite: bool,
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
    /// A guardian is waiting. The caller sets the bout up in `arena` with
    /// `count` of `guardian` and comes back to `lair` with the outcome.
    Fight {
        lair: usize,
        arena: String,
        family: String,
        guardian: String,
        count: u32,
    },
    /// Open the donation bowl over this screen. `DonateLoop`, which is a loop
    /// of its own with `MovePointer` and `CHECKGADGET` in it and two ways out.
    Bowl { consult: bool },
    /// The Valley gate stood open. The caller sets the bout up the same way
    /// and comes back through [`Run::valley_won`] or [`Run::valley_lost`].
    ///
    /// [`Run::valley_won`]: crate::run::Run::valley_won
    /// [`Run::valley_lost`]: crate::run::Run::valley_lost
    Guardian {
        arena: String,
        family: String,
        guardian: String,
        count: u32,
    },
}

impl Visit {
    pub fn open(place: &str) -> Visit {
        Visit {
            place: place.to_string(),
            cursor: 0,
            said: String::new(),
            dice: None,
            rite: false,
        }
    }

    /// Open, with what the place says on the way in.
    pub fn open_at(place: &str, def: &PlaceDef) -> Visit {
        Visit {
            said: def.intro.clone(),
            ..Visit::open(place)
        }
    }

    /// Step through a door, keeping what was just said and thrown: the dice
    /// screen shows the throw the tavern made.
    pub fn through(&self, place: &str) -> Visit {
        Visit {
            place: place.to_string(),
            cursor: 0,
            said: self.said.clone(),
            dice: self.dice,
            rite: false,
        }
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
        self.rite = false;
        match &choice.effect {
            Effect::Leave => Answer::Left,
            Effect::Go { place } => Answer::Went {
                place: place.clone(),
            },
            Effect::Closed { said } => {
                self.said = said.clone();
                Answer::Stayed { days: 0 }
            }
            // `ForestVillage` (0x112a): one life point, three is the ceiling,
            // and it costs nothing at all.
            Effect::Village { said, refused } => {
                self.said = if run.rest_at_village() {
                    said.clone()
                } else {
                    refused.clone()
                };
                Answer::Stayed { days: 0 }
            }
            Effect::Buy {
                item,
                said,
                too_dear,
                no_room,
            } => {
                self.said = match run.buy(item, items) {
                    Purchase::Bought { .. } => said.clone(),
                    Purchase::TooDear => too_dear.clone(),
                    Purchase::NoRoom => no_room.clone(),
                    Purchase::Unknown => format!("No {item} here."),
                };
                Answer::Stayed { days: 0 }
            }
            Effect::Use {
                item,
                said,
                refused,
            } => {
                self.said = match run.use_item(item, items) {
                    Used::Did => said.clone(),
                    _ => refused.clone(),
                };
                Answer::Stayed { days: 0 }
            }
            Effect::Wager { stake, room } => match run.throw_dice(*stake) {
                Wager::Threw(t) => {
                    self.dice = Some(t.dice);
                    self.said = t.describe(run.gold);
                    Answer::Went {
                        place: room.clone(),
                    }
                }
                Wager::TooPoor => {
                    self.said = "Your purse will not cover that.".into();
                    Answer::Stayed { days: 0 }
                }
                // `TavernOpenScene` turns an empty purse out of the door.
                Wager::Skint => {
                    self.said = "No coin, no game. Out.".into();
                    Answer::Left
                }
            },
            // The panel is the caller's: it is a pointer loop over four
            // gadgets, and `DonateLoop` does not return until one of the two
            // that close it is pressed.
            Effect::Bowl { consult } => Answer::Bowl { consult: *consult },
            Effect::Donate { gold } => {
                self.said = match run.donate_to_healer(*gold) {
                    Some(healing) => healing.describe().to_string(),
                    None => "Your purse will not stretch to that.".into(),
                };
                Answer::Stayed { days: 0 }
            }
            Effect::Consult { gold } => {
                self.said = run.consult_the_mystic(*gold, items).describe().to_string();
                Answer::Stayed { days: 0 }
            }
            Effect::Sell { item } => {
                self.said = match run.sell_to_temple(item, items) {
                    Sale::Sold { paid } => format!("The temple gives you {paid} gold for it."),
                    Sale::HaveNone => "You have none to sell.".into(),
                    Sale::NotWanted => "The temple has no use for that.".into(),
                    Sale::Unknown => format!("No {item} here."),
                };
                Answer::Stayed { days: 0 }
            }
            Effect::Wizard => {
                let n = run.day;
                let gift = run.visit_the_wizard(items);
                let mut said = gift.speech(n).to_string();
                let after = gift.aftermath(items);
                if !after.is_empty() {
                    said.push(' ');
                    said.push_str(&after);
                }
                if gift == Gift::Nothing {
                    said = Gift::Nothing.speech(0).to_string();
                }
                self.said = said;
                Answer::Stayed { days: 0 }
            }
            Effect::Offer => {
                let rite = run.rite_at_the_stones(None, items);
                self.said = rite.describe(items);
                // `0x10a0` compares `[0xf378]` against `0xffff` and leaves when
                // it is still that, so the set piece runs for an offering the
                // druids took and for nothing else.
                self.rite = matches!(rite, Rite::Blessed { .. });
                if matches!(rite, Rite::Won(_)) {
                    return Answer::Left;
                }
                Answer::Stayed { days: 0 }
            }
            Effect::Raid {
                lair,
                arena,
                family,
                guardian,
                count,
            } => match run.raid(*lair, items) {
                Raid::Guardian => Answer::Fight {
                    lair: *lair,
                    arena: arena.clone(),
                    family: family.clone(),
                    guardian: guardian.clone(),
                    count: *count,
                },
                Raid::Spoils(s) => {
                    self.said = s.describe(items);
                    Answer::Stayed { days: 0 }
                }
                Raid::Bare => {
                    self.said = "Nothing but bones.".into();
                    Answer::Stayed { days: 0 }
                }
            },
            Effect::Valley {
                arena,
                family,
                guardian,
                count,
            } => match run.valley() {
                Gate::Barred => {
                    self.said = Gate::Barred.describe();
                    Answer::Stayed { days: 0 }
                }
                Gate::Guardian => Answer::Guardian {
                    arena: arena.clone(),
                    family: family.clone(),
                    guardian: guardian.clone(),
                    count: *count,
                },
            },
        }
    }

    /// The guardian is down and the floor is yours: what to say about it, and
    /// the lair marked. The desktop calls this on the way back from the bout.
    pub fn won_lair(&mut self, lair: usize, items: &Items, run: &mut Run) {
        let spoils = run.lair_won(lair, items);
        self.said = format!("The guardian is slain. {}", spoils.describe(items));
    }

    /// The Valley's Guardian is down: the keys are spent and a moonstone is in
    /// the pack. What it says is `ValleyEnter`, the original's own four lines.
    pub fn won_valley(&mut self, items: &Items, run: &mut Run) {
        let stone = run.valley_won(items);
        self.said = format!(
            "{} The {} is yours.",
            crate::quest::VALLEY_ENTER.join(" "),
            stone.name()
        );
    }

    /// The Guardian won. Two life points, and the keys stay in the pack.
    pub fn lost_valley(&mut self, run: &mut Run) {
        run.valley_lost();
        self.said = "The Guardian drives you back out of the Valley.".into();
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
                name: "Potion of healing".into(),
                price: 25,
                virtue: Virtue::Restore,
                consumed: true,
            },
        );
        items
    }

    /// A village, which is what the four corners of the map hold: one line that
    /// gives a life point, and no price on it.
    fn village() -> PlaceDef {
        PlaceDef {
            name: "Village".into(),
            scene: "scene.hea".into(),
            x: 100,
            y: 100,
            w: 10,
            h: 10,
            hidden: false,
            intro: String::new(),
            icon: None,
            line: String::new(),
            knight: None,
            menu: [8, 8, 100, 100],
            boxes: None,
            text: None,
            dice: false,
            options: vec![
                Choice {
                    label: "Merchant".into(),
                    effect: Effect::Closed {
                        said: "Nothing to sell you.".into(),
                    },
                },
                Choice {
                    label: "Enter Village".into(),
                    effect: Effect::Village {
                        said: "Your own people take you in.".into(),
                        refused: "You are as whole as this place can make you.".into(),
                    },
                },
                Choice {
                    label: "Leave".into(),
                    effect: Effect::Leave,
                },
            ],
        }
    }

    /// A stall, as the pack authors one: a way in, a thing to buy, a potion to
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
            intro: String::new(),
            icon: None,
            line: String::new(),
            knight: None,
            menu: [8, 8, 100, 100],
            boxes: None,
            text: None,
            dice: false,
            options: vec![
                Choice {
                    label: "Flask of healing".into(),
                    effect: Effect::Buy {
                        item: "potion".into(),
                        said: "The potion is yours.".into(),
                        too_dear: "Come back with coin.".into(),
                        no_room: "You cannot carry another.".into(),
                    },
                },
                Choice {
                    label: "Drink a potion".into(),
                    effect: Effect::Use {
                        item: "potion".into(),
                        said: "You drain it.".into(),
                        refused: "You have none, or no need.".into(),
                    },
                },
                Choice {
                    label: "Back".into(),
                    effect: Effect::Go {
                        place: "highwood".into(),
                    },
                },
            ],
        }
    }

    fn world() -> Places {
        let mut p = Places::new();
        p.insert("village".into(), village());
        let mut far = village();
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
        let def = village();
        let mut run = Run::new(100);
        let mut v = Visit::open("village");
        assert!(matches!(
            def.options[v.cursor].effect,
            Effect::Closed { .. }
        ));
        v.choose(&def, &shop(), &mut run);
        assert!(!v.said.is_empty(), "a shut option should say why");
        v.move_by(&def, 1);
        assert!(
            v.said.is_empty(),
            "the refusal must not sit under the next option"
        );
    }

    /// A box, not a circle. `MOON:CheckGROOC` overlaps two rectangles, so a
    /// corner counts: standing at the bottom-right of a place's icon puts you
    /// inside it even though the middles are further apart than either half.
    #[test]
    fn a_place_is_a_box_and_its_corners_count() {
        let d = village(); // 10 x 10 at (100, 100); the token is 8 x 10.
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
            name: "t".into(),
            scene: "s".into(),
            x,
            y,
            w,
            h,
            hidden: false,
            intro: String::new(),
            icon: None,
            line: String::new(),
            knight: None,
            menu: [0, 0, 0, 0],
            boxes: None,
            text: None,
            dice: false,
            options: vec![],
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

    /// Walking onto a place puts it on the stack and opens nothing. The walker
    /// at image 0x6b5 only ever pushes; `_MAP:DisplayStack` is what reads, and
    /// nothing calls it until `ScrollINPUT` sees fire.
    #[test]
    fn standing_on_a_place_stacks_it_and_opens_nothing() {
        let places = world();
        let mut s = Overlaps::default();
        s.gather(&places, 60, 60, 0);
        assert!(s.is_empty(), "nowhere near");
        assert_eq!(s.only(), None);
        s.gather(&places, 102, 101, 0);
        assert_eq!(s.ids(), ["village"]);
        assert_eq!(
            s.only(),
            Some("village"),
            "one entry goes straight to StackDecision"
        );
        assert_eq!(s.answer(1), Some("village"));
    }

    /// The stack is rebuilt from nothing every frame, so it carries no memory of
    /// where you were and leaving a place cannot walk you back into it: what
    /// keeps you out is that fire has to be pressed again.
    #[test]
    fn the_stack_is_rebuilt_every_frame_and_remembers_nothing() {
        let places = world();
        let mut s = Overlaps::default();
        s.gather(&places, 100, 100, 0);
        assert_eq!(s.ids(), ["village"]);
        // Standing still is the same answer, not a second arrival.
        s.gather(&places, 100, 100, 0);
        assert_eq!(s.ids(), ["village"]);
        // Step off and the stack empties completely.
        s.gather(&places, 140, 140, 0);
        assert!(s.is_empty());
        // Straight from one place onto another, with no edge bookkeeping.
        s.gather(&places, 200, 40, 0);
        assert_eq!(s.ids(), ["highwood"]);
    }

    /// Two boxes at once is what the paper exists for: `DisplayStack` counts
    /// more than one, draws `CreatePaper` and waits on a number key. The lines
    /// are `1 `, `2 ` and so on from `KEYNUM` = `'1'`, with `OrderOpt`'s words
    /// after the space.
    #[test]
    fn two_places_at_once_make_a_numbered_paper() {
        let mut places = world();
        let mut lair = village();
        lair.name = "Lair".into();
        lair.line = "Enter Lair".into();
        lair.icon = Some(0x14);
        // Nine by five, in the corner of the village's own box, so the token
        // overlaps both at once.
        lair.x = 104;
        lair.y = 96;
        lair.w = 9;
        lair.h = 5;
        places.insert("lair.glade.1".into(), lair);
        let mut s = Overlaps::default();
        s.gather(&places, 100, 96, 0);
        assert_eq!(s.len(), 2, "both boxes are on the stack");
        assert_eq!(s.only(), None, "so nothing is entered without being asked");
        assert_eq!(s.paper(&places), ["1 Enter Lair", "2 Village"]);
        // The number keys answer, and only the live slots do.
        assert_eq!(s.answer(1), Some("lair.glade.1"));
        assert_eq!(s.answer(2), Some("village"));
        assert_eq!(
            s.answer(3),
            None,
            "an empty slot is refused and the paper waits"
        );
        assert_eq!(s.answer(0), None, "scan codes below 2 are not answers");
        assert_eq!(s.answer(10), None, "nor above 0x0a");
    }

    /// Five slots, cleared five at a time by `mov bx, 0x43c; mov cx, 5`. A
    /// sixth thing under your feet cannot be reached, here or there.
    #[test]
    fn the_stack_holds_five_and_no_more() {
        let mut places = Places::new();
        for i in 0..7 {
            let mut d = village();
            d.name = format!("place {i}");
            d.x = 100;
            d.y = 100;
            places.insert(format!("p{i}"), d);
        }
        let mut s = Overlaps::default();
        s.gather(&places, 100, 100, 0);
        assert_eq!(s.len(), OVERLAP_STACK);
        assert_eq!(s.answer(5).map(str::to_string), Some("p4".to_string()));
        assert_eq!(s.answer(6), None);
    }

    /// `OrderOpt` takes its words from `StackMessages`, not from the place's
    /// name, and a place the original does not have has no line to take.
    #[test]
    fn the_paper_line_is_the_recovered_one_when_there_is_one() {
        let mut d = village();
        assert_eq!(
            d.paper_line(),
            "Village",
            "a place with no recovered line falls back to its name"
        );
        d.line = "Enter the city of Highwood".into();
        assert_eq!(d.paper_line(), "Enter the city of Highwood");
    }

    /// `ForestVillage` (0x112a): one life point, free, and no day passes.
    #[test]
    fn your_own_village_gives_a_life_point_and_asks_nothing() {
        let def = village();
        let mut run = Run::new(100);
        run.lives = 1;
        run.finished_fight(30, true, 0);
        let mut v = Visit::open("village");
        v.move_by(&def, 1);
        let (day, gold) = (run.day, run.gold);
        assert_eq!(
            v.choose(&def, &shop(), &mut run),
            Answer::Stayed { days: 0 }
        );
        assert_eq!(run.lives, 2, "one life point");
        assert_eq!(run.health, 30, "and the routine touches nothing else");
        assert_eq!(run.day, day, "no day passes");
        assert_eq!(run.gold, gold, "and nothing is paid");
        assert_eq!(v.said, "Your own people take you in.");
    }

    /// `cmp byte [si+0x31], 3 / jge EncounterDone`: three is as high as a
    /// village goes, whatever a potion may do afterwards.
    #[test]
    fn a_village_stops_at_three_life_points() {
        let def = village();
        let mut run = Run::new(100);
        run.lives = 3;
        let mut v = Visit::open("village");
        v.move_by(&def, 1);
        assert_eq!(
            v.choose(&def, &shop(), &mut run),
            Answer::Stayed { days: 0 }
        );
        assert_eq!(run.lives, 3);
        assert_eq!(v.said, "You are as whole as this place can make you.");
    }

    /// `CheckGROOC` at 0x732: the four village frames are each gated on
    /// `[di+0x20]`, the knight's own colour index, so a village another knight
    /// owns never reaches the paper at all.
    #[test]
    fn a_village_belongs_to_one_knight_and_the_others_cannot_see_it() {
        let mut places = Places::new();
        let mut mine = village();
        mine.knight = Some(2);
        places.insert("village.c".into(), mine);
        let mut s = Overlaps::default();
        s.gather(&places, 100, 100, 2);
        assert_eq!(s.ids(), ["village.c"], "his own village is under his feet");
        for other in [0, 1, 3] {
            s.gather(&places, 100, 100, other);
            assert!(
                s.is_empty(),
                "knight {other} stands on the same ground and sees nothing"
            );
            assert!(s.paper(&places).is_empty());
        }
        // And a place that belongs to nobody is everybody's.
        places.insert("stones".into(), village());
        s.gather(&places, 100, 100, 0);
        assert_eq!(s.ids(), ["stones"]);
    }

    #[test]
    fn an_option_that_is_not_built_yet_says_so_and_costs_nothing() {
        let (def, mut run) = (village(), Run::new(100));
        let mut v = Visit::open("village");
        assert!(!def.options[0].effect.available());
        assert_eq!(
            v.choose(&def, &shop(), &mut run),
            Answer::Stayed { days: 0 }
        );
        assert_eq!(run, Run::new(100), "the run is untouched");
    }

    #[test]
    fn leaving_answers_that_you_left() {
        let (def, mut run) = (village(), Run::new(100));
        let mut v = Visit::open("village");
        v.move_by(&def, -1);
        assert_eq!(v.cursor, 2, "up from the top wraps to the bottom");
        assert_eq!(v.choose(&def, &shop(), &mut run), Answer::Left);
    }

    #[test]
    fn the_highlight_is_a_ring() {
        let (def, mut v) = (village(), Visit::open("village"));
        for expect in [1, 2, 0, 1] {
            v.move_by(&def, 1);
            assert_eq!(v.cursor, expect);
        }
        v.move_by(&def, 0);
        assert_eq!(v.cursor, 1, "standing still moves nothing");
    }

    /// Near is not on. The original has no notion of approaching a place at all:
    /// `CheckGROOC` overlaps two boxes and there is nothing between outside and
    /// inside, so a token a couple of pixels short of a box is on nothing.
    #[test]
    fn nearly_standing_on_a_place_is_standing_on_nothing() {
        let places = world();
        let mut s = Overlaps::default();
        s.gather(&places, 110, 105, 0);
        assert!(
            s.is_empty(),
            "two pixels clear of the box is clear of the box"
        );
        s.gather(&places, 109, 105, 0);
        assert_eq!(s.ids(), ["village"], "one pixel of overlap is inside");
    }

    #[test]
    fn a_place_survives_serialization() {
        let def = village();
        let json = serde_json::to_string(&def).unwrap();
        assert_eq!(serde_json::from_str::<PlaceDef>(&json).unwrap(), def);
        assert!(
            json.contains("\"do\":\"village\""),
            "effects are tagged in the data"
        );
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
        assert_eq!(run.kit.count("potion"), 1, "and a potion came back");
        assert_eq!(v.said, "The potion is yours.");
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

    /// A man with eight coins should be able to see that the potion is out of
    /// reach before he chooses it.
    #[test]
    fn what_you_cannot_afford_is_not_offered() {
        let (def, items) = (merchant(), shop());
        let mut run = Run::new(100);
        assert!(def.options[0].effect.available(), "the stall is open");
        assert!(
            !def.options[0].effect.offered(&items, &run),
            "but not to a pauper"
        );
        run.earn(25);
        assert!(def.options[0].effect.offered(&items, &run));
        // Nor is a potion you are not carrying.
        assert!(!def.options[1].effect.offered(&items, &run));
        run.kit.take("potion", 1);
        assert!(def.options[1].effect.offered(&items, &run));
    }

    #[test]
    fn drinking_at_the_stall_mends_you_and_empties_the_potion() {
        let (def, items) = (merchant(), shop());
        let mut run = Run::new(100);
        run.kit.take("potion", 1);
        run.finished_fight(40, true, 0);
        let mut v = Visit::open("m");
        v.move_by(&def, 1);
        v.choose(&def, &items, &mut run);
        // The recovered potion: `cmp` the two health words, and when they
        // differ health becomes the maximum outright (0xcad0).
        assert_eq!(run.health, 100);
        assert!(run.kit.is_empty(), "the potion is gone");
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
            Answer::Went {
                place: "highwood".into()
            }
        );
    }

    /// A stall is a room inside a town, not a landmark. Walking must never find
    /// it, however close to the map's origin its unused coordinates happen to
    /// put it.
    #[test]
    fn a_hidden_place_is_not_on_the_map() {
        let mut places = world();
        places.insert("highwood.merchant".into(), merchant());
        let mut s = Overlaps::default();
        s.gather(&places, 0, 0, 0);
        assert!(s.is_empty(), "standing on its coordinates finds nothing");
        assert!(
            s.paper(&places).is_empty(),
            "and it puts no line on the paper"
        );
    }

    /// The run's lair table is built from the pack, and each lair keeps the
    /// number its own option names whatever the ids sort like. That is what
    /// puts the forest's key in a forest lair rather than six places along.
    #[test]
    fn the_lair_table_is_read_off_the_pack_by_number() {
        let lair = |n: usize, family: &str| {
            let mut d = village();
            d.name = "Lair".into();
            d.options = vec![Choice {
                label: "Enter Lair".into(),
                effect: Effect::Raid {
                    lair: n,
                    arena: format!("a{n}"),
                    family: family.into(),
                    guardian: "troll".into(),
                    count: 1,
                },
            }];
            d
        };
        let mut places = Places::new();
        // Inserted so that the ids sort the other way round from the numbers.
        places.insert("a".into(), lair(2, "swamp"));
        places.insert("b".into(), lair(0, "forest"));
        places.insert("z".into(), lair(1, "waste"));
        assert_eq!(lair_families(&places), vec!["forest", "waste", "swamp"]);
        // A pack with no lairs asks the run to stock none.
        assert!(lair_families(&world()).is_empty());
        // And a hole is a hole, not a shift.
        places.remove("z");
        assert_eq!(lair_families(&places), vec!["forest", "", "swamp"]);
    }
    /// The Valley gate, from the outside: shut until four keys, and then it is
    /// the Guardian rather than a menu.
    #[test]
    fn the_valley_gate_is_a_menu_line_that_wants_four_keys() {
        let mut items = shop();
        for k in crate::moon::Key::ALL {
            items.insert(
                k.item().into(),
                ItemDef {
                    name: k.name().into(),
                    price: 12,
                    virtue: Virtue::Inert,
                    consumed: false,
                },
            );
        }
        let def = PlaceDef {
            name: "Valley of the Gods".into(),
            scene: "scene.bg4".into(),
            x: 0,
            y: 0,
            w: 10,
            h: 10,
            hidden: false,
            intro: String::new(),
            icon: Some(0x1c),
            line: "Enter Valley of the Gods".into(),
            knight: None,
            menu: [8, 12, 168, 40],
            boxes: None,
            text: None,
            dice: false,
            options: vec![
                Choice {
                    // `_MAP:knvalley`, verbatim.
                    label: "Enter Valley of the Gods".into(),
                    effect: Effect::Valley {
                        arena: String::new(),
                        family: "swamp".into(),
                        guardian: "demon".into(),
                        count: 1,
                    },
                },
                Choice {
                    label: "Leave".into(),
                    effect: Effect::Leave,
                },
            ],
        };
        let mut run = Run::new(100);
        run.kit.capacity = 20;
        let mut visit = Visit::open("valley");
        // The gate is always offered; what it does depends on the pack.
        assert!(def.options[0].effect.offered(&items, &run));
        assert_eq!(
            visit.choose(&def, &items, &mut run),
            Answer::Stayed { days: 0 }
        );
        assert_eq!(visit.said, crate::quest::NO_KEYS.join(" "));
        for k in crate::moon::Key::ALL {
            run.kit.take(k.item(), 1);
        }
        assert_eq!(
            visit.choose(&def, &items, &mut run),
            Answer::Guardian {
                arena: String::new(),
                family: "swamp".into(),
                guardian: "demon".into(),
                count: 1,
            },
            "four keys, and the Guardian is what is behind it"
        );
        // And back from the bout, both ways.
        let mut beaten = run.clone();
        let mut page = visit.clone();
        page.won_valley(&items, &mut beaten);
        assert_eq!(beaten.key_bits(), 0, "the keys are spent");
        assert_eq!(beaten.stones_held().len(), 1);
        assert!(page.said.starts_with("You have proven your skill"));
        let before = run.lives;
        visit.lost_valley(&mut run);
        assert_eq!(
            run.lives, before,
            "no life points to take from a run that has none"
        );
        assert_eq!(
            run.key_bits(),
            0xf,
            "and the keys stay, so the gate stays open"
        );
    }
}
