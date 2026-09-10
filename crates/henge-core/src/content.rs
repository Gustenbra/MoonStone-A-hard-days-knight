//! On-disk content types. These mirror what the baker writes and what our own
//! content will eventually be authored as, so the game reads one shape either way.

use crate::anim::Sequence;
use crate::arena::{Border, Field, Prop};
use crate::taskvm::{BankTables, ScriptSet};
use crate::wave::WaveDef;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One arena's `.T` file, as the baker writes it out.
///
/// `borders` is the header's own list, and it is a list: four of the fifty six
/// shipped layouts hold more than one rectangle. See `crate::arena`.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TerrainData {
    pub borders: Vec<Border>,
    pub placements: Vec<Prop>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ArenaData {
    pub family: String,
    pub terrain: TerrainData,
}

impl ArenaData {
    /// The ground of this arena: every rectangle its header names.
    pub fn field(&self) -> Field {
        Field::new(self.terrain.borders.clone())
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
    /// routine that does it (image `0x8dec`) first tests the selector against
    /// 4 and keeps `FO2.CMP` when it matches, so an arena of any family draws
    /// part of its scenery from `FO2`. That is the one entry this map holds;
    /// `Family::tile_sheet` says what the other selector values do, and they
    /// do not read this map at all.
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
    /// Which sheet a placement's first byte asks for, and whether it is drawn
    /// at all. `None` means the original draws nothing for it.
    ///
    /// **Recovered.** `Sholoop`, image `0x7ca9`, is the whole of the scenery
    /// walk, and the selector is the first thing it looks at:
    ///
    /// ```text
    /// 07ca9  mov  es, [0x8901]         ; the layout's placement records
    /// 07cad  mov  di, [ShowIndex]
    /// 07cb1  mov  al, es:[di]          ; the selector byte
    /// 07cb4  sub  ah, ah
    /// 07cb6  cmp  ax, 0xff  / je ShoDone   ; end of the list
    /// 07cbb  cmp  ax, 0xfe  / je ShoNext   ; skipped: this one draws nothing
    /// 07cc0  cmp  ax, 3     / je +3
    /// 07cc5  mov  ax, 4                    ; anything else becomes a 4
    /// 07cc8  call 0x8e6a                   ; load the sheet that selects
    /// ```
    ///
    /// and the loader it calls is
    ///
    /// ```text
    /// 08dec  mov  dx, FO2.CMP
    /// 08def  cmp  ax, 4 / je +8            ; a 4 keeps FO2
    /// 08df4  mov  bx, [0x694e]             ; the landscape code
    /// 08df8  mov  dx, [bx + TileTable]     ; the family's own sheet
    /// ```
    ///
    /// So **3 is the family's own sheet, 0xfe draws nothing, and every other
    /// value draws from `FO2`**. The release carries 3, 4, 0xfe and six
    /// placements that carry 1, and those six draw from `FO2` like a 4.
    pub fn tile_sheet(&self, selector: u8) -> Option<&str> {
        match selector {
            0xfe | 0xff => None,
            3 => Some(&self.sheet),
            _ => Some(
                self.tiles
                    .get(&4)
                    .map(String::as_str)
                    .unwrap_or(self.sheet.as_str()),
            ),
        }
    }
}

pub type Arenas = BTreeMap<String, ArenaData>;
pub type Families = BTreeMap<String, Family>;

/// One actor's per-frame walk step, as the image holds it: three rows of
/// `[x, z]` pairs, indexed by the walk cycle.
///
/// The image has six of these tables and no more. `TroggWALKR`/`U`/`D`
/// (DS:0x7746, 0x775e, 0x7772) are loaded by `TroggMove` (0x2e37, 0x2e43,
/// 0x2e4f, 0x2e5b) and serve all three troggs; `BKnightWALKR`/`U`/`D`
/// (0x7bb6, 0x7bca, 0x7bda) by `ControlBlackKnight`'s `M0$`..`M3$` (0x4be6,
/// 0x4bf2, 0x4bfe, 0x4c0a); `TrollWALKR` (0x7ba2) by `TrollMoveL`/`TrollMoveR`
/// (0x5648, 0x5667); and `MudmenWALK` (0x7b8e) by `MudmenMoveR`/`MudmenMoveL`
/// (0x5422, 0x5435). A whole-image scan for every instruction that loads one
/// of their addresses finds those twelve sites and nothing else.
///
/// The person's own knight has the same thing under different names,
/// `K_WalkRValue`/`K_WalkUpValue`/`K_WalkDownValue` (0x77fe, 0x7808, 0x7810),
/// which `ControlKnight` reads as three separate word tables rather than as
/// pairs. `BKnightWALK*` is the proof that the two are the same mechanism:
/// its rows are `(25,0) (3,0) (23,0) (4,0)`, `(0,2) (0,9) (0,2) (0,9)` and
/// `(0,8) (0,2) (0,9) (0,2)` — the knight's own three tables, pair by pair.
///
/// Everything else moves from its own scripts, not from a controller step:
/// the beast charges and wraps at the screen edge (`BeastCharge` 0x2fec,
/// `BeastChargeLeft` 0x2ffe), the ratman leaps (`RatmanLeap` 0x31e7), the
/// Balok jumps (`BalokJumping` 0x3714) and the dragon flies
/// (`ContinueDragon` 0xa5f5).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct WalkSpeed {
    /// Walking sideways. `MoveL` (0x4df5) negates the x it read, so the table
    /// is written rightward and the leftward walk is its mirror.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub right: Vec<[i32; 2]>,
    /// Walking away from the viewer. `MoveU` (0x4e59) negates the z.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub up: Vec<[i32; 2]>,
    /// And towards. `MoveD` does not negate.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub down: Vec<[i32; 2]>,
}

