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
    /// The beast is facing left. `+8` in the original; kept here because the
    /// beast turns round on the arena edge rather than on the knight.
    pub const LEFTWARD: u32 = 0x0080;
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
    Walk { dx: i32, dy: i32, script: Option<String> },
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
    Strike { script: String, damage: i32, fatal: bool },
    /// Held, and trying to get out of it. `MudmenEntangle` reads fire and
    /// down together off the joystick and nothing else, so this is that press
    /// rather than an order.
    Struggle,
}

// --------------------------------------------------------------- the tracker

/// What `MonsterTrack` answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Track {
    /// Which way the creature wants to walk, if at all.
    pub dx: i32,
    pub dy: i32,
    /// `ZPLANE`: the two are within the creature's `+0x56` of each other.
    pub plane: bool,
    /// The tracker's own return value: false when the creature is on the same
    /// plane and inside `+0x52` but outside `+0x54`, which is when its own
    /// controller gets to choose an attack.
    pub walking: bool,
    /// `FindDistance`: how far apart the two are across.
    pub distance: i32,
}

/// `MonsterTrack`, transcribed.
///
/// `CheckZAxis` puts them on the same plane when the depth difference is
/// within `+0x56`; otherwise the creature walks in depth and the tracker
/// answers "still walking". `CheckXAxis` against `+0x54` sends it to
/// `TrackBack`, which walks *away*; against `+0x52` it answers "in range";
/// beyond that `TrackOpponent` walks towards.
pub fn track(me: &Fighter, foe: &Fighter, def: &ActorDef) -> Track {
    let dx = foe.x - me.x;
    let dy = foe.y - me.y;
    let distance = dx.abs();
    let plane = dy.abs() <= def.depth_tolerance;
    let mut walking = false;
    let mut step_y = 0;
    if !plane {
        // `me.z > foe.z` sets bit 3, which `MoveU` reads: towards the horizon.
        step_y = if dy < 0 { -1 } else { 1 };
        walking = true;
    }
    if distance <= def.back_off {
        // `TrackBack`: give ground, whichever side he is on.
        return Track { dx: if dx >= 0 { -1 } else { 1 }, dy: step_y, plane, walking: true, distance };
    }
    if distance <= def.approach {
        return Track { dx: 0, dy: step_y, plane, walking, distance };
    }
    Track { dx: dx.signum(), dy: step_y, plane, walking: true, distance }
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
    if v >= 100 { v - 27 } else { v }
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
pub fn decide(s: &Sight, brain: &mut Brain, seed: &mut u16) -> Act {
    match s.def.controller() {
        Controller::Trogg => trogg(s, brain, seed, false),
        Controller::TroggSpear => trogg(s, brain, seed, true),
        Controller::Troll => troll(s, brain),
        Controller::Ratman => ratman(s, brain),
        Controller::Mudman => mudman(s, brain),
        Controller::Balok => balok(s, brain),
        Controller::Beast => beast(s, brain, seed),
        Controller::Demon => demon(s, brain),
        Controller::Dragon => dragon(s, brain),
        Controller::Claw => claw(s, brain),
        Controller::Knight => knight(s, brain),
    }
}

/// The walk an ordinary tracker asks for, or standing still.
fn walk(t: Track) -> Act {
    if t.dx == 0 && t.dy == 0 {
        Act::Idle
    } else {
        Act::Walk { dx: t.dx, dy: t.dy, script: None }
    }
}

/// Tick the cooldown the way `DemonAttack` and `TroggAttacks` do: down by one,
/// and the creature is free on the tick it reaches zero, not the one after.
fn cooling(brain: &mut Brain) -> bool {
    if brain.cooldown > 0 {
        brain.cooldown -= 1;
        return brain.cooldown > 0;
    }
    false
}

/// `ControlTrogg`, `TroggAttack`, `TroggAttacks`, `TroggSwing`, `TroggChop`.
fn trogg(s: &Sight, brain: &mut Brain, seed: &mut u16, spear: bool) -> Act {
    let t = track(s.me, s.foe, s.def);
    if t.walking && !t.plane {
        return walk(t);
    }
    let d = t.distance;
    // `TroggAttack`: inside the back-off range it gives ground rather than
    // striking, whatever the tracker said.
    if d <= s.def.back_off {
        return walk(t);
    }
    if !s.foe.alive() {
        // The finisher, and only with the gore on: `TroggAttack` tests
        // DS:0x700 before it does anything to a fallen knight.
        if !s.gore || !s.body || s.decapped {
            return Act::Idle;
        }
        if d > 100 {
            return walk(t);
        }
        if cooling(brain) {
            return Act::Idle;
        }
        brain.cooldown = 10;
        return Act::Attack { kind: Attack::Swing, spawn: None };
    }
    if cooling(brain) {
        return Act::Idle;
    }
    if spear {
        // The kind 0x10 branch of `TroggAttacks`: one lunge, and only inside
        // the approach range.
        if d > s.def.approach {
            return walk(t);
        }
        brain.cooldown = 20;
        return Act::Attack { kind: Attack::Lunge, spawn: None };
    }
    if d > 100 {
        // `TroggChop` refuses beyond 120 and walks instead.
        if d > 120 {
            return walk(t);
        }
        brain.cooldown = 10;
        return Act::Attack { kind: Attack::Chop, spawn: None };
    }
    // Inside a hundred: a swing, unless the roll comes up short *and* the
    // knight is holding a block, which the swing cannot get through and the
    // chop can.
    let roll = percent(seed);
    if roll <= 30 && s.foe.guarding() == Some(Attack::Block) {
        brain.cooldown = 10;
        return Act::Attack { kind: Attack::Chop, spawn: None };
    }
    brain.cooldown = 10;
    Act::Attack { kind: Attack::Swing, spawn: None }
}

/// `ControlTroll` and `TrollAttack`: the club inside a hundred, the overhead
/// chop from further out, and never two chops running.
fn troll(s: &Sight, brain: &mut Brain) -> Act {
    let t = track(s.me, s.foe, s.def);
    if t.walking {
        return walk(t);
    }
    let d = t.distance;
    if d >= 100 && d < 150 && brain.phase != 1 {
        brain.phase = 1;
        return Act::Attack { kind: Attack::Chop, spawn: None };
    }
    brain.phase = 0;
    Act::Attack { kind: Attack::Swing, spawn: None }
}

/// `ControlRatCollide`: it does not use the tracker at all. It slashes inside
/// forty, bites out to fifty, and leaps at anything further.
fn ratman(s: &Sight, brain: &mut Brain) -> Act {
    let dy = s.foe.y - s.me.y;
    let plane = dy.abs() <= s.def.depth_tolerance;
    let d = (s.foe.x - s.me.x).abs();
    let toward = (s.foe.x - s.me.x).signum();
    // `RatmanHit` sets a fifteen frame delay every time a blow of its own
    // lands, which is what keeps the slash from being a blur.
    if brain.cooldown > 0 {
        brain.cooldown -= 1;
        return Act::Idle;
    }
    if !s.foe.alive() {
        return Act::Idle;
    }
    let leap = |dy: i32| Act::Walk {
        dx: toward,
        dy,
        script: s.def.scripts_for("leap").first().cloned(),
    };
    if !plane {
        return leap(dy.signum());
    }
    if d <= 40 {
        brain.cooldown = 15;
        return Act::Attack { kind: Attack::Swing, spawn: None };
    }
    if d <= 50 {
        brain.cooldown = 15;
        return Act::Attack { kind: Attack::Lunge, spawn: None };
    }
    leap(0)
}

/// `ControlMudmen`: it reaches for you between seventy five and a hundred,
/// and inside that it goes under the ground and comes up beside you.
fn mudman(s: &Sight, brain: &mut Brain) -> Act {
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
            return Act::Strike { script: "Mudmen_Hit".into(), damage: 0, fatal: false };
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
    let t = track(s.me, s.foe, s.def);
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
    Act::Attack { kind: Attack::Swing, spawn: None }
}

/// `ControlBalok`: it closes in hops, uppercuts at arm's length, grabs from
/// further out, and stands off between a hundred and twenty and a hundred and
/// eighty unless you are throwing daggers at it.
fn balok(s: &Sight, brain: &mut Brain) -> Act {
    let dy = s.foe.y - s.me.y;
    let plane = dy.abs() <= s.def.depth_tolerance;
    let d = (s.foe.x - s.me.x).abs();
    let toward = (s.foe.x - s.me.x).signum();
    let hop = |dy: i32| Act::Walk { dx: toward, dy, script: None };
    if !s.foe.alive() {
        return Act::Idle;
    }
    if !plane {
        return hop(dy.signum());
    }
    if d <= 70 {
        brain.phase = 0;
        return hop(0);
    }
    if d <= 80 {
        if brain.phase == 1 {
            brain.phase = 0;
            return Act::Attack { kind: Attack::Chop, spawn: None };
        }
        brain.phase = 1;
        return Act::Attack { kind: Attack::Swing, spawn: None };
    }
    if d <= 120 {
        brain.phase = 0;
        return Act::Attack { kind: Attack::Chop, spawn: None };
    }
    if s.foe.daggers() > 0 || d > 180 {
        return hop(0);
    }
    Act::Idle
}

/// `ControlBeast`, `BeastCharge`, `SetBEASTZ` and `SetBeastTimer`: it does not
/// track at all. It runs from one side of the arena to the other, turns round
/// off the edge, waits, picks a depth and comes back.
fn beast(s: &Sight, brain: &mut Brain, seed: &mut u16) -> Act {
    let leftward = brain.flags & flag::LEFTWARD != 0;
    let (l, r) = (s.bounds.left, s.bounds.right);
    let turning = if leftward { s.me.x <= l } else { s.me.x >= r };
    if turning {
        brain.flags ^= flag::LEFTWARD;
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
    let dir = if brain.flags & flag::LEFTWARD != 0 { -1 } else { 1 };
    let want = s.foe.y + brain.walk as i32;
    let dy = (want - s.me.y).signum();
    Act::Walk { dx: dir, dy, script: None }
}

/// `ControlDemon` and `DemonAttack`: the slap inside a hundred, the zap out to
/// a hundred and thirty, the whip out to a hundred and forty, and the whip's
/// own four phase follow-through.
fn demon(s: &Sight, brain: &mut Brain) -> Act {
    if brain.flags & flag::UNBORN != 0 {
        // `[di+0x10]` is `Demon_Evolve`, so the demon's first script is its
        // own materialisation, and its last frame calls `AddDemonWhirl`.
        brain.flags &= !flag::UNBORN;
        return Act::Play("Demon_Evolve".into());
    }
    let t = track(s.me, s.foe, s.def);
    let d = t.distance;
    // The whip chain, which the original keeps as `DemonFLAGS` bits 1, 8,
    // 0x10 and 0x20 and follows through whatever the distance now is.
    if brain.phase > 0 {
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
        if brain.phase % 2 == 0 && caught {
            // The follow-through with the knight already caught: the original
            // hands him `Knight_SwSlapped` outright rather than waiting for a
            // weapon part to touch him.
            brain.flags &= !flag::CAUGHT;
            let hit = if brain.phase == 0 { "Demon_UWhipHit" } else { "Demon_OWhipHit" };
            return Act::Strike { script: hit.into(), damage: s.me.damage, fatal: false };
        }
        if d >= low && d <= high {
            brain.flags |= flag::CAUGHT;
            let caught_script =
                if brain.phase == 2 { "Demon_OWhipKnight" } else { "Demon_UWhipKnight" };
            return Act::Play(caught_script.into());
        }
        return Act::Play(script.into());
    }
    if cooling(brain) {
        return walk(t);
    }
    if !s.foe.alive() {
        return Act::Idle;
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
        return Act::Attack { kind: Attack::Chop, spawn: None };
    }
    if d <= 130 {
        brain.cooldown = 6;
        return Act::Attack { kind: Attack::Swing, spawn: None };
    }
    if d <= 140 {
        brain.cooldown = 5;
        brain.phase = 1;
        return Act::Attack { kind: Attack::Lunge, spawn: None };
    }
    walk(t)
}

/// `ControlDragon`: the set piece. The head lifts when you come inside a
/// hundred and forty and lowers when you go back out, and what it does to you
/// depends on which it is doing.
fn dragon(s: &Sight, brain: &mut Brain) -> Act {
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
        return Act::Walk { dx: 0, dy, script: s.def.scripts_for(row).get(frame).cloned() };
    }
    if !s.foe.alive() && brain.flags & flag::HEAD_MOVING == 0 {
        return Act::Stand(if up { "Dragon_HighStance".into() } else { "Dragon_Stance".into() });
    }
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
    let stance = || Act::Stand(if up { "Dragon_HighStance".into() } else { "Dragon_Stance".into() });
    if !s.foe.alive() {
        return stance();
    }
    // `TrackKnight`: the head shifts five pixels at a time inside thirty to a
    // hundred, and follows him in depth. `MonsterTrack` does the choosing; the
    // clamp is the original's own, and it is why the dragon never leaves its
    // corner of the arena.
    let t = track(s.me, s.foe, s.def);
    let dx = if s.me.x + t.dx * 5 < 30 || s.me.x + t.dx * 5 > 100 { 0 } else { t.dx };
    if t.dy != 0 || dx != 0 {
        return Act::Walk { dx, dy: t.dy, script: None };
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
            return Act::Attack { kind: Attack::Chop, spawn: Some("Dragon_Fire".into()) };
        }
        brain.cooldown = 6;
        return Act::Attack { kind: Attack::Lunge, spawn: None };
    }
    brain.cooldown = 8;
    // `Dragon_LowBreath`, kind 4.
    Act::Attack { kind: Attack::Swing, spawn: Some("Dragon_Fire".into()) }
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
    Act::Attack { kind: Attack::RThrust, spawn: None }
}

