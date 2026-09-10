//! What each creature does, rather than what a plain opponent would do.
//!
//! Build order item 37. The tracker and every creature's controller are named
//! routines in `MOON` and they read cleanly, so almost all of this is
//! transcription rather than design. The shape of the original is kept:
//!
//! * `MonsterTrack` decides *where* a creature wants to be, from the two
//!   ranges and the plane tolerance its `Set*Tables` routine wrote into the
//!   actor record (`+0x52`, `+0x54`, `+0x56`). It sets walk-direction bits in
//!   `+0x26` and answers "are you in range".
//! * The creature's own controller decides *what to do* once it is, and that
//!   is where the nine differ from one another. It writes the attack kind into
//!   `+0x28` and hands the task a script through `DS:0x783a`.
//!
//! The state each controller keeps lives in the actor record in the original
//! (`+0x0a` the walk frame, `+0x0b` a timer, `+0x48`/`+0x49` flags, `+0x4a` a
//! cooldown), so [`Brain`] lives on the fighter here for the same reason: it
//! is the creature's state, it serializes with the rest of the simulation, and
//! it goes into the fingerprint.
//!
//! Nothing in this module reads a clock, allocates randomness of its own, or
//! knows what a frame looks like. The one roll any of it makes
//! (`TroggAttacks`) goes through [`rnd`], which is the original's own
//! `_WIZARD:RND` transcribed, off a seed the bout carries.

use crate::combat::{Attack, Fighter, State};
use crate::content::ActorDef;
use crate::jump::Plan;
use serde::{Deserialize, Serialize};

/// Which of the original's controllers an actor runs.
///
/// The original dispatches on the actor's kind (`+0x35`) through
/// `CONTROLTABLE`, which `InitGameStart` fills. The name is carried on the
/// actor definition so a pack decides, rather than a match on an id in here.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Controller {
    /// `ControlTrogg`, and `TroggAttacks` with it: the axe and the hammer.
    Trogg,
    /// The same controller, taking the branch `TroggAttacks` makes for kind
    /// 0x10: one lunge, inside `+0x52`, and a long cooldown.
    TroggSpear,
    /// `ControlTroll`, `TrollAttack`.
    Troll,
    /// `ControlRatmen`, `ControlRatCollide`.
    Ratman,
    /// `ControlMudmen`, with `MudmenReach`, `MudmenIBury` and `MudmenAppear`.
    Mudman,
    /// `ControlBalok`.
    Balok,
    /// `ControlBeast`, `BeastCharge`, `SetBEASTZ`, `SetBeastTimer`.
    Beast,
    /// `ControlDemon`, `DemonAttack` and the four-phase whip.
    Demon,
    /// `ControlDragon`: the set piece, item 36.
    Dragon,
    /// `ControlClaw`: one of the dragon's two forelimbs.
    Claw,
    /// `ControlBlackKnight`, `BKnightMove`, `BKBlock`, `BKAttack`: the
    /// knight the machine plays, which is `CONTROLTABLE` slot 8. Slot 6 is
    /// `ControlKnight` and reads a joystick, so it never runs here.
    Knight,
}

impl Controller {
    pub fn from_name(name: &str) -> Option<Controller> {
        Some(match name {
            "trogg" => Controller::Trogg,
            "trogg_spear" => Controller::TroggSpear,
            "troll" => Controller::Troll,
            "ratman" => Controller::Ratman,
            "mudman" => Controller::Mudman,
            "balok" => Controller::Balok,
            "beast" => Controller::Beast,
            "demon" => Controller::Demon,
            "dragon" => Controller::Dragon,
            "claw" => Controller::Claw,
            "knight" => Controller::Knight,
            _ => return None,
        })
    }

    /// Whether a blow takes hit points off this creature at all.
    ///
    /// `ControlClaw` never calls `CalcDamage`: the dragon's two forelimbs are
    /// scenery that hits back, and the only thing that ends them is the
    /// dragon's own death (`Dragon_ClawDead`, and `DrDropClaws` after it).
    pub fn takes_damage(self) -> bool {
        self != Controller::Claw
    }

    /// Whether a landed blow of this creature's takes hold of what it hit,
    /// and for how many frames. `MudmenHit2` is the only one: it plays
    /// `Mudmen_EntangleKnight` and sets `+0x0b` to forty.
    pub fn seizes(self) -> Option<(&'static str, i32)> {
        match self {
            Controller::Mudman => Some(("Mudmen_EntangleKnight", 40)),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Controller::Trogg => "trogg",
            Controller::TroggSpear => "trogg_spear",
            Controller::Troll => "troll",
            Controller::Ratman => "ratman",
            Controller::Mudman => "mudman",
            Controller::Balok => "balok",
            Controller::Beast => "beast",
            Controller::Demon => "demon",
            Controller::Dragon => "dragon",
            Controller::Claw => "claw",
            Controller::Knight => "knight",
        }
    }
}

/// The controller state the original keeps in the actor record.
///
/// | here | original |
/// |---|---|
/// | `walk` | `+0x0a`, the walk frame, and the beast's charge frame |
/// | `timer` | `+0x0b`, the beast's pause and the mudman's hold |
/// | `flags` | `+0x48` and `+0x49`, and the per-creature `*FLAGS` word |
/// | `cooldown` | `+0x4a`, how long until this creature may strike again |
/// | `phase` | the demon's whip chain and the dragon's head, which the
///   original keeps as separate bits of `DemonFLAGS` and `DragonFLAGS` |
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Brain {
    pub cooldown: i32,
    pub timer: i32,
    /// `+0x28`, the kind of blow this actor is dealing, where its controller
    /// writes it outside an attack order.
    ///
    /// Only the beast needs it: `ControlBeast+17` (0x2fb2) writes 0x10 on
    /// every tick, before any branch, because the charge is its walk and it
    /// never enters an attack at all. Everything else writes `+0x28` on the
    /// frame it orders the blow, which is [`Act::Attack`]'s own kind.
    #[serde(default)]
    pub kind: Option<crate::combat::Attack>,
    pub flags: u32,
    pub walk: u32,
    pub phase: u8,
    /// Ticks until this controller may run again.
    ///
    /// **Judged and kept.** The original's task loop calls a controller once
    /// per game frame at most, when the task's `+1` is clear, and
    /// [`Fighter::ready`] is that test. What it is not is a *tick*: this
    /// engine ticks six times for each of the original's frames, so a
    /// controller gated only on `ready` would run six times per frame and
    /// every count one keeps, `TroggSwing`'s ten and `TroggSpear_Lunge`'s
    /// twenty among them, would run out six times too fast. This holds one
    /// controller call to one game frame, which is what the original does;
    /// removing it would not be more faithful, it would be six times faster.
    pub rest: i32,
    /// `+0x28` as `ControlBlackKnight` finds it, which is the kind of the
    /// last attack it ordered.
    ///
    /// `TroggStart` (0x2e08) zeroes `+0x28` at the top of every pass and
    /// `ControlBlackKnight` does not: it copies the field into `ATT`
    /// (0x4bb7) and then reads `ATT` back in `BKAttack` to keep from playing
    /// the same attack twice running. [`Fighter::attack`] is the same field,
    /// but this engine clears it when the fighter leaves the attack state, so
    /// the controller's own copy is kept here instead.
    #[serde(default)]
    pub att: Option<crate::combat::Attack>,
    /// The actor record's `+4`: how far off the ground this creature is,
    /// negative upward.
    ///
    /// `perdone` (0x99c6) carries it between the record and the task every
    /// frame, and `TASKRIGHT`/`TASKLEFT` place a part at `task_y + task_z +
    /// part_y`, so it is a screen offset and nothing else: a leaping ratman is
    /// drawn high and still stands, fights and sorts at the depth it left.
    /// This engine keeps the depth in `Fighter::y` and leaves `Task::z` free,
    /// so this is what goes into `Task::z`.
    #[serde(default)]
    pub height: i32,
    /// The slot in the table at DS:`0x76b2` this creature's jump is using.
    ///
    /// The original has six for the whole game and `ADDJUMP` (0x2b8d) walks
    /// them looking for one that is free or already this creature's; only the
    /// ratman and Balok ever ask, and neither can have two at once, so one per
    /// creature is the same table with the search taken out.
    #[serde(default)]
    pub jump: Option<crate::jump::Jump>,
}

/// `DemonFLAGS`, `DragonFLAGS`, `BalokFLAGS`, `MudmenFLAGS` and `+0x48`, as
/// far as any of them is reproduced.
pub mod flag {
    /// The demon has not made its entrance yet (`Demon_Evolve`).
    pub const UNBORN: u32 = 0x0001;
    /// `+0x48 & 0x10` on a mudman: it is under the ground.
    pub const BURIED: u32 = 0x0010;
    /// `MudmenFLAGS & 1`: it has hold of the knight.
    pub const ENTANGLING: u32 = 0x0020;
    /// `DemonFLAGS & 0x40`: the whip has caught the knight.
    pub const CAUGHT: u32 = 0x0040;
    /// `BeastFLAGS & 1`: alternate charges are dead on the knight's line.
    pub const ONLINE: u32 = 0x0100;
    /// This fighter has a controller of its own driving it, so it never falls
    /// back to standing on its own: it holds the last frame it was given
    /// until the controller names the next script, which is what `DS:0x783a`
    /// holding 0xffff means.
    pub const DRIVEN: u32 = 0x0200;
    /// `+0x48 & 1` on a ratman: it is in the air (`RatmanLeaps`, 0x3263).
    pub const LEAPING: u32 = 0x0400;
    /// `+0x48 & 4`: this leap is aimed at the tree rather than at the knight
    /// (`RatmanLeap+26`, 0x31c3).
    pub const TREE_BOUND: u32 = 0x0800;
    /// `+0x48 & 8`: it is in the tree (`RatWithinTree`, 0x32b4).
    pub const IN_TREE: u32 = 0x1000;
    /// `+0x48 & 0x10`: the eye gouge has been played and the leap away from
    /// the knight is due (`RatmanGouge`, 0x3388).
    pub const GOUGING: u32 = 0x2000;
    /// `+0x48 & 0x20`: it is sitting on the knight's head (`RatLeapHit+25`,
    /// 0x3558).
    pub const ON_HEAD: u32 = 0x4000;
    /// `+0x48 & 0x80`: a short hop rather than a leap, which `RatmanInitLeap`
    /// takes when the gap is forty or less (0x323b).
    pub const SHORT_HOP: u32 = 0x8000;
    /// `+0x49 & 1`: the knight has shaken it off his head and it is letting
    /// go (`RatmanOnHead+41`, 0x337c).
    pub const RELEASING: u32 = 0x0001_0000;
    /// `+0x49 & 4`: it has hold of the knight from the tree (`RatTailHit+5`,
    /// 0x3582).
    pub const HANGING: u32 = 0x0002_0000;
    /// `BalokGrabbed` (0x379b) has just run: the grab connected and
    /// `Balok_GrabKnight` is this frame's answer.
    ///
    /// The original has no bit for this because it does not need one:
    /// `BalokHit` is a branch *inside* `ControlBalok` and writes `[0x783a]`
    /// where it stands. This engine resolves a blow after the controllers have
    /// spoken, so the branch leaves a mark the controller reads on its next
    /// pass; `BalokFLAGS & 1`, which `BalokGrabbed` raises beside it, is what
    /// carries the frame after that.
    pub const GRABBED: u32 = 0x0004_0000;
}

/// The words a fight keeps for a whole species rather than for one creature.
///
/// Four of the five are the ratman's and Balok's own globals, and they are
/// globals in the original too: one rat leaping into the tree stops the next
/// from trying, one rat on the knight's head stops the next from landing
/// there, and the delay a landed claw buys is shared by every rat on the
/// screen. Kept on the bout, which is the smallest thing in this engine that
/// owns a whole fight.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shared {
    /// `RatFLAGS`, DS:`0x779c`. Bit 2 (`4`) the tree is taken, bit 3 (`8`)
    /// somebody is hanging off the knight, bit 5 (`0x20`) somebody is on his
    /// head. `InitKnightvsRatmen+67` (0x2360) zeroes it.
    pub rat: u32,
    /// `HitDelay`, DS:`0x779e`: fifteen frames after any rat's claw lands
    /// before any rat claws again (`RatmanHit+35`, 0x3513).
    pub hit_delay: i32,
    /// `BalokFLAGS`, DS:`0x7794`. Bit 0 it has the knight, bit 1 it is in the
    /// air, bit 5 the grab is over, bit 6 the bite or the squeeze is running.
    pub balok: u32,
    /// DS:`0x77a0`, the word `ControlBalokBite` (0x37c1) flips to alternate
    /// the bite and the squeeze.
    pub balok_bite: u32,
    /// DS:`0x779a`, how many frames of the current hop are left, which
    /// `BalokJumping` counts down beside the jump's own count.
    pub balok_hop: i32,
    /// `ShakeADD` (0x493f) was called this pass, which the bout turns into
    /// `ShakeCOUNT`. `BalokJumping+12` (0x374b) is the one controller that
    /// calls it, on a landing from a rise of eight or more; the only other
    /// caller in the game is the script `Troll_Chop`, whose gosub the bout
    /// sees for itself.
    #[serde(default)]
    pub shake: bool,
    /// DS:`0x7796`, the frame count the last `CalcJUMP` worked out.
    ///
    /// It is scratch in the original and every `CalcJUMP` overwrites it, but
    /// `BalokJumping` (0x36f5) reads it a frame after `BalokJump` wrote it, so
    /// it has to outlive the call.
    pub jump_steps: i32,
    /// `JumpHIEGHT` (DS:0x7798), which `CalcJUMP` (0x2b2e, 0x2b42, 0x2b55)
    /// writes and the Balok's landing reads back at 0x3741 to decide whether
    /// the thud is heavy enough to shake the screen. A global in the
    /// original, so it is whatever the last jump aimed at, and it is one
    /// here for the same reason.
    #[serde(default)]
    pub jump_height: i32,
    /// `DragonFLAGS`, DS:`0x7786`, the word the whole of `ControlDragon` keys
    /// off. `InitKnightvsDragon+84` (0x248c) zeroes it. See [`dragon_flag`].
    #[serde(default)]
    pub dragon: u32,
    /// `DDIS`, DS:`0x778a`: what `FindDistance` answered on the last
    /// `DragonMove` (0x387d), which `DragonAttack+38` (0x39fe) reads back.
    #[serde(default)]
    pub ddis: i32,
    /// `dragonbodge1`, `dragonbodge2` and `dragonbodge3`, DS:`0x77ec`,
    /// `0x77ee` and `0x77f0`. The first two are the two frames of nothing
    /// between one breath and the next (`DragonAttack+56`, 0x3a10, and
    /// `DragonLowAttack`, 0x3a51); the third is raised by `DragonHit2+26`
    /// (0x3aed) when the bite closes and read by `ControlKnight+21` (0x3ed7).
    /// `InitGameStart+380` (0x1d89) zeroes all three.
    #[serde(default)]
    pub dragon_bodge: [i32; 3],
    /// `DEAD_CLAWS`, DS:`0x77f4`: `InitKnightvsDragon+251` (0x2531) zeroes
    /// it, `DrDropClaws` (0x3be1) writes `0xffff`, and `ControlClaw` (0x3b24)
    /// kills its own task when it finds it so.
    #[serde(default)]
    pub dead_claws: i32,
    /// The dragon record's `+0x52` and `+0x54` as `TrackKnight` leaves them,
    /// once it has changed them from what `SetUpDragonTables` wrote.
    ///
    /// `TrackKnight+26` (0x3c02) saves `+0x52` into `DRN` and `TrackKnight+32`
    /// (0x3c08) saves `+0x52` **again** into `DCL`, and the restore at 0x3c7e
    /// puts `DRN` back into `+0x52` and `DCL` into `+0x54`. So the first
    /// breath the head tracks through leaves the back-off range equal to the
    /// approach range, sixty, for the rest of the fight. That is the code,
    /// and it is kept.
    #[serde(default)]
    pub dragon_ranges: Option<(i32, i32)>,
    /// `SLAPCNT`, DS:`0x7830`: the index [`BALOK_SLAP`] is read by.
    /// `InitSLAP` (0x44cb) writes `0xffff` into it, so the first `KnightSLAP`
    /// of a slap steps it to zero. See [`slap_entry`].
    #[serde(default)]
    pub slap_cnt: i32,
    /// `SLAP`, DS:`0x7832`: which way the slapped knight is flung, held as a
    /// facing byte — 1 right, 3 left, the same encoding as an actor's `+8`.
    ///
    /// **Eight sites write it**, and every one of them is a striker that can
    /// put the knight on `Knight_SwSlapped`:
    ///
    /// ```text
    /// 03b9f  mov word [SLAP], 1        ; ClawHit
    /// 043dc  mov byte [SLAP], 1        ; ClawStruck1
    /// 0363c  mov [SLAP], al            ; ControlBalok's uppercut, al = [si+8]
    /// 05697  mov [SLAP], al            ; TrollBunt
    /// 0505c  mov [SLAP], al            ; DemonAttack's slap
    /// 050a1  mov [SLAP], al            ; DemonAttack's whip
    /// 050d0  mov [SLAP], al            ; DemonOWhipFollow
    /// 0510b  mov [SLAP], al            ; DemonUWhipFollow
    /// ```
    ///
    /// A dragon's claw always bats rightward whichever side of the knight it
    /// is on; the Balok's uppercut, the troll's bunt and the demon's slap and
    /// whip all write their own `+8`, so those throw him the way the striker
    /// is facing.
    ///
    /// **`DemonSlap` (0x437f) is not one of the eight**, and an earlier note
    /// here read that as the demon slapping nobody. That was wrong.
    /// `DemonSlap` is the *struck* side of the blow and only turns the victim
    /// ([`struck_facing`]); the direction and the table are written a frame
    /// earlier by `DemonAttack` (0x5059..0x5062), on the controller's side,
    /// exactly as `ControlBalok` and `TrollBunt` write theirs. And
    /// `InitKnightvsDemon+40` (0x2765) puts `Knight_SwSlapped` on the knight's
    /// kind-0x10 row, so a demon's slap does throw him.
    #[serde(default)]
    pub slap: i32,
    /// `SLAPY`, DS:`0x782e`: the pointer `KnightSLAP` reads the throw's
    /// distances through, held here as the index into [`BALOK_SLAP`] that the
    /// pointer names. See [`slap_y`] for the two values it takes and
    /// [`slap_entry`] for the read.
    #[serde(default)]
    pub slap_y: i32,
}

/// `DragonFLAGS`' four bits, by the numbers the code tests.
pub mod dragon_flag {
    /// `0x10`: a head lift or lower is running (`DragonMove+2`, 0x386c).
    pub const HEAD_MOVING: u32 = 0x10;
    /// `0x20`: the head is up (`DragonMove+27`, 0x3885).
    pub const HEAD_UP: u32 = 0x20;
    /// `0x40`: the high breath is playing, so `TrackKnight` tracks to two
    /// (`DragonAttack+82`, 0x3a2a; `TrackKnight+20`, 0x3bfa).
    pub const BREATHING: u32 = 0x40;
    /// `0x80`: the knight has landed a blow, so the next high attack is the
    /// breath whatever the range (`DragonStruck+43`, 0x3a9c; `DragonAttack+93`,
    /// 0x3a35).
    pub const STRUCK: u32 = 0x80;
}

/// `BalokSLAP`, DS:`0x7818` (image 0x19bc8): how far a slapped knight is
/// thrown on each frame of the throw.
///
/// Eleven words read straight out of the load image, and eleven is exactly
/// its length — `SLAPY` begins at DS:`0x782e` with no gap after it:
///
/// ```text
/// 1e 00  19 00  14 00  14 00  14 00  f9 ff  fd ff  ff ff  00 00  00 00  00 00
/// 30     25     20     20     20     -7     -3     -1     0      0      0
/// ```
///
/// So the intent legible in the data is an arc: flung hard, coasting, a small
/// rebound the other way, and at rest.
///
/// **`SLAPY` (DS:`0x782e`), the pointer `KnightSLAP` reads the table
/// through, takes two values, and both of them live inside these eleven
/// words.** Six instructions in the image store to `0x782e`:
///
/// ```text
/// 0363f  mov word [SLAPY], BalokSLAP   ; ControlBalok's uppercut
/// 03ba5  mov word [SLAPY], BalokSLAP   ; ClawHit
/// 043e1  mov word [SLAPY], BalokSLAP   ; ClawStruck1
/// 0505f  mov word [SLAPY], BalokSLAP   ; DemonAttack's slap
/// 050a4  mov word [SLAPY], DemonWHIP   ; DemonAttack's whip
/// 0569a  mov word [SLAPY], BalokSLAP   ; TrollBunt
/// ```
///
/// `DemonWHIP` is DS:`0x7822`, which is `BalokSLAP + 10`: the same storage,
/// five words in. So the whip reads -7, -3, -1, 0 where a slap reads 30, 25,
/// 20, 20 — the whip drags the knight *towards* the demon, and a slap throws
/// him away. `InitSLAP` does not touch the pointer and nothing else in the
/// image stores to it, so [`Shared::slap_y`] is that index and [`slap_y`]
/// holds the two values: it is a pointer into one table, not a choice of two.
///
/// **Three of the eleven words are dead.** `Knight_SwSlapped` (DS:`0x1596`)
/// is the only script in the image that gosubs either routine, and it is two
/// frames long:
///
/// ```text
/// 1596  TASKSOUND 12 / TASKSOUND 08
/// 159a  TASKGOSUB InitSLAP
/// 159e  TASKHOLD 02 / TASKGOSUB KnightSLAP / two PARTs / ENDFRAME 00
/// 15b2  TASKHOLD 02 / TASKGOSUB KnightSLAP / three PARTs / ENDFRAME 00
/// 15cc  TASKGOTO Knight_GetUp
/// ```
///
/// Each frame is held twice and the gosub sits *after* the `TASKHOLD`, so it
/// runs on both shows: `TASKHOLD`'s handler (0x9aad) stores the resume point
/// as the instruction after itself —
///
/// ```text
/// 09ac1  mov byte [bx+1], 1
/// 09ac5  add word [di+2], 2          ; past the two-byte TASKHOLD
/// 09ac9  mov ax, [di+2]; mov [bx+2], ax
/// ```
///
/// — and the end-of-frame handler (0x9947) puts that back into `task+2` for
/// every held show. So `KnightSLAP` runs four times and `SLAPCNT` runs -1, 0,
/// 1, 2, 3 whichever word the pointer started on. A slap therefore reads
/// words 0 to 3 — 30, 25, 20, 20, ninety five pixels over four shows of two
/// frames — and the whip words 5 to 8, eleven pixels the other way. Word 4,
/// the last of the steady run, and words 9 and 10, the tail of the rebound,
/// were authored and never reached.
#[rustfmt::skip]
pub const BALOK_SLAP: [i32; 11] = [30, 25, 20, 20, 20, -7, -3, -1, 0, 0, 0];

/// The two words of [`BALOK_SLAP`] that `SLAPY` is ever pointed at, as
/// indices into it. The image's six stores to DS:`0x782e` name one or the
/// other and nothing else; see the table's own note.
pub mod slap_y {
    /// `BalokSLAP` itself, DS:`0x7818`: the claw's, the Balok's uppercut's,
    /// the troll's bunt's and the demon's slap's table.
    pub const BALOK_SLAP: i32 = 0;
    /// `DemonWHIP`, DS:`0x7822`, which is `BalokSLAP + 10` and so five words
    /// in: `DemonAttack`'s whip branch (0x50a4) is its only writer.
    pub const DEMON_WHIP: i32 = 5;
}

/// `[SLAPY + SLAPCNT*2]` as `KnightSLAP` reads it (0x4504..0x4512, and the
/// same six instructions again at 0x4538..0x4546):
///
/// ```text
/// 04504  mov di, [SLAPY]
/// 04508  mov ax, [SLAPCNT]; shl ax, 1
/// 0450d  push di; add di, ax; mov bx, [di]; pop di
/// ```
///
/// The read is unmasked and unchecked, as `progression_at`'s is. `base` is
/// [`Shared::slap_y`], the word of [`BALOK_SLAP`] the pointer names.
//
// TODO: what the original reads past the table's eleventh word is known —
// DS:`0x782e` is `SLAPY` (0x7818, the table's own address), `0x7830` is
// `SLAPCNT` and `0x7832` is `SLAP`, so indices 11, 12 and 13 would throw a
// knight 30744 pixels, then by the count, then by the direction — but no
// shipped script can reach it: `Knight_SwSlapped` is the only caller of
// `KnightSLAP` and runs it four times, from word 0 for a slap and word 5 for
// the whip. Nothing is built for the overrun and the step is skipped instead,
// because modelling it would mean giving `SLAPY` and `SLAP` word values for
// the sake of an unreachable branch.
pub fn slap_entry(base: i32, cnt: i32) -> Option<i32> {
    if cnt < 0 {
        return None;
    }
    let at = usize::try_from(base.checked_add(cnt)?).ok()?;
    BALOK_SLAP.get(at).copied()
}

/// The tail of `KnightSLAP` (0x4513) and of `KnightSLAPR` (0x4547): the
/// table's entry taken off or added to the task's own column `+4`, and the
/// clamp that follows — on one side only.
///
/// ```text
/// KnightSLAP, flung left:            KnightSLAPR, flung right:
/// 04513  sub word [si+4], bx         04547  add word [si+4], bx
/// 04516  cmp word [si+4], 0xa        0454a  cmp word [si+4], 0x140
/// 0451a  jge 04521                   0454f  jle 04551
/// 0451c  mov word [si+4], 0xa        ---- nothing ----
/// 04521  ret                         04551  ret
/// ```
///
/// **The rightward clamp is not there, and this is built without it on
/// purpose.** The bytes at 0x454a are `81 7c 04 40 01` (`cmp word [si+4],
/// 0x140`) followed by `7e 00` and then `c3`: a `jle` whose displacement is
/// **zero**, so the taken and the untaken branch both land on the same `ret`
/// and there is no clamp body to skip over. The leftward side is the same
/// shape with a real body — `83 7c 04 0a` (`cmp`), `7d 05` (a `jge` over five
/// bytes) and `c7 44 04 0a 00` (`mov word [si+4], 0xa`). The compare survives
/// on the right and its consequence does not, so either the symmetric clamp
/// was never written or it was deleted and the compare left behind. In the
/// shipped game a knight batted rightward has no upper bound of his own where
/// one batted leftward is stopped at column 10.
///
/// What keeps him on the board rightward is then not this routine but the
/// global clamp every position goes through on the next displayed frame
/// (`Fighter::walk` and `run_task`, both on `arena::GLOBAL`, which is
/// `CheckBorder`'s own `X_LOW`..`X_HIGH`). Do not "fix" the asymmetry here:
/// it is the original's, and
/// `bout::tests::the_rightward_slap_has_no_clamp_of_its_own` pins it.
pub fn slap_move(x: i32, slap: i32, entry: i32) -> i32 {
    // 044e8  cmp byte [SLAP], 1; 044ed je KnightSLAPR. Only exactly 1 is
    // rightward; 3, the other value anything writes, falls through to left.
    if slap == 1 {
        // 04547, and then the compare with no clamp under it.
        x + entry
    } else {
        // 04513, 04516, 0451a, 0451c.
        let moved = x - entry;
        if moved < 0xa {
            0xa
        } else {
            moved
        }
    }
}

/// `TalismanWrym`, image 0x43f4: what the dragon's blow comes down to for a
/// knight carrying Talismans of the Wyrm.
///
/// ```text
/// 043f4  mov bx, [di+0x44]         ; the knight's magic record
/// 043f7  xor cx, cx
/// 043f9  mov cl, [bx+8]            ; how many talismans, slot 8
/// 043fc  shr ax, cl                ; the blow, halved once per talisman
/// 043fe  cmp ax, 5
/// 04401  jg  04406
/// 04403  mov ax, 5                 ; and never under five
/// 04406  ret
/// ```
///
/// `DragonStruck1` (0x43ad) puts the bite's twenty and the fire's thirty
/// through it, `ClawStruck1` (0x43d3) the claw's ten, and nothing else calls
/// it. `shr ax, cl` on a 16 bit register with a count over fifteen is zero,
/// which the floor then lifts to five.
pub fn talisman_wrym(blow: i32, talismans: i32) -> i32 {
    let cl = talismans.clamp(0, 31) as u32;
    // 043fc  shr ax, cl
    let ax = (blow as u16).checked_shr(cl).unwrap_or(0) as i32;
    // 043fe  cmp ax, 5; jg; mov ax, 5
    if ax > 5 {
        ax
    } else {
        5
    }
}

/// The knight sheet's endurance, `+0x30` of the actor record, which
/// `RatLeapHit+34` (0x3561) reads to size how long a rat sits on his head.
/// It is the third of `CheckMaxAbility`'s three abilities (0xb7e1).
pub const ENDURANCE: i16 = 0x30;

/// `RatFLAGS`' three bits, by the numbers the code tests.
pub mod rat_flag {
    /// `4`: a rat has gone for the tree.
    pub const TREE: u32 = 4;
    /// `8`: a rat is hanging off the knight.
    pub const HANGING: u32 = 8;
    /// `0x20`: a rat is on the knight's head.
    pub const ON_HEAD: u32 = 0x20;
}

