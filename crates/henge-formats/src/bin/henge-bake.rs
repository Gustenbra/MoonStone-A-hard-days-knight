//! Bakes the original 1991 game into a *reference pack*: ordinary indexed PNGs,
//! WAV files and a manifest.
//!
//! This is the seam that makes incremental replacement work. After baking, the
//! game engine only ever reads PNG, WAV and JSON. It has no idea the original
//! formats exist. Replacing a character therefore means dropping new PNGs into
//! the `original` pack, not touching a line of code.
//!
//! The pack it writes is marked `derived-from-original`, so a release build
//! refuses to start while anything still resolves to it.
//!
//!   henge-bake <game-data-dir> <packs-dir>/reference

use anyhow::Context;
use henge_assets::{palette, FrameRect, Manifest, Provenance, Sheet, RECIPE};
use henge_core::content::{ActorDef, AttackDef};
use henge_core::taskvm::{Bank, BankTables, Instr, ScriptSet};
use henge_core::wave::WaveDef;
use henge_formats::tables::{self, ActorTables};
use henge_formats::taskvm::{all_scripts, Symbols};
use henge_formats::{piv, voc, Collide, Library, Sprite};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

/// Sprite banks grouped into the actors they actually belong to.
// Hand-aligned: one actor per line, so the banks an actor owns read as a row.
#[rustfmt::skip]
const ACTORS: &[(&str, &[&str])] = &[
    ("knight", &["KN1.OB", "KN2.OB", "KN3.OB", "KN4.OB", "KN5.OB"]),
    ("hero", &["HE1.OB", "HE2.OB", "HE3.OB"]),
    ("troll", &["TROLL1.CEL", "TROLL2.CEL"]),
    ("trogg_axe", &["TROGGAX1.CEL", "TROGGAX2.CEL"]),
    ("trogg_spear", &["TROGGSP1.CEL", "TROGGSP2.CEL"]),
    ("ratmen", &["RATMEN1.CEL", "RATMEN2.CEL"]),
    ("mudmen", &["MUDMEN1.CEL", "MUDMEN2.CEL"]),
    ("demon", &["DEMON1.CEL", "DEMON2.CEL", "DEMON3.CEL", "DEMON4.CEL"]),
    ("dragon", &["DRAGON1.CEL", "DRAGON2.CEL", "DRAGON5.CEL"]),
    ("balok", &["BALOK1.CEL", "BALOK2.CEL", "BALOK3.CEL"]),
    ("gore", &["BLO.CEL"]),
];

/// Arena families, each with its scenery sheet, full-screen backdrop, and the
/// eight arenas it rotates through.
///
/// **Recovered.** `_LOADER` holds four tables of eight filename pointers,
/// `PlainTable`, `ForestTable`, `SwampTable` and `WasteTable`, and picks the
/// sheet with `Table[counter]`, `inc counter`, `and counter, 7`. The scenery
/// sheet comes from `TileTable`, four words indexed by the landscape code,
/// which reads `FO1` for both plain and forest, `SW1` for swamp and `WA1` for
/// waste. The names below are those tables, in their order.
// Hand-aligned: the eight arena names sit under the family's four fields, so the
// four families read as four records of the original's own tables.
#[rustfmt::skip]
const ARENAS: &[(&str, &str, &str, &str, [&str; 8])] = &[
    ("waste", "wa", "WA1.CMP", "WAB1.CMP",
     ["wa1", "wa2", "wa3", "wa4", "wa5", "wa6", "wa7", "wa8"]),
    ("forest", "fo", "FO1.CMP", "FOB1.CMP",
     ["fo1", "fo2", "fo3", "fo4", "fo5", "fo6", "fo7", "fo8"]),
    ("swamp", "sw", "SW1.CMP", "SWB1.CMP",
     ["sw1", "sw2", "sw3", "sw4", "sw5", "sw6", "sw7", "sw8"]),
    // **The moors.** The original's own name for this family is in the
    // `_LOADER` public list, where the five generators run
    // `GENERATELANDSCAPE`, `GENERATEMOORES`, `GENERATEFOREST`,
    // `GENERATESWAMP`, `GENERATEWASTE`: four families and a dispatcher, in
    // landscape-code order, and the first of the four is the moors. The
    // routine the dispatch table sends code 0 to loads `GLB1.CMP` and then
    // reads `PlainTable[PLAINCOUNT]`, which is `GL1.t`..`GL8.t`. So the moors
    // is not a fifth family and there is no missing `MO*` file: it is the
    // ground this project files under its own `GL` prefix. Its scenery comes
    // from FO1 like the forest's, not from FO2, which is the sheet every
    // family reaches for when a placement's selector byte is 4.
    ("glade", "gl", "FO1.CMP", "GLB1.CMP",
     ["gl1", "gl2", "gl3", "gl4", "gl5", "gl6", "gl7", "gl8"]),
];

/// The sheet a placement asks for when its selector byte is 4, whatever the
/// family. See `Family::tiles` in `henge-core` for what established it.
const SHARED_TILES: &str = "FO2.CMP";

/// The four bank tables `TASKCELBUF` chooses between, by slot.
///
/// **Recovered**, out of the loaders at the addresses `docs/TASKVM.md` names.
/// A part record's bank selector is a slot number times four, and only one of
/// these tables says what artwork that slot holds, which is why a script is
/// meaningless on its own.
///
/// Table 1 is the knight, and it is always loaded: a knight is in every fight.
/// Table 2 is whichever creature was loaded, one at a time. Tables 3 and 4 are
/// loaded once at startup and shared. A slot with no file is a hole in the
/// table and stays a hole here, so slot numbers keep lining up.
const KNIGHT_BANKS: &[&str] = &["KN1.OB", "KN2.OB", "KN3.OB", "KN4.OB", "KN5.OB"];
const TABLE3_BANKS: &[&str] = &["MI.C", "KI.CEL", "KI.CEL", "KI.CEL", "KI.CEL", "PO.CEL"];
const TABLE4_BANKS: &[&str] = &["BLO.CEL", "BLO.CEL", "BLO.CEL", "BLO.CEL", "BLO.CEL"];
/// `DrBuffer`, the table the dragon's flight over the map runs on: five slots
/// all pointing at the same bank. See `bank_tables`.
const DRAGON_FLIGHT_BANKS: &[&str] = &["MI.C", "MI.C", "MI.C", "MI.C", "MI.C"];

/// Table 2, one creature at a time, read out of the creature loaders. An empty
/// name is a slot that loader leaves alone.
// Hand-aligned: one loader per line, so the slots of table 2 line up across rows.
#[rustfmt::skip]
const CREATURE_BANKS: &[(&str, &[&str])] = &[
    ("knight", KNIGHT_BANKS),
    ("hero", &["HE1.OB", "HE2.OB", "HE3.OB", "KN4.OB", "KN5.OB"]),
    ("troll", &["TROLL1.CEL", "TROLL2.CEL"]),
    ("trogg_axe", &["TROGGAX1.CEL", "TROGGAX2.CEL"]),
    ("trogg_spear", &[
        "TROGGSP1.CEL", "TROGGSP2.CEL", "TROGGSP2.CEL", "TROGGSP2.CEL", "TROGGSP2.CEL",
    ]),
    ("ratmen", &["RATMEN1.CEL", "RATMEN2.CEL"]),
    ("mudmen", &["MUDMEN1.CEL", "MUDMEN2.CEL"]),
    ("balok", &["BALOK1.CEL", "BALOK3.CEL", "BALOK2.CEL"]),
    ("dragon", &["DRAGON1.CEL", "DRAGON2.CEL", "", "", "DRAGON5.CEL"]),
    ("beast", &["BE1.C", "BE2.C"]),
    ("demon", &["DEMON2.CEL", "", "DEMON3.CEL", "DEMON4.CEL", "DEMON1.CEL"]),
];

/// The knight's five tables are not written here.
///
/// **Recovered, and read at bake time.** `SetKnightAnims` (image 0x1771)
/// falls into `SetUpKnight` (0x1786), which fills `KnightAttSw`,
/// `KnightHitSw`, `KnightDamSw`, `KnightWalSw` and `KnightBloSw` a word at a
/// time, and `SetKnightSwTables` (0x1f6a) points the actor record at them and
/// writes the stance, the recovery and the tracker's ranges. `henge_formats::
/// tables` runs those routines and `actor_definitions` builds the knight
/// from what they wrote: every state's script, every attack by kind, every
/// blow taken by the attacker's kind, and the guard that stops each. Which of
/// the attacks the joystick picks is `Rjoystick` and `Ljoystick`, in
/// `henge_core::combat::Attack::for_direction`; which walk row a direction
/// plays is `ControlKnight` (0x3fd6 to 0x4045), in `Fighter::step_among`.
///
/// The one choice left here is the row `scripts["attack"]` names for a
/// caller with a single button: fire alone is slot 0 of `KnightAttSw` in the
/// original, the stance, and it is the swing here.
const KNIGHT_ONE_BUTTON: u8 = 0x04;

/// What a blow does to a knight who is down: `MudmenStruck1` and
/// `KnightKnightStruck1`. `Knight_SwCollapse` is the second half of
/// `Knight_SwDeath`, the fall itself; `Knight_SwDeCap` sets `DeCapFLAG`,
/// skips to the collapse when the gore is off, and ends by killing the task.
/// `explode` is the third, and it is not a blow on a corpse at all:
/// `TrollStruck1` (0x438a) falls into `TrollOHead` (0x4397) for the troll's
/// overhead chop, and if that blow left the knight with nothing it writes
/// `Knight_Explode` instead of going on to `KnightSAnim`.
const KNIGHT_FINISHES: &[(&str, &str)] = &[
    ("collapse", "Knight_SwCollapse"),
    ("decap", "Knight_SwDeCap"),
    ("explode", "Knight_Explode"),
];

/// The scripts the game's own code hands to a task it spawns off the knight:
/// `KnifeThrow` starts the dagger on `SpeedKnife` and `ControlKnife` keeps
/// it on `Knife`. Both draw from `KN4.OB`, slot 3 of the knight's table.
///
/// And the rows one fight writes over his `*Hit` table: `InitKnightvsDragon`
/// (0x244d, 0x2452, 0x2457) puts `Knight_Burn` at kinds 4 and 0x10 and
/// `Knight_SwSlapped` at kind 0xa, and `InitKnightvsBalok` (0x258c) the slap
/// at kind 4, as do `InitKnightvsTroll` (0x26ba) at kind 4 and
/// `InitKnightvsDemon` (0x2765) at kind 0x10. `Knight_Burn` draws from
/// `KN5.OB`, slot 4 of his own table, and carries `Knight_BurnDeath` under its
/// `TASKDEAD`.
///
/// And the rows three fights write over his `*Att` table, which is
/// `henge_core::bout::Bout::knight_att_rows`: `InitKnightvsRatmen` (0x232d,
/// 0x2332) is the only place in the image either `Knight_SwOThrust` or
/// `Knight_SwDThrust` is reachable from, so neither is in the closure of his
/// own tables and both have to be named here. Both draw from banks 0, 1 and 3
/// of table 1, which is the knight's own.
//
// `Beast_BackToss` is here for the same reason and one more. `InitKnightvsBeast`
// (0x2283, 0x2288) puts it on two of the knight's own `*Hit` rows, and it opens
// `TASKCELBUF 2` so the knight's task draws the tossed knight out of the
// **beast's** cels. That could not be drawn while each actor carried its own
// four tables, and the answer turned out to be the second of the two the note
// here used to weigh: **the tables are the encounter's, not the actor's**. One
// loader runs per fight and fills all four of them; `+0x18` only says which one
// a task starts on. `henge_desktop::world` now resolves every part against the
// fight's own set (`World::encounter_banks`) and falls back to the actor's only
// outside a fight, so table 2 is the loaded creature's for everybody in the
// arena, knight included, exactly as it is in the original.
const KNIGHT_SPAWNED: &[&str] = &[
    "SpeedKnife",
    "Knife",
    "Knight_Burn",
    "Knight_SwSlapped",
    "Knight_SwOThrust",
    "Knight_SwDThrust",
    "Beast_BackToss",
    // `RatmanStruck1` (0x429b, 0x42b7) hands the knight both of these out of
    // the ratman's own table, the same way the beast's toss is handed him.
    "Ratman_KnightBit",
    "Ratman_KnightSlashed",
];

/// The spray `AddBlood` starts, on bank table 4. Every part of it is gated.
const BLOOD: &str = "Blood1";

/// `TroggTABLE`, DS:0x97a, four eight-byte `[x][y][z][facing]` records and a
/// zero word. The troggs take it from the front and the troll from record one.
/// Every x is off the side of the screen and every facing points into it.
const TROGG_SEATS: &[[i32; 4]] = &[
    [-50, 0, 100, 1],
    [360, 0, 150, 3],
    [340, 0, 50, 3],
    [-80, 0, 120, 1],
];
/// `BeastTABLE`, DS:0x99c, three records.
const BEAST_SEATS: &[[i32; 4]] = &[[-60, 0, 50, 1], [360, 0, 140, 3], [370, 0, 120, 3]];
/// `RatmanTABLE`, DS:0x9b6, five records. Record three is the only one in the
/// bestiary that starts on screen, at x 30.
const RATMAN_SEATS: &[[i32; 4]] = &[
    [-50, 0, 50, 1],
    [340, 0, 100, 3],
    [360, 0, 50, 3],
    [30, 0, 90, 1],
    [-50, 0, 50, 1],
];
/// `MudmanTABLE`, DS:0x9e0, five records.
const MUDMAN_SEATS: &[[i32; 4]] = &[
    [-80, 0, 50, 1],
    [380, 0, 100, 3],
    [380, 0, 50, 3],
    [-80, 0, 90, 1],
    [-80, 0, 50, 1],
];
/// `BalokTABLE`, DS:0xa5c, one record.
const BALOK_SEATS: &[[i32; 4]] = &[[-60, 0, 0, 1]];

/// `lev_adjust`, DS:0xa0a, sixty four signed bytes: eight rows of eight, one
/// row per creature and one entry per level of the knight.
///
/// **Recovered**, and it is read straight out of the data segment.
/// `AdjustLevel` (image 0x2824) works the level out from the knight's swing and
/// his experience, finds the row by looking `INITANIM` up in `KLTAB` (DS:0xa4c,
/// eight words: `SetBalokTables`, `SetRatmenTables`, `SetTroggAxeTables`,
/// `SetTroggHammerTables`, `SetTroggSpTables`, `SetUpMudmenTables`,
/// `SetTrollTable`, `SetBeastTables`), and subtracts the entry from
/// `TotalMonsters` at 0x290f. A row falls from positive to negative across its
/// eight, so a weak knight is sent fewer of a creature and a strong one more.
/// Each row is carried on its own creature, which is what `KLTAB` says it
/// belongs to.
const LEV_BALOK: &[i32] = &[2, 2, 1, 1, 0, -1, -1, -2];
const LEV_RATMAN: &[i32] = &[5, 4, 3, 2, 0, -1, -2, -4];
const LEV_TROGG_AXE: &[i32] = &[5, 4, 2, 0, 0, -1, -3, -4];
const LEV_TROGG_HAMMER: &[i32] = &[5, 4, 2, 0, 0, -1, -3, -4];
const LEV_TROGG_SPEAR: &[i32] = &[3, 3, 2, 2, 0, -1, -2, -3];
const LEV_MUDMAN: &[i32] = &[3, 2, 1, 0, 0, -1, -1, -2];
const LEV_TROLL: &[i32] = &[3, 2, 1, 0, 0, 0, 0, -1];
const LEV_BEAST: &[i32] = &[2, 1, 0, 0, -1, -1, -2, -3];

/// One creature's wave, as its own `InitKnightvs*` writes it and `AdjustLevel`
/// then moves it: `MaxMonsters`, `TotalMonsters`, the ceiling `AdjustLevel` will
/// not take `MaxMonsters` past, whether `INITMO` flips `SIDE`, whether `INITMO`
/// puts anything in at all, whether the fight opens through `INITMO` rather than
/// through `SetMonsterCombat`, and the row of `lev_adjust`.
///
/// `wave(0, ...)` is the absence of one, which is what the knight, the demon and
/// the dragon have: their `INITMO` is the bare `ret` at image 0x2059 and their
/// `InitKnightvs*` builds the opponent inline instead of calling
/// `SetMonsterCombat`, so nothing about the fight is a count.
const fn wave(
    max: i32,
    heads: i32,
    cap: i32,
    alternates: bool,
    opens_with_side: bool,
    level: &'static [i32],
) -> WaveSpec {
    WaveSpec {
        max,
        heads,
        cap,
        alternates,
        opens_with_side,
        level,
    }
}

#[derive(Clone, Copy)]
struct WaveSpec {
    max: i32,
    heads: i32,
    cap: i32,
    alternates: bool,
    opens_with_side: bool,
    level: &'static [i32],
}

/// No wave: the knight, the demon and the dragon.
const NO_WAVE: WaveSpec = wave(0, 0, 0, false, false, &[]);

/// One creature of the bestiary: what its own code decides, beside the
/// tables its `Set*Tables` routine writes, which are read out of the image
/// rather than written here.
#[derive(Clone, Copy)]
struct Creature {
    id: &'static str,
    name: &'static str,
    /// Key into `CREATURE_BANKS`: the loader whose table 2 the scripts index.
    banks: &'static str,
    /// The sheet the first bank was packed into, for `ActorDef::sheet`.
    sheet: &'static str,
    /// Whose `Set*Tables` record this creature is built from, as
    /// `henge_formats::tables` keys them: the stance, the recovery, the walk
    /// rows, the blow-taken table, the hit points and the tracker's ranges
    /// all come from there. Empty for a creature the original writes inline
    /// and this has not read.
    tables: &'static str,
    /// The script its controller plays as its attack, for a caller with one
    /// button: what the creature's own routine hands `DS:0x783a` first.
    attack: &'static [&'static str],
    /// The kind the attack above lands as: what the creature's own routine
    /// writes into `+0x28` before it plays the script, named the way the
    /// knight's kinds are. That is what indexes the knight's `KnightHitSw`.
    kind: &'static str,
    /// The creature's other attacks, by kind and by what its `*Struck1`
    /// handler takes off the knight for that kind. Which it picks when is
    /// its controller's business.
    alternates: &'static [(&'static str, &'static str, i32)],
    /// Which of the original's controllers it runs, as
    /// `henge_core::monster::Controller` names them.
    controller: &'static str,
    /// Script rows beyond the five states: the ratman's leap, the dragon's
    /// head lifting and lowering. Named for what the controller asks for.
    rows: &'static [(&'static str, &'static [&'static str])],
    /// A border this creature brings into the arena, `[left, right, top,
    /// bottom]`. Only the demon has one, and it is `SETDEMONBORD`.
    border: Option<[i32; 4]>,
    /// Scripts the game's own code starts beside it rather than jumps to:
    /// the demon's whirl, the dragon's fire.
    spawns: &'static [&'static str],
    /// Whether its blows go through `CheckBlock`: only the troggs' do.
    blockable: bool,
    /// Whether `AddBlood` is called when it is struck.
    bleeds: bool,
    /// What its blow takes off the knight: the number its own `*Struck1`
    /// handler subtracts from `[di+0x38]`, which is not always its `*Dam`
    /// table. `TroggStruck1` (0x42e7) and `RatmanStruck1` (0x4295, 0x42b1)
    /// read the table; `TroggSpearStruck1` (0x432e), `TrollStruck1`
    /// (0x438a), `BalokStruck1` (0x4285), `BeastStruck1` (0x4430),
    /// `DemonStruck1` (0x4366, 0x4372, 0x4378), `DragonStruck1` (0x43bd,
    /// 0x43c2) and `ClawStruck1` (0x43d3) carry their own numbers, and
    /// `BalokDam`, `TrollDam` and the creatures' `*Dam` entries for those
    /// kinds are never read. `MudmenStruck1` is `KnightStruck1`, which goes
    /// through `CalcDamage` and so does read `MudmenDam`.
    damage: i32,
    /// Ours: how close the plain opponent walks before it swings.
    reach: i32,
    /// Ours: pixels a tick, read off the walk offset tables where there is one.
    speed: [i32; 2],
    /// Ours: what it is worth to whoever puts it down.
    bounty: u32,
    /// The creature's own spawn table, `[x, y, z, facing]` per seat, as
    /// `InitNewMO` (0x27ee) reads it, and which seat the first arrival takes.
    ///
    /// The tables are in the readable span of DGROUP: `TroggTABLE` DS:0x97a,
    /// `BeastTABLE` 0x99c, `RatmanTABLE` 0x9b6, `MudmanTABLE` 0x9e0,
    /// `BalokTABLE` 0xa5c, each a run of eight-byte records closed by a zero
    /// word. The troll has none of its own: `InitKnightvsTroll` (0x26f1) hands
    /// `InitTrogg` the trogg's table. The demon's and the dragon's are written
    /// into the record in code instead, at 0x278d and 0x2476.
    seats: &'static [[i32; 4]],
    /// `SetMonsterCombat` (0x27e4) starts at record 0; `InitTrogg` (0x225b) and
    /// `InitMudmen` (0x2655) `xor [SIDE], 1` against the 0 `SetUpDKL` left and
    /// so start at record 1.
    first_seat: usize,
    /// How many of it a fight holds and how many of them at once: the counts its
    /// own `InitKnightvs*` writes, the ceiling and the `lev_adjust` row
    /// `AdjustLevel` (0x2824) moves them by, and which routine its `INITMO` is.
    /// See `henge_core::wave`.
    wave: WaveSpec,
}