/// A knight, or anything with no controller of its own. Deliberately plain:
/// it closes and swings, and it struggles out of a hold the way a person
/// would, which is fire and down together (`MudmenEntangle` reads exactly
/// those two bits).
fn knight(s: &Sight, brain: &mut Brain) -> Act {
    if s.me.held() {
        return Act::Struggle;
    }
    let t = track(s.me, s.foe, s.def);
    if !s.foe.alive() {
        if !s.gore || !s.body || s.decapped || t.distance > 100 {
            return Act::Idle;
        }
        if cooling(brain) {
            return Act::Idle;
        }
        brain.cooldown = 20;
        return Act::Attack { kind: Attack::Swing, spawn: None };
    }
    if t.walking {
        return walk(t);
    }
    if cooling(brain) {
        return Act::Idle;
    }
    brain.cooldown = 20;
    Act::Attack { kind: Attack::Swing, spawn: None }
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
        ActorDef { approach, back_off, depth_tolerance: 5, ..scripted_def() }
    }

    #[test]
    fn the_tracker_closes_holds_and_gives_ground() {
        let d = ranged(100, 80);
        let me = at(0, 50);
        // Beyond the approach range: walk in.
        assert_eq!(track(&me, &at(200, 50), &d).dx, 1);
        // Inside it but outside the back-off: hold, and answer "in range".
        let t = track(&me, &at(90, 50), &d);
        assert_eq!((t.dx, t.walking), (0, false));
        // Inside the back-off: give ground.
        let t = track(&me, &at(40, 50), &d);
        assert_eq!((t.dx, t.walking), (-1, true));
        // Off the plane: walk in depth, and never answer "in range".
        let t = track(&me, &at(90, 90), &d);
        assert!(t.walking && !t.plane && t.dy == 1);
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

    /// One tick of one controller against a knight standing at `foe_x`.
    fn ask(def: &ActorDef, brain: &mut Brain, me_x: i32, foe_x: i32, dy: i32) -> Act {
        let me = at(me_x, 50);
        let foe = at(foe_x, 50 + dy);
        let s = Sight {
            me: &me,
            foe: &foe,
            def,
            bounds: Bounds { left: 0, right: 319, top: 10, bottom: 114 },
            gore: true,
            body: false,
            decapped: false,
        };
        let mut seed = 0x2f1du16;
        decide(&s, brain, &mut seed)
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
        let seen: Vec<(&str, String)> =
            ranges.iter().map(|(n, a, b)| (*n, profile(n, *a, *b))).collect();
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
        assert_ne!(by("trogg"), by("trogg_spear"), "the spear takes its own branch");
        assert!(!by("beast").contains("Attack"), "the beast has no swing: {}", by("beast"));
        assert!(by("ratman").contains("Lunge"), "the ratman bites: {}", by("ratman"));
    }

    /// `TroggAttacks`: the overhead beyond a hundred, the swing inside it, and
    /// `TroggChop` refusing anything past a hundred and twenty.
    #[test]
    fn the_trogg_chops_at_arms_length_and_swings_up_close() {
        let def = creature("trogg", 100, 90);
        let mut b = Brain::default();
        assert_eq!(kind(&ask(&def, &mut b, 0, 110, 0)), Some(Attack::Chop), "110 is the chop");
        let mut b = Brain::default();
        assert_eq!(kind(&ask(&def, &mut b, 0, 95, 0)), Some(Attack::Swing), "95 is the swing");
        // Beyond a hundred and twenty `TroggChop` walks instead.
        let mut b = Brain::default();
        assert!(matches!(ask(&def, &mut b, 0, 125, 0), Act::Walk { .. }));
        // Inside the back-off range it gives ground rather than striking.
        let mut b = Brain::default();
        assert!(matches!(ask(&def, &mut b, 0, 60, 0), Act::Walk { dx: -1, .. }));
        // And a blow is followed by ten frames of nothing.
        let mut b = Brain::default();
        ask(&def, &mut b, 0, 95, 0);
        assert_eq!(b.cooldown, 10);
        assert!(matches!(ask(&def, &mut b, 0, 95, 0), Act::Idle));
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
        assert!(matches!(ask(&def, &mut b, 0, 135, 0), Act::Walk { .. }), "past the approach it walks");
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
        assert_eq!(kind(&ask(&def, &mut b, 0, 35, 0)), Some(Attack::Swing), "the slash");
        let mut b = Brain::default();
        assert_eq!(kind(&ask(&def, &mut b, 0, 45, 0)), Some(Attack::Lunge), "the bite");
        let mut b = Brain::default();
        assert!(matches!(ask(&def, &mut b, 0, 60, 0), Act::Walk { .. }), "the leap");
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
        assert_eq!(kind(&ask(&def, &mut b, 0, 80, 0)), Some(Attack::Swing), "the arm at eighty");
        let mut b = Brain::default();
        assert_eq!(ask(&def, &mut b, 0, 60, 0), Act::Play("Mudmen_IBury".into()));
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
        assert_eq!(ask(&def, &mut b, 0, 150, 0), Act::Idle, "it waits at a hundred and fifty");
        // The same position, with ten on his belt.
        let mut me = at(0, 50);
        let mut foe = at(150, 50);
        foe.record.set(crate::taskvm::field::DAGGERS, 10);
        me.brain = Brain::default();
        let s = Sight {
            me: &me,
            foe: &foe,
            def: &def,
            bounds: Bounds { left: 0, right: 319, top: 10, bottom: 114 },
            gore: true,
            body: false,
            decapped: false,
        };
        let mut seed = 1u16;
        let mut brain = Brain::default();
        assert!(
            matches!(decide(&s, &mut brain, &mut seed), Act::Walk { dx: 1, .. }),
            "a thrown dagger brings it in"
        );
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
        assert!(matches!(ask(&def, &mut b, 100, 40, 0), Act::Walk { dx: 1, .. }),
                "it charges away from him as readily as at him");
        // At the right edge it turns and waits.
        let mut b = Brain::default();
        assert_eq!(ask(&def, &mut b, 319, 40, 0), Act::Idle);
        assert!(b.flags & flag::LEFTWARD != 0, "it turned round");
        assert!((5..=20).contains(&b.timer), "and waits {} frames", b.timer);
        let before = b.timer;
        assert_eq!(ask(&def, &mut b, 319, 40, 0), Act::Idle);
        assert_eq!(b.timer, before - 1);
    }

    /// `DemonAttack`: the slap inside a hundred, the zap out to a hundred and
    /// thirty, the whip out to a hundred and forty, and the whip's own four
    /// phase follow-through afterwards.
    #[test]
    fn the_demon_arrives_then_slaps_zaps_and_whips_by_range() {
        let def = creature("demon", 95, 90);
        let mut b = Brain::default();
        b.flags |= flag::UNBORN;
        assert_eq!(ask(&def, &mut b, 0, 90, 0), Act::Play("Demon_Evolve".into()));
        assert_eq!(kind(&ask(&def, &mut b, 0, 90, 0)), Some(Attack::Chop), "the slap");
        let mut b = Brain::default();
        assert_eq!(kind(&ask(&def, &mut b, 0, 120, 0)), Some(Attack::Swing), "the zap");
        let mut b = Brain::default();
        assert_eq!(kind(&ask(&def, &mut b, 0, 138, 0)), Some(Attack::Lunge), "the whip");
        assert_eq!(b.phase, 1, "and the whip starts its follow-through");
        b.cooldown = 0;
        assert_eq!(ask(&def, &mut b, 0, 130, 0), Act::Play("Demon_OWhipKnight".into()));
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
        assert_eq!(kind(&ask(&def, &mut b, 100, 250, 0)), Some(Attack::Swing), "the low breath");
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
        assert_eq!(kind(&ask(&def, &mut up, 100, 200, 0)), Some(Attack::Chop), "the high breath");
        let mut up = b;
        up.cooldown = 0;
        assert_eq!(kind(&ask(&def, &mut up, 100, 170, 0)), Some(Attack::Lunge), "the bite");
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
        assert_eq!(ask(&def, &mut b, 5, 150, 0), Act::Idle, "and nothing beyond it");
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
        assert!(one.windows(2).any(|w| w[0] != w[1]), "the register is stuck: {one:?}");
    }
}
