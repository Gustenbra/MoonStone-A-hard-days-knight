//! The animation task VM.
//!
//! Moonstone does not hold animations as frame lists. Each actor runs a small
//! bytecode program, and every frame of that program names the sprite parts the
//! actor is made of on that tick. `docs/TASKVM.md` is the reference for the
//! instruction set, the frame record and how each of them was established; this
//! module is that machine, written as a deterministic interpreter.
//!
//! Three properties are deliberate and are what the multiplayer plan rests on.
//!
//! * **Integers only.** No float appears anywhere in the state or in the
//!   arithmetic, so two machines cannot disagree about a rounding.
//! * **No I/O and no clock.** A tick is a call to [`Task::step`]. Nothing here
//!   reads a file, a timer or a random number.
//! * **Nothing is drawn.** A step returns the parts to draw as logical data:
//!   which bank slot, which cel, where, and with which flags. Resolving a slot
//!   to a sheet and a cel to a rectangle is the caller's job, and the data it
//!   needs is [`Bank`], which is content rather than code.
//!
//! Two instructions call out of the machine into the original's own code, and
//! neither has a Rust equivalent:
//!
//! * `TASKGOSUB` is a near call into a named routine. All 41 targets that any
//!   shipped script uses are known by name and are listed in [`GOSUB_TARGETS`].
//!   The interpreter does not fake them and does not skip them silently: it
//!   emits [`Effect::Gosub`] carrying the name and what kind of thing that
//!   routine is, and a caller that has not implemented one has a recorded
//!   no-op rather than a hole. The 23 of them that play a sample are
//!   transcribed in [`crate::sound`].
//! * `TASKSOUND` names a sound id. The interpreter emits [`Effect::Sound`]; it
//!   has no idea what sound is. The handler is at image `0x9b38`:
//!   `mov al, [si+1]` and straight into `PLAY_SFX` (`0x5964`), then
//!   `add word ptr [di+2], 2`. Nothing gates it, nothing remembers it, and the
//!   id is translated per sound device inside `PLAY_SFX`, which is why an id is
//!   not a sample number. See `henge_audio::sfx`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Facing: the original stores 1 for right and 3 for left in `task+0x14`, and
/// the mirror is bit 1 of that byte.
pub const FACING_RIGHT: u8 = 1;
pub const FACING_LEFT: u8 = 3;

/// Part flag bits, as read in `PerformCOMMAND`.
pub mod part_flags {
    /// Also push this part onto `BodyPile`, the list the collision code walks.
    pub const BODY: u8 = 0x01;
    /// Also push it onto `WeoponPile`. A blade only threatens while a script
    /// says it does: the knight's stance carries its sword as `BODY`, and only
    /// the swing marks it `WEAPON`.
    pub const WEAPON: u8 = 0x02;
    /// Blit it a second time into the buffer at segment 0xac00.
    pub const PAGE2: u8 = 0x10;
    /// Do not fold this part into the actor's bounding box.
    pub const NO_BOUNDS: u8 = 0x40;
    /// Drop the part entirely, piles included, in bloodless mode.
    pub const GATED: u8 = 0x80;
}

/// The actor record fields the VM reads and writes, by their offset in that
/// record. Only the ones the machine itself needs are named.
pub mod field {
    /// `TASK_FLIP` copies the facing here.
    pub const FACING: i16 = 0x08;
    /// Hit points. `TASKDEAD` branches when this is not greater than zero.
    pub const HEALTH: i16 = 0x38;
    /// `+0x0b`, the frame counter `DecTimer` (0x2a63) runs down and
    /// `Balok_Stance`'s `TASKTESTEQ` reads: thirty frames between blinks.
    pub const TIMER: i16 = 0x0b;
    /// Daggers carried, a byte. `SetKnightEquipment` writes ten here,
    /// `Knight_SwKnife` opens with a `TASKTESTEQ` on it and goes back to the
    /// stance when it is zero, and `KnifeThrow` takes one off before it spawns
    /// the blade.
    pub const DAGGERS: i16 = 0x34;
}

/// Where execution is: a script by name, and an index into its instructions.
///
/// The original keeps a raw `DS` offset. Naming the script instead is what lets
/// a jump target be readable in the exported data and lets our own scripts be
/// authored without knowing anything about a 1991 data segment.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pc {
    pub script: String,
    pub at: usize,
}

impl Pc {
    pub fn start(script: impl Into<String>) -> Pc {
        Pc {
            script: script.into(),
            at: 0,
        }
    }

    pub fn is_unset(&self) -> bool {
        self.script.is_empty()
    }
}

/// One sprite part: a cel from one of the actor's bank slots, placed relative
/// to the task's own position.
///
/// The six-byte record in the original is `[u8 bank*4][u8 cel][i8 y][u8 flags]
/// [i16 x]`. The bank selector is divided out here, so `bank` is the slot
/// number the actor's bank table is indexed by.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Part {
    /// Which of the four bank tables, as `TASKCELBUF` last selected. 1 unless a
    /// script has said otherwise.
    #[serde(default = "one")]
    pub table: u8,
    /// Slot within that table.
    pub bank: u8,
    /// Cel within that bank.
    pub cel: u8,
    pub x: i16,
    pub y: i16,
    #[serde(default)]
    pub flags: u8,
}

fn one() -> u8 {
    1
}

impl Part {
    pub fn is(&self, flag: u8) -> bool {
        self.flags & flag != 0
    }
}

/// What the byte after `0xff` says happens once the frame has been shown.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum End {
    /// `ff 00`: the next frame follows.
    Next,
    /// `ff fe`: loop back if a `TASKLOOP` count is running, else carry on.
    Loop,
    /// `ff ff`: the end of the animation, unless a `TASKLOOP` count is running,
    /// in which case it loops like `ff fe` does. That last part is read off the
    /// handler at `0x99a2` and is easy to miss: the terminal form is not
    /// unconditional.
    Stop,
}

/// One instruction. The nineteen commands, the sprite-part record and the three
/// end-of-frame forms, which is everything `PerformCOMMAND` can dispatch.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Instr {
    Part(Part),
    EndFrame {
        end: End,
    },
    /// `0xfd`: resume where `TASKJUMP` left off. No shipped script contains one
    /// as an opcode; it is here so the machine is complete.
    ResumeJump,
    /// `0xfe` as an opcode, as opposed to as an end-of-frame byte.
    ResumeLoop,
    /// `TASK_FLIP`. 0xff toggles the mirror bit, which is the only form any
    /// shipped script uses.
    Flip {
        facing: u8,
    },
    /// `TASKGOTO`. Mode 3 jumps at once; any other mode arms the jump and takes
    /// it at the next end of frame.
    Goto {
        mode: u8,
        target: String,
    },
    /// `TASKHOLD`. Show the following frame this many times; 0 means once.
    Hold {
        count: u8,
    },
    /// `TASKJUMP`. Ballistic motion for a number of ticks. One script in the
    /// whole game uses it.
    Jump {
        arg: u8,
        ticks: u8,
        flags: u8,
        y_speed: u8,
        y_limit: u8,
        x_speed: u8,
        x_limit: u8,
    },
    /// `TASKLOOP`. Repeat up to the next `ff fe` this many times.
    Loop {
        count: u8,
    },
    /// `TASKSKIP`. Branch in bloodless mode.
    Skip {
        target: String,
    },
    /// `TASKTIME`. The handler is a bare `RET` that does not advance the script
    /// pointer, so the original would spin on it. No script emits one.
    Time,
    Sound {
        sample: u8,
    },
    /// `TASKMOVE`. With bit 0x40 the position is set outright; otherwise each
    /// axis is added, with the signs the handler applies.
    Move {
        flags: u8,
        x: i16,
        y: i16,
        z: i16,
    },
    /// `TASKSHADOW`. An empty target is the "off" form.
    Shadow {
        on: bool,
        script: String,
    },
    /// `TASKSAVE`. Stores into the actor record; mode bit 0 stores a byte.
    Save {
        mode: u8,
        field: i16,
        value: u16,
    },
    /// `TASKGOSUB`. A near call into the game's own code, which is why this is
    /// an effect and not an operation.
    Gosub {
        routine: String,
    },
    /// `TASKDEAD`. Branch when the actor's hit points are not above zero.
    Dead {
        target: String,
    },
    /// `TASKADDTASK`. Spawn a second task on that script.
    AddTask {
        target: String,
    },
    KillTask,
    /// `TASKCELBUF`. Choose which of the four bank tables the parts index.
    CelBuf {
        table: u8,
    },
    /// `TASKTESTEQ`. Branch if the field is zero; mode bit 0 tests a byte.
    TestEq {
        mode: u8,
        field: i16,
        target: String,
    },
    /// `TASKTESTNE`. Branch if the field is non-zero. No shipped script uses it.
    TestNe {
        mode: u8,
        field: i16,
        target: String,
    },
    /// `TASKANIMCLR`. Zero the VM state, keeping the shadow.
    AnimClr,
}