/// The bestiary: what each creature's own code decides.
///
/// **Recovered.** What its `Set*Tables` routine and `SetMonsterAnims` write
/// is not repeated here: `henge_formats::tables` runs those routines and
/// `creature_definition` takes the stance (`+0x10`), the recovery (`+0x12`),
/// the walk rows (`*Wal`, right at +0, up at +0x10, down at +0x20), the
/// blow-taken table (`*Hit`, by the attacker's kind: 2 lunge, 4 swing, 6
/// knife, 0xa right thrust, 0xc up thrust, 0x10 chop), the hit points
/// (`+0x38`, `+0x3c`), the kind (`+0x35`) and the tracker's ranges (`+0x52`,
/// `+0x54`, `+0x56`) from what they wrote. The blow taken is the `*Hit` entry
/// for a swing, the death is where that script's own `TASKDEAD` goes, and
/// the troggs' `*Att` tables give their attacks by kind.
///
/// What is here is what the creature's controller decides in code: the
/// attack it plays and the kind it writes, the rows its branches name, the
/// number its `*Struck1` handler takes off the knight, and the seats and the
/// wave its `InitKnightvs*` sets up. Numbers marked ours: `reach`, which is
/// the range the plain opponent swings at, and `bounty`, which the original
/// keeps no table for.
///
/// `speed` is **pixels per displayed frame, and only for an axis the
/// creature has no walk-speed table for**. Where it has one (the troggs, the
/// troll and the mudmen sideways) the table is read at bake time and this is
/// not looked at; see `ActorDef::walk_speed` and `tables::WALK_SPEED_TABLES`.
/// Where it has not, the number here is the literal its own controller
/// writes, cited on the row, or -- for the four creatures the original moves
/// from their scripts rather than by a controller step at all -- ours, marked
/// as such.
// Hand-aligned: the bestiary. Fields are grouped several to a line (identity, then
// the numbers, then the seats) so one creature is a handful of lines and the whole
// bestiary can be scanned and compared row against row. One field per line would
// make each creature forty lines and the table unreadable.
#[rustfmt::skip]
const CREATURES: &[Creature] = &[
    Creature {
        id: "troll", name: "Troll", banks: "troll", sheet: "actor.troll", tables: "troll",
        // The troll has two: `Troll_Bunt`, a club thrust that lands from 33
        // to 98 pixels out, and `Troll_Chop`, an overhead that lands from 105
        // to 160 and shakes the screen. The original picks by distance, which
        // is behaviour; the plain opponent closes in, so it gets the bunt.
        attack: &["Troll_Bunt"],
        // `TrollBunt` writes 4 and `TrollChop` 0x10. `TrollHit` is one script
        // for every kind, and `TrollStruck` calls `AddBlood`. `TrollStruck1`
        // (0x438a) takes seven off the knight for either, and never reads
        // `TrollDam`.
        kind: "swing",
        alternates: &[("chop", "Troll_Chop", 7)],
        // `ControlTroll`: the club inside a hundred, the overhead from a
        // hundred to a hundred and fifty, and never two overheads running.
        // `TrollStruck` (0x56d0) writes `Troll_Hit` for every kind and never
        // reads `TrollHit`, which `SetMonsterAnims` (0x1b22) filled with its
        // own address; `Troll_Hit`'s `TASKDEAD` is `Troll_Dies`.
        controller: "troll", rows: &[("hurt", &["Troll_Hit"])], border: None, spawns: &[],
        blockable: false,
        bleeds: true,
        damage: 7,
        // `TrollWALKR` steps 16, 26, 13, 26: twenty pixels a frame.
        // `TrollWALKR` (DS:0x7ba2) sideways, so only the depth is read here:
        // `ControlTroll` writes `mov word [0x7bf7], 0xfffb` at 0x5620 and
        // `5` at 0x5635 and jumps straight to `MonsterWalk`.
        reach: 80, speed: [0, 5], bounty: 40,
        // `InitKnightvsTroll` (0x26f1) hands `InitTrogg` the trogg's own table,
        // and `InitTrogg`'s `xor [SIDE], 1` starts it on record one.
        seats: TROGG_SEATS, first_seat: 1,
        // `InitKnightvsTroll` (0x26aa): one at a time and one owed, and
        // `AdjustLevel` (0x289e) will not take `MaxMonsters` past two for it.
        // Its fight opens through `INITMO` at 0x26f4 rather than through
        // `SetMonsterCombat`, so the first one in is chosen by `SIDE` too.
        wave: wave(1, 1, 2, true, true, LEV_TROLL),
    },
    Creature {
        id: "trogg_axe", name: "Trogg", banks: "trogg_axe", sheet: "actor.trogg_axe",
        tables: "trogg_axe",
        attack: &["TroggAxe_Swing"],
        // `TroggSwing` writes 4 and `TroggChop` 0x10; `TroggStruck1` (0x42e7)
        // reads `TroggDamAxe[kind]`, which is three for every kind, and runs
        // the knight's `CheckBlock`. The chop is not doubled: that is
        // `CalcDamage`'s, and `TroggStruck1` never calls it.
        kind: "swing",
        alternates: &[("chop", "TroggAxe_Chop", 3)],
        controller: "trogg", rows: &[], border: None, spawns: &[],
        blockable: true,
        bleeds: false,
        damage: 3,
        // `TroggWALKR` steps 0, 7, 23: ten pixels a frame.
        // Read by nobody: `TroggMove` (0x2e37, 0x2e43, 0x2e4f) has a table
        // for all three axes -- `TroggWALKR`, `TroggWALKU`, `TroggWALKD`.
        reach: 70, speed: [0, 0], bounty: 15,
        seats: TROGG_SEATS, first_seat: 0,
        // `InitKnightvsTroggAxe` (0x20b9): one at a time, three owed.
        wave: wave(1, 3, 0, true, false, LEV_TROGG_AXE),
    },
    Creature {
        id: "trogg_hammer", name: "Trogg", banks: "trogg_axe", sheet: "actor.trogg_axe",
        tables: "trogg_hammer",
        attack: &["TroggHammer_Swing"],
        // `TroggDamHammer` is two for every kind.
        kind: "swing",
        alternates: &[("chop", "TroggHammer_Chop", 2)],
        controller: "trogg", rows: &[], border: None, spawns: &[],
        blockable: true,
        bleeds: false,
        damage: 2,
        // Read by nobody: `TroggMove` (0x2e37, 0x2e43, 0x2e4f) has a table
        // for all three axes -- `TroggWALKR`, `TroggWALKU`, `TroggWALKD`.
        reach: 60, speed: [0, 0], bounty: 15,
        seats: TROGG_SEATS, first_seat: 0,
        // `InitKnightvsTroggHammer` (0x2145), the same numbers and its own row.
        wave: wave(1, 3, 0, true, false, LEV_TROGG_HAMMER),
    },
    Creature {
        id: "trogg_spear", name: "Trogg", banks: "trogg_spear", sheet: "actor.trogg_spear",
        tables: "trogg_spear",
        attack: &["TroggSpear_Lunge"],
        // `TroggAttacks` writes 2 for the spear. `TroggSpearStruck1` runs
        // `CheckBlock` and answers a block with `Knight_SwEvade`.
        kind: "lunge",
        alternates: &[],
        // `TroggAttacks` takes its kind 0x10 branch for the spear: one lunge,
        // only inside the approach range, and twenty frames before the next.
        controller: "trogg_spear", rows: &[], border: None, spawns: &[],
        blockable: true,
        bleeds: false,
        // There is no `TroggDamSp`: `TroggSpearStruck1+19` (0x432e) is
        // `sub word ptr [di+0x38], 3`.
        damage: 3,
        // Read by nobody: `TroggMove` (0x2e37, 0x2e43, 0x2e4f) has a table
        // for all three axes -- `TroggWALKR`, `TroggWALKU`, `TroggWALKD`.
        reach: 100, speed: [0, 0], bounty: 15,
        seats: TROGG_SEATS, first_seat: 0,
        // `InitKnightvsTroggSpear` (0x21e2).
        wave: wave(1, 3, 0, true, false, LEV_TROGG_SPEAR),
    },
    Creature {
        id: "ratmen", name: "Ratman", banks: "ratmen", sheet: "actor.ratmen", tables: "ratmen",
        attack: &["Ratman_Slash"],
        // `ControlRatCollide` writes 4 for the slash and 2 for the bite,
        // which is a grab and waits on 37. `RatmanStruck1` reads `RatmenDam`
        // for both (0x42b1, 0x4295): one and three on an ordinary night, and
        // `SetRatmenTables` rewrites both under the moon, which is the
        // `moon` table `creature_definition` reads off the same routine.
        kind: "swing",
        alternates: &[("lunge", "Ratman_Bite", 3)],
        controller: "ratman",
        // The whole of the ratman's repertoire, by the row `ControlRatCollide`
        // and the branches under it name. `RatmanLeaps` (0x325f) writes
        // `Ratman_Leap` outright; `RatmenWal`'s **up row**, which
        // `SetMonsterAnims+607` (0x1acd) fills with `Ratman_Leap1`..`4`, is
        // what `AnimWalk` draws while the arc is running, and is the
        // `walk_up` row the tables give.
        rows: &[
            ("leap", &["Ratman_Leap"]),
            ("fly", &["Ratman_Leap1", "Ratman_Leap2", "Ratman_Leap3", "Ratman_Leap4"]),
            // `RatmanGouged` (0x33fb).
            ("leaps", &["Ratman_Leaps"]),
            // `RatmanInTree` (0x32ce, 0x32d7), sixty apart either way.
            ("hover_far", &["Ratman_HoverR"]),
            ("hover_near", &["Ratman_HoverD"]),
            // `RatTailHit` (0x358b) and `RatHangKnight` (0x32fc, 0x3331,
            // 0x333a, 0x334a).
            ("snag", &["Ratman_SnagKnight"]),
            ("hang", &["Ratman_HangKnight"]),
            ("hung", &["Ratman_HungKnight"]),
            ("shake", &["Knight_HangSd"]),
            ("fall", &["Ratman_FallDown"]),
            // `RatLeapHit` (0x356a), `RatmanGouge` (0x338c) and
            // `RatmanOnHead+30` (0x3371).
            ("sit", &["Ratman_SitOnHead"]),
            ("gouge", &["Ratman_EyeGouge"]),
            ("whack", &["Ratman_KnightWhack"]),
            // `InitKnightvsRatmen+82` (0x236f) stands one actor on this in
            // the middle of the arena and `RatmanLeap` aims at it.
            ("tree", &["Rat_TreeBrush"]),
        ],
        border: None, spawns: &[],
        blockable: false,
        bleeds: false,
        damage: 1,
        // **Ours.** The original never gives a ratman a controller step: it
        // leaps (`RatmanLeap` 0x31e7 writes `+6` on an arc) and hangs off the
        // knight. This is the old flat two-a-tick expressed on the right
        // clock, six times a tick's worth once a frame, so nothing about the
        // ratman changes with the cadence.
        reach: 24, speed: [18, 6], bounty: 5,
        seats: RATMAN_SEATS, first_seat: 0,
        // `InitKnightvsRatmen` (0x2337): the one fight in the game that holds
        // two creatures at once, and two owed behind them.
        wave: wave(2, 2, 0, true, false, LEV_RATMAN),
    },
    Creature {
        id: "mudmen", name: "Mudman", banks: "mudmen", sheet: "actor.mudmen", tables: "mudmen",
        attack: &["Mudmen_ArmAttack"],
        // The mudman's routines never write `+0x28`, so it stays at zero and
        // `KnightHitSw[0]` is the stance: in the original its arm does not
        // stagger the knight, it entangles him, which is item 37. A swing
        // stands in so the blow is felt. `MudmenStruck1` is `KnightStruck1`
        // (0x4498), which goes through `CalcDamage` and reads `MudmenDam`:
        // two.
        kind: "swing",
        alternates: &[],
        // `ControlMudmen`: it reaches for you between seventy five and a
        // hundred and goes under the ground inside that.
        controller: "mudman",
        // `Mudmen_IBury` (DS:0x54e6) is the rear-up, and it is literally the
        // last frame of `Mudmen_Appear` (DS:0x5490): both scripts end at
        // 0x5530. `Mudmen_ToStance` (DS:0x5532) is the two frames that put the
        // arms back down, and it runs straight on into `Mudmen_Stance`.
        rows: &[
            ("bury", &["Mudmen_IBury"]),
            ("appear", &["Mudmen_Appear"]),
            ("stance", &["Mudmen_ToStance"]),
        ],
        border: None,
        // `Mudmen_KillKnight` (DS:0x594c) is the grab `MudmenBury` lands: six
        // frames of the knight being dragged under, with `KillKnight` gosubbed
        // at 0x59ac and `StopCombat` at 0x59de.
        spawns: &[
            "Mudmen_EntangleKnight",
            "Mudmen_ChokeKnight",
            "Mudmen_KnightSd",
            "Mudmen_KillKnight",
        ],
        blockable: false,
        bleeds: false,
        damage: 2,
        // `MudmenWALK` steps (12, 12), (10, 14): it comes at you on a
        // diagonal, eleven across and thirteen deep a frame.
        // `MudmenWALK` (DS:0x7b8e) sideways -- and it is x alone, since
        // `MudmenMoveR` shifts the cycle once (0x5420) where every other
        // mover shifts twice. The depth is the flat `+/-2` of `MudmenMoveU`
        // and `MudmenMoveD` (0x5447, 0x5451).
        reach: 90, speed: [0, 2], bounty: 25,
        // `InitMudmen` (0x2655) is the other `SIDE` one: record one.
        seats: MUDMAN_SEATS, first_seat: 1,
        // `InitKnightvsMudmen` (0x2620): two owed, and `AdjustLevel` (0x2898)
        // forces `MaxMonsters` back to one however strong the knight is. Its
        // fight opens through `INITMO` at 0x264c.
        wave: wave(1, 2, 1, true, true, LEV_MUDMAN),
    },
    Creature {
        id: "demon", name: "Demon", banks: "demon", sheet: "actor.demon", tables: "demon",
        // `InitKnightvsDemon` (0x2771) writes `Demon_Evolve` into `+0x10`,
        // so the demon's own stance slot is its materialisation, and that is
        // the `idle` the tables give; `Demon_Stance1` is `+0x12`, and the
        // four stances after it are what `Demon_Stance1`'s `TASKSAVE` into
        // `+0x10` cycles through. The demon has no `*Wal` table, so the
        // stances are its walk.
        attack: &["Demon_Slap"],
        // `DemonAttack` writes 0x10 for the slap, 4 for the zap, 2 for the
        // whip. `DemonStruck1` (0x4360) takes ten off for the slap, eight
        // for the whip and ten for anything else.
        kind: "chop",
        alternates: &[("swing", "Demon_Zap", 10), ("lunge", "Demon_Whip", 8)],
        controller: "demon",
        // `Demon_Stance1`'s `TASKSAVE` writes the stance cycle over `+0x10`
        // the moment the entrance has played, so the standing row is the
        // four stances and the entrance is a row of its own.
        // `DemonStruck` (0x51fc) writes `Demon_Hurt` outright; the demon has
        // no `*Hit` table.
        rows: &[
            ("evolve", &["Demon_Evolve"]),
            ("idle", &["Demon_Stance1", "Demon_Stance2", "Demon_Stance3", "Demon_Stance4"]),
            ("walk", &["Demon_Stance1", "Demon_Stance2", "Demon_Stance3", "Demon_Stance4"]),
            ("hurt", &["Demon_Hurt"]),
        ],
        // **Recovered: `SETDEMONBORD`.** The last routine in `GFX` writes one
        // record into the border list the arena's `.T` file otherwise fills,
        // and sets the deepest border row to match: 0 to 309 across, 10 to
        // 99 deep. Nothing in the shipped image calls it, so the border is
        // dead code there; it is real here, and it is what the demon does to
        // the screen.
        border: Some([0, 309, 10, 99]),
        // `AddDemonWhirl` starts the whirl on the demon's own banks; the whip
        // chain is four more scripts the controller hands over in turn.
        spawns: &[
            "Demon_Whirl", "Demon_OWhipMiss", "Demon_OWhipHit", "Demon_UWhipMiss",
            "Demon_UWhipHit", "Demon_OWhipKnight", "Demon_UWhipKnight",
        ],
        blockable: false,
        bleeds: false,
        damage: 10,
        // Five pixels a frame, both ways, and no table: `DemonMove` writes
        // the step as a literal, `mov ax, 5` / `mov ax, 0xfffb` for the
        // column (0x4fe2, 0x4fed) and `mov bx, 0xfffb` / `mov bx, 5` for the
        // depth (0x4ff6, 0x5001), then adds them at 0x5004 and 0x500a. The
        // old note here had the number right and the clock wrong: it was
        // being applied once a tick, so the demon walked six times as far.
        reach: 65, speed: [5, 5], bounty: 100,
        // `InitKnightvsDemon` writes the record itself, 0x278d..0x27ab:
        // x 100, y 5, z 100, facing 1. It is the one creature that opens on
        // screen and in the middle of it.
        seats: &[[100, 5, 100, 1]], first_seat: 0,
        // `InitKnightvsDemon` (0x27d1) sets `INITMO` to the bare `ret` at
        // 0x2059 and builds the demon itself, so there is no wave at all.
        wave: NO_WAVE,
    },
    Creature {
        id: "beast", name: "Beast", banks: "beast", sheet: "bank.be1", tables: "beast",
        // The beast has no swing: its run is the attack, every `Beast_Run`
        // frame carrying a weapon part. What it does to a knight it reaches
        // is `BeastStruck1` (0x4430), and the four scripts that answers with
        // are the rows below.
        attack: &["Beast_Run1"],
        // `ControlBeast` writes 0x10. `BeastHit2` is the blow-taken table; a
        // blow from above finds the head and its own death. `BeastStruck1`
        // takes five off the knight.
        kind: "chop",
        alternates: &[],
        // `ControlBeast`: it never tracks. It runs from one side of the arena
        // to the other, turns off the edge, waits five to twenty frames, picks
        // a depth and comes back.
        controller: "beast",
        // `BeastStruck1` (0x4430): the impale it plays itself when the blow
        // was the last one and the gore switch is on, and the toss the knight
        // plays out of the beast's own bank tables when it was not.
        rows: &[
            ("impale_back", &["Beast_ImpaleBack"]),
            ("impale_chest", &["Beast_ImpaleChest"]),
            ("back_toss", &["Beast_BackToss"]),
            ("chest_toss", &["Beast_ChestToss"]),
        ],
        border: None, spawns: &[],
        blockable: false,
        bleeds: false,
        damage: 5,
        // `BeastChargeOffsets` 33, 27, 17, 33: nearly thirty pixels a frame.
        // Its weapon parts are its own body, so it has to be allowed close:
        // three quarters of its width would keep it out of its own bite.
        // Read by nobody now: `BeastChargeOffsets` (DS:0x773e) is the
        // charge, and the depth is never stepped at all -- `SetBEASTZ`
        // (0x301d, 0x303b) writes `+6` once per turn and `BeastMove` touches
        // only `+2`. The 24 and 6 here were ours, from before the table was
        // found.
        reach: 40, speed: [0, 0], bounty: 30,
        seats: BEAST_SEATS, first_seat: 0,
        // `InitKnightvsBeast` (0x228d).
        wave: wave(1, 3, 0, true, false, LEV_BEAST),
    },
    Creature {
        id: "balok", name: "Balok", banks: "balok", sheet: "actor.balok", tables: "balok",
        attack: &["Balok_UpperCut"],
        // `ControlBalok` writes 4 for the uppercut and 0x10 for the grab.
        // `BalokStruck` calls `AddBlood`. `BalokStruck1+15` (0x4285) takes
        // five off the knight whatever the kind, and `BalokDam` is dead.
        // Balok has no `*Hit` or `*Wal` table: `Balok_UpperHit` is what
        // `BalokStruck` (0x3771) writes, and the hop is its controller's.
        kind: "swing",
        alternates: &[("chop", "Balok_Grab", 5)],
        controller: "balok",
        // The grab, which `BalokGrabbed` (0x37a0) and the three routines under
        // `ControlBalok`'s own flags word hand over in turn, and the knight
        // that bursts under a landing (`BalokJumping+76`, 0x371d).
        rows: &[
            ("walk", &["Balok_Jump", "Balok_Jumping"]),
            ("hurt", &["Balok_UpperHit"]),
            ("grab", &["Balok_GrabKnight"]),
            ("shake", &["Balok_ShakeKnight"]),
            ("bite", &["Balok_BiteKnight"]),
            ("squeeze", &["Balok_SqueezeKnight"]),
            ("slap_recover", &["Balok_SlapRecover"]),
        ],
        border: None, spawns: &[],
        blockable: false,
        bleeds: true,
        damage: 5,
        // Its uppercut lands from 41 to 74 pixels out, and its own width
        // keeps a knight sixty away, so it swings from just outside that.
        // **Ours**, as the ratman's: the Balok moves in jumps its own script
        // draws (`BalokJumping` 0x3714 writes `+2` and `+6` outright).
        reach: 70, speed: [12, 6], bounty: 80,
        seats: BALOK_SEATS, first_seat: 0,
        // `InitKnightvsBalok` (0x2591): two owed, `MaxMonsters` forced back to
        // one at 0x288a, and `InitBalok` (0x25cf) is the one `INITMO` with no
        // `xor [SIDE], 1`, so every Balok comes in at the same seat.
        wave: wave(1, 2, 1, false, false, LEV_BALOK),
    },
    Creature {
        id: "dragon", name: "Dragon", banks: "dragon", sheet: "actor.dragon", tables: "dragon",
        // The dragon does not walk anywhere. `TrackKnight` shifts its head
        // five pixels at a time inside a corridor thirty to a hundred wide and
        // follows the knight in depth, on the standing frame it is holding.
        // `DragonWal` holds no walk row at all: the rows at +0x10 and +0x20
        // are the head lifting and lowering, which the tables give as
        // `walk_up` and `walk_down` and the controller asks for as `lift`
        // and `lower`.
        attack: &["Dragon_HighBite"],
        // `DragonAttack` writes 2 for the bite, 4 for the low breath and 0x10
        // for the high one. `DragonStruck` calls `AddBlood`. `DragonStruck1`
        // (0x43ad) takes twenty off the knight for the bite (0x43bd) and
        // thirty for the fire (0x43c2), through `TalismanWrym`; the
        // `DragonDam` that `InitKnightvsDragon` rewrites at 0x245c is never
        // read for a blow on him.
        kind: "lunge",
        alternates: &[("swing", "Dragon_LowBreath", 30), ("chop", "Dragon_HighBreath", 30)],
        controller: "dragon",
        rows: &[
            ("walk", &["Dragon_Stance"]),
            ("lift", &[
                "Dragon_LiftHead1", "Dragon_LiftHead2", "Dragon_LiftHead3",
                "Dragon_LiftHead4", "Dragon_LiftHead5", "Dragon_LiftHead5",
                "Dragon_LiftHead5", "Dragon_LiftHead5",
            ]),
            ("lower", &[
                "Dragon_LowerHead1", "Dragon_LowerHead2", "Dragon_LowerHead3",
                "Dragon_LowerHead4", "Dragon_LowerHead5", "Dragon_LowerHead5",
                "Dragon_LowerHead5", "Dragon_LowerHead5",
            ]),
        ],
        border: None,
        // `AddDragonFIRE` starts the breath as a task of its own;
        // `Dragon_BitKnight` is where a bite that lands goes.
        spawns: &["Dragon_Fire", "Dragon_BitKnight", "Dragon_HighStance"],
        blockable: false,
        bleeds: true,
        damage: 20,
        // **Ours**, as the ratman's: the dragon flies, and `ContinueDragon`
        // (0xa5f5) writes its column itself.
        reach: 60, speed: [6, 6], bounty: 250,
        // `InitKnightvsDragon` 0x2476: the head at x 80, forty rows up, facing
        // right. Its `z` of 100 goes the way every other arrival's does.
        seats: &[[80, -40, 100, 1]], first_seat: 0,
        // `InitKnightvsDragon` (0x2519) sets `INITMO` to the `ret` as well.
        wave: NO_WAVE,
    },
    Creature {
        // `InitKnightvsDragon` sets two more actors up beside the dragon,
        // `Claw1TABLE` and `Claw2TABLE`, at x 5 and ten rows either side of
        // the head's depth. `ControlClaw` never subtracts a hit point: they
        // guard the ground in front of the dragon and die when it does.
        // Each claw's record is `SetUpDragonTables` with `Dragon_Claw` written
        // over the stance and the recovery, kind 0x16 and a plane of ten
        // (0x24b8 to 0x24c6), which `henge_formats::tables` reads as
        // `dragon_claw`.
        id: "dragon_claw", name: "Claw", banks: "dragon", sheet: "actor.dragon",
        tables: "dragon_claw",
        attack: &["Dragon_ClawSlap"],
        // `ControlClaw` writes 0xa, the rear thrust's kind, and `ClawStruck1`
        // (0x43d3) takes ten off the knight.
        kind: "rthrust",
        alternates: &[],
        controller: "claw",
        // `Dragon_Claw` holds a blow like a stance; `Dragon_ClawDead` is
        // `Dragon_ClawDead` (0x3b8d, `ClawStruck`).
        rows: &[
            ("walk", &["Dragon_Claw"]),
            ("hurt", &["Dragon_Claw"]),
            ("death", &["Dragon_ClawDead"]),
        ],
        border: None, spawns: &[],
        blockable: false,
        bleeds: false,
        damage: 10,
        reach: 60, speed: [0, 0], bounty: 0,
        // 0x249c and 0x24d5: both claws at x 5, depths 80 and 120, facing
        // right. `DragonMoveClaw1` then pins them either side of the head.
        seats: &[[5, 0, 80, 1], [5, 0, 120, 1]], first_seat: 0,
        // A claw is not a creature the fight counts; the dragon's set piece
        // builds both of them.
        wave: NO_WAVE,
    },
];

/// Where the terrain grids live in the fully unpacked `MAIN.EXE` load image.
///
/// These are the addresses `tools/symbolmap.py` reports for `_MAP:MapType` and
/// `_MAP:MapSLOW`, and the checks below refuse anything that does not look like
/// those tables, so a wrong image is caught rather than baked.
const IMAGE_LEN: usize = 178_224;
const MAPTYPE_AT: usize = 125_890;
const MAPSLOW_AT: usize = 124_890;
/// 40 columns by 26 rows. The going grid is read with the same index, so it is
/// taken at the same size even though `MapSLOW` itself is only 25 rows: the
/// original reads that last row past the end of it, and so do we.
const GRID_LEN: usize = 40 * 26;

/// `MOON:SelectPAL`, at `DS:0x892`, which is image offset `0x123b0 + 0x892`.
///
/// Thirty two Amiga words, which is what `ChooseKnight` hands to the routine
/// that copies `0x20` words into `PALLOC` and then to the fade. It sits in the
/// bottom of DGROUP, which the unpacker used to leave stale; see
/// `docs/REVERSING.md`.
const SELECTPAL_AT: usize = 0x123b0 + 0x892;
/// `MOON:CCOL`, the four portrait columns, right after `SelectPAL`.
const CCOL_AT: usize = 0x123b0 + 0x8d2;

/// The exit code the baker leaves when the image is missing or stale, so a
/// launcher can tell that case from every other failure and run
/// `tools/symbolmap.py` before trying again.
const STALE_IMAGE: i32 = 3;

/// Refuse to bake around a missing or stale `MAIN.EXE` image.
///
/// Everything that matters comes out of that image: the animation scripts,
/// the overworld grids, the lairs, the places and the select palette. For a
/// long time the baker printed a note and carried on without whichever of
/// those it could not read, and the game then started and quietly had the
/// wrong thing on screen. The select screen went black around its knights on
/// one machine that way: the code had learned to read `SelectPAL`, the pack
/// had been rebaked to the new recipe, and the image on that machine was the
/// one the old unpacker had left with the bottom of DGROUP stale. The recipe
/// stamp cannot see that, because the recipe is about the code and not about
/// the research files it reads.
///
/// So: no image is an error, the wrong length is an error, and a stale bottom
/// of DGROUP is an error, each with the command that fixes it. The stale test
/// is two facts about the recovered span that the stale copy cannot have:
/// `SelectPAL` entry 1 is white, `0x0fff`, and `CCOL` is `12, 88, 164, 240`.
/// Returns the fingerprint that goes into the manifest.
fn check_image(src: &str) -> anyhow::Result<String> {
    let fix = format!(
        "make it with\n  python3 tools/symbolmap.py \"{src}/MAIN.EXE\" research/symbols.json \
         --image research/main.final.bin"
    );
    let Some(bytes) = unpacked_image(src) else {
        anyhow::bail!("no unpacked MAIN.EXE image found (research/main.final.bin); {fix}");
    };
    anyhow::ensure!(
        bytes.len() == IMAGE_LEN,
        "the unpacked image is {} bytes, expected {IMAGE_LEN}; {fix}",
        bytes.len()
    );
    let word = |o: usize| u16::from_le_bytes([bytes[o], bytes[o + 1]]);
    let ccol: Vec<u16> = (0..4).map(|i| word(CCOL_AT + i * 2)).collect();
    anyhow::ensure!(
        word(SELECTPAL_AT + 2) == 0x0fff && ccol == [12, 88, 164, 240],
        "the unpacked image is stale: the bottom of DGROUP still holds the packed \
         file's bytes, which an older tools/symbolmap.py left there. {fix}"
    );
    Ok(fingerprint(&bytes))
}