impl WalkSpeed {
    pub fn is_empty(&self) -> bool {
        self.right.is_empty() && self.up.is_empty() && self.down.is_empty()
    }

    /// The step for one frame, given which way the actor is going and where
    /// its walk cycle stands.
    ///
    /// The three rows are not alternatives: `TroggMove` (0x2e31 through
    /// 0x2e5e) tests all four direction bits in turn and calls a mover for
    /// each that is set, so a creature going up and right has both its up
    /// pair and its right pair added, one after the other, in that order.
    /// Each row is indexed modulo its own length, which is `NextWalk`'s mask
    /// and zero-skip (0x4efc, 0x4f0f) expressed as data.
    pub fn step(&self, dx: i32, dz: i32, cycle: usize, flat: (i32, i32)) -> (i32, i32) {
        let pick = |row: &Vec<[i32; 2]>| -> Option<[i32; 2]> {
            if row.is_empty() {
                None
            } else {
                Some(row[cycle % row.len()])
            }
        };
        let (mut x, mut z) = (0, 0);
        // 02e31: `test byte ptr [si+0x26], 8` — up first.
        if dz < 0 {
            match pick(&self.up) {
                // 04e59: `neg word ptr [0x7bf7]`.
                Some([ax, az]) => {
                    x += ax;
                    z -= az;
                }
                None => z -= flat.1,
            }
        } else if dz > 0 {
            // 02e3d: `test byte ptr [si+0x26], 4`.
            match pick(&self.down) {
                Some([ax, az]) => {
                    x += ax;
                    z += az;
                }
                None => z += flat.1,
            }
        }
        // 02e49 and 02e55: right, then left, both off the same row.
        if dx != 0 {
            match pick(&self.right) {
                // 04df5: `neg word ptr [0x7bf5]` is the whole of `MoveL`.
                Some([ax, az]) => {
                    x += ax * dx.signum();
                    z += az;
                }
                None => x += flat.0 * dx.signum(),
            }
        }
        (x, z)
    }
}

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
    /// Pixels per **displayed frame**, for the actors whose own step the image
    /// does not hold a table for. See [`ActorDef::walk_speed`]: where there is
    /// a table, these are not read.
    pub speed_x: i32,
    pub speed_y: i32,
    /// The per-frame walk step, by walk-cycle index: the right row, the up row
    /// and the down row, each entry `[x, z]`.
    ///
    /// **Nothing in the original moves by a flat speed applied every tick.** A
    /// controller runs once per displayed frame and moves once, by the entry
    /// its own walk cycle is on. `MoveL`/`MoveR` (0x4dd5, 0x4e09) and
    /// `MoveU`/`MoveD` (0x4e3d, 0x4e64) all do the same four instructions:
    ///
    /// ```text
    /// 04e0e  mov al, byte ptr [si + 0xa]   ; the walk cycle
    /// 04e11  shl ax, 1
    /// 04e13  shl ax, 1                     ; times four: (x, z) pairs
    /// 04e17  add di, ax                    ; into the table the caller chose
    /// 04e19  mov ax, word ptr [di]         ; this frame's x step
    /// 04e1b  mov word ptr [0x7bf5], ax
    /// 04e1e  mov ax, word ptr [di + 2]     ; and its z step
    /// ```
    ///
    /// and `MonsterWalk` (0x4eeb, 0x4ef1) adds the pair once. The cycle is
    /// `[si+0xa]`, which `NextWalk` (0x4ef7) advances and masks with 7,
    /// skipping any index whose walk *script* row is zero — so the number of
    /// entries a table has is the number of scripts in the matching row, and
    /// [`ActorDef::sequences`] is what decides it, exactly as in the image.
    ///
    /// A row left empty means this actor has no table of that kind and falls
    /// back to `speed_x`/`speed_y` for that axis, which is what the troll
    /// (flat +/-5 depth, `ControlTroll` 0x5620 and 0x5635), the mudmen (flat
    /// +/-2, `MudmenMoveU`/`MudmenMoveD` 0x5447 and 0x5451) and the demon
    /// (flat +/-5 both ways, `DemonMove` 0x4fe2 through 0x5001) actually do.
    #[serde(default)]
    pub walk_speed: WalkSpeed,
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
    /// What putting this kind of fighter down is worth in experience.
    ///
    /// The original pays one for a knight (`BKwon`), two for the dragon and
    /// nothing for a creature met on the road, whose worth is the lair it
    /// guards (`LairWon`, one). Here the road is where the fights are, so a
    /// creature carries a figure of its own, and the baker says which are
    /// the original's and which are ours.
    #[serde(default)]
    pub experience: u32,
    /// Body box relative to the feet: [x_min, y_min, x_max, y_max], y upward.
    pub body: [i16; 4],
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
    /// each time the last has ended.
    ///
    /// **Recovered, and read at bake time.** The tables are `BSS`, filled by
    /// `SetKnightAnims` (0x1771) and `SetMonsterAnims` (0x186b), and each
    /// actor's record points at its own through `Set*Tables`; the baker runs
    /// those routines. `idle` is the record's `+0x10`, `recover` its `+0x12`,
    /// `walk`, `walk_up` and `walk_down` the three rows of its `*Wal` table
    /// at `+0`, `+0x10` and `+0x20`, which `ControlKnight` (0x3fd6) and
    /// `MoveU`/`MoveD`/`MoveR`/`MoveL` (0x4e39, 0x4e64, 0x4e09, 0x4dd5) pick
    /// between by the direction held: a horizontal bit takes row 0, else
    /// down takes `+0x20`, else up takes `+0x10`. See [`ActorDef::walk_row`].
    /// `hurt` is the `*Hit` entry for a swing and `death` where that script's
    /// own `TASKDEAD` goes. What a pack still chooses is `attack`, the row a
    /// caller with one button plays.
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
    /// The original steps its tasks once per pass of whichever loop is up
    /// (`0x9702`), so this is a count of the ticks of that loop's own clock and
    /// not a conversion between clocks. For a fight that clock is the programmed
    /// 54.6204 Hz timer, because `Combat` at `0x351` holds every pass to a
    /// deadline two ticks of the BIOS counter at `0000:046c` ahead — 109.849 ms,
    /// which is six timer ticks. See `henge_desktop`'s `TIMER_TICK` for the
    /// chain, and `RETRACE_TICK` for the loops that wait on vertical retraces
    /// instead.
    ///
    /// So for everything that fights this is **six**, and six is recovered
    /// rather than derived: thirteen sites write 6 into `DELAY` (`DS:0x91c`) —
    /// the eleven `InitKnightvs*` routines, `InitGameStart` and `InitPractice` —
    /// and nothing anywhere reads it back, because the loop hardcodes the same
    /// duration as those two BIOS ticks.
    ///
    /// It used to say the tick was one vertical retrace at 70.0863 a second, and
    /// to justify the six off the walk instead. That ran every fight 1.2832x
    /// fast: 85.61 ms to a frame where the original spends 109.849.
    #[serde(default = "one_tick")]
    pub script_ticks: u32,
    /// The attacks this actor has, by the name of the attack kind
    /// (`crate::combat::Attack::name`), each with its script and the damage
    /// the original's `*Dam` table gives it.
    ///
    /// **Recovered.** The knight's is `KnightAttSw` and `KnightDamSw`; a
    /// creature's is the scripts its own routine picks and the kind it writes
    /// into `+0x28` when it does. An actor with none here plays
    /// `scripts["attack"]` for every direction, which is how the bestiary
    /// fought before this table existed.
    #[serde(default)]
    pub attacks: BTreeMap<String, AttackDef>,
    /// What `damage` is the figure for, so another attack's blow is its own
    /// `*Dam` entry against this one's. That is `blow_ratio`, and it is the
    /// only thing this field is still needed for.
    ///
    /// It used to be "which of a creature's own attacks the one button gets",
    /// which was ours. It is not needed for that any more: every controller
    /// writes its own kind into `+0x28` and the pack gives each actor exactly
    /// the kinds its routine writes. `ControlBlackKnight` names six (swing 4,
    /// chop 0x10, lunge 2, knife 6, block 8, evade 0xe, at 0x4c87, 0x4cb5,
    /// 0x4cf1, 0x4d0b, 0x4d2b, 0x4d3f), `ControlRatCollide` two (0x3188,
    /// 0x319b), `TroggAttacks` three, and so on down the bestiary. The
    /// fallback `attack_for` still makes is for an actor a pack invents that
    /// asks for a kind it has no script for; none of the original's does.
    #[serde(default)]
    pub attack: String,
    /// The actor record's `+0x35`, which every `Set*Tables` routine writes
    /// and which the original uses to tell one actor from another wherever a
    /// branch needs to.
    ///
    /// It is what `StruckTable` (DS:0x7843) is indexed by: `KnightGotStruck`
    /// (0x4267) does `mov si, [di+0xe]` -- the striker -- and jumps through
    /// the table by *his* kind, so what a blow on a fallen knight does is
    /// the striker's business and not the blow's. It is also what
    /// `TroggFinishKnight` (0x42fc) tests to tell the axe trogg from the
    /// hammer, and what `TroggStruck`'s gate (0x2f31) and `KnifeThrow`
    /// (0x3e3e, the one write outside the eleven `Set*Tables`) read.
    ///
    /// **Zero is a real kind**, the beast's: `SetBeastTables` (0x22f1) writes
    /// it. The knight's 6 does not come from his record at all -- his
    /// `Set*Tables` routine writes no `+0x35` -- but from wherever a fight is
    /// stood up, `InitKnightBattle` (0x408, 0x418) and its two siblings; a
    /// computer knight's seat of the same record gets 8 from `InitGameStart`
    /// (0x1c69 and three more, one per seat), and the two share every
    /// `StruckTable` entry.
    #[serde(default)]
    pub record_kind: u8,
    /// The blow-taken script by the attacker's attack kind: the `*Hit` table,
    /// which `KnightSAnim` and `TroggStruck` index with the attacker's
    /// `+0x28`. Each of these scripts carries its own `TASKDEAD`, so which
    /// death an actor dies is decided here too: a trogg stabbed falls, one cut
    /// at the waist is split. A kind with no entry plays `scripts["hurt"]`.
    #[serde(default)]
    pub hurt_by: BTreeMap<String, String>,
    /// `KnightBloSw`, the block table: the incoming attack kind to the guard
    /// that stops it. `CheckBlock` compares the entry for the attacker's kind
    /// with what the defender is doing, and only a knight has one.
    #[serde(default)]
    pub blocks: BTreeMap<String, String>,
    /// Whether this actor's blows go through `CheckBlock` at all. The knight's
    /// and the troggs' do (`KnightKnightStruck1`, `TroggStruck1`,
    /// `TroggSpearStruck1`); every other creature's, and a thrown dagger's,
    /// land through `KnightStruck1`, which never asks.
    #[serde(default)]
    pub blockable: bool,
    /// What a blow does to this actor's body once it is down, while its frame
    /// still carries `BODY` parts: `collapse` and `decap`. `MudmenStruck1`
    /// decapitates a fallen knight for a swing and collapses him for anything
    /// else; `KnightKnightStruck1` decapitates him for any blow from another
    /// knight.
    #[serde(default)]
    pub finishes: BTreeMap<String, String>,
    /// Whether a blow on this actor spawns `Blood1` at the strike point.
    /// `AddBlood` is called from `TrollStruck`, `BalokStruck` and
    /// `DragonStruck` and from nowhere else.
    #[serde(default)]
    pub bleeds: bool,
    /// What the moon does to this actor, keyed by the phase
    /// (`crate::moon::Phase::key`): the hit points and the blow it is
    /// fielded with on that night instead of `health` and `damage`.
    ///
    /// **Recovered**, for the one creature that has it: `SetRatmenTables`
    /// reads the phase and writes seven and three under 0x2d, twelve and
    /// five under 0x31, over the five and one it wrote a moment before. On
    /// the data so a pack can give the moon to any creature it likes.
    #[serde(default)]
    pub moon: BTreeMap<String, MoonStat>,
    /// Which of the original's controllers this actor runs, by the name
    /// [`crate::monster::Controller`] knows it as.
    ///
    /// **Recovered.** The original dispatches on the actor's kind (`+0x35`)
    /// through `CONTROLTABLE`, which `InitGameStart` fills with `ControlTrogg`,
    /// `ControlTroll`, `ControlRatmen`, `ControlMudmen`, `ControlBalok`,
    /// `ControlBeast`, `ControlDemon`, `ControlDragon`, `ControlClaw` and
    /// `ControlKnight`. Empty means the plain one, which closes and swings.
    #[serde(default)]
    pub controller: String,
    /// A border this actor brings with it, as `[left, right, top, bottom]`.
    ///
    /// **Recovered**, and there is exactly one: `SETDEMONBORD`, the last
    /// routine of `GFX`, writes a single record into the buffer the arena's
    /// `.T` file fills and `SBORD` walks, and sets the deepest border row
    /// with it. It replaces the arena's whole list rather than joining it, so
    /// a fight against the demon is fought on the demon's own ground.
    /// `InitKnightvsDemon` calls it, at image `0x2752`. See `docs/TASKVM.md`.
    #[serde(default)]
    pub border: Option<[i32; 4]>,
    /// Where this actor stands at the opening of a bout, one record per seat,
    /// `[x, y, z, facing]` with the facing as the original writes it: 1 right,
    /// 3 left.
    ///
    /// **Recovered.** `InitNewMO` at image `0x27ee` copies four fields out of an
    /// eight-byte record into the actor record and nothing else:
    ///
    /// ```text
    /// 027fa  mov ax, [si]    ; mov [di+2], ax    x
    /// 027ff  mov ax, [si+2]  ; mov [di+4], ax    y, the height above the ground
    /// 02805  mov ax, [si+4]  ; mov [di+6], ax    z, the depth
    /// 0280b  mov ax, [si+6]  ; mov [di+8], al    facing
    /// ```
    ///
    /// and the tables are `TroggTABLE` (DS:`0x97a`, four records, also the
    /// troll's), `BeastTABLE` (`0x99c`, three), `RatmanTABLE` (`0x9b6`, five),
    /// `MudmanTABLE` (`0x9e0`, five) and `BalokTABLE` (`0xa5c`, one); the demon
    /// and the dragon have theirs written straight into the record by
    /// `InitKnightvsDemon` (`0x278d`) and `InitKnightvsDragon` (`0x2476`).
    ///
    /// Only `x` and `facing` survive the arrival. `AddPlayer` falls into
    /// `AddKnight`, whose store at `0x29b6` writes the rotating standing depth
    /// over `+6`, so the `z` here is dead; and this engine keeps no equivalent
    /// of the record's `+4`, which is zero in every creature's table anyway. The
    /// whole record is carried because the record is what the original holds.
    #[serde(default)]
    pub seats: Vec<[i32; 4]>,
    /// Which of [`ActorDef::seats`] the first of this actor to arrive takes.
    ///
    /// **Recovered**, and it is not always zero. `SetMonsterCombat` (`0x27e4`)
    /// walks the table from the front, so a trogg, a beast, a ratman and Balok
    /// all open on record 0. The troll and the mudmen instead go through
    /// `InitTrogg` (`0x225b`) and `InitMudmen` (`0x2655`), which do
    /// `xor word [SIDE], 1` and step the pointer on by eight when the result is
    /// not zero. `SetUpDKL` (`0x292e`) has just set `SIDE` to 0, so the first
    /// one in takes record **1** and comes in from the other side of the screen.
    ///
    /// Now that `SIDE` itself is modelled (see [`crate::wave`]) this is derived
    /// rather than needed: it is what [`crate::wave::WaveDef::opens_with_side`]
    /// produces for a fight that holds one creature at a time. It is still
    /// carried, and still what the fights with no wave at all are seated from,
    /// because it is what the table says.
    #[serde(default)]
    pub first_seat: usize,
    /// How many of this actor a fight holds, how many of them at once, and when
    /// the next one walks in: the three counts its `InitKnightvs*` writes and
    /// the row of `lev_adjust` that `AdjustLevel` moves them by. See
    /// [`crate::wave`].
    #[serde(default)]
    pub wave: WaveDef,
}