/// One animation script: a straight list of instructions.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Script {
    pub code: Vec<Instr>,
}

impl Script {
    pub fn new(code: Vec<Instr>) -> Script {
        Script { code }
    }
}

/// Every script an actor can run, by name.
pub type ScriptSet = BTreeMap<String, Script>;

// ---------------------------------------------------------------- bank tables

/// One sprite bank: where its cels start in a sheet, and how big each is.
///
/// A script is meaningless without one of these. A part names a slot and a cel
/// index, and only the bank table says which artwork that is. The sizes are
/// here because placement needs them: a mirrored part is drawn at
/// `task_x - (x + cel_width)`, so the width is part of the geometry rather than
/// a rendering detail.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Bank {
    /// Asset id of the sheet these cels live in.
    pub sheet: String,
    /// Index of this bank's first cel within that sheet.
    pub base: u32,
    /// `[width, height]` per cel, in the order the bank stores them.
    pub cels: Vec<[u16; 2]>,
    /// `COLLIDE.HIT`'s polyline for each cel, in cel-local pixels: x from the
    /// cel's left edge, y from its top.
    ///
    /// **Recovered.** `TaskPlace$` (0x98c1) pushes every part flagged
    /// `WEAPON` onto `WeoponPile` and every part flagged `BODY` onto
    /// `BodyPile`, ten bytes each, and `TaskCol_MainLoop` (0x9f26) walks one
    /// pile against the other. `COLCHK` (0x9fcd) then walks *this* list:
    /// `CHECKL` (0xa0da) takes a point at a time, `NOWID1` (0xa0ed) adds the
    /// weapon record's own x, and 0xa108 adds its y, so these are the samples
    /// along the blade that a strike is tested at. A cel whose entry is empty
    /// is the file's `00`, and `RIGHTON` (0xa022) answers no hit for it.
    ///
    /// Shorter than `cels`, or empty, for a bank the file says nothing about.
    #[serde(default)]
    pub hit: Vec<Vec<[i16; 2]>>,
}

impl Bank {
    pub fn cel(&self, cel: u8) -> Option<[u16; 2]> {
        self.cels.get(cel as usize).copied()
    }

    /// Which frame of the sheet a cel index is.
    pub fn frame(&self, cel: u8) -> Option<u32> {
        (self.cels.len() > cel as usize).then(|| self.base + cel as u32)
    }

    /// This cel's `COLLIDE.HIT` polyline, or `None` where the file names no
    /// line for it at all. An empty slice is the file's own `00`.
    pub fn hit_line(&self, cel: u8) -> Option<&[[i16; 2]]> {
        self.hit.get(cel as usize).map(Vec::as_slice)
    }
}

/// The bank tables an actor's scripts index through, keyed by the number
/// `TASKCELBUF` selects them with. Table 1 is the one a script uses unless it
/// says otherwise.
pub type BankTables = BTreeMap<u8, Vec<Bank>>;

/// Where a part ends up, once a bank table has resolved its geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placed {
    /// Frame index within the bank's sheet.
    pub frame: u32,
    pub w: u16,
    pub h: u16,
    /// Top left, in the same space the task's own position is in.
    pub x: i32,
    pub y: i32,
    pub flags: u8,
    pub mirror: bool,
}

/// Place one part, exactly as `TASKRIGHT` / `TASKLEFT` / `TASKPLACE` do.
///
/// ```text
/// screen_y = task_y + task_z + y
/// facing right   screen_x = task_x + x
/// facing left    screen_x = task_x - (x + cel_width)
/// ```
///
/// Getting the mirror term wrong is not subtle: the sword detaches from the
/// hand. That is how it was checked.
pub fn place(part: &Part, bank: &Bank, task: (i32, i32, i32), mirror: bool) -> Option<Placed> {
    let [w, h] = bank.cel(part.cel)?;
    let frame = bank.frame(part.cel)?;
    let (tx, ty, tz) = task;
    let x = if mirror {
        tx - (part.x as i32 + w as i32)
    } else {
        tx + part.x as i32
    };
    Some(Placed {
        frame,
        w,
        h,
        x,
        y: ty + tz + part.y as i32,
        flags: part.flags,
        mirror,
    })
}

// -------------------------------------------------------------------- effects

/// What kind of thing a `TASKGOSUB` target does. None of them is implemented
/// here, and this is the honest reason each one exists rather than a guess at
/// its body.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GosubKind {
    /// Plays a sample. All 23 of these have been disassembled and are
    /// transcribed in [`crate::sound`], which is where the ids they pass to
    /// `PLAY_SFX` come from. Three of the 23 are silent in the shipped image.
    Sound,
    /// Puts something else in the arena: a thrown knife, a whirl, a dropped part.
    Spawn,
    /// Blood and dismemberment, which the bloodless mode also gates elsewhere.
    Gore,
    /// Changes how the fight itself is being run.
    Control,
    /// Not one of the 37 the shipped scripts call.
    Unknown,
}