/// `BalokFLAGS`' four bits.
pub mod balok_flag {
    /// `1`: it has hold of the knight (`BalokGrabbed`, 0x37a6).
    pub const HELD: u32 = 1;
    /// `2`: a hop is running (`BalokJump+84`, 0x36c3).
    pub const JUMPING: u32 = 2;
    /// `0x20`: the grab is done with and the knight is to be let go
    /// (`ControlBalokGrab+11`, 0x37b9).
    pub const RELEASING: u32 = 0x20;
    /// `0x40`: a bite or a squeeze is playing (`ControlBalokBite+19`,
    /// 0x37d4).
    pub const CHEWING: u32 = 0x40;
}

/// What a controller decided to do this tick.
///
/// The original writes a script offset into `DS:0x783a` and a kind into
/// `+0x28`; this is that, named.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    /// Stand. `[0x783a]` left at the stance, or at `0xffff` for "carry on".
    Idle,
    /// Walk, optionally on a named script rather than the walk cycle.
    Walk {
        dx: i32,
        dy: i32,
        script: Option<String>,
    },
    /// One of this actor's own attacks, by the kind its routine writes.
    /// `spawn` is the task the routine starts beside it: `AddDragonFIRE` is
    /// the only one, and the fire is a task of its own rather than a part of
    /// the breath's own script.
    Attack { kind: Attack, spawn: Option<String> },
    /// Stand, on a named script. The dragon's stance is `Dragon_Stance` with
    /// its head down and `Dragon_HighStance` with it up, which the original
    /// swaps by having `Dragon_LiftHead1` write the new one into `+0x10`.
    Stand(String),
    /// `[0x783a]` left at zero: the task is killed and the record freed,
    /// which is `CLAWS_DEAD` (0x3b61) and nothing else a controller does.
    Vanish,
    /// Play this script outright, committed, with no blow of its own.
    Play(String),
    /// `MudmenAppear`: surface at this x, facing this way, on this script.
    Appear { x: i32, facing: i32, script: String },
    /// Take hold of the target for this many script frames.
    /// `MudmenHit2` does it when the arm lands.
    Seize { script: String, ticks: i32 },
    /// The blow a controller deals directly rather than through a weapon
    /// part: the demon's whip when it catches, the mudman's choke.
    Strike {
        script: String,
        damage: i32,
        fatal: bool,
        /// A script handed to the victim outright, rather than letting his
        /// own `*Hit` row decide what he plays.
        ///
        /// `DemonOWhipFollow` (0x50de) and `DemonUWhipFollow` (0x5119) both
        /// do exactly that: `mov si, 0x1596` -- `Knight_SwSlapped` -- then
        /// `call REPLACEANIM` (0x97ee) on the knight's own record, which
        /// `mov ax, [0x978]` / `call 0x9886` fetched two instructions
        /// earlier. Nothing about the blow is consulted; the whip has him,
        /// so he plays the thrown script and reads `SLAPY` through it.
        victim: Option<String>,
    },
    /// Held, and trying to get out of it. `MudmenEntangle` reads fire and
    /// down together off the joystick and nothing else, so this is that press
    /// rather than an order.
    Struggle,
    /// One frame of a ballistic arc: `RatmanLeaping` (0x3270) and
    /// `BalokJumping` (0x36d1) both write `ControlJump`'s three answers
    /// straight into `+2`, `+6` and `+4` and then name a frame to draw.
    ///
    /// The height goes into [`Brain::height`], since the controller owns it;
    /// `x` and `y` are the record's own and the bout writes them.
    Fly { x: i32, y: i32, script: String },
    /// One frame of a hold, from either side of it.
    ///
    /// `RatHangKnight` (0x32ed), `RatmanOnHead` (0x3353), `RatmanGouged`
    /// (0x3395) and `RatmanReleaseKnight` (0x343d) are the ratman's;
    /// `ControlBalokBite` (0x37c1), `ControlBalokCrush` (0x37dc) and
    /// `ControlBalokRelease` (0x37f0) are Balok's. All seven play a script of
    /// their own while the one held is off the board, take hit points off him
    /// without a blow-taken script of his own, and let go at the end.
    Grip {
        /// What this creature plays.
        script: String,
        /// Off the one held, this frame: one for `RatHangKnight`, five for
        /// `RatmanGouged`, all of them for a `KillKnight`.
        damage: i32,
        /// Off this creature, this frame: `RatHangKnight+29` (0x330a) puts the
        /// knight's own blow through `CalcDamage` and takes it off the rat.
        cost: i32,
        /// Everything he has left, `KillKnight` (0xab2).
        fatal: bool,
        /// Keep hold of him, or let go.
        hold: bool,
        /// What the one held is put on by `REPLACEANIM`: `Knight_Explode`
        /// under Balok's landing, `Knight_GetUp` when a hanging ratman is
        /// killed. Empty leaves him on whatever his own state names.
        victim: String,
    },
}

// --------------------------------------------------------------- the tracker
//
// Everything in this section is a literal translation of the routines at the
// addresses quoted, read off `research/main.final.bin` with the symbol table.
// The actor record fields they touch, as the code itself uses them:
//
//   +2   x            +6   z (the depth row)        +8   facing byte, 1 right,
//   +0x26 direction bits, 1 right 2 left 4 down 8 up      3 left (bit 1 mirrors)
//   +0x52 approach range   +0x54 back-off range   +0x56 plane tolerance
//
// `[0x77e8]` is the record the controller is running (`me`), `[0x77ea]` is
// `Opponent`. A facing of 1 is `+1` here and 3 is `-1`; the task VM turns it
// back into the byte the blit reads.

/// `FaceKnight`, image 0x3cf3.
///
/// ```text
/// 03cf5  mov di, [0x77e8]          ; me
/// 03cf9  mov si, [0x77ea]          ; Opponent
/// 03cfd  mov ax, [di+2]            ; me.x
/// 03d00  cmp ax, [si+2]            ; against foe.x
/// 03d03  jl  03d0c
/// 03d05  mov byte [di+8], 3        ; me.x >= foe.x: face left
/// 03d0c  mov byte [di+8], 1        ; me.x <  foe.x: face right
/// ```
///
/// Returns what it writes into `+8`. Equal x faces left: the branch is `jl`.
pub fn face_knight(me: &Fighter, foe: &Fighter) -> i32 {
    face_from(me.x, foe)
}

/// `FaceKnight` with `me.x` handed in, for a record about to be moved.
fn face_from(me_x: i32, foe: &Fighter) -> i32 {
    if me_x < foe.x {
        // 03d0c  mov byte ptr [di + 8], 1
        1
    } else {
        // 03d05  mov byte ptr [di + 8], 3
        -1
    }
}

/// `FindSide`, image 0x3d3c: the same comparison as `FaceKnight`, answered in
/// `ax` (3 or 1) rather than written into the record.
///
/// ```text
/// 03d46  mov ax, [di+2]
/// 03d49  cmp ax, [si+2]
/// 03d4c  jl  03d54
/// 03d4e  mov ax, 3
/// 03d54  mov ax, 1
/// ```
fn find_side(me_x: i32, foe: &Fighter) -> i32 {
    if me_x < foe.x {
        1
    } else {
        3
    }
}

/// `CheckZ`, image 0x3c9b, as `CheckZAxis` (0x3c8b) calls it with `di` the
/// record and `si` the opponent.
///
/// ```text
/// 03c9b  mov ax, 0
/// 03c9e  mov bx, [si+6]            ; foe.z
/// 03ca1  mov cx, [di+6]            ; me.z
/// 03ca4  sub bx, cx
/// 03ca6  jns 03caa
/// 03ca8  neg bx                    ; |foe.z - me.z|
/// 03caa  cmp bx, [di+0x56]
/// 03cad  jg  CheckZDone            ; further than the tolerance: ax stays 0
/// 03caf  mov ax, 1
/// ```
///
/// This engine keeps the depth row in `Fighter::y`, which is the original's
/// `+6`.
fn check_z(me: &Fighter, foe: &Fighter, def: &ActorDef) -> bool {
    check_z_from(me.y, foe, def)
}

/// `CheckZ` with `me.z` handed in.
fn check_z_from(me_y: i32, foe: &Fighter, def: &ActorDef) -> bool {
    let mut bx = foe.y - me_y;
    if bx < 0 {
        bx = -bx;
    }
    bx <= def.depth_tolerance
}

/// `CheckXAxis`, image 0x3cb3, with `bp` the range to test against. Answers
/// `ax`, and leaves `bx` holding the distance, which the callers rely on.
///
/// ```text
/// 03cbd  mov ax, 0
/// 03cc0  mov bx, [si+2]            ; foe.x
/// 03cc3  mov cx, [di+2]            ; me.x
/// 03cc6  sub bx, cx
/// 03cc8  jns 03ccc
/// 03cca  neg bx                    ; |foe.x - me.x|
/// 03ccc  cmp bx, bp
/// 03cce  jg  CheckXDone            ; further than bp: ax stays 0
/// 03cd0  mov ax, 1
/// ```
fn check_x_axis(me_x: i32, foe: &Fighter, bp: i32) -> (bool, i32) {
    let mut bx = foe.x - me_x;
    if bx < 0 {
        bx = -bx;
    }
    (bx <= bp, bx)
}

/// `FindDistance`, image 0x3cd6.
///
/// ```text
/// 03ce3  mov ax, [si+2]            ; me.x
/// 03ce6  sub ax, [di+2]            ; minus foe.x
/// 03ce9  jns 03cf0
/// 03ceb  neg ax
/// ```
pub fn find_distance(me: &Fighter, foe: &Fighter) -> i32 {
    let mut ax = me.x - foe.x;
    if ax < 0 {
        ax = -ax;
    }
    ax
}

/// What `MonsterTrack` answers, and what it wrote while answering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Track {
    /// The direction bits it set in `+0x26`: bit 1 is `dx = 1`, bit 2 is
    /// `dx = -1`, bit 8 (`MoveU`) is `dy = -1`, bit 4 (`MoveD`) is `dy = 1`.
    pub dx: i32,
    pub dy: i32,
    /// `ZPLANE`: the two are within the creature's `+0x56` of each other.
    pub plane: bool,
    /// Its return value in `ax`: 1 when the creature should walk, 0 when it
    /// is on the same plane, inside `+0x52` and outside `+0x54`, which is
    /// when its own controller gets to choose an attack.
    pub walking: bool,
    /// `bx` on the way out, which is `|foe.x - me.x|` from `CheckXAxis`.
    pub distance: i32,
}

/// `MonsterTrack`, image 0x56d9, translated line for line.
///
/// `facing` is the record's `+8`, which `FaceKnight` writes before anything
/// else is looked at. That is why a creature that walks away from the knight
/// still faces him, and why one that has stopped to swing has already turned.
///
/// ```text
/// 056d9  mov [TrackX], 0
/// 056df  mov [TrackZ], 0
/// 056e5  mov [ZPLANE], 0
/// 056eb  mov [DiagnolZ], 0
/// 056f1  mov si, [0x77e8]          ; me
/// 056f5  mov di, [0x77ea]          ; Opponent
/// 056f9  call FaceKnight
/// 056fc  mov [TrackFLAG], 0
/// 05702  call CheckZAxis
/// 05705  or  ax, ax
/// 05707  je  05711
/// 05709  mov [ZPLANE], 1
/// 0570f  jmp SameZPlane
/// 05711  mov bx, [si+6]            ; me.z
/// 05714  cmp bx, [di+6]            ; foe.z
/// 05717  jle 05725
/// 05719  or  byte [si+0x26], 8     ; deeper than him: up
/// 0571d  mov [TrackFLAG], 1
/// 05723  jmp SameZPlane
/// 05725  or  byte [si+0x26], 4     ; nearer than him: down
/// 05729  mov [TrackFLAG], 1
/// SameZPlane:
/// 0572f  mov bp, [si+0x54]
/// 05732  call CheckXAxis
/// 05735  or  ax, ax
/// 05737  jne TrackBack
/// 05739  mov bp, [si+0x52]
/// 0573c  call CheckXAxis
/// 0573f  or  ax, ax
/// 05741  je  TrackOpponent
/// 05743  mov ax, 0
/// 05746  mov ax, [TrackFLAG]
/// 05749  ret
/// TrackOpponent:
/// 0574a  call FindSide
/// 0574d  mov bx, [si+0x52]
/// 05750  cmp ax, 1
/// 05753  je  TrackRight
/// 05755  or  byte [si+0x26], 2     ; left
/// 05759  jmp TrackCollide
/// TrackRight:
/// 0575b  or  byte [si+0x26], 1     ; right
/// TrackCollide:
/// 0575f  mov ax, 1
/// 05762  ret
/// TrackBack:
/// 0576b  mov ax, [si+2]            ; me.x
/// 0576e  mov bx, [di+2]            ; foe.x
/// 05771  sub bx, ax
/// 05773  jns T1$
/// 05775  or  byte [si+0x26], 1     ; he is to the left: walk right
/// 05779  jmp T2$
/// T1$:
/// 0577b  or  byte [si+0x26], 2     ; he is to the right: walk left
/// T2$:
/// 0577f  mov ax, 1
/// 05782  ret
/// ```
pub fn track(me: &Fighter, foe: &Fighter, def: &ActorDef, facing: &mut i32) -> Track {
    track_from((me.x, me.y), foe, def, (def.approach, def.back_off), facing)
}

/// `MonsterTrack` with the record's `+2`, `+6`, `+0x52` and `+0x54` handed in
/// rather than read off the fighter: `TrackKnight` (0x3c0e, 0x3c13) writes
/// two and one into the ranges before it calls the tracker, and the position
/// it tracks from is the one it is about to move.
fn track_from(
    at: (i32, i32),
    foe: &Fighter,
    def: &ActorDef,
    ranges: (i32, i32),
    facing: &mut i32,
) -> Track {
    let (approach, back_off) = ranges;
    let (me_x, me_y) = at;
    let mut dx = 0;
    let mut dy = 0;
    // 056eb..056f5: the globals are cleared and the two records picked up.
    // 056f9  call FaceKnight
    *facing = face_from(me_x, foe);
    // 056fc  mov [TrackFLAG], 0
    let mut track_flag = false;
    let mut plane = false;
    // 05702  call CheckZAxis
    if check_z_from(me_y, foe, def) {
        // 05709  mov [ZPLANE], 1
        plane = true;
    } else if me_y > foe.y {
        // 05711..05717: me.z > foe.z
        // 05719  or byte ptr [si + 0x26], 8    (MoveU)
        dy = -1;
        // 0571d  mov [TrackFLAG], 1
        track_flag = true;
    } else {
        // 05725  or byte ptr [si + 0x26], 4    (MoveD)
        dy = 1;
        // 05729  mov [TrackFLAG], 1
        track_flag = true;
    }
    // SameZPlane:
    // 0572f  mov bp, [si+0x54]; 05732 call CheckXAxis; 05737 jne TrackBack
    let (inside_back_off, distance) = check_x_axis(me_x, foe, back_off);
    if inside_back_off {
        // TrackBack:
        // 0576b..05773: bx = foe.x - me.x; jns T1$
        if foe.x - me_x < 0 {
            // 05775  or byte ptr [si + 0x26], 1
            dx = 1;
        } else {
            // 0577b  or byte ptr [si + 0x26], 2
            dx = -1;
        }
        // 0577f  mov ax, 1
        return Track {
            dx,
            dy,
            plane,
            walking: true,
            distance,
        };
    }
    // 05739  mov bp, [si+0x52]; 0573c call CheckXAxis; 05741 je TrackOpponent
    let (inside_approach, distance) = check_x_axis(me_x, foe, approach);
    if inside_approach {
        // 05746  mov ax, [TrackFLAG]; 05749 ret
        return Track {
            dx,
            dy,
            plane,
            walking: track_flag,
            distance,
        };
    }
    // TrackOpponent:
    // 0574a  call FindSide; 05750 cmp ax, 1; 05753 je TrackRight
    if find_side(me_x, foe) == 1 {
        // 0575b  or byte ptr [si + 0x26], 1
        dx = 1;
    } else {
        // 05755  or byte ptr [si + 0x26], 2
        dx = -1;
    }
    // 0575f  mov ax, 1
    Track {
        dx,
        dy,
        plane,
        walking: true,
        distance,
    }
}

// ------------------------------------------------------------------- the roll

/// `_WIZARD:RND`, transcribed: eight rounds of a shift register.
///
/// Each round takes `ror(seed, 3) ^ seed`, keeps bit 1 of it as the bit
/// shifted into the top, and shifts the seed right one. The original keeps the
/// word at DS:`0xe22f`; here the bout carries it, so a fight is reproducible
/// and two machines roll the same way.
pub fn rnd(seed: u16) -> u16 {
    let mut ax = seed;
    for _ in 0..8 {
        let t = ax.rotate_right(3) ^ ax;
        let carry = (t >> 1) & 1;
        ax = (carry << 15) | (ax >> 1);
    }
    ax
}

/// `_WIZARD:GETPERCENT`: a roll, masked to seven bits, folded into 0..=99.
pub fn percent(seed: &mut u16) -> i32 {
    *seed = rnd(*seed);
    let v = (*seed & 0x7f) as i32;
    if v >= 100 {
        v - 27
    } else {
        v
    }
}

// -------------------------------------------------------------- the decisions

/// Everything a controller needs to know about the fight, gathered once.
pub struct Sight<'a> {
    pub me: &'a Fighter,
    pub foe: &'a Fighter,
    pub def: &'a ActorDef,
    /// The title screen's gore switch, on. `TroggAttack` reads it before it
    /// comes in for a fallen knight.
    pub gore: bool,
    /// Whether the fallen knight still has a body worth one more blow.
    pub body: bool,
    /// Whether anyone has taken the finisher yet: `DeCapFLAG`.
    pub decapped: bool,
    /// `DS:0x5b1`, the day count, which is what `BKBlock` and `BKAttack`
    /// index [`PROGRESSION`] with. `InitGameStart+60` (0x1c49) writes zero and
    /// `EncounterFini+35` (0x1167) adds one every fourth encounter, in step
    /// with `MoonCount` two bytes along; nothing else touches it. So the
    /// computer knight's nerve is the calendar: the longer the game has run,
    /// the less often it hesitates.
    pub progression: i32,
    /// Where the ratmen's tree stands: its column, the depth row its foot is
    /// on, and how far above that its branches are.
    ///
    /// `InitKnightvsRatmen+82` (0x236f) puts one actor in the arena beside the
    /// creatures, on `Rat_TreeBrush`, at x `0xa0`, `y` = `HalfSCAPE - 0xc8` and
    /// `z` = `HalfSCAPE`, and keeps its record in `TreeHANDLE` (DS:`0x69ae`).
    /// It is the only thing that reads it: `RatmanLeap+30` (0x31c7) finds its
    /// task and aims the leap at it.
    pub perch: Option<Perch>,
    /// What the opponent's blow would take off this creature: `CalcDamage`
    /// (0x2d67) called with the opponent in `si`, which `RatHangKnight+29`
    /// (0x330a) is the one controller that does.
    pub foe_blow: i32,
    /// `[0x6e26+0x38]`, the dragon's own hit points, which `ControlClaw+43`
    /// (0x3b4e) reads off the fixed record whichever claw is running. None
    /// when there is no dragon in the fight.
    pub head_health: Option<i32>,
}

/// The tree at DS:`0x69ae`, as far as anything in a fight cares about it.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Perch {
    pub x: i32,
    /// The depth row, which this engine keeps as a fighter's `y`.
    pub y: i32,
    /// `+4`, negative upward.
    pub height: i32,
}

/// One tick of one creature's own controller.
///
/// `seed` is the bout's roll register; only `TroggAttacks` spends it, which is
/// why it is threaded rather than stored.
///
/// `facing` is the record's `+8`, handed in as it stands and handed back as
/// the controller leaves it. The controllers write it directly in the
/// original (`FaceKnight` from `MonsterTrack` and `ControlRatCollide`,
/// `ControlBalok+101`, `BeastCharge`, `TrackKnight+55`), and the task loop
/// copies it into the task on the way out (`NOTEND+20`, `mov dh, [di+8]`, to
/// `TASKHANDLE` 0x9741, `mov [di+0x14], dh`). A controller that never
/// touches it leaves the creature facing the way it was.
///
/// `at` is the record's `+2` and `+6` the same way: `TrackKnight` (0x3c36,
/// 0x3c49, 0x3c51, 0x3c5a) writes them in place and `DragonMove` reads the
/// distance off what it wrote (0x387a), so the dragon's controller hands its
/// position back beside its facing. Every other controller leaves it alone.
pub fn decide(
    s: &Sight,
    brain: &mut Brain,
    seed: &mut u16,
    facing: &mut i32,
    shared: &mut Shared,
    at: &mut (i32, i32),
) -> Act {
    match s.def.controller() {
        Controller::Trogg => trogg(s, brain, seed, false, facing),
        Controller::TroggSpear => trogg(s, brain, seed, true, facing),
        Controller::Troll => troll(s, brain, facing, shared),
        Controller::Ratman => ratman(s, brain, facing, shared),
        Controller::Mudman => mudman(s, brain, facing),
        Controller::Balok => balok(s, brain, facing, shared),
        Controller::Beast => beast(s, brain, seed, facing, at),
        Controller::Demon => demon(s, brain, facing, shared),
        Controller::Dragon => dragon(s, brain, facing, shared, at),
        Controller::Claw => claw(s, shared),
        Controller::Knight => black_knight(s, brain, seed, facing),
    }
}

/// The walk an ordinary tracker asks for, or standing still.
fn walk(t: Track) -> Act {
    if t.dx == 0 && t.dy == 0 {
        Act::Idle
    } else {
        Act::Walk {
            dx: t.dx,
            dy: t.dy,
            script: None,
        }
    }
}

/// Tick the cooldown the way `DemonAttack` (0x5029) does: down by one, and
/// the demon is free on the frame it reaches zero.
///
/// ```text
/// 05029  cmp byte [si+0x4a], 0
/// 0502d  je  05035                 ; zero: attack
/// 0502f  sub byte [si+0x4a], 1
/// 05033  jne DemonMove             ; still counting: move
/// 05035  ...                       ; reached zero this frame: attack
/// ```
///
/// `TroggAttacks` does **not** share this shape: its decrement is followed by
/// an unconditional `jmp` to the tail (0x2eb1), so the trogg waits one frame
/// longer. It has its own test in [`trogg`] and must not use this.
fn cooling(brain: &mut Brain) -> bool {
    if brain.cooldown > 0 {
        brain.cooldown -= 1;
        return brain.cooldown > 0;
    }
    false
}

/// `MoveBACK`, image 0x5783: which way the walk cycle is stepped. Returns the
/// `bp` it leaves, 1 forwards and -1 backwards.
///
/// ```text
/// 05783  mov si, [0x77e8]          ; me
/// 05787  mov al, [si+8]            ; facing
/// 0578a  cmp al, 1
/// 0578c  je  MoveBACKR
/// 0578e  mov bp, 1                 ; not facing right (3)...
/// 05791  test byte [si+0x26], 2
/// 05795  jne m1$                   ; ...and walking left: forwards
/// 05797  mov bp, -1                ; walking right: backwards
/// 0579a  ret
/// MoveBACKR:
/// 0579b  mov bp, 1                 ; facing right...
/// 0579e  test byte [si+0x26], 1
/// 057a2  jne mm1$                  ; ...and walking right: forwards
/// 057a4  mov bp, -1                ; walking left: backwards
/// 057a7  ret
/// ```
///
/// `MoveU` (0x4e39) and `MoveD` (0x4e64) set `bp` to 1 themselves and are
/// called before `MoveR` and `MoveL`, which call this, so a step with any
/// sideways part takes its sign from here and a purely vertical one goes
/// forwards. `NextWalk` (0x4ef7) then does `add byte ptr [si+0xa], al` with
/// `al = bp`. Nothing in any of this writes `+8`: walking away from the
/// knight plays the walk backwards and leaves the creature facing him.
///
/// `facing` is the record's `+8` as this module carries it (1 right, -1
/// left); `dx` is the walk bit, 1 right, -1 left, 0 neither.
pub fn move_back(facing: i32, dx: i32) -> i32 {
    if dx == 0 {
        // MoveU / MoveD: `mov bp, 1`, and no call to MoveBACK.
        return 1;
    }
    if facing == 1 {
        // 0579b  mov bp, 1; 0579e test byte [si+0x26], 1; 057a2 jne mm1$
        if dx > 0 {
            1
        } else {
            -1
        }
    } else {
        // 0578e  mov bp, 1; 05791 test byte [si+0x26], 2; 05795 jne m1$
        if dx < 0 {
            1
        } else {
            -1
        }
    }
}

/// `ControlTrogg` (0x2ddf) and everything it jumps to: `TroggStart` (0x2e03),
/// `TroggMove` (0x2e22), `TroggAttack` (0x2e64), `TroggAttacks` (0x2ea7),
/// `TroggSwing` (0x2eed), `TroggChop` (0x2eff). Translated block for block,
/// with the same comparisons in the same order; every block quotes the
/// address it came from.
///
/// The two entry branches this does not take are handled where their
/// condition is raised. `TroggStruck` (0x2f19, on `+0xe`) picks the
/// blow-taken script and zeroes `+0x4a`; `TroggHit` (0x2f4d, on `+0xc`) plays
/// the recovery and sets `+0x4a` to ten. Both happen in the bout the moment a
/// blow lands, through [`trogg_struck`] and [`trogg_hit`], because that is
/// when the original sets `+0xe` and `+0xc`. Neither calls `FaceKnight`, so
/// neither turns the creature.
///
/// `DS:0x783a` is answered with an [`Act`]: `Act::Idle` is the stance
/// `ControlTrogg` writes there first (`mov ax, [di+0x10]; mov [0x783a], ax`
/// at 0x2de5), which every `jmp 0x2d52` that writes nothing else hands back.
///
/// ```text
/// ControlTrogg:
/// 02ddf  mov [0x77e8], si          ; me
/// 02de5  mov ax, [di+0x10]         ; the stance...
/// 02de8  mov [0x783a], ax          ; ...is the answer unless something else is
/// 02deb  mov ax, [KnightTable]
/// 02dee  mov [Opponent], ax        ; the opponent is the knight
/// 02df1  cmp word [si+0xe], 0
/// 02df5  je  02dfa
/// 02df7  jmp TroggStruck
/// 02dfa  cmp word [si+0xc], 0
/// 02dfe  je  TroggStart
/// 02e00  jmp TroggHit
/// TroggStart:
/// 02e03  mov word [si+0x26], 0     ; no walk bits
/// 02e08  mov word [si+0x28], 0     ; no attack kind
/// 02e0d  call MonsterTrack         ; FaceKnight is its first act
/// 02e10  or  ax, ax
/// 02e12  je  TroggAttack           ; in range on the plane: bx is the distance
/// 02e14  cmp word [ZPLANE], 0
/// 02e19  je  TroggMove             ; off the plane: walk
/// 02e1b  call FindDistance         ; on the plane, out of range
/// 02e1e  mov bx, ax
/// 02e20  jmp TroggAttack
/// TroggMove:
/// 02e22  cmp byte [si+0x26], 0
/// 02e26  jne 02e2b
/// 02e28  jmp 02d52                 ; no bits: the stance
/// 02e2b  ...                       ; MoveU / MoveD / MoveR / MoveL, then MonsterWalk
/// TroggAttack:
/// 02e64  mov si, [0x77e8]
/// 02e68  cmp bx, [si+0x54]
/// 02e6b  jle TroggMove             ; inside the back-off: give ground
/// 02e6d  mov di, [si+0x16]         ; the *Att table
/// 02e71  mov bx, [Opponent]
/// 02e75  cmp word [bx+0x38], 0
/// 02e7a  jg  TroggAttacks          ; he is alive
/// 02e7c  cmp word [0x700], 0
/// 02e81  je  02e86
/// 02e83  jmp 02d52                 ; gore off: nothing
/// 02e86  cmp bx, 0x64
/// 02e89  jg  TroggMove             ; the body is past a hundred
/// 02e8b  cmp word [DeCapFLAG], 0
/// 02e90  jne 02e9c                 ; someone already has
/// 02e92  cmp byte [si+0x4a], 0
/// 02e96  je  02e9f
/// 02e98  sub byte [si+0x4a], 1
/// 02e9c  jmp 02d52
/// 02e9f  mov word [DeCapFLAG], 1
/// 02ea5  jmp TroggSwing
/// TroggAttacks:
/// 02ea7  cmp byte [si+0x4a], 0
/// 02eab  je  02eb4
/// 02ead  sub byte [si+0x4a], 1
/// 02eb1  jmp 02d52                 ; counting down: the stance, whatever it reached
/// 02eb4  cmp byte [si+0x35], 0x10
/// 02eb8  jne 02ed4                 ; not the spear
/// 02eba  cmp bx, [si+0x52]
/// 02ebd  jle 02ec2
/// 02ebf  jmp TroggMove             ; the spear walks unless inside +0x52
/// 02ec2  mov byte [si+0x4a], 0x14
/// 02ec6  mov word [si+0x28], 2
/// 02ecb  mov ax, TroggSpear_Lunge
/// 02ece  mov [0x783a], ax
/// 02ed1  jmp 02d52
/// 02ed4  cmp bx, 0x64
/// 02ed7  jg  TroggChop             ; past a hundred: the overhead
/// 02ed9  call GETPERCENT
/// 02edc  cmp ax, 0x1e
/// 02edf  jg  TroggSwing            ; over thirty: the swing
/// 02ee1  mov bx, [KnightTable]
/// 02ee6  cmp word [bx+0x28], 8
/// 02eeb  je  TroggChop             ; he is holding the block: the overhead
/// TroggSwing:
/// 02eed  mov byte [si+0x4a], 0xa
/// 02ef1  mov word [si+0x28], 4
/// 02ef6  mov ax, [di+4]            ; Att[4]
/// 02ef9  mov [0x783a], ax
/// 02efc  jmp 02d52
/// TroggChop:
/// 02eff  cmp bx, 0x78
/// 02f02  jle 02f07
/// 02f04  jmp TroggMove             ; past a hundred and twenty: walk
/// 02f07  mov byte [si+0x4a], 0xa
/// 02f0b  mov word [si+0x28], 0x10
/// 02f10  mov ax, [di+0x10]         ; Att[0x10]
/// 02f13  mov [0x783a], ax
/// 02f16  jmp 02d52
/// ```
///
/// And the tail every branch ends on, which is how the facing reaches the
/// task: `dh` is `+8` as `FaceKnight` left it, and `TASKHANDLE` stores it
/// into `task+0x14` with the new script (0x9741).
///
/// ```text
/// 02d52  mov di, [0x77e8]
/// 02d56  mov si, [0x783a]          ; the script
/// 02d5a  mov ax, [di+2]            ; x
/// 02d5d  mov bx, [di+4]            ; y
/// 02d60  mov cx, [di+6]            ; z
/// 02d63  mov dh, [di+8]            ; facing
/// 02d66  ret
/// ```
fn trogg(s: &Sight, brain: &mut Brain, seed: &mut u16, spear: bool, facing: &mut i32) -> Act {
    // TroggStart:
    // 02e03  mov word [si+0x26], 0
    // 02e08  mov word [si+0x28], 0
    // 02e0d  call MonsterTrack
    let t = track(s.me, s.foe, s.def, facing);
    let bx = if !t.walking {
        // 02e10  or ax, ax; 02e12 je TroggAttack: bx as CheckXAxis left it.
        t.distance
    } else if !t.plane {
        // 02e14  cmp word [ZPLANE], 0; 02e19 je TroggMove
        return trogg_move(t);
    } else {
        // 02e1b  call FindDistance; 02e1e mov bx, ax; 02e20 jmp TroggAttack
        find_distance(s.me, s.foe)
    };
    // TroggAttack:
    // 02e68  cmp bx, [si+0x54]; 02e6b jle TroggMove
    if bx <= s.def.back_off {
        return trogg_move(t);
    }
    // 02e6d  mov di, [si+0x16]     (the *Att table: Act::Attack indexes it)
    // 02e75  cmp word [bx+0x38], 0; 02e7a jg TroggAttacks
    if s.foe.health > 0 {
        return trogg_attacks(s, brain, seed, spear, bx, t);
    }
    // 02e7c  cmp word [0x700], 0; 02e81 je 02e86; 02e83 jmp 02d52
    if !s.gore {
        return Act::Idle;
    }
    // 02e86  cmp bx, 0x64; 02e89 jg TroggMove
    if bx > 0x64 {
        return trogg_move(t);
    }
    // 02e8b  cmp word [DeCapFLAG], 0; 02e90 jne 02e9c
    if s.decapped {
        return Act::Idle;
    }
    // 02e92  cmp byte [si+0x4a], 0; 02e96 je 02e9f
    if brain.cooldown != 0 {
        // 02e98  sub byte [si+0x4a], 1; 02e9c jmp 02d52
        brain.cooldown -= 1;
        return Act::Idle;
    }
    // 02e9f  mov word [DeCapFLAG], 1: the bout raises its flag on seeing this
    // attack ordered against a knight with no hit points (`Bout::decap`).
    // 02ea5  jmp TroggSwing
    trogg_swing(brain)
}

