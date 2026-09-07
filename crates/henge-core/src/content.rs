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
    /// What waylays a traveller on this kind of ground, by actor id.
    ///
    /// **Design, not recovered.** The original decides its encounters in
    /// `_MAP`, and which creature a stretch of ground produces has not been
    /// read out of it. Troggs and ratmen in the woods, mudmen in the marsh and
    /// a troll in the waste is the obvious reading of the artwork and the
    /// manual, and it lives here so a better one is an edit to the data. An
    /// empty list means a knight, which is what stood in before the bestiary.
    #[serde(default)]
    pub creatures: Vec<String>,
}

impl Family {
    /// The creature the family's turn counter lands on, if it has any.
    pub fn creature(&self, pick: usize) -> Option<&str> {
        if self.creatures.is_empty() {
            return None;
        }
        Some(self.creatures[pick % self.creatures.len()].as_str())
    }

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
    /// What to call this kind of fighter on a plate. Empty means the id.
    #[serde(default)]
    pub name: String,
    pub health: i32,
    /// What one of this actor's blows takes off. Zero means the bout's own
    /// figure, which is how the knight is tuned; a creature carries the number
    /// its `*Dam` table in the original held.
    #[serde(default)]
    pub damage: i32,
    /// Pixels per tick. Arenas are far wider than they are deep, so horizontal
    /// movement is faster and vertical movement reads as depth.
    pub speed_x: i32,
    pub speed_y: i32,
    /// How far a strike lands, used by the opponent to judge spacing.
    pub reach: i32,
    /// How closely depth must line up before a strike can connect.
    pub depth_tolerance: i32,
    pub attack_cooldown: i32,
    /// The two ranges the original's monster tracker keeps on each actor at
    /// `+0x52` and `+0x54`: closer than `approach` it stops walking in, closer
    /// than `back_off` it gives ground. Recovered from the `Set*Tables`
    /// routines and carried here for the per-creature behaviour to use; the
    /// plain opponent judges spacing by `reach` and does not read them yet.
    #[serde(default)]
    pub approach: i32,
    #[serde(default)]
    pub back_off: i32,
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
    /// each time the last has ended. The original's tables are `BSS` and were
    /// thought lost; they are filled by `SetKnightAnims` and `SetMonsterAnims`
    /// in `MOON`, which is where the walk cycles, the attack scripts and the
    /// blow-taken scripts here were read from. Which of an actor's attacks the
    /// one button gets, and which blow it takes, is still a choice made in the
    /// baker, and it lives in the data where it can be changed.
    #[serde(default)]
    pub scripts: BTreeMap<String, Vec<String>>,
    /// The bank tables the scripts index through, keyed the way `TASKCELBUF`
    /// numbers them. A script is meaningless without one.
    #[serde(default)]
    pub banks: BankTables,
    /// Which of the bank tables a task starts on, before any `TASKCELBUF`.
    ///
    /// The original keeps it in the actor record at `+0x18`: the knight's
    /// tables routine stores the address of table 1 and every creature's
    /// stores table 2, which is why a troll's part records can name slot 0 and
    /// mean `TROLL1.CEL` while the knight's slot 0 is `KN1.OB`.
    #[serde(default = "one_table")]
    pub bank_table: u8,
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

fn one_table() -> u8 {
    1
}

impl Default for ActorDef {
    /// The empty definition, with the two fields that have a meaningful zero
    /// set to the one they default to on disk: a frame lasts a tick, and parts
    /// look up table 1.
    fn default() -> ActorDef {
        ActorDef {
            sheet: String::new(),
            name: String::new(),
            health: 0,
            damage: 0,
            speed_x: 0,
            speed_y: 0,
            reach: 0,
            depth_tolerance: 0,
            attack_cooldown: 0,
            approach: 0,
            back_off: 0,
            bounty: 0,
            body: [0; 4],
            girth: 0,
            sequences: BTreeMap::new(),
            animation: ScriptSet::new(),
            scripts: BTreeMap::new(),
            banks: BankTables::new(),
            bank_table: one_table(),
            origin: [0, 0],
            script_ticks: one_tick(),
        }
    }
}

impl ActorDef {
    pub fn sequence(&self, name: &str) -> Option<&Sequence> {
        self.sequences.get(name)
    }

    /// The name a plate prints: the one given, or the id it was filed under.
    pub fn display_name<'a>(&'a self, id: &'a str) -> &'a str {
        if self.name.is_empty() { id } else { &self.name }
    }