/// FNV-1a over the image, as sixteen hex digits. Not a security hash: it is
/// here so the manifest can say which image it was baked from, and the skip
/// at the top of `main` can see a research file change the way it sees a
/// recipe change.
fn fingerprint(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// The four tables that say where the map's places and lairs stand, all of
/// them in the bottom of DGROUP the unpacker used to leave stale.
///
/// A data symbol's image offset is `0x123b0 + its DS offset`, the same
/// arithmetic `SELECTPAL_AT` does. `MOON:MapIconsTABLE` is at DS:0x498,
/// `MOON:ForestLairs` at DS:0xa6a, `MOON:LairLocation` at DS:0xaca and
/// `MOON:LairType` at DS:0xb2a.
const MAPICONS_AT: usize = 0x123b0 + 0x498;
const FORESTLAIRS_AT: usize = 0x123b0 + 0xa6a;
const LAIRLOCATION_AT: usize = 0x123b0 + 0xaca;
const LAIRTYPE_AT: usize = 0x123b0 + 0xb2a;
/// Twenty four of everything: `mov cx, 0x18` in the lair initialiser at image
/// 0x1ea0, in `MOON:CheckLairEncounter` and in `_MAP:DisplayLairs`.
const LAIR_COUNT: usize = 24;
/// `MOON:MapIconsTABLE` is sixty bytes, which is nine six-byte records and the
/// negative one that stops the walk.
const MAPICONS_LEN: usize = 60;

fn main() -> anyhow::Result<()> {
    // `henge-bake --render-music <dir>` writes each recovered tune out as a
    // WAV, which is how a tune is listened to, or measured, without starting
    // the game. Research only, like the rest of this crate.
    let argv: Vec<String> = std::env::args().collect();
    if let Some(i) = argv.iter().position(|a| a == "--render-music") {
        let dir = argv
            .get(i + 1)
            .cloned()
            .unwrap_or_else(|| "research/music".into());
        let text =
            fs::read_to_string("research/tunes.json").context("reading research/tunes.json")?;
        let tunes: henge_audio::music::Tunes = serde_json::from_str(&text)?;
        fs::create_dir_all(&dir)?;
        for (name, score) in tunes.split() {
            let pcm = henge_audio::music::render(&score);
            let path = Path::new(&dir).join(format!("{name}.wav"));
            fs::write(
                &path,
                henge_audio::music::wav(&pcm, henge_audio::music::RATE),
            )?;
            println!(
                "{}: {} notes, {:.1}s, {}",
                path.display(),
                score.notes.len(),
                score.seconds(),
                if score.looping { "loops" } else { "runs once" }
            );
        }
        return Ok(());
    }
    // The two positionals are the source and the pack; a flag is not one of
    // them, or `play.sh`'s `--force` bakes a pack into a folder called that.
    let mut args = argv
        .iter()
        .skip(1)
        .filter(|a| !a.starts_with("--"))
        .cloned();
    let src = args.next().unwrap_or_else(|| ".".into());
    let out = args.next().unwrap_or_else(|| "packs/reference".into());
    let out = Path::new(&out);

    // A pack already baked by this same recipe is left alone, so the launcher
    // can call the baker every time without costing fifteen seconds a run.
    // Requiring a person to remember a --rebake flag does not work: the pack
    // silently stays as it was, and the game quietly runs without the music,
    // the animation scripts and the overworld grid it needs.
    let force = argv.iter().any(|a| a == "--force" || a == "--rebake");
    // The image comes first, before the skip: a pack that was baked from a
    // stale image and stamped with the current recipe must not be left alone.
    let image = match check_image(&src) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("henge-bake: {e:#}");
            std::process::exit(STALE_IMAGE);
        }
    };
    if !force && out.join("manifest.json").exists() {
        if let Ok(text) = fs::read_to_string(out.join("manifest.json")) {
            if let Ok(old) = serde_json::from_str::<serde_json::Value>(&text) {
                let same_recipe = old.get("recipe").and_then(|r| r.as_u64()) == Some(RECIPE as u64);
                let same_image = old.get("image").and_then(|r| r.as_str()) == Some(image.as_str());
                if same_recipe && same_image {
                    println!(
                        "pack is already baked to recipe {RECIPE} from this image; nothing to do"
                    );
                    return Ok(());
                }
                if same_recipe {
                    println!("the pack was baked from a different MAIN.EXE image; rebaking");
                } else {
                    println!("the pack was baked by an older recipe than {RECIPE}; rebaking");
                }
            }
        }
    }

    let lib = Library::open(&src).context("opening the original game data")?;
    fs::create_dir_all(out.join("sheets"))?;
    fs::create_dir_all(out.join("sounds"))?;

    let mut m = Manifest::new("reference", Provenance::DerivedFromOriginal);
    m.image = image;

    // Palettes, named after the arena family that owns them.
    for (name, _, sheet, _, _) in ARENAS {
        if let Ok(p) = lib.piv(sheet) {
            m.palettes.insert(format!("palette.{name}"), p.palette);
        }
    }
    let fallback = m.palettes.values().next().cloned().unwrap_or_else(|| {
        (0..32)
            .map(|i: u32| (i * 8) << 16 | (i * 8) << 8 | (i * 8))
            .collect()
    });

    // One sheet per actor, all its banks packed together in bank order.
    for (actor, banks) in ACTORS {
        let mut frames: Vec<Sprite> = Vec::new();
        for b in *banks {
            if let Ok(c) = lib.cel(b) {
                frames.extend(c.images);
            }
        }
        if frames.is_empty() {
            continue;
        }
        let id = format!("actor.{actor}");
        let file = format!("sheets/{actor}.png");
        let sheet = pack_sheet(&frames);
        write_indexed(
            &out.join(&file),
            sheet.width,
            sheet.height,
            &sheet.pixels,
            &fallback,
        )?;
        m.sheets.insert(
            id,
            Sheet {
                file,
                frames: sheet.rects,
            },
        );
    }

    // Everything else that is a sprite bank, so nothing is silently dropped.
    // `BOLD.F` and `SMALL.FON` are banks like any other: `CEL` files with no
    // palette of their own, whose indices mean whatever the screen they are
    // drawn on says. A sheet used to name the palette it was authored against
    // here so text could be translated by nearest colour; `GFX:TextP` blits a
    // glyph through 0x5d7f like every other cel and translates nothing.
    // The .C files (BE1, WI1, HEN1, MI) are banks too, in the same format.
    let claimed: Vec<String> = ACTORS
        .iter()
        .flat_map(|(_, banks)| banks.iter().map(|b| b.to_string()))
        .collect();
    for name in lib.with_extension(&["cel", "ob", "c", "f", "fon"]) {
        if claimed.contains(&name) {
            continue;
        }
        let Ok(c) = lib.cel(&name) else { continue };
        if c.images.is_empty() {
            continue;
        }
        let stem = name.split('.').next().unwrap_or(&name).to_lowercase();
        let file = format!("sheets/bank_{stem}.png");
        let sheet = pack_sheet(&c.images);
        write_indexed(
            &out.join(&file),
            sheet.width,
            sheet.height,
            &sheet.pixels,
            &fallback,
        )?;
        m.sheets.insert(
            format!("bank.{stem}"),
            Sheet {
                file,
                frames: sheet.rects,
            },
        );
    }

    // Full-screen images: towns, the map, intro art. .P files are PIVs too.
    for name in lib.with_extension(&["piv", "cmp", "p"]) {
        if let Ok(p) = lib.piv(&name) {
            let stem = name.split('.').next().unwrap_or(&name).to_lowercase();
            let file = format!("sheets/scene_{stem}.png");
            write_indexed(&out.join(&file), piv::W, piv::H, &p.pixels, &p.palette)?;
            m.sheets.insert(
                format!("scene.{stem}"),
                Sheet {
                    file,
                    frames: vec![FrameRect {
                        x: 0,
                        y: 0,
                        w: piv::W as u32,
                        h: piv::H as u32,
                        ox: 0,
                        oy: 0,
                    }],
                },
            );
            m.palettes
                .insert(format!("palette.scene.{stem}"), p.palette);
        }
    }

    // Samples, converted to plain WAV so the engine never learns about VOC.
    let mut sounds = 0;
    for name in lib.names() {
        let Ok(bytes) = lib.bytes(&name) else {
            continue;
        };
        if bytes.len() < 20 || &bytes[..19] != b"Creative Voice File" {
            continue;
        }
        match voc::parse(&bytes) {
            Ok(s) => {
                let stem = name.split('.').next().unwrap_or(&name).to_lowercase();
                let file = format!("sounds/{stem}.wav");
                fs::write(out.join(&file), s.to_wav())?;
                m.sounds.insert(format!("sfx.{stem}"), file);
                sounds += 1;
            }
            Err(e) => eprintln!("  {name}: {e}"),
        }
    }

    // Arena layouts, keyed by the family whose sheets they draw from.
    fs::create_dir_all(out.join("data"))?;
    let mut arenas = BTreeMap::new();
    for name in lib.with_extension(&["t"]) {
        let Ok(t) = lib.terrain(&name) else { continue };
        // The F09/SW9 stubs carry a garbage border list. An arena with no
        // scenery at all is still an arena, and `SWL2.T` is one: it has the
        // same borders as its neighbours and a placement list that is empty on
        // purpose. Dropping it left the fourteenth lair with no layout to
        // fight in.
        let sane = !t.borders.is_empty()
            && t.borders
                .iter()
                .all(|b| b.left < b.right && b.top < b.bottom && b.right < 640 && b.bottom < 400);
        if !sane {
            continue;
        }
        let stem = name.split('.').next().unwrap_or(&name).to_lowercase();
        // Each family says what its own files are called. Matching on the
        // first two letters of the family's *name* worked only because this
        // project happened to name the moors after its `GL` files; a family
        // renamed to what the original calls it would have sent every one of
        // its arenas to the fallback and drawn them over the forest's sky.
        let family = ARENAS
            .iter()
            .find(|(_, prefix, _, _, _)| stem.starts_with(prefix))
            .map(|(f, _, _, _, _)| *f)
            .unwrap_or("forest");
        arenas.insert(stem, serde_json::json!({ "family": family, "terrain": t }));
    }
    fs::write(
        out.join("data/arenas.json"),
        serde_json::to_string(&arenas)?,
    )?;
    m.data
        .insert("data.arenas".into(), "data/arenas.json".into());

    // `COLLIDE.HIT` is the weapon pile's own shape: the samples along each
    // blade cel that `COLCHK` (0x9fcd) walks. It is written out whole, and it
    // is also attached cel by cel to the bank tables below, which is where the
    // simulation reads it.
    let hits = match lib.bytes("COLLIDE.HIT") {
        Ok(bytes) => {
            let c = Collide::parse(&bytes)?;
            fs::write(out.join("data/hitlines.json"), serde_json::to_string(&c)?)?;
            m.data
                .insert("data.hitlines".into(), "data/hitlines.json".into());
            c
        }
        Err(_) => Collide::default(),
    };

    // Which sheets each arena family draws from, and the eight arenas its
    // counter rotates through.
    let key = |f: &str| format!("scene.{}", f.split('.').next().unwrap_or(f).to_lowercase());
    let families: BTreeMap<&str, serde_json::Value> = ARENAS
        .iter()
        .map(|(name, _, sheet, backdrop, rotation)| {
            (
                *name,
                serde_json::json!({
                    "sheet": key(sheet),
                    "backdrop": key(backdrop),
                    "tiles": { "4": key(SHARED_TILES) },
                    "arenas": rotation,
                }),
            )
        })
        .collect();
    fs::write(
        out.join("data/families.json"),
        serde_json::to_string(&families)?,
    )?;
    m.data
        .insert("data.families".into(), "data/families.json".into());

    // The animation task VM: every actor's bank tables, and every script.
    let banks = bank_tables(&lib, &hits);
    fs::write(out.join("data/banks.json"), serde_json::to_string(&banks)?)?;
    m.data.insert("data.banks".into(), "data/banks.json".into());

    let (scripts, tables) = match animation_scripts(&src) {
        Ok(Some(Recovered {
            scripts: set,
            tables,
        })) => {
            let all = || set.values().flat_map(|s| &s.code);
            println!(
                "task VM: {} scripts, {} part records, {} commands",
                set.len(),
                all().filter(|i| matches!(i, Instr::Part(_))).count(),
                all()
                    .filter(|i| !matches!(i, Instr::Part(_) | Instr::EndFrame { .. }))
                    .count(),
            );
            fs::write(out.join("data/scripts.json"), serde_json::to_string(&set)?)?;
            m.data
                .insert("data.scripts".into(), "data/scripts.json".into());
            (set, tables)
        }
        // `check_image` has already found the image, so what is missing is
        // the symbol table beside it, which the same command writes.
        Ok(None) => anyhow::bail!(
            "no symbol table found (research/symbols.json): make it with\n  \
             python3 tools/symbolmap.py \"{src}/MAIN.EXE\" research/symbols.json \
             --image research/main.final.bin"
        ),
        Err(e) => return Err(e.context("animation scripts")),
    };

    fs::write(
        out.join("data/actors.json"),
        actor_definitions(&scripts, &banks, &tables)?,
    )?;
    m.data
        .insert("data.actors".into(), "data/actors.json".into());

    fs::write(out.join("data/fonts.json"), font_definitions())?;
    m.data.insert("data.fonts".into(), "data/fonts.json".into());

    // The intro. `MINDSCAP` is a PIV with no extension, so nothing had picked
    // it up; `INTRO.STI` is the tile map behind the opening pan, and the cast
    // comes out of `INTR.EXE` once its image is expanded.
    match bake_intro(&lib, out, &mut m) {
        Ok(what) => println!("intro: {what}"),
        Err(e) => eprintln!("intro: {e:#}"),
    }

    // The overworld's two grids, lifted out of the unpacked executable. They
    // are what the map reads the ground under a traveller off; the lairs no
    // longer need them, because `LairType` says what ground each lair is
    // fought on and is the table the original itself asks.
    match overworld_tables(&src) {
        Ok(Some((terrain, going))) => {
            let land = serde_json::json!({ "terrain": terrain, "going": going });
            fs::write(
                out.join("data/overworld.json"),
                serde_json::to_string(&land)?,
            )?;
            m.data
                .insert("data.overworld".into(), "data/overworld.json".into());
        }
        Ok(None) => anyhow::bail!("overworld tables: the image vanished during the bake"),
        Err(e) => return Err(e.context("overworld tables")),
    }

    // `MOON:SelectPAL`, the character select screen's own thirty two, lifted
    // out of the same image. It is the one screen in the game whose palette is
    // in the executable rather than in a picture, because the screen has no
    // picture: `ChooseKnight` clears to entry 0 and blits four portraits on it.
    match select_palette(&src) {
        Ok(Some(pal)) => {
            m.palettes.insert("palette.select".into(), pal);
            println!("select: SelectPAL read out of the image");
        }
        Ok(None) => anyhow::bail!("SelectPAL: the image vanished during the bake"),
        Err(e) => return Err(e.context("SelectPAL")),
    }

    // Place icons, so a town is the size the artwork drew it.
    let icons: BTreeMap<u8, (i32, i32)> = lib
        .cel("MI.C")
        .map(|c| {
            c.images
                .iter()
                .enumerate()
                .map(|(i, s)| (i as u8, (s.real_width as i32, s.height as i32)))
                .collect()
        })
        .unwrap_or_default();
    // Where everything on the map stands, out of the same image. Without it
    // the pack keeps the two towns, whose walk-to points are code literals,
    // and has no lairs, no stones, no tower and no Valley: those live only in
    // `MapIconsTABLE` and `LairLocation`, and inventing them again would undo
    // the recovery.
    let marks = match map_marks(&src) {
        Ok(Some(m)) => {
            println!("map: {} places read out of MapIconsTABLE", m.len());
            m
        }
        Ok(None) => anyhow::bail!("MapIconsTABLE: the image vanished during the bake"),
        Err(e) => return Err(e.context("MapIconsTABLE")),
    };
    let lairs = match lair_table(&src) {
        Ok(Some(l)) => {
            println!(
                "lairs: {} read out of ForestLairs, LairLocation and LairType",
                l.len()
            );
            l
        }
        Ok(None) => anyhow::bail!("lair tables: the image vanished during the bake"),
        Err(e) => return Err(e.context("lair tables")),
    };
    fs::write(
        out.join("data/places.json"),
        place_definitions(&icons, &marks, &lairs),
    )?;
    m.data
        .insert("data.places".into(), "data/places.json".into());

    fs::write(out.join("data/items.json"), item_definitions())?;
    m.data.insert("data.items".into(), "data/items.json".into());

    fs::write(out.join("data/knights.json"), knight_definitions())?;
    m.data
        .insert("data.knights".into(), "data/knights.json".into());

    fs::write(out.join("data/palette-effects.json"), palette_effects())?;
    m.data.insert(
        "data.palette.effects".into(),
        "data/palette-effects.json".into(),
    );

    fs::write(out.join("data/battle-palette.json"), battle_palette(&src)?)?;
    m.data.insert(
        "data.battle_palette".into(),
        "data/battle-palette.json".into(),
    );

    fs::write(out.join("data/music-places.json"), music_places())?;
    m.data
        .insert("data.music.places".into(), "data/music-places.json".into());

    // The tunes, if `tools/tunes.py` has been run. Music is derived data like
    // everything else here, so it goes in the pack and never into the tree.
    match bake_music(out, &mut m) {
        Ok(0) => {
            println!("no music: run `python3 tools/tunes.py \"{src}\" research/tunes.json` first")
        }
        Ok(n) => println!("music: {n} tunes"),
        Err(e) => eprintln!("music: {e:#}"),
    }

    fs::write(out.join("manifest.json"), serde_json::to_string_pretty(&m)?)?;
    println!(
        "baked {} sheets, {} sounds, {} palettes, {} data blobs into {}",
        m.sheets.len(),
        sounds,
        m.palettes.len(),
        m.data.len(),
        out.display()
    );
    println!("marked derived-from-original: a release build will refuse to ship it.");
    Ok(())
}

struct Packed {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
    rects: Vec<FrameRect>,
}

/// Row packing: simple, stable, and the frame order stays the bank order, which
/// matters because the animation tables index into it.
fn pack_sheet(frames: &[Sprite]) -> Packed {
    const MAX_W: usize = 1024;
    const GAP: usize = 1;

    let mut rects = Vec::with_capacity(frames.len());
    let (mut x, mut y, mut row_h, mut width) = (0usize, 0usize, 0usize, 0usize);
    for f in frames {
        let (w, h) = (f.real_width.max(1), f.height.max(1));
        if x + w > MAX_W && x > 0 {
            x = 0;
            y += row_h + GAP;
            row_h = 0;
        }
        rects.push(FrameRect {
            x: x as u32,
            y: y as u32,
            w: w as u32,
            h: h as u32,
            // Anchor at the bottom centre: this game positions everything by feet.
            ox: -((w / 2) as i32),
            oy: -(h as i32),
        });
        x += w + GAP;
        row_h = row_h.max(h);
        width = width.max(x);
    }
    let height = y + row_h;
    let width = width.max(1);
    let height = height.max(1);

    let mut pixels = vec![0u8; width * height];
    for (f, r) in frames.iter().zip(&rects) {
        for row in 0..r.h as usize {
            for col in 0..r.w as usize {
                let v = f.pixels[row * f.width + col];
                pixels[(r.y as usize + row) * width + r.x as usize + col] = v;
            }
        }
    }
    Packed {
        width,
        height,
        pixels,
        rects,
    }
}

fn write_indexed(
    path: &Path,
    w: usize,
    h: usize,
    pixels: &[u8],
    palette: &[u32],
) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let file = fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Indexed);
    enc.set_depth(png::BitDepth::Eight);
    let mut pal = Vec::with_capacity(palette.len() * 3);
    for c in palette {
        pal.extend_from_slice(&[(c >> 16) as u8, (c >> 8) as u8, *c as u8]);
    }
    enc.set_palette(pal);
    let mut alpha = vec![255u8; palette.len()];
    if !alpha.is_empty() {
        alpha[0] = 0;
    }
    enc.set_trns(alpha);
    enc.write_header()?.write_image_data(pixels)?;
    Ok(())
}

/// Every actor's bank tables, so a part record's slot number means something.
///
/// Table 1 is the knight and is always loaded; table 2 is whichever creature
/// the encounter loaded; tables 3 and 4 are the shared icon and blood banks.
/// The knight gets the knight in both 1 and 2, because a bout between knights
/// loads one into each, which is what makes a script that switches tables
/// mid-animation work when both fighters are knights.
fn bank_tables(lib: &Library, hits: &Collide) -> BTreeMap<String, BankTables> {
    let mut cache: BTreeMap<String, Bank> = BTreeMap::new();
    let mut out = BTreeMap::new();
    for (creature, slots) in CREATURE_BANKS {
        let mut tables = BankTables::new();
        for (n, files) in [
            (1u8, KNIGHT_BANKS),
            (2, *slots),
            (3, TABLE3_BANKS),
            (4, TABLE4_BANKS),
        ] {
            let banks: Vec<Bank> = files
                .iter()
                .map(|f| bank_of(lib, f, hits, &mut cache))
                .collect();
            if banks.iter().any(|b| !b.cels.is_empty()) {
                tables.insert(n, banks);
            }
        }
        // Table 5 is not one of the four `TASKCELBUF` chooses between. It is
        // `DrBuffer` at DS:0xccbc, which `_MAP:ContinueDragon` builds by
        // writing the pointer at DS:0x8975 into all five of its slots, and
        // `Dragon_Flight1`..`8` run on it. That pointer is `MI.C`, the map
        // icon bank: the loader at 0x88b5 loads `ki.cel` first, then stores
        // where the *next* file will land before loading `mi.c` there, so the
        // address it keeps a second copy of is the map icons and not the moon.
        // Cels 34 to 41 of `MI.C` are the eight frames of a beating wing.
        if *creature == "dragon" {
            let banks: Vec<Bank> = DRAGON_FLIGHT_BANKS
                .iter()
                .map(|f| bank_of(lib, f, hits, &mut cache))
                .collect();
            if banks.iter().any(|b| !b.cels.is_empty()) {
                tables.insert(5, banks);
            }
        }
        out.insert(creature.to_string(), tables);
    }
    // **The stone circle's own table.** `_TAVERN`'s henge routine at image
    // 0xb398 loads `Hen1.c` into `DiceHANDLE` (`DS:0xd113`) and hands `bp` that
    // address to both of its `ADDTASK` calls, so the circle's two scripts index
    // through a one-bank table and nothing else. Table 1 is the table a script
    // uses unless it says otherwise, and neither of these says otherwise.
    let mut henge = BankTables::new();
    henge.insert(1, vec![bank_of(lib, "Hen1.c", hits, &mut cache)]);
    out.insert("henge".to_string(), henge);
    // **The dice table's own table**, the same shape and the same handle:
    // `_TAVERN:load_DiceBACK` at 0xaffd loads `dice.piv` and then `dice.cel`
    // into `DiceHANDLE` (0xb045 `mov dx, Tav3; call ObjLoadV`), and
    // `_TAVERN:ShakeDice` at 0xb18a hands `bp` that address to its `ADDTASK`,
    // so `DD_ShakeDice` and `DD_ThrowDice` index through one bank.
    let mut dice = BankTables::new();
    dice.insert(1, vec![bank_of(lib, "dice.cel", hits, &mut cache)]);
    out.insert("dice".to_string(), dice);
    out
}

/// One bank: which sheet the baker packed it into, where in that sheet it
/// starts, and how big each of its cels is.
///
/// The sizes matter to the simulation, not only to the renderer: a mirrored
/// part is placed at `task_x - (x + cel_width)`, so a width is geometry.
fn bank_of(lib: &Library, file: &str, hits: &Collide, cache: &mut BTreeMap<String, Bank>) -> Bank {
    if file.is_empty() {
        return Bank::default();
    }
    if let Some(b) = cache.get(file) {
        return b.clone();
    }
    // `COLLIDE.HIT` names its blocks the way the bank files are named, but not
    // always in the same case (`kn4.ob`, `Troll1.cel`), so match without it.
    let hit: Vec<Vec<[i16; 2]>> = hits
        .0
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(file))
        .map(|(_, frames)| {
            frames
                .iter()
                .map(|f| f.points.iter().map(|p| [p.x as i16, p.y as i16]).collect())
                .collect()
        })
        .unwrap_or_default();
    let sizes = |name: &str| -> Option<Vec<[u16; 2]>> {
        Some(
            lib.cel(name)
                .ok()?
                .images
                .iter()
                .map(|s| [s.real_width.max(1) as u16, s.height.max(1) as u16])
                .collect(),
        )
    };
    let bank = match ACTORS.iter().find(|(_, banks)| banks.contains(&file)) {
        Some((actor, banks)) => {
            let mut base = 0u32;
            let mut cels = Vec::new();
            for f in *banks {
                let Some(c) = sizes(f) else { continue };
                if *f == file {
                    cels = c;
                    break;
                }
                base += c.len() as u32;
            }
            Bank {
                sheet: format!("actor.{actor}"),
                base,
                cels,
                hit,
            }
        }
        None => {
            let stem = file.split('.').next().unwrap_or(file).to_lowercase();
            Bank {
                sheet: format!("bank.{stem}"),
                base: 0,
                cels: sizes(file).unwrap_or_default(),
                hit,
            }
        }
    };
    cache.insert(file.to_string(), bank.clone());
    bank
}

/// Every animation script in the original, read out of the unpacked load image.
///
/// The image and the symbol table are not something this crate can make:
/// `MAIN.EXE` is PKLITE outside and EXEPACK inside, and both layers are peeled
/// by running their own stubs under emulation in `tools/symbolmap.py`. This
/// looks for the two files that tool writes and says plainly when they are not
/// there, rather than pretending.
/// What comes out of the unpacked image besides the artwork: the scripts,
/// and the controller tables and actor records the `Set*` routines write.
struct Recovered {
    scripts: ScriptSet,
    /// By actor id, under the phases the code compares against:
    /// `SetRatmenTables` reads `[0x8989]` against 0x2d and 0x31, so the
    /// tables are read under those two nights and under one that is
    /// neither. See [`Tables`].
    tables: Tables,
}

/// Every actor's tables under each night the code distinguishes.
///
/// `SetRatmenTables` (0x23ae) and `RatNewMoon` (0x241b) are the only
/// routines that read the moon, comparing `[0x8989]` against 0x2d and 0x31;
/// every other night is the same as any other. Three runs cover it.
#[derive(Default)]
struct Tables {
    /// Under a night the code does not name: 0x2e.
    plain: BTreeMap<String, ActorTables>,
    /// Under 0x2d and 0x31, keyed by the phase's own key.
    nights: BTreeMap<&'static str, BTreeMap<String, ActorTables>>,
    /// The per-frame walk-speed tables each controller's own mover loads,
    /// keyed by the controller name this pack uses. See
    /// `tables::WALK_SPEED_TABLES`: the image has six such tables and twelve
    /// instructions that load one, and nothing else in the game moves by a
    /// controller step at all.
    walk_speed: BTreeMap<String, [Vec<[i32; 2]>; 3]>,
}