/// Every `TASKGOSUB` target any of the 236 shipped scripts calls, with what the
/// routine is for.
///
/// The addresses were resolved by `tools/taskvm.py`, and all 41 land exactly on
/// a named routine entry point, which is one of the checks that the link-time
/// offset correction is real. The **kind** beside each name started as a reading
/// of the name; the 23 marked [`GosubKind::Sound`] have since been disassembled
/// one by one and agree with it, and they are in [`crate::sound`] with their
/// addresses. The other eighteen are still the name and a hint.
///
/// Four of the 41 (`AddCrushSnd`, `AddMudSound`, `AddMudVoice` and
/// `PlayScareMusic`) belong to the mudmen, whose fourteen scripts were missed
/// while the set was believed to be 221: their prefix is `Mudmen`, not the
/// `Mudman` of `MudmanTABLE`.
pub const GOSUB_TARGETS: &[(&str, GosubKind)] = &[
    ("AddCrushSnd", GosubKind::Sound),
    ("AddDemonWhirl", GosubKind::Spawn),
    ("AddMudSound", GosubKind::Sound),
    ("AddMudVoice", GosubKind::Sound),
    ("AddTrollSND", GosubKind::Sound),
    ("BalokRoarSound", GosubKind::Sound),
    ("BalokThrashSound", GosubKind::Sound),
    ("BigLandAudio", GosubKind::Sound),
    ("CountTheDead", GosubKind::Control),
    ("DecTimer", GosubKind::Control),
    ("DrBreathSnd", GosubKind::Sound),
    ("DrDropClaws", GosubKind::Gore),
    ("DrDropHead", GosubKind::Gore),
    ("DrFireSnd", GosubKind::Sound),
    ("DragonChewSound", GosubKind::Sound),
    ("DragonKnightSound", GosubKind::Sound),
    ("FlipDemonWhirl", GosubKind::Control),
    ("GuardianDiesSnd", GosubKind::Sound),
    ("GuardianYellSnd", GosubKind::Sound),
    ("HengeThunderSnd", GosubKind::Sound),
    ("InitSLAP", GosubKind::Control),
    ("KAudio0", GosubKind::Sound),
    ("KillKnight", GosubKind::Control),
    ("KnifeThrow", GosubKind::Spawn),
    ("KnightGruntSound", GosubKind::Sound),
    ("KnightOFF", GosubKind::Control),
    ("KnightON", GosubKind::Control),
    ("KnightSLAP", GosubKind::Control),
    ("MonsterOFF", GosubKind::Control),
    ("PlayScareMusic", GosubKind::Sound),
    ("RatHangSound", GosubKind::Sound),
    ("RatImpaleSound", GosubKind::Sound),
    ("RatLeapSound", GosubKind::Sound),
    ("RatScreamSound", GosubKind::Sound),
    ("SetDecapFLAG", GosubKind::Gore),
    ("ShakeADD", GosubKind::Control),
    ("SkullRatSound", GosubKind::Sound),
    ("StopCombat", GosubKind::Control),
    ("StopDemonWhirl", GosubKind::Control),
    ("TrackKnight", GosubKind::Control),
    ("TroggRoarSound", GosubKind::Sound),
];

/// What a named `TASKGOSUB` target is for, or [`GosubKind::Unknown`].
pub fn gosub_kind(routine: &str) -> GosubKind {
    GOSUB_TARGETS
        .iter()
        .find(|(n, _)| *n == routine)
        .map_or(GosubKind::Unknown, |(_, k)| *k)
}

/// Something a script asked for that the interpreter cannot do itself.
///
/// The machine never plays a sound, spawns anything or calls into game code. It
/// says what was asked for and leaves it to the caller, which is what keeps the
/// simulation free of a platform.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "effect", rename_all = "snake_case")]
pub enum Effect {
    /// `TASKSOUND`: the byte the handler at `0x9b38` hands `PLAY_SFX` in `AL`.
    /// A sound id, which is not a sample number and not a sound: the table at
    /// DS:`0x7e6e` turns it into one of the 49 samples.
    Sound { sample: u8 },
    /// `TASKGOSUB`: a named routine in the original's code. Nothing here runs
    /// it. A caller that does not handle the name has a recorded no-op.
    Gosub { routine: String, kind: GosubKind },
    /// `TASKADDTASK`: a second task on that script.
    Spawn { script: String },
    /// `TASKSHADOW`: the actor's shadow script, or off.
    Shadow { on: bool, script: String },
    /// `TASKKILLTASK`: this task stopped itself.
    Killed,
    /// `TASKSAVE`: a store into the actor record.
    Save { field: i16, value: u16, byte: bool },
    /// The script pointer named something the script set does not contain. The
    /// task stops rather than running on into whatever follows.
    MissingScript { name: String },
    /// `TASKTIME`, whose handler does not advance the script pointer, so the
    /// original spins on it. Nothing emits one; if one ever appeared, the task
    /// stops here instead of hanging.
    Stalled,
}

// ----------------------------------------------------------------- the machine

/// The 0x1a-byte VM state record, one per task.
///
/// Field for field with `TaskCommand` in the original, with the two halves of
/// the pending `TASKGOTO` (`+8` and `+0xa`) kept as a target and a flag.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct VmState {
    pub hold_count: u8,
    pub hold_running: bool,
    pub hold_resume: Pc,
    pub loop_count: u8,
    pub loop_running: bool,
    pub loop_resume: Pc,
    pub goto_target: Pc,
    pub goto_armed: bool,
    pub shadow_on: bool,
    pub shadow_script: String,
    pub jump_arg: u8,
    pub jump_flags: u8,
    pub jump_running: bool,
    pub jump_ticks: u8,
    pub y_speed: u8,
    pub y_limit: u8,
    pub x_speed: u8,
    pub x_limit: u8,
    pub jump_resume: Pc,
}

impl VmState {
    /// What `TASKANIMCLR` does: zero the record, but put the shadow back.
    /// `ADDTASK` and `REPLACEANIM` zero it without that exception, which is
    /// [`VmState::default`].
    fn clear_keeping_shadow(&mut self) {
        let (on, script) = (self.shadow_on, std::mem::take(&mut self.shadow_script));
        *self = VmState::default();
        self.shadow_on = on;
        self.shadow_script = script;
    }
}

/// The actor record, as far as the VM is concerned: a sparse set of fields by
/// offset. `TASKDEAD` reads hit points out of it, `TASKTESTEQ` and `TASKTESTNE`
/// read any field, and `TASKSAVE` and `TASK_FLIP` write to it.
///
/// A map rather than a struct because the scripts address the record by raw
/// offset, and only two of those offsets are identified.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct TaskActor {
    pub fields: BTreeMap<i16, i32>,
}

impl TaskActor {
    pub fn with_health(health: i32) -> TaskActor {
        let mut a = TaskActor::default();
        a.set(field::HEALTH, health);
        a
    }

    pub fn get(&self, at: i16) -> i32 {
        self.fields.get(&at).copied().unwrap_or(0)
    }

    pub fn set(&mut self, at: i16, v: i32) {
        self.fields.insert(at, v);
    }

    pub fn health(&self) -> i32 {
        self.get(field::HEALTH)
    }

    pub fn set_health(&mut self, v: i32) {
        self.set(field::HEALTH, v);
    }
}

/// What one tick of a task produced.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Frame {
    /// The task's own position after this tick. Parts are placed against it.
    pub x: i32,
    pub y: i32,
    pub z: i32,
    /// 1 right, 3 left. Bit 1 is the mirror.
    pub facing: u8,
    pub parts: Vec<Part>,
    pub effects: Vec<Effect>,
    /// The animation ended on this tick. The original clears `task+1` here,
    /// which is what lets a controller hand the task a new script.
    pub finished: bool,
    /// The task gave up rather than looping forever. Always false for every
    /// shipped script; a malformed one is caught here instead of hanging.
    pub stalled: bool,
}

impl Frame {
    pub fn mirror(&self) -> bool {
        self.facing & 2 != 0
    }

    /// Parts carrying a flag, in script order.
    pub fn parts_with(&self, flag: u8) -> impl Iterator<Item = &Part> {
        self.parts.iter().filter(move |p| p.is(flag))
    }
}

/// How many instructions one tick may execute before the task is called stuck.
/// No shipped script comes close: the longest frame in the game is a handful of
/// commands and six parts.
const STEP_BUDGET: u32 = 4096;