/// An actor's numbers on one night of the moon.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct MoonStat {
    pub health: i32,
    pub damage: i32,
}

/// One attack of an actor: the script it plays and what the `*Dam` table
/// says it takes off, in the same units as [`ActorDef::damage`].
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct AttackDef {
    pub script: String,
    #[serde(default)]
    pub damage: i32,
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
            walk_speed: WalkSpeed::default(),
            record_kind: 0,
            reach: 0,
            depth_tolerance: 0,
            attack_cooldown: 0,
            approach: 0,
            back_off: 0,
            bounty: 0,
            experience: 0,
            body: [0; 4],
            sequences: BTreeMap::new(),
            animation: ScriptSet::new(),
            scripts: BTreeMap::new(),
            banks: BankTables::new(),
            bank_table: one_table(),
            origin: [0, 0],
            script_ticks: one_tick(),
            attacks: BTreeMap::new(),
            attack: String::new(),
            hurt_by: BTreeMap::new(),
            blocks: BTreeMap::new(),
            blockable: false,
            finishes: BTreeMap::new(),
            bleeds: false,
            moon: BTreeMap::new(),
            controller: String::new(),
            border: None,
            seats: Vec::new(),
            first_seat: 0,
            wave: WaveDef::default(),
        }
    }
}