/// `TroggAttacks`, 0x2ea7. `bx` is the distance, `t` what `MonsterTrack` set
/// in `+0x26`, for the `TroggMove` exits.
fn trogg_attacks(
    s: &Sight,
    brain: &mut Brain,
    seed: &mut u16,
    spear: bool,
    bx: i32,
    t: Track,
) -> Act {
    // 02ea7  cmp byte [si+0x4a], 0; 02eab je 02eb4
    if brain.cooldown != 0 {
        // 02ead  sub byte [si+0x4a], 1; 02eb1 jmp 02d52
        brain.cooldown -= 1;
        return Act::Idle;
    }
    // 02eb4  cmp byte [si+0x35], 0x10; 02eb8 jne 02ed4
    if spear {
        // 02eba  cmp bx, [si+0x52]; 02ebd jle 02ec2; 02ebf jmp TroggMove
        if bx > s.def.approach {
            return trogg_move(t);
        }
        // 02ec2  mov byte [si+0x4a], 0x14
        brain.cooldown = 0x14;
        // 02ec6  mov word [si+0x28], 2; 02ecb mov ax, TroggSpear_Lunge; 02ed1 jmp 02d52
        return Act::Attack {
            kind: Attack::Lunge,
            spawn: None,
        };
    }
    // 02ed4  cmp bx, 0x64; 02ed7 jg TroggChop
    if bx > 0x64 {
        return trogg_chop(brain, bx, t);
    }
    // 02ed9  call GETPERCENT; 02edc cmp ax, 0x1e; 02edf jg TroggSwing
    let ax = percent(seed);
    if ax > 0x1e {
        return trogg_swing(brain);
    }
    // 02ee2  mov bx, [KnightTable]; 02ee6 cmp word [bx+0x28], 8; 02eeb je TroggChop
    if s.foe.attack == Some(Attack::Block) {
        return trogg_chop(brain, bx, t);
    }
    trogg_swing(brain)
}

/// `TroggSwing`, 0x2eed.
fn trogg_swing(brain: &mut Brain) -> Act {
    // 02eed  mov byte [si+0x4a], 0xa
    brain.cooldown = 0xa;
    // 02ef1  mov word [si+0x28], 4; 02ef6 mov ax, [di+4]; 02ef9 mov [0x783a], ax
    Act::Attack {
        kind: Attack::Swing,
        spawn: None,
    }
}

/// `TroggChop`, 0x2eff.
fn trogg_chop(brain: &mut Brain, bx: i32, t: Track) -> Act {
    // 02eff  cmp bx, 0x78; 02f02 jle 02f07; 02f04 jmp TroggMove
    if bx > 0x78 {
        return trogg_move(t);
    }
    // 02f07  mov byte [si+0x4a], 0xa
    brain.cooldown = 0xa;
    // 02f0b  mov word [si+0x28], 0x10; 02f10 mov ax, [di+0x10]; 02f13 mov [0x783a], ax
    Act::Attack {
        kind: Attack::Chop,
        spawn: None,
    }
}

/// `TroggMove`, 0x2e22: `cmp byte [si+0x26], 0; jne 02e2b; jmp 02d52`. No
/// walk bit means the stance; otherwise `MoveU`, `MoveD`, `MoveR`, `MoveL`
/// and `MonsterWalk`, which is what an `Act::Walk` carrying those bits is.
fn trogg_move(t: Track) -> Act {
    walk(t)
}

/// `TroggStruck`, 0x2f19, the `+0xe` branch of `ControlTrogg`: the part of
/// it that touches the controller's own state. The blow-taken script and the
/// damage are the bout's business; this is the one line beside them.
///
/// ```text
/// 02f19  mov si, [di+0xe]          ; who struck it
/// 02f1c  mov byte [di+0x4a], 0     ; the cooldown is forgotten
/// ```
///
/// No `FaceKnight` here: a struck trogg keeps the facing it had.
pub fn trogg_struck(brain: &mut Brain) {
    brain.cooldown = 0;
}

/// `TroggHit`, 0x2f4d, the `+0xc` branch of `ControlTrogg`: the recovery
/// script (`+0x12`) and ten frames before the next blow.
///
/// ```text
/// 02f4e  mov ax, [di+0x12]
/// 02f51  mov [0x783a], ax          ; the recovery
/// 02f55  mov byte [di+0x4a], 0xa
/// ```
///
/// What follows (0x2f59 on) only matters for the spear's toss of a corpse.
/// No `FaceKnight` here either.
pub fn trogg_hit(brain: &mut Brain) {
    brain.cooldown = 0xa;
}

/// `KnightGotStruck`, image 0x4267: what a landed blow does to the facing of
/// whoever took it.
///
/// ```text
/// 04267  mov si, [di+0xe]          ; di took the blow, si landed it
/// 0426a  xor ax, ax
/// 0426c  mov al, [si+0x35]         ; the striker's kind
/// 0426f  mov bx, 0x7843
/// 04272  add bx, ax
/// 04274  jmp [bx]                  ; into the striker's own *Struck1
/// ```
///
/// Only three of the table's entries touch `+8`, and one of the three cannot
/// be reached:
///
/// ```text
/// DemonStruck1:
/// 04360  cmp word [si+0x28], 0x10
/// 04364  jne 0436c
/// 04366  sub word [di+0x38], 0xa
/// 0436a  jmp DemonSlap
/// 0436c  cmp word [si+0x28], 2
/// 04370  jne 04378
/// 04372  sub word [di+0x38], 8
/// 04376  jmp DemonSlap
/// 04378  sub word [di+0x38], 0xa
/// 0437c  jmp KnightSAnim           ; any other kind: the facing is left alone
/// DemonSlap:
/// 0437f  mov al, [si+8]
/// 04382  xor al, 2
/// 04384  mov [di+8], al            ; turned to face the demon
///
/// ClawStruck1:
/// 043d3  mov ax, 0xa
/// 043d6  call TalismanWrym
/// 043d9  sub word [di+0x38], ax
/// 043dc  mov byte [0x7832], 1      ; the slap goes rightward
/// 043e7  mov word [0x783a], 0x1596 ; Knight_SwSlapped
/// 043ed  mov byte [di+8], 3        ; and he is left facing left
///
/// BalokStruck1:
/// 04276  mov word [si+0x28], 4
/// 0427b  jne 04285                 ; ZF is still `add bx, ax`'s, and
/// 0427d  mov al, [si+8]            ; 0x7843 + kind is never zero, so the
/// 04280  xor al, 2                 ; jump is always taken and these three
/// 04282  mov [di+8], al            ; instructions are dead code
/// 04285  sub word [di+0x38], 5
/// ```
///
/// So a demon's slap and a dragon's claw turn whoever they land on, a balok's
/// slap does not, and no other striker writes `+8` from here. Answers the new
/// `+8` for the fighter that took the blow, 1 right and -1 left, or `None`
/// when this striker's entry leaves it as it was.
///
/// `kind` is the striker's `+0x28`, and `striker_facing` its `+8`.
pub fn struck_facing(
    striker: Controller,
    kind: Option<Attack>,
    striker_facing: i32,
) -> Option<i32> {
    match striker {
        // 04360..04384: kinds 0x10 and 2 fall into `DemonSlap`, nothing else
        // does. `xor al, 2` on 1 gives 3 and on 3 gives 1.
        Controller::Demon => match kind {
            Some(Attack::Chop) | Some(Attack::Lunge) => {
                Some(if striker_facing < 0 { 1 } else { -1 })
            }
            _ => None,
        },
        // 043ed  mov byte ptr [di + 8], 3
        Controller::Claw => Some(-1),
        _ => None,
    }
}

/// `RatmanHit`, image 0x34f0: the `+0xc` branch of `ControlRatmen`, which is
/// the one place in the game a creature turns something it has hit.
///
/// ```text
/// 034f0  mov si, [di+0xc]          ; what it hit
/// 034f3  cmp byte [si+0x35], 0x12
/// 034f7  jne 034fc
/// 034f9  jmp ControlRatCollide     ; another ratman: nothing happens
/// 034fc  test byte [di+0x48], 1
/// 03500  jne RatLeapHit            ; the leap has its own branch
/// 03502  test byte [di+0x48], 8
/// 03506  jne RatTailHit            ; so has the tail from a tree
/// 03508  mov word [0x783a], 0xffff
/// 0350e  mov word [di+0x48], 0
/// 03513  mov word [HitDelay], 0xf
/// 03519  mov al, [si+8]
/// 0351c  cmp al, [di+8]
/// 0351f  jne 03524
/// 03521  call FlipKnight           ; both facing the same way: turn him round
/// ```
///
/// and `FlipKnight`, image 0x3d13, which turns the knight's task rather than
/// the record and lets `perdone` carry it back:
///
/// ```text
/// 03d1a  mov ax, [KnightTable]
/// 03d1d  call FINDTASK
/// 03d20  mov si, ax
/// 03d24  je  03d34                 ; no task: nothing
/// 03d26  xor byte [si+0x14], 2     ; the mirror bit
/// 03d2e  mov al, [si+0x14]
/// 03d31  mov byte [di+8], al       ; and the record follows it
/// ```
///
/// `same_kind` is the `0x12` test and `busy` the two flag branches, which are
/// the leap and the tail. Answers whether whoever was hit is flipped.
pub fn ratman_flips(same_kind: bool, busy: bool, victim_facing: i32, ratman_facing: i32) -> bool {
    // 034f3..034f9, then 034fc..03506.
    if same_kind || busy {
        return false;
    }
    // 03519  mov al, [si+8]; 0351c cmp al, [di+8]; 0351f jne
    victim_facing.signum() == ratman_facing.signum()
}

/// `ControlTroll` and `TrollAttack`: the club inside a hundred, the overhead
/// chop from further out, and never two chops running.
///
/// ```text
/// TrollAttack:
/// 05678  call FindDistance
/// 0567b  mov si, [0x77e8]
/// 0567f  cmp ax, 0x64; jl TrollBunt
/// 05684  cmp ax, 0x96; jl TrollChop
/// TrollBunt:
/// 05689  mov word [si+0x28], 4
/// 0568e  mov word [0x783a], Troll_Bunt
/// 05694  mov al, [si+8]
/// 05697  mov [SLAP], al                ; the bunt throws him the troll's way
/// 0569a  mov word [SLAPY], BalokSLAP
/// TrollChop:
/// 056a3  cmp word [si+0x28], 0x10; je TrollBunt
/// 056a9  mov word [si+0x28], 0x10
/// 056ae  mov word [0x783a], Troll_Chop
/// ```
///
/// The bunt is the second striker in the game to write `SLAP` off its own
/// `+8`, and `InitKnightvsTroll+16` (0x26ba) is what makes the write count:
/// `Knight_SwSlapped` on the knight's kind-4 row, so the club throws him the
/// way a claw or an uppercut does. See `Bout::troll_struck_knight`.
fn troll(s: &Sight, brain: &mut Brain, facing: &mut i32, shared: &mut Shared) -> Act {
    // ControlTroll+53 (0x55f9): `call MonsterTrack`.
    let t = track(s.me, s.foe, s.def, facing);
    if t.walking {
        return walk(t);
    }
    let d = t.distance;
    if (100..150).contains(&d) && brain.phase != 1 {
        brain.phase = 1;
        return Act::Attack {
            kind: Attack::Chop,
            spawn: None,
        };
    }
    brain.phase = 0;
    // 05694  mov al, [si+8]; 05697 mov [SLAP], al; 0569a mov [SLAPY], BalokSLAP.
    // `+8` is what `MonsterTrack` left a few instructions ago, which is
    // `*facing` here.
    shared.slap = if *facing < 0 { 3 } else { 1 };
    shared.slap_y = slap_y::BALOK_SLAP;
    Act::Attack {
        kind: Attack::Swing,
        spawn: None,
    }
}

/// The first script of a named row, or nothing where a pack has not got one.
fn row(def: &ActorDef, name: &str) -> String {
    def.scripts_for(name).first().cloned().unwrap_or_default()
}

/// `NextWalk` (0x4ef7) and `AnimWalk` (0x4f1a): step the walk frame and name
/// the script that row holds at it.
///
/// ```text
/// NextWalk:
/// 04ef7  mov ax, bp                ; 1 forwards, -1 back
/// 04ef9  add byte [si+0xa], al
/// 04efc  and byte [si+0xa], 7
/// 04f02  mov di, [si+0x1c]         ; the walk table
/// 04f05  mov al, [si+0xa]; shl ax, 1; add ax, dx   ; dx is the row
/// 04f0f  cmp word [di], 0; je NextWalk             ; skip the empty tail
/// AnimWalk:
/// 04f1a  mov al, [si+0xa] ... mov [0x783a], ax
/// ```
///
/// The tail of a row is zeros, and the loop steps over them, so a four entry
/// row cycles four ways round. That is what this is.
fn next_walk(brain: &mut Brain, names: &[String]) -> String {
    if names.is_empty() {
        return String::new();
    }
    brain.walk = (brain.walk + 1) % names.len() as u32;
    names[brain.walk as usize].clone()
}

/// `ControlRatmen` (0x30d8) and everything under it, translated block for
/// block: `ControlRatCollide` (0x30fc), `RatmanLeap` (0x31a9),
/// `RatmanInitLeap` (0x3215), `RatNormalLeap` (0x325c), `RatmanLeaps`
/// (0x325f), `RatmanLeaping` (0x3270), `RatWithinTree` (0x32af),
/// `RatmanInTree` (0x32b8), `RatLeapOutTree` (0x32e0), `RatHangKnight`
/// (0x32ed), `RatmanOnHead` (0x3353), `RatmanGouge` (0x3383), `RatmanGouged`
/// (0x3395) and `RatmanReleaseKnight` (0x343d).
///
/// The eight branches `ControlRatCollide` opens with are the whole of it, and
/// they are taken in this order:
///
/// ```text
/// 030fc  mov word [si+0x28], 0          ; no attack kind unless one is chosen
/// 03105  test byte [di+0x48], 0x80; jne RatmanLeaping   ; a short hop
/// 0310e  test byte [di+0x48], 1;    jne RatmanLeaping   ; a leap
/// 03117  test byte [di+0x48], 0x20; jne RatmanOnHead
/// 03120  test byte [di+0x49], 1;    jne RatmanReleaseKnight
/// 03129  test byte [di+0x48], 0x10; jne RatmanGouged
/// 03132  test byte [di+0x48], 8;    jne RatmanInTree
/// 0313b  test byte [di+0x49], 4;    jne RatHangKnight
/// 03144  call FaceKnight
/// 03147  call CheckZAxis; or ax, ax; je RatmanLeap
/// 0314e  test word [RatFLAGS], 0x20; jne exit    ; one is on his head
/// 03159  test word [RatFLAGS], 8;    jne exit    ; one is hanging off him
/// 03164  cmp word [knight+0x38], 0; jle exit
/// 03171  cmp word [HitDelay], 0; jne (sub 1; exit)
/// 03180  call FindDistance
/// 03183  cmp ax, 0x28; jle (kind 4, Ratman_Slash)
/// 03196  cmp ax, 0x32; jle (kind 2, Ratman_Bite)
/// 031a9  RatmanLeap
/// ```
///
/// The two joysticks the original reads (`RatHangKnight+0`, `RatmanOnHead+0`)
/// are fire and down together, which is the same press `MudmenEntangle` reads
/// and which [`crate::combat::Fighter::step_gated`] already answers by
/// dropping the hold. So both branches ask whether the one held is still held
/// rather than reading a button of their own.
fn ratman(s: &Sight, brain: &mut Brain, facing: &mut i32, shared: &mut Shared) -> Act {
    // 03105 / 0310e: a hop and a leap both land in RatmanLeaping.
    if brain.flags & (flag::SHORT_HOP | flag::LEAPING) != 0 {
        return ratman_leaping(s, brain, shared);
    }
    // 03117  test byte ptr [di + 0x48], 0x20
    if brain.flags & flag::ON_HEAD != 0 {
        return ratman_on_head(s, brain);
    }
    // 03120  test byte ptr [di + 0x49], 1
    if brain.flags & flag::RELEASING != 0 {
        // RatmanReleaseKnight, 0x343d: the knight comes back on the board on
        // his own `+0x12`, and `RatFLAGS`' head bit is put down.
        //   0343d  mov ax, [KnightTable]; call TASKSTANDBY
        //   03443  mov word [di+0xc], 0; mov word [di+0xe], 0
        //   03451  mov si, [di+0x12]; call REPLACEANIM
        //   03457  and word [RatFLAGS], 0xffdf
        //   0345c  mov word [0x783a], 0
        brain.flags &= !flag::RELEASING;
        shared.rat &= !rat_flag::ON_HEAD;
        return Act::Grip {
            script: String::new(),
            damage: 0,
            cost: 0,
            fatal: false,
            hold: false,
            victim: String::new(),
        };
    }
    // 03129  test byte ptr [di + 0x48], 0x10
    if brain.flags & flag::GOUGING != 0 {
        return ratman_gouged(s, brain, shared);
    }
    // 03132  test byte ptr [di + 0x48], 8
    if brain.flags & flag::IN_TREE != 0 {
        return ratman_in_tree(s, brain, shared);
    }
    // 0313b  test byte ptr [di + 0x49], 4
    if brain.flags & flag::HANGING != 0 {
        return ratman_hang_knight(s, brain, shared);
    }
    // 03144  call FaceKnight
    *facing = face_knight(s.me, s.foe);
    // 03147  call CheckZAxis; 0314c je RatmanLeap
    if !check_z(s.me, s.foe, s.def) {
        return ratman_leap(s, brain, shared);
    }
    // 0314e / 03159: one rat at a time is on him, and one at a time hangs
    // off him; the rest stand and watch.
    if shared.rat & (rat_flag::ON_HEAD | rat_flag::HANGING) != 0 {
        return Act::Idle;
    }
    // 03168  cmp word ptr [si + 0x38], 0; 0316c jg; else the exit
    if !s.foe.alive() {
        return Act::Idle;
    }
    // 03171  cmp [HitDelay], 0; 03178 sub [HitDelay], 1; then the exit.
    // The delay is one word for the whole fight, not one per rat.
    if shared.hit_delay != 0 {
        shared.hit_delay -= 1;
        return Act::Idle;
    }
    // 03180  call FindDistance
    let d = find_distance(s.me, s.foe);
    // 03183  cmp ax, 0x28; 03186 jg
    if d <= 40 {
        return Act::Attack {
            kind: Attack::Swing,
            spawn: None,
        };
    }
    // 03196  cmp ax, 0x32; 03199 jg RatmanLeap
    if d <= 50 {
        return Act::Attack {
            kind: Attack::Lunge,
            spawn: None,
        };
    }
    ratman_leap(s, brain, shared)
}

/// `RatmanLeap`, image 0x31a9: the first rat to want to leap goes for the
/// tree, and every one after it goes for the knight.
///
/// ```text
/// 031ad  mov word [si+0x28], 0
/// 031b2  mov byte [si+0xa], 0                ; the walk frame starts again
/// 031b6  test word [RatFLAGS], 4
/// 031bc  jne RatmanInitLeap                  ; the tree is taken
/// 031be  or  word [RatFLAGS], 4
/// 031c3  or  byte [si+0x48], 4               ; this one is tree bound
/// 031c7  mov ax, [TreeHANDLE]; call FINDTASK
/// 031d3  mov word [di+0x4a], 0x1e            ; thirty frames up there
/// 031d9  mov bx, 0x77cc
/// 031dc  mov [bx], di
/// 031de  mov ax, [di+2];  mov [bx+4], ax     ; x0
/// 031e4  mov ax, [di+6];  mov [bx+6], ax     ; z0
/// 031ea  mov word [bx+8], 0xffec             ; y0, twenty above the ground
/// 031ef  mov ax, [si+4];  mov [bx+0xa], ax   ; x1, the tree task's x
/// 031f5  mov ax, [si+8];  mov [bx+0xc], ax   ; z1, its z
/// 031fb  mov ax, [si+6];  mov [bx+0xe], ax   ; y1, its y
/// 03201  add word [bx+0xc], 3
/// 03205  mov word [bx+0x10], 0xe             ; fourteen frames
/// 0320a  mov word [bx+0x12], 0
/// 03210  call ADDJUMP
/// 03213  jmp RatmanLeaps
/// ```
///
/// `y0` is twenty above the ground and not the rat's own height, which is the
/// instruction as written. With the tree at `HalfSCAPE - 200` the difference
/// is far more than five, so this is the one jump in the game that takes
/// `ADDJUMP`'s `JUMPDOWN` branch and starts at rest.
fn ratman_leap(s: &Sight, brain: &mut Brain, shared: &mut Shared) -> Act {
    // 031b2  mov byte ptr [si + 0xa], 0
    brain.walk = 0;
    // 031b6  test word ptr [0x779c], 4
    let perch = s.perch.filter(|_| shared.rat & rat_flag::TREE == 0);
    let Some(tree) = perch else {
        return ratman_init_leap(s, brain);
    };
    shared.rat |= rat_flag::TREE;
    brain.flags |= flag::TREE_BOUND;
    // 031d3  mov word ptr [di + 0x4a], 0x1e
    brain.cooldown = 0x1e;
    let plan = crate::jump::Plan {
        x0: s.me.x,
        z0: s.me.y,
        y0: -20,
        x1: tree.x,
        z1: tree.y + 3,
        y1: tree.height,
        steps: 0xe,
        rise: 0,
    };
    brain.jump = Some(plan.start());
    ratman_leaps(s, brain)
}

/// `RatmanInitLeap`, image 0x3215: aim at the knight, and hop rather than
/// leap when he is close.
///
/// ```text
/// 03215  mov bp, [si+0x52]              ; the approach range
/// 03218  call CalcJUMP
/// 0321b  mov ax, [0x7796]; shr ax, 1    ; half the frame count
/// 03220  cmp ax, 8; jge; mov ax, 8      ; and never fewer than eight
/// 03230  mov [0x77cc+0x10], ax
/// 03234  cmp word [0x76b0], 0x28
/// 03239  jg  RatNormalLeap              ; forty or more apart: a leap
/// 0323b  or  byte [si+0x48], 0x80       ; closer: a short hop
/// 03243  mov word [0x77cc+0x12], 2      ; two pixels of rise
/// 03249  call ADDJUMP
/// 0324c  mov bp, 1; mov dx, 0x10        ; and it is drawn on the up row now
/// 03256  call NextWalk; jmp AnimWalk
/// RatNormalLeap:
/// 0325c  call ADDJUMP                   ; and fall into RatmanLeaps
/// ```
fn ratman_init_leap(s: &Sight, brain: &mut Brain) -> Act {
    let aim = crate::jump::calc(
        (s.me.x, s.me.y, brain.height),
        (s.foe.x, s.foe.y, s.foe.brain.height),
        s.def.approach,
    );
    let mut plan = aim.plan;
    // 0321b: half the frames, floored at eight.
    let mut steps = aim.steps >> 1;
    if steps < 8 {
        steps = 8;
    }
    plan.steps = steps;
    // 03234  cmp word ptr [0x76b0], 0x28
    if aim.reach > 0x28 {
        // RatNormalLeap into RatmanLeaps.
        brain.jump = Some(plan.start());
        return ratman_leaps(s, brain);
    }
    brain.flags |= flag::SHORT_HOP;
    plan.rise = 2;
    brain.jump = Some(plan.start());
    // 0324c: the hop is drawn on the up row from its first frame.
    Act::Play(next_walk(brain, s.def.scripts_for("fly")))
}

/// `RatmanLeaps`, image 0x325f: `+0x48 |= 1`, and the frame it leaves the
/// ground on.
///
/// ```text
/// 03263  or  byte ptr [si + 0x48], 1
/// 03267  mov word ptr [0x783a], Ratman_Leap
/// ```
fn ratman_leaps(s: &Sight, brain: &mut Brain) -> Act {
    brain.flags |= flag::LEAPING;
    Act::Play(row(s.def, "leap"))
}