impl Tables {
    fn read(img: &[u8], syms: &Symbols) -> anyhow::Result<Tables> {
        use henge_core::moon::Phase;
        let mut t = Tables {
            plain: tables::all_actor_tables(img, syms, Phase::Gibbous.cel() as u16)?,
            nights: BTreeMap::new(),
            walk_speed: BTreeMap::new(),
        };
        for phase in [Phase::Full, Phase::New] {
            t.nights.insert(
                phase.key(),
                tables::all_actor_tables(img, syms, phase.cel() as u16)?,
            );
        }
        // The walk-speed tables belong to the controller, not to the record:
        // `TroggMove` names `TroggWALKR` outright (0x2e4f) and all three
        // troggs go through it. So they are read once, by controller, and
        // cut to the length of that actor's own walk script rows, which is
        // what `NextWalk`'s zero-skip makes the cycle.
        for src in tables::WALK_SPEED_TABLES
            .iter()
            .chain(std::iter::once(&tables::BLACK_KNIGHT_WALK_SPEED))
        {
            // The record whose walk script rows measure this controller's
            // cycle. Actors that share a controller share the rows' shape:
            // all three troggs have `*_WalkR1..3` and `*_WalkU1..4`, and the
            // black knight walks the knight's own `Knight_SwWalk*`.
            let id = match src.controller {
                "beast" => "beast",
                "trogg" => "trogg_axe",
                "trogg_spear" => "trogg_spear",
                "troll" => "troll",
                "mudman" => "mudmen",
                "knight" => "knight",
                other => anyhow::bail!("no record named for controller {other}"),
            };
            let rows = t
                .plain
                .get(id)
                .map(|a| a.walk.clone())
                .ok_or_else(|| anyhow::anyhow!("no walk script rows read for {id}"))?;
            t.walk_speed.insert(
                src.controller.to_string(),
                tables::walk_speed(img, syms, src, &rows)?,
            );
        }
        Ok(t)
    }

    /// One actor's tables on an ordinary night.
    fn of(&self, id: &str) -> Option<&ActorTables> {
        self.plain.get(id)
    }

    /// What the moon does to one actor: the nights on which its hit points
    /// or its blow of the given kind differ from an ordinary night's.
    fn moon(&self, id: &str, kind: u8) -> BTreeMap<String, henge_core::content::MoonStat> {
        let Some(plain) = self.plain.get(id) else {
            return BTreeMap::new();
        };
        let mut out = BTreeMap::new();
        for (key, under) in &self.nights {
            let Some(t) = under.get(id) else { continue };
            let health = t.health.unwrap_or(0);
            let damage = t.damage.get(&kind).copied().unwrap_or(0);
            if t.health != plain.health || t.damage.get(&kind) != plain.damage.get(&kind) {
                out.insert(
                    (*key).to_string(),
                    henge_core::content::MoonStat { health, damage },
                );
            }
        }
        out
    }
}

fn animation_scripts(src: &str) -> anyhow::Result<Option<Recovered>> {
    let images = [
        std::env::args().nth(3).unwrap_or_default(),
        "research/main.final.bin".into(),
        format!("{src}/main.final.bin"),
    ];
    let symbols = [
        std::env::args().nth(4).unwrap_or_default(),
        "research/symbols.json".into(),
        format!("{src}/symbols.json"),
    ];
    let find = |c: &[String]| {
        c.iter()
            .filter(|p| !p.is_empty())
            .find_map(|p| fs::read(p).ok())
    };
    let (Some(img), Some(sym)) = (find(&images), find(&symbols)) else {
        return Ok(None);
    };
    anyhow::ensure!(
        img.len() == IMAGE_LEN,
        "the unpacked image is {} bytes, expected {IMAGE_LEN}",
        img.len()
    );
    let syms = Symbols::parse(&String::from_utf8_lossy(&sym))?;
    let (set, report) = all_scripts(&img, &syms)?;
    anyhow::ensure!(
        report.scripts > 200,
        "only {} scripts parsed; the image or the symbols are not the ones this was \
         recovered from",
        report.scripts
    );
    let tables = Tables::read(&img, &syms).context("the controller tables")?;
    Ok(Some(Recovered {
        scripts: set,
        tables,
    }))
}

/// Every script a set of roots can reach, following every branch.
///
/// An actor definition carries its own scripts, so it has to carry everything
/// they can jump to as well: a swing ends by going to the stance, and a blow
/// taken with no hit points left goes to a death. Anything short of the closure
/// would leave the interpreter pointing at a name nothing defines.
fn closure_of(all: &ScriptSet, roots: &[&str]) -> ScriptSet {
    let mut out = ScriptSet::new();
    let mut queue: Vec<String> = roots.iter().map(|r| r.to_string()).collect();
    let mut seen: BTreeSet<String> = queue.iter().cloned().collect();
    while let Some(name) = queue.pop() {
        let Some(script) = all.get(&name) else {
            continue;
        };
        for i in &script.code {
            let target = match i {
                Instr::Goto { target, .. }
                | Instr::Skip { target }
                | Instr::Dead { target }
                | Instr::AddTask { target }
                | Instr::TestEq { target, .. }
                | Instr::TestNe { target, .. } => target,
                Instr::Shadow { script, .. } => script,
                _ => continue,
            };
            if !target.is_empty() && seen.insert(target.clone()) {
                queue.push(target.clone());
            }
        }
        out.insert(name, script.clone());
    }
    out
}

/// Where the task's origin sits above the actor's feet.
///
/// Read off the actor's own standing frame rather than chosen: the origin is
/// the point the original places parts against, and the feet are the lowest
/// pixel of the parts that frame is made of.
fn origin_of(scripts: &ScriptSet, banks: &BankTables, table: u8, standing: &str) -> [i16; 2] {
    let mut lowest = 0i32;
    let Some(script) = scripts.get(standing) else {
        return [0, 0];
    };
    for i in &script.code {
        let Instr::Part(p) = i else { continue };
        let Some([_, h]) = banks
            .get(&table)
            .and_then(|t| t.get(p.bank as usize))
            .and_then(|b| b.cel(p.cel))
        else {
            continue;
        };
        lowest = lowest.max(p.y as i32 + h as i32);
    }
    [0, -(lowest as i16)]
}

/// The extent of a standing frame's `BODY` parts, as `[left, top, right,
/// bottom]` about the task origin, facing right.
///
/// The parts the script flags `BODY` are what the original's collision code
/// walks, so this is the figure as the original sees it, not as it is drawn:
/// a trogg's spear held out in front is not `BODY` and a blow through it
/// touches nothing.
fn body_extent(
    scripts: &ScriptSet,
    banks: &BankTables,
    table: u8,
    standing: &str,
) -> Option<[i32; 4]> {
    let script = scripts.get(standing)?;
    let mut out: Option<[i32; 4]> = None;
    for i in &script.code {
        let Instr::Part(p) = i else { continue };
        if !p.is(henge_core::taskvm::part_flags::BODY) {
            continue;
        }
        let [w, h] = banks.get(&table)?.get(p.bank as usize)?.cel(p.cel)?;
        let (l, t) = (p.x as i32, p.y as i32);
        let (r, b) = (l + w as i32, t + h as i32);
        out = Some(match out {
            None => [l, t, r, b],
            Some([ol, ot, or, ob]) => [ol.min(l), ot.min(t), or.max(r), ob.max(b)],
        });
    }
    out
}

/// The knight, as the original animates him.
///
/// **The frame lists are gone, and so are the hand-written tables.** The
/// frame lists used to live here, chosen by eye out of `KN1.OB`; the tables
/// that replaced them were transcribed from `SetUpKnight` by hand. Now the
/// routines are run (`henge_formats::tables`) and the knight is what they
/// wrote:
///
/// ```text
/// SetKnightSwTables 01f83  [di+0x10] = Knight_SwStance     scripts["idle"]
///                   01f88  [di+0x12] = Knight_SwRecover    scripts["recover"]
///                   01f92  [di+0x52] = 0x64                approach
///                   01f9c  [di+0x54] = 0x50                back_off
///                   01f97  [di+0x56] = 4                   depth_tolerance
/// SetUpKnight       01786  KnightAttSw[kind]               attacks[kind].script
///                   017e4  KnightDamSw[kind]               attacks[kind].damage
///                   017b5  KnightHitSw[kind]               hurt_by[kind]
///                   01805  KnightWalSw rows +0, +0x10, +0x20  walk, walk_up, walk_down
///                   01852  KnightBloSw[kind]               blocks[kind]
/// ```
///
/// The death is not in any table: it is where the blow-taken script's own
/// `TASKDEAD` goes, and `Knight_SwWaistHit` and `Knight_SwShoulderHit` both
/// go to `Knight_SwDeath`.
///
/// The numbers that are still ours are the ones that were never in the
/// routines: how fast he walks, how far he reaches, how long an opponent
/// waits between swings, and what he is carrying.
fn actor_definitions(
    scripts: &ScriptSet,
    banks: &BTreeMap<String, BankTables>,
    tables: &Tables,
) -> anyhow::Result<String> {
    let knight_banks = banks.get("knight").cloned().unwrap_or_default();
    let kt = tables
        .of("knight")
        .ok_or_else(|| anyhow::anyhow!("knight: SetKnightSwTables was not read"))?;
    let mut def = ActorDef {
        sheet: "actor.knight".into(),
        name: "Knight".into(),
        health: 100,
        // **Read by nobody.** Both seats of this definition have a table.
        // The person's is `K_WalkRValue` and its two siblings, which
        // `ControlKnight` (0x3ec4) reads through `KnightWalkRight`/`Up`/`Down`
        // (0x4048/0x4067/0x4080) and which `henge_core::combat` carries as
        // `KNIGHT_WALK_R_VALUE`. A `flag::DRIVEN` seat's is `BKnightWALKR`/`U`/`D`,
        // which `ControlBlackKnight`'s `M0$`..`M3$` (0x4be6, 0x4bf2, 0x4bfe,
        // 0x4c0a) load before calling `MoveU`/`MoveD`/`MoveR`/`MoveL`, and
        // which is `walk_speed` below.
        //
        // The two hold the same numbers -- `BKnightWALKR` is
        // `(25,0) (3,0) (23,0) (4,0)` against `K_WalkRValue`'s `25 3 23 4` --
        // so the earlier reading here, that a computer knight walks at a flat
        // two a tick because `ControlBlackKnight` calls "the flat movers",
        // was wrong twice over: those movers are not flat, and the black
        // knight has tables of his own that say so.
        speed_x: 0,
        speed_y: 0,
        walk_speed: {
            let rows = tables.walk_speed.get("knight").cloned().unwrap_or_default();
            henge_core::content::WalkSpeed {
                right: rows[0].clone(),
                up: rows[1].clone(),
                down: rows[2].clone(),
            }
        },
        reach: 38,
        // **Not from the record.** `SetKnightSwTables` writes no `+0x35` at
        // all; the knight's is written where a fight is set up, by
        // `InitKnightBattle` (0x408, 0x418), `PracticeCombat5` (0x10d, 0x11d)
        // and `DistanceDONE` (0xa4e1), all six of them `mov byte [reg+0x35], 6`.
        // A computer knight's seat of the same record gets 8 instead, from
        // `InitGameStart`'s four writes (0x1c69, 0x1c88, 0x1ca7, 0x1cc6) --
        // one per seat -- and the two share every `StruckTable` entry
        // (`KnightKnightStruck1`, 0x4407), so one number serves here.
        record_kind: 6,
        depth_tolerance: kt.plane.unwrap_or(0),
        attack_cooldown: 45,
        approach: kt.approach.unwrap_or(0),
        back_off: kt.back_off.unwrap_or(0),
        // What a fallen knight is carrying, for whoever is left standing. Not
        // recovered: the original names `BESTOWGOLD` and a `GOLD` readout but
        // no table of what anything is worth, so this is a number chosen
        // against the prices. Three foes put down pays for a potion and leaves
        // change.
        bounty: 15,
        // `BKwon`: a knight put down is one point of experience.
        experience: 1,
        body: [-9, 0, 9, 50],
        // Six, because six is what the original writes. `InitKnightvsKnight+0x32`
        // (0x208c) stores 6 into `DELAY` (DS:0x91c), and so do the other ten
        // `InitKnightvs*` routines, `InitGameStart+0xda` (0x1ce7) and
        // `InitPractice+0x41` (0x202f) — thirteen writes, no read anywhere in the
        // image. `DELAY` is the length of
        // one combat frame in ticks of the 54.6204 Hz timer the game programs:
        // `Combat` at 0x351 holds each pass to two ticks of the BIOS counter at
        // 0000:046c, which is 109.849 ms, which is six of those timer ticks. See
        // `ActorDef::script_ticks` and `henge_desktop`'s `TIMER_TICK`.
        //
        // This used to be justified off the walk instead, which was the wrong
        // way round: it read the stride's travel against a flat two pixels a
        // tick and arrived at six that way. The person's own per-frame speed is
        // the walk-speed table cited above, not a multiple of a flat per-tick
        // number, so that derivation had nothing to stand on even before the
        // clock under it turned out to be the retrace rather than the timer.
        script_ticks: 6,
        bank_table: 1,
        ..ActorDef::default()
    };
    anyhow::ensure!(
        kt.bank_table == Some(tables::TABLE_1),
        "knight: SetKnightSwTables names bank table {:?}, not table 1",
        kt.bank_table
    );
    scripts_from_tables(&mut def, kt, KNIGHT_ONE_BUTTON, scripts)
        .map_err(|e| anyhow::anyhow!("knight: {e}"))?;
    def.attack = "swing".into();
    def.finishes = KNIGHT_FINISHES
        .iter()
        .map(|(k, s)| (k.to_string(), s.to_string()))
        .collect();
    def.blockable = true;
    // `CONTROLTABLE[6]` is `ControlKnight`, which reads a joystick, and slot 8
    // is `ControlBlackKnight` (`InitGameStart+241`, 0x1cfe). A knight in a
    // seat the machine plays runs the second, which is `Controller::Knight`
    // here; a knight a person plays never runs a controller at all.
    def.controller = "knight".into();
    // Where a knight stands at the opening of a bout. `SetKnightCombat`
    // (0x2962) writes the first record a field at a time, `mov [di+2], 0xfa;
    // mov [di+4], 0; mov [di+6], 0x64; mov byte [di+8], 3`, so the player's
    // knight is at x 250 facing left. The second is the only other one the
    // original has, and both the routines that place it agree to the word:
    // `InitPractice` (0x200a) and `InitKnightvsKnight` (0x206b) put him at
    // x 30 facing right. Both `z` words die in `AddKnight` like everyone's.
    def.seats = vec![[250, 0, 100, 3], [30, 0, 75, 1]];
    let roots: Vec<&str> = def
        .scripts
        .values()
        .flatten()
        .map(String::as_str)
        .chain(def.attacks.values().map(|a| a.script.as_str()))
        .chain(def.hurt_by.values().map(String::as_str))
        .chain(def.finishes.values().map(String::as_str))
        .chain(KNIGHT_SPAWNED.iter().copied())
        .collect();
    def.animation = closure_of(scripts, &roots);
    def.origin = origin_of(&def.animation, &knight_banks, 1, &kt.stance);
    def.banks = knight_banks;
    def.validate().map_err(|e| anyhow::anyhow!("knight: {e}"))?;
    let mut actors = BTreeMap::from([("knight".to_string(), def)]);
    for c in CREATURES {
        let def = creature_definition(c, scripts, banks, tables)?;
        actors.insert(c.id.to_string(), def);
    }
    Ok(serde_json::to_string(&actors)?)
}

/// The name an attack kind is filed under, or nothing for kind 0, which is
/// the stance and not an attack.
fn kind_name(kind: u8) -> Option<&'static str> {
    henge_core::combat::Attack::ALL
        .iter()
        .find(|a| a.kind() == kind)
        .map(|a| a.name())
}

/// Where a script's own `TASKDEAD` goes: the death a blow taken on it ends
/// in. The first one in the script, which is the only one any has.
fn dead_target(scripts: &ScriptSet, name: &str) -> Option<String> {
    scripts.get(name)?.code.iter().find_map(|i| match i {
        Instr::Dead { target } if !target.is_empty() => Some(target.clone()),
        _ => None,
    })
}

/// Fill an actor's states, attacks, blows taken and blocks from what its
/// `Set*Tables` routine and the controller tables say, the same way for the
/// knight and for every creature that has the tables:
///
/// * `idle` is `+0x10` and `recover` `+0x12`;
/// * `walk`, `walk_up` and `walk_down` are the `*Wal` rows, where the table
///   has them;
/// * `attacks` is the `*Att` table by kind with the `*Dam` entry beside it,
///   and `attack` is `*Att[one_button]`;
/// * `hurt_by` is the `*Hit` table by the attacker's kind, `hurt` is its
///   entry for `one_button`, and `death` is where that script's `TASKDEAD`
///   goes;
/// * `blocks` is the `*Blo` table.
///
/// Kind 0 of each table is the stance, which no attack has, and is left out.
fn scripts_from_tables(
    def: &mut ActorDef,
    t: &ActorTables,
    one_button: u8,
    scripts: &ScriptSet,
) -> anyhow::Result<()> {
    anyhow::ensure!(!t.stance.is_empty(), "the record names no stance");
    def.scripts.insert("idle".into(), vec![t.stance.clone()]);
    if !t.recover.is_empty() {
        def.scripts
            .insert("recover".into(), vec![t.recover.clone()]);
    }
    for (row, names) in [
        ("walk", &t.walk[0]),
        ("walk_up", &t.walk[1]),
        ("walk_down", &t.walk[2]),
    ] {
        if !names.is_empty() {
            def.scripts.insert(row.into(), names.clone());
        }
    }
    // `SetMonsterAnims+0x2b1` (0x1b1c) fills `TrollHit` with the table's
    // own address eight times over, so its entries name a table and not a
    // script; `TrollStruck` (0x56d0) never reads it and writes `Troll_Hit`
    // outright. An entry that is not a script is left out here, and the
    // creature's own rows say what its `*Struck` writes.
    let is_script = |name: &String| scripts.contains_key(name);
    for (kind, script) in t.attacks.iter().filter(|(_, s)| is_script(s)) {
        let Some(name) = kind_name(*kind) else {
            continue;
        };
        def.attacks.insert(
            name.into(),
            AttackDef {
                script: script.clone(),
                damage: t.damage.get(kind).copied().unwrap_or(0),
            },
        );
    }
    if let Some(script) = t.attacks.get(&one_button) {
        def.scripts.insert("attack".into(), vec![script.clone()]);
    }
    for (kind, script) in t.hits.iter().filter(|(_, s)| is_script(s)) {
        let Some(name) = kind_name(*kind) else {
            continue;
        };
        def.hurt_by.insert(name.into(), script.clone());
    }
    if let Some(hit) = t.hits.get(&one_button).filter(|s| is_script(s)) {
        def.scripts.insert("hurt".into(), vec![hit.clone()]);
        if let Some(death) = dead_target(scripts, hit) {
            def.scripts.insert("death".into(), vec![death]);
        }
    }
    for (kind, guard) in &t.blocks {
        let (Some(k), Some(g)) = (kind_name(*kind), kind_name(*guard)) else {
            continue;
        };
        def.blocks.insert(k.into(), g.into());
    }
    Ok(())
}

/// One creature, built the same way the knight is: its `Set*Tables` record
/// and the controller tables it points into, the closure of the scripts its
/// states reach, its loader's bank tables, and an origin and a hit box read
/// off its own standing frame.
///
/// The definition is checked whole before it is written. A script named
/// wrongly, a state left empty or a part that no bank in the table can
/// supply stops the bake with the creature and the script named, because
/// the alternative is a creature that composites to a heap and has to be
/// noticed by eye.
fn creature_definition(
    c: &Creature,
    scripts: &ScriptSet,
    banks: &BTreeMap<String, BankTables>,
    recovered: &Tables,
) -> anyhow::Result<ActorDef> {
    let tables = banks
        .get(c.banks)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("{}: no bank tables for loader {}", c.id, c.banks))?;
    let t = recovered
        .of(c.tables)
        .ok_or_else(|| anyhow::anyhow!("{}: no Set*Tables record read for {}", c.id, c.tables))?;
    // Every creature's tables routine stores table 2 into `+0x18`.
    let table = 2u8;
    anyhow::ensure!(
        t.bank_table == Some(tables::TABLE_2),
        "{}: the record names bank table {:?}, not table 2",
        c.id,
        t.bank_table
    );
    let kind = henge_core::combat::Attack::from_name(c.kind)
        .ok_or_else(|| anyhow::anyhow!("{}: {} is not an attack kind", c.id, c.kind))?;
    let mut def = ActorDef {
        sheet: c.sheet.into(),
        name: c.name.into(),
        health: t.health.unwrap_or(0),
        damage: c.damage,
        // Only read where this creature's controller has no table of that
        // kind: the troll's and the mudmen's depth, and the demon's both
        // ways. **Per displayed frame, not per tick** — see `walk_speed`.
        speed_x: c.speed[0],
        speed_y: c.speed[1],
        walk_speed: {
            let rows = recovered
                .walk_speed
                .get(c.controller)
                .cloned()
                .unwrap_or_default();
            henge_core::content::WalkSpeed {
                right: rows[0].clone(),
                up: rows[1].clone(),
                down: rows[2].clone(),
            }
        },
        reach: c.reach,
        // `+0x35`, which this creature's own `Set*Tables` routine writes and
        // `StruckTable` is indexed by.
        record_kind: t.kind.unwrap_or(0),
        depth_tolerance: t.plane.unwrap_or(0),
        attack_cooldown: 45,
        approach: t.approach.unwrap_or(0),
        back_off: t.back_off.unwrap_or(0),
        bounty: c.bounty,
        // What it is worth in experience. The original pays two for the
        // dragon (`_dragon_won`) and nothing for anything else met on the
        // road, whose worth there is the lair it guards. Here the road is
        // where the fights are, so every creature is worth one, the dragon
        // and the demon two; the dragon's is the original's, the rest ours.
        experience: match c.id {
            "dragon" | "demon" => 2,
            _ => 1,
        },
        // Six ticks a frame, as the knight, and for the same reason: each of the
        // eleven `InitKnightvs*` routines writes 6 into `DELAY` (DS:0x91c), which is one
        // combat frame in ticks of the programmed 54.6204 Hz timer. Nothing
        // reads `DELAY` back because `Combat` at 0x351 hardcodes the same
        // duration as a deadline two BIOS ticks ahead. See
        // `ActorDef::script_ticks`.
        script_ticks: 6,
        bank_table: table,
        // What the moon does to it: `SetRatmenTables` is the one routine
        // that reads `[0x8989]`, and the tables were read under each night
        // it compares against. The slash is the kind this creature's blow
        // is filed under; the bite it rewrites beside it is not carried,
        // since `MoonStat` holds one blow.
        moon: recovered.moon(c.tables, kind.kind()),
        ..ActorDef::default()
    };
    // The states the record and the tables name: the stance, the recovery,
    // the walk rows, and the blow taken for the knight's swing with the
    // death its `TASKDEAD` names. The troggs' `*Att` tables give attacks by
    // kind; everyone else's attack is what its controller writes.
    scripts_from_tables(&mut def, t, 4, scripts).map_err(|e| anyhow::anyhow!("{}: {e}", c.id))?;
    // The attack its own controller plays, at the kind it writes, taking
    // the number its `*Struck1` handler subtracts.
    def.attacks.insert(
        c.kind.to_string(),
        AttackDef {
            script: c.attack[0].to_string(),
            damage: c.damage,
        },
    );
    def.scripts.insert(
        "attack".into(),
        c.attack.iter().map(|n| n.to_string()).collect(),
    );
    for (kind, script, damage) in c.alternates {
        def.attacks.insert(
            kind.to_string(),
            AttackDef {
                script: script.to_string(),
                damage: *damage,
            },
        );
    }
    // Rows the controller names in code, over anything the tables gave the
    // same name: the demon's stance cycle, Balok's hop and hit, the claw's.
    for (row, names) in c.rows {
        def.scripts.insert(
            row.to_string(),
            names.iter().map(|n| n.to_string()).collect(),
        );
    }
    // A blow taken named in code rather than in a table dies where its own
    // `TASKDEAD` goes, the same as one from a table.
    if def.scripts_for("death").is_empty() {
        if let Some(death) = def
            .scripts_for("hurt")
            .first()
            .and_then(|h| dead_target(scripts, h))
        {
            def.scripts.insert("death".into(), vec![death]);
        }
    }
    def.controller = c.controller.to_string();
    def.border = c.border;
    def.seats = c.seats.to_vec();
    def.first_seat = c.first_seat;
    def.wave = WaveDef {
        max: c.wave.max,
        heads: c.wave.heads,
        cap: c.wave.cap,
        alternates: c.wave.alternates,
        // Everything with counts of its own has a real `INITMO`; the three with
        // none are `NO_WAVE`, whose `max` of nought is what says so.
        reinforced: c.wave.max > 0,
        opens_with_side: c.wave.opens_with_side,
        level: c.wave.level.to_vec(),
    };
    def.attack = c.kind.to_string();
    def.blockable = c.blockable;
    def.bleeds = c.bleeds;
    let stance = def
        .scripts_for("idle")
        .first()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("{}: no stance", c.id))?;
    let roots: Vec<&str> = def
        .scripts
        .values()
        .flatten()
        .map(String::as_str)
        .chain(def.attacks.values().map(|a| a.script.as_str()))
        .chain(def.hurt_by.values().map(String::as_str))
        .chain(c.spawns.iter().copied())
        .chain(c.bleeds.then_some(BLOOD))
        .collect();
    let animation = closure_of(scripts, &roots);
    let origin = origin_of(&animation, &tables, table, &stance);
    // The hit box is the standing frame's `BODY` parts, narrowed to their
    // middle half the way the knight's was authored (his stance is 36 wide
    // and his box is 18), and as tall as those parts.
    let extent = body_extent(&animation, &tables, table, &stance).ok_or_else(|| {
        anyhow::anyhow!("{}: {stance} has no BODY parts to size a box from", c.id)
    })?;
    let [l, tp, r, b] = extent;
    let (w, mid) = (r - l, (l + r) / 2);
    let feet = -(origin[1] as i32);
    def.body = [
        (mid - w / 4) as i16,
        (feet - b).max(0) as i16,
        (mid + w / 4) as i16,
        (feet - tp) as i16,
    ];
    def.origin = origin;
    def.banks = tables;
    def.animation = animation;
    def.validate()
        .map_err(|e| anyhow::anyhow!("{}: {e}", c.id))?;
    Ok(def)
}

