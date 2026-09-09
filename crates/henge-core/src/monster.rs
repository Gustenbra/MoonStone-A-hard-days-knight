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

use crate::arena::Bounds;
use crate::combat::{Attack, Fighter, State};
use crate::content::ActorDef;
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
    /// A knight, or anything with no controller of its own: close, and swing.
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
    pub flags: u32,
    pub walk: u32,
    pub phase: u8,
    /// Ticks until this controller may run again. The original's task loop
    /// calls a controller once per game frame at most, and this engine ticks
    /// six times for each of those, so without it every count a controller
    /// keeps would run six times too fast.
    pub rest: i32,
}

/// `DemonFLAGS`, `DragonFLAGS`, `BalokFLAGS`, `MudmenFLAGS` and `+0x48`, as
/// far as any of them is reproduced.
pub mod flag {
    /// The demon has not made its entrance yet (`Demon_Evolve`).
    pub const UNBORN: u32 = 0x0001;
    /// `DragonFLAGS & 0x20`: the head is up.
    pub const HEAD_UP: u32 = 0x0002;
    /// `DragonFLAGS & 0x10`: a head lift or lower is running.
    pub const HEAD_MOVING: u32 = 0x0004;
    /// `DragonFLAGS & 0x80`: the knight has landed a blow, so the high attack
    /// is the breath rather than the bite.
    pub const STRUCK: u32 = 0x0008;
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
    },
    /// Held, and trying to get out of it. `MudmenEntangle` reads fire and
    /// down together off the joystick and nothing else, so this is that press
    /// rather than an order.
    Struggle,
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
    if me.x < foe.x {
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
fn find_side(me: &Fighter, foe: &Fighter) -> i32 {
    if me.x < foe.x {
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
    let mut bx = foe.y - me.y;
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
fn check_x_axis(me: &Fighter, foe: &Fighter, bp: i32) -> (bool, i32) {
    let mut bx = foe.x - me.x;
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
    let mut dx = 0;
    let mut dy = 0;
    // 056eb..056f5: the globals are cleared and the two records picked up.
    // 056f9  call FaceKnight
    *facing = face_knight(me, foe);
    // 056fc  mov [TrackFLAG], 0
    let mut track_flag = false;
    let mut plane = false;
    // 05702  call CheckZAxis
    if check_z(me, foe, def) {
        // 05709  mov [ZPLANE], 1
        plane = true;
    } else if me.y > foe.y {
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
    let (inside_back_off, distance) = check_x_axis(me, foe, def.back_off);
    if inside_back_off {
        // TrackBack:
        // 0576b..05773: bx = foe.x - me.x; jns T1$
        if foe.x - me.x < 0 {
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
    let (inside_approach, distance) = check_x_axis(me, foe, def.approach);
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
    if find_side(me, foe) == 1 {
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
    pub bounds: Bounds,
    /// The title screen's gore switch, on. `TroggAttack` reads it before it
    /// comes in for a fallen knight.
    pub gore: bool,
    /// Whether the fallen knight still has a body worth one more blow.
    pub body: bool,
    /// Whether anyone has taken the finisher yet: `DeCapFLAG`.
    pub decapped: bool,
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
pub fn decide(s: &Sight, brain: &mut Brain, seed: &mut u16, facing: &mut i32) -> Act {
    match s.def.controller() {
        Controller::Trogg => trogg(s, brain, seed, false, facing),
        Controller::TroggSpear => trogg(s, brain, seed, true, facing),
        Controller::Troll => troll(s, brain, facing),
        Controller::Ratman => ratman(s, brain, facing),
        Controller::Mudman => mudman(s, brain, facing),
        Controller::Balok => balok(s, brain, facing),
        Controller::Beast => beast(s, brain, seed, facing),
        Controller::Demon => demon(s, brain, facing),
        Controller::Dragon => dragon(s, brain, facing),
        Controller::Claw => claw(s, brain),
        Controller::Knight => knight(s, brain, facing),
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

/// `ControlTroll` and `TrollAttack`: the club inside a hundred, the overhead
/// chop from further out, and never two chops running.
fn troll(s: &Sight, brain: &mut Brain, facing: &mut i32) -> Act {
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
    Act::Attack {
        kind: Attack::Swing,
        spawn: None,
    }
}

/// `ControlRatCollide`: it does not use the tracker at all. It slashes inside
/// forty, bites out to fifty, and leaps at anything further.
fn ratman(s: &Sight, brain: &mut Brain, facing: &mut i32) -> Act {
    let dy = s.foe.y - s.me.y;
    let toward = (s.foe.x - s.me.x).signum();
    let leap = |dy: i32| Act::Walk {
        dx: toward,
        dy,
        script: s.def.scripts_for("leap").first().cloned(),
    };
    // ControlRatCollide+72 (0x3144): `call FaceKnight`, once the in-tree,
    // on-head and hanging branches have been passed over.
    *facing = face_knight(s.me, s.foe);
    // 03147  call CheckZAxis; 0314c je RatmanLeap
    if !check_z(s.me, s.foe, s.def) {
        return leap(dy.signum());
    }
    // 03168  cmp word ptr [si + 0x38], 0; 0316c jg; else the exit
    if !s.foe.alive() {
        return Act::Idle;
    }
    // 03171  cmp [HitDelay], 0; 03178 sub [HitDelay], 1; then the exit.
    // `RatmanHit` sets the fifteen frame delay every time a blow of its own
    // lands, which is what keeps the slash from being a blur.
    if brain.cooldown > 0 {
        brain.cooldown -= 1;
        return Act::Idle;
    }
    // 03180  call FindDistance
    let d = find_distance(s.me, s.foe);
    // 03183  cmp ax, 0x28; 03186 jg
    if d <= 40 {
        brain.cooldown = 15;
        return Act::Attack {
            kind: Attack::Swing,
            spawn: None,
        };
    }
    if d <= 50 {
        brain.cooldown = 15;
        return Act::Attack {
            kind: Attack::Lunge,
            spawn: None,
        };
    }
    leap(0)
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
            };
        }
        if !s.foe.held() {
            // He tore free: `Mudmen_KnightSd`, and it costs the mudman a point.
            brain.flags &= !flag::ENTANGLING;
            return Act::Strike {
                script: "Mudmen_Hit".into(),
                damage: 0,
                fatal: false,
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

/// `ControlBalok`: it closes in hops, uppercuts at arm's length, grabs from
/// further out, and stands off between a hundred and twenty and a hundred and
/// eighty unless you are throwing daggers at it.
fn balok(s: &Sight, brain: &mut Brain, facing: &mut i32) -> Act {
    let dy = s.foe.y - s.me.y;
    let d = (s.foe.x - s.me.x).abs();
    let toward = (s.foe.x - s.me.x).signum();
    let hop = |dy: i32| Act::Walk {
        dx: toward,
        dy,
        script: None,
    };
    // 035e0  cmp word ptr [di + 0x38], 0; 035e4 jg; else the exit
    if !s.foe.alive() {
        return Act::Idle;
    }
    // ControlBalok+80..107: its own `FaceKnight`, against `balok_seek_x`.
    //   035e9  mov ax, [di+2]; 035ec mov [balok_seek_x], ax
    //   035f5  mov ax, [si+2]; 035f8 cmp ax, [balok_seek_x]; 035fc jl 03604
    //   035fe  mov byte ptr [si + 8], 3
    //   03604  mov byte ptr [si + 8], 1
    *facing = if s.me.x < s.foe.x { 1 } else { -1 };
    // 03608  call CheckZAxis; 0360d je BalokJump
    if !check_z(s.me, s.foe, s.def) {
        return hop(dy.signum());
    }
    if d <= 70 {
        brain.phase = 0;
        return hop(0);
    }
    if d <= 80 {
        if brain.phase == 1 {
            brain.phase = 0;
            return Act::Attack {
                kind: Attack::Chop,
                spawn: None,
            };
        }
        brain.phase = 1;
        return Act::Attack {
            kind: Attack::Swing,
            spawn: None,
        };
    }
    if d <= 120 {
        brain.phase = 0;
        return Act::Attack {
            kind: Attack::Chop,
            spawn: None,
        };
    }
    if s.foe.daggers() > 0 || d > 180 {
        return hop(0);
    }
    Act::Idle
}

/// `ControlBeast`, `BeastCharge`, `SetBEASTZ` and `SetBeastTimer`: it does not
/// track at all. It runs from one side of the arena to the other, turns round
/// off the edge, waits, picks a depth and comes back.
fn beast(s: &Sight, brain: &mut Brain, seed: &mut u16, facing: &mut i32) -> Act {
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
    // The edges here are the arena's own bounds rather than 340 and 0, and
    // the beast is not set down beyond them, because this engine's walk gate
    // keeps every fighter inside the field; see `Fighter::walk`.
    let leftward = *facing < 0;
    let (l, r) = (s.bounds.left, s.bounds.right);
    let turning = if leftward { s.me.x <= l } else { s.me.x >= r };
    if turning {
        // 02ff8 / 03009: the turn is a write to `+8`.
        *facing = if leftward { 1 } else { -1 };
        // `SetBEASTZ`: every other pass is dead on his line, and the one
        // between it is up to twenty eight rows off.
        brain.flags ^= flag::ONLINE;
        // `SetBeastTimer`: five to twenty frames off the edge.
        *seed = rnd(*seed);
        brain.timer = ((*seed & 0xf) | 5) as i32;
        brain.walk = if brain.flags & flag::ONLINE != 0 {
            0
        } else {
            *seed = rnd(*seed);
            (*seed & 7) as u32 * 4
        };
        return Act::Idle;
    }
    if brain.timer > 0 {
        brain.timer -= 1;
        return Act::Idle;
    }
    // `BeastMove`, 0x307c: `test byte ptr [di + 8], 2; je; neg bx`, so the
    // charge goes the way the record faces.
    let dir = if *facing < 0 { -1 } else { 1 };
    let want = s.foe.y + brain.walk as i32;
    let dy = (want - s.me.y).signum();
    Act::Walk {
        dx: dir,
        dy,
        script: None,
    }
}

/// `ControlDemon` and `DemonAttack`: the slap inside a hundred, the zap out to
/// a hundred and thirty, the whip out to a hundred and forty, and the whip's
/// own four phase follow-through.
fn demon(s: &Sight, brain: &mut Brain, facing: &mut i32) -> Act {
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
        if brain.phase.is_multiple_of(2) && caught {
            // The follow-through with the knight already caught: the original
            // hands him `Knight_SwSlapped` outright rather than waiting for a
            // weapon part to touch him.
            brain.flags &= !flag::CAUGHT;
            let hit = if brain.phase == 0 {
                "Demon_UWhipHit"
            } else {
                "Demon_OWhipHit"
            };
            return Act::Strike {
                script: hit.into(),
                damage: s.me.damage,
                fatal: false,
            };
        }
        if d >= low && d <= high {
            brain.flags |= flag::CAUGHT;
            let caught_script = if brain.phase == 2 {
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
        return Act::Attack {
            kind: Attack::Lunge,
            spawn: None,
        };
    }
    walk(t)
}

/// `ControlDragon`: the set piece. The head lifts when you come inside a
/// hundred and forty and lowers when you go back out, and what it does to you
/// depends on which it is doing.
fn dragon(s: &Sight, brain: &mut Brain, facing: &mut i32) -> Act {
    let d = (s.foe.x - s.me.x).abs();
    let up = brain.flags & flag::HEAD_UP != 0;
    if brain.flags & flag::HEAD_MOVING != 0 {
        // `DragonHeadMove`: the counter runs the lift or the lower, and the
        // head follows him in depth five rows a frame while it does.
        brain.cooldown -= 1;
        if brain.cooldown <= 0 {
            brain.flags &= !flag::HEAD_MOVING;
        }
        let dy = (s.foe.y - s.me.y).signum();
        let row = if up { "lift" } else { "lower" };
        let n = s.def.scripts_for(row).len();
        let frame = (brain.walk as usize).min(n.saturating_sub(1));
        brain.walk += 1;
        return Act::Walk {
            dx: 0,
            dy,
            script: s.def.scripts_for(row).get(frame).cloned(),
        };
    }
    // DragonMove+0xb (0x3877): `call TrackKnight`, before the range at
    // 0x3880 is looked at, so the head has faced right (`TrackKnight+0x37`,
    // 0x3c1f: `mov byte ptr [si + 8], 1`, whatever `FaceKnight` wrote) on
    // every frame a head move is not already running.
    let t = track(s.me, s.foe, s.def, facing);
    *facing = 1;
    if !s.foe.alive() && brain.flags & flag::HEAD_MOVING == 0 {
        return Act::Stand(if up {
            "Dragon_HighStance".into()
        } else {
            "Dragon_Stance".into()
        });
    }
    // 0387a  call FindDistance; 03880 cmp ax, 0x8c; 03883 jge DragonMoveLow
    if d >= 140 && up {
        brain.flags &= !flag::HEAD_UP;
        brain.flags |= flag::HEAD_MOVING;
        brain.cooldown = 9;
        brain.walk = 0;
        return Act::Idle;
    }
    if d < 140 && !up {
        brain.flags |= flag::HEAD_UP | flag::HEAD_MOVING;
        brain.cooldown = 13;
        brain.walk = 0;
        return Act::Idle;
    }
    let stance = || {
        Act::Stand(if up {
            "Dragon_HighStance".into()
        } else {
            "Dragon_Stance".into()
        })
    };
    if !s.foe.alive() {
        return stance();
    }
    // `TrackKnight`: the head shifts five pixels at a time inside thirty to a
    // hundred, and follows him in depth. `MonsterTrack` does the choosing; the
    // clamp is the original's own, and it is why the dragon never leaves its
    // corner of the arena.
    // `TrackKnight`, 0x3be8: `MonsterTrack` on the head's record (0x3c18),
    // and then, whatever `FaceKnight` wrote, `mov byte ptr [si + 8], 1` at
    // 0x3c1f. The head always faces right. `t` is what it answered above.
    let dx = if s.me.x + t.dx * 5 < 30 || s.me.x + t.dx * 5 > 100 {
        0
    } else {
        t.dx
    };
    if t.dy != 0 || dx != 0 {
        return Act::Walk {
            dx,
            dy: t.dy,
            script: None,
        };
    }
    if cooling(brain) {
        return stance();
    }
    if up {
        // `DragonAttack`: past seventy the head is too far back to bite, so it
        // breathes; and once the knight has landed a blow (`DragonFLAGS` bit
        // 7) it breathes whatever the range.
        if d > 70 || brain.flags & flag::STRUCK != 0 {
            brain.flags &= !flag::STRUCK;
            brain.cooldown = 8;
            // `Dragon_HighBreath`, kind 0x10, with `AddDragonFIRE` beside it.
            return Act::Attack {
                kind: Attack::Chop,
                spawn: Some("Dragon_Fire".into()),
            };
        }
        brain.cooldown = 6;
        return Act::Attack {
            kind: Attack::Lunge,
            spawn: None,
        };
    }
    brain.cooldown = 8;
    // `Dragon_LowBreath`, kind 4.
    Act::Attack {
        kind: Attack::Swing,
        spawn: Some("Dragon_Fire".into()),
    }
}

/// `ControlClaw`: it never moves and never takes a blow. It slaps whatever
/// comes inside a hundred on its own plane, and it dies when the dragon does.
fn claw(s: &Sight, _brain: &mut Brain) -> Act {
    if !s.foe.alive() {
        return Act::Idle;
    }
    if !s.me.alive() {
        return Act::Idle;
    }
    if (s.foe.y - s.me.y).abs() > s.def.depth_tolerance {
        return Act::Idle;
    }
    if s.foe.x > 100 {
        return Act::Idle;
    }
    Act::Attack {
        kind: Attack::RThrust,
        spawn: None,
    }
}

/// A knight, or anything with no controller of its own. Deliberately plain:
/// it closes and swings, and it struggles out of a hold the way a person
/// would, which is fire and down together (`MudmenEntangle` reads exactly
/// those two bits).
fn knight(s: &Sight, brain: &mut Brain, facing: &mut i32) -> Act {
    if s.me.held() {
        return Act::Struggle;
    }
    // ControlBlackKnight+69 (0x4bbe): `call MonsterTrack`.
    let t = track(s.me, s.foe, s.def, facing);
    if !s.foe.alive() {
        if !s.gore || !s.body || s.decapped || t.distance > 100 {
            return Act::Idle;
        }
        if cooling(brain) {
            return Act::Idle;
        }
        brain.cooldown = 20;
        return Act::Attack {
            kind: Attack::Swing,
            spawn: None,
        };
    }
    if t.walking {
        return walk(t);
    }
    if cooling(brain) {
        return Act::Idle;
    }
    brain.cooldown = 20;
    Act::Attack {
        kind: Attack::Swing,
        spawn: None,
    }
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
            let mut b = Brain::default();
            let mut facing = 1;
            ask_facing(&def, &mut b, 200, 100, 0, &mut facing);
            assert_eq!(facing, -1, "{name} to the knight's right faces left");
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
    fn creature(controller: &str, approach: i32, back_off: i32) -> ActorDef {
        ActorDef {
            controller: controller.into(),
            approach,
            back_off,
            depth_tolerance: 5,
            speed_x: 2,
            speed_y: 1,
            ..scripted_def()
        }
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
            bounds: Bounds {
                left: 0,
                right: 319,
                top: 10,
                bottom: 114,
            },
            gore: true,
            body: false,
            decapped: false,
        };
        let mut seed = 0x2f1du16;
        decide(&s, brain, &mut seed, facing)
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
            bounds: Bounds {
                left: 0,
                right: 319,
                top: 10,
                bottom: 114,
            },
            gore: true,
            body,
            decapped: false,
        };
        let mut seed = 0x2f1du16;
        let mut facing = 1;
        assert!(matches!(
            decide(&sight(true), &mut b, &mut seed, &mut facing),
            Act::Idle
        ));
        assert!(matches!(
            decide(&sight(true), &mut b, &mut seed, &mut facing),
            Act::Idle
        ));
        assert_eq!(b.cooldown, 0);
        // And `TroggAttack` never asks whether the corpse still shows a body:
        // 0x2e86 is the distance, 0x2e8b the flag, 0x2e92 the count, and
        // then the swing. `Sight::body` is the black knight's concern.
        assert_eq!(
            kind(&decide(&sight(false), &mut b, &mut seed, &mut facing)),
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
                bounds: Bounds {
                    left: 0,
                    right: 319,
                    top: 10,
                    bottom: 114,
                },
                gore: true,
                body: false,
                decapped: false,
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
                &mut facing
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
                &mut facing
            )),
            Some(Attack::Chop)
        );
        let mut s = low;
        assert_eq!(
            kind(&decide(
                &sight(&me, &open, &def),
                &mut Brain::default(),
                &mut s,
                &mut facing
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
        let mut b = Brain::default();
        assert!(
            matches!(ask(&def, &mut b, 0, 60, 0), Act::Walk { .. }),
            "the leap"
        );
        // A landed blow buys fifteen frames: `RatmanHit` sets `HitDelay`.
        let mut b = Brain::default();
        ask(&def, &mut b, 0, 35, 0);
        assert_eq!(b.cooldown, 15);
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
            bounds: Bounds {
                left: 0,
                right: 319,
                top: 10,
                bottom: 114,
            },
            gore: true,
            body: false,
            decapped: false,
        };
        let mut seed = 1u16;
        let mut brain = Brain::default();
        let mut facing = -1;
        assert!(
            matches!(
                decide(&s, &mut brain, &mut seed, &mut facing),
                Act::Walk { dx: 1, .. }
            ),
            "a thrown dagger brings it in"
        );
        assert_eq!(facing, 1, "ControlBalok+107: me.x < foe.x writes 1 into +8");
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
        // At the right edge it turns and waits: `BeastCharge+18` writes 3
        // into `+8`, and nothing else about it changes.
        let mut b = Brain::default();
        let mut facing = 1;
        assert_eq!(ask_facing(&def, &mut b, 319, 40, 0, &mut facing), Act::Idle);
        assert_eq!(facing, -1, "it turned round: +8 is 3");
        assert!((5..=20).contains(&b.timer), "and waits {} frames", b.timer);
        let before = b.timer;
        assert_eq!(ask_facing(&def, &mut b, 319, 40, 0, &mut facing), Act::Idle);
        assert_eq!(b.timer, before - 1);
        assert_eq!(facing, -1, "and keeps facing left while it waits");
        // Off the edge and facing left, it charges left: `BeastMove+41`.
        b.timer = 0;
        assert!(matches!(
            ask_facing(&def, &mut b, 319, 40, 0, &mut facing),
            Act::Walk { dx: -1, .. }
        ));
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

    /// `ControlDragon`: the head lifts inside a hundred and forty and lowers
    /// again outside it, and what it does to you depends on which it is.
    #[test]
    fn the_dragon_lifts_and_lowers_its_head_by_range() {
        let def = creature("dragon", 60, 20);
        // The head sits at the far end of its own corridor, which is where
        // `TrackKnight` clamps it: thirty to a hundred and no further.
        let mut b = Brain::default();
        assert_eq!(
            kind(&ask(&def, &mut b, 100, 250, 0)),
            Some(Attack::Swing),
            "the low breath"
        );
        // Inside a hundred and forty the head comes up, over thirteen frames.
        let mut b = Brain::default();
        assert_eq!(ask(&def, &mut b, 100, 200, 0), Act::Idle);
        assert!(b.flags & flag::HEAD_UP != 0 && b.flags & flag::HEAD_MOVING != 0);
        assert_eq!(b.cooldown, 13, "thirteen frames to lift it");
        let mut lifting = 0;
        while b.flags & flag::HEAD_MOVING != 0 {
            assert!(matches!(ask(&def, &mut b, 100, 200, 0), Act::Walk { .. }));
            lifting += 1;
            assert!(lifting < 40, "the head never stopped");
        }
        assert_eq!(lifting, 13);
        // Head up and far out: the breath. Head up and close: the bite.
        let mut up = b;
        up.cooldown = 0;
        assert_eq!(
            kind(&ask(&def, &mut up, 100, 200, 0)),
            Some(Attack::Chop),
            "the high breath"
        );
        let mut up = b;
        up.cooldown = 0;
        assert_eq!(
            kind(&ask(&def, &mut up, 100, 170, 0)),
            Some(Attack::Lunge),
            "the bite"
        );
        // Once a blow has landed on it, it breathes rather than bites.
        let mut up = b;
        up.cooldown = 0;
        up.flags |= flag::STRUCK;
        assert_eq!(kind(&ask(&def, &mut up, 100, 170, 0)), Some(Attack::Chop));
        // And backing off lowers the head again, in nine.
        let mut down = b;
        down.cooldown = 0;
        assert_eq!(ask(&def, &mut down, 100, 250, 0), Act::Idle);
        assert!(down.flags & flag::HEAD_UP == 0 && down.flags & flag::HEAD_MOVING != 0);
        assert_eq!(down.cooldown, 9, "nine frames to lower it");
    }

    /// `ControlClaw`: it slaps whatever comes inside a hundred on its plane
    /// and does nothing else at all.
    #[test]
    fn a_claw_slaps_only_what_comes_inside_a_hundred() {
        let def = creature("claw", 0, 0);
        let mut b = Brain::default();
        assert_eq!(kind(&ask(&def, &mut b, 5, 90, 0)), Some(Attack::RThrust));
        assert_eq!(
            ask(&def, &mut b, 5, 150, 0),
            Act::Idle,
            "and nothing beyond it"
        );
        assert_eq!(ask(&def, &mut b, 5, 90, 40), Act::Idle, "nor off its plane");
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
