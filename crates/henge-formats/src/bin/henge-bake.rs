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
use henge_formats::taskvm::{all_scripts, Symbols};
use henge_formats::{piv, voc, Collide, Library, Sprite};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

/// Sprite banks grouped into the actors they actually belong to.
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

/// What waylays a traveller on each kind of ground, in the order the family's
/// turn counter brings them round.
///
/// **Design, not recovered.** Which creature a stretch of the map produces is
/// decided in `_MAP` and has not been read out of it. This is the obvious
/// reading: troggs and ratmen under the trees, mudmen in the marsh, a troll
/// in the waste. The beast, Balok, the demon and the dragon are lair and
/// set-piece encounters in the original and do not wait by the road here
/// either; `--foe` puts any of them in an arena.
const AMBUSHES: &[(&str, &[&str])] = &[
    ("glade", &["trogg_axe", "ratmen", "trogg_hammer"]),
    ("forest", &["trogg_axe", "ratmen", "trogg_spear"]),
    ("swamp", &["mudmen", "trogg_spear", "mudmen"]),
    ("waste", &["troll", "trogg_hammer", "trogg_axe"]),
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

/// Which script each of the knight's states plays.
///
/// **Recovered, as far as it goes.** The controller's tables are `BSS`, which
/// is why they were thought lost, but `SetKnightAnims` fills them at start-up:
/// `KnightWalSw` holds `Knight_SwWalkR1` through `R4` as the right-facing row,
/// `KnightAttSw` the nine attacks by kind (lunge, swing, knife, block, three
/// thrusts, evade, chop), and `KnightHitSw` the blow taken for each kind, with
/// `Knight_SwWaistHit` for a swing. What is still ours is which one attack the
/// one button gets, and which blow is taken for it: the swing, and the
/// shoulder hit it was checked against when the VM landed. The walk's shape is
/// the original's: four single-frame scripts, each ending on `ff ff`, the
/// controller handing over the next each time the last has ended.
const KNIGHT_SCRIPTS: &[(&str, &[&str])] = &[
    ("idle", &["Knight_SwStance"]),
    ("walk", &[
        "Knight_SwWalkR1", "Knight_SwWalkR2", "Knight_SwWalkR3", "Knight_SwWalkR4",
    ]),
    ("attack", &["Knight_SwSwing"]),
    ("hurt", &["Knight_SwShoulderHit"]),
    ("death", &["Knight_SwDeath"]),
    // `+0x12`, what `KnightHitNormal` hands over the moment a blow lands.
    ("recover", &["Knight_SwRecover"]),
];

/// `KnightAttSw` and `KnightDamSw`, as `SetUpKnight` fills them: the script
/// for each attack kind and what the blow is worth before `CalcDamage` adds
/// strength and the sword. The chop is written as eight rather than the
/// table's four because `CalcDamage` doubles a chop after the additions, and
/// the block and the evade take nothing off anyone.
///
/// **Recovered.** Which of these the joystick picks is `Rjoystick` and
/// `Ljoystick`, in `henge_core::combat::Attack::for_direction`.
const KNIGHT_ATTACKS: &[(&str, &str, i32)] = &[
    ("lunge", "Knight_SwLunge", 3),
    ("swing", "Knight_SwSwing", 4),
    ("knife", "Knight_SwKnife", 3),
    ("block", "Knight_SwBlock", 0),
    ("rthrust", "Knight_SwRThrust", 2),
    ("uthrust", "Knight_SwUThrust", 3),
    ("evade", "Knight_SwEvade", 0),
    ("chop", "Knight_SwChop", 8),
];

/// `KnightHitSw`: the blow the knight takes, by the attacker's kind. A cut
/// is taken at the waist, a stab or a chop at the shoulder; a blow of kind
/// block or evade cannot land, so those rows (`Knight_SwRecover`) are not
/// carried.
const KNIGHT_HURT: &[(&str, &str)] = &[
    ("lunge", "Knight_SwWaistHit"),
    ("swing", "Knight_SwWaistHit"),
    ("knife", "Knight_SwShoulderHit"),
    ("rthrust", "Knight_SwWaistHit"),
    ("uthrust", "Knight_SwShoulderHit"),
    ("chop", "Knight_SwShoulderHit"),
];

/// `KnightBloSw`, which the name had suggested was blood and is the block
/// table `CheckBlock` reads: the guard that stops each kind. The rows for
/// the knife and the up thrust are left at zero in the original, which is
/// what an idle knight holds; that quirk lives in `Fighter::blocks`.
const KNIGHT_BLOCKS: &[(&str, &str)] = &[
    ("chop", "evade"),
    ("swing", "block"),
    ("lunge", "evade"),
    ("rthrust", "evade"),
];

/// What a blow does to a knight who is down: `MudmenStruck1` and
/// `KnightKnightStruck1`. `Knight_SwCollapse` is the second half of
/// `Knight_SwDeath`, the fall itself; `Knight_SwDeCap` sets `DeCapFLAG`,
/// skips to the collapse when the gore is off, and ends by killing the task.
const KNIGHT_FINISHES: &[(&str, &str)] = &[
    ("collapse", "Knight_SwCollapse"),
    ("decap", "Knight_SwDeCap"),
];

/// The scripts the game's own code hands to a task it spawns off the knight:
/// `KnifeThrow` starts the dagger on `SpeedKnife` and `ControlKnife` keeps
/// it on `Knife`. Both draw from `KN4.OB`, slot 3 of the knight's table.
const KNIGHT_SPAWNED: &[&str] = &["SpeedKnife", "Knife"];

/// The spray `AddBlood` starts, on bank table 4. Every part of it is gated.
const BLOOD: &str = "Blood1";

/// One creature of the bestiary: which scripts its five states play, and the
/// numbers the original's own set-up routine gives it.
#[derive(Clone, Copy)]
struct Creature {
    id: &'static str,
    name: &'static str,
    /// Key into `CREATURE_BANKS`: the loader whose table 2 the scripts index.
    banks: &'static str,
    /// The sheet the first bank was packed into, for `ActorDef::sheet`.
    sheet: &'static str,
    idle: &'static [&'static str],
    walk: &'static [&'static str],
    attack: &'static [&'static str],
    hurt: &'static [&'static str],
    death: &'static [&'static str],
    /// The kind the attack above lands as: what the creature's own routine
    /// writes into `+0x28` before it plays the script, named the way the
    /// knight's kinds are. That is what indexes the knight's `KnightHitSw`.
    kind: &'static str,
    /// The creature's other attacks, by kind and by what the `*Dam` table
    /// gives that kind. Which it picks when is its controller's business.
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
    /// `*Hit`: the blow-taken script by the knight's attack kind, each with
    /// its own `TASKDEAD` and so its own death. A kind missing here takes
    /// `hurt`.
    hurt_by: &'static [(&'static str, &'static str)],
    /// Whether its blows go through `CheckBlock`: only the troggs' do.
    blockable: bool,
    /// Whether `AddBlood` is called when it is struck.
    bleeds: bool,
    /// `+0x38` and `+0x3c` of the actor record, from `Set*Tables`.
    health: i32,
    /// What its blow takes off, from the `*Dam` table `SetMonsterAnims` fills,
    /// or a stand-in where the original sets it in code that has not been read.
    damage: i32,
    /// `+0x52`, `+0x54` and `+0x56`: stop approaching, give ground, same plane.
    approach: i32,
    back_off: i32,
    depth: i32,
    /// Ours: how close the plain opponent walks before it swings.
    reach: i32,
    /// Ours: pixels a tick, read off the walk offset tables where there is one.
    speed: [i32; 2],
    /// Ours: what it is worth to whoever puts it down.
    bounty: u32,
    /// Ours: how much ground it stands on, or zero to take three quarters of
    /// the standing frame's width the way the knight's was set.
    girth: i32,
    /// What the moon does to it: the phase key, the hit points and the blow it
    /// is fielded with on that night. Only the ratman has one.
    moon: &'static [(&'static str, i32, i32)],
}