/// Which character each glyph in a font bank draws.
///
/// The game looks glyphs up through a table inside `MAIN.EXE` that is not
/// recovered, so this was read off the artwork: the banks turned out to run
/// A-Z, then a-z, then 0-9, then punctuation.
///
/// **The map was read off the artwork, and it has now been checked against the
/// executable's own table and is right.** `GFX:TextASCII` at image 107,446 is
/// 95 bytes, indexed by `character - 32`, and `TextP` reads it with
/// `sub al, 0x20; mov di, TextASCII; add di, ax; mov al, [di]`. It says:
///
/// ```text
/// A-Z -> 0..25    a-z -> 26..51   0-9 -> 52..61
/// !   -> 62       .   -> 64       ,   -> 65
/// #   -> 66       $   -> 67       %   -> 68
/// anything with no glyph -> 69    '   -> 70       / and \ -> 71
/// ```
///
/// which is this map exactly, with one difference and one gap. The difference
/// is the bold font's glyph 71, guessed here as a bar and actually the slash,
/// the same as the small font's. The gap is glyph 63: **no character maps to
/// it**, so the `?` below is the one entry the table cannot confirm and it is
/// kept as the reading of the artwork it always was.
///
/// A few glyphs near the end are ornaments whose meaning is not obvious. They
/// are left unmapped rather than guessed at, which costs nothing: an unmapped
/// glyph is simply never drawn.
///
/// **The metrics are recovered too.** `TextP` advances by the glyph's own cel
/// width and nothing else, except that `CheckBOLD` sets bit 3 of the record's
/// flag word whenever the current font is `BOLD.F`, and `TextP` then does
/// `sub word ptr [textwidth], 3` before it draws. So the bold face tracks three
/// pixels tight and the small face not at all, and the space is glyph 69 like
/// any other character: fifteen wide in the bold bank, five in the small.
fn font_definitions() -> String {
    const LETTERS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!?.,";
    // Six more, read off the artwork the same way the letters were. Glyph 69 is
    // the blank the `space` field already points at; 70 is an apostrophe; the
    // two banks then diverge, with the small font's last glyph a diagonal stroke
    // and the bold font's a horizontal bar. A status panel prints health as
    // `have/most`, which is why the small font's slash is worth having.
    const SMALL_TAIL: &str = "#$% '/";
    const BOLD_TAIL: &str = "#$% '/";
    serde_json::json!({
        "bold": {
            "sheet": "bank.bold",
            // glyphs[i] is the character glyph i draws.
            "glyphs": format!("{LETTERS}{BOLD_TAIL}"),
            "space": 69,
            // Glyph 69 is fifteen wide and the bold face tracks three tight.
            "space_width": 12,
            "tracking": -3,
            "line_height": 20
        },
        "small": {
            "sheet": "bank.small",
            "glyphs": format!("{LETTERS}{SMALL_TAIL}"),
            "space": 69,
            // Glyph 69 is five wide, and only the bold face is tracked.
            "space_width": 5,
            "tracking": 0,
            "line_height": 8
        }
    })
    .to_string()
}

/// `_MAP:MapType` and `_MAP:MapSLOW`, read straight out of the fully unpacked
/// `MAIN.EXE` load image.
///
/// The image is not something this crate can make: `MAIN.EXE` is PKLITE
/// outside and EXEPACK inside, and both layers are peeled by running their own
/// stubs under emulation in `tools/symbolmap.py`. So this looks for the file
/// that tool writes and says plainly when it is not there, rather than pretending.
///
/// Everything about the result is checked before it is used. The image has to
/// be the right length; every terrain byte has to be one of the four codes
/// `MOON:ColourBackdrop` branches on; and all four codes have to appear, since
/// a table with only one value in it would be a table read from the wrong
/// place. A wrong image fails these rather than baking a plausible lie.
fn overworld_tables(src: &str) -> anyhow::Result<Option<(Vec<u8>, Vec<u8>)>> {
    let candidates = [
        std::env::args().nth(3).unwrap_or_default(),
        "research/main.final.bin".into(),
        format!("{src}/main.final.bin"),
    ];
    let Some(bytes) = candidates
        .iter()
        .filter(|p| !p.is_empty())
        .find_map(|p| fs::read(p).ok())
    else {
        return Ok(None);
    };
    anyhow::ensure!(
        bytes.len() == IMAGE_LEN,
        "the unpacked image is {} bytes, expected {IMAGE_LEN}",
        bytes.len()
    );
    let terrain = bytes[MAPTYPE_AT..MAPTYPE_AT + GRID_LEN].to_vec();
    let going = bytes[MAPSLOW_AT..MAPSLOW_AT + GRID_LEN].to_vec();
    anyhow::ensure!(
        terrain.iter().all(|c| matches!(c, 0 | 2 | 4 | 6)),
        "MapType holds a code the game never branches on"
    );
    for code in [0u8, 2, 4, 6] {
        anyhow::ensure!(
            terrain.contains(&code),
            "MapType has no cells of terrain {code}, so it is not MapType"
        );
    }
    // Only the 1,000 bytes MapSLOW actually owns are checked. The 40 after
    // them are the first row of MapType, which the original reads because it
    // indexes both grids the same way and MapSLOW is one row shorter. Those
    // bytes are terrain codes being used as delay masks, which is nonsense but
    // is the nonsense the game plays, and it only shows on the very bottom row
    // of the map.
    anyhow::ensure!(
        going[..40 * 25].iter().all(|c| *c < 4),
        "MapSLOW holds a mask wider than the two bits CheckSLOW uses"
    );
    Ok(Some((terrain, going)))
}

/// `MOON:SelectPAL`, the character select screen's own thirty two colours.
///
/// **Recovered, and it used to be the one palette in the game that was not.**
/// The select screen has no picture behind it: `ChooseKnight`'s first call
/// clears video memory to entry 0, and the only palette it ever loads is this
/// one, out of the executable's data segment rather than out of a PIV. Its
/// bytes sit in the bottom of DGROUP, which the unpacker left stale until
/// `tools/symbolmap.py` learned to finish the EXEPACK stream; see
/// `docs/REVERSING.md`.
///
/// Everything about the result is checked before it is used, the same way the
/// terrain grids are. Every entry has to be an Amiga `0x0RGB` word, so nothing
/// may have anything in its top nibble; the whole thing may not be black, which
/// is what a still-stale image would give; and the four ramps the portraits are
/// painted in have to be there, which is what `saturated` counts. A wrong image
/// fails these rather than baking a plausible lie.
///
/// What comes out reads as the artwork does: greys at 1 to 4 that all four
/// portraits share, the bold face's five entries at 5 and 9 to 12 the way
/// `CH.PIV` and `MESSAGE.PIV` carry them, greens at 6 to 8, a teal at 15 for
/// the highlight frame, browns at 16 to 20 and then each knight's own: blue at
/// 24 to 26, a gold at 27, red at 29 to 31. That is `KnightGlowColours`' blue,
/// gold, emerald and red arriving independently, which is the check that these
/// are the right sixty four bytes.
fn select_palette(src: &str) -> anyhow::Result<Option<Vec<u32>>> {
    let Some(bytes) = unpacked_image(src) else {
        return Ok(None);
    };
    anyhow::ensure!(
        bytes.len() == IMAGE_LEN,
        "the unpacked image is {} bytes, expected {IMAGE_LEN}",
        bytes.len()
    );
    let words: Vec<u16> = (0..palette::ENTRIES)
        .map(|i| {
            let o = SELECTPAL_AT + i * 2;
            u16::from_le_bytes([bytes[o], bytes[o + 1]])
        })
        .collect();
    anyhow::ensure!(
        words.iter().all(|w| *w <= 0x0fff),
        "SelectPAL holds a word wider than the twelve bits a colour has, so the \
         bottom of DGROUP is still the stale duplicate: re-run tools/symbolmap.py"
    );
    anyhow::ensure!(
        words.iter().any(|w| *w != 0),
        "SelectPAL is all black, so it is not SelectPAL"
    );
    let saturated = words
        .iter()
        .filter(|w| {
            let (r, g, b) = ((*w >> 8) & 0xf, (*w >> 4) & 0xf, *w & 0xf);
            r.max(g).max(b) - r.min(g).min(b) >= 4
        })
        .count();
    anyhow::ensure!(
        saturated >= 8,
        "SelectPAL has only {saturated} colours in it, and four knights in four \
         colours need more than that"
    );
    Ok(Some(
        words
            .iter()
            .map(|w| {
                let (r, g, b) = ((*w >> 8) & 0xf, (*w >> 4) & 0xf, *w & 0xf);
                ((r as u32 * 17) << 16) | ((g as u32 * 17) << 8) | (b as u32 * 17)
            })
            .collect(),
    ))
}

/// The fully unpacked `MAIN.EXE` load image, wherever the person running this
/// keeps it. Not something this crate can make: see `animation_scripts`.
fn unpacked_image(src: &str) -> Option<Vec<u8>> {
    [
        std::env::args().nth(3).unwrap_or_default(),
        "research/main.final.bin".into(),
        format!("{src}/main.final.bin"),
    ]
    .iter()
    .filter(|p| !p.is_empty())
    .find_map(|p| fs::read(p).ok())
}

/// Where each place sits, and how big it is.
///
/// **Two of the five coordinates are recovered and the rest are not, and the
/// difference is worth stating.** `_MAP:KnightGoesToTown` carries the two towns
/// as literals: a knight heading for Highwood walks to map (94, 47) and for
/// Waterdeep to (297, 157), and the same routine works out which is nearer from
/// the grid cells (12, 7) and (37, 20). Those cells are exactly what the
/// recovered grid formula turns those pixels into, which is what makes both
/// pairs trustworthy rather than merely present.
///
/// What those numbers are is the spot a knight is sent to, not the corner of
/// the picture. The corner lives in `MOON:MapIconsTABLE`, which used to be
/// unreadable: it is in the first 2,906 bytes of DGROUP, and the unpacker left
/// that span stale. So the box is **built** here rather than recovered: the
/// icon's size comes from the `MI.C` bank, which is real, and it is hung so
/// that the recovered destination sits in the middle of it. Both towns land on
/// their own artwork when it is drawn, which is the check that it is not
/// nonsense, but it remains a construction.
///
/// **That span is readable now.** `tools/symbolmap.py` finishes the EXEPACK
/// stream instead of stopping where the emulated stub stops, and
/// `MapIconsTABLE`, `LairLocation` and `LairType` are all in what came back. So
/// every place on the map could stop being a construction. That is the map's
/// own change and not this one, and it has since been made: every place on the
/// map, the four home villages included, is read out of `MapIconsTABLE` now,
/// and the two towns keep the construction below only as a fallback for a pack
/// baked without the image.
/// What each entry of `MOON:CombatTable` puts in the arena.
///
/// **Recovered**, out of `MOON:InitGameStart` at image 0x1d44, which is a run
/// of `mov word ptr [si + n], imm` with `si` holding `CombatTable`'s DS
/// offset 0x6988. The table is BSS, so it is zero in the load image and only
/// these immediates say what is in it. They are link-time code addresses and
/// take the seven-step correction `tools/symbolmap.py` fits, after which all
/// thirteen land exactly on an `InitKnightvs*` entry point.
///
/// The run just above it, at image 0x1ced with `si` at DS:0x6964, fills the
/// parallel controller table with `ControlBeast`, `ControlMudmen`,
/// `ControlDemon`, `ControlKnight`, `ControlBlackKnight`, `ControlDragon`,
/// three `ControlTrogg`s, `ControlRatmen` and so on, slot for slot. Two
/// tables filled in the same order from two different sets of routines is
/// what pins the meaning of each slot; slot 4 is the black knight, whose
/// setup routine is the plain knight's.
///
/// Slots 10, 11, 13, 15 and 17 are never written and no lair asks for them.
/// Three of the thirteen set up a knight, who is not in the bestiary because
/// he is the player's own actor; see [`guardian_is_known`]. No lair asks for
/// one of those either.
const GUARDIANS: &[(u16, &str)] = &[
    (0, "beast"),
    (1, "mudmen"),
    (2, "demon"),
    (3, "knight"),
    (4, "knight"),
    (5, "dragon"),
    (6, "trogg_axe"),
    (7, "trogg_hammer"),
    (8, "trogg_spear"),
    (9, "ratmen"),
    (12, "balok"),
    (14, "knight"),
    (16, "troll"),
];

/// Whether the pack has somebody to put in the arena under this name. The
/// knight is not in `CREATURES`: he is the player's own actor, and the three
/// `CombatTable` slots that set up a knight fight name him rather than a
/// creature.
fn guardian_is_known(id: &str) -> bool {
    id == "knight" || CREATURES.iter().any(|c| c.id == id)
}

/// One lair, as the four tables the initialiser copies from describe it.
struct LairPlace {
    /// `ForestLairs[2n]`, a byte offset into `CombatTable`, resolved through
    /// [`GUARDIANS`] to a creature the bestiary has.
    guardian: &'static str,
    /// `ForestLairs[2n+1]`, which the initialiser writes to record `+0x04`:
    /// `TotalMonsters`, everything that comes at you before the lair is clear,
    /// not everything that stands in the arena at once.
    count: u32,
    /// `LairLocation[2n]` and `[2n+1]`, record `+0x0a` and `+0x0c`. The corner
    /// `_MAP:DisplayLairs` blits icon frame 0x14 at.
    x: i32,
    y: i32,
    /// `LairType[n]`, record `+0x0e`, in `_MAP:MapType`'s own coding: the
    /// landscape `InitLair` hands to `ColourBackdrop`, which is what decides
    /// the arena family. It is the ground the fight happens on, and it is not
    /// read back off the map: lair 15 stands one cell into the treeline and is
    /// still fought in the marsh.
    code: u8,
}

/// The twenty four lairs: the guardian, the head count, where it stands and
/// what ground it is fought on.
///
/// **Recovered, all four tables.** The initialiser's copy loop at image
/// 0x1ea0 is what says what each of them is. It runs twenty four times over
/// eighteen-byte records with `di` on `ForestLairs`, `bx` on `LairLocation`,
/// `bp` on `LairType` and `si` on `LairFile`:
///
/// ```text
/// mov ax, [di]  ; add di, 2 ; mov [si+0x02], ax    which CombatTable entry
/// mov ax, [di]  ; add di, 2 ; mov [si+0x04], ax    how many
/// mov ax, [bx]  ; add bx, 2 ; mov [si+0x0a], ax    x
/// mov ax, [bx]  ; add bx, 2 ; mov [si+0x0c], ax    y
/// mov ax, [bp]  ; add bp, 2 ; mov [si+0x0e], ax    landscape
/// mov ax, [si]  ; add si, 2 ; mov [si+0x10], ax    arena layout
/// ```
///
/// So `ForestLairs` is 24 pairs of words and 96 bytes, `LairLocation` 24 pairs
/// and 96 bytes, and `LairType` 24 single words and 48 bytes, which is exactly
/// what the symbol table gives for their sizes. Nothing here is inferred from
/// the shape of the bytes; the reading is the code's.
///
/// **The arena layouts and their order are recovered too**, and were already.
/// `MOON:LairFile` is 24 pointers to `fol1.t`..`fol6.t`, `wal1.t`..`wal6.t`,
/// `swl1.t`..`swl6.t`, `gll1.t`..`gll6.t`, and it starts four bytes past the
/// end of the span the unpacker used to leave stale, so it is the one of the
/// four the load image carried even then. That order is what plants the keys:
/// the initialiser steps six records between one key and the next, so lair 0
/// to 5 are the forest's, 6 to 11 the wastes', 12 to 17 the marsh's and 18 to
/// 23 the glades', matching `moon::Key::ALL`.
///
/// Everything is checked before it is used, the way the terrain grids are.
/// `LairType` has to agree with `LairFile`'s own family for all twenty four,
/// which is two independent tables saying the same thing; every guardian has
/// to be a slot `InitGameStart` fills and a creature the bestiary has; every
/// coordinate has to be on the map; and no two lairs may stand on one spot.
/// A stale image fails these rather than baking a plausible lie.
fn lair_table(src: &str) -> anyhow::Result<Option<Vec<LairPlace>>> {
    let Some(bytes) = unpacked_image(src) else {
        return Ok(None);
    };
    anyhow::ensure!(
        bytes.len() == IMAGE_LEN,
        "the unpacked image is {} bytes, expected {IMAGE_LEN}",
        bytes.len()
    );
    let word = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
    let mut lairs = Vec::with_capacity(LAIR_COUNT);
    for n in 0..LAIR_COUNT {
        let slot = word(FORESTLAIRS_AT + n * 4) / 2;
        let count = word(FORESTLAIRS_AT + n * 4 + 2);
        let x = word(LAIRLOCATION_AT + n * 4) as i32;
        let y = word(LAIRLOCATION_AT + n * 4 + 2) as i32;
        let code = word(LAIRTYPE_AT + n * 2);
        let guardian = GUARDIANS
            .iter()
            .find(|(s, _)| *s == slot)
            .map(|(_, id)| *id);
        let guardian = guardian.with_context(|| {
            format!("lair {n} wants CombatTable entry {slot}, which InitGameStart never fills")
        })?;
        anyhow::ensure!(
            guardian_is_known(guardian),
            "lair {n} is guarded by {guardian}, which the pack has nobody for"
        );
        anyhow::ensure!(
            (1..=64).contains(&count),
            "lair {n} fields {count} monsters, so ForestLairs is not ForestLairs"
        );
        anyhow::ensure!(
            matches!(code, 0 | 2 | 4 | 6),
            "lair {n} is fought on landscape {code}, which ColourBackdrop never branches on"
        );
        let family = family_of(code as u8);
        let want = henge_core::moon::Key::ALL[n / 6].family();
        anyhow::ensure!(
            family == want,
            "lair {n} is {family} by LairType and {want} by LairFile, so one of the two \
             tables is being read out of the wrong place"
        );
        anyhow::ensure!(
            (0..=320 - LAIR_W).contains(&x) && (0..=200 - LAIR_H).contains(&y),
            "lair {n} stands at ({x}, {y}), which is off the map"
        );
        lairs.push(LairPlace {
            guardian,
            count: count as u32,
            x,
            y,
            code: code as u8,
        });
    }
    for (n, a) in lairs.iter().enumerate() {
        anyhow::ensure!(
            !lairs[n + 1..].iter().any(|b| b.x == a.x && b.y == a.y),
            "two lairs stand at ({}, {})",
            a.x,
            a.y
        );
    }
    Ok(Some(lairs))
}

/// The landscape codes `_MAP:MapType` and `MOON:LairType` share, in the names
/// this project files the four arena families under. `_MAP:FindLandscape`
/// stores the byte and `MOON:ColourBackdrop` switches on it.
fn family_of(code: u8) -> &'static str {
    match code {
        2 => "forest",
        4 => "swamp",
        6 => "waste",
        _ => "glade",
    }
}

/// The arena layout each lair is fought in: `fol1`, `wal1` and the rest,
/// which is `LairFile` with the extension taken off.
fn lair_arena(family: &str, n: usize) -> String {
    let prefix = match family {
        "forest" => "fo",
        "waste" => "wa",
        "swamp" => "sw",
        _ => "gl",
    };
    format!("{prefix}l{}", n + 1)
}

/// `MOON:MapIconsTABLE`: where every place on the overworld stands.
///
/// **Recovered.** `MOON:CheckGROOC`'s walk at image 0x70b reads three words a
/// pass, `mov ax, [si]; add si, 2` three times over, stops on a negative first
/// word, and hands the three to the overlap test as icon frame, x and y. The
/// box it measures comes from `MOON:GetWIDTH` on that frame, so the pair is
/// the icon's top-left corner and not its middle. Sixty bytes is nine records
/// and the terminator.
///
/// What the frames are is recovered from the same routine and from
/// `_MAP:StackMessages`, the table of lines the map's own gadget list carries:
/// 0x15 to 0x18 are the four villages, each gated on `[di+0x20]`, the knight's
/// own index, so a village belongs to one knight and only he may enter it;
/// 0x19 is Highwood, 0x1a Waterdeep, 0x1b Stonehenge, 0x1c the Valley of the
/// Gods and 0x1e Math the Wizard.
///
/// Checked before use: the walk has to terminate inside the table, every frame
/// has to be one `StackMessages` has a line for, no frame may appear twice,
/// every corner has to be on the map, and both towns have to be there, since
/// `_MAP:KnightGoesToTown` carries their walk-to points as literals and they
/// are the one cross-check the map has on itself.
fn map_marks(src: &str) -> anyhow::Result<Option<BTreeMap<u8, (i32, i32)>>> {
    let Some(bytes) = unpacked_image(src) else {
        return Ok(None);
    };
    anyhow::ensure!(
        bytes.len() == IMAGE_LEN,
        "the unpacked image is {} bytes, expected {IMAGE_LEN}",
        bytes.len()
    );
    let word = |at: usize| i16::from_le_bytes([bytes[at], bytes[at + 1]]) as i32;
    let mut marks: BTreeMap<u8, (i32, i32)> = BTreeMap::new();
    let mut at = MAPICONS_AT;
    loop {
        anyhow::ensure!(
            at + 6 <= MAPICONS_AT + MAPICONS_LEN,
            "MapIconsTABLE runs off the end of itself without a negative frame"
        );
        let frame = word(at);
        if frame < 0 {
            break;
        }
        let (x, y) = (word(at + 2), word(at + 4));
        anyhow::ensure!(
            (0x15..=0x22).contains(&frame),
            "MapIconsTABLE names icon frame {frame}, which StackMessages has no line for"
        );
        anyhow::ensure!(
            (0..320).contains(&x) && (0..200).contains(&y),
            "the place at icon frame {frame} stands at ({x}, {y}), which is off the map"
        );
        anyhow::ensure!(
            marks.insert(frame as u8, (x, y)).is_none(),
            "MapIconsTABLE names icon frame {frame} twice"
        );
        at += 6;
    }
    for (frame, town) in [(0x19u8, "Highwood"), (0x1a, "Waterdeep")] {
        anyhow::ensure!(
            marks.contains_key(&frame),
            "MapIconsTABLE has no {town}, so it is not MapIconsTABLE"
        );
    }
    Ok(Some(marks))
}

/// `MI.C` frame 0x14, the icon `_MAP:DisplayLairs` blits at every lair whose
/// coordinates are not negative. Nine by five.
const LAIR_W: i32 = 9;
const LAIR_H: i32 = 5;