/// One animation task: the running state of one actor's script.
///
/// The original's task record is 0x24 bytes and carries a few things that are
/// outputs of the draw rather than state (screen x and y, the current cel's
/// width and height, the two pile heads). Those are not kept here, because the
/// interpreter does not draw.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Task {
    pub pc: Pc,
    /// `task+0`. A killed task is inactive and produces nothing.
    pub active: bool,
    /// `task+1`. False once the animation has ended, which is the signal a
    /// controller waits on before handing over a new script.
    pub running: bool,
    /// `task+0x14`: 1 right, 3 left, bit 1 mirrors.
    pub facing: u8,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    /// `task+0x18`, as a table number rather than an address.
    pub table: u8,
    pub vm: VmState,
    /// The last frame that had anything in it.
    ///
    /// A script that has ended leaves its pointer on the terminal `0xff`, so it
    /// produces no new parts and the picture on screen stays as it was. Keeping
    /// the parts is how that is reproduced without re-running the frame, which
    /// would fire its sounds and its `TASKGOSUB`s again every tick.
    pub shown: Vec<Part>,
}

impl Task {
    /// A task about to run a script from its first instruction.
    pub fn new(script: impl Into<String>, x: i32, y: i32, facing: u8) -> Task {
        Task {
            pc: Pc::start(script),
            active: true,
            running: true,
            facing,
            x,
            y,
            z: 0,
            table: 1,
            vm: VmState::default(),
            shown: Vec::new(),
        }
    }

    /// Hand the task a new script. `REPLACEANIM` zeroes the whole VM state,
    /// shadow included, which is what this does.
    pub fn replace(&mut self, script: impl Into<String>) {
        self.pc = Pc::start(script);
        self.active = true;
        self.running = true;
        self.vm = VmState::default();
        self.shown.clear();
    }

    pub fn mirror(&self) -> bool {
        self.facing & 2 != 0
    }

    /// One tick.
    ///
    /// Runs the script from the current pointer until the end of a frame, and
    /// returns what that frame is made of. `bloodless` is the DS:0x700 mode
    /// flag: it gates `TASKSKIP` and drops every part carrying
    /// [`part_flags::GATED`].
    pub fn step(&mut self, set: &ScriptSet, actor: &mut TaskActor, bloodless: bool) -> Frame {
        let mut frame = Frame {
            x: self.x,
            y: self.y,
            z: self.z,
            facing: self.facing,
            ..Frame::default()
        };
        if !self.active {
            return frame;
        }

        let mut budget = STEP_BUDGET;
        loop {
            if budget == 0 {
                frame.effects.push(Effect::Stalled);
                frame.stalled = true;
                self.active = false;
                break;
            }
            budget -= 1;

            let Some(script) = set.get(&self.pc.script) else {
                frame.effects.push(Effect::MissingScript {
                    name: self.pc.script.clone(),
                });
                frame.stalled = true;
                self.active = false;
                self.running = false;
                break;
            };
            // Running off the end of a script is not something any shipped one
            // does; every single one terminates on `ff ff`. Treat it as the end
            // of the animation rather than reading past it.
            let Some(instr) = script.code.get(self.pc.at) else {
                self.running = false;
                frame.finished = true;
                break;
            };

            match instr.clone() {
                Instr::Part(p) => {
                    if !(bloodless && p.is(part_flags::GATED)) {
                        frame.parts.push(Part {
                            table: self.table,
                            ..p
                        });
                    }
                    self.pc.at += 1;
                }
                Instr::EndFrame { end } => {
                    self.end_of_frame(end, &mut frame);
                    break;
                }
                Instr::ResumeJump => self.pc = self.vm.jump_resume.clone(),
                Instr::ResumeLoop => self.pc = self.vm.loop_resume.clone(),
                Instr::Flip { facing } => {
                    if facing == 0xff {
                        self.facing ^= 2;
                    } else {
                        self.facing = facing;
                    }
                    actor.set(field::FACING, self.facing as i32);
                    self.pc.at += 1;
                }
                Instr::Goto { mode, target } => {
                    if mode == 3 {
                        self.pc = Pc::start(target);
                    } else {
                        self.vm.goto_target = Pc::start(target);
                        self.vm.goto_armed = true;
                        self.pc.at += 1;
                    }
                }
                Instr::Hold { count } => {
                    self.vm.hold_count = if count == 0 { 1 } else { count };
                    self.vm.hold_running = true;
                    self.pc.at += 1;
                    self.vm.hold_resume = self.pc.clone();
                }
                Instr::Jump {
                    arg,
                    ticks,
                    flags,
                    y_speed,
                    y_limit,
                    x_speed,
                    x_limit,
                } => {
                    self.vm.jump_running = true;
                    self.vm.jump_arg = arg;
                    self.vm.jump_flags = flags;
                    self.vm.jump_ticks = ticks;
                    self.vm.y_speed = y_speed;
                    self.vm.y_limit = y_limit;
                    self.vm.x_speed = x_speed;
                    self.vm.x_limit = x_limit;
                    self.pc.at += 1;
                    self.vm.jump_resume = self.pc.clone();
                }
                Instr::Loop { count } => {
                    self.vm.loop_count = count;
                    self.vm.loop_running = true;
                    self.pc.at += 1;
                    self.vm.loop_resume = self.pc.clone();
                }
                Instr::Skip { target } => {
                    if bloodless {
                        self.pc = Pc::start(target);
                    } else {
                        self.pc.at += 1;
                    }
                }
                Instr::Time => {
                    // The handler is a bare RET and does not advance the script
                    // pointer, so the original spins here forever. Stop instead.
                    frame.effects.push(Effect::Stalled);
                    frame.stalled = true;
                    self.active = false;
                    break;
                }
                Instr::Sound { sample } => {
                    frame.effects.push(Effect::Sound { sample });
                    self.pc.at += 1;
                }
                Instr::Move { flags, x, y, z } => {
                    self.do_move(flags, x, y, z);
                    self.pc.at += 1;
                }
                Instr::Shadow { on, script } => {
                    self.vm.shadow_on = on;
                    self.vm.shadow_script = script.clone();
                    frame.effects.push(Effect::Shadow { on, script });
                    self.pc.at += 1;
                }
                Instr::Save { mode, field, value } => {
                    let byte = mode & 1 != 0;
                    let stored = if byte {
                        (actor.get(field) & !0xff) | (value as i32 & 0xff)
                    } else {
                        value as i32
                    };
                    actor.set(field, stored);
                    frame.effects.push(Effect::Save { field, value, byte });
                    self.pc.at += 1;
                }
                Instr::Gosub { routine } => {
                    // `DecTimer` (0x2a63) is the one `TASKGOSUB` target that
                    // touches nothing but the actor record, and the frame that
                    // calls it reads what it wrote two instructions later:
                    //
                    //   02a63  sub byte ptr [di+0xb], 1
                    //   02a67  jge 02a72
                    //   02a69  call RND          ; and al, 0x1f is then dead
                    //   02a6e  mov byte ptr [di+0xb], 0x1e
                    //
                    // `Balok_Stance` is `TASKGOSUB DecTimer` followed at once
                    // by `TASKTESTEQ +0xb -> Balok_Blink`, so the count has to
                    // be down before the branch is taken or Balok blinks every
                    // frame; `Balok_Blink` ends on `ff 00` rather than `ff ff`,
                    // so blinking every frame is a stance whose animation never
                    // ends and a creature whose controller is never asked
                    // again. Everything else a gosub does is a call into the
                    // game and stays an effect.
                    if routine == "DecTimer" {
                        let left = actor.get(field::TIMER) - 1;
                        actor.set(field::TIMER, if left < 0 { 0x1e } else { left });
                    }
                    let kind = gosub_kind(&routine);
                    frame.effects.push(Effect::Gosub { routine, kind });
                    self.pc.at += 1;
                }
                Instr::Dead { target } => {
                    // `cmp word [bp+0x38], 0 / jg` : alive means strictly
                    // greater than zero.
                    if actor.health() <= 0 {
                        self.pc = Pc::start(target);
                        self.vm.clear_keeping_shadow();
                    } else {
                        self.pc.at += 1;
                    }
                }
                Instr::AddTask { target } => {
                    frame.effects.push(Effect::Spawn { script: target });
                    self.pc.at += 1;
                }
                Instr::KillTask => {
                    actor.set(0, 0);
                    self.active = false;
                    frame.effects.push(Effect::Killed);
                    self.pc.at += 1;
                    break;
                }
                Instr::CelBuf { table } => {
                    self.table = table;
                    self.pc.at += 1;
                }
                Instr::TestEq {
                    mode,
                    field,
                    target,
                } => {
                    if is_zero(actor.get(field), mode) {
                        self.pc = Pc::start(target);
                    } else {
                        self.pc.at += 1;
                    }
                }
                Instr::TestNe {
                    mode,
                    field,
                    target,
                } => {
                    if !is_zero(actor.get(field), mode) {
                        self.pc = Pc::start(target);
                    } else {
                        self.pc.at += 1;
                    }
                }
                Instr::AnimClr => {
                    self.vm.clear_keeping_shadow();
                    self.pc.at += 1;
                }
            }
        }

        frame.x = self.x;
        frame.y = self.y;
        frame.z = self.z;
        frame.facing = self.facing;
        if frame.parts.is_empty() {
            if !self.running {
                frame.parts = self.shown.clone();
            }
        } else {
            self.shown = frame.parts.clone();
        }
        frame
    }

