//! On-disk content types. These mirror what the baker writes and what our own
//! content will eventually be authored as, so the game reads one shape either way.

use crate::anim::Sequence;
use crate::arena::{Bounds, Prop};
use crate::taskvm::{BankTables, ScriptSet};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TerrainData {
    pub left: u16,
    pub right: u16,
    pub bottom: u16,
    pub top: u16,
    pub placements: Vec<Prop>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ArenaData {
    pub family: String,
    pub terrain: TerrainData,
}

impl ArenaData {
    pub fn bounds(&self) -> Bounds {
        Bounds {
            left: self.terrain.left as i32,
            right: self.terrain.right as i32,
            top: self.terrain.top as i32,
            bottom: self.terrain.bottom as i32,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Family {
    /// Asset id of the scenery sheet cells are cut from.
    pub sheet: String,
    /// Asset id of the full-screen backdrop.
    pub backdrop: String,
    /// The sheet a placement draws from, keyed by the placement's first byte.
    ///
    /// **Recovered.** `_LOADER` picks the tile sheet from `TileTable`, four
    /// words indexed by the landscape code, which reads `FO1.CMP` for both
    /// plain and forest, `SW1.CMP` for swamp and `WA1.CMP` for waste. The
    /// routine that does it first tests the selector against 4 and keeps
    /// `FO2.CMP` when it matches, so an arena of any family draws part of its
    /// scenery from `FO2`. Every `.T` placement in the game carries 3, 4 or
    /// 0xfe in that byte, and compositing the three readings shows only one of
    /// them makes a coherent picture: 4 from `FO2`, everything else from the
    /// family's own sheet.
    #[serde(default)]
    pub tiles: BTreeMap<u8, String>,
    /// The eight arenas this family rotates through, in file order.
    ///
    /// **Recovered.** Each family has a table of eight filename pointers
    /// (`PlainTable`, `ForestTable`, `SwampTable`, `WasteTable`) and a counter
    /// beside it. Generating an arena reads `Table[counter]`, loads it, then
    /// does `inc counter` and `and counter, 7`. The choice is a rotation, not a
    /// roll: the eight sheets of a family come round in order and repeat every
    /// eighth fight in it.
    #[serde(default)]
    pub arenas: Vec<String>,
}

impl Family {
    /// Which sheet a placement's first byte asks for.
    pub fn tile_sheet(&self, selector: u8) -> &str {
        self.tiles.get(&selector).unwrap_or(&self.sheet)
    }
}

pub type Arenas = BTreeMap<String, ArenaData>;
pub type Families = BTreeMap<String, Family>;

/// Everything the simulation needs to know about one kind of fighter. All of it
/// is data, so retuning the feel of the game is editing JSON, not editing Rust.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ActorDef {
    /// Asset id of the sprite sheet this actor's frames index into.
    pub sheet: String,
    pub health: i32,
    /// Pixels per tick. Arenas are far wider than they are deep, so horizontal
    /// movement is faster and vertical movement reads as depth.
    pub speed_x: i32,
    pub speed_y: i32,
    /// How far a strike lands, used by the opponent to judge spacing.
    pub reach: i32,
    /// How closely depth must line up before a strike can connect.
    pub depth_tolerance: i32,
    pub attack_cooldown: i32,
    /// What this kind of fighter is carrying, for whoever is left standing.
    /// Per-creature and in the data, so a troll can be worth more than a rat
    /// without a line of Rust changing.
    #[serde(default)]
    pub bounty: u32,
    /// Body box relative to the feet: [x_min, y_min, x_max, y_max], y upward.
    pub body: [i16; 4],
    /// How much ground this fighter stands on, for keeping two of them apart.
    /// Separate from `body`, which is the hit box: a body narrow enough to make
    /// strikes feel fair is much narrower than the drawn figure, so using it
    /// for spacing let four knights stand inside one another.
    /// Zero, or absent, means fall back to the body's width.
    #[serde(default)]
    pub girth: i32,
    /// Frame lists, for an actor animated by hand rather than by script.
    ///
    /// This is the simple authoring path and the one our own artwork will use
    /// first: a list of frames, each held for a number of ticks. An actor that
    /// fills [`ActorDef::animation`] instead runs the recovered task VM and
    /// ignores this entirely.
    #[serde(default)]
    pub sequences: BTreeMap<String, Sequence>,
    /// The task VM scripts this actor's states play.
    ///
    /// Empty for an actor animated from `sequences`. When it is filled, this
    /// actor is driven by [`crate::taskvm`]: it holds the closure of every
    /// script its states can reach, so an actor definition is self contained
    /// and a jump can never land outside it.
    #[serde(default)]
    pub animation: ScriptSet,
    /// Which script, or cycle of scripts, each state plays.
    ///
    /// A cycle is how the original walks: `Knight_SwWalkR1` through `R4` are
    /// four single-frame scripts, and the controller hands over the next one
    /// each time the last has ended. Which scripts a state uses is a controller
    /// decision, and the original's controller table is not recovered, so this
    /// mapping is ours and lives in the data where it can be changed.
    #[serde(default)]
    pub scripts: BTreeMap<String, Vec<String>>,
    /// The bank tables the scripts index through, keyed the way `TASKCELBUF`
    /// numbers them. A script is meaningless without one.
    #[serde(default)]
    pub banks: BankTables,
    /// Where the task's own origin sits relative to the actor's feet.
    ///
    /// The original places parts against a point near the top of the figure,
    /// and this engine positions everything by the feet. This is the offset
    /// between the two, taken from the actor's own standing frame rather than
    /// chosen: it is where the lowest pixel of that frame falls.
    #[serde(default)]
    pub origin: [i16; 2],
    /// How many ticks one script frame lasts.
    ///
    /// The original ran its task loop once per game frame, and this engine
    /// ticks sixty times a second, so the two have to be related by something.
    /// For the knight it is derived rather than felt: `Knight_SwWalkOn` bakes
    /// its own travel into its part offsets, and it covers about 47 pixels in
    /// the four frames of one stride. At a walking speed of two pixels a tick
    /// that is six ticks a frame, which is the number that makes his feet keep
    /// up with the ground he is crossing.
    #[serde(default = "one_tick")]
    pub script_ticks: u32,
}

fn one_tick() -> u32 {
    1
}

impl ActorDef {
    pub fn sequence(&self, name: &str) -> Option<&Sequence> {
        self.sequences.get(name)
    }

    /// Whether this actor is animated by the task VM rather than by frame lists.
    pub fn scripted(&self) -> bool {
        !self.animation.is_empty() && !self.scripts.is_empty()
    }

    /// The scripts a state cycles through, in order.
    pub fn scripts_for(&self, state: &str) -> &[String] {
        self.scripts.get(state).map_or(&[], Vec::as_slice)
    }

    /// The bank a part names, through the table `TASKCELBUF` last selected.
    pub fn bank(&self, table: u8, slot: u8) -> Option<&crate::taskvm::Bank> {
        self.banks.get(&table)?.get(slot as usize).filter(|b| !b.cels.is_empty())
    }
}

pub type Actors = BTreeMap<String, ActorDef>;
pub type ActorData = Actors;

/// A font bank, and which character each of its glyphs draws.
///
/// The original looks glyphs up through a table inside its executable. That
/// table is not recovered, so this mapping was read off the artwork instead and
/// lives as content, which means a replacement font needs no code change.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FontDef {
    /// Asset id of the glyph bank.
    pub sheet: String,
    /// `glyphs[i]` is the character glyph `i` draws. Glyphs past the end of this
    /// string are ornaments with no character, and are never drawn.
    pub glyphs: String,
    /// Glyph index of the blank used for a space.
    pub space: usize,
    pub space_width: i32,
    /// Pixels between characters.
    pub tracking: i32,
    pub line_height: i32,
}

pub type Fonts = BTreeMap<String, FontDef>;

/// Items live in [`crate::item`] beside the pack and the purse that use them,
/// and are re-exported here so a caller loading content finds them where it
/// finds everything else the packs declare.
pub use crate::item::{ItemDef, Items};