/// `RatmanLeaping`, image 0x3270, with `RatWithinTree` (0x32af) on the end.
///
/// ```text
/// 03270  mov ax, [0x77e8]; call ControlJump
/// 03276  mov [si+2], bx; mov [si+6], cx; mov [si+4], dx
/// 03283  or  ax, ax; je 0x329a                 ; still in the air
/// 03287  test byte [si+0x48], 4
/// 0328b  jne RatWithinTree
/// 0328d  mov word [si+4], 0                    ; down, and on the ground
/// 03292  mov word [si+0x48], 0
/// 03297  jmp exit                              ; the stance
/// 0329a  mov bp, 1; mov dx, 0x10
/// 032a0  cmp word [si+4], -0xa
/// 032a4  jle 0x32a9; mov dx, 0                 ; low enough for the walk row
/// 032a9  call NextWalk; jmp AnimWalk
/// RatWithinTree:
/// 032af  mov word [si+0x48], 0
/// 032b4  or  byte [si+0x48], 8                 ; and fall into RatmanInTree
/// ```
fn ratman_leaping(s: &Sight, brain: &mut Brain, shared: &mut Shared) -> Act {
    let Some(mut jump) = brain.jump else {
        // Nothing running: the only way out the original has is landing.
        brain.flags &= !(flag::LEAPING | flag::SHORT_HOP | flag::TREE_BOUND);
        brain.height = 0;
        return Act::Idle;
    };
    let step = jump.step();
    brain.jump = Some(jump);
    brain.height = step.y;
    if !step.done {
        // 0329a: the up row above ten off the ground, the walk row under it.
        let row = if step.y <= -10 { "fly" } else { "walk" };
        let script = next_walk(brain, s.def.scripts_for(row));
        return Act::Fly {
            x: step.x,
            y: step.z,
            script,
        };
    }
    brain.jump = None;
    if brain.flags & flag::TREE_BOUND != 0 {
        // RatWithinTree, 0x32af, which falls into RatmanInTree. The position
        // 0x3276 wrote is carried out with it: the arc's last frame is
        // written before 0x3283 asks whether the arc is over.
        brain.flags &= !(flag::LEAPING | flag::SHORT_HOP | flag::TREE_BOUND);
        brain.flags |= flag::IN_TREE;
        return match ratman_in_tree(s, brain, shared) {
            Act::Play(script) => Act::Fly {
                x: step.x,
                y: step.z,
                script,
            },
            other => other,
        };
    }
    // 0328d: back on the ground with every bit down.
    brain.flags &= !(flag::LEAPING | flag::SHORT_HOP | flag::TREE_BOUND);
    brain.height = 0;
    Act::Fly {
        x: step.x,
        y: step.z,
        script: String::new(),
    }
}

/// `RatmanInTree`, image 0x32b8, and `RatLeapOutTree` (0x32e0).
///
/// ```text
/// 032bc  sub word [si+0x4a], 1
/// 032c0  je  RatLeapOutTree
/// 032c2  mov di, [KnightTable]; call FindDistance
/// 032c9  cmp ax, 0x3c
/// 032cc  jl  0x32d7
/// 032ce  mov word [0x783a], Ratman_HoverR      ; sixty or more away
/// 032d7  mov word [0x783a], Ratman_HoverD      ; nearer than that
/// RatLeapOutTree:
/// 032e0  mov word [si+0x48], 0
/// 032e5  and word [RatFLAGS], 0xfffb           ; the tree is free again
/// 032ea  jmp RatmanInitLeap
/// ```
fn ratman_in_tree(s: &Sight, brain: &mut Brain, shared: &mut Shared) -> Act {
    brain.cooldown -= 1;
    if brain.cooldown == 0 {
        brain.flags &= !(flag::IN_TREE | flag::TREE_BOUND);
        shared.rat &= !rat_flag::TREE;
        brain.height = 0;
        return ratman_init_leap(s, brain);
    }
    let far = find_distance(s.me, s.foe) >= 0x3c;
    Act::Play(row(s.def, if far { "hover_far" } else { "hover_near" }))
}

/// `RatHangKnight`, image 0x32ed: it has him by the shoulders and takes a
/// point off him every frame until he shakes it loose.
///
/// ```text
/// 032ed  call JOY1
/// 032f0  test bx, 0x10; je 0x333a           ; fire...
/// 032f6  test bx, 4;    je 0x333a           ; ...and down together
/// 032fc  mov word [0x783a], Knight_HangSd
/// 03302  mov di, [0x77e8]; mov si, [KnightTable]
/// 0330a  call CalcDamage                    ; the knight's own blow...
/// 0330d  sub word [di+0x38], ax             ; ...comes off the rat
/// 03310  jg  exit
/// 03312  mov ax, [KnightTable]; call TASKSTANDBY
/// 03318  mov word [di+0xc], 0; mov word [di+0xe], 0
/// 03326  mov si, Knight_GetUp; call REPLACEANIM
/// 0332c  and word [RatFLAGS], 0xfff7
/// 03331  mov word [0x783a], Ratman_FallDown
/// 0333a  mov word [0x783a], Ratman_HangKnight
/// 03340  mov si, [KnightTable]; sub word [si+0x38], 1
/// 03348  jg  exit
/// 0334a  mov word [0x783a], Ratman_HungKnight
/// ```
fn ratman_hang_knight(s: &Sight, brain: &mut Brain, shared: &mut Shared) -> Act {
    if !s.foe.held() {
        // Fire and down together: the knight has swung at it.
        if s.foe_blow >= s.me.health {
            // The blow finished it: it lets go and falls.
            brain.flags &= !flag::HANGING;
            shared.rat &= !rat_flag::HANGING;
            return Act::Grip {
                script: row(s.def, "fall"),
                damage: 0,
                cost: s.foe_blow,
                fatal: false,
                hold: false,
                // 03326  mov si, Knight_GetUp; call REPLACEANIM
                victim: "Knight_GetUp".into(),
            };
        }
        return Act::Grip {
            script: row(s.def, "shake"),
            damage: 0,
            cost: s.foe_blow,
            fatal: false,
            hold: true,
            victim: String::new(),
        };
    }
    // 0333a: one point a frame, and the last one is the death.
    let last = s.foe.health <= 1;
    Act::Grip {
        script: row(s.def, if last { "hung" } else { "hang" }),
        damage: 1,
        cost: 0,
        fatal: false,
        hold: true,
        victim: String::new(),
    }
}

/// `RatmanOnHead`, image 0x3353, with `RatmanGouge` (0x3383) on the end.
///
/// ```text
/// 03353  call JOY1
/// 03356  test bx, 0x10; jne 0x336b          ; fire held?
/// 0335c  sub word [di+0x4a], 1
/// 03360  je  RatmanGouge
/// 03362  mov word [0x783a], Ratman_SitOnHead
/// 0336b  test bx, 4; je 0x335c              ; fire, but not down: sit on
/// 03371  mov word [0x783a], Ratman_KnightWhack
/// 03377  mov word [di+0x48], 0
/// 0337c  or  byte [di+0x49], 1              ; and let go next frame
/// RatmanGouge:
/// 03383  mov word [di+0x48], 0
/// 03388  or  byte [di+0x48], 0x10
/// 0338c  mov word [0x783a], Ratman_EyeGouge
/// ```
fn ratman_on_head(s: &Sight, brain: &mut Brain) -> Act {
    if !s.foe.held() {
        // 03371: he got a hand to it.
        brain.flags &= !flag::ON_HEAD;
        brain.flags |= flag::RELEASING;
        return Act::Grip {
            script: row(s.def, "whack"),
            damage: 0,
            cost: 0,
            fatal: false,
            hold: true,
            victim: String::new(),
        };
    }
    brain.cooldown -= 1;
    if brain.cooldown == 0 {
        // RatmanGouge.
        brain.flags &= !flag::ON_HEAD;
        brain.flags |= flag::GOUGING;
        return Act::Grip {
            script: row(s.def, "gouge"),
            damage: 0,
            cost: 0,
            fatal: false,
            hold: true,
            victim: String::new(),
        };
    }
    Act::Grip {
        script: row(s.def, "sit"),
        damage: 0,
        cost: 0,
        fatal: false,
        hold: true,
        victim: String::new(),
    }
}

/// `RatmanGouged`, image 0x3395: the eye gouge is over, so it throws itself a
/// hundred and fifty pixels clear and leaves the knight five points down.
///
/// ```text
/// 03395  and word [RatFLAGS], 0xffdf
/// 0339a  mov si, 0x77cc; mov di, [0x77e8]
/// 033a1  mov word [di+0x4a], 0x11
/// 033a6  mov [si], di
/// 033a8  mov ax, [di+2]; mov [si+4], ax        ; x0
/// 033ae  mov ax, [di+6]; mov [si+6], ax        ; z0
/// 033b4  mov ax, [di+4]; mov [si+8], ax; add word [si+8], -0x5a
/// 033be  mov ax, [di+2]; mov [si+0xa], ax; add word [si+0xa], 0x96
/// 033c9  mov ax, [di+6]; mov [si+0xc], ax
/// 033cf  mov ax, [di+4]; mov [si+0xe], ax
/// 033d5  mov word [si+0x10], 0x11              ; seventeen frames
/// 033da  mov word [si+0x12], 0x78
/// 033df  add si, 0x14                          ; dead: ADDJUMP finds its own
/// 033e2  call ADDJUMP
/// 033e9  mov word [di+0x48], 0
/// 033ee  or  byte [di+0x48], 1
/// 033f2  mov byte [di+0xa], 0
/// 033f6  mov word [di+4], 0xffba               ; seventy off the ground
/// 033fb  mov word [0x783a], Ratman_Leaps
/// 03404  mov ax, [KnightTable]; call FINDTASK  ; his task takes the rat's
/// 0340f  mov al, [rat+8]; mov [di+0x14], al    ; facing
/// 03417  mov di, [KnightTable]
/// 0341b  mov word [di+0xc], 0; mov word [di+0xe], 0
/// 03425  mov si, [di+0x10]                     ; his stance...
/// 03428  sub word [di+0x38], 5
/// 0342c  jg  0x3431
/// 0342e  mov si, Knight_SwDeath                ; ...or his death
/// 03431  call REPLACEANIM
/// 03434  mov ax, [KnightTable]; call TASKSTANDBY
/// ```
///
/// The jump starts ninety rows *above* where it means to end and ends where
/// the rat's own `+4` is, so `ADDJUMP` takes the upward branch; `+4` is then
/// written to seventy up anyway, which is what the first frame is drawn at
/// before `ControlJump` takes it over.
fn ratman_gouged(s: &Sight, brain: &mut Brain, shared: &mut Shared) -> Act {
    shared.rat &= !rat_flag::ON_HEAD;
    // 033a1  mov word ptr [di + 0x4a], 0x11
    brain.cooldown = 0x11;
    let plan = crate::jump::Plan {
        x0: s.me.x,
        z0: s.me.y,
        y0: brain.height - 0x5a,
        x1: s.me.x + 0x96,
        z1: s.me.y,
        y1: brain.height,
        steps: 0x11,
        rise: 0x78,
    };
    brain.jump = Some(plan.start());
    brain.flags &= !(flag::GOUGING | flag::ON_HEAD | flag::RELEASING);
    brain.flags |= flag::LEAPING;
    // 033f2  mov byte ptr [di + 0xa], 0
    brain.walk = 0;
    // 033f6  mov word ptr [di + 4], 0xffba
    brain.height = -0x46;
    Act::Grip {
        script: row(s.def, "leaps"),
        damage: 5,
        cost: 0,
        fatal: false,
        hold: false,
        victim: String::new(),
    }
}

/// `ControlMudmen`: it reaches for you between seventy five and a hundred,
/// and inside that it goes under the ground and comes up beside you.
fn mudman(s: &Sight, brain: &mut Brain, facing: &mut i32) -> Act {
    // Holding the knight: `MudmenEntangle` counts down, and the choke at the
    // end of it is `KillKnight`.
    if brain.flags & flag::ENTANGLING != 0 {
        brain.timer -= 1;
        if brain.timer <= 0 {
            brain.flags &= !flag::ENTANGLING;
            return Act::Strike {
                script: "Mudmen_ChokeKnight".into(),
                damage: 0,
                fatal: true,
                victim: None,
            };
        }
        if !s.foe.held() {
            // He tore free: `Mudmen_KnightSd`, and it costs the mudman a point.
            brain.flags &= !flag::ENTANGLING;
            return Act::Strike {
                script: "Mudmen_Hit".into(),
                damage: 0,
                fatal: false,
                victim: None,
            };
        }
        return Act::Play("Mudmen_EntangleKnight".into());
    }
    if brain.flags & flag::BURIED != 0 {
        brain.timer -= 1;
        if brain.timer > 0 {
            return Act::Idle;
        }
        brain.flags &= !flag::BURIED;
        // `MudmenAppear`: seventy five pixels to one side of him, and on the
        // side that keeps it on the screen.
        let side = if s.foe.x >= 160 { -1 } else { 1 };
        return Act::Appear {
            x: s.foe.x + 75 * side,
            facing: -side,
            script: "Mudmen_Appear".into(),
        };
    }
    // ControlMudmen+105 (0x5357): `call MonsterTrack`, after the entangle,
    // choke, surface and bury branches.
    let t = track(s.me, s.foe, s.def, facing);
    if t.walking && !t.plane {
        return walk(t);
    }
    let d = t.distance;
    if d <= 50 || d >= 100 {
        return walk(t);
    }
    if d < 75 {
        // `MudmenIBury`. The original checks the plane and the distance again
        // when it surfaces; here the timer is the whole of it.
        brain.flags |= flag::BURIED;
        brain.timer = 8;
        return Act::Play("Mudmen_IBury".into());
    }
    if !t.plane {
        return walk(t);
    }
    Act::Attack {
        kind: Attack::Swing,
        spawn: None,
    }
}

/// `ControlBalok` (0x3599) and everything under it: `BalokJump` (0x366f),
/// `BalokJumping` (0x36d1), `ControlBalokGrab` (0x37ae), `ControlBalokBite`
/// (0x37c1), `ControlBalokCrush` (0x37dc) and `ControlBalokRelease` (0x37f0).
///
/// It closes in hops, uppercuts at arm's length, grabs from further out, and
/// stands off between a hundred and twenty and a hundred and eighty unless you
/// are throwing daggers at it.
///
/// ```text
/// 035ad  test word [BalokFLAGS], 2;    jne BalokJumping
/// 035b8  cmp  word [si+0xe], 0;        jne BalokStruck
/// 035c1  cmp  word [si+0xc], 0;        jne BalokHit
/// 035ca  test word [BalokFLAGS], 1;    jne ControlBalokGrab
/// 035d5  test word [BalokFLAGS], 0x20; jne ControlBalokRelease
/// 035e0  cmp  word [di+0x38], 0; jle exit
/// 035f5  cmp  ax, [0x7790]; jl; mov byte [si+8], 3 / mov byte [si+8], 1
/// 03608  call CheckZAxis; je BalokJump
/// 0360f  mov  word [BalokFLAGS], 0
/// 03615  call FindDistance
/// 03618  cmp  ax, 0x46; jg;  mov byte [si+0x49], 0; jmp BalokJump
/// 03623  cmp  ax, 0x50; jg;  cmp word [si+0x28], 4; je 0x364d
///        mov [0x783a], Balok_UpperCut; mov [si+0x28], 4
///        mov al, [si+8]; mov [SLAP], al; mov [0x782e], BalokSLAP
/// 03648  cmp  ax, 0x78; jg;  mov [si+0x28], 0x10; mov [0x783a], Balok_Grab
/// 0365b  cmp  byte [knight+0x34], 0; jne BalokJump      ; he has daggers
/// 03667  cmp  ax, 0xb4; jg BalokJump; else exit
/// ```
fn balok(s: &Sight, brain: &mut Brain, facing: &mut i32, shared: &mut Shared) -> Act {
    // 035ad  test word ptr [0x7794], 2
    if shared.balok & balok_flag::JUMPING != 0 {
        return balok_jumping(s, brain, shared);
    }
    // `BalokGrabbed` (0x379b), which is `BalokHit`'s own branch: the grab
    // connected on the frame just gone, so this one is `Balok_GrabKnight`.
    if brain.flags & flag::GRABBED != 0 {
        brain.flags &= !flag::GRABBED;
        return Act::Grip {
            script: "Balok_GrabKnight".into(),
            damage: 0,
            cost: 0,
            fatal: false,
            hold: true,
            victim: String::new(),
        };
    }
    // 035ca  test word ptr [0x7794], 1
    if shared.balok & balok_flag::HELD != 0 {
        // ControlBalokGrab, 0x37ae: one shake, and the grab is over.
        shared.balok &= !balok_flag::HELD;
        shared.balok |= balok_flag::RELEASING;
        return Act::Grip {
            script: "Balok_ShakeKnight".into(),
            damage: 0,
            cost: 0,
            fatal: false,
            hold: true,
            victim: String::new(),
        };
    }
    // 035d5  test word ptr [0x7794], 0x20
    if shared.balok & balok_flag::RELEASING != 0 {
        return balok_release(s, shared);
    }
    // 035e0  cmp word ptr [di + 0x38], 0; 035e4 jg; else the exit
    if !s.foe.alive() {
        return Act::Idle;
    }
    // ControlBalok+80..107: its own `FaceKnight`, against `[0x7790]`.
    *facing = if s.me.x < s.foe.x { 1 } else { -1 };
    // 03608  call CheckZAxis; 0360d je BalokJump
    if !check_z(s.me, s.foe, s.def) {
        return balok_jump(s, brain, shared);
    }
    // 0360f  mov word ptr [0x7794], 0
    shared.balok = 0;
    let d = find_distance(s.me, s.foe);
    // 03618  cmp ax, 0x46
    if d <= 0x46 {
        // 0361d  mov byte ptr [si + 0x49], 0: the hop's own wait is dropped.
        brain.timer = 0;
        return balok_jump(s, brain, shared);
    }
    // 03623  cmp ax, 0x50; 03628 cmp word [si+0x28], 4; 0362c je 0x364d.
    // `+0x28` is the record's, which this engine clears the moment a fighter
    // leaves the attack state, so the controller keeps its own copy of it in
    // `Brain::att` exactly as `ControlBlackKnight` keeps `ATT`.
    if d <= 0x50 && brain.att != Some(Attack::Swing) {
        brain.att = Some(Attack::Swing);
        // 03639  mov al, [si+8]; 0363c mov [SLAP], al
        // 0363f  mov word [SLAPY], BalokSLAP
        //
        // The uppercut throws its victim the way it is itself facing rather
        // than always rightward as a claw does, and `+8` is what
        // ControlBalok+80..107 set a dozen instructions ago. `SLAPY` gets the
        // table's own first word, which is what a throw reads; see the
        // table's note for the whip, which is the one store that does not.
        shared.slap = if *facing < 0 { 3 } else { 1 };
        shared.slap_y = slap_y::BALOK_SLAP;
        return Act::Attack {
            kind: Attack::Swing,
            spawn: None,
        };
    }
    // 03648  cmp ax, 0x78, which `0x364d` is also where a repeated uppercut
    // lands: an uppercut is never played twice running, and the grab is what
    // comes instead.
    if d <= 0x78 {
        brain.att = Some(Attack::Chop);
        return Act::Attack {
            kind: Attack::Chop,
            spawn: None,
        };
    }
    // 0365b  cmp byte ptr [bx + 0x34], 0; 03667 cmp ax, 0xb4
    if s.foe.daggers() > 0 || d > 0xb4 {
        return balok_jump(s, brain, shared);
    }
    Act::Idle
}

/// `BalokJump`, image 0x366f.
///
/// ```text
/// 0366f  mov bp, 0x50; call CalcJUMP
/// 03675  cmp word [0x7796], 3; jle exit          ; too close to be worth it
/// 03683  cmp byte [si+0x49], 0; je 0x3692
/// 03689  sub byte [si+0x49], 1; jne exit         ; five frames between hops
/// 03692  mov word [si+0x28], 0
/// 03697  mov si, 0x77cc + 0x10
/// 0369d  mov ax, [0x7796]; cmp ax, 0x14; jle; mov ax, 0x14
/// 036a8  mov [si], ax                            ; at most twenty frames
/// 036ad  mov [0x779a], ax
/// 036b1  mov ax, [0x7798]; mov [si+2], ax        ; the rise
/// 036ba  call ADDJUMP
/// 036bd  mov word [BalokFLAGS], 0; or word [BalokFLAGS], 2
/// 036c8  mov word [0x783a], Balok_Jump
/// ```
fn balok_jump(s: &Sight, brain: &mut Brain, shared: &mut Shared) -> Act {
    let aim = crate::jump::calc(
        (s.me.x, s.me.y, brain.height),
        (s.foe.x, s.foe.y, s.foe.brain.height),
        0x50,
    );
    // 03675  cmp word ptr [0x7796], 3
    if aim.steps <= 3 {
        return Act::Idle;
    }
    // 03683: the wait between hops, `+0x49`.
    if brain.timer != 0 {
        brain.timer -= 1;
        if brain.timer != 0 {
            return Act::Idle;
        }
    }
    let mut plan = aim.plan;
    plan.steps = aim.steps.min(0x14);
    plan.rise = aim.rise;
    shared.jump_steps = aim.steps;
    // 036b1 reads it back off DS:0x7798 at the landing.
    shared.jump_height = aim.rise;
    shared.balok_hop = plan.steps;
    brain.jump = Some(plan.start());
    shared.balok = balok_flag::JUMPING;
    Act::Play("Balok_Jump".into())
}

/// `BalokJumping`, image 0x36d1: the hop, and the landing that kills whoever
/// is under it.
///
/// ```text
/// 036d1  mov byte [si+0x49], 0
/// 036d5  mov ax, [0x77e8]; call ControlJump
/// 036df  mov [si+2], bx; mov [si+6], cx; mov [si+4], dx
/// 036e8  sub word [0x779a], 1; je 0x372a          ; the hop is over
/// 036ef  mov word [0x783a], Balok_Jumping
/// 036f5  mov ax, [0x7796]; shr ax, 1
/// 036fa  cmp ax, [0x779a]; jl exit                ; still on the way up
/// 03700  cmp dx, -0x28; jl exit                   ; still forty off the ground
/// 03709  call FindDistance; cmp ax, 0xa; jg exit
/// 03711  mov ax, [di+2]; mov [si+2], ax           ; land on him exactly
/// 03717  mov ax, [di+6]; mov [si+6], ax
/// 0371d  mov si, Knight_Explode; call REPLACEANIM
/// 03723  call KillKnight                          ; and fall into the landing
/// 0372a  mov byte [si+0x49], 5                    ; five frames before another
/// 0372e  mov word [0x783a], Balok_Jump
/// 03734  mov word [si+4], 0
/// 03739  mov byte [BalokFLAGS], 0
/// 0373e  mov ax, 2                                ; dead: the calls clobber it
/// 03741  cmp word [0x7798], 8
/// 03746  jl  LittleLandAudio                      ; sound 0x2f
/// 03748  call BigLandAudio                        ; sound 0x2d, and the screen
/// 0374b  call ShakeADD                            ; shakes
/// ```
///
/// The landing is tested against the *hop's* own counter at DS:`0x779a` and
/// not against the jump slot's, and the two run together, so the second half
/// of the hop is where it can land on somebody.
fn balok_jumping(s: &Sight, brain: &mut Brain, shared: &mut Shared) -> Act {
    // 036d1  mov byte ptr [si + 0x49], 0
    brain.timer = 0;
    let Some(mut jump) = brain.jump else {
        shared.balok &= !balok_flag::JUMPING;
        brain.height = 0;
        return Act::Idle;
    };
    let step = jump.step();
    brain.jump = Some(jump);
    brain.height = step.y;
    shared.balok_hop -= 1;
    let landed = shared.balok_hop == 0;
    if !landed {
        // 036f5  mov ax, [0x7796]; shr ax, 1; cmp ax, [0x779a]; jl exit
        // 03700  cmp dx, -0x28; jl exit
        // 03709  call FindDistance; cmp ax, 0xa; jg exit
        let over = (shared.jump_steps >> 1) >= shared.balok_hop
            && step.y >= -0x28
            && (step.x - s.foe.x).abs() <= 0xa
            && s.foe.alive();
        if !over {
            return Act::Fly {
                x: step.x,
                y: step.z,
                script: "Balok_Jumping".into(),
            };
        }
        // 03711: it comes down on him, and that is the whole of him.
        brain.jump = None;
        brain.height = 0;
        brain.timer = 5;
        shared.balok &= !balok_flag::JUMPING;
        return Act::Grip {
            script: "Balok_Jump".into(),
            damage: 0,
            cost: 0,
            fatal: true,
            hold: false,
            // 0371d  mov si, Knight_Explode; call REPLACEANIM
            victim: "Knight_Explode".into(),
        };
    }
    // 0372a: the ordinary landing.
    //
    //   0372a  mov byte [si+0x49], 5          ; the wait before the next hop
    //   0372e  mov word [0x783a], Balok_Jump
    //   03734  mov word [si+4], 0             ; back on the ground
    //   03739  mov byte [BalokFLAGS], 0
    //   0373e  mov ax, 2
    //   03741  cmp word [JumpHIEGHT], 8
    //   03746  jl  03758                      ; a low hop lands quietly
    //   03748  call 03751                     ; sound 0x2d, the thud
    //   0374b  call ShakeADD
    //
    // **The shake is the landing's, and only from height.** A hop the
    // tracker sized under eight rows makes no sound and no shudder, which is
    // why the Balok's little shuffling jumps do not rattle the screen and the
    // one it comes down at you from does.
    let heavy = shared.jump_height >= 8;
    brain.jump = None;
    brain.height = 0;
    brain.timer = 5;
    shared.balok &= !balok_flag::JUMPING;
    if heavy {
        shared.shake = true;
    }
    Act::Fly {
        x: step.x,
        y: step.z,
        script: "Balok_Jump".into(),
    }
}

/// `ControlBalokRelease`, image 0x37f0, with `ControlBalokBite` (0x37c1) and
/// `ControlBalokCrush` (0x37dc).
///
/// ```text
/// 037f0  mov di, [KnightTable]
/// 037f4  cmp word [di+0x38], 0
/// 037f8  jle ControlBalokBite                ; already dead: it eats him
/// 037fa  mov word [di+0xc], 0; mov word [di+0xe], 0
/// 03804  call TASKSTANDBY                    ; back on the board
/// 0380e  mov si, [di+0x10]; call REPLACEANIM ; on his own stance
/// 03817  call FINDTASK
/// 03820  mov ax, [si+2]; mov bx, 0x4b
/// 03826  test byte [si+8], 2; je; neg bx     ; seventy five to whichever side
/// 0382e  add ax, bx; mov [di+4], ax
/// 03834  mov word [BalokFLAGS], 0
/// 0383a  mov ax, [si+0x12]                   ; Balok_Recover
/// ControlBalokBite:
/// 037c1  xor word [0x77a0], 1
/// 037c6  je  ControlBalokCrush               ; every other one is the squeeze
/// 037c8  mov word [0x783a], Balok_BiteKnight
/// 037ce  mov word [BalokFLAGS], 0; or word [BalokFLAGS], 0x40
/// ControlBalokCrush:
/// 037dc  mov word [0x783a], Balok_SqueezeKnight
/// ```
///
/// Neither the bite nor the squeeze takes a hit point itself: both scripts
/// call `KillKnight` through `TASKGOSUB` partway through, which is what
/// finishes him, and `0x40` is a bit nothing in `ControlBalok` reads, so the
/// frame after one of them the creature is back on the ordinary path.
fn balok_release(s: &Sight, shared: &mut Shared) -> Act {
    if !s.foe.alive() {
        // 037c1  xor word ptr [0x77a0], 1
        shared.balok_bite ^= 1;
        let crush = shared.balok_bite == 0;
        shared.balok = balok_flag::CHEWING;
        return Act::Grip {
            script: if crush {
                "Balok_SqueezeKnight".into()
            } else {
                "Balok_BiteKnight".into()
            },
            damage: 0,
            cost: 0,
            fatal: false,
            hold: true,
            victim: String::new(),
        };
    }
    shared.balok = 0;
    Act::Grip {
        script: "Balok_Recover".into(),
        damage: 0,
        cost: 0,
        fatal: false,
        hold: false,
        victim: String::new(),
    }
}