    /// The handler at image 0x993a, in its own order: a running hold first, a
    /// running jump second, a pending goto third, and only then the byte after
    /// the `0xff`.
    fn end_of_frame(&mut self, end: End, frame: &mut Frame) {
        if self.vm.hold_running {
            self.vm.hold_count = self.vm.hold_count.wrapping_sub(1);
            if self.vm.hold_count != 0 {
                self.pc = self.vm.hold_resume.clone();
                return;
            }
        }
        self.vm.hold_running = false;

        if self.vm.jump_running {
            self.vm.jump_ticks = self.vm.jump_ticks.wrapping_sub(1);
            if self.vm.jump_ticks as i8 >= 0 {
                self.ballistic();
                return;
            }
            // The original skips the store that would clear the flag here, so a
            // jump whose counter has gone negative keeps decrementing it and
            // falls through to the terminator every frame. Reproduced rather
            // than tidied, because tidying it would change what a script does.
        }

        if self.vm.goto_armed {
            self.vm.goto_armed = false;
            self.pc = std::mem::take(&mut self.vm.goto_target);
            return;
        }

        match end {
            End::Next => self.pc.at += 1,
            End::Loop | End::Stop => {
                if self.vm.loop_running {
                    self.vm.loop_count = self.vm.loop_count.wrapping_sub(1);
                    if self.vm.loop_count != 0 {
                        self.pc = self.vm.loop_resume.clone();
                        return;
                    }
                }
                self.vm.loop_running = false;
                if end == End::Stop {
                    // The pointer stays on the terminator, which is what makes
                    // the last frame keep being shown.
                    self.running = false;
                    frame.finished = true;
                } else {
                    self.pc.at += 1;
                }
            }
        }
    }

    /// `TASKMOVE`, with the signs read off `MoveX` / `MoveY` / `MoveDone`.
    fn do_move(&mut self, flags: u8, x: i16, y: i16, z: i16) {
        if flags & 0x40 != 0 {
            self.x = x as i32;
            self.y = y as i32;
            self.z = z as i32;
            return;
        }
        let facing_left = self.facing == FACING_LEFT;
        let mut v = x as i32;
        if facing_left == (flags & 0x01 == 0) {
            v = -v;
        }
        self.x -= v;

        let mut v = y as i32;
        if flags & 0x08 == 0 {
            v = -v;
        }
        self.y -= v;

        let mut v = z as i32;
        if flags & 0x20 == 0 {
            v = -v;
        }
        self.z -= v;
    }

    /// The ballistic step a running `TASKJUMP` takes at each end of frame,
    /// transcribed from the routine at image 0x9ceb.
    ///
    /// One script in the whole game uses `TASKJUMP` (`Beast_BackToss`), and two
    /// branches of this are strange enough to be worth naming: the upward form
    /// assigns the *speed* into the y position when the addition comes out
    /// non-negative, and it then compares the low byte of the *position*
    /// against the speed limit. Both are what the instructions say. They are
    /// reproduced rather than corrected, because a guess at what was meant
    /// would be a guess about behaviour nobody has observed.
    fn ballistic(&mut self) {
        let mut moved = false;
        let f = self.vm.jump_flags;

        if f & 0x02 != 0 {
            moved = true;
            let speed = self.vm.y_speed as i32;
            let sum = self.y + speed;
            if sum >= 0 {
                self.y = speed;
                self.vm.jump_flags &= !0x02;
            } else {
                self.y = sum;
                if f & 0x20 == 0 && (self.y as u8) < self.vm.y_limit {
                    self.vm.y_speed = ((self.y as u8) as u16).wrapping_mul(2) as u8;
                }
            }
        } else if f & 0x01 != 0 {
            moved = true;
            let speed = self.vm.y_speed;
            self.y -= speed as i32;
            if f & 0x20 == 0 && speed > self.vm.y_limit {
                self.vm.y_speed = speed >> 1;
            }
        }

        // 0x10 and 0x04 are the same move with the facing test the other way
        // round: whichever matches the facing subtracts, the other adds.
        for (bit, when_mirrored) in [(0x10u8, true), (0x04u8, false)] {
            if f & bit == 0 {
                continue;
            }
            moved = true;
            let speed = self.vm.x_speed;
            if self.mirror() == when_mirrored {
                self.x -= speed as i32;
            } else {
                self.x += speed as i32;
            }
            if f & 0x80 == 0 && speed > self.vm.x_limit {
                self.vm.x_speed = speed >> 1;
            }
        }

        if f & 0x40 == 0 {
            self.pc = self.vm.jump_resume.clone();
        }
        if !moved {
            self.vm.jump_running = false;
        }
    }

    /// The task's contribution to a simulation fingerprint.
    ///
    /// Everything that decides what the next tick does is folded in. `shown` is
    /// not: it is the previous tick's output, entirely determined by the
    /// pointer and the state that are.
    pub fn hash_into(&self, mix: &mut impl FnMut(i64)) {
        hash_pc(&self.pc, mix);
        mix(self.active as i64);
        mix(self.running as i64);
        mix(self.facing as i64);
        mix(self.x as i64);
        mix(self.y as i64);
        mix(self.z as i64);
        mix(self.table as i64);
        let v = &self.vm;
        mix(v.hold_count as i64);
        mix(v.hold_running as i64);
        hash_pc(&v.hold_resume, mix);
        mix(v.loop_count as i64);
        mix(v.loop_running as i64);
        hash_pc(&v.loop_resume, mix);
        hash_pc(&v.goto_target, mix);
        mix(v.goto_armed as i64);
        mix(v.shadow_on as i64);
        for b in v.shadow_script.as_bytes() {
            mix(*b as i64);
        }
        mix(v.jump_arg as i64);
        mix(v.jump_flags as i64);
        mix(v.jump_running as i64);
        mix(v.jump_ticks as i64);
        mix(v.y_speed as i64);
        mix(v.y_limit as i64);
        mix(v.x_speed as i64);
        mix(v.x_limit as i64);
        hash_pc(&v.jump_resume, mix);
    }
}

fn hash_pc(pc: &Pc, mix: &mut impl FnMut(i64)) {
    for b in pc.script.as_bytes() {
        mix(*b as i64);
    }
    mix(pc.at as i64);
}