impl ActorDef {
    pub fn sequence(&self, name: &str) -> Option<&Sequence> {
        self.sequences.get(name)
    }

    /// The name a plate prints: the one given, or the id it was filed under.
    pub fn display_name<'a>(&'a self, id: &'a str) -> &'a str {
        if self.name.is_empty() {
            id
        } else {
            &self.name
        }
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
            if self.scripts_for(state).is_empty() {
                return Err(format!("state {state} names no script"));
            }
        }
        // Those five and whatever else is listed, such as the recovery.
        for (state, names) in &self.scripts {
            for n in names {
                if !self.animation.contains_key(n) {
                    return Err(format!(
                        "state {state} names {n}, which is not in the script set"
                    ));
                }
                pending.push((n.clone(), self.bank_table));
            }
        }
        // The tables added with combat depth name scripts too, and a typo in
        // any of them would otherwise be an attack that stands still.
        for (what, name) in self
            .attacks
            .iter()
            .map(|(k, a)| (format!("attack {k}"), &a.script))
            .chain(
                self.hurt_by
                    .iter()
                    .map(|(k, s)| (format!("hurt_by {k}"), s)),
            )
            .chain(
                self.finishes
                    .iter()
                    .map(|(k, s)| (format!("finish {k}"), s)),
            )
        {
            if !self.animation.contains_key(name) {
                return Err(format!(
                    "{what} names {name}, which is not in the script set"
                ));
            }
            pending.push((name.clone(), self.bank_table));
        }
        if !self.attack.is_empty() && !self.attacks.contains_key(&self.attack) {
            return Err(format!(
                "the default attack {} is not in the attack table",
                self.attack
            ));
        }
        for (k, g) in &self.blocks {
            if crate::combat::Attack::from_name(k).is_none()
                || crate::combat::Attack::from_name(g).is_none_or(|a| !a.is_guard())
            {
                return Err(format!(
                    "block table entry {k}: {g} is not an attack kind and a guard"
                ));
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
                    return Err(format!(
                        "{name} branches to {branch}, which is not in the set"
                    ));
                }
                pending.push((branch.clone(), table));
                // A `TASKGOTO` with mode 3 transfers at once (`0x9843`, and
                // `Task::step`'s own `if mode == 3`), so nothing after it in
                // this script is reached from this entry. Walking on past it
                // is how the beast's toss came to be rejected: the decoder
                // reads a script to its terminating `ff ff`, and
                // `Beast_BackToss` runs straight through the bytes of
                // `Beast_ChestToss`, `Beast_KnightDown` and
                // `Beast_ImpaleChest`, whose `TASKCELBUF 1` then makes every
                // later part look as though it came out of the wrong table.
                // Each of those runs is a named script of its own and is
                // checked under its own entry, with the table it is really
                // entered with.
                if matches!(i, Instr::Goto { mode: 3, .. }) {
                    break;
                }
            }
        }
        Ok(())
    }

    /// Which controller this actor runs. An actor that names none, or names
    /// one the engine does not know, gets the plain opponent.
    pub fn controller(&self) -> crate::monster::Controller {
        crate::monster::Controller::from_name(&self.controller)
            .unwrap_or(crate::monster::Controller::Knight)
    }

    /// The rectangle this actor narrows a fight to, if it has one.
    pub fn ground(&self) -> Option<Border> {
        self.border.map(|[left, right, top, bottom]| Border {
            left,
            right,
            top,
            bottom,
        })
    }

    /// Where the `n`th of this actor to arrive stands, as `(x, facing)` with
    /// the facing as this engine keeps it: 1 right, -1 left.
    ///
    /// `first_seat` is where `InitNewMO`'s pointer starts and `n` is how many of
    /// this actor came in before. The wrap is ours: the original never fields
    /// more of anything than its table has records for, and running off the end
    /// of a table there reads the next table's bytes.
    pub fn seat(&self, n: usize) -> Option<(i32, i32)> {
        if self.seats.is_empty() {
            return None;
        }
        let [x, _y, _z, facing] = self.seats[(self.first_seat + n) % self.seats.len()];
        Some((x, if facing == 3 { -1 } else { 1 }))
    }

    /// The same record, by its own index in the table rather than by how many
    /// arrived before.
    ///
    /// This is what the wave machinery uses, because `SIDE` and
    /// `SetMonsterCombat` name a record outright and `first_seat` is the answer
    /// one of them already gave. The wrap is [`ActorDef::seat`]'s.
    pub fn seat_at(&self, index: usize) -> Option<(i32, i32)> {
        if self.seats.is_empty() {
            return None;
        }
        let [x, _y, _z, facing] = self.seats[index % self.seats.len()];
        Some((x, if facing == 3 { -1 } else { 1 }))
    }

    /// Whether this actor is animated by the task VM rather than by frame lists.
    pub fn scripted(&self) -> bool {
        !self.animation.is_empty() && !self.scripts.is_empty()
    }

    /// The scripts a state cycles through, in order.
    pub fn scripts_for(&self, state: &str) -> &[String] {
        self.scripts.get(state).map_or(&[], Vec::as_slice)
    }

    /// The walk row a step plays: `ControlKnight`'s `A1$` to `A4$` (0x3fd6
    /// to 0x4012) and `TroggMove` (0x2e2b) both test up, then down, then
    /// right, then left, and each sets the row offset as it goes, so the
    /// last bit set wins: a horizontal step is drawn on row 0 whatever else
    /// is held, a plain step down on `+0x20`, a plain step up on `+0x10`.
    ///
    /// `TrollWal` and `MudmenWal` have one row, so a row the table has not
    /// got falls back to the right-facing one. The original would read the
    /// next table's bytes as a script there; nothing in it ever does.
    pub fn walk_row(&self, dx: i32, dy: i32) -> &[String] {
        let row = if dx != 0 {
            "walk"
        } else if dy > 0 {
            "walk_down"
        } else if dy < 0 {
            "walk_up"
        } else {
            "walk"
        };
        let names = self.scripts_for(row);
        if names.is_empty() {
            self.scripts_for("walk")
        } else {
            names
        }
    }

    /// The bank a part names, through the table `TASKCELBUF` last selected.
    pub fn bank(&self, table: u8, slot: u8) -> Option<&crate::taskvm::Bank> {
        self.banks
            .get(&table)?
            .get(slot as usize)
            .filter(|b| !b.cels.is_empty())
    }

    /// The script and kind an attack resolves to, the way `KnightAttack`
    /// indexes `KnightAttSw`: the entry for that kind if the actor has one,
    /// else the actor's default attack, else the one `scripts["attack"]`
    /// names, which is then taken to be a swing.
    ///
    /// The fallback is what lets the plain opponent, which only ever asks for
    /// a swing, fight with a creature whose one attack is a lunge or a bite.
    pub fn attack_for(
        &self,
        wanted: crate::combat::Attack,
    ) -> Option<(String, crate::combat::Attack)> {
        use crate::combat::Attack;
        if let Some(a) = self.attacks.get(wanted.name()) {
            return Some((a.script.clone(), wanted));
        }
        if let Some(a) = self.attacks.get(&self.attack) {
            let kind = Attack::from_name(&self.attack).unwrap_or(Attack::Swing);
            return Some((a.script.clone(), kind));
        }
        self.scripts_for("attack")
            .first()
            .map(|s| (s.clone(), Attack::Swing))
    }

    /// The blow-taken script for a blow of that kind, or the one for any blow.
    pub fn hurt_for(&self, by: Option<crate::combat::Attack>) -> Option<String> {
        by.and_then(|a| self.hurt_by.get(a.name()).cloned())
            .or_else(|| self.scripts_for("hurt").first().cloned())
    }

    /// The hit points and blow this actor has under a given moon: the entry
    /// for that phase, or its everyday numbers.
    pub fn under_moon(&self, phase: &str) -> (i32, i32) {
        self.moon
            .get(phase)
            .map_or((self.health, self.damage), |m| (m.health, m.damage))
    }

    /// What an attack takes off against the default attack's figure: the
    /// `*Dam` table entry for it over the entry for [`ActorDef::attack`], so
    /// the knight's rear thrust is half his swing. His chop is the same
    /// entry as the swing, four; it is `CalcDamage` that doubles it, after
    /// adding the sheet. One to one when the actor has no table to say
    /// otherwise.
    pub fn blow_ratio(&self, attack: crate::combat::Attack) -> (i32, i32) {
        let this = self.attacks.get(attack.name()).map_or(0, |a| a.damage);
        let base = self.attacks.get(&self.attack).map_or(0, |a| a.damage);
        if this > 0 && base > 0 {
            (this, base)
        } else {
            (1, 1)
        }
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
    use super::Family;
    use crate::combat::tests::scripted_def;
    use crate::taskvm::{End, Instr, Part, Script};
    use std::collections::BTreeMap;

    /// `Sholoop`'s reading of the placement selector, which is the byte that
    /// says both which sheet a scenery cell comes from and whether it is drawn
    /// at all. The release carries 3, 4, 0xfe and six 1s, and getting 0xfe
    /// wrong put stone slabs and loose foliage over twenty eight arenas.
    #[test]
    fn the_placement_selector_skips_fe_and_sends_everything_but_three_to_fo2() {
        let mut tiles = BTreeMap::new();
        tiles.insert(4u8, "scene.fo2".to_string());
        let f = Family {
            sheet: "scene.fo1".into(),
            backdrop: "scene.fob1".into(),
            tiles,
            arenas: Vec::new(),
        };
        assert_eq!(f.tile_sheet(3), Some("scene.fo1"));
        assert_eq!(f.tile_sheet(4), Some("scene.fo2"));
        // The six placements in the release that carry 1 go the same way a 4
        // does: `cmp ax, 3 / je / mov ax, 4`.
        assert_eq!(f.tile_sheet(1), Some("scene.fo2"));
        assert_eq!(f.tile_sheet(7), Some("scene.fo2"));
        // And these two draw nothing.
        assert_eq!(f.tile_sheet(0xfe), None);
        assert_eq!(f.tile_sheet(0xff), None);
    }

    /// A pack that names no shared sheet still has to draw something, so the
    /// family's own sheet stands in rather than the cell vanishing.
    #[test]
    fn a_family_with_no_shared_sheet_falls_back_to_its_own() {
        let f = Family {
            sheet: "scene.wa1".into(),
            backdrop: "scene.wab1".into(),
            tiles: BTreeMap::new(),
            arenas: Vec::new(),
        };
        assert_eq!(f.tile_sheet(4), Some("scene.wa1"));
        assert_eq!(f.tile_sheet(0xfe), None);
    }

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
                Instr::Part(Part {
                    table: 1,
                    bank: 0,
                    cel: 200,
                    x: 0,
                    y: 0,
                    flags: 0,
                }),
                Instr::EndFrame { end: End::Stop },
            ]),
        );
        let err = d.validate().unwrap_err();
        assert!(err.contains("cel 200"), "{err}");

        let mut d = scripted_def();
        d.animation.insert(
            "stance".into(),
            Script::new(vec![
                Instr::Part(Part {
                    table: 1,
                    bank: 3,
                    cel: 0,
                    x: 0,
                    y: 0,
                    flags: 0,
                }),
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
                Instr::Part(Part {
                    table: 1,
                    bank: 0,
                    cel: 0,
                    x: 0,
                    y: 0,
                    flags: 0,
                }),
                Instr::Goto {
                    mode: 0,
                    target: "walk1".into(),
                },
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

    /// `InitNewMO`'s seat record, read the way this engine needs it: the x and
    /// the facing, with the original's 1 and 3 turned into 1 and -1.
    #[test]
    fn a_seat_gives_the_column_and_the_facing_the_original_wrote() {
        let mut d = super::ActorDef {
            // `TroggTABLE`, DS:0x97a.
            seats: vec![
                [-50, 0, 100, 1],
                [360, 0, 150, 3],
                [340, 0, 50, 3],
                [-80, 0, 120, 1],
            ],
            ..Default::default()
        };
        assert_eq!(d.seat(0), Some((-50, 1)));
        assert_eq!(d.seat(1), Some((360, -1)));
        // Past the end it comes round again, which is ours: the original never
        // fields more of anything than its table holds.
        assert_eq!(d.seat(4), Some((-50, 1)));
        // `InitTrogg` and `InitMudmen` start the pointer one record in.
        d.first_seat = 1;
        assert_eq!(d.seat(0), Some((360, -1)));
        assert_eq!(d.seat(3), Some((-50, 1)));
        // An actor with no table has no opinion, rather than standing at zero.
        assert_eq!(super::ActorDef::default().seat(0), None);
    }
}

/// One frame of an intro animation: how many of the intro's own frames it is
/// held for, and the sprite parts it is made of.
///
/// The intro runs the same task VM the game does, but its `INITTASK` fills one
/// more handler slot, so its opcode numbers are its own; and its scripts use
/// only `TASKHOLD`, `TASKGOTO`, `TASKLOOP`, `TASKGOSUB` and part records. That
/// makes them flattenable at bake time, which is why the intro carries frames
/// rather than a [`ScriptSet`]: nothing in it branches on the state of a fight.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct IntroFrame {
    pub hold: u8,
    pub parts: Vec<crate::taskvm::Part>,
}

/// The intro's cast.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct IntroCast {
    /// Slot to asset id. The routine that starts an intro task loads `bp` with
    /// `0x445d` and the bank table entries are four bytes apart, so a part
    /// record's selector divided by four is the slot.
    pub banks: Vec<String>,
    /// Script offset in `DGROUP`, as `"1583"`, to its flattened frames.
    pub scripts: BTreeMap<String, Vec<IntroFrame>>,
    /// For a script that goes round, the frame it comes back to. A script with
    /// no entry here ends on `ff ff` and is not drawn once it has run out.
    #[serde(default)]
    pub loops: BTreeMap<String, u32>,
}