/// `ControlBeast`, `BeastCharge`, `SetBEASTZ` and `SetBeastTimer`: it does not
/// track at all. It runs from one side of the arena to the other, turns round
/// off the edge, waits, picks a depth and comes back.
fn beast(
    s: &Sight,
    brain: &mut Brain,
    seed: &mut u16,
    facing: &mut i32,
    at: &mut (i32, i32),
) -> Act {
    // 02fb2  mov word ptr [di + 0x28], 0x10
    //
    // **Unconditional, on every tick of the controller**, before any branch:
    // a beast's kind is the charge whether it is running, turning or waiting
    // off the edge. It never enters an attack state at all -- the charge is
    // its walk -- so a kind written only on an attack is a kind it never has,
    // and `InitKnightvsBeast`'s rows (0x2283, 0x2288), which are keyed on it,
    // could never be reached. That is the second reason `Beast_BackToss` was
    // never seen; the first was the banks.
    brain.kind = Some(Attack::Chop);
    // `BeastCharge`, 0x2fe6: the facing is the record's own `+8`, read to
    // choose which edge to test and written when the edge is reached.
    //
    //   02fe6  cmp byte ptr [di + 8], 3
    //   02fea  je  BeastChargeLeft
    //   02fec  cmp word ptr [di + 2], 0x154 ; facing right: past 340?
    //   02ff1  jl  BeastMove
    //   02ff3  mov word ptr [di + 2], 0x17c ; set down at 380
    //   02ff8  mov byte ptr [di + 8], 3     ; and turned to face left
    //   02ffc  jmp SetBEASTZ
    //   BeastChargeLeft:
    //   02ffe  cmp word ptr [di + 2], 0
    //   03002  jns BeastMove                ; facing left: past 0?
    //   03004  mov word ptr [di + 2], 0xffce ; set down at -50
    //   03009  mov byte ptr [di + 8], 1     ; and turned to face right
    //
    // **The edges are the literal ones, and the beast really is set down
    // beyond them.** It disappears off one side and comes back in at a fresh
    // depth, which is thirty pixels of run-up past the screen at either end.
    let leftward = *facing < 0;
    // 02fec and 02ffe.
    let turning = if leftward {
        s.me.x < 0
    } else {
        s.me.x >= 0x154
    };
    if turning {
        // 02ff3 / 03004: set down past the edge, facing back in.
        at.0 = if leftward { -50 } else { 0x17c };
        // 02ff8 / 03009: the turn is a write to `+8`.
        *facing = if leftward { 1 } else { -1 };
        // `SetBEASTZ`, 0x300d, and **this is the only place the beast's depth
        // is ever written**:
        //
        //   0300d  xor word ptr [BeastFLAGS], 1
        //   03012  je  03022
        //   03015  bx = [Opponent]; ax = [bx+6]
        //   0301d  mov word ptr [di+6], ax        ; dead on his line
        //   03020  jmp SetBeastTimer
        //   03022  ax = [Opponent's +6]
        //   0302c  call rnd; mov bx, ax
        //   03032  and bx, 7; shl bx, 1; shl bx, 1  ; 0, 4, 8 .. 28
        //   03039  add ax, bx
        //   0303b  mov word ptr [di+6], ax        ; and off it
        //
        // Every other turn is dead on the knight's row and the one between is
        // up to twenty eight rows deeper. Never nearer, and never adjusted
        // again until the next turn.
        brain.flags ^= flag::ONLINE;
        at.1 = if brain.flags & flag::ONLINE != 0 {
            s.foe.y
        } else {
            *seed = rnd(*seed);
            s.foe.y + ((*seed & 7) as i32) * 4
        };
        // `SetBeastTimer`, 0x303e, which it falls into:
        //
        //   0303e  call rnd; and ax, 0xf; or ax, 5
        //   03047  mov byte ptr [di+0xb], al      ; five to fifteen
        //   0304a  sub byte ptr [di+0xb], 1
        //   0304e  je  BeastMove
        //   03050  jmp 02d52                     ; stand still this frame
        //
        // **The timer is written, decremented once, and never read again.**
        // `or ax, 5` cannot leave nought, so the decrement cannot reach it
        // and the `je` at 0x304e is dead; nothing else in the image touches
        // `+0xb` on a beast (`DecTimer` at 0x2a63 is a different routine and
        // has no callers). So the beast stands still for exactly **one**
        // frame at the edge and charges straight back.
        //
        // This engine used to hold it there for the five to twenty frames the
        // number looks like, which is the shape of the code and not what it
        // does. The roll still happens, because it is what the routine does
        // and it moves the seed on.
        *seed = rnd(*seed);
        brain.timer = 0;
        return Act::Idle;
    }
    // `BeastMove`, 0x3053, the whole of it:
    //
    //   03053  add byte ptr [di+0xa], 1
    //   03057  and byte ptr [di+0xa], 3        ; four frames, not eight
    //   0305b  si = [di+0x1c]; [0x783a] = si[cycle]
    //   0306b  ax = cycle * 2
    //   03072  bx = BeastChargeOffsets + ax; bx = [bx]
    //   0307c  test byte ptr [di+8], 2; je; neg bx
    //   03084  add word ptr [di+2], bx
    //   03087  jmp 02d52
    //
    // **The column, and nothing else.** The beast does not track: it charges
    // dead straight across at whatever depth the last turn gave it, and the
    // only thing that changes its depth is the next turn. Steering it at the
    // knight every frame -- which is what this did -- is what had it milling
    // around his feet instead of running past him.
    //
    // `BeastChargeOffsets` (DS:0x773e) is `[33, 27, 17, 33]`, four words, and
    // it is a walk-speed table under another name: the sweep that found the
    // other six looked for `*WALK*` and this is not called that.
    let dir = if *facing < 0 { -1 } else { 1 };
    Act::Walk {
        dx: dir,
        dy: 0,
        script: None,
    }
}

/// `ControlDemon` and `DemonAttack`: the slap inside a hundred, the zap out to
/// a hundred and thirty, the whip out to a hundred and forty, and the whip's
/// own four phase follow-through.
///
/// Two of those four branches write the throw's direction and table:
///
/// ```text
/// 0504e  mov word [si+0x28], 0x10      ; the slap
/// 05053  mov word [0x783a], Demon_Slap
/// 05059  mov al, [si+8]
/// 0505c  mov [SLAP], al
/// 0505f  mov word [SLAPY], BalokSLAP
/// ...
/// 0508e  mov word [si+0x28], 2         ; the whip
/// 05093  mov word [0x783a], Demon_Whip
/// 0509e  mov al, [si+8]
/// 050a1  mov [SLAP], al
/// 050a4  mov word [SLAPY], DemonWHIP   ; five words in: the drag, not the throw
/// ```
///
/// The zap (0x5072) writes neither. The slap's row over the knight's `*Hit`
/// table is `InitKnightvsDemon+40` (0x2765), so `Knight_SwSlapped` reads the
/// words this branch chose; see `Bout::demon_struck_knight`.
fn demon(s: &Sight, brain: &mut Brain, facing: &mut i32, shared: &mut Shared) -> Act {
    if brain.flags & flag::UNBORN != 0 {
        // `[di+0x10]` is `Demon_Evolve`, so the demon's first script is its
        // own materialisation, and its last frame calls `AddDemonWhirl`.
        brain.flags &= !flag::UNBORN;
        return Act::Play("Demon_Evolve".into());
    }
    // The whip chain, which the original keeps as `DemonFLAGS` bits 1, 8,
    // 0x10 and 0x20 (ControlDemon+67..+122) and takes before `MonsterTrack`
    // is reached, so a demon following its whip through does not turn.
    if brain.phase > 0 {
        let d = find_distance(s.me, s.foe);
        if cooling(brain) {
            return Act::Idle;
        }
        let caught = brain.flags & flag::CAUGHT != 0;
        // The phase this pass *is*, kept before the advance below. Testing
        // the advanced value was a bug of its own: the two follows are
        // phases 2 and 4, and after `brain.phase = next` those read 3 and 0,
        // so only the under whip's follow ever fired and
        // `DemonOWhipFollow` (0x50de) was unreachable.
        let now = brain.phase;
        let (script, next, low, high, hold) = match brain.phase {
            // `DemonOFollowT`
            1 => ("Demon_OWhipMiss", 2u8, 120, 140, 4),
            // `DemonOWhipFollow`
            2 => ("Demon_OWhipHit", 3, 0, -1, 3),
            // `DemonUFollowT`
            3 => ("Demon_UWhipMiss", 4, 130, 150, 5),
            // `DemonUWhipFollow`
            _ => ("Demon_UWhipHit", 0, 0, -1, 5),
        };
        brain.phase = next;
        brain.cooldown = hold;
        if matches!(now, 2 | 4) && caught {
            // The follow-through with the knight already caught: the original
            // hands him `Knight_SwSlapped` outright rather than waiting for a
            // weapon part to touch him.
            // `DemonOWhipFollow` (0x50de) and `DemonUWhipFollow` (0x5119),
            // which are the same six instructions with a different cooldown:
            //
            //   050c0  test word [DemonFLAGS], 0x40   ; has it caught him?
            //   050c9  mov si, [0xa64]                ; the demon's record
            //   050cd  mov al, [si+8]
            //   050d0  mov [SLAP], al                 ; the way IT faces
            //   050d4  mov ax, [0x978]; call 0x9886   ; the knight's record
            //   050de  mov si, 0x1596                 ; Knight_SwSlapped
            //   050e1  call REPLACEANIM
            //   050e5  and word [DemonFLAGS], 0xffbf  ; let him go
            //   050ea  mov byte [si+0x4a], 3          ; 5 for the under whip
            //
            // The direction is written again here, off the demon's `+8` as
            // it stands now rather than as it stood when the whip went out,
            // and `SLAPY` still points at `DemonWHIP`, whose four words are
            // -7, -3, -1, 0. So the knight is dragged in, not thrown.
            brain.flags &= !flag::CAUGHT;
            shared.slap = if *facing < 0 { 3 } else { 1 };
            shared.slap_y = slap_y::DEMON_WHIP;
            let hit = if now == 4 {
                "Demon_UWhipHit"
            } else {
                "Demon_OWhipHit"
            };
            return Act::Strike {
                script: hit.into(),
                damage: s.me.damage,
                fatal: false,
                victim: Some("Knight_SwSlapped".into()),
            };
        }
        if d >= low && d <= high {
            brain.flags |= flag::CAUGHT;
            // The catch happens on the two tracking phases, 1 and 3.
            // Written against `now` rather than the advanced value, which
            // happened to agree here and did not above.
            let caught_script = if now == 1 {
                "Demon_OWhipKnight"
            } else {
                "Demon_UWhipKnight"
            };
            return Act::Play(caught_script.into());
        }
        return Act::Play(script.into());
    }
    // 04fb1  cmp word ptr [si + 0x38], 0; 04fb5 jg; else the exit
    if !s.foe.alive() {
        return Act::Idle;
    }
    // ControlDemon+135 (0x4fba): `call MonsterTrack`.
    let t = track(s.me, s.foe, s.def, facing);
    let d = t.distance;
    // `DemonAttack`, 0x5029: the cooldown, and `DemonMove` while it runs.
    if cooling(brain) {
        return walk(t);
    }
    // `ControlDemon` only reaches `DemonAttack` on the same plane: off it,
    // `ZPLANE` is zero and the tracker's walk bits carry it across instead.
    if !t.plane {
        return walk(t);
    }
    if d <= 100 {
        // `demonbodge`: two turns of nothing between slaps.
        if brain.timer > 0 {
            brain.timer -= 1;
            return Act::Idle;
        }
        brain.timer = 2;
        brain.cooldown = 9;
        // 05059  mov al, [si+8]; 0505c mov [SLAP], al
        // 0505f  mov word [SLAPY], BalokSLAP
        shared.slap = if *facing < 0 { 3 } else { 1 };
        shared.slap_y = slap_y::BALOK_SLAP;
        return Act::Attack {
            kind: Attack::Chop,
            spawn: None,
        };
    }
    if d <= 130 {
        brain.cooldown = 6;
        return Act::Attack {
            kind: Attack::Swing,
            spawn: None,
        };
    }
    if d <= 140 {
        brain.cooldown = 5;
        brain.phase = 1;
        // 0509e  mov al, [si+8]; 050a1 mov [SLAP], al
        // 050a4  mov word [SLAPY], DemonWHIP
        //
        // The whip's own table, which is [`BALOK_SLAP`] five words in and so
        // all negative: a caught knight is dragged towards the demon rather
        // than thrown from it. `DemonOWhipFollow` (0x50de) and
        // `DemonUWhipFollow` (0x5119) hand him `Knight_SwSlapped` to read it
        // with, and both write the direction again themselves off the
        // demon's facing as it stands then; see the whip chain above.
        shared.slap = if *facing < 0 { 3 } else { 1 };
        shared.slap_y = slap_y::DEMON_WHIP;
        return Act::Attack {
            kind: Attack::Lunge,
            spawn: None,
        };
    }
    walk(t)
}

// ------------------------------------------------------------------ the dragon
//
// `ControlDragon` (0x3843) and everything under it, translated block for
// block off `research/main.final.bin`. The dragon is not a creature that
// walks up to you: its record is the fixed one at DS:`0x6e26`, its head sits
// in a corridor thirty to a hundred wide at the left of the arena, and the
// only movement it has is `TrackKnight` shuffling that head five pixels at a
// time and the two ballistic arcs `DragonMove` and `DragonMoveLow` put it on
// to lift it and lower it. The two claws are records of their own on
// `ControlClaw` (0x3b24), pinned to the head's depth by `DragonMoveClaw1`
// (0x3bb4), which every path out of the head's controller runs through.
//
// The two branches at the top of `ControlDragon`, `DragonStruck` (0x3a73) for
// `+0xe` and `DragonHit2` (0x3ad5) for `+0xc`, are the bout's: this engine
// resolves a blow after the controllers have spoken, so they are in
// `Bout::dragon_struck` and `Bout::dragon_bites` with the same listings.

/// `TrackKnight`, image 0x3be8: the head follows the knight, five pixels at a
/// time, inside its corridor, and faces right whatever `FaceKnight` said.
///
/// ```text
/// 03be8  mov si, 0x6e26            ; the dragon's record
/// 03beb  mov word [si+0x26], 0
/// 03bf0  mov [0x77e8], si
/// 03bf4  mov ax, [KnightTable]; mov [Opponent], ax
/// 03bfa  test word [DragonFLAGS], 0x40
/// 03c00  je  03c18
/// 03c02  mov ax, [si+0x52]; mov [DRN], ax    ; breathing: the ranges saved
/// 03c08  mov ax, [si+0x52]; mov [DCL], ax    ; (+0x52 twice, see Shared)
/// 03c0e  mov word [si+0x52], 2
/// 03c13  mov word [si+0x54], 1               ; and tracked to two
/// 03c18  call MonsterTrack
/// 03c1b  mov si, [0x77e8]
/// 03c1f  mov byte [si+8], 1                  ; the head faces right
/// 03c23  mov ax, [si+0x26]
/// 03c26  test ax, 1; je 03c39
/// 03c2b  mov bx, [si+2]; add bx, 5
/// 03c31  cmp bx, 0x64; jge 03c39             ; right, up to a hundred
/// 03c36  mov [si+2], bx
/// 03c39  test ax, 2; je 03c4c
/// 03c3e  mov bx, [si+2]; sub bx, 5
/// 03c44  cmp bx, 0x1e; jle 03c4c             ; left, down to thirty
/// 03c49  mov [si+2], bx
/// 03c4c  test ax, 8; je 03c55
/// 03c51  sub word [si+6], 5                  ; up a row of five
/// 03c55  test ax, 4; je 03c5e
/// 03c5a  add word [si+6], 5                  ; down a row of five
/// 03c5e  mov ax, [0x77e8]; call 0x96a2       ; its task
/// 03c6a  mov ax, [di+2]; mov [si+4], ax      ; task x
/// 03c70  mov ax, [di+6]; mov [si+8], ax      ; task z
/// 03c76  test word [DragonFLAGS], 0x40; je 03c8a
/// 03c7e  mov ax, [DRN]; mov [si+0x52], ax    ; the ranges put back
/// 03c84  mov ax, [DCL]; mov [si+0x54], ax
/// 03c8a  ret
/// ```
///
/// Called from `DragonMove+11` (0x3877) every frame a head move is not
/// running, and by `TASKGOSUB` from inside `Dragon_HighBreath` (0x4448) and
/// `Dragon_LowBreath` (0x4130), five times each, which is how the head keeps
/// pointing at him while the fire is out. The task's x and z are copied off
/// the record here (0x3c6a, 0x3c70); this engine's task follows the fighter
/// every tick, so `at` going back onto the fighter is that copy.
pub fn track_knight(
    foe: &Fighter,
    def: &ActorDef,
    shared: &mut Shared,
    facing: &mut i32,
    at: &mut (i32, i32),
) -> Track {
    // 03bfa  test word ptr [DragonFLAGS], 0x40
    let breathing = shared.dragon & dragon_flag::BREATHING != 0;
    let standing = shared.dragon_ranges.unwrap_or((def.approach, def.back_off));
    let ranges = if breathing {
        // 03c0e  mov word ptr [si + 0x52], 2; 03c13 mov word ptr [si + 0x54], 1
        (2, 1)
    } else {
        standing
    };
    // 03c18  call MonsterTrack
    let t = track_from(*at, foe, def, ranges, facing);
    // 03c1f  mov byte ptr [si + 8], 1
    *facing = 1;
    // 03c26  test ax, 1
    if t.dx == 1 {
        // 03c2b  mov bx, [si+2]; add bx, 5; cmp bx, 0x64; jge
        let bx = at.0 + 5;
        if bx < 0x64 {
            at.0 = bx;
        }
    }
    // 03c39  test ax, 2
    if t.dx == -1 {
        // 03c3e  mov bx, [si+2]; sub bx, 5; cmp bx, 0x1e; jle
        let bx = at.0 - 5;
        if bx > 0x1e {
            at.0 = bx;
        }
    }
    // 03c4c  test ax, 8
    if t.dy == -1 {
        // 03c51  sub word ptr [si + 6], 5
        at.1 -= 5;
    }
    // 03c55  test ax, 4
    if t.dy == 1 {
        // 03c5a  add word ptr [si + 6], 5
        at.1 += 5;
    }
    // 03c76  test word ptr [DragonFLAGS], 0x40; 03c7e..03c87 the restore,
    // with `DCL` holding what `+0x52` held (0x3c08).
    if breathing {
        shared.dragon_ranges = Some((standing.0, standing.0));
    }
    t
}

/// The stance `ControlDragon` opens with: `mov ax, [si+0x10]` (0x3843),
/// which `Dragon_LiftHead1`'s `TASKSAVE` (0x4200) has made
/// `Dragon_HighStance` and `Dragon_LowerHead1`'s (0x433c) `Dragon_Stance`.
/// Bit 0x20 is set on the same controller pass that names `LiftHead1`, so
/// the bit and the saved word agree on every pass that reads them.
fn dragon_stance(shared: &Shared) -> Act {
    Act::Stand(if shared.dragon & dragon_flag::HEAD_UP != 0 {
        "Dragon_HighStance".into()
    } else {
        "Dragon_Stance".into()
    })
}

/// The `JUMPD` template `DragonMove+68` (0x38af) and `DragonMoveLow+43`
/// (0x3929) fill and hand to `ADDJUMP`: the head from where it is to x 100,
/// the knight's own depth row and the height given, over the frames given.
///
/// ```text
/// 038ac  mov si, 0x77cc            ; JUMPD
/// 038af  mov di, 0x6e26            ; the head
/// 038b2  mov word [di+0x4a], 0xd   ; (9 at 0x392c)
/// 038b7  mov [si], di              ; +0   the actor
/// 038bc  mov ax, [di+2]; mov [si+4], ax     ; x0
/// 038c4  mov ax, [di+6]; mov [si+6], ax     ; z0
/// 038cc  mov ax, [di+4]; mov [si+8], ax     ; y0, the height
/// 038d4  mov word [si+0xa], 0x64            ; x1 = 100
/// 038db  mov ax, [bx+6]; mov [si+0xc], ax   ; z1 = the knight's depth
/// 038e3  mov word [si+0xe], 0xffba          ; y1 = -70 (-30 at 0x395d)
/// 038ea  mov word [si+0x10], 0xd            ; 13 frames (9 at 0x3964)
/// 038f1  mov word [si+0x12], 0x50           ; rise 0x50 (0x78 at 0x396b)
/// 038f9  call ADDJUMP
/// ```
fn dragon_head_plan(
    at: (i32, i32),
    height: i32,
    foe: &Fighter,
    y1: i32,
    steps: i32,
    rise: i32,
) -> Plan {
    Plan {
        x0: at.0,
        z0: at.1,
        y0: height,
        x1: 0x64,
        z1: foe.y,
        y1,
        steps,
        rise,
    }
}

/// `DragonHeadMove`, image 0x3979: one frame of the lift or the lower.
///
/// ```text
/// 03979  sub word [di+0x4a], 1
/// 0397d  jne 03984
/// 0397f  and word [DragonFLAGS], 0xffef   ; the count ran out: not moving
/// 03984  mov si, [Opponent]
/// 03988  mov ax, [di+6]
/// 0398b  cmp ax, [si+6]
/// 0398e  jl  03996
/// 03990  sub word [di+6], 5                ; deeper than him: up five
/// 03994  jmp 0399a
/// 03996  add word [di+6], 5                ; nearer: down five
/// 0399a  mov ax, di; call ControlJump      ; bx = x, dx = height (cx, the
/// 0399f  mov di, 0x6e26                    ; arc's own z, is not stored)
/// 039a2  mov [di+2], bx
/// 039a5  mov [di+4], dx
/// 039a8  add byte [di+0xa], 1
/// 039ac  xor ax, ax; mov al, [di+0xa]
/// 039b1  cmp al, 8; jl 039b7
/// 039b5  mov al, 7                         ; the row is eight long
/// 039b7  shl ax, 1
/// 039b9  mov si, [di+0x1c]                 ; DragonWal
/// 039bc  test word [DragonFLAGS], 0x20
/// 039c2  je  039cf
/// 039c4  add si, ax; mov ax, [si+0x10]     ; up: the lift row
/// 039c9  mov [0x783a], ax
/// 039cc  jmp DragonMoveClaw1
/// 039cf  add si, ax; mov ax, [si+0x20]     ; down: the lower row
/// 039d4  mov [0x783a], ax
/// 039d7  jmp DragonMoveClaw1
/// ```
///
/// The frame counter and the arc are the same number of frames (0x38b2 and
/// 0x38ea, 0x392c and 0x3964), so the arc lands on the pass that clears the
/// bit. The walk byte runs on past seven and the row's last script is held.
fn dragon_head_move(s: &Sight, brain: &mut Brain, shared: &mut Shared, at: &mut (i32, i32)) -> Act {
    // 03979  sub word ptr [di + 0x4a], 1; 0397d jne
    brain.cooldown -= 1;
    if brain.cooldown == 0 {
        // 0397f  and word ptr [DragonFLAGS], 0xffef
        shared.dragon &= !dragon_flag::HEAD_MOVING;
    }
    // 03988  mov ax, [di+6]; cmp ax, [si+6]; jl
    if at.1 < s.foe.y {
        // 03996  add word ptr [di + 6], 5
        at.1 += 5;
    } else {
        // 03990  sub word ptr [di + 6], 5
        at.1 -= 5;
    }
    // 0399c  call ControlJump; 039a2 mov [di+2], bx; 039a5 mov [di+4], dx
    if let Some(j) = brain.jump.as_mut() {
        let step = j.step();
        at.0 = step.x;
        brain.height = step.y;
        if step.done {
            brain.jump = None;
        }
    }
    // 039a8  add byte ptr [di + 0xa], 1
    brain.walk = (brain.walk + 1) & 0xff;
    // 039ae  mov al, [di+0xa]; cmp al, 8; jl; mov al, 7
    let frame = brain.walk.min(7) as usize;
    // 039bc  test word ptr [DragonFLAGS], 0x20
    let row = if shared.dragon & dragon_flag::HEAD_UP != 0 {
        // 039c6  mov ax, [si+0x10]: `DragonWal` +0x10, the lift row
        "lift"
    } else {
        // 039d1  mov ax, [si+0x20]: +0x20, the lower row
        "lower"
    };
    match s.def.scripts_for(row).get(frame) {
        Some(script) => Act::Stand(script.clone()),
        None => dragon_stance(shared),
    }
}

/// `ControlDragon`, image 0x3843, from `DragonMove` (0x386c) down.
///
/// ```text
/// 03843  mov ax, [si+0x10]; mov [0x783a], ax   ; the stance, see dragon_stance
/// 03849  mov [0x77e8], si; mov di, si
/// 0384f  mov ax, [KnightTable]; mov [Opponent], ax
/// 03855  cmp word [di+0xe], 0; jne DragonStruck  ; Bout::dragon_struck
/// 0385e  cmp word [di+0xc], 0; jne DragonHit2    ; Bout::dragon_bites
/// 03867  mov word [di+0x28], 0
/// DragonMove:
/// 0386c  test word [DragonFLAGS], 0x10
/// 03872  jne DragonHeadMove
/// 03877  call TrackKnight
/// 0387a  call FindDistance; mov [DDIS], ax
/// 03880  cmp ax, 0x8c
/// 03883  jge DragonMoveLow
/// 03885  test word [DragonFLAGS], 0x20
/// 0388b  jne DragonAttack                  ; inside 140 and up: attack
/// 03890  or  word [DragonFLAGS], 0x10
/// 03895  or  word [DragonFLAGS], 0x20      ; inside 140 and down: lift
/// 0389a  mov byte [di+0xa], 0
/// 0389e  mov si, [di+0x1c]; mov ax, [si+0x10]; mov [0x783a], ax   ; LiftHead1
/// 038a7  ...the JUMPD template, see dragon_head_plan, to -70 in 13
/// 038f9  call ADDJUMP
/// 038fc  jmp DragonMoveClaw1
/// DragonMoveLow:
/// 038ff  test word [DragonFLAGS], 0x20
/// 03905  je  DragonAttack                  ; outside 140 and down: attack
/// 0390a  or  word [DragonFLAGS], 0x10
/// 0390f  and word [DragonFLAGS], 0xffdf    ; outside 140 and up: lower
/// 03914  mov byte [di+0xa], 0
/// 03918  mov si, [di+0x1c]; mov ax, [si+0x20]; mov [0x783a], ax   ; LowerHead1
/// 03921  ...the template, to -30 in 9
/// 03973  call ADDJUMP
/// 03976  jmp DragonMoveClaw1
/// DragonAttack:
/// 039da  and word [DragonFLAGS], 0xffbf    ; not breathing
/// 039df  mov si, [Opponent]
/// 039e3  cmp word [si+0x38], 0; jg 039ec
/// 039e9  jmp DragonMoveClaw1               ; he is down: the stance
/// 039ec  cmp word [ZPLANE], 0; jne 039f6
/// 039f3  jmp DragonMoveClaw1               ; off his plane: the stance
/// 039f6  test word [DragonFLAGS], 0x20
/// 039fc  je  DragonLowAttack
/// 039fe  cmp word [DDIS], 0x46
/// 03a03  jle 03a35                         ; inside seventy: see below
/// 03a05  and word [DragonFLAGS], 0xff7f
/// 03a0b  mov word [di+0x28], 0x10
/// 03a10  cmp word [dragonbodge1], 0
/// 03a15  je  03a1e
/// 03a17  dec word [dragonbodge1]
/// 03a1b  jmp DragonMoveClaw1               ; a frame of nothing
/// 03a1e  mov word [dragonbodge1], 2
/// 03a24  mov word [0x783a], Dragon_HighBreath
/// 03a2a  or  word [DragonFLAGS], 0x40
/// 03a2f  call AddDragonFIRE
/// 03a32  jmp DragonMoveClaw1
/// 03a35  test word [DragonFLAGS], 0x80
/// 03a3b  jne 03a05                         ; struck: the breath anyway
/// 03a3d  and word [DragonFLAGS], 0xff7f
/// 03a43  mov word [di+0x28], 2
/// 03a48  mov word [0x783a], Dragon_HighBite
/// 03a4e  jmp DragonMoveClaw1
/// DragonLowAttack:
/// 03a51  cmp word [dragonbodge2], 0
/// 03a56  je  03a5f
/// 03a58  dec word [dragonbodge2]
/// 03a5c  jmp DragonMoveClaw1
/// 03a5f  mov word [dragonbodge2], 2
/// 03a65  mov word [di+0x28], 4
/// 03a6a  mov word [0x783a], Dragon_LowBreath
/// 03a70  jmp DragonMoveClaw1
/// DragonMoveClaw1:
/// 03bb4  mov bx, [Claw1TABLE]; mov bp, [Claw2TABLE]
/// 03bbc  mov si, 0x6e26
/// 03bbf  mov ax, [si+6]; add ax, 0xa; mov [bx+6], ax    ; claw one ten deeper
/// 03bc8  sub ax, 0x1e; mov [bp+6], ax                   ; claw two twenty nearer
/// 03bcf  jmp NOTEND+3
/// ```
///
/// `DragonMoveClaw1` is the bout's: `Bout::monster_intent` pins each claw
/// to the head's depth on the claw's own pass, which is the same two numbers
/// on the same frame. Nothing here reads a cooldown: the two `dragonbodge`
/// words are the only wait the dragon has, and the ranges the head tracks
/// by are `TrackKnight`'s.
fn dragon(
    s: &Sight,
    brain: &mut Brain,
    facing: &mut i32,
    shared: &mut Shared,
    at: &mut (i32, i32),
) -> Act {
    use dragon_flag::*;
    // 0386c  test word ptr [DragonFLAGS], 0x10; 03872 jne DragonHeadMove
    if shared.dragon & HEAD_MOVING != 0 {
        return dragon_head_move(s, brain, shared, at);
    }
    // 03877  call TrackKnight
    let t = track_knight(s.foe, s.def, shared, facing, at);
    // 0387a  call FindDistance; 0387d mov [DDIS], ax
    let mut ax = at.0 - s.foe.x;
    if ax < 0 {
        ax = -ax;
    }
    shared.ddis = ax;
    // 03880  cmp ax, 0x8c; 03883 jge DragonMoveLow
    if ax >= 0x8c {
        // DragonMoveLow: 038ff test 0x20; je DragonAttack
        if shared.dragon & HEAD_UP != 0 {
            // 0390a  or 0x10; 0390f and 0xffdf; 03914 mov byte [di+0xa], 0
            shared.dragon |= HEAD_MOVING;
            shared.dragon &= !HEAD_UP;
            brain.walk = 0;
            // 0392c  mov word ptr [di + 0x4a], 9
            brain.cooldown = 9;
            // 03921..03973: the template, and ADDJUMP
            brain.jump = Some(dragon_head_plan(*at, brain.height, s.foe, -0x1e, 9, 0x78).start());
            // 0391b  mov ax, [si+0x20]: `DragonWal` +0x20, Dragon_LowerHead1
            return match s.def.scripts_for("lower").first() {
                Some(script) => Act::Stand(script.clone()),
                None => dragon_stance(shared),
            };
        }
    } else if shared.dragon & HEAD_UP == 0 {
        // 03890  or 0x10; 03895 or 0x20; 0389a mov byte [di+0xa], 0
        shared.dragon |= HEAD_MOVING | HEAD_UP;
        brain.walk = 0;
        // 038b2  mov word ptr [di + 0x4a], 0xd
        brain.cooldown = 13;
        // 038a7..038f9: the template, and ADDJUMP
        brain.jump = Some(dragon_head_plan(*at, brain.height, s.foe, -0x46, 13, 0x50).start());
        // 038a1  mov ax, [si+0x10]: `DragonWal` +0x10, Dragon_LiftHead1
        return match s.def.scripts_for("lift").first() {
            Some(script) => Act::Stand(script.clone()),
            None => dragon_stance(shared),
        };
    }
    // DragonAttack:
    // 039da  and word ptr [DragonFLAGS], 0xffbf
    shared.dragon &= !BREATHING;
    // 039e3  cmp word ptr [si + 0x38], 0; jg
    if s.foe.health <= 0 {
        return dragon_stance(shared);
    }
    // 039ec  cmp word ptr [ZPLANE], 0; jne
    if !t.plane {
        return dragon_stance(shared);
    }
    // 039f6  test word ptr [DragonFLAGS], 0x20; je DragonLowAttack
    if shared.dragon & HEAD_UP == 0 {
        // DragonLowAttack:
        // 03a51  cmp word ptr [dragonbodge2], 0; je
        if shared.dragon_bodge[1] != 0 {
            // 03a58  dec word ptr [dragonbodge2]
            shared.dragon_bodge[1] -= 1;
            return dragon_stance(shared);
        }
        // 03a5f  mov word ptr [dragonbodge2], 2
        shared.dragon_bodge[1] = 2;
        // 03a65  mov word ptr [di + 0x28], 4; 03a6a Dragon_LowBreath
        return Act::Attack {
            kind: Attack::Swing,
            spawn: None,
        };
    }
    // 039fe  cmp word ptr [DDIS], 0x46; jle 03a35
    // 03a35  test word ptr [DragonFLAGS], 0x80; jne 03a05
    if shared.ddis > 0x46 || shared.dragon & STRUCK != 0 {
        // 03a05  and word ptr [DragonFLAGS], 0xff7f
        shared.dragon &= !STRUCK;
        // 03a0b  mov word ptr [di + 0x28], 0x10
        // 03a10  cmp word ptr [dragonbodge1], 0; je
        if shared.dragon_bodge[0] != 0 {
            // 03a17  dec word ptr [dragonbodge1]
            shared.dragon_bodge[0] -= 1;
            return dragon_stance(shared);
        }
        // 03a1e  mov word ptr [dragonbodge1], 2
        shared.dragon_bodge[0] = 2;
        // 03a2a  or word ptr [DragonFLAGS], 0x40
        shared.dragon |= BREATHING;
        // 03a24  Dragon_HighBreath; 03a2f call AddDragonFIRE
        return Act::Attack {
            kind: Attack::Chop,
            spawn: Some("Dragon_Fire".into()),
        };
    }
    // 03a3d  and word ptr [DragonFLAGS], 0xff7f
    shared.dragon &= !STRUCK;
    // 03a43  mov word ptr [di + 0x28], 2; 03a48 Dragon_HighBite
    Act::Attack {
        kind: Attack::Lunge,
        spawn: None,
    }
}