/// `TASKTESTEQ` and `TASKTESTNE` compare a byte when mode bit 0 is set and a
/// word otherwise. Mode bit 1 is tested by both handlers and both branches of
/// that test land on the same instruction, so it does nothing at all.
fn is_zero(v: i32, mode: u8) -> bool {
    if mode & 1 != 0 {
        v as u8 == 0
    } else {
        v as u16 == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(cel: u8) -> Instr {
        Instr::Part(Part {
            table: 1,
            bank: 0,
            cel,
            x: 0,
            y: 0,
            flags: 0,
        })
    }

    fn frame_of(cel: u8) -> Vec<Instr> {
        vec![part(cel), Instr::EndFrame { end: End::Next }]
    }

    fn set(scripts: &[(&str, Vec<Instr>)]) -> ScriptSet {
        scripts
            .iter()
            .map(|(n, c)| (n.to_string(), Script::new(c.clone())))
            .collect()
    }

    /// Run a task for `n` ticks and report the cel of the first part of each.
    fn cels(task: &mut Task, s: &ScriptSet, a: &mut TaskActor, n: usize) -> Vec<Option<u8>> {
        (0..n)
            .map(|_| task.step(s, a, false).parts.first().map(|p| p.cel))
            .collect()
    }

    #[test]
    fn a_hold_shows_the_same_frame_that_many_times() {
        let s = set(&[(
            "a",
            vec![
                Instr::Hold { count: 3 },
                part(1),
                Instr::EndFrame { end: End::Next },
                part(2),
                Instr::EndFrame { end: End::Stop },
            ],
        )]);
        let mut t = Task::new("a", 0, 0, FACING_RIGHT);
        let mut a = TaskActor::with_health(10);
        assert_eq!(
            cels(&mut t, &s, &mut a, 5),
            vec![Some(1), Some(1), Some(1), Some(2), Some(2)],
            "three shows of the held frame, then the last frame, which then persists"
        );
    }

    /// `TASKLOOP` counts the block between itself and the next `ff fe`.
    #[test]
    fn a_loop_repeats_its_block_the_stated_number_of_times() {
        let s = set(&[(
            "a",
            vec![
                Instr::Loop { count: 3 },
                part(1),
                Instr::EndFrame { end: End::Next },
                part(2),
                Instr::EndFrame { end: End::Loop },
                part(9),
                Instr::EndFrame { end: End::Stop },
            ],
        )]);
        let mut t = Task::new("a", 0, 0, FACING_RIGHT);
        let mut a = TaskActor::with_health(10);
        assert_eq!(
            cels(&mut t, &s, &mut a, 8),
            vec![
                Some(1),
                Some(2), // pass one
                Some(1),
                Some(2), // pass two
                Some(1),
                Some(2), // pass three
                Some(9),
                Some(9), // out of the loop, then held
            ]
        );
        assert!(!t.running, "the animation ended");
    }

    /// The terminal `ff ff` is not unconditional: the handler at 0x99a2 checks
    /// the loop counter first, exactly as the `ff fe` path does.
    #[test]
    fn a_running_loop_makes_even_the_final_terminator_loop() {
        let s = set(&[(
            "a",
            vec![
                Instr::Loop { count: 2 },
                part(1),
                Instr::EndFrame { end: End::Stop },
            ],
        )]);
        let mut t = Task::new("a", 0, 0, FACING_RIGHT);
        let mut a = TaskActor::with_health(10);
        let f = t.step(&s, &mut a, false);
        assert!(!f.finished, "the first pass loops rather than ending");
        assert!(t.running);
        let f = t.step(&s, &mut a, false);
        assert!(f.finished, "the second pass exhausts the loop and ends");
        assert!(!t.running);
    }

    /// A `TASKGOTO` in mode 0 is taken at the *next* end of frame, not at once,
    /// so the frame it sits in front of is still shown.
    #[test]
    fn a_pending_goto_shows_its_own_frame_first() {
        let s = set(&[
            (
                "a",
                vec![
                    Instr::Goto {
                        mode: 0,
                        target: "b".into(),
                    },
                    part(1),
                    Instr::EndFrame { end: End::Stop },
                ],
            ),
            ("b", frame_of(7)),
        ]);
        let mut t = Task::new("a", 0, 0, FACING_RIGHT);
        let mut a = TaskActor::with_health(10);
        assert_eq!(t.step(&s, &mut a, false).parts[0].cel, 1);
        assert_eq!(t.step(&s, &mut a, false).parts[0].cel, 7);
        assert_eq!(t.pc.script, "b");
    }

    #[test]
    fn a_goto_in_mode_three_is_taken_at_once() {
        let s = set(&[
            (
                "a",
                vec![
                    part(1),
                    Instr::Goto {
                        mode: 3,
                        target: "b".into(),
                    },
                ],
            ),
            ("b", frame_of(7)),
        ]);
        let mut t = Task::new("a", 0, 0, FACING_RIGHT);
        let mut a = TaskActor::with_health(10);
        let f = t.step(&s, &mut a, false);
        assert_eq!(f.parts.len(), 2, "both scripts contributed to one frame");
        assert_eq!((f.parts[0].cel, f.parts[1].cel), (1, 7));
    }

    /// `TASKDEAD` is the branch on hit points, and taking it also clears the VM
    /// state the way `TASKANIMCLR` does.
    #[test]
    fn a_dead_branch_is_taken_only_when_hit_points_run_out() {
        let s = set(&[
            (
                "hurt",
                vec![
                    Instr::Dead {
                        target: "death".into(),
                    },
                    part(1),
                    Instr::EndFrame { end: End::Stop },
                ],
            ),
            ("death", frame_of(9)),
        ]);
        let mut alive = Task::new("hurt", 0, 0, FACING_RIGHT);
        let mut a = TaskActor::with_health(1);
        assert_eq!(alive.step(&s, &mut a, false).parts[0].cel, 1);

        let mut dead = Task::new("hurt", 0, 0, FACING_RIGHT);
        let mut d = TaskActor::with_health(0);
        assert_eq!(
            dead.step(&s, &mut d, false).parts[0].cel,
            9,
            "zero counts as dead"
        );
        d.set_health(-5);
        let mut below = Task::new("hurt", 0, 0, FACING_RIGHT);
        assert_eq!(below.step(&s, &mut d, false).parts[0].cel, 9);
    }

    /// Taking a `TASKDEAD` also runs the `TASKANIMCLR` body, so nothing the
    /// interrupted animation had running survives into the death script.
    #[test]
    fn a_taken_dead_branch_clears_the_vm_state() {
        let s = set(&[
            (
                "hurt",
                vec![
                    Instr::Loop { count: 9 },
                    part(1),
                    Instr::EndFrame { end: End::Next },
                    Instr::Dead {
                        target: "death".into(),
                    },
                    part(2),
                    Instr::EndFrame { end: End::Loop },
                ],
            ),
            ("death", frame_of(9)),
        ]);
        let mut t = Task::new("hurt", 0, 0, FACING_RIGHT);
        let mut a = TaskActor::with_health(10);
        t.step(&s, &mut a, false);
        assert!(
            t.vm.loop_running,
            "the loop is armed while the knight is alive"
        );
        a.set_health(0);
        let f = t.step(&s, &mut a, false);
        assert_eq!(t.pc.script, "death");
        assert_eq!(f.parts[0].cel, 9);
        assert!(
            !t.vm.loop_running,
            "and the interrupted loop did not survive"
        );
    }

    #[test]
    fn a_gosub_is_reported_by_name_and_never_run() {
        let s = set(&[(
            "a",
            vec![
                Instr::Gosub {
                    routine: "KnightGruntSound".into(),
                },
                Instr::Gosub {
                    routine: "NoSuchRoutine".into(),
                },
                part(1),
                Instr::EndFrame { end: End::Stop },
            ],
        )]);
        let mut t = Task::new("a", 0, 0, FACING_RIGHT);
        let mut a = TaskActor::with_health(10);
        let f = t.step(&s, &mut a, false);
        assert_eq!(
            f.effects,
            vec![
                Effect::Gosub {
                    routine: "KnightGruntSound".into(),
                    kind: GosubKind::Sound
                },
                Effect::Gosub {
                    routine: "NoSuchRoutine".into(),
                    kind: GosubKind::Unknown
                },
            ],
            "an unimplemented target is a recorded no-op, not a silent skip"
        );
        assert_eq!(f.parts[0].cel, 1, "and the frame still draws");
    }

    #[test]
    fn every_gosub_target_the_shipped_scripts_call_is_named() {
        assert_eq!(
            GOSUB_TARGETS.len(),
            41,
            "the 41 distinct targets in the data"
        );
        let mut names: Vec<&str> = GOSUB_TARGETS.iter().map(|(n, _)| *n).collect();
        let before = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), before, "no name is listed twice");
        assert_eq!(gosub_kind("KnifeThrow"), GosubKind::Spawn);
        assert_eq!(gosub_kind("DrDropHead"), GosubKind::Gore);
    }

    #[test]
    fn a_sound_is_a_cue_and_not_a_sound() {
        let s = set(&[(
            "a",
            vec![
                Instr::Sound { sample: 0x0b },
                part(1),
                Instr::EndFrame { end: End::Stop },
            ],
        )]);
        let mut t = Task::new("a", 0, 0, FACING_RIGHT);
        let mut a = TaskActor::with_health(10);
        assert_eq!(
            t.step(&s, &mut a, false).effects,
            vec![Effect::Sound { sample: 0x0b }]
        );
    }

    /// A sound lands on the frame its script names it on, and on no other.
    ///
    /// This is `Knight_SwSwing`'s own shape, command for command as the baked
    /// script holds it: `TASKGOSUB KnightGruntSound`, `TASKHOLD 2`, the first
    /// frame, then `TASKSOUND 0x0b` (`swish`), `TASKHOLD 1` and the second. So
    /// the grunt is asked for on the first tick, the held frame repeats without
    /// asking for anything a second time, and the swish arrives on the third
    /// tick, which is the frame where the blade is out. Nothing about that is
    /// inferred from the fighter's state: it is four bytes of script.
    #[test]
    fn the_knights_swing_sounds_on_the_frame_the_script_says() {
        let s = set(&[(
            "Knight_SwSwing",
            vec![
                Instr::Gosub {
                    routine: "KnightGruntSound".into(),
                },
                Instr::Hold { count: 2 },
                part(32),
                Instr::EndFrame { end: End::Next },
                Instr::Sound { sample: 0x0b },
                Instr::Hold { count: 1 },
                part(34),
                Instr::EndFrame { end: End::Next },
                part(35),
                Instr::EndFrame { end: End::Stop },
            ],
        )]);
        let mut t = Task::new("Knight_SwSwing", 0, 0, FACING_RIGHT);
        let mut a = TaskActor::with_health(10);
        let mut heard: Vec<(usize, Vec<u8>)> = Vec::new();
        for tick in 0..4 {
            let f = t.step(&s, &mut a, false);
            let ids: Vec<u8> = f
                .effects
                .iter()
                .filter_map(|e| match e {
                    Effect::Sound { sample } => Some(*sample),
                    _ => None,
                })
                .collect();
            if !ids.is_empty() {
                heard.push((tick, ids));
            }
            if tick == 0 {
                assert!(f.effects.contains(&Effect::Gosub {
                    routine: "KnightGruntSound".into(),
                    kind: GosubKind::Sound,
                }));
            } else {
                assert!(
                    !f.effects.iter().any(|e| matches!(e, Effect::Gosub { .. })),
                    "the held frame does not call the routine again"
                );
            }
        }
        assert_eq!(heard, vec![(2, vec![0x0b])], "one swish, on the third tick");
    }

    /// Bloodless mode does two things, and this checks both: `TASKSKIP` is
    /// taken, and every part carrying flag 0x80 is dropped.
    #[test]
    fn bloodless_mode_skips_and_drops() {
        let gore = Instr::Part(Part {
            table: 1,
            bank: 0,
            cel: 5,
            x: 0,
            y: 0,
            flags: part_flags::GATED,
        });
        let s = set(&[
            ("a", vec![part(1), gore, Instr::EndFrame { end: End::Stop }]),
            (
                "b",
                vec![
                    Instr::Skip { target: "c".into() },
                    part(1),
                    Instr::EndFrame { end: End::Stop },
                ],
            ),
            ("c", frame_of(9)),
        ]);
        let mut a = TaskActor::with_health(10);

        let mut bloody = Task::new("a", 0, 0, FACING_RIGHT);
        assert_eq!(bloody.step(&s, &mut a, false).parts.len(), 2);
        let mut clean = Task::new("a", 0, 0, FACING_RIGHT);
        assert_eq!(clean.step(&s, &mut a, true).parts.len(), 1);

        let mut on = Task::new("b", 0, 0, FACING_RIGHT);
        assert_eq!(on.step(&s, &mut a, true).parts[0].cel, 9);
        let mut off = Task::new("b", 0, 0, FACING_RIGHT);
        assert_eq!(off.step(&s, &mut a, false).parts[0].cel, 1);
    }

    #[test]
    fn flipping_toggles_the_mirror_bit_and_tells_the_actor() {
        let s = set(&[(
            "a",
            vec![
                Instr::Flip { facing: 0xff },
                part(1),
                Instr::EndFrame { end: End::Stop },
            ],
        )]);
        let mut t = Task::new("a", 0, 0, FACING_RIGHT);
        let mut a = TaskActor::with_health(10);
        t.step(&s, &mut a, false);
        assert_eq!(t.facing, FACING_LEFT);
        assert!(t.mirror());
        assert_eq!(a.get(field::FACING), FACING_LEFT as i32);
    }

    /// The four sign rules of `TASKMOVE`, read off `MoveX` / `MoveY` / `MoveDone`.
    #[test]
    fn move_takes_its_signs_from_the_facing_and_the_flag_bits() {
        let mut a = TaskActor::with_health(10);
        let run = |facing: u8, flags: u8| {
            let s = set(&[(
                "a",
                vec![
                    Instr::Move {
                        flags,
                        x: 10,
                        y: 3,
                        z: 2,
                    },
                    Instr::EndFrame { end: End::Stop },
                ],
            )]);
            let mut t = Task::new("a", 100, 100, facing);
            t.step(&s, &mut TaskActor::with_health(10), false);
            (t.x, t.y, t.z)
        };
        assert_eq!(
            run(FACING_RIGHT, 0x00).0,
            90,
            "facing right, bit 0 clear: x -= v"
        );
        assert_eq!(
            run(FACING_RIGHT, 0x01).0,
            110,
            "facing right, bit 0 set: x += v"
        );
        assert_eq!(
            run(FACING_LEFT, 0x00).0,
            110,
            "facing left, bit 0 clear: x += v"
        );
        assert_eq!(
            run(FACING_LEFT, 0x01).0,
            90,
            "facing left, bit 0 set: x -= v"
        );
        assert_eq!(run(FACING_RIGHT, 0x08).1, 97, "bit 3 set: y -= v");
        assert_eq!(run(FACING_RIGHT, 0x00).1, 103, "bit 3 clear: y += v");
        assert_eq!(run(FACING_RIGHT, 0x20).2, -2, "bit 5 set: z -= v");
        assert_eq!(run(FACING_RIGHT, 0x00).2, 2, "bit 5 clear: z += v");

        let s = set(&[(
            "a",
            vec![
                Instr::Move {
                    flags: 0x40,
                    x: 7,
                    y: 8,
                    z: 9,
                },
                Instr::EndFrame { end: End::Stop },
            ],
        )]);
        let mut t = Task::new("a", 100, 100, FACING_RIGHT);
        t.step(&s, &mut a, false);
        assert_eq!(
            (t.x, t.y, t.z),
            (7, 8, 9),
            "bit 0x40 sets the position outright"
        );
    }

    #[test]
    fn a_finished_script_keeps_showing_its_last_frame() {
        let s = set(&[("a", vec![part(3), Instr::EndFrame { end: End::Stop }])]);
        let mut t = Task::new("a", 0, 0, FACING_RIGHT);
        let mut a = TaskActor::with_health(10);
        let first = t.step(&s, &mut a, false);
        assert!(first.finished);
        let later = t.step(&s, &mut a, false);
        assert_eq!(later.parts, first.parts, "the picture stays put");
        assert!(later.effects.is_empty(), "and nothing fires a second time");
    }

    #[test]
    fn a_missing_script_stops_the_task_rather_than_running_on() {
        let s = set(&[(
            "a",
            vec![Instr::Goto {
                mode: 3,
                target: "gone".into(),
            }],
        )]);
        let mut t = Task::new("a", 0, 0, FACING_RIGHT);
        let mut a = TaskActor::with_health(10);
        let f = t.step(&s, &mut a, false);
        assert!(f.stalled);
        assert_eq!(
            f.effects,
            vec![Effect::MissingScript {
                name: "gone".into()
            }]
        );
        assert!(!t.active);
    }

    #[test]
    fn a_script_that_loops_forever_gives_up_instead_of_hanging() {
        let s = set(&[(
            "a",
            vec![Instr::Goto {
                mode: 3,
                target: "a".into(),
            }],
        )]);
        let mut t = Task::new("a", 0, 0, FACING_RIGHT);
        let mut a = TaskActor::with_health(10);
        assert!(t.step(&s, &mut a, false).stalled);
    }

    #[test]
    fn killtask_stops_the_task_and_zeroes_the_actor() {
        let s = set(&[("a", vec![part(1), Instr::KillTask, part(2)])]);
        let mut t = Task::new("a", 0, 0, FACING_RIGHT);
        let mut a = TaskActor::with_health(10);
        let f = t.step(&s, &mut a, false);
        assert_eq!(f.parts.len(), 1);
        assert!(f.effects.contains(&Effect::Killed));
        assert!(!t.active);
        assert_eq!(a.get(0), 0);
        assert!(
            t.step(&s, &mut a, false).parts.is_empty(),
            "an inactive task does nothing"
        );
    }

    #[test]
    fn testeq_compares_a_byte_or_a_word_as_the_mode_says() {
        let s = set(&[
            (
                "w",
                vec![
                    Instr::TestEq {
                        mode: 0,
                        field: 4,
                        target: "z".into(),
                    },
                    part(1),
                    Instr::EndFrame { end: End::Stop },
                ],
            ),
            (
                "b",
                vec![
                    Instr::TestEq {
                        mode: 1,
                        field: 4,
                        target: "z".into(),
                    },
                    part(1),
                    Instr::EndFrame { end: End::Stop },
                ],
            ),
            ("z", frame_of(9)),
        ]);
        // 0x100 is zero in a byte and non-zero in a word.
        let mut a = TaskActor::default();
        a.set(4, 0x100);
        let mut w = Task::new("w", 0, 0, FACING_RIGHT);
        assert_eq!(
            w.step(&s, &mut a, false).parts[0].cel,
            1,
            "non-zero as a word"
        );
        let mut b = Task::new("b", 0, 0, FACING_RIGHT);
        assert_eq!(b.step(&s, &mut a, false).parts[0].cel, 9, "zero as a byte");
    }

    #[test]
    fn animclr_keeps_the_shadow_and_replace_does_not() {
        let s = set(&[(
            "a",
            vec![
                Instr::Shadow {
                    on: true,
                    script: "Ratman_Shadow".into(),
                },
                Instr::Hold { count: 9 },
                Instr::AnimClr,
                part(1),
                Instr::EndFrame { end: End::Stop },
            ],
        )]);
        let mut t = Task::new("a", 0, 0, FACING_RIGHT);
        let mut a = TaskActor::with_health(10);
        t.step(&s, &mut a, false);
        assert!(!t.vm.hold_running, "the hold was cleared");
        assert!(t.vm.shadow_on, "the shadow survived");
        assert_eq!(t.vm.shadow_script, "Ratman_Shadow");
        t.replace("a");
        assert!(!t.vm.shadow_on, "REPLACEANIM zeroes the shadow too");
    }

    #[test]
    fn a_part_is_placed_with_the_mirror_term() {
        let bank = Bank {
            sheet: "s".into(),
            base: 100,
            cels: vec![[20, 30], [8, 8]],
            hit: Vec::new(),
        };
        let p = Part {
            table: 1,
            bank: 0,
            cel: 0,
            x: 5,
            y: -9,
            flags: 0,
        };
        let right = place(&p, &bank, (100, 50, 3), false).unwrap();
        assert_eq!((right.x, right.y, right.frame), (105, 44, 100));
        let left = place(&p, &bank, (100, 50, 3), true).unwrap();
        assert_eq!(left.x, 100 - (5 + 20), "task_x - (x + cel_width)");
        assert_eq!(left.y, 44, "y does not mirror");
        assert!(place(&Part { cel: 9, ..p }, &bank, (0, 0, 0), false).is_none());
    }

    /// The property every networked plan rests on: state that survives a
    /// snapshot exactly, and keeps agreeing once both copies run on.
    #[test]
    fn a_task_survives_a_round_trip_through_serialization() {
        let s = set(&[
            (
                "a",
                vec![
                    Instr::Loop { count: 4 },
                    Instr::Hold { count: 2 },
                    part(1),
                    Instr::EndFrame { end: End::Next },
                    Instr::Sound { sample: 3 },
                    part(2),
                    Instr::EndFrame { end: End::Loop },
                    Instr::Goto {
                        mode: 0,
                        target: "b".into(),
                    },
                    part(3),
                    Instr::EndFrame { end: End::Stop },
                ],
            ),
            ("b", frame_of(7)),
        ]);
        let hash = |t: &Task| {
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            t.hash_into(&mut |v| {
                h ^= v as u64;
                h = h.wrapping_mul(0x1000_0000_01b3);
            });
            h
        };

        let mut t = Task::new("a", 40, 90, FACING_LEFT);
        let mut a = TaskActor::with_health(30);
        for _ in 0..7 {
            t.step(&s, &mut a, false);
        }

        let json = serde_json::to_string(&t).unwrap();
        let mut restored: Task = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, t);
        assert_eq!(
            hash(&restored),
            hash(&t),
            "the fingerprint survives a reload"
        );

        let mut b = a.clone();
        for _ in 0..40 {
            t.step(&s, &mut a, false);
            restored.step(&s, &mut b, false);
            assert_eq!(
                hash(&restored),
                hash(&t),
                "and keeps agreeing tick for tick"
            );
        }
        assert_eq!(a, b);
    }
}
