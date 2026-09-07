//! On-disk content types. These mirror what the baker writes and what our own
//! content will eventually be authored as, so the game reads one shape either way.

use crate::anim::Sequence;
use crate::arena::{Bounds, Prop};
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
#[derive(Serialize, Deserialize, Clone, Debug)]
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
    pub sequences: BTreeMap<String, Sequence>,
}

impl ActorDef {
    pub fn sequence(&self, name: &str) -> Option<&Sequence> {
        self.sequences.get(name)
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