/// `ControlClaw`, image 0x3b24: a forelimb.
///
/// ```text
/// 03b24  cmp word [DEAD_CLAWS], -1
/// 03b29  je  CLAWS_DEAD
/// 03b2b  mov ax, [si+0x10]; mov [0x783a], ax   ; the stance, Dragon_Claw
/// 03b31  mov [0x77e8], si; mov di, si
/// 03b37  mov ax, [KnightTable]; mov [Opponent], ax; mov si, [Opponent]
/// 03b41  cmp word [di+0xe], 0; jne ClawStruck
/// 03b47  cmp word [di+0xc], 0; jne ClawHit
/// 03b4d  push bx; mov bx, 0x6e26
/// 03b51  cmp word [bx+0x38], 0; pop bx
/// 03b56  jg  DragonAlive
/// 03b58  mov word [0x783a], Dragon_ClawDead   ; the head is down: so is this
/// 03b5e  jmp NOTEND+3
/// CLAWS_DEAD:
/// 03b61  mov word [0x783a], 0                 ; the task killed
/// 03b67  jmp NOTEND+3
/// DragonAlive:
/// 03b6a  cmp word [si+0x38], 0; jle 03b8b     ; he is down: the stance
/// 03b70  call CheckZAxis; or ax, ax; je 03b8b ; off its plane: the stance
/// 03b77  cmp word [si+2], 0x64; jg 03b8b      ; past a hundred: the stance
/// 03b7d  mov word [di+0x28], 0xa
/// 03b82  mov word [0x783a], Dragon_ClawSlap
/// 03b88  jmp NOTEND+3
/// 03b8b  jmp DragonMoveClaw1
/// ClawStruck:
/// 03b8d  mov ax, [di+0x10]; mov [0x783a], ax  ; struck: the stance, no damage
/// 03b93  jmp NOTEND+3
/// ClawHit:
/// 03b96  mov si, [di+0xc]
/// 03b99  cmp byte [si+0x35], 0xa; je 03bb1    ; it hit the head: nothing
/// 03b9f  mov word [SLAP], 1
/// 03ba5  mov word [SLAPY], BalokSLAP          ; the slap's direction and table
/// 03bab  mov ax, [di+0x10]; mov [0x783a], ax  ; and the stance
/// 03bb1  jmp NOTEND+3
/// ```
///
/// `ClawStruck` is [`Fighter::struck`] with [`Controller::takes_damage`]
/// false: the stance is the claw's own `*Hit` row. `ClawHit`'s `SLAP` words
/// feed `KnightSLAP` inside `Knight_SwSlapped`, which is built now
/// ([`slap_move`], `Bout::knight_slap`): `ClawHit+9` (0x3b9f) and
/// `ClawStruck1+9` (0x43dc) write the same 1 on the same blow, the second of
/// them on the knight's own side of it, so `Bout::dragon_struck_knight` makes
/// that write where it hands him the script and the branch is still the
/// stance here.
fn claw(s: &Sight, shared: &Shared) -> Act {
    // 03b24  cmp word ptr [DEAD_CLAWS], -1; je CLAWS_DEAD
    if shared.dead_claws == -1 {
        // 03b61  mov word ptr [0x783a], 0
        return Act::Vanish;
    }
    // 03b51  cmp word ptr [bx + 0x38], 0; jg DragonAlive
    if s.head_health.is_none_or(|h| h <= 0) {
        // 03b58  mov word ptr [0x783a], Dragon_ClawDead
        return Act::Stand("Dragon_ClawDead".into());
    }
    // DragonAlive:
    // 03b6a  cmp word ptr [si + 0x38], 0; jle
    if s.foe.health <= 0 {
        return Act::Idle;
    }
    // 03b70  call CheckZAxis; or ax, ax; je
    if !check_z(s.me, s.foe, s.def) {
        return Act::Idle;
    }
    // 03b77  cmp word ptr [si + 2], 0x64; jg
    if s.foe.x > 0x64 {
        return Act::Idle;
    }
    // 03b7d  mov word ptr [di + 0x28], 0xa; 03b82 Dragon_ClawSlap
    Act::Attack {
        kind: Attack::RThrust,
        spawn: None,
    }
}

/// `Progression`, DS:0x7c01, twenty bytes of data segment.
///
/// ```text
/// 14 0a 08 07 06 05 05 05 05 05 05 05 05 05 05 05 05 05 05 05
/// ```
///
/// `BKBlock` and `BKAttack` are the only readers, and the index is the day
/// count at DS:0x5b1 ([`Sight::progression`]). The byte is a percentage:
/// below it `BKBlock` does not even look at what the opponent is doing, and
/// at or below it `BKAttack` backs away instead of striking. Twenty on the
/// first day, ten on the second, and five from the sixth on.
///
/// The table is exactly twenty bytes: the next data symbol, `demonbodge`, is
/// at 0x7c15. `BKAttack` does not mask its index, so from the twenty-first
/// day the read falls into `demonbodge`, which the image initialises to zero,
/// and the knight stops hesitating altogether. `BKBlock` does mask, with
/// `and cx, 7`, so its own index wraps round to the first eight bytes.
#[rustfmt::skip]
pub const PROGRESSION: [i32; 20] = [
    0x14, 0x0a, 0x08, 0x07, 0x06, 0x05, 0x05, 0x05, 0x05, 0x05,
    0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05,
];

/// `[Progression + cx]` as `BKAttack` reads it, unmasked: past the table's
/// twenty bytes the read is `demonbodge`'s low byte, zero in the image.
fn progression_at(day: i32) -> i32 {
    if day < 0 {
        return PROGRESSION[0];
    }
    match PROGRESSION.get(day as usize) {
        Some(v) => *v,
        // 04cd2  cmp al, [bx]: off the end of the table, and zero.
        None => 0,
    }
}

/// `ControlBlackKnight`, image 0x4b79, and the whole controller under it:
/// `BKnightMove` (0x4bd3), `BKnightAttack` (0x4c13), `BKBlock` (0x4c40),
/// `_evadechop` (0x4cad), `BKAttack` (0x4cc3). Translated block for block.
///
/// This is the routine `InitGameStart+241` (0x1cfe) puts in `CONTROLTABLE`
/// slot 8, and kind 8 is what `InitGameStart` writes into `+0x35` of all four
/// knight records (0x1c69, 0x1c88, 0x1cab, 0x1cca). Slot 6 holds
/// `ControlKnight`, which reads the joystick, so every knight the machine
/// plays runs this and no knight a person plays ever does.
///
/// The opponent is picked at the top and is always the other knight:
///
/// ```text
/// 04b79  mov [0x77e8], si          ; me
/// 04b7d  mov di, si
/// 04b80  mov ax, [di+0x10]         ; the stance is the answer unless
/// 04b83  mov [0x783a], ax          ; something else is
/// 04b87  mov word [si+0x26], 0     ; no walk bits, and +0x28 is NOT cleared
/// 04b8c  cmp si, [KnightTable]
/// 04b90  jne 04b9c
/// 04b92  mov ax, [0x897b]          ; I am knight one: knight two
/// 04b96  mov [Opponent], ax
/// 04b9a  jmp 04ba4
/// 04b9c  mov ax, [KnightTable]     ; otherwise knight one
/// 04b9f  mov [Opponent], ax
/// 04ba4  cmp word [si+0xe], 0
/// 04ba8  je  04bad
/// 04baa  jmp BKnightStruck
/// 04bad  cmp word [si+0xc], 0
/// 04bb1  je  04bb6
/// 04bb3  jmp BKnightHit
/// 04bb7  mov ax, [si+0x28]
/// 04bba  mov [ATT], ax             ; the kind it last ordered
/// 04bbe  call MonsterTrack
/// 04bc1  or  ax, ax
/// 04bc3  je  BKnightAttack         ; in range on the plane: bx is the distance
/// 04bc5  cmp word [ZPLANE], 0
/// 04bca  je  BKnightMove           ; off the plane: walk
/// 04bcc  call FindDistance
/// 04bcf  mov bx, ax
/// 04bd1  jmp BKnightAttack
/// ```
///
/// `[0x8979]` and `[0x897b]` are the two knights of a knight-versus-knight
/// fight: `PracticeCombat5` (0x00fe, 0x0104) writes the first two records into
/// them and `InitKnightvsKnight+9` (0x2063) reads the second. There is no
/// third. Which of the two is `me` only decides which is `Opponent`, and the
/// bout hands the target in, so that branch is nothing here.
///
/// The `+0xe` and `+0xc` branches are raised where the blow lands, the way
/// the trogg's are: see [`black_knight_struck`] and [`black_knight_hit`].
fn black_knight(s: &Sight, brain: &mut Brain, seed: &mut u16, facing: &mut i32) -> Act {
    // Held by a mudman. The original cannot reach this: the only fight that
    // fields a kind 8 knight is knight versus knight, and a knight has no
    // hold. It is kept for the arena browser, which can field anything
    // against anything, and it is the same press `MudmenEntangle` reads.
    if s.me.held() {
        return Act::Struggle;
    }
    // 04bb7  mov ax, [si+0x28]; 04bba mov [ATT], ax
    let att = brain.att;
    // 04bbe  call MonsterTrack
    let t = track(s.me, s.foe, s.def, facing);
    // 04bc1  or ax, ax; 04bc3 je BKnightAttack
    let bx = if !t.walking {
        t.distance
    } else if !t.plane {
        // 04bc5  cmp word [ZPLANE], 0; 04bca je BKnightMove
        return bk_move(t);
    } else {
        // 04bcc  call FindDistance; 04bcf mov bx, ax
        find_distance(s.me, s.foe)
    };
    // BKnightAttack:
    // 04c13  mov di, [si+0x16]     (the *Att table: Act::Attack indexes it)
    // 04c17  mov bx, [Opponent]; 04c1b cmp word [bx+0x38], 0; 04c20 jg BKBlock
    if s.foe.health > 0 {
        return bk_block(s, brain, seed, att, bx, t, *facing);
    }
    // 04c22  mov byte [si+0x4a], 0
    brain.cooldown = 0;
    // 04c26  cmp bx, 0x5a; 04c29 jg BKnightMove
    if bx > 0x5a {
        return bk_move(t);
    }
    // 04c2b  cmp word [DeCapFLAG], 0; 04c30 jne A0$ (the stance)
    //
    // No gore test and no cooldown on this path, unlike `TroggAttack`, and it
    // does not raise `DeCapFLAG` itself: the decapitation script's own
    // `SetDecapFLAG` does that, which the bout sees as a corpse on a
    // finishing script.
    if s.decapped {
        return Act::Idle;
    }
    // 04c32  mov word [si+0x28], 4; 04c37 mov ax, [di+4]
    brain.att = Some(Attack::Swing);
    Act::Attack {
        kind: Attack::Swing,
        spawn: None,
    }
}

/// `BKnightMove`, image 0x4bd3.
///
/// ```text
/// 04bd3  cmp byte [si+0x26], 0
/// 04bd7  jne M0$
/// 04bd9  jmp 02d52                 ; no walk bit: the stance
/// M0$:
/// 04bdc  and byte [si+0x48], 0x7f  ; walking gives the evade its use back
/// 04be0  test byte [si+0x26], 8; 04be6 mov di, BKnightWALKU; call MoveU
/// 04bec  test byte [si+0x26], 4; 04bf2 mov di, BKnightWALKD; call MoveD
/// 04bf8  test byte [si+0x26], 1; 04bfe mov di, BKnightWALKR; call MoveR
/// 04c04  test byte [si+0x26], 2; 04c0a mov di, BKnightWALKR; call MoveL
/// 04c10  jmp MonsterWalk
/// ```
///
/// The four walk tables are the knight's own walk cycle, which is what an
/// [`Act::Walk`] with no script of its own plays; `MoveL` takes the same
/// `BKnightWALKR` as `MoveR` and mirrors it, which this engine does by the
/// facing. The `and byte [si+0x48], 0x7f` is [`Fighter::evaded`], and
/// `Fighter::apply` already clears it for any `State::Walk` order.
fn bk_move(t: Track) -> Act {
    walk(t)
}

/// `BKBlock`, image 0x4c40: whether to answer what the opponent is doing
/// rather than start something.
///
/// ```text
/// 04c41  call GETPERCENT
/// 04c45  mov cx, [0x5b1]           ; the day count
/// 04c49  and cx, 7
/// 04c4d  mov bx, Progression
/// 04c50  add bx, cx
/// 04c52  cmp al, [bx]
/// 04c55  jl  BKAttack              ; under the day's figure: do not look
/// 04c58  mov bx, [Opponent]
/// 04c5c  mov al, [bx+8]            ; his facing
/// 04c60  cmp al, [si+8]            ; against mine, as FaceKnight left it
/// 04c63  je  BKAttack              ; both facing the same way: his back is
///                                  ; to me, so there is nothing to stop
/// 04c66  cmp word [bx+0x28], 4     ; he is swinging
/// 04c6f  jne _notswing
/// 04c76  mov al, [bx+8]            ; the same comparison again, which
/// 04c7a  cmp al, [si+8]            ; cannot fail here
/// 04c7d  je  _notswing
/// 04c7f  call FindDistance
/// 04c82  cmp ax, 0x78
/// 04c85  jg  BKAttack              ; a hundred and twenty away: not yet
/// 04c87  mov word [si+0x28], 8     ; the block
/// 04c8c  mov ax, [di+8]
/// 04c92  jmp 02d52
/// _notswing:
/// 04c9a  cmp word [bx+0x28], 0x10  ; the overhead chop
/// 04c9f  je  _evadechop
/// 04ca6  cmp word [bx+0x28], 2     ; or the lunge
/// 04cab  jne BKAttack
/// _evadechop:
/// 04cad  call FindDistance
/// 04cb0  cmp ax, 0x78
/// 04cb3  jg  BKAttack
/// 04cb5  mov word [si+0x28], 0xe   ; the evade
/// 04cba  mov ax, [di+0xe]
/// 04cc0  jmp 02d52
/// ```
///
/// So a swing is blocked and a chop or a lunge is ducked, which is exactly
/// what `KnightBloSw` pairs them with, and neither is attempted from further
/// than a hundred and twenty.
fn bk_block(
    s: &Sight,
    brain: &mut Brain,
    seed: &mut u16,
    att: Option<Attack>,
    bx: i32,
    t: Track,
    facing: i32,
) -> Act {
    // 04c41  call GETPERCENT
    let al = percent(seed);
    // 04c45  mov cx, [0x5b1]; 04c49 and cx, 7
    let cx = (s.progression & 7) as usize;
    // 04c52  cmp al, [bx]; 04c55 jl BKAttack
    if al < PROGRESSION[cx] {
        return bk_attack(s, brain, seed, att, bx, t);
    }
    // 04c5c  mov al, [bx+8]; 04c60 cmp al, [si+8]; 04c63 je BKAttack
    if s.foe.facing == facing {
        return bk_attack(s, brain, seed, att, bx, t);
    }
    // 04c6a  cmp word [bx+0x28], 4; 04c6f jne _notswing
    if s.foe.attack == Some(Attack::Swing) {
        // 04c7f  call FindDistance; 04c82 cmp ax, 0x78; 04c85 jg BKAttack
        if find_distance(s.me, s.foe) > 0x78 {
            return bk_attack(s, brain, seed, att, bx, t);
        }
        // 04c87  mov word [si+0x28], 8
        brain.att = Some(Attack::Block);
        return Act::Attack {
            kind: Attack::Block,
            spawn: None,
        };
    }
    // _notswing: 04c9a chop, 04ca6 lunge, anything else BKAttack
    if s.foe.attack == Some(Attack::Chop) || s.foe.attack == Some(Attack::Lunge) {
        // _evadechop:
        // 04cad  call FindDistance; 04cb0 cmp ax, 0x78; 04cb3 jg BKAttack
        if find_distance(s.me, s.foe) > 0x78 {
            return bk_attack(s, brain, seed, att, bx, t);
        }
        // 04cb5  mov word [si+0x28], 0xe
        brain.att = Some(Attack::Evade);
        return Act::Attack {
            kind: Attack::Evade,
            spawn: None,
        };
    }
    bk_attack(s, brain, seed, att, bx, t)
}

/// `BKAttack`, image 0x4cc3: which attack, by range, and never the same one
/// twice running.
///
/// ```text
/// 04cc4  call GETPERCENT
/// 04cc8  mov cx, [0x5b1]           ; the day count, NOT masked here
/// 04ccd  mov bx, Progression
/// 04cd0  add bx, cx
/// 04cd2  cmp al, [bx]
/// 04cd5  jg  K0$                   ; over the day's figure: strike
/// 04cd7  call RND                  ; under it: give ground
/// 04cda  and ax, 7
/// 04cdd  mov bx, 0x5a
/// 04ce0  add bx, ax                ; ninety plus nought to seven, and dead:
/// 04ce2  jmp BKnightMove           ; BKnightMove reads only the walk bits
/// K0$:
/// 04ce5  cmp bx, 0x5a
/// 04ce8  jg  KK1$
/// 04cea  cmp word [ATT], 4
/// 04cef  je  KK1$                  ; not a swing twice running
/// 04cf1  mov word [si+0x28], 4     ; inside ninety: the swing
/// 04cf6  mov ax, [di+4]
/// KK1$:
/// 04cff  cmp bx, 0x5f
/// 04d02  jg  K2$
/// 04d04  cmp word [ATT], 0x10
/// 04d09  je  K2$
/// 04d0b  mov word [si+0x28], 0x10  ; inside ninety five: the overhead chop
/// 04d10  mov ax, [di+0x10]
/// K2$:
/// 04d19  cmp bx, 0x64
/// 04d1c  jg  K3$
/// 04d1e  cmp byte [si+0x34], 0     ; daggers left
/// 04d22  je  K5$                   ; none: lunge whatever it last did
/// 04d24  cmp word [ATT], 2
/// 04d29  je  K3$                   ; some, and it just lunged: throw one
/// K5$:
/// 04d2b  mov word [si+0x28], 2     ; inside a hundred: the lunge
/// 04d30  mov ax, [di+2]
/// K3$:
/// 04d39  cmp byte [si+0x34], 0
/// 04d3d  je  K4$
/// 04d3f  mov word [si+0x28], 6     ; further, with daggers: throw one
/// 04d44  mov ax, [di+6]
/// K4$:
/// 04d4d  jmp BKnightMove           ; further, with none: close
/// ```
///
/// `+0x34` is the dagger count: `SetKnightEquipment+38` (0x1fc8) writes ten,
/// `KnifeThrow+3` (0x3e2b) takes one off, `BuyDagger` and `MerchantDagger`
/// put them back, and `ControlBalok+199` reads the same byte.
///
/// The roll at 0x4cd7 is `RND` and not `GETPERCENT`, and what it builds in
/// `bx` is never read, because `BKnightMove` looks only at the walk bits
/// `MonsterTrack` already set. The roll is still spent, so it is spent here.
fn bk_attack(
    s: &Sight,
    brain: &mut Brain,
    seed: &mut u16,
    att: Option<Attack>,
    bx: i32,
    t: Track,
) -> Act {
    // 04cc4  call GETPERCENT
    let al = percent(seed);
    // 04cc8  mov cx, [0x5b1]: no `and cx, 7` on this one
    // 04cd2  cmp al, [bx]; 04cd5 jg K0$
    if al <= progression_at(s.progression) {
        // 04cd7  call RND; 04cda and ax, 7; 04cdd mov bx, 0x5a; 04ce0 add bx, ax
        *seed = rnd(*seed);
        return bk_move(t);
    }
    // K0$: 04ce5 cmp bx, 0x5a; 04ce8 jg KK1$; 04cea cmp [ATT], 4; 04cef je KK1$
    if bx <= 0x5a && att != Some(Attack::Swing) {
        brain.att = Some(Attack::Swing);
        return Act::Attack {
            kind: Attack::Swing,
            spawn: None,
        };
    }
    // KK1$: 04cff cmp bx, 0x5f; 04d02 jg K2$; 04d04 cmp [ATT], 0x10; 04d09 je K2$
    if bx <= 0x5f && att != Some(Attack::Chop) {
        brain.att = Some(Attack::Chop);
        return Act::Attack {
            kind: Attack::Chop,
            spawn: None,
        };
    }
    // K2$: 04d19 cmp bx, 0x64; 04d1c jg K3$
    // 04d1e  cmp byte [si+0x34], 0; 04d22 je K5$; 04d24 cmp [ATT], 2; 04d29 je K3$
    if bx <= 0x64 && (s.me.daggers() == 0 || att != Some(Attack::Lunge)) {
        brain.att = Some(Attack::Lunge);
        return Act::Attack {
            kind: Attack::Lunge,
            spawn: None,
        };
    }
    // K3$: 04d39 cmp byte [si+0x34], 0; 04d3d je K4$
    if s.me.daggers() != 0 {
        // 04d3f  mov word [si+0x28], 6
        brain.att = Some(Attack::Knife);
        return Act::Attack {
            kind: Attack::Knife,
            spawn: None,
        };
    }
    // K4$: 04d4d jmp BKnightMove
    bk_move(t)
}

/// `BKnightStruck`, image 0x4d50, and `BlackKnightStruck` (0x4d7f) under it:
/// the `+0xe` branch, which is the one thing the player's knight has no
/// equivalent of.
///
/// ```text
/// 04d50  mov di, [0x77e8]          ; me
/// 04d54  cmp word [di+0x38], 0
/// 04d58  je  BlackKnightStruck     ; no hit points: take it
/// 04d5a  mov si, [di+0xe]          ; whoever struck me
/// 04d5d  call CheckBlock
/// 04d60  cmp word [blockflag], 0
/// 04d65  je  BlackKnightStruck     ; not stopped: take it
/// 04d67  mov di, [0x77e8]
/// 04d6b  mov ax, [di+0x28]         ; the guard I am holding
/// 04d6e  mov si, [di+0x16]
/// 04d71  add si, ax
/// 04d73  mov ax, [si]              ; Att[that guard], played again
/// 04d78  and byte [di+0x48], 0x7f  ; and the evade's one use given back
/// 04d7c  jmp 02d52
/// ```
///
/// `CheckBlock` (0x420d) is already [`Fighter::blocks`], and the replay of
/// `Att[+0x28]` is `REPLACEANIM` on the script the fighter is already on,
/// which this engine does for any order that repeats. The line that is not
/// anywhere else is `and byte [di+0x48], 0x7f`: a stopped blow gives the
/// computer knight its evade back at once, where a person's knight only gets
/// it back by walking (`M0$`, and `ControlKnight`). Answers whether the
/// evade's one use is returned.
pub fn black_knight_blocked(controller: Controller) -> bool {
    // 04d78, on the blocked path and nowhere else.
    controller == Controller::Knight
}

/// `BKnightHit`, image 0x4da8, with `BKnightHitNormal` (0x4dbd) and
/// `BKnightHitKnight` (0x4dc6): the `+0xc` branch.
///
/// ```text
/// 04da8  mov di, [si+0xc]          ; what I hit
/// 04dab  cmp byte [di+0x35], 6
/// 04daf  je  BKnightHitKnight      ; a knight a person is playing
/// 04db1  cmp word [si+0x28], 0x10
/// 04db5  je  BKnightHitKnight      ; or my own overhead chop
/// 04db7  cmp word [si+0x28], 2
/// 04dbb  je  BKnightHitKnight      ; or my own lunge
/// BKnightHitNormal:
/// 04dbd  mov ax, [si+0x12]         ; the recovery: the blow is over
/// 04dc0  mov [0x783a], ax
/// 04dc3  jmp 02d52
/// BKnightHitKnight:
/// 04dc6  cmp word [di+0x28], 0xe
/// 04dca  je  BKnightHitNormal      ; he ducked it: recover after all
/// 04dcc  mov word [0x783a], 0xffff ; otherwise carry on: the blow follows
/// 04dd2  jmp 02d52                 ; through
/// ```
///
/// The player's knight has the same branch in `KnightHit1` (0x4123) and its
/// rule is not the same one: there the chop and the lunge recover like
/// anything else and only the up thrust carries on. Note also that
/// `BKnightHit` tests the victim's kind against 6 alone where `KnightHit1`
/// tests 6 and 8, so a computer knight that lands a swing on another computer
/// knight recovers from it.
///
/// `kind` is the striker's `+0x28`, `victim_player_knight` the `+0x35` test,
/// and `victim_evading` the victim's own `+0x28` being 0xe. Answers whether
/// the swing carries on (`0xffff`) rather than recovering.
pub fn black_knight_carries_on(
    kind: Option<Attack>,
    victim_player_knight: bool,
    victim_evading: bool,
) -> bool {
    // 04dab / 04db1 / 04db7
    let hit_knight =
        victim_player_knight || kind == Some(Attack::Chop) || kind == Some(Attack::Lunge);
    // 04dc6  cmp word [di+0x28], 0xe; 04dca je BKnightHitNormal
    hit_knight && !victim_evading
}

/// The state a fighter needs to answer a controller's questions, so this
/// module needs nothing from the bout.
impl Fighter {
    /// Held by something: the mudman's arms. A held fighter's own intent is
    /// ignored, and only the escape is read.
    pub fn held(&self) -> bool {
        self.holder.is_some()
    }

    /// How many daggers are left on the belt. `ControlBalok` reads exactly
    /// this to decide whether to close.
    pub fn daggers(&self) -> i32 {
        self.record.get(crate::taskvm::field::DAGGERS)
    }