/// Where each place sits, and how big it is.
///
/// **Every coordinate on the map is recovered.**
/// `MOON:MapIconsTABLE` is the original's own list of what stands on the
/// overworld and where, and it is read out of the image by [`map_marks`]. Each
/// record's pair is the top-left corner of the icon the overlap test measures,
/// so it is exactly what a place's box wants; the size still comes from the
/// `MI.C` bank, because the table carries a frame number and not a rectangle.
/// The lairs come out of [`lair_table`] the same way.
///
/// That table used to be unreadable: it is in the bottom of DGROUP, and the
/// unpacker left that span stale. Everything below used to be **built** here
/// instead, by centring the two towns on the walk-to points
/// `_MAP:KnightGoesToTown` carries as literals, (94, 47) and (297, 157), and
/// by hanging the rest on whatever landmark the map painted nearby. That is
/// gone. What the construction got wrong is worth recording: the ruin in the
/// southern woods this project called a hermit's is Stonehenge, and the ring in
/// the middle of it all this project called Stonehenge is the Valley of the
/// Gods. Both were sited on the right artwork under the wrong name. There was
/// also a second healer, a hermit, invented and stood in the southern woods;
/// the original has no such place and it is gone.
///
/// The two towns keep the `KnightGoesToTown` construction as a fallback, so a
/// pack baked without the unpacked image still has somewhere to buy a sword.
/// Everything else the table names is baked only when the table is there.
///
/// **The four villages are recovered and baked.** Frames 0x15 to 0x18 are in
/// the table at (18, 11), (286, 11), (0, 187) and (303, 192), one in each
/// corner, and `MOON:CheckGROOC` at image 0x732 gates each on `[di+0x20]`, the
/// knight's own colour index, so a village belongs to one knight and the other
/// three cannot see it at all. That gate is the place's `knight`. What is in one
/// is `ForestVillage` at 0x112a, which all four go to (`TakingMoon`, 0xc99 to
/// 0xcb6): one life point, and `cmp byte [si+0x31], 3 / jge` will not take a
/// knight past three. It costs nothing and no day passes.
///
/// **The lines the map's paper carries are recovered**, from `_MAP`:
/// `knhigh` `Enter the city of Highwood`, `knwater` `Enter the city of
/// Waterdeep`, `knhenge` `Enter Stonehenge`, `knmath` `Visit Math the Wizard`,
/// `knvalley` `Enter Valley of the Gods` and `knlair` `Enter Lair`. They are
/// baked as each place's `line`, which is what `_MAP:OrderOpt` composes a
/// numbered line out of: kind 2 takes `knlair` outright, kind 1 takes `knkn`
/// (`Battle with `) and the rival's name, and everything else indexes
/// `StackMessages` at `DS:0xc404` by `kind - 0x15`, the kind being the `MI.C`
/// frame `MapIconsTABLE` gives. The villages' line is `knvillage`, which is
/// `StackMessages[0]` and reads `Enter Village` for all four of them.
///
/// **A town's five gadgets open five routines, and none of them is a place.**
/// `MOON:HWLOOP` (image 0xe35) and `WDLOOP` (0xd7a) branch on the id of the
/// gadget fire was over: `MERC` (0xe9c) is `mov ax, 5` and the status panel,
/// `HTEM` (0xeab) `mov ax, 6` and the same panel, `TAV` (0xe90) `_TAVERN`'s
/// routine at 0xb007, `HEAL` (0xeba) `_WIZARD`'s at 0xba66, `MYST` (0xdd5)
/// `_WIZARD`'s at 0xb935, and `CEXIT` (0xe0b) the map. So the merchant and the
/// temple are two pages of the character sheet, the tavern is the hand over
/// `TAV.PIV` with six gadgets on its parchment, and the healer and the mystic
/// are a greeting, the donation bowl and a verdict over `HEA.PIV` and
/// `MYS.PIV`. All of that is `henge_core::town`; a place carries only which
/// door each of its five boxes is. The six hidden rooms with lists of lines
/// that used to stand behind those boxes (a stall, a tavern menu, a dice room,
/// a healer's menu, a temple's list, a mystic's menu) are gone.
fn place_definitions(
    icons: &BTreeMap<u8, (i32, i32)>,
    marks: &BTreeMap<u8, (i32, i32)>,
    lairs: &[LairPlace],
) -> String {
    // MI.C frame numbers, which are also the kinds the original's menu table is
    // indexed by: 0x19 Highwood, 0x1a Waterdeep, 0x1b Stonehenge.
    let icon = |frame: u8, fallback: (i32, i32)| *icons.get(&frame).unwrap_or(&fallback);
    // A place's box, hung so that `goal` is the middle of it. `goal` is where
    // the traveller's own 8x10 token stands, so its middle is offset by half of
    // that before the icon is centred on it. Only the two towns still need it,
    // and only when `MapIconsTABLE` is not there to be read.
    let at = |goal: (i32, i32), size: (i32, i32)| {
        (
            goal.0 + 4 - size.0 / 2,
            goal.1 + 5 - size.1 / 2,
            size.0,
            size.1,
        )
    };
    // A place whose corner `MapIconsTABLE` gives and whose size `MI.C` does.
    let mark =
        |frame: u8, size: (i32, i32)| marks.get(&frame).map(|(x, y)| (*x, *y, size.0, size.1));
    let highwood =
        mark(0x19, icon(0x19, (25, 32))).unwrap_or_else(|| at((94, 47), icon(0x19, (25, 32))));
    let waterdeep =
        mark(0x1a, icon(0x1a, (32, 28))).unwrap_or_else(|| at((297, 157), icon(0x1a, (32, 28))));
    let stones = mark(0x1b, icon(0x1b, (18, 12)));
    let valley = mark(0x1c, icon(0x1c, (13, 10)));
    let wizard = mark(0x1e, icon(0x1e, (7, 20)));
    // The four home villages, frames 0x15 to 0x18 in table order, which is also
    // the order of the knights' own colour indices that `CheckGROOC` gates them
    // on. `MI.C` gives each its size the way it gives a town one.
    let villages: Vec<(usize, (i32, i32, i32, i32))> = (0x15u8..=0x18)
        .enumerate()
        .filter_map(|(whose, frame)| mark(frame, icon(frame, (8, 10))).map(|box_| (whose, box_)))
        .collect();

    // `ForestVillage` (0x112a): one life point, and three is as high as it goes.
    // What it says is ours; the routine itself says nothing, it jumps back out
    // to the map through `ColourStatus`.
    let village = serde_json::json!({
        "do": "village",
        "said": "Your own people take you in.",
        "refused": "They have nothing more to give."
    });
    let leave = serde_json::json!({ "do": "leave" });
    // `HWLOOP`'s ladder, by the name of each rung's routine.
    let door = |which: &str| serde_json::json!({ "do": "door", "door": which });

    let mut places = serde_json::Map::new();

    places.insert(
        "highwood".into(),
        serde_json::json!({
            "name": "Highwood",
            // `_MAP:knhigh` at `DS:0xc329`, which `StackMessages[0x19 - 0x15]`
            // points at: the line the map's paper carries for this box.
            "line": "Enter the city of Highwood",
            "scene": "scene.highwood",
            "x": highwood.0, "y": highwood.1, "w": highwood.2, "h": highwood.3,
            "menu": [256, 0, 62, 200],
            // `MOON:InitHighWood` at image 0xec9: five gadgets, 64 wide, at
            // `[si+0xa]` 0x100 and `[si+0xc]` 0x1e, 0x42, 0x6a, 0x8c and 0xb7,
            // heights 0x10, 0x10, 0x10, 0x1a and 0xc, with `+0xe` 1 to 5, which
            // is the ladder `HWLOOP` at 0xe35 tests: `MERC`, `TAV`, `HEAL`,
            // `HTEM`, `CEXIT`. They sit on `Merchant`, `Tavern`, `Healer`,
            // `High Temple` and `Exit`, which `HIGHWOOD.PIV` has painted down
            // its parchment, so nothing is drawn over them.
            "boxes": [
                [256, 0x1e, 64, 0x10],
                [256, 0x42, 64, 0x10],
                [256, 0x6a, 64, 0x10],
                [256, 0x8c, 64, 0x1a],
                [256, 0xb7, 64, 0x0c]
            ],
            // `HWINIT` at 0xe1a: `mov word ptr [PointerX], 0x122; mov word
            // ptr [PointerY], 0x64`, on the way in and every time a door
            // comes back.
            "pointer": [0x122, 0x64],
            "options": [
                { "label": "Merchant", "effect": door("merchant") },
                { "label": "Tavern",   "effect": door("tavern") },
                { "label": "Healer",   "effect": door("healer") },
                { "label": "Temple",   "effect": door("temple") },
                { "label": "Leave",    "effect": leave }
            ]
        }),
    );
    places.insert(
        "waterdeep".into(),
        serde_json::json!({
            "name": "Waterdeep",
            // `_MAP:knwater` at `DS:0xc344`, `StackMessages[0x1a - 0x15]`.
            "line": "Enter the city of Waterdeep",
            "scene": "scene.waterdee",
            "x": waterdeep.0, "y": waterdeep.1, "w": waterdeep.2, "h": waterdeep.3,
            "menu": [2, 0, 62, 200],
            // `MOON:InitWaterDeep` at image 0xf4a, the same five on the other
            // edge: x 0, y 0x1a, 0x3f, 0x65, 0x86 and 0xb6, heights 0x10, 0x10,
            // 0x10, 0x1f and 0xc. The fourth is the mystic rather than the
            // temple, which is the only difference between the two towns.
            "boxes": [
                [0, 0x1a, 64, 0x10],
                [0, 0x3f, 64, 0x10],
                [0, 0x65, 64, 0x10],
                [0, 0x86, 64, 0x1f],
                [0, 0xb6, 64, 0x0c]
            ],
            // `WDINIT` at 0xd5f: (0x1e, 0x64).
            "pointer": [0x1e, 0x64],
            "options": [
                { "label": "Merchant", "effect": door("merchant") },
                { "label": "Tavern",   "effect": door("tavern") },
                { "label": "Healer",   "effect": door("healer") },
                { "label": "Mystic",   "effect": door("mystic") },
                { "label": "Leave",    "effect": leave }
            ]
        }),
    );
    // The four home villages, one in each corner of the map.
    //
    // **A village has no picture and no menu.** `_MAP:StackDecision` hands the
    // kind to `TakingMoon`, which jumps frames 0x15 to 0x18 straight to
    // `ForestVillage` (0xc99 to 0xcb6, all four to 0x112a); that routine gives
    // the life point and returns through `ColourStatus` to the map. No backdrop
    // is loaded anywhere on the path, so the village is a line on the paper and
    // nothing more. `scene` is empty to say so, and `henge`'s own map opens it
    // where it stands instead of changing screens.
    for (whose, (x, y, w, h)) in &villages {
        places.insert(
            format!("village.{}", whose + 1),
            serde_json::json!({
                "name": "Village",
                // `_MAP:knvillage` at `DS:0xc3a0`, which is
                // `StackMessages[0x15 - 0x15]` and the entry for all four.
                "line": "Enter Village",
                "scene": "",
                "x": x, "y": y, "w": w, "h": h,
                // `CheckGROOC` 0x732: frame 0x15 is knight 0's, 0x16 knight
                // 1's, 0x17 knight 2's and 0x18 knight 3's.
                "knight": whose,
                "menu": [0, 0, 0, 0],
                "options": [
                    { "label": "Enter Village", "effect": village },
                    { "label": "Leave",         "effect": leave }
                ]
            }),
        );
    }
    // The stones, at `MapIconsTABLE`'s frame 0x1b. `MOON:Henge` tests the
    // moonstone bits against tonight's moon before it offers anything else;
    // short of that the druids take an offering, which is what the
    // between-days screen tells you to bring them: `Offer a magic item within
    // Stonehenge and Danu will grant you a longer life`.
    if let Some(stones) = stones {
        places.insert(
            "stones".into(),
            serde_json::json!({
                "name": "The Stones",
                // `_MAP:knhenge` at `DS:0xc360`, `StackMessages[0x1b - 0x15]`.
                "line": "Enter Stonehenge",
                "scene": "scene.hen1",
                "x": stones.0, "y": stones.1, "w": stones.2, "h": stones.3,
                "menu": [8, 16, 148, 40],
                "text": [6, 146, 308, 42],
                "options": [
                    { "label": "Offer a magic item", "effect": { "do": "offer" } },
                    { "label": "Leave",              "effect": leave }
                ]
            }),
        );
    }
    // Math's tower, at frame 0x1e. `WizardIntro` is what he says on the way in;
    // ringing the bell is one roll against thirty, seventy and ninety, and
    // leaving sets the grudge to seventy whatever it gave, so a second visit
    // the same day is dangerous and a third is close to certain.
    if let Some(wizard) = wizard {
        places.insert(
            "wizard".into(),
            serde_json::json!({
                "name": "Math the Wizard",
                // `_MAP:knmath` at `DS:0xc371`, `StackMessages[0x1e - 0x15]`.
                "line": "Visit Math the Wizard",
                "scene": "scene.wi1",
                "x": wizard.0, "y": wizard.1, "w": wizard.2, "h": wizard.3,
                "menu": [6, 6, 128, 40],
                "text": [4, 112, 312, 76],
                "intro": "As you ring the bell at the bottom of the foreboding wizard's tower, a sense of unease rises in the air. A tall, dark figure slowly rises onto the balcony some fifty feet above your head and with a low, powerful breath, the mighty wizard Math speaks:",
                "options": [
                    { "label": "Visit Math the Wizard", "effect": { "do": "wizard" } },
                    { "label": "Leave",                 "effect": leave }
                ]
            }),
        );
    }

    // The Valley of the Gods, which is what four keys are for.
    //
    // **The label is recovered**: `_MAP:knvalley` is `Enter Valley of the
    // Gods`, and what the gate says when it is shut is `NoKeysMessage`, whose
    // three lines are `henge_core::quest::NO_KEYS`. **The Guardian is
    // recovered too**: `MOON:FightDemon` calls `InitKnightvsDemon`, which sets
    // 250 health, one monster and `ColourBackDrop` 4. **The ground is the
    // wastes'**: the demon's loader at image 0x8bfa opens with `LoadWasteBack`
    // (0x8d89), which loads `WAB1.CMP` and nothing else, and `BlueDemon`'s
    // ground words are `WasteCOLOUR`'s browns with blues where its greens
    // were, so over any other backdrop the demon's palette is wrong. The
    // loader reads no `.T` layout at all, so the original fights it on the
    // bare backdrop; here the arena is left empty and the waste's own
    // rotation picks one, which is scenery the original does not draw.
    //
    // **Recovered too, now**: where it stands. Frame 0x1c of `MapIconsTABLE`
    // puts it on the green ring in the mountains that the map picture already
    // draws, which is the artwork this project used to call Stonehenge. The
    // original blits no icon over it, since only `DisplayLairs` blits anything
    // on the map, so neither does this.
    //
    // **Ours**: the picture behind the gate, which is the intro plate of a
    // portal standing open on an altar. MOON names no picture for the Valley;
    // the only one it names at all is `bg8.piv`, and that is the ending's.
    if let Some((x, y, w, h)) = valley {
        places.insert(
            "valley".into(),
            serde_json::json!({
                "name": "Valley of the Gods",
                // `_MAP:knvalley` at `DS:0xc387`, `StackMessages[0x1c - 0x15]`.
                "line": "Enter Valley of the Gods",
                "scene": "scene.bg4",
                "x": x, "y": y, "w": w, "h": h,
                "menu": [8, 12, 168, 40],
                "text": [6, 146, 308, 42],
                "options": [
                    {
                        "label": "Enter Valley of the Gods",
                        "effect": {
                            "do": "valley",
                            "arena": "",
                            "family": "waste",
                            "guardian": "demon",
                            "count": 1
                        }
                    },
                    { "label": "Leave", "effect": leave }
                ]
            }),
        );
    }

    // And the lairs, straight out of `lair_table`: the guardian, the head
    // count, the corner and the ground, all four the original's. The scene
    // behind the menu is this project's, one backdrop to a family.
    //
    // `count` is `TotalMonsters`, which the original feeds into the arena in
    // waves. This project fields what a bout seats and no more, so a lair of
    // fourteen ratmen puts three of them in front of you; the number is
    // carried through as it stands rather than rounded down here, because
    // rounding it down would throw away the recovered value.
    let (w, h) = (LAIR_W, LAIR_H);
    for (index, lair) in lairs.iter().enumerate() {
        let family = family_of(lair.code);
        let n = index % 6;
        let scene = match family {
            "forest" => "scene.fob1",
            "waste" => "scene.wab1",
            "swamp" => "scene.swb1",
            _ => "scene.glb1",
        };
        places.insert(
            format!("lair.{family}.{}", n + 1),
            serde_json::json!({
                "name": "Lair",
                // `_MAP:knlair` at `DS:0xc3ae`, which `OrderOpt` takes for
                // kind 2 without going through `StackMessages` at all.
                "line": "Enter Lair",
                "scene": scene,
                "x": lair.x, "y": lair.y, "w": w, "h": h,
                "icon": 0x14,
                "menu": [8, 100, 160, 40],
                "text": [6, 146, 308, 42],
                "options": [
                    {
                        "label": "Enter Lair",
                        "effect": {
                            "do": "raid",
                            "lair": index,
                            "arena": lair_arena(family, n),
                            "family": family,
                            "guardian": lair.guardian,
                            "count": lair.count
                        }
                    },
                    { "label": "Leave", "effect": leave }
                ]
            }),
        );
    }

    serde_json::Value::Object(places).to_string()
}

/// What there is to carry, and what a stall asks for it.
///
/// Every item here is the original's. A flask and a draught of this project's
/// own invention used to sit in front of the ten, chosen against each other
/// rather than read out of anything; they are gone.
///
/// **The ten magic items are recovered**, names, prices and what they do.
/// `MagicName` (DS:`0xe38d`) pairs each slot of a knight's magic record with
/// its name, `pu9`..`pu17` are the merchant's own lines with the price in
/// the text, and `MagicCast` in `_STATUS` is a chain of `cmp bx, slot` that
/// says what each does; the three worn ones are read by the derivation
/// routine at 0x28d and by `CalcDamage`. The two that act on the dragon are
/// the dragon's: the talisman stays `inert` because it is worn by being
/// carried, `TalismanWrym` (0x43f4) reading the count in the magic record
/// and shifting the dragon's blow right that many times, floored at five,
/// which `henge_core::monster::talisman_wrym` does; and the Scroll of the
/// Wyrm is `wyrm`, `MagicCast` slot 0x10 (0xcb60) and `StatusDone` (0xbe57),
/// which set the dragon over the map after the knight picked. The prices
/// `se*` sells them back for are half.
///
/// **The ids of the ten are the ones `henge_core::service::magic_item` names**,
/// because every bestowal in the game goes through that table: the wizard's
/// gift, what is on a lair's floor, and what the temple will buy back. A pack
/// that files one of them under another id simply never has it handed out, so
/// the two have to agree and the engine's side is the one that cannot move.
///
/// The four keys are here so that one carried out of a lair has a name to be
/// listed under. They are `moon::Key::item` ids, they carry no price, and
/// `moon::is_token` keeps them off every counter in the game.
///
/// It lands in the reference pack for now because it is authored alongside the
/// places that sell it, and those carry the original's own town art.
fn item_definitions() -> String {
    serde_json::json!({
        // The original's own magic, slot by slot.
        "potion": {
            "name": "Potion of healing", "price": 20, "consumed": true,
            "virtue": { "does": "restore" }
        },
        "gem_of_seeing": {
            "name": "Gem of seeing", "price": 32, "consumed": true,
            "virtue": { "does": "sight", "astray": 0, "returns": true }
        },
        "ring_of_protection": {
            "name": "Ring of protection", "price": 50, "consumed": false,
            "virtue": { "does": "ward", "health": 20 }
        },
        "talisman_of_the_wyrm": {
            "name": "Talisman of the Wyrm", "price": 52, "consumed": false,
            "virtue": { "does": "inert" }
        },
        "scroll_of_haste": {
            "name": "Scroll of Haste", "price": 36, "consumed": true,
            "virtue": { "does": "haste" }
        },
        "scroll_of_acquisition": {
            "name": "Scroll of Aquisition", "price": 52, "consumed": true,
            "virtue": { "does": "seize" }
        },
        "scroll_of_the_hawk": {
            "name": "Scroll of the Hawk", "price": 52, "consumed": true,
            "virtue": { "does": "sight", "astray": 16, "returns": false }
        },
        "scroll_of_the_wyrm": {
            "name": "Scroll of the Wyrm", "price": 40, "consumed": true,
            "virtue": { "does": "wyrm" }
        },
        "scroll_of_protection": {
            "name": "Scroll of Protection", "price": 24, "consumed": true,
            "virtue": { "does": "protection", "backfire": 11 }
        },
        // The four lair keys, one hidden in each family's six. `MOON:Valley`
        // wants all four bits of `+0x14` set; a pack that carries items by id
        // needs four ids, and these are `moon::Key::item`'s.
        //
        // **The price is recovered and no counter in henge will take it.**
        // `_STATUS` has `Buy Key for 12 GP` and `Sell Key for 6 GP`, and the
        // half is `GoldSell`'s own `shr ax, 1`, which is a check on both. But
        // that page is a trade between two knights (`BuyMoonstone` ORs the bit
        // into the buyer's `+0x14` and XORs it out of the seller's), not a
        // shop, and henge has one knight to a run, so `moon::is_token` still
        // refuses to sell one. The number is here because it is the
        // original's, and because a second player at the same map will want
        // it.
        "key.forest": {
            "name": "Key of the forest", "price": 12, "consumed": false,
            "virtue": { "does": "inert" }
        },
        "key.waste": {
            "name": "Key of the wastes", "price": 12, "consumed": false,
            "virtue": { "does": "inert" }
        },
        "key.swamp": {
            "name": "Key of the marsh", "price": 12, "consumed": false,
            "virtue": { "does": "inert" }
        },
        "key.glade": {
            "name": "Key of the glades", "price": 12, "consumed": false,
            "virtue": { "does": "inert" }
        },

        // The four moonstones, which is what the Valley of the Gods pays for
        // four keys: `MOON:Valley` sets one bit of `+0x16` as `1 << (rnd & 3)`
        // and `MOON:Henge` ends the game for whoever stands in the circle with
        // the one whose night it is.
        //
        // **Three of the four names are recovered**, from the status panel's
        // own list beside `Key to the Valley`: `New moon Moonstone`,
        // `Full Moonstone` and `Half Moonstone`. There are four bits and three
        // names, so `Gibbous Moonstone` is ours; see `moon::Moonstone`, where
        // the two routines that pair a bit with a phase also disagree with
        // each other. The price is `Buy Moonstone for 20 GP` and
        // `Sell Moonstone for 10 GP`, and is refused here for the same reason
        // the keys' is.
        "moonstone.new": {
            "name": "New moon Moonstone", "price": 20, "consumed": false,
            "virtue": { "does": "inert" }
        },
        "moonstone.full": {
            "name": "Full Moonstone", "price": 20, "consumed": false,
            "virtue": { "does": "inert" }
        },
        "moonstone.half": {
            "name": "Half Moonstone", "price": 20, "consumed": false,
            "virtue": { "does": "inert" }
        },
        "moonstone.gibbous": {
            "name": "Gibbous Moonstone", "price": 20, "consumed": false,
            "virtue": { "does": "inert" }
        },

        // Swords and armour, and these *are* recovered.
        //
        // The names are the strings `_STATUS` prints beside them: `ab9`..`ab17`.
        // The prices are the merchant's own lines, `pu1`..`pu5`, which carry the
        // number in the text. What each is worth in a fight comes from the code:
        // `CalcDamage` adds two, three and five for the three better blades and
        // nothing for the long sword, and the derivation routine at 0x28d adds
        // ten, twenty and thirty health for the three better suits and nothing
        // for padded. Only chain mail and battle armour add to the stride, which
        // looks like an oversight in the original and is kept because it is what
        // the original does.
        //
        // The merchant's panel sells three of the suits and two of the
        // blades through `BuyArmour` and `BuyWeapon`, whose prices are the
        // immediates in those routines (0xcd50, 0xcd73, 0xcd96, 0xcdb9,
        // 0xcddb) and agree with these; the dagger is `BuyDagger`'s two.
        "dagger": {
            "name": "Dagger", "price": 2, "consumed": false,
            "virtue": { "does": "weapon", "damage": 0 }
        },
        "long_sword": {
            "name": "Long sword", "price": 0, "consumed": false,
            "virtue": { "does": "weapon", "damage": 0 }
        },
        "broad_sword": {
            "name": "Broad sword", "price": 10, "consumed": false,
            "virtue": { "does": "weapon", "damage": 2 }
        },
        "claymore": {
            "name": "Claymore sword", "price": 25, "consumed": false,
            "virtue": { "does": "weapon", "damage": 3 }
        },
        "sword_of_sharpness": {
            "name": "Sword of Sharpness", "price": 100, "consumed": false,
            "virtue": { "does": "weapon", "damage": 5 }
        },
        "padded_armour": {
            "name": "Padded armour", "price": 0, "consumed": false,
            "virtue": { "does": "armour", "health": 0, "stride": 0 }
        },
        "chain_mail": {
            "name": "Chain mail", "price": 30, "consumed": false,
            "virtue": { "does": "armour", "health": 10, "stride": 2 }
        },
        "plate_armour": {
            "name": "Plate armour", "price": 50, "consumed": false,
            "virtue": { "does": "armour", "health": 20, "stride": 0 }
        },
        "battle_armour": {
            "name": "Battle armour", "price": 75, "consumed": false,
            "virtue": { "does": "armour", "health": 30, "stride": 2 }
        }
    })
    .to_string()
}

/// The four knights.
///
/// **Recovered, and now including the names.** `InitKnights` walks the four
/// player records and gives each one a name buffer and a starting square on the
/// map, branching on the knight's own colour index at `+0x20`:
///
/// ```text
/// index 0  BNAME   (10, 10)     index 1  GNAME   (300, 5)
/// index 2  ENAME   (26, 180)    index 3  RNAME   (300, 185)
/// ```
///
/// One corner each. `KnightGlowColours` branches on the same index and gives
/// the colours those initials stand for, as three 12-bit shades apiece: blue,
/// gold, emerald, red. The fifth entry in that routine, a dark purple, is the
/// one every computer knight wears. Those are the shades a knight glows
/// towards when nearly dead; the shades his armour is actually painted in
/// are `ColourKnight`'s, and they are in `battle_palette` below with the
/// glow triples beside them.
///
/// **The names themselves are read out of the bottom of DGROUP**, which the
/// unpacker used to leave stale and now does not (`docs/REVERSING.md`):
///
/// ```text
/// image 0x128ca  BNAME  SIR_GODBER    DS:0x530
/// image 0x128e0  GNAME  SIR_RICHARD   DS:0x51a
/// image 0x128f6  ENAME  SIR_JEFFREY   DS:0x546
/// image 0x1290c  RNAME  SIR_EDWARD    DS:0x55c
/// ```
///
/// Each is a 21-byte buffer padded with spaces, because `TypeName` lets a
/// player type over it. Which name belongs to which knight is not a guess:
/// `InitKnights` above pairs the pointer with the index, and `ChooseFIRE` pairs
/// the same four pointers with the same four indices a second time, writing
/// `NAMEy` and `+0x20` together on each branch. The initial is the colour's,
/// not the name's: B is blue, G gold, E emerald, R red.
///
/// **The underscore is drawn as a space.** `TextASCII` at `DS:0x8006` maps a
/// character to a glyph by `char - 0x20`, and `'_'` and `' '` both map to glyph
/// 69, which is the blank. The font has no underscore at all. The original
/// stores one because `TypeName` scans the buffer for the first *space* to find
/// where typing starts, so an underscore keeps the whole default name editable
/// while the screen still reads `SIR GODBER`. Baked with the space.
///
/// **`SIR BANNER`, `SIR DWAIN`, `SIR BALAIN` and `SIR GUNTHER` are not these
/// four.** They are `Enemy1Name`..`Enemy4Name` at image 0x18cd2, and
/// `InitGameStart` hands them to the four knight records before anybody
/// chooses, with colour index 4, the dark purple, and the corners (15, 100),
/// (300, 100), (160, 20) and (160, 180). A seat a person takes is overwritten
/// by `ChooseFIRE` and `InitKnights`; a seat nobody takes keeps the enemy name
/// and rides the map on its own. Those four were what this project called the
/// player knights, which was wrong.
///
/// The stat block is `SetKnightEquipment`: one of each ability, five life
/// points, ten daggers, ten gold, a long sword and padded armour. It writes
/// ninety-nine into the health field and the routine at 0x28d overwrites it a
/// moment later, so the twenty a knight really starts with is left to that
/// arithmetic here as well.
///
/// **And the four do not differ.** `InitKnights` separates them by name, colour
/// and corner; every stat block it produces is the same. The shape allows four
/// different ones, because that is what the data ought to allow, but shipping
/// four different ones would be an invention dressed as a recovery.
fn knight_definitions() -> String {
    // 12-bit RGB, as the original stores it, widened by the usual nibble * 17.
    let knight = |name: &str, shades: [u32; 3], home: [i32; 2]| {
        let widen = |v: u32| {
            (((v >> 8) & 0xf) * 17) << 16 | (((v >> 4) & 0xf) * 17) << 8 | ((v & 0xf) * 17)
        };
        serde_json::json!({
            "name": name,
            "shades": shades.iter().map(|c| widen(*c)).collect::<Vec<u32>>(),
            "home": home,
            "strength": 1,
            "constitution": 1,
            "endurance": 1,
            "life": 5,
            "daggers": 10,
            "gold": 10,
            "weapon": "long_sword",
            "armour": "padded_armour"
        })
    };
    serde_json::json!([
        // `BNAME`, `GNAME`, `ENAME`, `RNAME`, in the colour index order
        // `InitKnights` and `ChooseFIRE` both branch on. The underscore each
        // one carries is the blank glyph, so it is written as a space.
        knight("SIR GODBER", [0x00c, 0x009, 0x006], [10, 10]),
        knight("SIR RICHARD", [0xfa0, 0xe70, 0xc50], [300, 5]),
        knight("SIR JEFFREY", [0xae8, 0x6b5, 0x473], [26, 180]),
        knight("SIR EDWARD", [0xd00, 0x900, 0x500], [300, 185]),
    ])
    .to_string()
}