impl IntroCast {
    /// Which frame of a script is showing `n` of the intro's own frames after
    /// it started.
    ///
    /// A script that runs off its end shows **nothing**. The frame builder at
    /// `0x3fbe` walks the script from the task's own pointer and emits a part
    /// for every record it passes; a task left sitting on `ff ff` reaches
    /// `0x418b` on the first byte and returns having emitted none. So a druid
    /// whose walk is over is gone from the picture, not frozen in the last
    /// pose he struck.
    ///
    /// A script that loops goes round from [`IntroCast::loops`], which is where
    /// its `TASKGOTO` sends it.
    pub fn frame_at<'a>(&'a self, script: &str, n: u32) -> Option<&'a IntroFrame> {
        let frames = self.scripts.get(script)?;
        let span = |f: &[IntroFrame]| -> u32 { f.iter().map(|f| f.hold.max(1) as u32).sum() };
        let total = span(frames);
        let mut left = n;
        if left >= total {
            let from = *self.loops.get(script)? as usize;
            let head = span(frames.get(..from.min(frames.len())).unwrap_or(frames));
            let cycle = total.saturating_sub(head);
            if cycle == 0 {
                return frames.last();
            }
            left = head + (left - head) % cycle;
        }
        for f in frames {
            let hold = f.hold.max(1) as u32;
            if left < hold {
                return Some(f);
            }
            left -= hold;
        }
        frames.last()
    }
}