    /// Standing on the spot, doing nothing anyone need answer.
    pub fn resting(&self) -> bool {
        matches!(self.state, State::Idle | State::Walk)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::tests::scripted_def;
    use std::collections::BTreeSet;

    fn at(x: i32, y: i32) -> Fighter {
        let d = scripted_def();
        Fighter::new("k", &d, x, y, 1)
    }

    fn ranged(approach: i32, back_off: i32) -> ActorDef {
        ActorDef {
            approach,
            back_off,
            depth_tolerance: 5,
            ..scripted_def()
        }
    }

    #[test]
    fn the_tracker_closes_holds_and_gives_ground() {
        let d = ranged(100, 80);
        let me = at(0, 50);
        let mut facing = -1;
        // Beyond the approach range: walk in.
        assert_eq!(track(&me, &at(200, 50), &d, &mut facing).dx, 1);
        // Inside it but outside the back-off: hold, and answer "in range".
        let t = track(&me, &at(90, 50), &d, &mut facing);
        assert_eq!((t.dx, t.walking), (0, false));
        // Inside the back-off: give ground.
        let t = track(&me, &at(40, 50), &d, &mut facing);
        assert_eq!((t.dx, t.walking), (-1, true));
        // Off the plane: walk in depth, and never answer "in range".
        let t = track(&me, &at(90, 90), &d, &mut facing);
        assert!(t.walking && !t.plane && t.dy == 1);
    }

    /// `MonsterTrack+32` is `call FaceKnight` before a single range is
    /// looked at, so every answer above turned the creature towards him,
    /// including the one that walks it away.
    #[test]
    fn the_tracker_faces_the_knight_before_it_decides_anything() {
        let d = ranged(100, 80);
        // He is to the right, at every range and on either plane: `+8` is 1.
        for (fx, fy) in [(200, 50), (90, 50), (40, 50), (90, 90)] {
            let mut facing = -1;
            track(&at(0, 50), &at(fx, fy), &d, &mut facing);
            assert_eq!(facing, 1, "foe at {fx},{fy}");
        }
        // He is to the left: 3. Giving ground walks right while facing left.
        let mut facing = 1;
        let t = track(&at(100, 50), &at(60, 50), &d, &mut facing);
        assert_eq!((facing, t.dx), (-1, 1), "backs away rightward, facing him");
        // `FaceKnight+16` is `jl`: the same column faces left.
        let mut facing = 1;
        track(&at(100, 50), &at(100, 50), &d, &mut facing);
        assert_eq!(facing, -1);
    }

    /// The trogg re-faces on every run of its controller, which is what the
    /// video the owner sent showed it failing to do: standing to the
    /// knight's right, swinging away from him.
    #[test]
    fn a_trogg_to_the_right_of_the_knight_turns_to_face_him_before_it_swings() {
        let def = creature("trogg", 100, 90);
        let mut b = Brain::default();
        // It was facing right, from wherever it last walked. The knight is
        // ninety five to its left, which is the swing.
        let mut facing = 1;
        let act = ask_facing(&def, &mut b, 200, 105, 0, &mut facing);
        assert_eq!(kind(&act), Some(Attack::Swing));
        assert_eq!(
            facing, -1,
            "`TroggStart+10` runs `MonsterTrack`, which faces him first"
        );
        // And inside the back-off it gives ground to the right, still facing
        // left at him.
        let mut b = Brain::default();
        let mut facing = 1;
        let act = ask_facing(&def, &mut b, 200, 150, 0, &mut facing);
        assert!(matches!(act, Act::Walk { dx: 1, .. }), "{act:?}");
        assert_eq!(facing, -1);
    }

    /// Every controller that tracks turns towards him; the ones that keep a
    /// facing of their own keep it.
    #[test]
    fn who_faces_the_knight_and_who_does_not() {
        for name in [
            "trogg",
            "trogg_spear",
            "troll",
            "ratman",
            "mudman",
            "balok",
            "demon",
            "knight",
        ] {
            let def = creature(name, 100, 90);
            // A fresh mind each side: a ratman that has already thrown itself
            // into the air on the first ask is in `RatmanLeaping` on the
            // second, and that branch turns nobody, which is the original.
            let mut b = Brain::default();
            let mut facing = 1;
            ask_facing(&def, &mut b, 200, 100, 0, &mut facing);
            assert_eq!(facing, -1, "{name} to the knight's right faces left");
            let mut b = Brain::default();
            let mut facing = -1;
            ask_facing(&def, &mut b, 0, 100, 0, &mut facing);
            assert_eq!(facing, 1, "{name} to the knight's left faces right");
        }
        // `TrackKnight+55`: the dragon's head faces right whatever side he is.
        let def = creature("dragon", 60, 20);
        let mut b = Brain::default();
        let mut facing = -1;
        ask_facing(&def, &mut b, 100, 40, 0, &mut facing);
        assert_eq!(facing, 1);
        // `ControlClaw` never writes `+8`.
        let def = creature("claw", 0, 0);
        let mut facing = -1;
        ask_facing(&def, &mut b, 5, 90, 0, &mut facing);
        assert_eq!(facing, -1);
    }

    /// A definition with one creature's controller and its recovered ranges.
    ///
    /// The ratman is given the script rows the baker gives it, under their own
    /// names, because half of its repertoire is choosing between them.
    fn creature(controller: &str, approach: i32, back_off: i32) -> ActorDef {
        let mut def = ActorDef {
            controller: controller.into(),
            approach,
            back_off,
            depth_tolerance: 5,
            speed_x: 2,
            speed_y: 1,
            ..scripted_def()
        };
        if controller == "ratman" {
            for (row, names) in [
                ("leap", vec!["Ratman_Leap"]),
                (
                    "fly",
                    vec![
                        "Ratman_Leap1",
                        "Ratman_Leap2",
                        "Ratman_Leap3",
                        "Ratman_Leap4",
                    ],
                ),
                ("leaps", vec!["Ratman_Leaps"]),
                ("hover_far", vec!["Ratman_HoverR"]),
                ("hover_near", vec!["Ratman_HoverD"]),
                ("snag", vec!["Ratman_SnagKnight"]),
                ("hang", vec!["Ratman_HangKnight"]),
                ("hung", vec!["Ratman_HungKnight"]),
                ("shake", vec!["Knight_HangSd"]),
                ("fall", vec!["Ratman_FallDown"]),
                ("sit", vec!["Ratman_SitOnHead"]),
                ("gouge", vec!["Ratman_EyeGouge"]),
                ("whack", vec!["Ratman_KnightWhack"]),
            ] {
                def.scripts.insert(
                    row.to_string(),
                    names.iter().map(|n| n.to_string()).collect(),
                );
            }
        }
        def
    }

    /// One tick of one controller against a knight standing at `foe_x`,
    /// with the creature facing `facing` on the way in; what `+8` holds on
    /// the way out is handed back through it.
    fn ask_facing(
        def: &ActorDef,
        brain: &mut Brain,
        me_x: i32,
        foe_x: i32,
        dy: i32,
        facing: &mut i32,
    ) -> Act {
        let me = at(me_x, 50);
        let foe = at(foe_x, 50 + dy);
        let s = Sight {
            me: &me,
            foe: &foe,
            def,
            gore: true,
            body: false,
            decapped: false,
            progression: 0,
            perch: None,
            foe_blow: 0,
            head_health: None,
        };
        let mut seed = 0x2f1du16;
        let mut at = (me.x, me.y);
        decide(
            &s,
            brain,
            &mut seed,
            facing,
            &mut crate::monster::Shared::default(),
            &mut at,
        )
    }

    /// The same, handing back `+2` and `+6` as the controller left them:
    /// `TrackKnight` (0x3c51) and `BeastCharge` (0x2ff3) both write the
    /// record's own position where they stand.
    fn ask_at(
        def: &ActorDef,
        brain: &mut Brain,
        me_x: i32,
        foe_x: i32,
        dy: i32,
        facing: &mut i32,
    ) -> (Act, (i32, i32)) {
        let me = at(me_x, 50);
        let foe = at(foe_x, 50 + dy);
        let s = Sight {
            me: &me,
            foe: &foe,
            def,
            gore: true,
            body: false,
            decapped: false,
            progression: 0,
            perch: None,
            foe_blow: 0,
            head_health: None,
        };
        let mut seed = 0x2f1du16;
        let mut here = (me.x, me.y);
        let act = decide(
            &s,
            brain,
            &mut seed,
            facing,
            &mut crate::monster::Shared::default(),
            &mut here,
        );
        (act, here)
    }

    /// The same, keeping the fight's own shared words and, for the ratman, a
    /// tree to leap into: what the two creatures with a repertoire need.
    fn ask_shared(
        def: &ActorDef,
        brain: &mut Brain,
        shared: &mut Shared,
        me: &Fighter,
        foe: &Fighter,
        perch: Option<Perch>,
    ) -> Act {
        let s = Sight {
            me,
            foe,
            def,
            gore: true,
            body: false,
            decapped: false,
            progression: 0,
            perch,
            foe_blow: 3,
            head_health: None,
        };
        let mut seed = 0x2f1du16;
        let mut facing = 1;
        let mut at = (me.x, me.y);
        decide(&s, brain, &mut seed, &mut facing, shared, &mut at)
    }

    /// One tick of one controller against a knight standing at `foe_x`.
    fn ask(def: &ActorDef, brain: &mut Brain, me_x: i32, foe_x: i32, dy: i32) -> Act {
        let mut facing = 1;
        ask_facing(def, brain, me_x, foe_x, dy, &mut facing)
    }

    fn kind(a: &Act) -> Option<Attack> {
        match a {
            Act::Attack { kind, .. } => Some(*kind),
            _ => None,
        }
    }

    /// The point of item 37: nine creatures, one position, nine answers.
    /// Standing eighty pixels away on the same plane, no two of these agree
    /// about what to do, which is what a plain opponent could never give.
    #[test]
    fn every_creature_answers_the_same_position_in_its_own_way() {
        let ranges = [
            ("trogg", 100, 90),
            ("trogg_spear", 130, 120),
            ("troll", 150, 90),
            ("ratman", 40, 30),
            ("mudman", 80, 75),
            ("balok", 80, 60),
            ("beast", 2, 1),
            ("demon", 95, 90),
            ("dragon", 60, 20),
        ];
        // What each of them does at eight ranges, from on top of the knight
        // to right across the arena.
        let profile = |name: &str, approach, back_off| {
            let def = creature(name, approach, back_off);
            let mut out = String::new();
            for d in [20, 45, 60, 80, 100, 120, 140, 180] {
                // A fresh mind at each range: what is being compared is what
                // each creature makes of a distance, not how long its last
                // blow left it standing.
                let mut brain = Brain::default();
                out.push_str(&format!("{:?};", ask(&def, &mut brain, 100, 100 + d, 0)));
            }
            out
        };
        let seen: Vec<(&str, String)> = ranges
            .iter()
            .map(|(n, a, b)| (*n, profile(n, *a, *b)))
            .collect();
        let answers: BTreeSet<&String> = seen.iter().map(|(_, a)| a).collect();
        assert_eq!(
            answers.len(),
            seen.len(),
            "two creatures still fight the same: {seen:#?}"
        );
        // And the shapes are the ones the routines have: the two troggs share
        // a controller and take different branches of it, and the beast is
        // the only one that never closes on him at all.
        let by = |n: &str| seen.iter().find(|(k, _)| *k == n).unwrap().1.clone();
        assert_ne!(
            by("trogg"),
            by("trogg_spear"),
            "the spear takes its own branch"
        );
        assert!(
            !by("beast").contains("Attack"),
            "the beast has no swing: {}",
            by("beast")
        );
        assert!(
            by("ratman").contains("Lunge"),
            "the ratman bites: {}",
            by("ratman")
        );
    }

    /// `TroggAttacks`: the overhead beyond a hundred, the swing inside it, and
    /// `TroggChop` refusing anything past a hundred and twenty.
    #[test]
    fn the_trogg_chops_at_arms_length_and_swings_up_close() {
        let def = creature("trogg", 100, 90);
        let mut b = Brain::default();
        assert_eq!(
            kind(&ask(&def, &mut b, 0, 110, 0)),
            Some(Attack::Chop),
            "110 is the chop"
        );
        let mut b = Brain::default();
        assert_eq!(
            kind(&ask(&def, &mut b, 0, 95, 0)),
            Some(Attack::Swing),
            "95 is the swing"
        );
        // Beyond a hundred and twenty `TroggChop` walks instead.
        let mut b = Brain::default();
        assert!(matches!(ask(&def, &mut b, 0, 125, 0), Act::Walk { .. }));
        // Inside the back-off range it gives ground rather than striking.
        let mut b = Brain::default();
        assert!(matches!(
            ask(&def, &mut b, 0, 60, 0),
            Act::Walk { dx: -1, .. }
        ));
        // And a blow is followed by ten frames of nothing.
        let mut b = Brain::default();
        ask(&def, &mut b, 0, 95, 0);
        assert_eq!(b.cooldown, 10);
        assert!(matches!(ask(&def, &mut b, 0, 95, 0), Act::Idle));
    }

    /// `TroggAttacks+0` to `+0xa` (0x2ea7..0x2eb1): `cmp byte [si+0x4a], 0;
    /// je attack; sub byte [si+0x4a], 1; jmp 0x2d52`. The decrement is
    /// followed by an unconditional jump to the stance, so ten frames of
    /// cooldown are ten frames of standing and the blow comes on the
    /// eleventh. `DemonAttack` (0x502f) is `sub; jne`, which frees the demon
    /// on the tenth; the trogg must not share it.
    #[test]
    fn the_trogg_stands_for_every_frame_of_its_cooldown_and_strikes_on_the_next() {
        let def = creature("trogg", 100, 90);
        let mut b = Brain::default();
        assert_eq!(kind(&ask(&def, &mut b, 0, 110, 0)), Some(Attack::Chop));
        assert_eq!(b.cooldown, 10, "TroggChop+8: mov byte [si+0x4a], 0xa");
        for frame in 1..=10 {
            assert!(
                matches!(ask(&def, &mut b, 0, 110, 0), Act::Idle),
                "frame {frame} of the cooldown is the stance"
            );
            assert_eq!(b.cooldown, 10 - frame, "one off per controller run");
        }
        assert_eq!(
            kind(&ask(&def, &mut b, 0, 110, 0)),
            Some(Attack::Chop),
            "the eleventh strikes"
        );
        // The finisher's count at TroggAttack+0x2e (0x2e92) has the same
        // shape: `je 02e9f; sub; jmp 02d52`.
        let mut b = Brain {
            cooldown: 2,
            ..Brain::default()
        };
        // Ninety five away: past the back-off at 0x2e68, inside the hundred
        // at 0x2e86.
        let me = at(0, 50);
        let foe = Fighter {
            health: 0,
            ..at(95, 50)
        };
        let sight = |body| Sight {
            me: &me,
            foe: &foe,
            def: &def,
            gore: true,
            body,
            decapped: false,
            progression: 0,
            perch: None,
            foe_blow: 0,
            head_health: None,
        };
        let mut seed = 0x2f1du16;
        let mut facing = 1;
        assert!(matches!(
            decide(
                &sight(true),
                &mut b,
                &mut seed,
                &mut facing,
                &mut Shared::default(),
                &mut (0, 0)
            ),
            Act::Idle
        ));
        assert!(matches!(
            decide(
                &sight(true),
                &mut b,
                &mut seed,
                &mut facing,
                &mut Shared::default(),
                &mut (0, 0)
            ),
            Act::Idle
        ));
        assert_eq!(b.cooldown, 0);
        // And `TroggAttack` never asks whether the corpse still shows a body:
        // 0x2e86 is the distance, 0x2e8b the flag, 0x2e92 the count, and
        // then the swing. `Sight::body` is the black knight's concern.
        assert_eq!(
            kind(&decide(
                &sight(false),
                &mut b,
                &mut seed,
                &mut facing,
                &mut Shared::default(),
                &mut (0, 0)
            )),
            Some(Attack::Swing),
            "TroggAttack+0x41: jmp TroggSwing"
        );
        assert_eq!(b.cooldown, 10);
    }

    /// `TroggStruck+3` (0x2f1c) is `mov byte ptr [di+0x4a], 0` and
    /// `TroggHit+8` (0x2f55) is `mov byte ptr [di+0x4a], 0xa`; neither runs
    /// `FaceKnight`.
    #[test]
    fn a_blow_taken_forgets_the_cooldown_and_a_blow_landed_restarts_it() {
        let mut b = Brain {
            cooldown: 7,
            ..Brain::default()
        };
        trogg_struck(&mut b);
        assert_eq!(b.cooldown, 0);
        trogg_hit(&mut b);
        assert_eq!(b.cooldown, 10);
    }

    /// `KnightGotStruck` (0x4267) and the three entries of its table that
    /// name `+8`, including the one that cannot be reached.
    #[test]
    fn only_a_demon_slap_and_a_dragon_claw_turn_what_they_hit() {
        // DemonStruck1 (0x4360): kind 0x10 and kind 2 fall into `DemonSlap`.
        for kind in [Attack::Chop, Attack::Lunge] {
            // 0437f  mov al, [si+8]; 04382 xor al, 2; 04384 mov [di+8], al
            assert_eq!(
                struck_facing(Controller::Demon, Some(kind), 1),
                Some(-1),
                "{kind:?}: turned to face a demon that faces right"
            );
            assert_eq!(struck_facing(Controller::Demon, Some(kind), -1), Some(1));
        }
        // 04378: any other kind of the demon's leaves the facing alone.
        for kind in [Attack::Swing, Attack::Knife, Attack::RThrust] {
            assert_eq!(struck_facing(Controller::Demon, Some(kind), 1), None);
        }
        // ClawStruck1+26 (0x43ed): `mov byte ptr [di + 8], 3`, whichever way
        // the claw itself faces.
        assert_eq!(
            struck_facing(Controller::Claw, Some(Attack::Swing), 1),
            Some(-1)
        );
        assert_eq!(struck_facing(Controller::Claw, None, -1), Some(-1));
        // BalokStruck1's own flip (0x427d) is dead code: the `jne` at 0x427b
        // reads the flags `add bx, ax` left at 0x4272, and `0x7843 + kind` is
        // never zero, so the jump over it is always taken.
        assert_eq!(
            struck_facing(Controller::Balok, Some(Attack::Swing), 1),
            None
        );
        // Nothing else in the table writes `+8`.
        for c in [
            Controller::Trogg,
            Controller::TroggSpear,
            Controller::Troll,
            Controller::Ratman,
            Controller::Mudman,
            Controller::Beast,
            Controller::Dragon,
            Controller::Knight,
        ] {
            assert_eq!(struck_facing(c, Some(Attack::Swing), 1), None, "{c:?}");
        }
    }

    /// `RatmanHit` (0x34f0): the claw that spins a knight caught with his
    /// back to it, and the three ways out before it.
    #[test]
    fn a_ratmans_claw_flips_whoever_it_catches_facing_the_same_way() {
        // 03519..03521: the two facings equal, so `call FlipKnight`.
        assert!(ratman_flips(false, false, 1, 1));
        assert!(ratman_flips(false, false, -1, -1));
        // 0351f  jne 03524: facing each other, nothing happens.
        assert!(!ratman_flips(false, false, -1, 1));
        assert!(!ratman_flips(false, false, 1, -1));
        // 034f3  cmp byte [si+0x35], 0x12: one of its own is not turned.
        assert!(!ratman_flips(true, false, 1, 1));
        // 034fc and 03502: the leap and the tail take their own branch and
        // never reach the flip.
        assert!(!ratman_flips(false, true, 1, 1));
    }

    /// `MoveBACK` (0x5783), all four corners and the vertical case.
    #[test]
    fn the_walk_runs_backwards_when_the_step_goes_against_the_facing() {
        // 0579b..057a4: facing right, walking right forwards, left backwards.
        assert_eq!(move_back(1, 1), 1);
        assert_eq!(move_back(1, -1), -1);
        // 0578e..05797: facing left, walking left forwards, right backwards.
        assert_eq!(move_back(-1, -1), 1);
        assert_eq!(move_back(-1, 1), -1);
        // MoveU (0x4e39) and MoveD (0x4e64): `mov bp, 1`, no MoveBACK.
        assert_eq!(move_back(1, 0), 1);
        assert_eq!(move_back(-1, 0), 1);
    }

    /// `TroggAttacks+0x2d` (0x2ed4) to `+0x44` (0x2eeb): past a hundred the
    /// overhead, and inside it the roll against thirty, with the knight's
    /// `+0x28` compared to 8 only when the roll comes up at thirty or under.
    #[test]
    fn the_trogg_chops_through_a_held_block_when_the_roll_is_low() {
        let def = creature("trogg", 100, 90);
        let me = at(0, 50);
        let blocking = Fighter {
            attack: Some(Attack::Block),
            state: State::Guard,
            ..at(95, 50)
        };
        let open = at(95, 50);
        fn sight<'a>(me: &'a Fighter, foe: &'a Fighter, def: &'a ActorDef) -> Sight<'a> {
            Sight {
                me,
                foe,
                def,
                gore: true,
                body: false,
                decapped: false,
                progression: 0,
                perch: None,
                foe_blow: 0,
                head_health: None,
            }
        }
        // Walk the register until it hands out a roll at thirty or under,
        // and one over thirty, and check the branch each takes.
        let mut seed = 0x2f1du16;
        let mut low = None;
        let mut high = None;
        for _ in 0..200 {
            let probe = seed;
            let mut s2 = probe;
            let roll = percent(&mut s2);
            if roll <= 30 && low.is_none() {
                low = Some(probe);
            }
            if roll > 30 && high.is_none() {
                high = Some(probe);
            }
            seed = rnd(seed);
        }
        let (low, high) = (low.expect("a low roll"), high.expect("a high roll"));
        let mut facing = 1;
        // 02edc  cmp ax, 0x1e; 02edf jg TroggSwing: over thirty never reads +0x28.
        let mut s = high;
        assert_eq!(
            kind(&decide(
                &sight(&me, &blocking, &def),
                &mut Brain::default(),
                &mut s,
                &mut facing,
                &mut Shared::default(),
                &mut (0, 0)
            )),
            Some(Attack::Swing)
        );
        // 02ee6  cmp word [bx+0x28], 8; 02eeb je TroggChop.
        let mut s = low;
        assert_eq!(
            kind(&decide(
                &sight(&me, &blocking, &def),
                &mut Brain::default(),
                &mut s,
                &mut facing,
                &mut Shared::default(),
                &mut (0, 0)
            )),
            Some(Attack::Chop)
        );
        let mut s = low;
        assert_eq!(
            kind(&decide(
                &sight(&me, &open, &def),
                &mut Brain::default(),
                &mut s,
                &mut facing,
                &mut Shared::default(),
                &mut (0, 0)
            )),
            Some(Attack::Swing),
            "a low roll on an open knight is still the swing"
        );
    }

    /// The spear takes `TroggAttacks`' kind 0x10 branch: one lunge, only
    /// inside `+0x52`, and twenty frames before the next.
    #[test]
    fn the_spear_trogg_only_lunges() {
        let def = creature("trogg_spear", 130, 120);
        let mut b = Brain::default();
        assert_eq!(kind(&ask(&def, &mut b, 0, 125, 0)), Some(Attack::Lunge));
        assert_eq!(b.cooldown, 20, "the spear waits twice as long as the axe");
        let mut b = Brain::default();
        assert!(
            matches!(ask(&def, &mut b, 0, 135, 0), Act::Walk { .. }),
            "past the approach it walks"
        );
    }

    /// `TrollAttack` compares the current kind before it chops, so the troll
    /// never swings its club overhead twice running.
    #[test]
    fn the_troll_never_chops_twice_running() {
        let def = creature("troll", 150, 90);
        let mut b = Brain::default();
        assert_eq!(kind(&ask(&def, &mut b, 0, 120, 0)), Some(Attack::Chop));
        assert_eq!(kind(&ask(&def, &mut b, 0, 120, 0)), Some(Attack::Swing));
        assert_eq!(kind(&ask(&def, &mut b, 0, 120, 0)), Some(Attack::Chop));
        // Inside a hundred it is always the club.
        let mut b = Brain::default();
        assert_eq!(kind(&ask(&def, &mut b, 0, 95, 0)), Some(Attack::Swing));
        assert_eq!(kind(&ask(&def, &mut b, 0, 95, 0)), Some(Attack::Swing));
    }

    /// `ControlRatCollide`: forty for the slash, fifty for the bite, and a
    /// leap at anything further. It never uses the tracker.
    #[test]
    fn the_ratman_slashes_bites_and_leaps_by_range() {
        let def = creature("ratman", 40, 30);
        let mut b = Brain::default();
        assert_eq!(
            kind(&ask(&def, &mut b, 0, 35, 0)),
            Some(Attack::Swing),
            "the slash"
        );
        let mut b = Brain::default();
        assert_eq!(
            kind(&ask(&def, &mut b, 0, 45, 0)),
            Some(Attack::Lunge),
            "the bite"
        );
        // The leap, which used to be a walk on a named script and is now the
        // original's own ballistic arc. `RatmanInitLeap` (0x3215) aims it,
        // `RatmanLeaps` (0x325f) names the frame and raises `+0x48 & 1`, and
        // `RatmanLeaping` (0x3270) steps it until it comes down.
        let mut b = Brain::default();
        let mut shared = Shared::default();
        let (me, foe) = (at(0, 50), at(120, 50));
        let first = ask_shared(&def, &mut b, &mut shared, &me, &foe, None);
        assert_eq!(
            first,
            Act::Play("Ratman_Leap".into()),
            "RatmanLeaps names the frame it leaves the ground on"
        );
        assert!(b.flags & flag::LEAPING != 0, "0x3263: +0x48 |= 1");
        assert!(b.jump.is_some(), "and a slot in the jump table is running");
        // Every frame after it is `RatmanLeaping` moving the rat itself.
        let mut me = me;
        let mut airborne = 0;
        let mut highest = 0;
        let mut landed = false;
        for _ in 0..40 {
            match ask_shared(&def, &mut b, &mut shared, &me, &foe, None) {
                Act::Fly { x, y, script } => {
                    me.x = x;
                    me.y = y;
                    highest = highest.min(b.height);
                    if script.is_empty() {
                        landed = true;
                        break;
                    }
                    airborne += 1;
                }
                other => panic!("expected a step of the arc, got {other:?}"),
            }
        }
        assert!(landed, "the arc ends");
        assert!(airborne >= 4, "and lasts more than a frame: {airborne}");
        assert!(highest < 0, "it left the ground: {highest}");
        assert_eq!(b.height, 0, "0x328d: mov word ptr [si + 4], 0");
        assert_eq!(b.flags & flag::LEAPING, 0, "0x3292: mov [si+0x48], 0");
        assert!(me.x > 0, "and it covered ground: {}", me.x);
        // Close in it is a hop rather than a leap: `RatmanInitLeap+31`
        // (0x323b) raises `+0x48 & 0x80` and gives it two pixels of rise.
        let mut b = Brain::default();
        let mut shared = Shared::default();
        let hop = ask_shared(&def, &mut b, &mut shared, &at(0, 50), &at(60, 50), None);
        assert!(b.flags & flag::SHORT_HOP != 0, "0x323b: +0x48 |= 0x80");
        assert!(
            b.flags & flag::LEAPING == 0,
            "and never through RatmanLeaps"
        );
        assert!(
            matches!(hop, Act::Play(ref n) if n.starts_with("Ratman_Leap")),
            "drawn on the up row from the first frame: {hop:?}"
        );
    }

    /// `RatmanLeap` (0x31a9): the first rat to want to leap takes the tree,
    /// sits in it for thirty frames, and the next one has to go for the
    /// knight because `RatFLAGS & 4` is up.
    #[test]
    fn the_first_ratman_leaps_into_the_tree_and_the_next_one_does_not() {
        let def = creature("ratman", 40, 30);
        let tree = Perch {
            x: 160,
            y: 112,
            height: -88,
        };
        let mut shared = Shared::default();
        let mut b = Brain::default();
        let mut me = at(0, 50);
        let foe = at(120, 50);
        let first = ask_shared(&def, &mut b, &mut shared, &me, &foe, Some(tree));
        assert_eq!(first, Act::Play("Ratman_Leap".into()));
        assert!(b.flags & flag::TREE_BOUND != 0, "0x31c3: +0x48 |= 4");
        assert_eq!(shared.rat & rat_flag::TREE, rat_flag::TREE, "0x31be");
        assert_eq!(b.cooldown, 0x1e, "0x31d3: thirty frames up there");
        // Fourteen frames of arc, and then it is in the tree.
        let mut frames = 0;
        for _ in 0..30 {
            match ask_shared(&def, &mut b, &mut shared, &me, &foe, Some(tree)) {
                Act::Fly { x, y, .. } => {
                    me.x = x;
                    me.y = y;
                    frames += 1;
                }
                _ => break,
            }
        }
        assert_eq!(frames, 14, "0x3205: the arc is fourteen frames long");
        assert!(b.flags & flag::IN_TREE != 0, "0x32b4: +0x48 |= 8");
        assert!(
            (me.x - tree.x).abs() <= 2,
            "and it is in the tree at {}",
            me.x
        );
        assert!(b.height < -40, "and up it: {}", b.height);
        // In the tree it hovers, and it hangs there for the rest of its count.
        let hover = ask_shared(&def, &mut b, &mut shared, &me, &foe, Some(tree));
        assert!(
            matches!(hover, Act::Play(ref n) if n.starts_with("Ratman_Hover")),
            "{hover:?}"
        );
        // A second rat finds the tree taken and aims at the knight instead.
        let mut second = Brain::default();
        let out = ask_shared(&def, &mut second, &mut shared, &at(0, 50), &foe, Some(tree));
        assert_eq!(out, Act::Play("Ratman_Leap".into()));
        assert_eq!(
            second.flags & flag::TREE_BOUND,
            0,
            "0x31bc: the tree is taken"
        );
    }

    /// `RatmanOnHead` (0x3353), `RatmanGouge` (0x3383) and `RatmanGouged`
    /// (0x3395): it sits, it gouges, and it throws itself clear leaving five
    /// points off him.
    #[test]
    fn a_ratman_on_the_head_sits_gouges_and_throws_itself_clear() {
        let def = creature("ratman", 40, 30);
        let mut shared = Shared::default();
        let mut b = Brain::default();
        b.flags |= flag::ON_HEAD;
        b.cooldown = 3;
        let mut me = at(100, 50);
        let mut foe = at(100, 50);
        foe.holder = Some(0);
        for left in [2, 1] {
            let sit = ask_shared(&def, &mut b, &mut shared, &me, &foe, None);
            assert_eq!(sit, grip("Ratman_SitOnHead", 0, true));
            assert_eq!(b.cooldown, left);
        }
        let gouge = ask_shared(&def, &mut b, &mut shared, &me, &foe, None);
        assert_eq!(gouge, grip("Ratman_EyeGouge", 0, true), "0x3383");
        assert!(b.flags & flag::GOUGING != 0 && b.flags & flag::ON_HEAD == 0);
        let done = ask_shared(&def, &mut b, &mut shared, &me, &foe, None);
        assert_eq!(done, grip("Ratman_Leaps", 5, false), "0x3395: five points");
        assert_eq!(b.height, -0x46, "0x33f6: mov word ptr [di + 4], 0xffba");
        assert!(b.flags & flag::LEAPING != 0, "0x33ee: +0x48 |= 1");
        // And a hundred and fifty pixels of arc away from him.
        for _ in 0..24 {
            if let Act::Fly { x, y, .. } = ask_shared(&def, &mut b, &mut shared, &me, &foe, None) {
                me.x = x;
                me.y = y;
            } else {
                break;
            }
        }
        assert!(
            me.x >= 200,
            "0x33c4: a hundred and fifty clear, at {}",
            me.x
        );
    }

    fn grip(script: &str, damage: i32, hold: bool) -> Act {
        Act::Grip {
            script: script.into(),
            damage,
            cost: 0,
            fatal: false,
            hold,
            victim: String::new(),
        }
    }

    /// `MudmenReach` between seventy five and a hundred, `MudmenIBury` inside
    /// that, and `MudmenAppear` seventy five pixels to one side of him.
    #[test]
    fn the_mudman_reaches_then_goes_under_the_ground() {
        let def = creature("mudman", 80, 75);
        let mut b = Brain::default();
        assert_eq!(
            kind(&ask(&def, &mut b, 0, 80, 0)),
            Some(Attack::Swing),
            "the arm at eighty"
        );
        let mut b = Brain::default();
        assert_eq!(
            ask(&def, &mut b, 0, 60, 0),
            Act::Play("Mudmen_IBury".into())
        );
        assert!(b.flags & flag::BURIED != 0);
        // It stays under until the timer runs out, then comes up beside him.
        let mut act = ask(&def, &mut b, 0, 60, 0);
        for _ in 0..12 {
            if matches!(act, Act::Appear { .. }) {
                break;
            }
            act = ask(&def, &mut b, 0, 60, 0);
        }
        match act {
            Act::Appear { x, facing, .. } => {
                assert_eq!(x, 135, "seventy five to the far side of him");
                assert_eq!(facing, -1);
            }
            other => panic!("the mudman never surfaced: {other:?}"),
        }
    }

    /// `ControlBalok` hangs back between a hundred and twenty and a hundred
    /// and eighty, and closes the moment a dagger comes out.
    #[test]
    fn balok_stands_off_unless_you_throw_daggers() {
        let def = creature("balok", 80, 60);
        let mut b = Brain::default();
        assert_eq!(
            ask(&def, &mut b, 0, 150, 0),
            Act::Idle,
            "it waits at a hundred and fifty"
        );
        // The same position, with ten on his belt.
        let mut me = at(0, 50);
        let mut foe = at(150, 50);
        foe.record.set(crate::taskvm::field::DAGGERS, 10);
        me.brain = Brain::default();
        let s = Sight {
            me: &me,
            foe: &foe,
            def: &def,
            gore: true,
            body: false,
            decapped: false,
            progression: 0,
            perch: None,
            foe_blow: 0,
            head_health: None,
        };
        let mut seed = 1u16;
        let mut brain = Brain::default();
        let mut facing = -1;
        let mut shared = Shared::default();
        // `BalokJump` (0x366f), which used to be a walk and is the original's
        // own arc: `CalcJUMP` aims it eighty short of him, `ADDJUMP` starts it
        // and `BalokFLAGS` bit 1 goes up.
        assert_eq!(
            decide(
                &s,
                &mut brain,
                &mut seed,
                &mut facing,
                &mut shared,
                &mut (0, 0)
            ),
            Act::Play("Balok_Jump".into()),
            "a thrown dagger brings it in"
        );
        assert_eq!(facing, 1, "ControlBalok+107: me.x < foe.x writes 1 into +8");
        assert_eq!(shared.balok & balok_flag::JUMPING, balok_flag::JUMPING);
        assert!(
            brain.jump.is_some(),
            "and a slot in the jump table is running"
        );
        assert!(
            shared.balok_hop > 0 && shared.balok_hop <= 0x14,
            "0x369d: twenty frames at the most, {}",
            shared.balok_hop
        );
        // Every frame after it is `BalokJumping` moving Balok itself, and it
        // comes down eighty short of him with five frames before the next hop.
        let mut walked = me.clone();
        let mut highest = 0;
        for _ in 0..0x18 {
            let s = Sight {
                me: &walked,
                foe: &foe,
                def: &def,
                gore: true,
                body: false,
                decapped: false,
                progression: 0,
                perch: None,
                foe_blow: 0,
                head_health: None,
            };
            match decide(
                &s,
                &mut brain,
                &mut seed,
                &mut facing,
                &mut shared,
                &mut (0, 0),
            ) {
                Act::Fly { x, y, script } => {
                    walked.x = x;
                    walked.y = y;
                    highest = highest.min(brain.height);
                    if script == "Balok_Jump" {
                        break;
                    }
                }
                other => panic!("expected a step of the arc, got {other:?}"),
            }
        }
        assert!(highest < 0, "it left the ground: {highest}");
        assert_eq!(brain.height, 0, "0x3734: mov word ptr [si + 4], 0");
        assert_eq!(brain.timer, 5, "0x372a: mov byte ptr [si + 0x49], 5");
        assert_eq!(shared.balok & balok_flag::JUMPING, 0);
        assert!(walked.x > 40, "and it covered ground: {}", walked.x);
        // The uppercut at arm's length, the grab from further out.
        let mut b = Brain::default();
        assert_eq!(kind(&ask(&def, &mut b, 0, 75, 0)), Some(Attack::Swing));
        let mut b = Brain::default();
        assert_eq!(kind(&ask(&def, &mut b, 0, 100, 0)), Some(Attack::Chop));
    }

    /// `BeastCharge` and `SetBeastTimer`: the beast never tracks. It runs to
    /// the edge, turns, and waits five to twenty frames before coming back.
    #[test]
    fn the_beast_charges_turns_and_waits() {
        let def = creature("beast", 2, 1);
        let mut b = Brain::default();
        let mut facing = 1;
        assert!(
            matches!(
                ask_facing(&def, &mut b, 100, 40, 0, &mut facing),
                Act::Walk { dx: 1, .. }
            ),
            "it charges away from him as readily as at him"
        );
        assert_eq!(
            facing, 1,
            "and `BeastCharge` leaves +8 alone short of the edge"
        );
        // Short of 0x154 it does not turn, even past the right-hand edge of
        // the screen: the literal is 340, not 319, and it was only ever 319
        // here because a border gate of ours held the beast inside the arena.
        let mut b = Brain::default();
        let mut facing = 1;
        assert!(matches!(
            ask_facing(&def, &mut b, 330, 40, 0, &mut facing),
            Act::Walk { dx: 1, .. }
        ));
        assert_eq!(facing, 1, "still charging at 330");
        // At 0x154 it turns, is set down at 0x17c, and waits: `BeastCharge`
        // 0x2fec writes the column at 0x2ff3 and `+8` at 0x2ff8.
        let mut b = Brain::default();
        let mut facing = 1;
        let (act, at) = ask_at(&def, &mut b, 0x154, 40, 0, &mut facing);
        assert_eq!(act, Act::Idle);
        assert_eq!(at.0, 0x17c, "02ff3: set down at 380, off the screen");
        assert_eq!(facing, -1, "02ff8: it turned round, +8 is 3");
        // **It stands for exactly one frame, not for the five to fifteen the
        // roll looks like.** `SetBeastTimer` (0x303e) writes `rnd & 0xf | 5`
        // into `+0xb` and immediately does `sub byte ptr [di+0xb], 1; je
        // BeastMove` -- and `or ax, 5` cannot leave nought, so the decrement
        // cannot reach it and the `je` is dead. Nothing else in the image
        // touches a beast's `+0xb`, so the number is written, stepped once,
        // and never read. The next frame it is past the edge and charging.
        assert!(matches!(
            ask_facing(&def, &mut b, 0x17c, 40, 0, &mut facing),
            Act::Walk { dx: -1, .. }
        ));
        assert_eq!(facing, -1, "and still facing the way it turned");
        // And the left edge is 0, where it is set down at -50: 0x2ffe.
        let mut b = Brain::default();
        let mut facing = -1;
        let (act, at) = ask_at(&def, &mut b, -1, 40, 0, &mut facing);
        assert_eq!(act, Act::Idle);
        assert_eq!(at.0, -50, "03004");
        assert_eq!(facing, 1, "03009");
    }

    /// `DemonAttack`: the slap inside a hundred, the zap out to a hundred and
    /// thirty, the whip out to a hundred and forty, and the whip's own four
    /// phase follow-through afterwards.
    #[test]
    fn the_demon_arrives_then_slaps_zaps_and_whips_by_range() {
        let def = creature("demon", 95, 90);
        let mut b = Brain::default();
        b.flags |= flag::UNBORN;
        assert_eq!(
            ask(&def, &mut b, 0, 90, 0),
            Act::Play("Demon_Evolve".into())
        );
        assert_eq!(
            kind(&ask(&def, &mut b, 0, 90, 0)),
            Some(Attack::Chop),
            "the slap"
        );
        let mut b = Brain::default();
        assert_eq!(
            kind(&ask(&def, &mut b, 0, 120, 0)),
            Some(Attack::Swing),
            "the zap"
        );
        let mut b = Brain::default();
        assert_eq!(
            kind(&ask(&def, &mut b, 0, 138, 0)),
            Some(Attack::Lunge),
            "the whip"
        );
        assert_eq!(b.phase, 1, "and the whip starts its follow-through");
        b.cooldown = 0;
        assert_eq!(
            ask(&def, &mut b, 0, 130, 0),
            Act::Play("Demon_OWhipKnight".into())
        );
        assert!(b.flags & flag::CAUGHT != 0);
    }

    /// `DemonOWhipFollow` (0x50de) and `DemonUWhipFollow` (0x5119): once the
    /// whip has him, the follow-through hands him `Knight_SwSlapped`
    /// outright, writes the direction again off the demon's own `+8`, and
    /// lets him go.
    ///
    /// It is that script, and only that script, that gosubs `InitSLAP` and
    /// `KnightSLAP`, so without the hand-off the knight took the damage and
    /// stood where he was: `SLAPY` pointed at `DemonWHIP` and nothing ever
    /// read it. Its four words are -7, -3, -1, 0, which is a drag towards
    /// the demon rather than a throw away from it.
    #[test]
    fn the_whips_follow_through_hands_the_knight_the_thrown_script() {
        let def = creature("demon", 95, 90);
        let mut b = Brain::default();
        let mut shared = Shared::default();
        let mut facing = 1;
        let me = at(0, 50);
        let far = at(138, 50);
        let near = at(130, 50);
        // Out at the whip's range, which starts the chain.
        assert_eq!(
            kind(&ask_shared(&def, &mut b, &mut shared, &me, &far, None)),
            Some(Attack::Lunge)
        );
        assert_eq!(shared.slap_y, slap_y::DEMON_WHIP, "050a4");
        // The first follow catches him.
        b.cooldown = 0;
        assert_eq!(
            ask_shared(&def, &mut b, &mut shared, &me, &near, None),
            Act::Play("Demon_OWhipKnight".into())
        );
        assert!(b.flags & flag::CAUGHT != 0);
        // And the next one is the hand-off.
        b.cooldown = 0;
        let act = ask_facing(&def, &mut b, 0, 130, 0, &mut facing);
        match act {
            Act::Strike { victim, .. } => assert_eq!(
                victim.as_deref(),
                Some("Knight_SwSlapped"),
                "050de: mov si, 0x1596; call REPLACEANIM",
            ),
            other => panic!("the follow-through should strike, not {other:?}"),
        }
        assert_eq!(
            b.flags & flag::CAUGHT,
            0,
            "050e5: and word [DemonFLAGS], 0xffbf"
        );
    }

    /// The dragon's definition as the pack carries it: the two `DragonWal`
    /// rows and the ranges `SetUpDragonTables` (0x2556) writes.
    fn dragon_def() -> ActorDef {
        let mut def = creature("dragon", 60, 20);
        for (row, names) in [
            (
                "lift",
                vec![
                    "Dragon_LiftHead1",
                    "Dragon_LiftHead2",
                    "Dragon_LiftHead3",
                    "Dragon_LiftHead4",
                    "Dragon_LiftHead5",
                    "Dragon_LiftHead5",
                    "Dragon_LiftHead5",
                    "Dragon_LiftHead5",
                ],
            ),
            (
                "lower",
                vec![
                    "Dragon_LowerHead1",
                    "Dragon_LowerHead2",
                    "Dragon_LowerHead3",
                    "Dragon_LowerHead4",
                    "Dragon_LowerHead5",
                    "Dragon_LowerHead5",
                    "Dragon_LowerHead5",
                    "Dragon_LowerHead5",
                ],
            ),
        ] {
            def.scripts
                .insert(row.into(), names.into_iter().map(String::from).collect());
        }
        def
    }

    /// One pass of `ControlDragon` with the record's `+2`, `+6` and `+4`
    /// handed in and back, as the routine reads and writes them.
    fn ask_dragon(
        def: &ActorDef,
        brain: &mut Brain,
        shared: &mut Shared,
        at: &mut (i32, i32),
        foe: (i32, i32),
    ) -> Act {
        let me = at_height(at.0, at.1, brain.height);
        let foe = super::tests::at(foe.0, foe.1);
        let s = Sight {
            me: &me,
            foe: &foe,
            def,
            gore: true,
            body: false,
            decapped: false,
            progression: 0,
            perch: None,
            foe_blow: 0,
            head_health: Some(200),
        };
        let mut seed = 0x2f1du16;
        let mut facing = 1;
        decide(&s, brain, &mut seed, &mut facing, shared, at)
    }

    fn at_height(x: i32, y: i32, height: i32) -> Fighter {
        let mut f = at(x, y);
        f.brain.height = height;
        f
    }

    /// `DragonMove` (0x386c): inside a hundred and forty the head comes up
    /// on a thirteen frame arc to x 100 and a height of minus seventy, and
    /// outside it goes back down on a nine frame arc to minus thirty, each
    /// walking its `DragonWal` row and following him in depth five rows at
    /// a time. `DragonHeadMove` clears bit 0x10 on the pass the count runs
    /// out, which is the pass the arc lands.
    #[test]
    fn the_dragon_lifts_and_lowers_its_head_on_the_jump_engine() {
        use dragon_flag::*;
        let def = dragon_def();
        let mut b = Brain {
            height: -40,
            ..Brain::default()
        };
        let mut sh = Shared::default();
        // `InitKnightvsDragon` (0x2476): x 80, height -40, z 100.
        let mut at = (80, 100);
        // 03880  cmp ax, 0x8c; jl: inside a hundred and forty, head down.
        let act = ask_dragon(&def, &mut b, &mut sh, &mut at, (200, 110));
        assert_eq!(
            act,
            Act::Stand("Dragon_LiftHead1".into()),
            "0x38a4: DragonWal+0x10"
        );
        assert_eq!(
            sh.dragon & (HEAD_MOVING | HEAD_UP),
            HEAD_MOVING | HEAD_UP,
            "0x3890, 0x3895"
        );
        assert_eq!(b.cooldown, 13, "0x38b2: mov word [di+0x4a], 0xd");
        assert!(b.jump.is_some(), "0x38f9: call ADDJUMP");
        // `TrackKnight` ran first (0x3877): he is to the right and further
        // out than the approach range, so the head stepped five towards him.
        assert_eq!(at.0, 85, "0x3c36: five to the right");
        // `DragonHeadMove` thirteen times: the row is walked, the arc is
        // stepped, and the head follows him down in depth.
        let mut frames = 0;
        let mut seen: Vec<String> = Vec::new();
        while sh.dragon & HEAD_MOVING != 0 {
            let act = ask_dragon(&def, &mut b, &mut sh, &mut at, (200, 110));
            match act {
                Act::Stand(name) => seen.push(name),
                other => panic!("a head move is a stance frame, not {other:?}"),
            }
            frames += 1;
            assert!(frames < 40, "the head never stopped");
        }
        assert_eq!(
            frames, 13,
            "0x3979: one off +0x4a a frame until it is nought"
        );
        assert_eq!(
            seen[0], "Dragon_LiftHead2",
            "0x39a8: the walk byte is one on the first pass"
        );
        assert_eq!(seen[3], "Dragon_LiftHead5");
        assert_eq!(seen[12], "Dragon_LiftHead5", "0x39b5: capped at seven");
        assert!(
            b.jump.is_none(),
            "the arc landed on the pass the count ran out"
        );
        // `ADDJUMP` divides the run into 10.6 steps and `ControlJump` shifts
        // them back, so the arc lands a pixel short of the hundred it was
        // aimed at: fifteen pixels over thirteen frames is 73/64 a frame,
        // thirteen of which is 949/64, and 85 + 949/64 is 99.
        assert_eq!(
            at.0, 99,
            "0x38d4: x1 is a hundred, less the arc's own rounding"
        );
        assert!(
            (b.height + 70).abs() <= 1,
            "0x38e3: y1 is minus seventy: {}",
            b.height
        );
        // 03990 / 03996: five rows toward him each pass, and no further
        // than the arc's own frames carry it.
        assert_eq!(
            at.1, 110,
            "he was ten deeper and it followed, then stayed put"
        );
        // Head up and the knight backing out past a hundred and forty:
        // `DragonMoveLow` (0x390a) lowers it in nine.
        let act = ask_dragon(&def, &mut b, &mut sh, &mut at, (260, 110));
        assert_eq!(
            act,
            Act::Stand("Dragon_LowerHead1".into()),
            "0x391b: DragonWal+0x20"
        );
        assert_eq!(
            sh.dragon & HEAD_UP,
            0,
            "0x390f: and word [DragonFLAGS], 0xffdf"
        );
        assert_eq!(b.cooldown, 9, "0x392c");
        let mut frames = 0;
        while sh.dragon & HEAD_MOVING != 0 {
            ask_dragon(&def, &mut b, &mut sh, &mut at, (260, 110));
            frames += 1;
        }
        assert_eq!(frames, 9);
        assert!(
            (b.height + 30).abs() <= 1,
            "0x395d: y1 is minus thirty: {}",
            b.height
        );
    }

    /// `DragonAttack` (0x39da): head up, past seventy or once struck it is
    /// the high breath with the fire beside it, two frames of nothing between
    /// (`dragonbodge1`); inside seventy it is the bite; and with the head
    /// down `DragonLowAttack` is the low breath, with its own two frames of
    /// nothing (`dragonbodge2`) and no fire task, because that script draws
    /// its own.
    #[test]
    fn the_dragon_bites_close_and_breathes_far_with_two_frames_between() {
        use dragon_flag::*;
        let def = dragon_def();
        let mut b = Brain {
            height: -70,
            ..Brain::default()
        };
        let mut sh = Shared {
            dragon: HEAD_UP,
            ..Shared::default()
        };
        // On the head's own plane, 0x39ec, and inside seventy of x 100.
        let mut at = (100, 100);
        let bite = ask_dragon(&def, &mut b, &mut sh, &mut at, (160, 100));
        assert_eq!(
            bite,
            Act::Attack {
                kind: Attack::Lunge,
                spawn: None
            },
            "0x3a43: kind 2, Dragon_HighBite"
        );
        assert_eq!(sh.ddis, 60, "0x387d: DDIS is what FindDistance answered");
        // Past seventy: the breath, the fire, the bit, and then two passes
        // of the stance before the next.
        let mut at = (100, 100);
        let breath = ask_dragon(&def, &mut b, &mut sh, &mut at, (190, 100));
        assert_eq!(
            breath,
            Act::Attack {
                kind: Attack::Chop,
                spawn: Some("Dragon_Fire".into())
            },
            "0x3a24: Dragon_HighBreath and AddDragonFIRE"
        );
        assert_eq!(sh.dragon & BREATHING, BREATHING, "0x3a2a");
        assert_eq!(sh.dragon_bodge[0], 2, "0x3a1e");
        for _ in 0..2 {
            let mut at = (100, 100);
            assert_eq!(
                ask_dragon(&def, &mut b, &mut sh, &mut at, (190, 100)),
                Act::Stand("Dragon_HighStance".into()),
                "0x3a17: dec and the stance"
            );
        }
        assert_eq!(sh.dragon_bodge[0], 0);
        let mut at = (100, 100);
        assert!(matches!(
            ask_dragon(&def, &mut b, &mut sh, &mut at, (190, 100)),
            Act::Attack {
                kind: Attack::Chop,
                ..
            }
        ));
        // Struck: the breath even inside seventy, and the bit comes down.
        sh.dragon |= STRUCK;
        sh.dragon_bodge[0] = 0;
        let mut at = (100, 100);
        assert!(matches!(
            ask_dragon(&def, &mut b, &mut sh, &mut at, (160, 100)),
            Act::Attack {
                kind: Attack::Chop,
                ..
            }
        ));
        assert_eq!(
            sh.dragon & STRUCK,
            0,
            "0x3a05: and word [DragonFLAGS], 0xff7f"
        );
        // Off his plane, or with him down: the stance and nothing else.
        let mut at = (100, 100);
        assert_eq!(
            ask_dragon(&def, &mut b, &mut sh, &mut at, (160, 130)),
            Act::Stand("Dragon_HighStance".into()),
            "0x39ec: ZPLANE"
        );
        // Head down, outside a hundred and forty: the low breath, with its
        // own wait and no fire task.
        let mut sh = Shared::default();
        let mut b = Brain {
            height: -30,
            ..Brain::default()
        };
        let mut at = (100, 100);
        assert_eq!(
            ask_dragon(&def, &mut b, &mut sh, &mut at, (250, 100)),
            Act::Attack {
                kind: Attack::Swing,
                spawn: None
            },
            "0x3a6a: Dragon_LowBreath"
        );
        assert_eq!(sh.dragon_bodge[1], 2, "0x3a5f");
        let mut at = (100, 100);
        assert_eq!(
            ask_dragon(&def, &mut b, &mut sh, &mut at, (250, 100)),
            Act::Stand("Dragon_Stance".into())
        );
    }

    /// `TrackKnight` (0x3be8): five pixels at a time, never to a hundred or
    /// down to thirty, five rows in depth, always facing right; and while
    /// the breath bit is up it tracks to two and one and puts the ranges
    /// back with `+0x54` taking what `+0x52` held (0x3c08, 0x3c84).
    #[test]
    fn the_head_keeps_to_its_corridor_and_the_breath_spoils_its_back_off() {
        let def = dragon_def();
        let foe = at(300, 100);
        let mut sh = Shared::default();
        let mut facing = -1;
        // At ninety five the next step would reach a hundred: refused.
        let mut at_ = (95, 100);
        track_knight(&foe, &def, &mut sh, &mut facing, &mut at_);
        assert_eq!(at_.0, 95, "0x3c31: cmp bx, 0x64; jge");
        assert_eq!(facing, 1, "0x3c1f: mov byte [si+8], 1");
        let mut at_ = (90, 100);
        track_knight(&foe, &def, &mut sh, &mut facing, &mut at_);
        assert_eq!(at_.0, 95);
        // Inside the back-off range it gives ground, but never to thirty.
        let near = at(50, 100);
        let mut at_ = (35, 100);
        track_knight(&near, &def, &mut sh, &mut facing, &mut at_);
        assert_eq!(at_.0, 35, "0x3c44: cmp bx, 0x1e; jle");
        let mut at_ = (40, 100);
        track_knight(&near, &def, &mut sh, &mut facing, &mut at_);
        assert_eq!(at_.0, 35);
        // Depth: five rows toward him when off his plane.
        let deep = at(300, 120);
        let mut at_ = (95, 100);
        track_knight(&deep, &def, &mut sh, &mut facing, &mut at_);
        assert_eq!(at_.1, 105, "0x3c5a: add word [si+6], 5");
        assert_eq!(
            sh.dragon_ranges, None,
            "not breathing: the ranges untouched"
        );
        // Breathing: tracked to two, and the back-off spoiled on the way out.
        sh.dragon |= dragon_flag::BREATHING;
        let close = at(97, 100);
        let mut at_ = (90, 100);
        track_knight(&close, &def, &mut sh, &mut facing, &mut at_);
        assert_eq!(at_.0, 95, "outside two: closes");
        assert_eq!(sh.dragon_ranges, Some((60, 60)), "0x3c84: DCL was +0x52");
        // And from then on, breath or not, the back-off is sixty: inside it
        // the head backs away from him.
        sh.dragon &= !dragon_flag::BREATHING;
        let mut at_ = (95, 100);
        let t = track_knight(&at(140, 100), &def, &mut sh, &mut facing, &mut at_);
        assert_eq!(t.dx, -1, "TrackBack inside a back-off of sixty");
        assert_eq!(at_.0, 90);
    }

    /// `TalismanWrym` (0x43f4): the blow halved once per talisman, floored
    /// at five, and a shift count past the register's width is nought.
    #[test]
    fn the_talisman_halves_the_dragons_blow_down_to_five() {
        assert_eq!(talisman_wrym(30, 0), 30);
        assert_eq!(talisman_wrym(30, 1), 15);
        assert_eq!(talisman_wrym(30, 2), 7);
        assert_eq!(talisman_wrym(30, 3), 5, "04403: mov ax, 5");
        assert_eq!(talisman_wrym(20, 1), 10);
        assert_eq!(talisman_wrym(10, 1), 5);
        assert_eq!(talisman_wrym(10, 20), 5);
    }

    /// `ControlClaw` (0x3b24): it slaps whatever comes inside a hundred on
    /// its plane while the head lives, holds `Dragon_ClawDead` once the head
    /// is down, and kills its own task when `DrDropClaws` has run.
    #[test]
    fn a_claw_slaps_inside_a_hundred_and_dies_with_the_head() {
        let def = creature("claw", 0, 0);
        let me = at(5, 90);
        let ask = |foe: &Fighter, shared: &Shared, head: Option<i32>| {
            let s = Sight {
                me: &me,
                foe,
                def: &def,
                gore: true,
                body: false,
                decapped: false,
                progression: 0,
                perch: None,
                foe_blow: 0,
                head_health: head,
            };
            let mut seed = 0x2f1du16;
            let mut facing = 1;
            let mut sh = *shared;
            let mut b = Brain::default();
            decide(&s, &mut b, &mut seed, &mut facing, &mut sh, &mut (5, 90))
        };
        let sh = Shared::default();
        assert_eq!(
            kind(&ask(&at(90, 90), &sh, Some(200))),
            Some(Attack::RThrust),
            "0x3b7d: kind 0xa, Dragon_ClawSlap"
        );
        assert_eq!(
            ask(&at(150, 90), &sh, Some(200)),
            Act::Idle,
            "0x3b77: past a hundred"
        );
        assert_eq!(
            ask(&at(90, 130), &sh, Some(200)),
            Act::Idle,
            "0x3b70: off its plane"
        );
        let mut down = at(90, 90);
        down.health = 0;
        assert_eq!(ask(&down, &sh, Some(200)), Act::Idle, "0x3b6a: he is down");
        assert_eq!(
            ask(&at(90, 90), &sh, Some(0)),
            Act::Stand("Dragon_ClawDead".into()),
            "0x3b58: the head is down"
        );
        let dropped = Shared {
            dead_claws: -1,
            ..Shared::default()
        };
        assert_eq!(
            ask(&at(90, 90), &dropped, Some(0)),
            Act::Vanish,
            "0x3b61: CLAWS_DEAD"
        );
    }

    #[test]
    fn the_roll_is_the_originals_and_is_deterministic() {
        // Two runs from one seed agree, and the register moves.
        let mut a = 0x1234u16;
        let mut b = 0x1234u16;
        let one: Vec<i32> = (0..8).map(|_| percent(&mut a)).collect();
        let two: Vec<i32> = (0..8).map(|_| percent(&mut b)).collect();
        assert_eq!(one, two);
        assert!(one.iter().all(|v| (0..100).contains(v)), "{one:?}");
        assert!(
            one.windows(2).any(|w| w[0] != w[1]),
            "the register is stuck: {one:?}"
        );
    }
}