/// Which tune plays where.
///
/// **Recovered.** `LOADMUSIC` is at image `0x900d` and takes the tune number in
/// `ax`; `MusicTable` at DS `0x84f0` is eighteen four byte records, six tunes by
/// three sound cards, and the second word of each is the disk it lives on.
/// Five places call it, and each is followed by `mov ah, 0; int 60h`:
///
/// | caller | `ax` | tune |
/// |---|---|---|
/// | the tavern, image 0xb012 (the routine at 0xb007, `TAV`) | 2 | `tune3`, stopped by `LeaveTavern` |
/// | the routine that opens the henge, before `_TAVERN:HengeLOOP` | 1 | `tune2` |
/// | `_WIZARD:LoadWizard` | 1 | `tune2`, stopped at `e5$` |
/// | the healer, image 0xba66 (`HEAL`), `mov ax, 3` at 0xba66 | 3 | `tune4`, stopped by `MysticFini` |
/// | the mystic, image 0xb935 (`MYST`), `mov ax, 4` at 0xb935 | 4 | `tune5`, stopped by `MysticFini` |
///
/// The last two were read here as `MysticUpDown+39` and `_bestow_done+7`,
/// the nearest names, and the fifth was taken for a moment inside the
/// wizard's tower: `WDLOOP`'s `WHEA` rung (0xe02) calls 0xba66 and its `MYST`
/// rung (0xdd8) calls 0xb935, so 0xba66 is the healer and 0xb935 the mystic,
/// and each has a tune. The three are keyed by door, since none of the
/// three is a place of its own.
/// Tunes 1 and 6 are never loaded by `MAIN.EXE`: they ship on disk A with the
/// intro and belong to `INTR.EXE`, whose own start call could not be traced to
/// a tune number. **The intro is given tune 1 by elimination**, which is
/// inference and not recovery: disk A holds exactly the intro, the ending and
/// those two tunes, and the intro comes first. The ending's tune 6 waits with
/// the rest of the ending sequence.
fn music_places() -> String {
    serde_json::json!({
        "door.tavern": "music.tune3",
        "stones": "music.tune2",
        "wizard": "music.tune2",
        "door.healer": "music.tune4",
        "door.mystic": "music.tune5",
        "intro": "music.tune1"
    })
    .to_string()
}

/// The recovered tunes, split into one score per asset.
///
/// `research/tunes.json` is what `tools/tunes.py` writes: the note stream of
/// each `RTUNEn.BIN`, taken by running the game's own MPU-401 driver under
/// emulation. Absent is not an error, the same way a missing symbol table is
/// not: the game plays without music and says so.
fn bake_music(out: &Path, m: &mut Manifest) -> anyhow::Result<usize> {
    let path = Path::new("research/tunes.json");
    if !path.exists() {
        return Ok(0);
    }
    let text = fs::read_to_string(path).context("reading research/tunes.json")?;
    let tunes: henge_audio::music::Tunes =
        serde_json::from_str(&text).context("research/tunes.json is not a tune file")?;
    fs::create_dir_all(out.join("music"))?;
    let mut n = 0;
    for (name, score) in tunes.split() {
        let file = format!("music/{name}.json");
        fs::write(out.join(&file), serde_json::to_string(&score)?)?;
        m.music.insert(format!("music.{name}"), file);
        n += 1;
    }
    Ok(n)
}

/// `BattlePal`: the colours a bout writes over its backdrop's palette.
///
/// **Recovered, all of it.** The original does not recolour a knight by
/// substituting pixels; it rewrites palette entries, and every fighter's
/// artwork is painted against fixed indices. `MOON:ColourBackDrop` (image
/// 0x460f) copies the backdrop picture's thirty two words from `DS:0x80bb`,
/// where the picture loader at 0x875e leaves them, into `BattlePal`
/// (`DS:0x7a80`), stores the creature code in `COLOURS` and dispatches on it
/// with `si = BattlePal + 18`, entry 9. Each `InitKnightvs*` routine ends by
/// jumping there with the code in `ax`: beast 0, mudmen 2, demon 4, a second
/// knight 6, dragon 0xa, trogg with axe or hammer 0xc, trogg with spear
/// 0x10, ratmen 0x12, balok 0x18, troll 0x20. The routines it lands on:
///
/// ```text
/// 0x466c ColourBeast      9..15  776 443 731 520 300 09a c00
/// 0x4691 Colour2ndKnight  9..11  ColourKnight on the second knight's record
/// 0x469b ColourRatmen     9..15  653 942 720 500 a96 875 c00
/// 0x46c0 ColourTroggAxe   9..15  on the wastes  025 004 001 830 400 f80 c00
///                                on the moors   104 102 000 600 300 693 c00
///                                anywhere else  500 200 000 b40 610 895 c00
/// 0x473d ColourBalok      9..15  f96 c63 930 842 521 f63 c00
/// 0x4762 ColourTroll      9..14  55a 347 123 001 f00 800
/// 0x4781 ColourDemon      9..31  twenty three words copied from BlueDemon, DS:0x7992
/// 0x478d ColourMudmen     9..15  332 ccb b81 851 630 f52 900
/// 0x47b1 ColourDragon     9..15  c00 976 700 500 754 c30 a00, then 29..31 fc0 f80 c50
/// ```
///
/// All of them fall into `ColourMainKnight` (0x47e4), which fades out, sets
/// `si = BattlePal + 12` and calls `ColourKnight` (0x480e) on the main
/// knight's record. That writes entries 6, 7 and 8 by the colour index at
/// `+0x20`: `00a 007 004` for 0, `f80 c50 a30` for 1, `8c6 593 251` for 2,
/// `f22 b22 700` for 3, and `206 103 001` on the last branch, which is not
/// guarded, so it is what every other index gets; the computer's knights carry
/// 4. `KnightGlowColours` (0x9a5) is the same shape and gives the brighter
/// triple each knight pulses towards below ten health: `00c 009 006`,
/// `fa0 e70 c50`, `ae8 6b5 473`, `d00 900 500`, `408 305 003`.
///
/// Then `ColourBackdrop` (0x4879), lower case d, writes the ground. It returns
/// at once when `COLOURS` is 4, the demon, and otherwise branches on the
/// landscape code at `DS:0x694e`: 0 copies `PlainsCOLOUR` to entries 16 to 28,
/// 2 copies `ForestCOLOUR`, 4 writes `ffd 998 776 443` into 1 to 4 and copies
/// `SwampCOLOUR`, 6 writes the same four and copies `WasteCOLOUR`. The four
/// tables are thirteen words each at `DS:0x78d6`, `0x78f0`, `0x7924` and
/// `0x790a`, and they are read out of the image here rather than retyped.
/// The landscape codes are the families in the order `GENERATELANDSCAPE`
/// dispatches them, so 0 is the moors, which this project files as the glade.
/// Last, entry 0 is made black and, unless `COLOURS` is the dragon's 0xa,
/// entry 15 is made `c00`, which is why blood is the same red in every arena.
///
/// The second knight is not a recolour either. `InitKnightvsKnight` loads
/// `HE1.OB`, `HE2.OB` and `HE3.OB` (0x89cd) into the creature table in place
/// of `KN1` to `KN3`, and those banks are the knight painted in 9, 10 and 11
/// instead of 6, 7 and 8. The pack's `hero` bank table is that table.
///
/// Every backdrop picture in the release already carries its own family's
/// ground table at 16 to 28 and the four greys at 1 to 4, so the ground
/// writes change nothing on the original's pictures; they are here because
/// they are what the code does, and a pack with its own backdrops gets them.
fn battle_palette(src: &str) -> anyhow::Result<String> {
    let Some(bytes) = unpacked_image(src) else {
        anyhow::bail!("the battle palette is read out of MAIN.EXE, and there is no image");
    };
    let words = |at: usize, n: usize| -> Vec<u16> {
        (0..n)
            .map(|i| u16::from_le_bytes([bytes[at + i * 2], bytes[at + i * 2 + 1]]))
            .collect()
    };
    const DGROUP: usize = 0x123b0;
    let ground_table = |ds: usize| words(DGROUP + ds, 13);
    let plains = ground_table(0x78d6);
    let forest = ground_table(0x78f0);
    let waste = ground_table(0x790a);
    let swamp = ground_table(0x7924);
    let blue_demon = words(DGROUP + 0x7992, 23);
    for (name, table) in [
        ("PlainsCOLOUR", &plains),
        ("ForestCOLOUR", &forest),
        ("WasteCOLOUR", &waste),
        ("SwampCOLOUR", &swamp),
        ("BlueDemon", &blue_demon),
    ] {
        anyhow::ensure!(
            table.iter().all(|w| *w <= 0x0fff) && table.iter().any(|w| *w != 0),
            "{name} does not read as twelve bit colour words: the image is not the one the \
             symbol table describes"
        );
    }
    // Two words checked by content so a wrong image fails here and not on screen:
    // `ForestCOLOUR` opens `210 321` and `WasteCOLOUR` opens `322 432`.
    anyhow::ensure!(
        forest[..2] == [0x210, 0x321] && waste[..2] == [0x322, 0x432],
        "ForestCOLOUR or WasteCOLOUR is not where the symbol table says"
    );
    let write = |at: u8, words: &[u16]| serde_json::json!({ "at": at, "words": words });
    let greys = [0xffdu16, 0x998, 0x776, 0x443];
    let block = |words: &[u16]| serde_json::json!({ "writes": [write(9, words)] });
    Ok(serde_json::json!({
        "knights": [
            [0x00a, 0x007, 0x004],
            [0xf80, 0xc50, 0xa30],
            [0x8c6, 0x593, 0x251],
            [0xf22, 0xb22, 0x700],
            [0x206, 0x103, 0x001],
        ],
        "glow": [
            [0x00c, 0x009, 0x006],
            [0xfa0, 0xe70, 0xc50],
            [0xae8, 0x6b5, 0x473],
            [0xd00, 0x900, 0x500],
            [0x408, 0x305, 0x003],
        ],
        "ground": {
            "glade":  [write(16, &plains)],
            "forest": [write(16, &forest)],
            "swamp":  [write(1, &greys), write(16, &swamp)],
            "waste":  [write(1, &greys), write(16, &waste)],
        },
        "creatures": {
            "beast":  block(&[0x776, 0x443, 0x731, 0x520, 0x300, 0x09a, 0xc00]),
            "ratmen": block(&[0x653, 0x942, 0x720, 0x500, 0xa96, 0x875, 0xc00]),
            // `ColourTroggAxe` serves all three troggs: axe and hammer come in
            // as 0xc, the spear as 0x10, and both codes land on it.
            "trogg_axe": {
                "writes": [write(9, &[0x500, 0x200, 0x000, 0xb40, 0x610, 0x895, 0xc00])],
                "by_family": {
                    "waste": [write(9, &[0x025, 0x004, 0x001, 0x830, 0x400, 0xf80, 0xc00])],
                    "glade": [write(9, &[0x104, 0x102, 0x000, 0x600, 0x300, 0x693, 0xc00])],
                },
            },
            "trogg_hammer": {
                "writes": [write(9, &[0x500, 0x200, 0x000, 0xb40, 0x610, 0x895, 0xc00])],
                "by_family": {
                    "waste": [write(9, &[0x025, 0x004, 0x001, 0x830, 0x400, 0xf80, 0xc00])],
                    "glade": [write(9, &[0x104, 0x102, 0x000, 0x600, 0x300, 0x693, 0xc00])],
                },
            },
            "trogg_spear": {
                "writes": [write(9, &[0x500, 0x200, 0x000, 0xb40, 0x610, 0x895, 0xc00])],
                "by_family": {
                    "waste": [write(9, &[0x025, 0x004, 0x001, 0x830, 0x400, 0xf80, 0xc00])],
                    "glade": [write(9, &[0x104, 0x102, 0x000, 0x600, 0x300, 0x693, 0xc00])],
                },
            },
            "balok":  block(&[0xf96, 0xc63, 0x930, 0x842, 0x521, 0xf63, 0xc00]),
            "troll":  block(&[0x55a, 0x347, 0x123, 0x001, 0xf00, 0x800]),
            "demon":  { "writes": [write(9, &blue_demon)], "keeps_ground": true },
            "mudmen": block(&[0x332, 0xccb, 0xb81, 0x851, 0x630, 0xf52, 0x900]),
            "dragon": {
                "writes": [
                    write(9, &[0xc00, 0x976, 0x700, 0x500, 0x754, 0xc30, 0xa00]),
                    write(29, &[0xfc0, 0xf80, 0xc50]),
                ],
                "keeps_blood": true,
            },
        },
    })
    .to_string())
}

/// Which palette entries move on which screen.
///
/// **Recovered.** The original installs exactly two things through
/// `COLOURCYCLE` and `COLOURGLOW` from a named screen, and both are here:
///
/// * `_MAP:MapEffects` calls `COLOURGLOW(0x1f, 0x0ff, 1, 0)` and then
///   `COLOURCYCLE(0x15, 0x17, 1, 0x0c)`, keeping the handles in `GemXY+4` and
///   `RiverHANDLE`. So on the overworld the last palette entry breathes towards
///   a bright cyan every frame, and entries 21 to 23 rotate upwards every
///   twelfth frame. The symbol says what those three are: the river.
/// * `MOON:ChooseKnight` loads `SelectPAL` and calls
///   `COLOURGLOW(0x0f, 0x088, 1, 0)`, so entry fifteen breathes towards a teal
///   on the character select screen. The call is read straight off `0x158e`:
///   index 15, target 0x088, period 1, repeat 0, and repeat 0 is the original's
///   forever, because `COLCON` swaps the glow's two ends when it arrives.
///
///   **What breathes is one sprite, not the screen.** `SEL.CEL` cel 1 is a
///   64 by 76 hollow frame drawn entirely in index 15 and in no other index,
///   and `ChooseRefresh` blits it round whichever portrait is chosen; none of
///   the four portraits touches 15, and the screen behind them is cleared to
///   entry 0. So on this screen entry 15 belongs to the highlight frame alone,
///   and glowing it glows the highlight and nothing else.
///
///   This was taken out once, for a real reason that has since gone away: the
///   screen was being drawn over `CH.PIV`, whose entry 15 is the night sky, so
///   the recovered glow repainted the whole background twice a second. The
///   backdrop was the invention, not the glow. `SelectPAL` is readable now
///   (`select_palette` above), it puts `0x066` at entry 15, and the glow walks
///   that to `0x088` and back.
///
/// The other two glows in the game hang off the fight rather than the screen,
/// and both are wired in `main.rs`: `MudmenGlowOn` (entry 14) and
/// `KnightGlowOn`, which is entries 6, 7 and 8 for a knight down to ten
/// health, and 9 to 11 for a second knight, walking towards the `glow`
/// triples in `battle_palette` below. Neither is keyed by screen, so neither
/// is in this table.
fn palette_effects() -> String {
    serde_json::json!({
        "map": {
            "cycles": [{ "first": 0x15, "last": 0x17, "up": true, "period": 12 }],
            "glows":  [{ "index": 0x1f, "target": 0x0ff, "period": 1, "repeat": 0 }]
        },
        // `MOON:ChooseKnight`, `0x158e`: `ax = 0x0f`, `bx = 0x088`, `cx = 1`,
        // `dx = 0`. On this screen entry 15 is the highlight frame and nothing
        // else, so this is the frame breathing.
        "select": { "glows": [{ "index": 0x0f, "target": 0x088, "period": 1, "repeat": 0 }] }
    })
    .to_string()
}

// ------------------------------------------------------------------- the intro

/// The publisher's logo, the opening panorama and the cast.
///
/// **`MINDSCAP` has no extension**, which is the only reason it was never
/// baked: the loop that turns full-screen images into sheets asks for `.piv`,
/// `.cmp` and `.p`. It is an ordinary five-plane PIV.
///
/// **`INTRO.STI` is a tile map.** `FindTile` cuts tile *n* out of a 320x200
/// sheet at `((n % 10) * 32, (n / 10) * 25)` and the routine that walks the map
/// reads ten big-endian words to a row, dividing each by 80 to pick which of
/// three loaded sheets it comes from. `panfile1..3` are `bg1a`, `bg1c` and
/// `bg1b`, and the 960 bytes are 48 rows: one 320 by 1200 panorama, which the
/// intro pans a 200-tall window down. Only `bg1a`'s palette is copied to the
/// live one, so all three sheets are composited in it, as the original does.
///
/// **The cast** is the intro's own animation scripts, flattened to frames.
fn bake_intro(lib: &Library, out: &std::path::Path, m: &mut Manifest) -> anyhow::Result<String> {
    use henge_formats::introexe;
    let mut done: Vec<String> = Vec::new();

    if let Ok(bytes) = lib.bytes("mindscap") {
        let p = piv::Piv::parse(&bytes)?;
        let file = "sheets/scene_mindscap.png".to_string();
        write_indexed(&out.join(&file), piv::W, piv::H, &p.pixels, &p.palette)?;
        m.sheets.insert(
            "scene.mindscap".into(),
            Sheet {
                file,
                frames: vec![FrameRect {
                    x: 0,
                    y: 0,
                    w: piv::W as u32,
                    h: piv::H as u32,
                    ox: 0,
                    oy: 0,
                }],
            },
        );
        m.palettes
            .insert("palette.scene.mindscap".into(), p.palette);
        done.push("the Mindscape logo".into());
    }

    if let Ok(sti) = lib.bytes("intro.sti") {
        let a = lib.piv("bg1a.piv")?;
        let c = lib.piv("bg1c.piv")?;
        let b = lib.piv("bg1b.piv")?;
        let pan = introexe::panorama(&sti, &[&a, &c, &b])?;
        anyhow::ensure!(
            pan.width == henge_core::intro::PAN_W as usize
                && pan.height == henge_core::intro::PAN_H as usize,
            "the panorama came out {}x{}, not the 320x1200 the map describes",
            pan.width,
            pan.height
        );
        let file = "sheets/scene_intropan.png".to_string();
        write_indexed(
            &out.join(&file),
            pan.width,
            pan.height,
            &pan.pixels,
            &a.palette,
        )?;
        m.sheets.insert(
            "scene.intropan".into(),
            Sheet {
                file,
                frames: vec![FrameRect {
                    x: 0,
                    y: 0,
                    w: pan.width as u32,
                    h: pan.height as u32,
                    ox: 0,
                    oy: 0,
                }],
            },
        );
        m.palettes
            .insert("palette.scene.intropan".into(), a.palette);
        done.push(format!("a {}x{} panorama", pan.width, pan.height));
    }

    // **`CO.STI` is the ending's own panorama**, read exactly the way
    // `INTRO.STI` is. `0xd8c` puts the three sheet segments in the tile
    // engine's table as `[0x444b]`, `[0x4457]` and `[0x4459]`, which the
    // ending's loader filled with `bg7`, `bg8` and `bg2a`; every tile in the
    // file is under eighty, so only `bg7`'s are actually used. Its bottom eight
    // rows are `bg7` whole and the thirty nine above them are one repeated sky
    // tile with three stars in it, which is the sky the camera rises into.
    if let Ok(sti) = lib.bytes("co.sti") {
        let bg7 = lib.piv("bg7.piv")?;
        let bg8 = lib.piv("bg8.piv")?;
        let bg2a = lib.piv("bg2a.piv")?;
        let pan = introexe::panorama(&sti, &[&bg7, &bg8, &bg2a])?;
        anyhow::ensure!(
            pan.width == henge_core::intro::PAN_W as usize
                && pan.height == henge_core::intro::PAN_H as usize,
            "the ending's panorama came out {}x{}, not 320x1200",
            pan.width,
            pan.height
        );
        let file = "sheets/scene_copan.png".to_string();
        write_indexed(
            &out.join(&file),
            pan.width,
            pan.height,
            &pan.pixels,
            &bg7.palette,
        )?;
        m.sheets.insert(
            "scene.copan".into(),
            Sheet {
                file,
                frames: vec![FrameRect {
                    x: 0,
                    y: 0,
                    w: pan.width as u32,
                    h: pan.height as u32,
                    ox: 0,
                    oy: 0,
                }],
            },
        );
        done.push(format!(
            "the ending's {}x{} panorama",
            pan.width, pan.height
        ));
    }

    match intro_image() {
        Ok(Some(img)) => {
            let cast = introexe::cast(&img)?;
            let n = cast.scripts.len();
            fs::write(out.join("data/intro.json"), serde_json::to_string(&cast)?)?;
            m.data.insert("data.intro".into(), "data/intro.json".into());
            done.push(format!("{n} animation scripts"));
            // The ending's cast is the same image read with the ending's own
            // bank table, which is a different six files in a different order.
            let end = introexe::ending_cast(&img)?;
            let n = end.scripts.len();
            fs::write(out.join("data/ending.json"), serde_json::to_string(&end)?)?;
            m.data
                .insert("data.ending".into(), "data/ending.json".into());
            done.push(format!("{n} of the ending's"));
        }
        Ok(None) => done.push("no cast (no unpacked INTR.EXE image)".into()),
        Err(e) => done.push(format!("no cast ({e:#})")),
    }

    anyhow::ensure!(!done.is_empty(), "nothing of the intro could be baked");
    Ok(done.join(", "))
}