    /// Check that a scripted actor is whole, and say what is wrong if not.
    ///
    /// Every script a state names has to be in the set; every branch any of
    /// those scripts takes has to land in the set; and every part every script
    /// draws has to resolve through the bank tables to a cel that exists, on
    /// the table it will be looked up in. A typo in a script name or a bank
    /// table one slot short comes out here as a message, rather than as a
    /// creature that stands still or composites to a heap.
    ///
    /// Parts are checked on the table they will actually be looked up in. A
    /// script's table is whatever `TASKCELBUF` last chose, and that survives a
    /// jump, so the check walks every reachable `(script, table)` pair from the
    /// states' own scripts on [`ActorDef::bank_table`], carrying the table
    /// through each branch. That is what lets `Beast_BackToss` switch to the
    /// creature table, jump into a script that draws the knight through table
    /// 1, and be found correct, where checking each script on its own would
    /// have read the knight's cels against the beast's banks.
    pub fn validate(&self) -> Result<(), String> {
        use crate::taskvm::Instr;
        use std::collections::BTreeSet;
        if !self.scripted() {
            return if self.sequences.is_empty() {
                Err("no scripts and no frame lists".into())
            } else {
                Ok(())
            };
        }
        let mut pending: Vec<(String, u8)> = Vec::new();
        for state in ["idle", "walk", "attack", "hurt", "death"] {
            let names = self.scripts_for(state);
            if names.is_empty() {
                return Err(format!("state {state} names no script"));
            }
            for n in names {
                if !self.animation.contains_key(n) {
                    return Err(format!("state {state} names {n}, which is not in the script set"));
                }
                pending.push((n.clone(), self.bank_table));
            }
        }
        let mut seen: BTreeSet<(String, u8)> = BTreeSet::new();
        while let Some((name, start)) = pending.pop() {
            if !seen.insert((name.clone(), start)) {
                continue;
            }
            let Some(script) = self.animation.get(&name) else {
                return Err(format!("{name} is reached but is not in the script set"));
            };
            let mut table = start;
            for i in &script.code {
                let branch = match i {
                    Instr::Part(p) => {
                        let Some(bank) = self.bank(table, p.bank) else {
                            return Err(format!(
                                "{name} draws from table {table} slot {}, which holds no bank",
                                p.bank
                            ));
                        };
                        if bank.cel(p.cel).is_none() {
                            return Err(format!(
                                "{name} draws cel {} of table {table} slot {}, which has only {}",
                                p.cel,
                                p.bank,
                                bank.cels.len()
                            ));
                        }
                        continue;
                    }
                    Instr::CelBuf { table: t } => {
                        table = *t;
                        continue;
                    }
                    Instr::Goto { target, .. }
                    | Instr::Skip { target }
                    | Instr::Dead { target }
                    | Instr::AddTask { target }
                    | Instr::TestEq { target, .. }
                    | Instr::TestNe { target, .. } => target,
                    Instr::Shadow { script, .. } => script,
                    _ => continue,
                };
                if branch.is_empty() {
                    continue;
                }
                if !self.animation.contains_key(branch) {
                    return Err(format!("{name} branches to {branch}, which is not in the set"));
                }
                pending.push((branch.clone(), table));
            }
        }
        Ok(())
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

/// The health the original's knight starts a run with: ten times a
/// constitution of one, plus nothing for padded armour, plus ten. Every
/// creature's hit points and blow in the reference pack are at this scale,
/// read off the `Set*Tables` routines that sit beside the knight's, so a
/// fight at any other scale moves them by the ratio it moves the knight.
pub const ORIGINAL_KNIGHT_HEALTH: i32 = 20;

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

#[cfg(test)]
mod tests {
    use crate::combat::tests::scripted_def;
    use crate::taskvm::{End, Instr, Part, Script};

    #[test]
    fn a_whole_scripted_actor_validates() {
        assert_eq!(scripted_def().validate(), Ok(()));
    }

    /// The reason the check exists: a state naming a script that is not
    /// there, which would otherwise be a creature that quietly stands still.
    #[test]
    fn a_misspelt_script_name_is_reported_by_name() {
        let mut d = scripted_def();
        d.scripts.insert("attack".into(), vec!["Swing".into()]);
        let err = d.validate().unwrap_err();
        assert!(err.contains("attack") && err.contains("Swing"), "{err}");

        let mut d = scripted_def();
        d.scripts.remove("hurt");
        assert!(d.validate().unwrap_err().contains("hurt"));
    }

    #[test]
    fn a_part_past_the_end_of_its_bank_is_reported() {
        let mut d = scripted_def();
        d.animation.insert(
            "stance".into(),
            Script::new(vec![
                Instr::Part(Part { table: 1, bank: 0, cel: 200, x: 0, y: 0, flags: 0 }),
                Instr::EndFrame { end: End::Stop },
            ]),
        );
        let err = d.validate().unwrap_err();
        assert!(err.contains("cel 200"), "{err}");

        let mut d = scripted_def();
        d.animation.insert(
            "stance".into(),
            Script::new(vec![
                Instr::Part(Part { table: 1, bank: 3, cel: 0, x: 0, y: 0, flags: 0 }),
                Instr::EndFrame { end: End::Stop },
            ]),
        );
        assert!(d.validate().unwrap_err().contains("slot 3"));
    }

    /// A creature's scripts index the creature table, and a branch keeps the
    /// table the script switched to, so the check has to follow the switch:
    /// the same script is right on one table and wrong on the other.
    #[test]
    fn parts_are_checked_on_the_table_the_script_reaches_them_with() {
        let mut d = scripted_def();
        // Nothing on table 2, and the actor starts there.
        d.bank_table = 2;
        assert!(d.validate().unwrap_err().contains("table 2"));

        // The stance switches back to table 1 before drawing; the branch it
        // takes afterwards inherits table 1, so the walk it jumps to is fine.
        d.animation.insert(
            "stance".into(),
            Script::new(vec![
                Instr::CelBuf { table: 1 },
                Instr::Part(Part { table: 1, bank: 0, cel: 0, x: 0, y: 0, flags: 0 }),
                Instr::Goto { mode: 0, target: "walk1".into() },
                Instr::EndFrame { end: End::Stop },
            ]),
        );
        for state in ["walk", "attack", "hurt", "death"] {
            d.scripts.insert(state.into(), vec!["stance".into()]);
        }
        assert_eq!(d.validate(), Ok(()));
    }

    #[test]
    fn a_missing_branch_target_is_reported() {
        let mut d = scripted_def();
        d.animation.remove("fall");
        let err = d.validate().unwrap_err();
        assert!(err.contains("fall"), "{err}");
    }
}
