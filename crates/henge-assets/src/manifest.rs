//! What a pack declares about itself.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Where a pack's contents came from. This is the whole point of the pack system:
/// a build can refuse to ship if anything still resolves to derived material.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Provenance {
    /// Made by us. Safe to distribute.
    OriginalWork,
    /// Baked out of the 1991 game. Development only, never distributed.
    DerivedFromOriginal,
}

/// One frame within a sheet. `ox`/`oy` place the frame relative to the actor's
/// feet, which is the anchor everything in this game is positioned by.
#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct FrameRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    #[serde(default)]
    pub ox: i32,
    #[serde(default)]
    pub oy: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Sheet {
    /// Path relative to the pack root.
    pub file: String,
    pub frames: Vec<FrameRect>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Manifest {
    pub pack: String,
    pub provenance: Provenance,
    #[serde(default)]
    pub sheets: BTreeMap<String, Sheet>,
    #[serde(default)]
    pub sounds: BTreeMap<String, String>,
    #[serde(default)]
    pub music: BTreeMap<String, String>,
    /// Named 32-entry palettes as 0xRRGGBB.
    #[serde(default)]
    pub palettes: BTreeMap<String, Vec<u32>>,
    /// Arbitrary JSON blobs: arena layouts, hit lines, animation scripts.
    #[serde(default)]
    pub data: BTreeMap<String, String>,
}

impl Manifest {
    pub fn new(pack: impl Into<String>, provenance: Provenance) -> Manifest {
        Manifest {
            pack: pack.into(),
            provenance,
            sheets: BTreeMap::new(),
            sounds: BTreeMap::new(),
            music: BTreeMap::new(),
            palettes: BTreeMap::new(),
            data: BTreeMap::new(),
        }
    }

    /// Every logical id this pack provides.
    pub fn ids(&self) -> impl Iterator<Item = &String> {
        self.sheets
            .keys()
            .chain(self.sounds.keys())
            .chain(self.music.keys())
            .chain(self.palettes.keys())
            .chain(self.data.keys())
    }
}