/// The expanded `INTR.EXE` image, if the unpacked one is to hand.
fn intro_image() -> anyhow::Result<Option<Vec<u8>>> {
    use henge_formats::introexe;
    let candidates = ["research/intro.final.bin"];
    let Some(raw) = candidates.iter().find_map(|p| fs::read(p).ok()) else {
        return Ok(None);
    };
    let img = introexe::expand(&raw)?;
    introexe::check(&img)?;
    Ok(Some(img))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The baked script set and bank tables, if a pack has been baked. Both
    /// are derived from the original and gitignored, so a checkout without
    /// them cannot run this; it says so rather than passing quietly.
    fn baked() -> Option<(ScriptSet, BTreeMap<String, BankTables>)> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/reference/data");
        let scripts = fs::read_to_string(root.join("scripts.json")).ok()?;
        let banks = fs::read_to_string(root.join("banks.json")).ok()?;
        Some((
            serde_json::from_str(&scripts).ok()?,
            serde_json::from_str(&banks).ok()?,
        ))
    }

    /// The controller tables, read out of the unpacked image beside the pack.
    fn recovered() -> Option<Tables> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../research");
        let img = fs::read(root.join("main.final.bin")).ok()?;
        let syms = fs::read_to_string(root.join("symbols.json")).ok()?;
        Tables::read(&img, &Symbols::parse(&syms).ok()?).ok()
    }

    /// The ground tables read out of the image are the ones the backdrop
    /// pictures were saved with: `ForestCOLOUR` is `FOB1.CMP`'s entries 16 to
    /// 28, and so on for each family, which corroborates both the addresses
    /// and the reading of `ColourBackdrop`. Every creature the bestiary
    /// fields has a block, and every knight index a triple, so no fighter is
    /// left in the backdrop's colours.
    #[test]
    fn the_battle_palette_agrees_with_the_backdrops() {
        use henge_core::battle_palette::{narrow, BattleColours};
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/reference");
        let Ok(text) = fs::read_to_string(root.join("data/battle-palette.json")) else {
            eprintln!("no baked pack under packs/reference: battle palette check skipped");
            return;
        };
        let colours: BattleColours = serde_json::from_str(&text).unwrap();
        let manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join("manifest.json")).unwrap()).unwrap();
        let palette = |id: &str| -> Vec<u16> {
            manifest["palettes"][id]
                .as_array()
                .unwrap_or_else(|| panic!("no palette {id}"))
                .iter()
                .map(|v| narrow(v.as_u64().unwrap() as u32))
                .collect()
        };
        for (family, _, _, backdrop, _) in ARENAS {
            let stem = backdrop.split('.').next().unwrap().to_lowercase();
            let pic = palette(&format!("palette.scene.{stem}"));
            let ground = &colours.ground[*family];
            let table = ground
                .iter()
                .find(|w| w.at == 16)
                .expect("thirteen words at 16");
            assert_eq!(table.words.len(), 13, "{family}");
            assert_eq!(
                &pic[16..29],
                &table.words[..],
                "{family}: {backdrop} was not saved with its ground table"
            );
            if let Some(greys) = ground.iter().find(|w| w.at == 1) {
                assert_eq!(&pic[1..5], &greys.words[..], "{family}: the four greys");
            }
        }
        assert_eq!(colours.knights.len(), 5);
        assert_eq!(colours.glow.len(), 5);
        assert_eq!(colours.knights[1], [0xf80, 0xc50, 0xa30], "the gold knight");
        for c in CREATURES {
            if c.id == "dragon_claw" {
                continue;
            }
            let block = colours
                .creatures
                .get(c.id)
                .unwrap_or_else(|| panic!("{}: no colour block", c.id));
            for family in ["glade", "forest", "swamp", "waste"] {
                let first = block
                    .on(family)
                    .first()
                    .unwrap_or_else(|| panic!("{}: nothing written", c.id));
                assert_eq!(first.at, 9, "{}: a creature begins at entry 9", c.id);
            }
        }
        // The demon's twenty three words are `WasteCOLOUR`'s browns with blues
        // for its greens, which is how it was known to be fought over `WAB1`.
        let demon = &colours.creatures["demon"].writes[0].words;
        let waste = &colours.ground["waste"]
            .iter()
            .find(|w| w.at == 16)
            .unwrap()
            .words;
        assert_eq!(&demon[7..12], &waste[..5]);
    }

    /// Every creature in the bestiary builds and validates against the real
    /// scripts and bank tables: every script its states name is in the set,
    /// every branch lands, and every part resolves to a cel on the table it
    /// is drawn through. A typo in `CREATURES` fails here by name.
    #[test]
    fn every_creature_in_the_bestiary_is_whole() {
        let (Some((scripts, banks)), Some(tables)) = (baked(), recovered()) else {
            eprintln!("no baked pack under packs/reference: bestiary check skipped");
            return;
        };
        assert!(
            scripts.len() >= 236,
            "the pack has {} scripts, expected the full 236",
            scripts.len()
        );
        let mut ids = Vec::new();
        for c in CREATURES {
            let def = creature_definition(c, &scripts, &banks, &tables)
                .unwrap_or_else(|e| panic!("{}: {e:#}", c.id));
            assert!(def.scripted(), "{}: not scripted", c.id);
            assert_eq!(
                def.bank_table, 2,
                "{}: creatures draw through table 2",
                c.id
            );
            assert!(def.health > 0 && def.damage > 0, "{}: no stat block", c.id);
            assert!(
                def.origin[1] < 0,
                "{}: the origin sits above the feet",
                c.id
            );
            ids.push(c.id);
        }
        for want in [
            "troll",
            "trogg_axe",
            "trogg_spear",
            "ratmen",
            "mudmen",
            "demon",
            "beast",
            "balok",
            "dragon",
        ] {
            assert!(ids.contains(&want), "the bestiary is missing {want}");
        }
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "an id is listed twice");
    }

    /// The `.T` header is a border list, and four layouts have more than one
    /// record in it.
    ///
    /// Reading it as a single rectangle put the placement walk eight or
    /// sixteen bytes out of step in exactly those four, which cost three of
    /// them most of their scenery and `SWL2` all of it. Any regression in the
    /// header parse shows up here as a short border list or an empty layout.
    #[test]
    fn the_arena_headers_carry_a_border_list_and_four_of_them_carry_more_than_one() {
        use henge_core::content::Arenas;
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/reference/data");
        let Ok(text) = fs::read_to_string(root.join("arenas.json")) else {
            eprintln!("no baked pack under packs/reference: arena header check skipped");
            return;
        };
        let arenas: Arenas = serde_json::from_str(&text).expect("arenas.json parses");
        assert_eq!(arenas.len(), 56, "the pack should hold 56 layouts");
        for (name, a) in &arenas {
            assert!(!a.terrain.borders.is_empty(), "{name}: no border list");
            let first = a.terrain.borders[0];
            assert_eq!(
                (first.left, first.right, first.top),
                (0, 319, 10),
                "{name}: the first record is the tree line across the whole screen"
            );
            assert!(
                (80..=159).contains(&first.bottom),
                "{name}: tree line at {}",
                first.bottom
            );
        }
        let counts = |n: &str| arenas[n].terrain.borders.len();
        assert_eq!(counts("fo7"), 2);
        assert_eq!(counts("sw6"), 2);
        assert_eq!(counts("swl2"), 2);
        assert_eq!(counts("gll4"), 3);
        for (name, a) in &arenas {
            if counts(name) == 1 {
                continue;
            }
            assert!(
                a.terrain.placements.len() > 30,
                "{name}: {} placements, so the walk went out of step again",
                a.terrain.placements.len()
            );
        }
        // `FO7`'s second record is the root mass in the middle of the screen.
        assert_eq!(
            (
                arenas["fo7"].terrain.borders[1].left,
                arenas["fo7"].terrain.borders[1].right,
                arenas["fo7"].terrain.borders[1].bottom
            ),
            (66, 164, 103)
        );
        // And the deepest of a layout's records is the row its fighters stand
        // below, which is what `FindHalfBORD` measures from.
        assert_eq!(arenas["fo7"].field().floor(), 103);
    }

    /// The placement selector byte, counted across the whole release.
    ///
    /// `Sholoop` (image `0x7ca9`) reads it as four cases and not two: 0xff ends
    /// the list, 0xfe draws nothing at all, 3 goes to `TileTable`, and anything
    /// else is forced to 4 and draws from `FO2`. This pins the census the walk
    /// has to cope with, so a parse that slid the placement list out of step,
    /// or a baker that quietly dropped the odd byte, shows up as a changed
    /// count rather than as scenery in the wrong place.
    #[test]
    fn the_placement_selectors_are_three_four_fe_and_six_ones() {
        use henge_core::content::Arenas;
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/reference/data");
        let Ok(text) = fs::read_to_string(root.join("arenas.json")) else {
            eprintln!("no baked pack under packs/reference: selector census skipped");
            return;
        };
        let arenas: Arenas = serde_json::from_str(&text).expect("arenas.json parses");
        let mut census: BTreeMap<u8, usize> = BTreeMap::new();
        for a in arenas.values() {
            for p in &a.terrain.placements {
                *census.entry(p.sheet).or_default() += 1;
            }
        }
        assert_eq!(
            census.get(&3).copied(),
            Some(3338),
            "the family's own sheet"
        );
        assert_eq!(census.get(&4).copied(), Some(1606), "the shared FO2 sheet");
        assert_eq!(
            census.get(&0xfe).copied(),
            Some(168),
            "and these draw nothing"
        );
        assert_eq!(census.get(&1).copied(), Some(6), "six records carry a 1");
        assert_eq!(
            census.len(),
            4,
            "no other selector is in the release: {census:?}"
        );
        // 0xff is the terminator and is never a record.
        assert!(!census.contains_key(&0xff));
        // Twenty eight of the fifty six layouts carry at least one 0xfe, which
        // is how many were wrong while they were being drawn.
        let with_fe = arenas
            .values()
            .filter(|a| a.terrain.placements.iter().any(|p| p.sheet == 0xfe))
            .count();
        assert_eq!(with_fe, 28);
    }

    /// Every creature runs one of the original's controllers, and every
    /// controller in the engine is one some creature runs. Item 37 is only
    /// done if nobody is left on the plain opponent by accident.
    #[test]
    fn every_creature_names_a_controller_the_engine_knows() {
        use henge_core::monster::Controller;
        let mut used: BTreeSet<&str> = BTreeSet::new();
        for c in CREATURES {
            let known = Controller::from_name(c.controller);
            assert!(
                known.is_some(),
                "{}: no controller called {}",
                c.id,
                c.controller
            );
            assert_ne!(
                known,
                Some(Controller::Knight),
                "{} is still fighting like a knight in a costume",
                c.id
            );
            used.insert(c.controller);
        }
        for want in [
            "trogg",
            "trogg_spear",
            "troll",
            "ratman",
            "mudman",
            "balok",
            "beast",
            "demon",
            "dragon",
            "claw",
        ] {
            assert!(used.contains(want), "nothing in the bestiary runs {want}");
        }
    }

    /// Every creature knows where it stands at the opening of a bout, and the
    /// records are the original's: `InitNewMO` (0x27ee) reads `[x][y][z][facing]`
    /// and the facing word is the original's 1 or 3.
    #[test]
    fn every_creature_carries_the_seat_table_init_new_mo_reads() {
        for c in CREATURES {
            assert!(!c.seats.is_empty(), "{}: no seat table", c.id);
            assert!(
                c.first_seat < c.seats.len(),
                "{}: first seat {} is past the end of a table of {}",
                c.id,
                c.first_seat,
                c.seats.len()
            );
            for [x, _y, _z, facing] in c.seats {
                assert!(
                    *facing == 1 || *facing == 3,
                    "{}: facing {facing} is not 1 or 3",
                    c.id
                );
                // Every creature but the demon and the dragon comes on from
                // beyond the screen's own columns, which is why the tables
                // hold numbers `CheckBorder` would never allow.
                assert!((-100..=400).contains(x), "{}: x {x} is nowhere", c.id);
            }
        }
        // `TroggTABLE`, DS:0x97a, word for word, and the troll shares it.
        let trogg = CREATURES.iter().find(|c| c.id == "trogg_axe").unwrap();
        assert_eq!(trogg.seats, TROGG_SEATS);
        assert_eq!(trogg.first_seat, 0);
        assert_eq!(
            TROGG_SEATS,
            &[
                [-50, 0, 100, 1],
                [360, 0, 150, 3],
                [340, 0, 50, 3],
                [-80, 0, 120, 1]
            ]
        );
        // `SetUpDKL` zeroes `SIDE` and `InitTrogg`/`InitMudmen` then xor it, so
        // these two open on record one and come in from the right.
        for id in ["troll", "mudmen"] {
            let c = CREATURES.iter().find(|c| c.id == id).unwrap();
            assert_eq!(c.first_seat, 1, "{id} should open on the second record");
            assert_eq!(c.seats[1][3], 3, "{id}'s second record faces left");
        }
        assert_eq!(
            CREATURES.iter().find(|c| c.id == "troll").unwrap().seats,
            TROGG_SEATS
        );
    }

    /// The counts every `InitKnightvs*` writes, and the `lev_adjust` row
    /// `AdjustLevel` moves them by. These are what decides how many creatures a
    /// lair puts in front of a player, so a wrong one is a wrong fight.
    #[test]
    fn every_creature_carries_the_counts_its_own_init_knightvs_writes() {
        for c in CREATURES {
            let w = &c.wave;
            // `NO_WAVE` is the knight's, the demon's and the dragon's: their
            // `INITMO` is the `ret` at 0x2059 and nothing about the fight is a
            // count. Everything else has all four numbers.
            if w.max == 0 {
                assert_eq!((w.heads, w.cap), (0, 0), "{}: half a wave", c.id);
                assert!(w.level.is_empty(), "{}: a lev_adjust row and no wave", c.id);
                assert!(!w.alternates && !w.opens_with_side, "{}: half a wave", c.id);
                continue;
            }
            assert!(w.heads > 0, "{}: MaxMonsters but no TotalMonsters", c.id);
            assert_eq!(w.level.len(), 8, "{}: lev_adjust is eight to a row", c.id);
            // A row falls from positive to negative across its eight, which is
            // what makes a strong knight meet more of a creature than a weak
            // one. It is not strictly monotonic, so this is the shape and not
            // the order: it starts at or above nothing and ends at or below it.
            assert!(
                w.level[0] >= 0 && w.level[7] <= 0,
                "{}: lev_adjust runs the wrong way",
                c.id
            );
            for v in w.level {
                assert!(
                    (-8..=8).contains(v),
                    "{}: lev_adjust entry {v} is not a signed byte",
                    c.id
                );
            }
            if w.cap > 0 {
                assert!(
                    w.cap <= 2,
                    "{}: AdjustLevel's three ceilings are 1, 1 and 2",
                    c.id
                );
            }
        }
        // The three the ceilings belong to, and what they are: `mov
        // [MaxMonsters], 1` at 0x288a for Balok and 0x2898 for the mudmen, and
        // `cmp [MaxMonsters], 2 / jle` at 0x289e for the troll.
        let cap = |id: &str| CREATURES.iter().find(|c| c.id == id).unwrap().wave.cap;
        assert_eq!((cap("balok"), cap("mudmen"), cap("troll")), (1, 1, 2));
        assert_eq!(cap("trogg_axe"), 0, "nothing holds a trogg fight down");
        // `InitKnightvsRatmen` (0x2337) is the only `MaxMonsters` of two in the
        // game: every other fight holds one creature at a time.
        for c in CREATURES.iter().filter(|c| c.wave.max > 0) {
            let want = if c.id == "ratmen" { 2 } else { 1 };
            assert_eq!(c.wave.max, want, "{}: MaxMonsters", c.id);
        }
        // And the head counts, routine by routine: 3 for the three troggs and
        // the beast (0x20bf, 0x214b, 0x21e8, 0x2293), 2 for the ratmen, Balok
        // and the mudmen (0x233d, 0x2597, 0x2626), 1 for the troll (0x26c5).
        let heads = |id: &str| CREATURES.iter().find(|c| c.id == id).unwrap().wave.heads;
        for id in ["trogg_axe", "trogg_hammer", "trogg_spear", "beast"] {
            assert_eq!(heads(id), 3, "{id}: TotalMonsters");
        }
        for id in ["ratmen", "balok", "mudmen"] {
            assert_eq!(heads(id), 2, "{id}: TotalMonsters");
        }
        assert_eq!(heads("troll"), 1);
        // `InitBalok` (0x25cf) is the one `INITMO` with no `xor [SIDE], 1`;
        // `InitKnightvsMudmen` (0x264c) and `InitKnightvsTroll` (0x26f4) are the
        // two fights that open through `INITMO` instead of `SetMonsterCombat`.
        let w = |id: &str| CREATURES.iter().find(|c| c.id == id).unwrap().wave;
        assert!(!w("balok").alternates);
        for id in [
            "trogg_axe",
            "trogg_hammer",
            "trogg_spear",
            "beast",
            "ratmen",
            "mudmen",
            "troll",
        ] {
            assert!(w(id).alternates, "{id} should flip SIDE");
        }
        for c in CREATURES.iter().filter(|c| c.wave.opens_with_side) {
            assert!(
                c.id == "mudmen" || c.id == "troll",
                "{} does not open through INITMO",
                c.id
            );
            assert_eq!(
                c.first_seat, 1,
                "{}: and SIDE puts the first one on record one",
                c.id
            );
        }
    }

    /// The four home villages, `MapIconsTABLE` frames 0x15 to 0x18, each gated
    /// on the knight's own colour index by `CheckGROOC` at image 0x732.
    #[test]
    fn the_four_villages_are_one_to_a_knight() {
        let mut marks = marks();
        for (i, frame) in (0x15u8..=0x18).enumerate() {
            marks.insert(frame, (20 + i as i32 * 60, 170));
        }
        let places: serde_json::Value =
            serde_json::from_str(&place_definitions(&BTreeMap::new(), &marks, &lairs())).unwrap();
        let mut whose: Vec<u64> = Vec::new();
        for (id, def) in places.as_object().unwrap() {
            if !id.starts_with("village.") {
                continue;
            }
            assert_eq!(def["line"], "Enter Village", "_MAP:knvillage, DS:0xc3a0");
            // `ForestVillage` loads no backdrop, so the village is a line on the
            // paper and not a screen.
            assert_eq!(def["scene"], "");
            assert_eq!(def["options"][0]["effect"]["do"], "village");
            whose.push(def["knight"].as_u64().unwrap());
        }
        whose.sort();
        assert_eq!(
            whose,
            vec![0, 1, 2, 3],
            "one village to each of the four knights"
        );
    }

    /// The hermit, a second healer this project invented and stood in the
    /// southern woods, is gone, and so is everything that was sold only there.
    #[test]
    fn nothing_on_the_map_is_sited_by_us() {
        let places = places();
        let marks = marks();
        for (id, def) in places.as_object().unwrap() {
            if def["hidden"].as_bool().unwrap_or(false) {
                continue;
            }
            let (x, y) = (def["x"].as_i64().unwrap(), def["y"].as_i64().unwrap());
            let known = marks
                .values()
                .any(|(mx, my)| *mx as i64 == x && *my as i64 == y)
                || lairs().iter().any(|l| l.x as i64 == x && l.y as i64 == y);
            assert!(known, "{id} stands at ({x}, {y}), which no table names");
        }
        assert!(
            places.get("healer").is_none(),
            "the hermit is not in the original"
        );
    }

    /// **Item 59.** The moors is landscape code 0, whose generator loads
    /// `GLB1.CMP` and rotates `GL1.t`..`GL8.t`; the four families and their
    /// file prefixes are what the four `GENERATE*` routines read. A family
    /// whose prefix does not match its own rotation would send every one of
    /// its arenas to the fallback, which is how a whole family goes missing.
    #[test]
    fn every_family_owns_the_layouts_its_own_prefix_claims() {
        let mut prefixes: BTreeSet<&str> = BTreeSet::new();
        for (name, prefix, _, _, rotation) in ARENAS {
            assert!(
                prefixes.insert(prefix),
                "{name}: two families claim {prefix}"
            );
            for arena in rotation {
                assert!(
                    arena.starts_with(prefix),
                    "{name} rotates through {arena}, which is not a {prefix} layout"
                );
            }
        }
        assert_eq!(prefixes.len(), 4, "four generators, four families");
        // And the moors keeps its own backdrop, which is the one thing that
        // tells it apart from the forest it shares a scenery sheet with.
        let moors = ARENAS
            .iter()
            .find(|(n, ..)| *n == "glade")
            .expect("no moors family");
        assert_eq!(moors.1, "gl");
        assert_eq!(moors.3, "GLB1.CMP");
    }

    /// The knight is what `SetUpKnight` (0x1786) and `SetKnightSwTables`
    /// (0x1f6a) wrote: every one of the eight kinds has its script and its
    /// `KnightDamSw` entry, every kind its blow taken, the three walk rows
    /// are the three rows of `KnightWalSw`, and the block table is
    /// `KnightBloSw`. Nothing here is typed in.
    #[test]
    fn the_knight_is_built_from_the_recovered_tables() {
        let (Some((scripts, banks)), Some(tables)) = (baked(), recovered()) else {
            return;
        };
        let json = actor_definitions(&scripts, &banks, &tables).unwrap();
        let actors: BTreeMap<String, ActorDef> = serde_json::from_str(&json).unwrap();
        let k = &actors["knight"];
        let t = tables.of("knight").unwrap();
        for (kind, script) in &t.attacks {
            let Some(name) = kind_name(*kind) else {
                continue;
            };
            assert_eq!(k.attacks[name].script, *script);
            assert_eq!(
                k.attacks[name].damage,
                t.damage.get(kind).copied().unwrap_or(0)
            );
            assert_eq!(k.hurt_by[name], t.hits[kind]);
        }
        assert_eq!(k.attacks.len(), 8);
        assert_eq!(k.attacks["chop"].damage, 4, "KnightDamSw[0x10] as written");
        assert_eq!(k.scripts_for("walk"), t.walk[0].as_slice());
        assert_eq!(k.scripts_for("walk_up"), t.walk[1].as_slice());
        assert_eq!(k.scripts_for("walk_down"), t.walk[2].as_slice());
        assert_eq!(k.scripts_for("idle"), std::slice::from_ref(&t.stance));
        assert_eq!(k.scripts_for("recover"), std::slice::from_ref(&t.recover));
        assert_eq!(k.scripts_for("death"), ["Knight_SwDeath".to_string()]);
        assert_eq!(k.blocks.len(), 4);
        assert_eq!(k.blocks["swing"], "block");
        assert_eq!(k.blocks["chop"], "evade");
        assert_eq!((k.approach, k.back_off, k.depth_tolerance), (100, 80, 4));
        // And the troll's blow taken is what `TrollStruck` writes, not the
        // table that names itself.
        let troll = &actors["troll"];
        assert!(troll.hurt_by.is_empty());
        assert_eq!(troll.scripts_for("hurt"), ["Troll_Hit".to_string()]);
        assert_eq!(troll.scripts_for("death"), ["Troll_Dies".to_string()]);
        assert_eq!(troll.damage, 7);
    }

    /// The check is real: misspell one script and the creature is refused,
    /// with the state and the name in the message.
    #[test]
    fn a_misspelt_creature_script_is_refused_by_name() {
        let (Some((scripts, banks)), Some(tables)) = (baked(), recovered()) else {
            return;
        };
        let mut c = Creature { ..CREATURES[0] };
        c.attack = &["Troll_Chopp"];
        let err = creature_definition(&c, &scripts, &banks, &tables)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("Troll_Chopp") && err.contains("attack"),
            "{err}"
        );
    }

    /// A stand-in for `MapIconsTABLE`, so the place table can be exercised
    /// without the unpacked executable to read the real one out of. The frames
    /// are the real ones; the corners are not, and are only spread out enough
    /// that nothing lands on anything.
    fn marks() -> BTreeMap<u8, (i32, i32)> {
        [
            (0x19u8, (10, 10)),
            (0x1a, (200, 10)),
            (0x1b, (100, 10)),
            (0x1c, (150, 40)),
            (0x1e, (250, 40)),
        ]
        .into_iter()
        .collect()
    }

    /// A stand-in for the four lair tables, in the same shape `lair_table`
    /// hands back: the landscape codes in `LairFile`'s own family order, six to
    /// a family, guardians `InitGameStart` really fills `CombatTable` with, a
    /// different count in every record so the baker can be caught crossing two
    /// of them, and corners that clear every other place.
    fn lairs() -> Vec<LairPlace> {
        const SLOTS: [u16; 6] = [0, 1, 6, 7, 9, 16];
        (0..LAIR_COUNT)
            .map(|i| LairPlace {
                guardian: GUARDIANS
                    .iter()
                    .find(|(s, _)| *s == SLOTS[i % 6])
                    .unwrap()
                    .1,
                count: i as u32 + 1,
                x: 4 + (i % 6) as i32 * 50,
                y: 100 + (i / 6) as i32 * 20,
                code: [2u8, 6, 4, 0][i / 6],
            })
            .collect()
    }

    fn places() -> serde_json::Value {
        serde_json::from_str(&place_definitions(&BTreeMap::new(), &marks(), &lairs())).unwrap()
    }

    /// Every slot `InitGameStart` writes into `CombatTable` names a creature
    /// the bestiary has, and no slot is named twice. A lair asking for a slot
    /// this table has not got is what `lair_table` refuses.
    #[test]
    fn every_combat_table_slot_names_a_creature() {
        let mut seen = BTreeSet::new();
        for (slot, id) in GUARDIANS {
            assert!(
                seen.insert(*slot),
                "CombatTable slot {slot} is listed twice"
            );
            assert!(
                guardian_is_known(id),
                "CombatTable slot {slot} is {id}, which the pack has nobody for"
            );
        }
    }

    /// The twenty four lairs, in `LairFile` order, each on its own layout.
    ///
    /// This is the check that the keys land where the quest expects them:
    /// `Run::stock_lairs` plants one key per family six records apart, so a
    /// lair whose number and family disagree hides the forest's key in a marsh.
    #[test]
    fn the_lairs_are_the_twenty_four_lairfile_names_in_order() {
        let places = places();
        let mut seen: BTreeMap<usize, (String, String, String, u64)> = BTreeMap::new();
        for def in places.as_object().unwrap().values() {
            for choice in def["options"].as_array().unwrap() {
                let e = &choice["effect"];
                if e["do"] != "raid" {
                    continue;
                }
                let was = seen.insert(
                    e["lair"].as_u64().unwrap() as usize,
                    (
                        e["arena"].as_str().unwrap().into(),
                        e["family"].as_str().unwrap().into(),
                        e["guardian"].as_str().unwrap().into(),
                        e["count"].as_u64().unwrap(),
                    ),
                );
                assert!(was.is_none(), "two places claim lair {}", e["lair"]);
            }
        }
        let want: Vec<String> = ["fo", "wa", "sw", "gl"]
            .iter()
            .flat_map(|p| (1..=6).map(move |n| format!("{p}l{n}")))
            .collect();
        assert_eq!(want.len(), 24);
        for (n, arena) in want.iter().enumerate() {
            let (got, family, guardian, count) = seen
                .get(&n)
                .unwrap_or_else(|| panic!("no lair numbered {n}"));
            assert_eq!(got, arena, "lair {n} is fought on the wrong layout");
            // `LairFile` order is forest, waste, swamp, glade, which is also
            // `moon::Key::ALL`, which is what puts each key in its own ground.
            let want_family = henge_core::moon::Key::ALL[n / 6].family();
            assert_eq!(family, want_family, "lair {n} is on the wrong ground");
            assert!(
                CREATURES.iter().any(|c| c.id == guardian),
                "lair {n} is guarded by {guardian}, which is not in the bestiary"
            );
            // `ForestLairs` pairs a head count with each guardian, and each
            // record has to reach its own lair and no other. The stand-in
            // numbers every lair differently so a crossed pair shows up.
            //
            // The bound this used to carry, one to three, was written when the
            // count was this project's own invention and a bout seats four.
            // The recovered `TotalMonsters` runs from three to fourteen, so
            // the bound was a statement about the invention and not about the
            // game; what belongs here is that the table is copied faithfully.
            assert_eq!(
                *count,
                n as u64 + 1,
                "lair {n} was given another lair's head count"
            );
        }
    }

    /// Every door leads somewhere and every counter sells something the pack
    /// has. A stall naming an item that is not declared is a dead line that
    /// only shows up by walking into it.
    #[test]
    fn every_door_and_every_counter_in_the_pack_resolves() {
        let places = places();
        let places = places.as_object().unwrap();
        let items: serde_json::Value = serde_json::from_str(&item_definitions()).unwrap();
        let items = items.as_object().unwrap();
        for (id, def) in places {
            for choice in def["options"].as_array().unwrap() {
                let e = &choice["effect"];
                if let Some(to) = e.get("place").and_then(|p| p.as_str()) {
                    assert!(
                        places.contains_key(to),
                        "{id} has a door to {to}, which does not exist"
                    );
                }
                if let Some(item) = e.get("item").and_then(|p| p.as_str()) {
                    assert!(
                        items.contains_key(item),
                        "{id} deals in {item}, which the pack has not got"
                    );
                }
            }
        }
    }

    /// The ten magic slots the engine hands out have to be items the pack
    /// declares, or the wizard's gift and every lair floor quietly falls back
    /// to whichever of the ten does exist.
    #[test]
    fn the_pack_declares_every_magic_slot_the_engine_can_bestow() {
        let items: serde_json::Value = serde_json::from_str(&item_definitions()).unwrap();
        let items = items.as_object().unwrap();
        for (_, slot) in henge_core::service::MAGIC_TABLE {
            let id = henge_core::service::magic_item(slot)
                .unwrap_or_else(|| panic!("slot {slot:#x} names nothing"));
            assert!(
                items.contains_key(id),
                "the pack has no {id}, which is magic slot {slot:#x}"
            );
        }
        // And the four keys, so one carried out of a lair has a name.
        for key in henge_core::moon::Key::ALL {
            assert!(
                items.contains_key(key.item()),
                "the pack has no {}",
                key.item()
            );
        }
    }

    /// No two places on the map share ground. Two boxes overlapping would make
    /// one of them unreachable, since walking opens the nearer of the pair.
    #[test]
    fn nothing_on_the_map_stands_on_anything_else() {
        let places = places();
        let boxes: Vec<(String, i64, i64, i64, i64)> = places
            .as_object()
            .unwrap()
            .iter()
            .filter(|(_, d)| !d["hidden"].as_bool().unwrap_or(false))
            .map(|(id, d)| {
                let n = |k: &str| d[k].as_i64().unwrap();
                (id.clone(), n("x"), n("y"), n("w"), n("h"))
            })
            .collect();
        assert!(
            boxes.len() >= 24 + 5,
            "only {} places on the map",
            boxes.len()
        );
        for (i, a) in boxes.iter().enumerate() {
            assert!(a.1 >= 0 && a.1 + a.3 <= 320, "{} is off the map", a.0);
            assert!(a.2 >= 0 && a.2 + a.4 <= 200, "{} is off the map", a.0);
            for b in &boxes[i + 1..] {
                let apart =
                    a.1 + a.3 <= b.1 || b.1 + b.3 <= a.1 || a.2 + a.4 <= b.2 || b.2 + b.4 <= a.2;
                assert!(apart, "{} and {} stand on the same ground", a.0, b.0);
            }
        }
    }
}