/// The bestiary, as the original sets each creature up.
///
/// **Recovered, most of it.** `SetKnightAnims` and `SetMonsterAnims` in
/// `MOON` fill the controller tables that were thought lost: for each
/// creature a walk table (`*Wal`: the right-facing cycle at +0, up at +0x10,
/// down at +0x20, each zero terminated), an attack table (`*Att`, by attack
/// kind), a blow-taken table (`*Hit`, indexed by the attacker's attack kind,
/// 2 lunge, 4 swing, 6 knife, 0xa right thrust, 0xc up thrust, 0x10 chop) and
/// a damage table (`*Dam`, same index). The `Set*Tables` routines called from
/// each `InitKnightvs*` write the stat block: hit points at `+0x38` and
/// `+0x3c`, the tracker's two ranges at `+0x52` and `+0x54`, its plane
/// tolerance at `+0x56`, the creature's kind at `+0x35`, and the stance and
/// recovery scripts at `+0x10` and `+0x12`.
///
/// The walk cycles are the `*Wal` right-facing rows exactly. The blow taken
/// is the `*Hit` entry for a swing, the one attack the knight has here, and
/// the death is where that script's own `TASKDEAD` goes, so a creature dies
/// the way the original kills it for that blow. The attack is one of the
/// creature's own from its `*Att` table or, for the ones the code chooses
/// directly (spear, ratman, troll, balok, demon, dragon), the script that
/// routine picks first.
///
/// Numbers marked ours: `reach`, which is the range the plain opponent
/// swings at and sits inside the recovered `approach` so the weapon actually
/// crosses the body; `speed`, read off the `*WALKR` offset tables where the
/// creature has one and chosen otherwise; and `bounty`, which the original
/// keeps no table for.
const CREATURES: &[Creature] = &[
    Creature {
        id: "troll", name: "Troll", banks: "troll", sheet: "actor.troll",
        idle: &["Troll_Stance"],
        walk: &["Troll_Walk1", "Troll_Walk2", "Troll_Walk3", "Troll_Walk4"],
        // The troll has two: `Troll_Bunt`, a club thrust that lands from 33
        // to 98 pixels out, and `Troll_Chop`, an overhead that lands from 105
        // to 160 and shakes the screen. The original picks by distance, which
        // is behaviour; the plain opponent closes in, so it gets the bunt.
        attack: &["Troll_Bunt"],
        // `SetMonsterAnims` fills `TrollHit` with the table's own address
        // rather than `Troll_Hit`, eight times over, which reads as a slip in
        // the original; `Troll_Hit` is the only blow-taken script it has.
        hurt: &["Troll_Hit"],
        death: &["Troll_Dies"],
        // `TrollBunt` writes 4 and `TrollChop` 0x10. `TrollHit` is one script
        // for every kind, and `TrollStruck` calls `AddBlood`.
        kind: "swing",
        alternates: &[("chop", "Troll_Chop", 6)],
        // `ControlTroll`: the club inside a hundred, the overhead from a
        // hundred to a hundred and fifty, and never two overheads running.
        controller: "troll", rows: &[], border: None, spawns: &[],
        hurt_by: &[],
        blockable: false,
        bleeds: true,
        health: 40, damage: 3, approach: 150, back_off: 90, depth: 5,
        // `TrollWALKR` steps 16, 26, 13, 26: twenty pixels a frame.
        reach: 80, speed: [3, 1], bounty: 40, girth: 0, moon: &[],
    },
    Creature {
        id: "trogg_axe", name: "Trogg", banks: "trogg_axe", sheet: "actor.trogg_axe",
        idle: &["TroggAxe_Stance"],
        walk: &["TroggAxe_WalkR1", "TroggAxe_WalkR2", "TroggAxe_WalkR3"],
        attack: &["TroggAxe_Swing"],
        hurt: &["TroggAxe_WaistHit"],
        death: &["TroggAxe_Split"],
        // `TroggSwing` writes 4 and `TroggChop` 0x10; `TroggHitAxe` is the
        // row below, and `TroggStruck1` runs the knight's `CheckBlock`.
        kind: "swing",
        alternates: &[("chop", "TroggAxe_Chop", 6)],
        controller: "trogg", rows: &[], border: None, spawns: &[],
        hurt_by: &[
            ("lunge", "TroggAxe_Stabbed"), ("swing", "TroggAxe_WaistHit"),
            ("knife", "TroggAxe_ShoulderHit"), ("rthrust", "TroggAxe_Stabbed"),
            ("uthrust", "TroggAxe_ShoulderHit"), ("chop", "TroggAxe_ShoulderHit"),
        ],
        blockable: true,
        bleeds: false,
        health: 20, damage: 3, approach: 100, back_off: 90, depth: 5,
        // `TroggWALKR` steps 0, 7, 23: ten pixels a frame.
        reach: 70, speed: [2, 1], bounty: 15, girth: 0, moon: &[],
    },
    Creature {
        id: "trogg_hammer", name: "Trogg", banks: "trogg_axe", sheet: "actor.trogg_axe",
        idle: &["TroggHammer_Stance"],
        walk: &["TroggHammer_WalkR1", "TroggHammer_WalkR2", "TroggHammer_WalkR3"],
        attack: &["TroggHammer_Swing"],
        hurt: &["TroggHammer_WaistHit"],
        death: &["TroggHammer_Split"],
        kind: "swing",
        alternates: &[("chop", "TroggHammer_Chop", 4)],
        controller: "trogg", rows: &[], border: None, spawns: &[],
        hurt_by: &[
            ("lunge", "TroggHammer_Stabbed"), ("swing", "TroggHammer_WaistHit"),
            ("knife", "TroggHammer_ShoulderHit"), ("rthrust", "TroggHammer_Stabbed"),
            ("uthrust", "TroggHammer_ShoulderHit"), ("chop", "TroggHammer_ShoulderHit"),
        ],
        blockable: true,
        bleeds: false,
        health: 20, damage: 2, approach: 70, back_off: 65, depth: 5,
        reach: 60, speed: [2, 1], bounty: 15, girth: 0, moon: &[],
    },
    Creature {
        id: "trogg_spear", name: "Trogg", banks: "trogg_spear", sheet: "actor.trogg_spear",
        idle: &["TroggSpear_Stance"],
        walk: &["TroggSpear_WalkR1", "TroggSpear_WalkR2", "TroggSpear_WalkR3"],
        attack: &["TroggSpear_Lunge"],
        hurt: &["TroggSpear_WaistHit"],
        death: &["TroggSpear_Split"],
        // `TroggAttacks` writes 2 for the spear. `TroggSpearStruck1` runs
        // `CheckBlock` and answers a block with `Knight_SwEvade`.
        kind: "lunge",
        alternates: &[],
        // `TroggAttacks` takes its kind 0x10 branch for the spear: one lunge,
        // only inside the approach range, and twenty frames before the next.
        controller: "trogg_spear", rows: &[], border: None, spawns: &[],
        hurt_by: &[
            ("lunge", "TroggSpear_Stabbed"), ("swing", "TroggSpear_WaistHit"),
            ("knife", "TroggSpear_ShoulderHit"), ("rthrust", "TroggSpear_Stabbed"),
            ("uthrust", "TroggSpear_ShoulderHit"), ("chop", "TroggSpear_ShoulderHit"),
        ],
        blockable: true,
        bleeds: false,
        // No `TroggDamSp` exists; the spear's blow is set where the lunge
        // lands, in code not yet read. Three is the axe's, as a stand-in.
        health: 15, damage: 3, approach: 130, back_off: 120, depth: 5,
        reach: 100, speed: [2, 1], bounty: 15, girth: 0, moon: &[],
    },
    Creature {
        id: "ratmen", name: "Ratman", banks: "ratmen", sheet: "actor.ratmen",
        idle: &["Ratman_Stance"],
        walk: &["Ratman_Roll1", "Ratman_Roll2", "Ratman_Roll3", "Ratman_Roll4"],
        attack: &["Ratman_Slash"],
        hurt: &["Ratman_Knocked"],
        death: &["Ratman_KnockDead"],
        // `ControlRatCollide` writes 4 for the slash and 2 for the bite,
        // which is a grab and waits on 37. `RatmenHit` is the row below.
        kind: "swing",
        // `ControlRatCollide` writes 4 for the slash and 2 for the bite, and
        // `RatmenDam` gives one for the slash and three for the bite.
        alternates: &[("lunge", "Ratman_Bite", 3)],
        controller: "ratman",
        // `RatmenWal`'s up row is the leap, which is what it crosses ground on.
        rows: &[("leap", &["Ratman_Leap"])],
        border: None, spawns: &[],
        hurt_by: &[
            ("lunge", "Ratman_Stabbed"), ("swing", "Ratman_Knocked"),
            ("knife", "Ratman_Stabbed"), ("rthrust", "Ratman_Stabbed"),
            ("uthrust", "Ratman_Stabbed"), ("chop", "Ratman_HitOnHead"),
        ],
        blockable: false,
        bleeds: false,
        // **The one creature the moon moves**, and the record's earlier reading
        // of it was off by a row. `SetRatmenTables` writes five hit points and
        // then fills `RatmenDam` twice: `[bx+2]` with three and `[bx+4]` with
        // one. Those tables are nine words indexed by attack kind, two to an
        // entry, so `[bx+4]` is kind 4, which is what `ControlRatCollide`
        // writes for the slash; `[bx+2]` is kind 2, the bite, which is item
        // 37's. The slash is therefore one, not three. On the full moon the
        // routine rewrites them as seven and three, and on the new moon as
        // twelve and five, and that is the table below.
        health: 5, damage: 1, approach: 40, back_off: 30, depth: 5,
        reach: 24, speed: [3, 1], bounty: 5, girth: 0,
        moon: &[("full", 7, 3), ("new", 12, 5)],
    },
    Creature {
        id: "mudmen", name: "Mudman", banks: "mudmen", sheet: "actor.mudmen",
        idle: &["Mudmen_Stance"],
        walk: &["Mudmen_Move1", "Mudmen_Move3", "Mudmen_Move1", "Mudmen_Move2"],
        attack: &["Mudmen_ArmAttack"],
        hurt: &["Mudmen_Hit"],
        death: &["Mudmen_Dies"],
        // The mudman's routines never write `+0x28`, so it stays at zero and
        // `KnightHitSw[0]` is the stance: in the original its arm does not
        // stagger the knight, it entangles him, which is item 37. A swing
        // stands in so the blow is felt.
        kind: "swing",
        alternates: &[],
        // `ControlMudmen`: it reaches for you between seventy five and a
        // hundred and goes under the ground inside that.
        controller: "mudman",
        rows: &[("bury", &["Mudmen_IBury"]), ("appear", &["Mudmen_Appear"])],
        border: None,
        spawns: &["Mudmen_EntangleKnight", "Mudmen_ChokeKnight", "Mudmen_KnightSd"],
        hurt_by: &[],
        blockable: false,
        bleeds: false,
        health: 30, damage: 2, approach: 80, back_off: 75, depth: 5,
        // `MudmenWALK` steps (12, 12), (10, 14): it comes at you on a
        // diagonal, eleven across and thirteen deep a frame.
        reach: 90, speed: [2, 2], bounty: 25, girth: 0, moon: &[],
    },
    Creature {
        id: "demon", name: "Demon", banks: "demon", sheet: "actor.demon",
        // `[di+0x10]` is `Demon_Evolve`, so the demon's own stance slot is
        // its materialisation; the four stances that follow it are what
        // `Demon_Stance1`'s `TASKSAVE` into `+0x10` cycles through.
        idle: &["Demon_Stance1", "Demon_Stance2", "Demon_Stance3", "Demon_Stance4"],
        walk: &["Demon_Stance1", "Demon_Stance2", "Demon_Stance3", "Demon_Stance4"],
        attack: &["Demon_Slap"],
        hurt: &["Demon_Hurt"],
        death: &["Demon_Death"],
        // `DemonAttack` writes 0x10 for the slap, 4 for the zap, 2 for the whip.
        kind: "chop",
        alternates: &[("swing", "Demon_Zap", 4), ("lunge", "Demon_Whip", 4)],
        controller: "demon",
        rows: &[("evolve", &["Demon_Evolve"])],
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
        hurt_by: &[],
        blockable: false,
        bleeds: false,
        // `InitKnightvsDemon` writes 250 hit points and no maximum. Its blow
        // is not in a `*Dam` table; four is a stand-in between a troll's and
        // a dragon's bite. It moves five pixels a frame, which is one a tick.
        health: 250, damage: 4, approach: 95, back_off: 90, depth: 2,
        reach: 65, speed: [1, 1], bounty: 100, girth: 0, moon: &[],
    },
    Creature {
        id: "beast", name: "Beast", banks: "beast", sheet: "bank.be1",
        idle: &["Beast_Drool1"],
        walk: &["Beast_Run1", "Beast_Run2", "Beast_Run3", "Beast_Run4"],
        // The beast has no swing: its run is the attack, every `Beast_Run`
        // frame carrying a weapon part, and what it does to a knight it
        // reaches (`Beast_BackToss`, `Beast_ChestToss`) is the knight's own
        // animation. Until that is built, the first run frame is its attack.
        attack: &["Beast_Run1"],
        hurt: &["Beast_LowerHit"],
        death: &["Beast_LowerDead"],
        // `ControlBeast` writes 0x10. `BeastHit2` is the row below; a blow
        // from above finds the head and its own death.
        kind: "chop",
        alternates: &[],
        // `ControlBeast`: it never tracks. It runs from one side of the arena
        // to the other, turns off the edge, waits five to twenty frames, picks
        // a depth and comes back.
        controller: "beast", rows: &[], border: None, spawns: &[],
        hurt_by: &[
            ("lunge", "Beast_LowerHit"), ("swing", "Beast_LowerHit"),
            ("knife", "Beast_LowerHit"), ("rthrust", "Beast_LowerHit"),
            ("uthrust", "Beast_UpperHit"), ("chop", "Beast_UpperHit"),
        ],
        blockable: false,
        bleeds: false,
        // Ten hit points, and a tracker that closes to two pixels. Its blow
        // is not in a `*Dam` table; three is a stand-in.
        health: 10, damage: 3, approach: 2, back_off: 1, depth: 5,
        // `BeastChargeOffsets` 33, 27, 17, 33: nearly thirty pixels a frame.
        // Its weapon parts are its own body, so it has to be allowed close:
        // three quarters of its width would keep it out of its own bite.
        reach: 40, speed: [4, 1], bounty: 30, girth: 20, moon: &[],
    },
    Creature {
        id: "balok", name: "Balok", banks: "balok", sheet: "actor.balok",
        idle: &["Balok_Stance"],
        walk: &["Balok_Jump", "Balok_Jumping"],
        attack: &["Balok_UpperCut"],
        hurt: &["Balok_UpperHit"],
        death: &["Balok_Dead"],
        // `ControlBalok` writes 4 for the uppercut and 0x10 for the grab.
        // `BalokStruck` calls `AddBlood`.
        kind: "swing",
        alternates: &[("chop", "Balok_Grab", 4)],
        controller: "balok", rows: &[], border: None, spawns: &[],
        hurt_by: &[],
        blockable: false,
        bleeds: true,
        health: 30, damage: 4, approach: 80, back_off: 60, depth: 10,
        // Its uppercut lands from 41 to 74 pixels out, and its own width
        // keeps a knight sixty away, so it swings from just outside that.
        reach: 70, speed: [2, 1], bounty: 80, girth: 0, moon: &[],
    },
    Creature {
        id: "dragon", name: "Dragon", banks: "dragon", sheet: "actor.dragon",
        idle: &["Dragon_Stance"],
        // The dragon does not walk anywhere. `TrackKnight` shifts its head
        // five pixels at a time inside a corridor thirty to a hundred wide and
        // follows the knight in depth, on the standing frame it is holding.
        walk: &["Dragon_Stance"],
        attack: &["Dragon_HighBite"],
        hurt: &["Dragon_Hit"],
        death: &["Dragon_Dead"],
        // `DragonAttack` writes 2 for the bite, 4 for the low breath and 0x10
        // for the high one. `DragonStruck` calls `AddBlood`.
        kind: "lunge",
        alternates: &[("swing", "Dragon_LowBreath", 30), ("chop", "Dragon_HighBreath", 30)],
        controller: "dragon",
        // `DragonWal` holds no walk row at all: the rows at +0x10 and +0x20
        // are the head lifting and lowering, five scripts each with the fifth
        // repeated to fill the eight `NextWalk` steps through.
        rows: &[
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
        hurt_by: &[],
        blockable: false,
        bleeds: true,
        // `SetUpDragonTables` writes 200 hit points and a maximum of 120.
        // `DragonDam`, as `InitKnightvsDragon` overwrites it for this fight,
        // is 10 for a lunge or a right thrust and 30 for a swing or a chop.
        health: 200, damage: 10, approach: 60, back_off: 20, depth: 5,
        // The girth is a fraction of the figure, which is 212 wide: the bite
        // is at its origin, and a knight kept the figure's width away could
        // never be bitten.
        reach: 60, speed: [1, 1], bounty: 250, girth: 50, moon: &[],
    },
    Creature {
        // `InitKnightvsDragon` sets two more actors up beside the dragon,
        // `Claw1TABLE` and `Claw2TABLE`, at x 5 and ten rows either side of
        // the head's depth. `ControlClaw` never subtracts a hit point: they
        // guard the ground in front of the dragon and die when it does.
        id: "dragon_claw", name: "Claw", banks: "dragon", sheet: "actor.dragon",
        idle: &["Dragon_Claw"],
        walk: &["Dragon_Claw"],
        attack: &["Dragon_ClawSlap"],
        hurt: &["Dragon_Claw"],
        death: &["Dragon_ClawDead"],
        // `ControlClaw` writes 0xa, the rear thrust's kind, and
        // `InitKnightvsDragon` writes 10 into that row of `DragonDam`.
        kind: "rthrust",
        alternates: &[],
        controller: "claw", rows: &[], border: None, spawns: &[],
        hurt_by: &[],
        blockable: false,
        bleeds: false,
        // `SetUpDragonTables` runs after the fifty is written and puts the
        // dragon's own 200 and 120 back over it, so fifty never takes effect.
        health: 200, damage: 10, approach: 0, back_off: 0, depth: 10,
        reach: 60, speed: [0, 0], bounty: 0, girth: 30, moon: &[],
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
        let dir = argv.get(i + 1).cloned().unwrap_or_else(|| "research/music".into());
        let text = fs::read_to_string("research/tunes.json")
            .context("reading research/tunes.json")?;
        let tunes: henge_audio::music::Tunes = serde_json::from_str(&text)?;
        fs::create_dir_all(&dir)?;
        for (name, score) in tunes.split() {
            let pcm = henge_audio::music::render(&score);
            let path = Path::new(&dir).join(format!("{name}.wav"));
            fs::write(&path, henge_audio::music::wav(&pcm, henge_audio::music::RATE))?;
            println!(
                "{}: {} notes, {:.1}s, {}",
                path.display(), score.notes.len(), score.seconds(),
                if score.looping { "loops" } else { "runs once" }
            );
        }
        return Ok(());
    }
    let mut args = std::env::args().skip(1);
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
                    println!("pack is already baked to recipe {RECIPE} from this image; nothing to do");
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
    let fallback = m
        .palettes
        .values()
        .next()
        .cloned()
        .unwrap_or_else(|| (0..32).map(|i: u32| (i * 8) << 16 | (i * 8) << 8 | i * 8).collect());

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
        write_indexed(&out.join(&file), sheet.width, sheet.height, &sheet.pixels, &fallback)?;
        m.sheets.insert(id, Sheet { file, frames: sheet.rects });
    }

    // Everything else that is a sprite bank, so nothing is silently dropped.
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
        write_indexed(&out.join(&file), sheet.width, sheet.height, &sheet.pixels, &fallback)?;
        m.sheets.insert(format!("bank.{stem}"), Sheet { file, frames: sheet.rects });
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
                        x: 0, y: 0, w: piv::W as u32, h: piv::H as u32, ox: 0, oy: 0,
                    }],
                },
            );
            m.palettes.insert(format!("palette.scene.{stem}"), p.palette);
        }
    }

    // Samples, converted to plain WAV so the engine never learns about VOC.
    let mut sounds = 0;
    for name in lib.names() {
        let Ok(bytes) = lib.bytes(&name) else { continue };
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
            && t.borders.iter().all(|b| {
                b.left < b.right && b.top < b.bottom && b.right < 640 && b.bottom < 400
            });
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
    fs::write(out.join("data/arenas.json"), serde_json::to_string(&arenas)?)?;
    m.data.insert("data.arenas".into(), "data/arenas.json".into());

    if let Ok(bytes) = lib.bytes("COLLIDE.HIT") {
        let c = Collide::parse(&bytes)?;
        fs::write(out.join("data/hitlines.json"), serde_json::to_string(&c)?)?;
        m.data.insert("data.hitlines".into(), "data/hitlines.json".into());
    }

    // Which sheets each arena family draws from, and the eight arenas its
    // counter rotates through.
    let key = |f: &str| format!("scene.{}", f.split('.').next().unwrap_or(f).to_lowercase());
    let families: BTreeMap<&str, serde_json::Value> = ARENAS
        .iter()
        .map(|(name, _, sheet, backdrop, rotation)| {
            let creatures: &[&str] =
                AMBUSHES.iter().find(|(f, _)| f == name).map_or(&[], |(_, c)| *c);
            (*name, serde_json::json!({
                "sheet": key(sheet),
                "backdrop": key(backdrop),
                "tiles": { "4": key(SHARED_TILES) },
                "arenas": rotation,
                "creatures": creatures,
            }))
        })
        .collect();
    fs::write(out.join("data/families.json"), serde_json::to_string(&families)?)?;
    m.data.insert("data.families".into(), "data/families.json".into());

    // The animation task VM: every actor's bank tables, and every script.
    let banks = bank_tables(&lib);
    fs::write(out.join("data/banks.json"), serde_json::to_string(&banks)?)?;
    m.data.insert("data.banks".into(), "data/banks.json".into());

    let scripts = match animation_scripts(&src) {
        Ok(Some(set)) => {
            let all = || set.values().flat_map(|s| &s.code);
            println!(
                "task VM: {} scripts, {} part records, {} commands",
                set.len(),
                all().filter(|i| matches!(i, Instr::Part(_))).count(),
                all().filter(|i| !matches!(i, Instr::Part(_) | Instr::EndFrame { .. })).count(),
            );
            fs::write(out.join("data/scripts.json"), serde_json::to_string(&set)?)?;
            m.data.insert("data.scripts".into(), "data/scripts.json".into());
            set
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

    fs::write(out.join("data/actors.json"), actor_definitions(&scripts, &banks)?)?;
    m.data.insert("data.actors".into(), "data/actors.json".into());

    fs::write(out.join("data/fonts.json"), font_definitions())?;
    m.data.insert("data.fonts".into(), "data/fonts.json".into());

    // The intro. `MINDSCAP` is a PIV with no extension, so nothing had picked
    // it up; `INTRO.STI` is the tile map behind the opening pan, and the cast
    // comes out of `INTR.EXE` once its image is expanded.
    match bake_intro(&lib, &out, &mut m) {
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
            fs::write(out.join("data/overworld.json"), serde_json::to_string(&land)?)?;
            m.data.insert("data.overworld".into(), "data/overworld.json".into());
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
            println!("lairs: {} read out of ForestLairs, LairLocation and LairType", l.len());
            l
        }
        Ok(None) => anyhow::bail!("lair tables: the image vanished during the bake"),
        Err(e) => return Err(e.context("lair tables")),
    };
    fs::write(out.join("data/places.json"), place_definitions(&icons, &marks, &lairs))?;
    m.data.insert("data.places".into(), "data/places.json".into());

    fs::write(out.join("data/items.json"), item_definitions())?;
    m.data.insert("data.items".into(), "data/items.json".into());

    fs::write(out.join("data/knights.json"), knight_definitions())?;
    m.data.insert("data.knights".into(), "data/knights.json".into());

    fs::write(out.join("data/palette-effects.json"), palette_effects())?;
    m.data.insert("data.palette.effects".into(), "data/palette-effects.json".into());

    fs::write(out.join("data/music-places.json"), music_places())?;
    m.data.insert("data.music.places".into(), "data/music-places.json".into());

    // The tunes, if `tools/tunes.py` has been run. Music is derived data like
    // everything else here, so it goes in the pack and never into the tree.
    match bake_music(out, &mut m) {
        Ok(0) => println!(
            "no music: run `python3 tools/tunes.py \"{src}\" research/tunes.json` first"
        ),
        Ok(n) => println!("music: {n} tunes"),
        Err(e) => eprintln!("music: {e:#}"),
    }

    fs::write(out.join("manifest.json"), serde_json::to_string_pretty(&m)?)?;
    println!(
        "baked {} sheets, {} sounds, {} palettes, {} data blobs into {}",
        m.sheets.len(), sounds, m.palettes.len(), m.data.len(), out.display()
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
            x: x as u32, y: y as u32, w: w as u32, h: h as u32,
            // Anchor at the bottom centre: this game positions everything by feet.
            ox: -((w / 2) as i32), oy: -(h as i32),
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
    Packed { width, height, pixels, rects }
}

fn write_indexed(
    path: &Path, w: usize, h: usize, pixels: &[u8], palette: &[u32],
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
fn bank_tables(lib: &Library) -> BTreeMap<String, BankTables> {
    let mut cache: BTreeMap<String, Bank> = BTreeMap::new();
    let mut out = BTreeMap::new();
    for (creature, slots) in CREATURE_BANKS {
        let mut tables = BankTables::new();
        for (n, files) in [(1u8, KNIGHT_BANKS), (2, *slots), (3, TABLE3_BANKS), (4, TABLE4_BANKS)] {
            let banks: Vec<Bank> = files.iter().map(|f| bank_of(lib, f, &mut cache)).collect();
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
            let banks: Vec<Bank> =
                DRAGON_FLIGHT_BANKS.iter().map(|f| bank_of(lib, f, &mut cache)).collect();
            if banks.iter().any(|b| !b.cels.is_empty()) {
                tables.insert(5, banks);
            }
        }
        out.insert(creature.to_string(), tables);
    }
    out
}

/// One bank: which sheet the baker packed it into, where in that sheet it
/// starts, and how big each of its cels is.
///
/// The sizes matter to the simulation, not only to the renderer: a mirrored
/// part is placed at `task_x - (x + cel_width)`, so a width is geometry.
fn bank_of(lib: &Library, file: &str, cache: &mut BTreeMap<String, Bank>) -> Bank {
    if file.is_empty() {
        return Bank::default();
    }
    if let Some(b) = cache.get(file) {
        return b.clone();
    }
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
            Bank { sheet: format!("actor.{actor}"), base, cels }
        }
        None => {
            let stem = file.split('.').next().unwrap_or(file).to_lowercase();
            Bank { sheet: format!("bank.{stem}"), base: 0, cels: sizes(file).unwrap_or_default() }
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
fn animation_scripts(src: &str) -> anyhow::Result<Option<ScriptSet>> {
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
    let find = |c: &[String]| c.iter().filter(|p| !p.is_empty()).find_map(|p| fs::read(p).ok());
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
    Ok(Some(set))
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
        let Some(script) = all.get(&name) else { continue };
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
    let Some(script) = scripts.get(standing) else { return [0, 0] };
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
fn body_extent(scripts: &ScriptSet, banks: &BankTables, table: u8, standing: &str) -> Option<[i32; 4]> {
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
/// **The frame lists are gone.** They used to live here, chosen by eye out of
/// `KN1.OB`: an eight frame walk, a four frame swing with hit lines drawn by
/// feel, and a collapse. What replaces them is the original's own scripts,
/// running on the task VM in `henge-core`, and with them come things no hand
/// authored list had: the knight is composed of several parts a frame rather
/// than one sprite, his sword is a separate cel that follows his hand, a blow
/// he takes carries `TASKDEAD` so it turns into a death by itself if it was the
/// last one he could take, and his swing announces itself with the original's
/// own `KnightGruntSound` and sample 0x0b.
///
/// The numbers that are still ours are the ones that were never in the scripts:
/// how fast he walks, how far he reaches, how long an opponent waits between
/// swings, and what he is carrying. Those are combat tuning, not animation.
///
/// Without an unpacked `MAIN.EXE` there are no scripts, and the knight is
/// written out with none. That is deliberate: a second, hand authored set kept
/// beside the real one is exactly what this item was for removing.
fn actor_definitions(
    scripts: &ScriptSet,
    banks: &BTreeMap<String, BankTables>,
) -> anyhow::Result<String> {
    let knight_banks = banks.get("knight").cloned().unwrap_or_default();
    let roots: Vec<&str> = KNIGHT_SCRIPTS
        .iter()
        .flat_map(|(_, v)| v.iter().copied())
        .chain(KNIGHT_ATTACKS.iter().map(|(_, s, _)| *s))
        .chain(KNIGHT_HURT.iter().map(|(_, s)| *s))
        .chain(KNIGHT_FINISHES.iter().map(|(_, s)| *s))
        .chain(KNIGHT_SPAWNED.iter().copied())
        .collect();
    let animation = closure_of(scripts, &roots);
    let mut def = ActorDef {
        sheet: "actor.knight".into(),
        name: "Knight".into(),
        health: 100,
        speed_x: 2,
        speed_y: 1,
        reach: 38,
        depth_tolerance: 6,
        attack_cooldown: 45,
        // The tracker's ranges from `SetKnightSwTables`: the knight stops
        // closing at a hundred pixels and gives ground inside eighty, on a
        // plane four deep. Carried for the behaviour work; `reach` above is
        // what the plain opponent uses today.
        approach: 100,
        back_off: 80,
        // What a fallen knight is carrying, for whoever is left standing. Not
        // recovered: the original names `BESTOWGOLD` and a `GOLD` readout but
        // no table of what anything is worth, so this is a number chosen
        // against the prices. Three foes put down pays for a flask and leaves
        // change.
        bounty: 15,
        // `BKwon`: a knight put down is one point of experience.
        experience: 1,
        body: [-9, 0, 9, 50],
        // Wider than the hit box on purpose. The hit box is narrow so that a
        // strike has to be aimed; the girth is roughly the drawn figure, so
        // four knights in one arena stand beside each other rather than inside
        // each other. Median standing frame in KN1.OB is 29 wide.
        girth: 28,
        origin: origin_of(&animation, &knight_banks, 1, "Knight_SwStance"),
        // One stride of `Knight_SwWalkOn` covers about 47 pixels in four
        // frames, and he walks two pixels a tick. See `ActorDef::script_ticks`.
        script_ticks: 6,
        banks: knight_banks,
        bank_table: 1,
        ..ActorDef::default()
    };
    for (state, names) in KNIGHT_SCRIPTS {
        def.scripts.insert(state.to_string(), names.iter().map(|n| n.to_string()).collect());
    }
    for (kind, script, damage) in KNIGHT_ATTACKS {
        def.attacks.insert(kind.to_string(), AttackDef { script: script.to_string(), damage: *damage });
    }
    def.attack = "swing".into();
    def.hurt_by = KNIGHT_HURT.iter().map(|(k, s)| (k.to_string(), s.to_string())).collect();
    def.blocks = KNIGHT_BLOCKS.iter().map(|(k, g)| (k.to_string(), g.to_string())).collect();
    def.finishes = KNIGHT_FINISHES.iter().map(|(k, s)| (k.to_string(), s.to_string())).collect();
    def.blockable = true;
    // `CONTROLTABLE[6]` is `ControlKnight`, which reads a joystick. A knight
    // in a seat the machine plays gets the plain opponent, which closes and
    // swings and struggles out of a hold.
    def.controller = "knight".into();
    def.animation = animation;
    if !def.animation.is_empty() {
        def.validate().map_err(|e| anyhow::anyhow!("knight: {e}"))?;
    }
    if def.animation.is_empty() {
        eprintln!("the knight has no animation: bake with an unpacked MAIN.EXE image");
    }
    let mut actors = BTreeMap::from([("knight".to_string(), def)]);
    if !scripts.is_empty() {
        for c in CREATURES {
            let def = creature_definition(c, scripts, banks)?;
            actors.insert(c.id.to_string(), def);
        }
    }
    Ok(serde_json::to_string(&actors)?)
}

/// One creature, built the same way the knight is: the closure of the
/// scripts its states reach, its loader's bank tables, and an origin, a hit
/// box and a girth read off its own standing frame.
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
) -> anyhow::Result<ActorDef> {
    let tables = banks
        .get(c.banks)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("{}: no bank tables for loader {}", c.id, c.banks))?;
    // `+0x12`, the recovery script every creature's `*Hit` hands over the
    // moment its blow lands: the stance for all but the beast and Balok.
    let recover = match c.id {
        "beast" => "Beast_TurnAround",
        "balok" => "Balok_Recover",
        _ => c.idle[0],
    };
    let roots: Vec<&str> = [c.idle, c.walk, c.attack, c.hurt, c.death]
        .iter()
        .flat_map(|v| v.iter().copied())
        .chain(c.alternates.iter().map(|(_, s, _)| *s))
        .chain(c.rows.iter().flat_map(|(_, v)| v.iter().copied()))
        .chain(c.spawns.iter().copied())
        .chain(c.hurt_by.iter().map(|(_, s)| *s))
        .chain(c.bleeds.then_some(BLOOD))
        .chain(std::iter::once(recover))
        .collect();
    let animation = closure_of(scripts, &roots);
    // Every creature's tables routine stores table 2 into `+0x18`.
    let table = 2u8;
    let stance = c.idle[0];
    let origin = origin_of(&animation, &tables, table, stance);
    // The hit box is the standing frame's `BODY` parts, narrowed to their
    // middle half the way the knight's was authored (his stance is 36 wide
    // and his box is 18), and as tall as those parts. The girth is three
    // quarters of the drawn width, which is the knight's 28 against 36.
    let extent = body_extent(&animation, &tables, table, stance)
        .ok_or_else(|| anyhow::anyhow!("{}: {stance} has no BODY parts to size a box from", c.id))?;
    let [l, t, r, b] = extent;
    let (w, mid) = (r - l, (l + r) / 2);
    let feet = -(origin[1] as i32);
    let body = [
        (mid - w / 4) as i16,
        (feet - b).max(0) as i16,
        (mid + w / 4) as i16,
        (feet - t) as i16,
    ];
    let mut def = ActorDef {
        sheet: c.sheet.into(),
        name: c.name.into(),
        health: c.health,
        damage: c.damage,
        speed_x: c.speed[0],
        speed_y: c.speed[1],
        reach: c.reach,
        depth_tolerance: c.depth,
        attack_cooldown: 45,
        approach: c.approach,
        back_off: c.back_off,
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
        body,
        girth: if c.girth > 0 { c.girth } else { w * 3 / 4 },
        origin,
        // Six ticks a frame, as the knight: every `InitKnightvs*` routine also
        // writes 6 into `DELAY`, though nothing in the image reads it back.
        script_ticks: 6,
        banks: tables,
        bank_table: table,
        animation,
        moon: c
            .moon
            .iter()
            .map(|(phase, health, damage)| {
                ((*phase).to_string(), henge_core::content::MoonStat {
                    health: *health,
                    damage: *damage,
                })
            })
            .collect(),
        ..ActorDef::default()
    };
    for (state, names) in [
        ("idle", c.idle),
        ("walk", c.walk),
        ("attack", c.attack),
        ("hurt", c.hurt),
        ("death", c.death),
    ] {
        def.scripts.insert(state.to_string(), names.iter().map(|n| n.to_string()).collect());
    }
    def.scripts.insert("recover".into(), vec![recover.to_string()]);
    def.attacks.insert(
        c.kind.to_string(),
        AttackDef { script: c.attack[0].to_string(), damage: c.damage },
    );
    for (kind, script, damage) in c.alternates {
        def.attacks
            .insert(kind.to_string(), AttackDef { script: script.to_string(), damage: *damage });
    }
    for (row, names) in c.rows {
        def.scripts.insert(row.to_string(), names.iter().map(|n| n.to_string()).collect());
    }
    def.controller = c.controller.to_string();
    def.border = c.border;
    def.attack = c.kind.to_string();
    def.hurt_by = c.hurt_by.iter().map(|(k, s)| (k.to_string(), s.to_string())).collect();
    def.blockable = c.blockable;
    def.bleeds = c.bleeds;
    def.validate().map_err(|e| anyhow::anyhow!("{}: {e}", c.id))?;
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
    let Some(bytes) = unpacked_image(src) else { return Ok(None) };
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
/// own change and not this one; until somebody makes it, what is below stands.
///
/// The healer and the stones have no recovered coordinates at all. They are
/// placed on the landmarks the map already draws.
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
    let Some(bytes) = unpacked_image(src) else { return Ok(None) };
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
        let guardian = GUARDIANS.iter().find(|(s, _)| *s == slot).map(|(_, id)| *id);
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
        lairs.push(LairPlace { guardian, count: count as u32, x, y, code: code as u8 });
    }
    for (n, a) in lairs.iter().enumerate() {
        anyhow::ensure!(
            !lairs[n + 1..].iter().any(|b| b.x == a.x && b.y == a.y),
            "two lairs stand at ({}, {})", a.x, a.y
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
    let Some(bytes) = unpacked_image(src) else { return Ok(None) };
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
/// **Every coordinate on the map is recovered now except the hermit's.**
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
/// southern woods this project called the hermit's is Stonehenge, and the ring
/// in the middle of it all this project called Stonehenge is the Valley of the
/// Gods. Both were sited on the right artwork under the wrong name.
///
/// The two towns keep the `KnightGoesToTown` construction as a fallback, so a
/// pack baked without the unpacked image still has somewhere to buy a sword.
/// Everything else the table names is baked only when the table is there.
///
/// **The four villages are recovered and not baked.** Frames 0x15 to 0x18 are
/// in the table at (18, 11), (286, 11), (0, 187) and (303, 192), one in each
/// corner, and `MOON:CheckGROOC` gates each on `[di+0x20]`, the knight's own
/// index, so a village belongs to one knight and only he may enter it. Nothing
/// in this project has a village to enter yet, and putting four unguarded ones
/// on the map would be worse than leaving them off.
///
/// **The hermit is ours, and now has no landmark behind it.** It used to stand
/// on the ruin in the southern woods, which turns out to be Stonehenge's own
/// artwork; it has been moved off it into the deep woods to the west, clear of
/// every recovered box, because a second healer that is not in the original
/// should not sit on top of a place that is.
///
/// **The menu lines a map gadget carries are recovered**, from `_MAP`:
/// `knhigh` `Enter the city of Highwood`, `knwater` `Enter the city of
/// Waterdeep`, `knhenge` `Enter Stonehenge`, `knmath` `Visit Math the Wizard`
/// and `knlair` `Enter Lair`. Those are the words the original puts on its own
/// list when you are standing on one, so they are the words used here.
///
/// **A stall is a room, not a menu line.** The merchant, the tavern, the town
/// healer, the temple and the mystic are all their own places, marked `hidden`
/// so walking can never find one, reached through the town's own menu and
/// leaving back into it. That keeps a town's front door short and gives each
/// room a box wide enough for what it has to say.
///
/// **Two prices.** The hermit in the woods takes only days. The town healer
/// takes coin and gives it all to whatever it will buy, which is `HealDon`.
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
        (goal.0 + 4 - size.0 / 2, goal.1 + 5 - size.1 / 2, size.0, size.1)
    };
    // A place whose corner `MapIconsTABLE` gives and whose size `MI.C` does.
    let mark = |frame: u8, size: (i32, i32)| {
        marks.get(&frame).map(|(x, y)| (*x, *y, size.0, size.1))
    };
    let highwood = mark(0x19, icon(0x19, (25, 32)))
        .unwrap_or_else(|| at((94, 47), icon(0x19, (25, 32))));
    let waterdeep = mark(0x1a, icon(0x1a, (32, 28)))
        .unwrap_or_else(|| at((297, 157), icon(0x1a, (32, 28))));
    let stones = mark(0x1b, icon(0x1b, (18, 12)));
    let valley = mark(0x1c, icon(0x1c, (13, 10)));
    let wizard = mark(0x1e, icon(0x1e, (7, 20)));
    // **Ours.** The hermit is not in the original at all, so no table places
    // him. He stands deep in the southern woods, clear of every recovered box
    // and of Stonehenge in particular, which is where he used to stand.
    let healer = (58, 170, 10, 10);

    let heal = |days: u32, gold: u32| {
        serde_json::json!({
            "do": "heal", "days": days, "gold": gold,
            "said": "Rest well. You are whole again.",
            "refused": "You are unmarked. Keep your days.",
            "too_poor": "I keep no man for nothing."
        })
    };
    let leave = serde_json::json!({ "do": "leave" });
    let go = |place: &str| serde_json::json!({ "do": "go", "place": place });
    let buy = |item: &str| {
        serde_json::json!({
            "do": "buy", "item": item,
            "said": "A fair trade. Keep it dry.",
            "too_dear": "Come back when your purse is heavier.",
            "no_room": "You are carrying all you can."
        })
    };
    let drink = |item: &str| {
        serde_json::json!({
            "do": "use", "item": item,
            "said": "You drain it, and the ache goes out of you.",
            "refused": "You have none, or no need of one."
        })
    };
    let sell = |item: &str| serde_json::json!({ "do": "sell", "item": item });
    let donate = |gold: u32| serde_json::json!({ "do": "donate", "gold": gold });
    let consult = |gold: u32| serde_json::json!({ "do": "consult", "gold": gold });
    let wager = |stake: u32, room: &str| {
        serde_json::json!({ "do": "wager", "stake": stake, "room": room })
    };

    let mut places = serde_json::Map::new();

    // A stall sells the same goods wherever it stands; only the box moves,
    // because it has to sit where that particular painting has room.
    let stall = |name: &str, scene: &str, menu: serde_json::Value, back: &str| {
        serde_json::json!({
            "name": name,
            "scene": scene,
            "hidden": true,
            "x": 0, "y": 0, "w": 0, "h": 0,
            "menu": menu,
            // The goods are the original's merchant's own list, `pu1`..`pu17`,
            // and the henge flask and draught beside them. Casting is done on
            // the character sheet, as the original does it on the status
            // screen, so a stall only sells.
            "options": [
                { "label": "Flask of healing",      "effect": buy("flask") },
                { "label": "Draught of life",       "effect": buy("elixir") },
                { "label": "Potion of healing",     "effect": buy("potion") },
                { "label": "Broad sword",           "effect": buy("broad_sword") },
                { "label": "Claymore sword",        "effect": buy("claymore") },
                { "label": "Sword of Sharpness",    "effect": buy("sword_of_sharpness") },
                { "label": "Chain mail",            "effect": buy("chain_mail") },
                { "label": "Plate armour",          "effect": buy("plate_armour") },
                { "label": "Battle armour",         "effect": buy("battle_armour") },
                { "label": "Gem of seeing",         "effect": buy("gem_of_seeing") },
                { "label": "Ring of protection",    "effect": buy("ring_of_protection") },
                { "label": "Scroll of Haste",       "effect": buy("scroll_of_haste") },
                { "label": "Scroll of the Hawk",    "effect": buy("scroll_of_the_hawk") },
                { "label": "Scroll of Protection",  "effect": buy("scroll_of_protection") },
                { "label": "Drink a flask",         "effect": drink("flask") },
                { "label": "Drink a draught",       "effect": drink("elixir") },
                { "label": "Back",                  "effect": go(back) }
            ]
        })
    };

    // The tavern, over `TAV.PIV`, whose own painted panel is the five stakes
    // and an exit: `_TAVERN` gives its gadgets `[si+0x10]` of one to five and
    // the picture writes `1 gold` to `5 gold` beside them. The box is put
    // exactly over that panel so the live list replaces the painted one.
    let tavern = |town: &str| {
        let room = format!("{town}.dice");
        serde_json::json!({
            "name": "Tavern",
            "scene": "scene.tav",
            "hidden": true,
            "x": 0, "y": 0, "w": 0, "h": 0,
            "menu": [258, 0, 62, 200],
            "options": [
                { "label": "1 gold", "effect": wager(1, &room) },
                { "label": "2 gold", "effect": wager(2, &room) },
                { "label": "3 gold", "effect": wager(3, &room) },
                { "label": "4 gold", "effect": wager(4, &room) },
                { "label": "5 gold", "effect": wager(5, &room) },
                { "label": "Exit",   "effect": go(town) }
            ]
        })
    };

    // The dice table, `DICE.PIV`. The three faces go on top of the picture
    // where `RollDice` blits them, and the words on the plank beside them,
    // which is where `BETLOSER`, `PLAYERPOT` and `CONT` are written: x 174,
    // y 126, 140 and 152. `Press fire to continue` is `_TAVERN:CONT`.
    let dice = |town: &str| {
        serde_json::json!({
            "name": "Dice",
            "scene": "scene.dice",
            "hidden": true,
            "dice": true,
            "x": 0, "y": 0, "w": 0, "h": 0,
            "text": [164, 112, 144, 34],
            "menu": [164, 148, 144, 30],
            "options": [
                { "label": "Press fire to continue",
                  "effect": go(&format!("{town}.tavern")) }
            ]
        })
    };

    // The town healer, `HEA.PIV`. `HealDon` takes the whole donation and
    // spends it down: ten mends every wound, fifteen buys a life point, and
    // what is left over stays in his pot. The three amounts offered are ours,
    // because the original's gadget adds and subtracts a coin at a time; the
    // greeting is `HT1a`..`HT1c` verbatim.
    let town_healer = |town: &str| {
        serde_json::json!({
            "name": "Healer",
            "scene": "scene.hea",
            "hidden": true,
            "x": 0, "y": 0, "w": 0, "h": 0,
            "menu": [6, 88, 136, 58],
            "text": [4, 148, 312, 40],
            "intro": "Good Day Sir Knight, would you care for a healing.  I have the best roots, herbs and leeches on this side of the land. I am at your service for a small donation",
            "options": [
                { "label": "Donate",  "effect": donate(10) },
                { "label": "Donate",  "effect": donate(25) },
                { "label": "Donate",  "effect": donate(50) },
                { "label": "Back",    "effect": go(town) }
            ]
        })
    };

    for (town, scene, stall_menu) in [
        ("highwood", "scene.highwood", serde_json::json!([140, 0, 178, 200])),
        ("waterdeep", "scene.waterdee", serde_json::json!([2, 0, 178, 200])),
    ] {
        places.insert(
            format!("{town}.merchant"),
            stall("Merchant", scene, stall_menu.clone(), town),
        );
        places.insert(format!("{town}.tavern"), tavern(town));
        places.insert(format!("{town}.dice"), dice(town));
        places.insert(format!("{town}.healer"), town_healer(town));
    }

    // The temple. `_STATUS:TTemple` and `SellToTemple` are a gadget list of
    // `se7`..`se18`, one per magic slot, and `GoldSell` pays half the price the
    // merchant asks. The original draws it over whatever screen is up, and
    // there is no temple picture in the game files, so it is a room on the
    // town's own art like the stall next door. That much is ours.
    places.insert(
        "highwood.temple".into(),
        serde_json::json!({
            "name": "Temple",
            "scene": "scene.highwood",
            "hidden": true,
            "x": 0, "y": 0, "w": 0, "h": 0,
            "menu": [140, 0, 178, 200],
            "options": [
                { "label": "Sell Potion of healing",     "effect": sell("potion") },
                { "label": "Sell Gem of seeing",         "effect": sell("gem_of_seeing") },
                { "label": "Sell Sword of Sharpness",    "effect": sell("sword_of_sharpness") },
                { "label": "Sell Ring of protection",    "effect": sell("ring_of_protection") },
                { "label": "Sell Talisman",              "effect": sell("talisman_of_the_wyrm") },
                { "label": "Sell scroll of Haste",       "effect": sell("scroll_of_haste") },
                { "label": "Sell scroll of the Hawk",    "effect": sell("scroll_of_the_hawk") },
                { "label": "Sell scroll of Aquisition",  "effect": sell("scroll_of_acquisition") },
                { "label": "Sell scroll of the Wyrm",    "effect": sell("scroll_of_the_wyrm") },
                { "label": "Sell scroll of Protection",  "effect": sell("scroll_of_protection") },
                { "label": "Sell Flask of healing",      "effect": sell("flask") },
                { "label": "Back",                       "effect": go("highwood") }
            ]
        }),
    );

    // The mystic, `MYS.PIV`. `MysticUpDown` rolls against `DonationTAB`, six
    // records of a threshold and a signed delta, so a bigger donation buys
    // better odds; the three amounts are picked one to a band. The greeting is
    // `MY1a`..`MY1c` verbatim.
    places.insert(
        "waterdeep.mystic".into(),
        serde_json::json!({
            "name": "Mystic",
            "scene": "scene.mys",
            "hidden": true,
            "x": 0, "y": 0, "w": 0, "h": 0,
            "menu": [6, 88, 136, 58],
            "text": [4, 148, 312, 40],
            "intro": "Welcome my child.  I am here to help you in your quest I have the powers to reach into the cosmos and give your body new skills and agility.",
            "options": [
                { "label": "Donate", "effect": consult(5) },
                { "label": "Donate", "effect": consult(25) },
                { "label": "Donate", "effect": consult(50) },
                { "label": "Back",   "effect": go("waterdeep") }
            ]
        }),
    );

    places.insert(
        "highwood".into(),
        serde_json::json!({
            "name": "Highwood",
            "scene": "scene.highwood",
            "x": highwood.0, "y": highwood.1, "w": highwood.2, "h": highwood.3,
            "menu": [256, 0, 62, 200],
            "options": [
                { "label": "Merchant", "effect": go("highwood.merchant") },
                { "label": "Tavern",   "effect": go("highwood.tavern") },
                { "label": "Healer",   "effect": go("highwood.healer") },
                { "label": "Temple",   "effect": go("highwood.temple") },
                { "label": "Leave",    "effect": leave }
            ]
        }),
    );
    places.insert(
        "waterdeep".into(),
        serde_json::json!({
            "name": "Waterdeep",
            "scene": "scene.waterdee",
            "x": waterdeep.0, "y": waterdeep.1, "w": waterdeep.2, "h": waterdeep.3,
            "menu": [2, 0, 62, 200],
            "options": [
                { "label": "Merchant", "effect": go("waterdeep.merchant") },
                { "label": "Tavern",   "effect": go("waterdeep.tavern") },
                { "label": "Healer",   "effect": go("waterdeep.healer") },
                { "label": "Mystic",   "effect": go("waterdeep.mystic") },
                { "label": "Leave",    "effect": leave }
            ]
        }),
    );
    places.insert(
        "healer".into(),
        serde_json::json!({
            "name": "The Healer",
            "scene": "scene.hea",
            "x": healer.0, "y": healer.1, "w": healer.2, "h": healer.3,
            "menu": [6, 88, 154, 58],
            "text": [4, 148, 312, 40],
            "options": [
                { "label": "Tend my wounds", "effect": heal(3, 0) },
                { "label": "Drink a flask",  "effect": drink("flask") },
                { "label": "Leave",          "effect": leave }
            ]
        }),
    );
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
    // 250 health, one monster and `ColourBackDrop` 4. The arena is left empty
    // so the swamp's own rotation picks one, the way the road does.
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
                            "family": "swamp",
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
/// The flask and the draught are ours: they predate the recovered potion
/// and were chosen against each other, a flask about two won fights and a
/// draught about five. The rest is the original's.
///
/// **The ten magic items are recovered**, names, prices and what they do.
/// `MagicName` (DS:`0xe38d`) pairs each slot of a knight's magic record with
/// its name, `pu9`..`pu17` are the merchant's own lines with the price in
/// the text, and `MagicCast` in `_STATUS` is a chain of `cmp bx, slot` that
/// says what each does; the three worn ones are read by the derivation
/// routine at 0x28d and by `CalcDamage`. The two left inert are recovered
/// too and wait on the dragon's set piece: `TalismanWrym` shifts the
/// dragon's fire right once per talisman and floors it at five, and the
/// Scroll of the Wyrm sets `WyrmFLAG` so `KnightWyrm` can send the dragon
/// after a rival. The prices `se*` sells them back for are half.
///
/// **The ids of the ten are the ones `henge_core::service::magic_item` names**,
/// because every bestowal in the game goes through that table: the wizard's
/// gift, what is on a lair's floor, and what the temple will buy back. A pack
/// that files one of them under another id simply never has it handed out, so
/// the two have to agree and the engine's side is the one that cannot move.
/// That is also why the flask, which is ours, is `flask` and not `potion`: the
/// original's own Potion of Healing is slot 0 and has the better claim to it.
///
/// The four keys are here so that one carried out of a lair has a name to be
/// listed under. They are `moon::Key::item` ids, they carry no price, and
/// `moon::is_token` keeps them off every counter in the game.
///
/// It lands in the reference pack for now because it is authored alongside the
/// places that sell it, and those carry the original's own town art.
fn item_definitions() -> String {
    serde_json::json!({
        "flask": {
            "name": "Flask of healing",
            "price": 25,
            "consumed": true,
            "virtue": { "does": "heal", "health": 40 }
        },
        "elixir": {
            "name": "Draught of life",
            "price": 70,
            "consumed": true,
            "virtue": { "does": "heal", "health": 100 }
        },

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
            "virtue": { "does": "inert" }
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
        // Nothing sells these yet: the merchant's list is flasks, and putting
        // swords on it is the economy's business rather than the shell's. They
        // are here because a knight starts wearing two of them and the status
        // panel has to be able to name what they are worth.
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
/// one every computer knight wears.
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
            (((v >> 8) & 0xf) * 17) << 16 | (((v >> 4) & 0xf) * 17) << 8 | (v & 0xf) * 17
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
        knight("SIR GODBER",  [0x00c, 0x009, 0x006], [10, 10]),
        knight("SIR RICHARD", [0xfa0, 0xe70, 0xc50], [300, 5]),
        knight("SIR JEFFREY", [0xae8, 0x6b5, 0x473], [26, 180]),
        knight("SIR EDWARD",  [0xd00, 0x900, 0x500], [300, 185]),
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
/// | `_TAVERN:load_DiceBACK` | 2 | `tune3`, stopped by `LeaveTavern` |
/// | the routine that opens the henge, before `_TAVERN:HengeLOOP` | 1 | `tune2` |
/// | `_WIZARD:LoadWizard` | 1 | `tune2`, stopped at `e5$` |
/// | `_WIZARD:MysticUpDown` | 3 | `tune4`, stopped by `MysticFini` |
/// | `_WIZARD:_bestow_done` | 4 | `tune5` |
///
/// The last is a moment inside the wizard's tower rather than a room of its
/// own, so it is recovered and not wired; see `BUILD_ORDER.md` item 77.
/// Tunes 1 and 6 are never loaded by `MAIN.EXE`: they ship on disk A with the
/// intro and belong to `INTR.EXE`, whose own start call could not be traced to
/// a tune number. **The intro is given tune 1 by elimination**, which is
/// inference and not recovery: disk A holds exactly the intro, the ending and
/// those two tunes, and the intro comes first. The ending's tune 6 waits with
/// the rest of the ending sequence.
fn music_places() -> String {
    serde_json::json!({
        "highwood.dice": "music.tune3",
        "waterdeep.dice": "music.tune3",
        "stones": "music.tune2",
        "wizard": "music.tune2",
        "waterdeep.mystic": "music.tune4",
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
/// The other two glows in the game hang off the fight rather than the screen:
/// `MudmenGlowOn` (entry 14, wired in `main.rs`) and `KnightGlowOn`, which is
/// entries 6, 7 and 8 for a knight down to ten health. See `BUILD_ORDER.md`
/// item 75 for why the second is recovered but not wired.
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
        Some((serde_json::from_str(&scripts).ok()?, serde_json::from_str(&banks).ok()?))
    }

    /// Every creature in the bestiary builds and validates against the real
    /// scripts and bank tables: every script its states name is in the set,
    /// every branch lands, and every part resolves to a cel on the table it
    /// is drawn through. A typo in `CREATURES` fails here by name.
    #[test]
    fn every_creature_in_the_bestiary_is_whole() {
        let Some((scripts, banks)) = baked() else {
            eprintln!("no baked pack under packs/reference: bestiary check skipped");
            return;
        };
        assert!(scripts.len() >= 236, "the pack has {} scripts, expected the full 236", scripts.len());
        let mut ids = Vec::new();
        for c in CREATURES {
            let def = creature_definition(c, &scripts, &banks)
                .unwrap_or_else(|e| panic!("{}: {e:#}", c.id));
            assert!(def.scripted(), "{}: not scripted", c.id);
            assert_eq!(def.bank_table, 2, "{}: creatures draw through table 2", c.id);
            assert!(def.health > 0 && def.damage > 0, "{}: no stat block", c.id);
            assert!(def.origin[1] < 0, "{}: the origin sits above the feet", c.id);
            ids.push(c.id);
        }
        for want in ["troll", "trogg_axe", "trogg_spear", "ratmen", "mudmen", "demon", "beast", "balok", "dragon"] {
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
            assert_eq!((first.left, first.right, first.top), (0, 319, 10),
                "{name}: the first record is the tree line across the whole screen");
            assert!((80..=159).contains(&first.bottom), "{name}: tree line at {}", first.bottom);
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
            assert!(a.terrain.placements.len() > 30,
                "{name}: {} placements, so the walk went out of step again",
                a.terrain.placements.len());
        }
        // `FO7`'s second record is the root mass in the middle of the screen.
        assert_eq!(
            (arenas["fo7"].terrain.borders[1].left,
             arenas["fo7"].terrain.borders[1].right,
             arenas["fo7"].terrain.borders[1].bottom),
            (66, 164, 103)
        );
        // And the deepest of a layout's records is the row its fighters stand
        // below, which is what `FindHalfBORD` measures from.
        assert_eq!(arenas["fo7"].field().floor(), 103);
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
            assert!(known.is_some(), "{}: no controller called {}", c.id, c.controller);
            assert_ne!(
                known,
                Some(Controller::Knight),
                "{} is still fighting like a knight in a costume",
                c.id
            );
            used.insert(c.controller);
        }
        for want in [
            "trogg", "trogg_spear", "troll", "ratman", "mudman", "balok", "beast", "demon",
            "dragon", "claw",
        ] {
            assert!(used.contains(want), "nothing in the bestiary runs {want}");
        }
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
            assert!(prefixes.insert(prefix), "{name}: two families claim {prefix}");
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
        let moors = ARENAS.iter().find(|(n, ..)| *n == "glade").expect("no moors family");
        assert_eq!(moors.1, "gl");
        assert_eq!(moors.3, "GLB1.CMP");
    }

    /// The check is real: misspell one script and the creature is refused,
    /// with the state and the name in the message.
    #[test]
    fn a_misspelt_creature_script_is_refused_by_name() {
        let Some((scripts, banks)) = baked() else { return };
        let mut c = Creature { ..CREATURES[0] };
        c.attack = &["Troll_Chopp"];
        let err = creature_definition(&c, &scripts, &banks).unwrap_err().to_string();
        assert!(err.contains("Troll_Chopp") && err.contains("attack"), "{err}");
    }

    /// Every family's ambush list names creatures the bestiary has.
    #[test]
    fn every_ambush_names_a_creature_in_the_bestiary() {
        for (family, list) in AMBUSHES {
            assert!(!list.is_empty(), "{family} lists nothing");
            for id in *list {
                assert!(CREATURES.iter().any(|c| c.id == *id), "{family} names {id}, which is not in the bestiary");
            }
        }
    }

    /// A stand-in for `MapIconsTABLE`, so the place table can be exercised
    /// without the unpacked executable to read the real one out of. The frames
    /// are the real ones; the corners are not, and are only spread out enough
    /// that nothing lands on anything.
    fn marks() -> BTreeMap<u8, (i32, i32)> {
        [(0x19u8, (10, 10)), (0x1a, (200, 10)), (0x1b, (100, 10)), (0x1c, (150, 40)), (0x1e, (250, 40))]
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
                guardian: GUARDIANS.iter().find(|(s, _)| *s == SLOTS[i % 6]).unwrap().1,
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
            assert!(seen.insert(*slot), "CombatTable slot {slot} is listed twice");
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
            let (got, family, guardian, count) =
                seen.get(&n).unwrap_or_else(|| panic!("no lair numbered {n}"));
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
            assert_eq!(*count, n as u64 + 1, "lair {n} was given another lair's head count");
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
                    assert!(places.contains_key(to), "{id} has a door to {to}, which does not exist");
                }
                if let Some(item) = e.get("item").and_then(|p| p.as_str()) {
                    assert!(items.contains_key(item), "{id} deals in {item}, which the pack has not got");
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
            assert!(items.contains_key(id), "the pack has no {id}, which is magic slot {slot:#x}");
        }
        // And the four keys, so one carried out of a lair has a name.
        for key in henge_core::moon::Key::ALL {
            assert!(items.contains_key(key.item()), "the pack has no {}", key.item());
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
        assert!(boxes.len() >= 24 + 5, "only {} places on the map", boxes.len());
        for (i, a) in boxes.iter().enumerate() {
            assert!(a.1 >= 0 && a.1 + a.3 <= 320, "{} is off the map", a.0);
            assert!(a.2 >= 0 && a.2 + a.4 <= 200, "{} is off the map", a.0);
            for b in &boxes[i + 1..] {
                let apart = a.1 + a.3 <= b.1 || b.1 + b.3 <= a.1
                    || a.2 + a.4 <= b.2 || b.2 + b.4 <= a.2;
                assert!(apart, "{} and {} stand on the same ground", a.0, b.0);
            }
        }
    }
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
fn bake_intro(
    lib: &Library,
    out: &std::path::Path,
    m: &mut Manifest,
) -> anyhow::Result<String> {
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
                    x: 0, y: 0, w: piv::W as u32, h: piv::H as u32, ox: 0, oy: 0,
                }],
            },
        );
        m.palettes.insert("palette.scene.mindscap".into(), p.palette);
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
        write_indexed(&out.join(&file), pan.width, pan.height, &pan.pixels, &a.palette)?;
        m.sheets.insert(
            "scene.intropan".into(),
            Sheet {
                file,
                frames: vec![FrameRect {
                    x: 0, y: 0, w: pan.width as u32, h: pan.height as u32, ox: 0, oy: 0,
                }],
            },
        );
        m.palettes.insert("palette.scene.intropan".into(), a.palette);
        done.push(format!("a {}x{} panorama", pan.width, pan.height));
    }

    match intro_cast() {
        Ok(Some(cast)) => {
            let n = cast.scripts.len();
            fs::write(out.join("data/intro.json"), serde_json::to_string(&cast)?)?;
            m.data.insert("data.intro".into(), "data/intro.json".into());
            done.push(format!("{n} animation scripts"));
        }
        Ok(None) => done.push("no cast (no unpacked INTR.EXE image)".into()),
        Err(e) => done.push(format!("no cast ({e:#})")),
    }

    anyhow::ensure!(!done.is_empty(), "nothing of the intro could be baked");
    Ok(done.join(", "))
}

/// The intro's cast, if the unpacked `INTR.EXE` image is to hand.
fn intro_cast() -> anyhow::Result<Option<henge_core::content::IntroCast>> {
    use henge_formats::introexe;
    let candidates = ["research/intro.final.bin"];
    let Some(raw) = candidates.iter().find_map(|p| fs::read(p).ok()) else {
        return Ok(None);
    };
    let img = introexe::expand(&raw)?;
    introexe::check(&img)?;
    Ok(Some(introexe::cast(&img)?))
}