#[cfg(test)]
mod intro_cast_tests {
    use super::{IntroCast, IntroFrame};
    use crate::taskvm::Part;
    use std::collections::BTreeMap;

    fn frame(cel: u8, hold: u8) -> IntroFrame {
        IntroFrame {
            hold,
            parts: vec![Part {
                table: 1,
                bank: 3,
                cel,
                y: 0,
                flags: 0,
                x: 0,
            }],
        }
    }

    fn cast(frames: Vec<IntroFrame>, loops: Option<u32>) -> IntroCast {
        let mut scripts = BTreeMap::new();
        scripts.insert("2927".to_string(), frames);
        let mut l = BTreeMap::new();
        if let Some(from) = loops {
            l.insert("2927".to_string(), from);
        }
        IntroCast {
            banks: vec!["bank.dw1".into()],
            scripts,
            loops: l,
        }
    }

    /// A script that finishes on `ff ff` is not drawn once it has run out. The
    /// frame builder walks from the task's own pointer and a pointer left on
    /// the `0xff` emits no parts at all, so the forest's druids leave the
    /// screen rather than piling up against its left edge.
    #[test]
    fn a_script_that_has_run_out_shows_nothing() {
        let c = cast(vec![frame(1, 1), frame(2, 1), frame(3, 1)], None);
        assert_eq!(c.frame_at("2927", 0).unwrap().parts[0].cel, 1);
        assert_eq!(c.frame_at("2927", 2).unwrap().parts[0].cel, 3);
        assert!(c.frame_at("2927", 3).is_none(), "it is gone, not frozen");
        assert!(c.frame_at("2927", 900).is_none());
    }

    /// One that loops goes round from where its `TASKGOTO` sends it, so the
    /// standing figures keep standing for as long as the scene lasts and their
    /// torches keep flickering.
    #[test]
    fn a_looping_script_goes_round_from_its_own_jump() {
        let c = cast(vec![frame(1, 1), frame(2, 1), frame(3, 1)], Some(1));
        assert_eq!(c.frame_at("2927", 2).unwrap().parts[0].cel, 3);
        assert_eq!(c.frame_at("2927", 3).unwrap().parts[0].cel, 2);
        assert_eq!(c.frame_at("2927", 4).unwrap().parts[0].cel, 3);
        assert_eq!(c.frame_at("2927", 99).unwrap().parts[0].cel, 2);
    }

    /// A hold counts for as many of the intro's frames as it says.
    #[test]
    fn a_held_frame_stays_up_for_its_whole_hold() {
        let c = cast(vec![frame(1, 4), frame(2, 1)], None);
        for n in 0..4 {
            assert_eq!(c.frame_at("2927", n).unwrap().parts[0].cel, 1);
        }
        assert_eq!(c.frame_at("2927", 4).unwrap().parts[0].cel, 2);
        assert!(c.frame_at("2927", 5).is_none());
    }
}
