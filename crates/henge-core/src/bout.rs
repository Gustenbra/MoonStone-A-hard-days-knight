//! A bout: any number of fighters in one arena.
//!
//! This is where combat is actually resolved, and it lives in the simulation
//! rather than in the renderer for three reasons. It can be tested headlessly,
//! it can run on a server that has no graphics at all, and it can be replayed
//! from a seed to prove it is deterministic.
//!
//! The bout takes one `Intent` per fighter and does not care where they came
//! from. A keyboard, an AI and a network packet are interchangeable, which is
//! the seam networked play plugs into.

use crate::arena::{Arrivals, Border, Field, GLOBAL};
use crate::combat::{line_hits_body, Attack, Fighter, Intent, Order, State};
use crate::content::ActorDef;
use crate::monster::Controller;
use crate::taskvm::{self, field, Effect, Task, TaskActor, FACING_LEFT, FACING_RIGHT};
use crate::wave::Wave;
use serde::{Deserialize, Serialize};

/// Reported so a caller can play a sound, shake the screen, or log a replay,
/// without the simulation knowing any of those things exist.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct HitEvent {
    pub attacker: usize,
    pub target: usize,
    pub damage: i32,
    pub fatal: bool,
}

/// A blow that was stopped: `blockflag` coming back set from `CheckBlock`.
/// Reported beside the hits, and never as one, because nothing was hurt.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parry {
    pub attacker: usize,
    pub target: usize,
    /// What stopped it.
    pub with: Attack,
}

/// A task in the arena that is not a fighter: a thrown dagger, a spray of
/// blood. The original keeps ten task slots and puts these in them beside the
/// fighters, each with a controller of its own (`ControlKnife`, `ControlMisc`);
/// here they are their own list, stepped by the same interpreter on the same
/// bank tables, and just as deterministic.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Missile {
    /// The fighter it came from, whose blows it deals and whom it cannot hit.
    pub owner: usize,
    /// Whose definition holds its scripts and banks.
    pub actor: String,
    pub task: Task,
    pub record: TaskActor,
    /// Where its feet would be, for depth: the y the owner had when it left.
    pub depth: i32,
    /// The attack kind it lands as, or none for something that only draws.
    pub attack: Option<Attack>,
    /// The script `ControlKnife` hands the task each time its frame ends:
    /// `Knife`, twenty pixels forward and the blade. Empty for a task that
    /// runs one script and kills itself, which is what `Blood1` does.
    pub flight: String,
    pub script_tick: u32,
    /// Connected, or flown off the edge: gone at the end of the tick.
    pub spent: bool,
    /// This task rides on its owner rather than flying: `ControlDemon` copies
    /// the demon's position into the whirl's task every frame, and takes the
    /// whirl away when the demon dies (`StopDemonWhirl`).
    #[serde(default)]
    pub follow: bool,
}

impl Missile {
    /// The blow of the frame being shown: the weapon parts, as rectangles.
    fn hit_line(&self, def: &ActorDef) -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        for p in self.task.shown.iter().filter(|p| p.is(taskvm::part_flags::WEAPON)) {
            let Some(bank) = def.bank(p.table, p.bank) else { continue };
            let t = &self.task;
            let Some(r) = taskvm::place(p, bank, (t.x, t.y, t.z), t.mirror()) else { continue };
            let (l, top) = (r.x, r.y);
            let (rr, b) = (r.x + r.w as i32, r.y + r.h as i32);
            out.extend([(l, top), (rr, top), (rr, b), (l, b), (l, top)]);
        }
        out
    }

    fn hash_into(&self, mix: &mut impl FnMut(i64)) {
        mix(self.owner as i64);
        for b in self.actor.as_bytes() {
            mix(*b as i64);
        }
        self.task.hash_into(mix);
        for (at, v) in &self.record.fields {
            mix(*at as i64);
            mix(*v as i64);
        }
        mix(self.depth as i64);
        mix(self.follow as i64);
        mix(self.attack.map_or(-1, |a| a.kind() as i64));
        for b in self.flight.as_bytes() {
            mix(*b as i64);
        }
        mix(self.script_tick as i64);
        mix(self.spent as i64);
    }
}

/// What `ControlKnife` stops the dagger at: past the right edge of the
/// screen, or ten pixels past the left.
const KNIFE_RIGHT_EDGE: i32 = 0x14a;
const KNIFE_LEFT_EDGE: i32 = -10;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Bout {
    pub fighters: Vec<Fighter>,
    /// The ground of the arena: the rectangles nobody may walk up into, as the
    /// `.T` header lists them. Applied every frame to every fighter.
    #[serde(default)]
    pub field: Field,
    pub damage: i32,
    /// Ticks since only one fighter (or none) was left standing.
    pub settled_for: u32,
    /// The gore switch, off: the original's DS:0x700, which the title
    /// screen's option row toggles and every `TASKSKIP` and `GATED` part
    /// reads. Zero there is gore on, so this is false by default too.
    #[serde(default)]
    pub bloodless: bool,
    /// Daggers in flight, and blood.
    #[serde(default)]
    pub missiles: Vec<Missile>,
    /// The blows stopped on the last tick. Output, like the hits, and not
    /// part of the fingerprint.
    #[serde(default)]
    pub parries: Vec<Parry>,
    /// `_WIZARD:RND`'s shift register, which the creatures' controllers roll
    /// against. The original keeps one word for the whole game at DS:`0xe22f`;
    /// here it belongs to the bout, so a fight replays and two machines agree.
    #[serde(default = "default_rng")]
    pub rng: u16,
    /// `DeCapFLAG`, DS:`0x7841`: somebody has gone for the fallen knight's
    /// head, so nobody else does. `InitCombat` (0x307) zeroes it; `TroggAttack`
    /// sets it on the way to its swing (0x2e9f), `SetDecapFLAG` (0x3e76) sets
    /// it from a script's `TASKGOSUB`, and `TroggAttack` (0x2e8b) and
    /// `BKnightAttack` (0x4c2b) read it.
    #[serde(default)]
    pub decap: bool,
    /// How many creatures this fight still owes, how many it holds at once, and
    /// what one of them is fielded with: `TotalMonsters`, `MaxMonsters`,
    /// `NumberInCombat` and `SIDE`. See [`crate::wave`].
    ///
    /// An empty one owes nothing, which is what a knight against a knight is.
    #[serde(default)]
    pub wave: Wave,
    /// `AddCNT`, DS:`0xa68`, the rotation every arrival's standing depth comes
    /// out of.
    ///
    /// The original's lives in BSS and nothing ever resets it, so the caller
    /// that sets a bout up carries it from one fight to the next and leaves it
    /// here; a creature walking in mid-fight takes the next place in the same
    /// rotation, which is the reason it has to live on the bout as well.
    #[serde(default)]
    pub arrivals: Arrivals,
}

/// Any non-zero start; the original seeds its register off the BIOS tick,
/// which is exactly the sort of thing this engine may not do.
fn default_rng() -> u16 {
    0x2f1d
}

impl Bout {
    pub fn new(field: Field, fighters: Vec<Fighter>) -> Bout {
        Bout {
            fighters,
            field,
            damage: 25,
            settled_for: 0,
            bloodless: false,
            missiles: Vec::new(),
            parries: Vec::new(),
            rng: default_rng(),
            decap: false,
            wave: Wave::default(),
            arrivals: Arrivals::default(),
        }
    }

    pub fn alive(&self) -> impl Iterator<Item = usize> + '_ {
        self.fighters
            .iter()
            .enumerate()
            .filter(|(_, f)| f.alive())
            .map(|(i, _)| i)
    }

    pub fn alive_count(&self) -> usize {
        self.fighters.iter().filter(|f| f.alive()).count()
    }

    /// Some(index) once exactly one fighter is left, None while a fight is on or
    /// if everybody died.
    pub fn winner(&self) -> Option<usize> {
        let mut it = self.alive();
        match (it.next(), it.next()) {
            (Some(i), None) => Some(i),
            _ => None,
        }
    }

    /// Whether the fight is over.
    ///
    /// `CountTheDead` ends one on two conditions and no others: the player's own
    /// hit points at or below nothing (`0x21a`, and the `StopCombat` his own
    /// death script calls), or nothing owed and nothing standing (`0x223` and
    /// `0x22a`). So a fight that still owes creatures is not settled however
    /// empty the screen looks, which is what used to keep the rest of a
    /// fourteen strong lair from ever arriving.
    ///
    /// A bout with no wave at all, which is every knight against a knight, is
    /// over when one is left, as it always was.
    pub fn settled(&self) -> bool {
        if self.wave.max > 0 {
            let player_down = self.fighters.first().is_some_and(|f| !f.alive());
            // The count is `NumberInCombat`'s, and the arena is asked as well,
            // so a death script that somehow never reaches its `TASKGOSUB`
            // cannot leave a fight nothing is able to end.
            let standing = self.fighters.iter().skip(1).any(|f| f.alive());
            return player_down || (!self.wave.owed() && !standing);
        }
        self.alive_count() <= 1
    }

    /// `InitNewMO` (`0x27ee`) and the `AddPlayer` (`0x2989`) it ends with: one
    /// more creature on the screen.
    ///
    /// ```text
    /// 027ee  add  word [NumberInCombat], 1
    /// 027f5  call FindTABLE                  ; a free actor slot
    /// 027fa  mov  ax, [si]     / mov [di+2], ax    x
    /// 027ff  mov  ax, [si+2]   / mov [di+4], ax    y
    /// 02805  mov  ax, [si+4]   / mov [di+6], ax    z, which AddPlayer overwrites
    /// 0280b  mov  ax, [si+6]   / mov [di+8], al    facing
    /// 02814  mov  bx, [INITANIM] / call bx         the stat block
    /// 0281d  call AddPlayer                        the standing depth
    /// ```
    ///
    /// `seat` is the record `SIDE` or `SetMonsterCombat` named. The hit points
    /// and the blow are the wave's, because what `INITANIM` writes has already
    /// been through the moon and through this engine's own scale by the time a
    /// bout is built, and the bout is not the place that knows either.
    pub fn field_creature(&mut self, actor: &str, def: &ActorDef, seat: usize) -> usize {
        // 027ee  add word [NumberInCombat], 1
        self.wave.arrived();
        let (x, facing) = def.seat_at(seat).unwrap_or((GLOBAL.left + 50, 1));
        // 029b6  mov word [di+6], ax: the rotation's depth over the record's own.
        let y = self.field.standing_row(self.arrivals.next());
        let mut f = Fighter::new(actor, def, x, y, facing);
        if self.wave.health > 0 {
            f.max_health = self.wave.health;
            f.health = self.wave.health;
            f.record.set_health(self.wave.health);
        }
        if self.wave.damage > 0 {
            f.damage = self.wave.damage;
        }
        self.fighters.push(f);
        self.fighters.len() - 1
    }

    /// The nearest living fighter that is not `me`, for an opponent to aim at.
    pub fn nearest_foe(&self, me: usize) -> Option<usize> {
        let m = &self.fighters[me];
        self.alive()
            .filter(|i| *i != me && !self.fighters[*i].hidden)
            .min_by_key(|i| {
                let f = &self.fighters[*i];
                (f.x - m.x).abs() + (f.y - m.y).abs() * 2
            })
    }

    /// The nearest fallen fighter that can still be struck, for a finisher.
    pub fn nearest_body<'a, F>(&self, me: usize, def_of: F) -> Option<usize>
    where
        F: Fn(&str) -> &'a ActorDef,
    {
        let m = &self.fighters[me];
        self.fighters
            .iter()
            .enumerate()
            .filter(|(i, f)| *i != me && f.finishable(def_of(&f.actor)))
            .min_by_key(|(_, f)| (f.x - m.x).abs() + (f.y - m.y).abs() * 2)
            .map(|(i, _)| i)
    }

    /// Is a corpse still being dealt with: a finisher playing on a fallen
    /// fighter, which a caller ending the bout on a count should wait for.
    pub fn finishing<'a, F>(&self, def_of: F) -> bool
    where
        F: Fn(&str) -> &'a ActorDef,
    {
        self.fighters.iter().any(|f| {
            f.state == State::Dead
                && def_of(&f.actor).finishes.values().any(|s| *s == f.script)
                && f.task.as_ref().map_or(false, |t| t.active && t.running)
        })
    }

    /// One tick, with every fighter the same kind. `intents` is indexed to
    /// match `fighters`; a short slice is treated as idle for the rest, which
    /// keeps a caller honest without panicking mid-fight.
    pub fn step(&mut self, def: &ActorDef, intents: &[Intent]) -> Vec<HitEvent> {
        self.step_with(|_| def, intents)
    }

    /// What one fighter's blow of one kind takes off: `CalcDamage`.
    ///
    /// The `*Dam` entry for the kind, which is the fighter's own figure, or
    /// the bout's where the actor leaves it at zero, scaled by the attack's
    /// entry against the default attack's; then the sheet's bonus, strength
    /// and the sword, which `CalcDamage` adds before it doubles a chop. The
    /// table already carries the chop doubled, so here the doubling is
    /// applied to the bonus: `(4 + 1) * 2` comes out as `8 + 1 * 2`.
    fn blow(&self, attacker: usize, def: &ActorDef, attack: Attack) -> i32 {
        let f = &self.fighters[attacker];
        let base = match f.damage {
            0 => self.damage,
            d => d,
        };
        let (num, den) = def.blow_ratio(attack);
        let bonus = if attack == Attack::Chop { f.bonus * 2 } else { f.bonus };
        (base * num / den + bonus).max(1)
    }

    /// `KnifeThrow`: one dagger off the thrower, and a task of its own on
    /// `SpeedKnife` at the thrower's position and facing, on his banks.
    fn throw_knife(&mut self, owner: usize, def: &ActorDef) {
        let f = &mut self.fighters[owner];
        let Some(task) = f.task.as_ref() else { return };
        let daggers = f.record.get(field::DAGGERS);
        if daggers <= 0 || !def.animation.contains_key("SpeedKnife") {
            return;
        }
        f.record.set(field::DAGGERS, daggers - 1);
        let mut knife = Task::new("SpeedKnife", task.x, task.y, task.facing);
        knife.table = def.bank_table;
        knife.z = task.z;
        let mut m = Missile {
            owner,
            actor: f.actor.clone(),
            task: knife,
            record: TaskActor::default(),
            depth: f.y,
            attack: Some(Attack::Knife),
            flight: "Knife".into(),
            script_tick: 0,
            spent: false,
            follow: false,
        };
        // Its first frame now, so it is on screen the tick it leaves the hand.
        m.task.step(&def.animation, &mut m.record, self.bloodless);
        self.missiles.push(m);
    }

    /// A task that rides on a fighter: the demon's whirl. It is a missile like
    /// any other, stepped by the same interpreter, but its position is the
    /// owner's rather than its own, which is what `ControlDemon` does to it
    /// every frame.
    fn attach(&mut self, owner: usize, script: &str, def: &ActorDef) {
        if !def.animation.contains_key(script) {
            return;
        }
        if self.missiles.iter().any(|m| m.follow && m.owner == owner) {
            return;
        }
        let f = &self.fighters[owner];
        let Some(t) = f.task.as_ref() else { return };
        let mut task = Task::new(script, t.x, t.y, t.facing);
        task.table = def.bank_table;
        task.z = t.z;
        let mut m = Missile {
            owner,
            actor: f.actor.clone(),
            task,
            record: TaskActor::default(),
            depth: f.y,
            attack: None,
            flight: String::new(),
            script_tick: 0,
            spent: false,
            follow: true,
        };
        m.task.step(&def.animation, &mut m.record, self.bloodless);
        self.missiles.push(m);
    }

    /// `AddDragonFIRE`: a task on `Dragon_Fire` at the dragon's own position
    /// plus 0x37 across and five deeper, which burns whatever it touches.
    fn breathe(&mut self, owner: usize, script: &str, def: &ActorDef) {
        if !def.animation.contains_key(script) {
            return;
        }
        let f = &self.fighters[owner];
        let Some(t) = f.task.as_ref() else { return };
        let facing = t.facing;
        let dir = if f.facing < 0 { -1 } else { 1 };
        let mut task = Task::new(script, t.x + 0x37 * dir, t.y, facing);
        task.table = def.bank_table;
        task.z = t.z;
        let mut m = Missile {
            owner,
            actor: f.actor.clone(),
            task,
            record: TaskActor::default(),
            depth: f.y + 5,
            attack: Some(Attack::Chop),
            flight: String::new(),
            script_tick: 0,
            spent: false,
            follow: false,
        };
        m.task.step(&def.animation, &mut m.record, self.bloodless);
        self.missiles.push(m);
    }

    /// `AddBlood`: a spray at the strike point, facing the way the knight
    /// does, on bank table 4, which is `BLO.CEL` five times over. Every part
    /// of `Blood1` is gated, so with the gore off the task runs its five
    /// frames drawing nothing and kills itself.
    fn add_blood(&mut self, owner: usize, at: (i32, i32), depth: i32, actor: &str, def: &ActorDef) {
        if !def.animation.contains_key("Blood1") || !def.banks.contains_key(&4) {
            return;
        }
        let facing = if self.fighters[owner].facing < 0 { FACING_LEFT } else { FACING_RIGHT };
        let mut blood = Task::new("Blood1", at.0, at.1, facing);
        blood.table = 4;
        let mut m = Missile {
            owner,
            actor: actor.to_string(),
            task: blood,
            record: TaskActor::default(),
            depth,
            attack: None,
            flight: String::new(),
            script_tick: 0,
            spent: false,
            follow: false,
        };
        m.task.step(&def.animation, &mut m.record, self.bloodless);
        self.missiles.push(m);
    }

    /// `SETDEMONBORD`: a fight is fought on the border of any actor that
    /// brings one of its own.
    ///
    /// **Recovered.** The arena's `.T` file opens with a count and that many
    /// eight-byte border records, and the loader takes the deepest of them as
    /// the row the knights are stood below; `SBORD` walks the same list every
    /// frame and clears the walk bits that would carry a fighter into one.
    /// `SETDEMONBORD`, the last routine in `GFX`, **overwrites** the whole
    /// list with one record: 0 to 309 across, 10 to 99 deep. So the demon does
    /// not decorate the screen and it does not add to the tree line, it
    /// replaces the ground the fight is fought on. `InitKnightvsDemon` calls
    /// it at image `0x2752`.
    pub fn apply_actor_borders<'a, F>(&mut self, def_of: F)
    where
        F: Fn(&str) -> &'a ActorDef,
    {
        let own: Vec<Border> = self
            .fighters
            .iter()
            .filter_map(|f| def_of(&f.actor).ground())
            .filter(|b| b.is_sane())
            .collect();
        if let Some(b) = own.first() {
            self.field.narrow_to(*b);
        }
    }

    /// The task loop's `+0xc` branch for a creature whose controller has one:
    /// `TroggHit` (0x2f4d) plays the recovery, which [`Fighter::recover`]
    /// already did, and writes ten into `+0x4a` (0x2f55). It does not call
    /// `FaceKnight`, so the facing is left alone here as well.
    fn hit_something(&mut self, attacker: usize, def: &ActorDef) {
        if matches!(def.controller(), Controller::Trogg | Controller::TroggSpear) {
            crate::monster::trogg_hit(&mut self.fighters[attacker].brain);
        }
    }

    /// The task loop's `+0xe` branch: `TroggStruck` (0x2f19) zeroes `+0x4a`
    /// (0x2f1c) beside picking the blow-taken script, which
    /// [`Fighter::struck`] already did. No `FaceKnight` on this path either.
    fn got_struck(&mut self, target: usize, def: &ActorDef) {
        if matches!(def.controller(), Controller::Trogg | Controller::TroggSpear) {
            crate::monster::trogg_struck(&mut self.fighters[target].brain);
        }
    }

    /// Whether anyone is being finished off: the original's `DeCapFLAG`, which
    /// `TroggAttack` tests so that only one creature comes in for the head.
    ///
    /// The flag itself is [`Bout::decap`]. A corpse already on a finishing
    /// script counts too, which covers a finisher that reached it by a route
    /// that never raised the flag.
    fn decapping<'a, F>(&self, def_of: &F) -> bool
    where
        F: Fn(&str) -> &'a ActorDef,
    {
        self.decap
            || self
                .fighters
                .iter()
                .any(|f| def_of(&f.actor).finishes.values().any(|s| *s == f.script))
    }

    /// One tick of one fighter's own controller, for a seat the machine plays.
    ///
    /// The bout still cannot tell a controller from a keyboard: this hands
    /// back an [`Intent`] like any other, and what it writes on the fighter is
    /// the same thing the original writes into the actor record. It runs only
    /// on the tick a script frame ended, which is when the original's task
    /// loop calls a controller at all, so a cooldown of ten is ten frames
    /// rather than ten sixtieths of a second.
    pub fn monster_intent<'a, F>(&mut self, me: usize, target: usize, def_of: F, gore: bool) -> Intent
    where
        F: Fn(&str) -> &'a ActorDef,
    {
        use crate::monster::{decide, Act, Sight};
        // `DragonMoveClaw1` and `ControlClaw`: the two forelimbs sit ten rows
        // either side of the head's own depth and follow it, and when the
        // dragon is down they play `Dragon_ClawDead` and go.
        if def_of(&self.fighters[me].actor).controller() == Controller::Claw {
            let head = self
                .fighters
                .iter()
                .position(|f| def_of(&f.actor).controller() == Controller::Dragon && f.alive());
            match head {
                Some(h) => {
                    let (y, off) = (self.fighters[h].y, self.fighters[me].brain.timer);
                    let (_, ny) = GLOBAL.clamp(self.fighters[me].x, y + off);
                    self.fighters[me].y = ny;
                }
                None if self.fighters[me].alive() => {
                    let t_def = def_of(&self.fighters[me].actor);
                    let left = self.fighters[me].health.max(1);
                    self.fighters[me].health = left;
                    self.fighters[me].take_hit(left);
                    let _ = t_def;
                    return Intent::default();
                }
                None => return Intent::default(),
            }
        }
        self.fighters[me].brain.flags |= crate::monster::flag::DRIVEN;
        let free = {
            let f = &self.fighters[me];
            // A held fighter is not free, but its controller still runs: the
            // struggle out of a hold is the one thing it is allowed to say.
            f.alive() && (f.holder.is_some() || f.ready(def_of(&f.actor))) && f.brain.rest <= 0
        };
        if !free {
            let mut i = self.fighters[me].drive;
            i.attack = false;
            return i;
        }
        let decapped = self.decapping(&def_of);
        let (act, mut intent) = {
            let def = def_of(&self.fighters[me].actor);
            let foe = &self.fighters[target];
            let t_def = def_of(&foe.actor);
            let sight = Sight {
                me: &self.fighters[me],
                foe,
                def,
                bounds: GLOBAL,
                gore,
                body: foe.finishable(t_def),
                decapped,
            };
            let mut brain = self.fighters[me].brain;
            let mut seed = self.rng;
            // The record's `+8` goes in as it stands and comes back as the
            // controller left it: `FaceKnight` and its kin write it inside
            // the controller, and `TASKHANDLE` (0x9741) takes `dh`, which
            // `NOTEND+20` loaded from `[di+8]`, into the task on the way
            // out. A creature's facing is set here and nowhere else.
            let mut facing = self.fighters[me].facing;
            let act = decide(&sight, &mut brain, &mut seed, &mut facing);
            // TroggAttack+0x3b (0x2e9f): `mov word ptr [DeCapFLAG], 1`, on
            // the one path that orders an attack on a knight with no hit
            // points left. The controller cannot reach the bout's word, so
            // the bout reads the decision off the order.
            let finishing = matches!(act, Act::Attack { .. })
                && foe.health <= 0
                && matches!(def.controller(), Controller::Trogg | Controller::TroggSpear);
            if finishing {
                self.decap = true;
            }
            self.fighters[me].brain = brain;
            self.fighters[me].facing = facing;
            self.rng = seed;
            (act, Intent::default())
        };
        let order = |state, script: String, attack| Some(Order { state, script, attack });
        self.fighters[me].ordered = match act {
            Act::Idle => order(State::Idle, String::new(), None),
            Act::Walk { dx, dy, script } => {
                intent.dx = dx;
                intent.dy = dy;
                order(State::Walk, script.unwrap_or_default(), None)
            }
            Act::Attack { kind, spawn } => {
                let def = def_of(&self.fighters[me].actor);
                // `AddDragonFIRE`: the breath is a task of its own, started
                // ahead of the head and one row deeper.
                if let Some(script) = spawn {
                    self.breathe(me, &script, def);
                }
                match def.attack_for(kind) {
                    Some((script, k)) => {
                        let state = if k.is_guard() { State::Guard } else { State::Attack };
                        order(state, script, Some(k))
                    }
                    None => order(State::Attack, String::new(), Some(kind)),
                }
            }
            Act::Stand(script) => order(State::Idle, script, None),
            Act::Play(script) => order(State::Attack, script, None),
            Act::Appear { x, facing, script } => {
                let b = GLOBAL;
                let f = &mut self.fighters[me];
                let (nx, ny) = b.clamp(x, f.y);
                f.x = nx;
                f.y = ny;
                f.facing = facing;
                order(State::Attack, script, None)
            }
            Act::Seize { script, ticks } => {
                self.fighters[target].holder = Some(me);
                self.fighters[target].hidden = true;
                self.fighters[me].brain.timer = ticks;
                order(State::Attack, script, None)
            }
            // Fire and down together, which is all `MudmenEntangle` reads.
            Act::Struggle => {
                intent.dy = 1;
                intent.attack = true;
                None
            }
            Act::Strike { script, damage, fatal } => {
                let t_def = def_of(&self.fighters[target].actor);
                self.fighters[target].holder = None;
                self.fighters[target].hidden = false;
                if fatal {
                    let left = self.fighters[target].health.max(1);
                    self.fighters[target].struck(t_def, left, None);
                } else if damage > 0 {
                    self.fighters[target].struck(t_def, damage, None);
                }
                order(State::Attack, script, None)
            }
        };
        // The standing order is the walk, never the press: a struggle is
        // read on the frame it is made and not held down for six ticks.
        self.fighters[me].brain.rest = def_of(&self.fighters[me].actor).script_ticks.max(1) as i32;
        self.fighters[me].drive = Intent { attack: false, ..intent };
        intent
    }

    /// One tick, with each fighter looked up by the actor it is.
    ///
    /// A bout used to take one definition for everyone in it, which was true
    /// while everyone was a knight. An ambush is a troll against a knight, so
    /// the definition is now asked for per fighter, by `Fighter::actor`. The
    /// lookup is a closure rather than a map so the caller decides what a
    /// missing name means; a map that returned nothing would have had to be
    /// answered with a panic in the middle of a fight.
    pub fn step_with<'a, F>(&mut self, def_of: F, intents: &[Intent]) -> Vec<HitEvent>
    where
        F: Fn(&str) -> &'a ActorDef,
    {
        self.parries.clear();
        let bloodless = self.bloodless;

        // A blow in the air this tick: who or what is swinging it, the shape,
        // the kind, and whose feet it is judged level from.
        struct Blow {
            attacker: usize,
            missile: Option<usize>,
            line: Vec<(i32, i32)>,
            attack: Option<Attack>,
            depth: i32,
        }
        let mut blows: Vec<Blow> = Vec::new();

        for i in 0..self.fighters.len() {
            if self.fighters[i].brain.rest > 0 {
                self.fighters[i].brain.rest -= 1;
            }
            let intent = intents.get(i).copied().unwrap_or_default();
            let def = def_of(&self.fighters[i].actor);
            let line = self.fighters[i].step_gated(def, intent, &self.field, bloodless);
            if !line.is_empty() {
                let f = &self.fighters[i];
                blows.push(Blow { attacker: i, missile: None, line, attack: f.attack, depth: f.y });
            }
            // What the script asked the game to do this tick. `KnifeThrow` is
            // the one call the knight's own scripts make that puts something
            // new in the arena; `TASKADDTASK` is the general form of it.
            let effects: Vec<Effect> = self.fighters[i].effects.clone();
            for e in effects {
                match e {
                    Effect::Gosub { routine, .. } if routine == "KnifeThrow" => {
                        self.throw_knife(i, def);
                    }
                    // `AddDemonWhirl`: a second task on the demon's own banks
                    // that rides along with it until `StopDemonWhirl`.
                    Effect::Gosub { routine, .. } if routine == "AddDemonWhirl" => {
                        self.attach(i, "Demon_Whirl", def);
                    }
                    Effect::Gosub { routine, .. } if routine == "StopDemonWhirl" => {
                        self.missiles.retain(|m| !(m.follow && m.owner == i));
                    }
                    // The demon's zap: `KnightOFF` takes ten off him and
                    // takes him off the board, `KnightON` puts him back a
                    // hundred and thirty seven pixels to the demon's side.
                    Effect::Gosub { routine, .. } if routine == "KnightOFF" => {
                        if let Some(t) = self.nearest_foe(i) {
                            let t_def = def_of(&self.fighters[t].actor);
                            self.fighters[t].struck(t_def, 10, None);
                            self.fighters[t].hidden = true;
                        }
                    }
                    Effect::Gosub { routine, .. } if routine == "KnightON" => {
                        let (x, y, facing) = {
                            let f = &self.fighters[i];
                            (f.x, f.y, f.facing)
                        };
                        let bounds = GLOBAL;
                        for t in 0..self.fighters.len() {
                            if t == i || !self.fighters[t].hidden {
                                continue;
                            }
                            let (nx, ny) = bounds.clamp(x + 0x89 * facing, y);
                            let f = &mut self.fighters[t];
                            f.x = nx;
                            f.y = ny;
                            f.facing = -facing;
                            f.hidden = false;
                        }
                    }
                    // `SetDecapFLAG` (0x3e76): `mov word ptr [DeCapFLAG], 1`.
                    Effect::Gosub { routine, .. } if routine == "SetDecapFLAG" => {
                        self.decap = true;
                    }
                    // `CountTheDead` (0x213), which every one of the eighteen
                    // death scripts in the bestiary calls through `TASKGOSUB`,
                    // and which is the whole of the wave logic: one off
                    // `NumberInCombat`, one off `TotalMonsters`, and then
                    // `CountDone` (0x243) walks `INITMO` until the screen holds
                    // `MaxMonsters` again or nothing is left to send. See
                    // `crate::wave`.
                    Effect::Gosub { routine, .. } if routine == "CountTheDead" => {
                        let actor = self.fighters[i].actor.clone();
                        for _ in 0..self.wave.dead() {
                            let seat = self.wave.next_seat(&def.wave);
                            self.field_creature(&actor, def, seat);
                        }
                    }
                    // `KillKnight`, which the dragon's own chewing calls.
                    Effect::Gosub { routine, .. } if routine == "KillKnight" => {
                        if let Some(t) = self.nearest_foe(i) {
                            let t_def = def_of(&self.fighters[t].actor);
                            let left = self.fighters[t].health.max(1);
                            self.fighters[t].holder = None;
                            self.fighters[t].struck(t_def, left, None);
                        }
                    }
                    Effect::Spawn { script } => {
                        let f = &self.fighters[i];
                        if let Some(t) = f.task.as_ref() {
                            let mut task = Task::new(script, t.x, t.y, t.facing);
                            task.table = t.table;
                            task.z = t.z;
                            let mut m = Missile {
                                owner: i,
                                actor: f.actor.clone(),
                                task,
                                record: TaskActor::default(),
                                depth: f.y,
                                attack: None,
                                flight: String::new(),
                                script_tick: 0,
                                spent: false,
                                follow: false,
                            };
                            m.task.step(&def.animation, &mut m.record, bloodless);
                            self.missiles.push(m);
                        }
                    }
                    _ => {}
                }
            }
        }

        // The missiles: `ControlKnife` for a dagger, which flies until it
        // touches something or leaves the screen; `ControlMisc` for the rest,
        // which run their one script to its `TASKKILLTASK`.
        for k in 0..self.missiles.len() {
            let def = def_of(&self.missiles[k].actor);
            // A task riding on a fighter follows him, and goes when he does.
            if self.missiles[k].follow {
                let owner = self.missiles[k].owner;
                let (alive, at) = match self.fighters.get(owner) {
                    Some(f) => (f.alive(), f.task.as_ref().map(|t| (t.x, t.y, t.z, t.facing))),
                    None => (false, None),
                };
                let m = &mut self.missiles[k];
                match (alive, at) {
                    (true, Some((x, y, z, facing))) => {
                        m.task.x = x;
                        m.task.y = y;
                        m.task.z = z;
                        m.task.facing = facing;
                        m.depth = y;
                    }
                    _ => m.spent = true,
                }
            }
            let m = &mut self.missiles[k];
            m.script_tick += 1;
            if m.script_tick >= def.script_ticks.max(1) {
                m.script_tick = 0;
                if !m.task.running && !m.flight.is_empty() {
                    m.task.replace(m.flight.clone());
                }
                m.task.step(&def.animation, &mut m.record, bloodless);
            }
            if !m.task.active {
                m.spent = true;
            }
            if !m.flight.is_empty() {
                let off = if m.task.mirror() {
                    m.task.x <= KNIFE_LEFT_EDGE
                } else {
                    m.task.x >= KNIFE_RIGHT_EDGE
                };
                if off {
                    m.spent = true;
                }
            }
            if !m.spent && m.attack.is_some() {
                let line = m.hit_line(def);
                if !line.is_empty() {
                    blows.push(Blow {
                        attacker: m.owner,
                        missile: Some(k),
                        line,
                        attack: m.attack,
                        depth: m.depth,
                    });
                }
            }
        }

        // Resolve every blow against every other fighter. A blow connects at
        // most once, so a single strike cannot damage two people, which matters
        // the moment there are more than two in the arena.
        let mut events = Vec::new();
        for blow in blows {
            let attacker = blow.attacker;
            let a_def = def_of(&self.fighters[attacker].actor);
            let damage = blow.attack.map_or_else(
                || {
                    let f = &self.fighters[attacker];
                    (match f.damage { 0 => self.damage, d => d }) + f.bonus
                },
                |a| self.blow(attacker, a_def, a),
            );
            let mut connected = false;
            for target in 0..self.fighters.len() {
                if target == attacker {
                    continue;
                }
                let t_def = def_of(&self.fighters[target].actor);
                // Two kinds of fighter can disagree about how deep a plane
                // is. The looser of the two decides, both ways: a creature
                // that reaches you across ten rows of depth can be reached
                // across the same ten, or a knight who cannot step to its
                // row would be untouchable to it and it to him.
                let plane = a_def.depth_tolerance.max(t_def.depth_tolerance);
                if (blow.depth - self.fighters[target].y).abs() > plane {
                    continue;
                }
                if self.fighters[target].hidden {
                    continue;
                }
                if self.fighters[target].alive() {
                    let body = self.fighters[target].body(t_def);
                    if !line_hits_body(&blow.line, body) {
                        continue;
                    }
                    connected = true;
                    // `CheckBlock`, for the blows that go through it.
                    let a_facing = self.fighters[attacker].facing;
                    let checked = blow.missile.is_none() && a_def.blockable;
                    if let (true, Some(a)) = (checked, blow.attack) {
                        if self.fighters[target].blocks(t_def, a_facing, a) {
                            let with = self.fighters[target].guarding().unwrap_or(a);
                            self.parries.push(Parry { attacker, target, with });
                            self.fighters[attacker].recover(a_def);
                            self.hit_something(attacker, a_def);
                            break;
                        }
                    }
                    let evading = self.fighters[target].guarding() == Some(Attack::Evade);
                    self.fighters[target].struck(t_def, damage, blow.attack);
                    self.got_struck(target, t_def);
                    // `DragonStruck` sets `DragonFLAGS` bit 7 the moment a
                    // knight lands anything, and from then on the dragon
                    // breathes rather than bites. Nothing else reads the bit.
                    self.fighters[target].brain.flags |= crate::monster::flag::STRUCK;
                    let fatal = !self.fighters[target].alive();
                    events.push(HitEvent { attacker, target, damage, fatal });
                    if t_def.bleeds {
                        let at = strike_point(&blow.line, body);
                        let depth = self.fighters[target].y;
                        let actor = self.fighters[target].actor.clone();
                        self.add_blood(attacker, at, depth, &actor, t_def);
                    }
                    // `MudmenHit2`: the mudman's arm does not stagger a
                    // knight, it takes hold of him. Forty frames to tear free
                    // by pressing fire and down, and the choke at the end.
                    let seizes = a_def.controller().seizes().filter(|(script, _)| {
                        blow.missile.is_none()
                            && self.fighters[target].alive()
                            && a_def.animation.contains_key(*script)
                    });
                    if let Some((script, ticks)) = seizes {
                        self.fighters[target].holder = Some(attacker);
                        // The original removes the knight's own task and draws
                        // him inside `Mudmen_EntangleKnight` instead.
                        self.fighters[target].hidden = true;
                        let f = &mut self.fighters[attacker];
                        f.brain.flags |= crate::monster::flag::ENTANGLING;
                        f.brain.timer = ticks;
                        f.ordered = Some(Order {
                            state: State::Attack,
                            script: script.to_string(),
                            attack: None,
                        });
                    }
                    // The swing is over the moment it lands, save for the
                    // exceptions `KnightHitNormal` and `KnightHitKnight` make.
                    if blow.missile.is_none() && blow.attack != Some(Attack::UThrust) && !evading {
                        self.fighters[attacker].recover(a_def);
                    }
                    if blow.missile.is_none() {
                        self.hit_something(attacker, a_def);
                    }
                    break;
                }
                // Down, but still a body while the kneel lasts.
                let Some(body) = self.fighters[target].corpse_body(t_def) else { continue };
                if !self.fighters[target].finishable(t_def) || !line_hits_body(&blow.line, body) {
                    continue;
                }
                let own_kind = self.fighters[attacker].actor == self.fighters[target].actor;
                if self.fighters[target].finish(t_def, blow.attack, own_kind) {
                    connected = true;
                    let decapitating = blow.attack == Some(Attack::Swing) && !bloodless;
                    if blow.missile.is_none() && !decapitating {
                        self.fighters[attacker].recover(a_def);
                    }
                    if blow.missile.is_none() {
                        self.hit_something(attacker, a_def);
                    }
                    break;
                }
            }
            if connected {
                match blow.missile {
                    Some(k) => self.missiles[k].spent = true,
                    None => self.fighters[attacker].struck = true,
                }
            }
        }
        self.missiles.retain(|m| !m.spent);

        self.separate(&def_of);
        if self.settled() {
            self.settled_for += 1;
        }
        events
    }

    /// Living fighters at the same depth cannot occupy the same ground. Pushing
    /// them apart rather than blocking movement keeps a scrappy close-quarters
    /// fight readable instead of jamming people into a stalemate.
    ///
    /// Two different kinds of fighter are kept apart by the wider of the two
    /// girths and judged level by the looser of the two depth tolerances, so
    /// a troll and a knight agree about whether they are standing on each
    /// other whichever of them is asked.
    fn separate<'a, F>(&mut self, def_of: &F)
    where
        F: Fn(&str) -> &'a ActorDef,
    {
        let girth_of = |def: &ActorDef| {
            if def.girth > 0 { def.girth } else { (def.body[2] - def.body[0]) as i32 }
        };
        let living: Vec<usize> = self.alive().collect();
        for a in 0..living.len() {
            for b in a + 1..living.len() {
                let (i, j) = (living[a], living[b]);
                let (di, dj) = (def_of(&self.fighters[i].actor), def_of(&self.fighters[j].actor));
                if (self.fighters[i].y - self.fighters[j].y).abs()
                    > di.depth_tolerance.max(dj.depth_tolerance)
                {
                    continue;
                }
                let min_gap = girth_of(di).max(girth_of(dj));
                let gap = self.fighters[j].x - self.fighters[i].x;
                if gap.abs() >= min_gap {
                    continue;
                }
                let push = (min_gap - gap.abs() + 1) / 2;
                let dir = if gap >= 0 { 1 } else { -1 };
                let bounds = GLOBAL;
                let (ix, _) = bounds.clamp(self.fighters[i].x - push * dir, self.fighters[i].y);
                let (jx, _) = bounds.clamp(self.fighters[j].x + push * dir, self.fighters[j].y);
                self.fighters[i].x = ix;
                self.fighters[j].x = jx;
            }
        }
    }

    /// A cheap fingerprint of the whole simulation.
    ///
    /// Two bouts fed the same inputs must agree on this at every tick. That is
    /// the property networked play is built on, and the only way to keep it is
    /// to check it continuously rather than to hope.
    pub fn state_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |v: i64| {
            h ^= v as u64;
            h = h.wrapping_mul(0x1000_0000_01b3);
        };
        mix(self.bloodless as i64);
        for f in &self.fighters {
            mix(f.x as i64);
            mix(f.y as i64);
            mix(f.facing as i64);
            mix(f.health as i64);
            mix(f.damage as i64);
            mix(f.state as i64);
            mix(f.struck as i64);
            mix(f.blocked as i64);
            mix(f.attack.map_or(-1, |a| a.kind() as i64));
            for b in f.script.as_bytes() {
                mix(*b as i64);
            }
            mix(f.evaded as i64);
            mix(f.restart as i64);
            mix(f.bonus as i64);
            mix(f.cursed as i64);
            mix(f.brain.cooldown as i64);
            mix(f.brain.timer as i64);
            mix(f.brain.flags as i64);
            mix(f.brain.walk as i64);
            mix(f.brain.phase as i64);
            mix(f.brain.rest as i64);
            mix(f.holder.map_or(-1, |h| h as i64));
            mix(f.hidden as i64);
            mix(f.drive.dx as i64);
            mix(f.drive.dy as i64);
            match &f.ordered {
                None => mix(-1),
                Some(o) => {
                    mix(o.state as i64);
                    mix(o.attack.map_or(-1, |a| a.kind() as i64));
                    for b in o.script.as_bytes() {
                        mix(*b as i64);
                    }
                }
            }
            mix(f.player.frame as i64);
            mix(f.player.ticks_in_frame as i64);
            mix(f.player.finished as i64);
            // The task VM is part of the simulation, not a decoration on it: a
            // swing's hit shape comes out of the frame it is showing, so two
            // machines that disagree about where a script is disagree about
            // combat. Everything that decides the next tick goes in.
            mix(f.cycle as i64);
            mix(f.script_tick as i64);
            if let Some(t) = &f.task {
                t.hash_into(&mut mix);
            }
            for (at, v) in &f.record.fields {
                mix(*at as i64);
                mix(*v as i64);
            }
        }
        for m in &self.missiles {
            m.hash_into(&mut mix);
        }
        mix(self.settled_for as i64);
        mix(self.rng as i64);
        h
    }
}

/// Where a blow landed, for the blood: the middle of the overlap between the
/// blow's extent and the body it found. The original has the pixel where the
/// weapon pile and the body pile first met (`CXx`, `CY`); this is the same
/// place at rectangle resolution.
fn strike_point(line: &[(i32, i32)], body: (i32, i32, i32, i32)) -> (i32, i32) {
    let (l, t, r, b) = body;
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for (x, y) in line {
        x0 = x0.min(*x);
        y0 = y0.min(*y);
        x1 = x1.max(*x);
        y1 = y1.max(*y);
    }
    let (ox0, oy0) = (x0.max(l), y0.max(t));
    let (ox1, oy1) = (x1.min(r), y1.min(b));
    if ox0 <= ox1 && oy0 <= oy1 {
        ((ox0 + ox1) / 2, (oy0 + oy1) / 2)
    } else {
        ((l + r) / 2, (t + b) / 2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::{EndBehaviour, Frame, Sequence};
    use std::collections::BTreeMap;

    fn def() -> ActorDef {
        let seq = |name: &str, sprites: &[u16], end, hits: bool| {
            let frames = sprites
                .iter()
                .map(|s| Frame {
                    sprite: *s,
                    ticks: 1,
                    hit: if hits { vec![[10, 30], [40, 30]] } else { vec![] },
                    ..Frame::default()
                })
                .collect();
            (name.to_string(), Sequence { name: name.into(), frames, end })
        };
        let mut sequences = BTreeMap::new();
        for (k, v) in [
            seq("idle", &[0], EndBehaviour::Loop, false),
            seq("walk", &[1, 2], EndBehaviour::Loop, false),
            seq("attack", &[3, 4], EndBehaviour::HoldLast, true),
            seq("hurt", &[5], EndBehaviour::HoldLast, false),
            seq("death", &[6], EndBehaviour::HoldLast, false),
        ] {
            sequences.insert(k, v);
        }
        ActorDef {
            sheet: "test".into(), health: 100, speed_x: 2, speed_y: 1,
            reach: 40, depth_tolerance: 6, attack_cooldown: 30,
            bounty: 0,
            girth: 0,
            body: [-9, 0, 9, 52], sequences,
            ..ActorDef::default()
        }
    }

    /// One arena's ground. The tree line is put high enough that every
    /// fighter in these tests stands below it and is free to walk.
    fn arena_field() -> Field {
        Field::new(vec![Border { left: 0, right: 319, bottom: 60, top: 10 }])
    }

    fn four() -> Bout {
        let d = def();
        Bout::new(
            arena_field(),
            (0..4)
                .map(|i| Fighter::new("k", &d, 40 + i * 70, 100, 1))
                .collect(),
        )
    }

    /// A lair fight is not over while it still owes creatures, and each death
    /// brings the next one in: `CountTheDead` (0x213) and `CountDone` (0x243),
    /// with `InitNewMO` (0x27ee) putting it where its seat record says.
    ///
    /// This replaces nothing: the bout used to end the moment one fighter was
    /// left, which is what kept a lair of fourteen to whatever the arena seated.
    #[test]
    fn a_fight_that_still_owes_creatures_is_not_settled_and_tops_itself_up() {
        use crate::wave::{Wave, WaveDef};
        let mut def = def();
        // `TroggTABLE`'s first two records: one off the left edge facing right,
        // one off the right edge facing left.
        def.seats = vec![[-50, 0, 100, 1], [360, 0, 150, 3]];
        def.health = 10;
        let wave_def = WaveDef {
            max: 1,
            heads: 4,
            cap: 0,
            alternates: true,
            reinforced: true,
            opens_with_side: false,
            level: Vec::new(),
        };
        let mut b = Bout::new(
            arena_field(),
            vec![Fighter::new("k", &def, 200, 100, -1), Fighter::new("m", &def, -50, 100, 1)],
        );
        b.wave = Wave::open(&wave_def, None, &crate::wave::Level::default());
        b.wave.arrived();
        b.wave.health = 10;
        assert_eq!((b.wave.max, b.wave.total, b.wave.in_combat), (1, 4, 1));

        let mut arrived = 1;
        let mut killed = 0;
        while !b.settled() {
            // Kill whatever creature is standing, then run what its death
            // script's `TASKGOSUB CountTheDead` runs.
            let Some(i) = (1..b.fighters.len()).find(|i| b.fighters[*i].alive()) else {
                panic!("the wave owes {} and the arena is empty", b.wave.total);
            };
            b.fighters[i].health = 0;
            b.fighters[i].state = State::Dead;
            killed += 1;
            for _ in 0..b.wave.dead() {
                let seat = b.wave.next_seat(&wave_def);
                let n = b.field_creature("m", &def, seat);
                arrived += 1;
                // `SIDE` alternates, so the arrivals come in from opposite
                // sides: record one first off the 0 `SetUpDKL` leaves.
                let want = if seat == 1 { (360, -1) } else { (-50, 1) };
                assert_eq!((b.fighters[n].x, b.fighters[n].facing), want);
                // And each is fielded with what the wave carries, not with the
                // definition's own hit points.
                assert_eq!(b.fighters[n].health, 10);
            }
            assert!(killed < 20, "the count has to run out");
        }
        // `max + total - 1`: one on the screen and four owed is four fights.
        assert_eq!((killed, arrived), (4, 4));
        assert!(b.wave.done());
        assert_eq!(b.wave.in_combat, 0);
        // The player is still up, so the bout is settled by the count and not
        // by him going down.
        assert!(b.fighters[0].alive());
    }

    /// The other of `CountTheDead`'s two endings: `or ax, ax / jle StopCombat`
    /// on the player's own hit points at 0x21a. A lair that still owes ten
    /// creatures is over the moment he falls.
    #[test]
    fn the_player_going_down_ends_a_fight_that_still_owes_creatures() {
        use crate::wave::{Wave, WaveDef};
        let d = def();
        let mut b = Bout::new(
            arena_field(),
            vec![Fighter::new("k", &d, 200, 100, -1), Fighter::new("m", &d, 40, 100, 1)],
        );
        let wave_def =
            WaveDef { max: 1, heads: 10, reinforced: true, ..WaveDef::default() };
        b.wave = Wave::open(&wave_def, None, &crate::wave::Level::default());
        b.wave.arrived();
        assert!(!b.settled(), "ten owed, so nothing about this is over");
        b.fighters[0].health = 0;
        b.fighters[0].state = State::Dead;
        assert!(b.settled());
    }

    /// `SETDEMONBORD`, and the gate on it: a fight narrows to the demon's own
    /// rectangle when a demon is in it, and to nothing otherwise.
    #[test]
    fn a_demon_brings_its_own_border_and_nothing_else_does() {
        use crate::combat::tests::scripted_def;
        let plain = scripted_def();
        let mut demon = scripted_def();
        demon.border = Some([0, 309, 10, 99]);
        let mut ugly = scripted_def();
        // A border the data got wrong is refused rather than shrinking the
        // arena to nothing.
        ugly.border = Some([200, 100, 90, 10]);

        let arena = arena_field();
        let pick = |name: &str| -> &ActorDef {
            match name {
                "demon" => Box::leak(Box::new(demon.clone())),
                "ugly" => Box::leak(Box::new(ugly.clone())),
                _ => Box::leak(Box::new(plain.clone())),
            }
        };

        let mut b = Bout::new(arena.clone(), vec![Fighter::new("k", &plain, 40, 100, 1)]);
        b.apply_actor_borders(pick);
        assert_eq!(b.field, arena, "an ordinary fight is fought on the arena's own ground");

        let mut b = Bout::new(
            arena.clone(),
            vec![Fighter::new("k", &plain, 40, 100, 1), Fighter::new("demon", &demon, 200, 100, -1)],
        );
        b.apply_actor_borders(pick);
        assert_eq!(
            b.field.borders,
            vec![Border { left: 0, right: 309, bottom: 99, top: 10 }],
            "the demon's record replaces the arena's list rather than joining it"
        );
        assert_eq!(b.field.floor(), 99, "and the ground begins fifteen rows higher");

        let mut b = Bout::new(
            arena.clone(),
            vec![Fighter::new("k", &plain, 40, 100, 1), Fighter::new("ugly", &ugly, 200, 100, -1)],
        );
        b.apply_actor_borders(pick);
        assert_eq!(b.field, arena, "an inverted border is refused");
    }

    /// A creature does not press a button: its own controller names the
    /// script and the kind, the bout carries the order, and the fighter plays
    /// it. This is the seam item 37 hangs on.
    #[test]
    fn a_creature_takes_its_orders_from_its_own_controller() {
        use crate::combat::tests::depth_def;
        let mut trogg = depth_def();
        trogg.controller = "trogg".into();
        trogg.approach = 100;
        trogg.back_off = 90;
        trogg.depth_tolerance = 5;
        let knight = depth_def();
        let pick = |name: &str| -> &ActorDef {
            match name {
                "trogg" => Box::leak(Box::new(trogg.clone())),
                _ => Box::leak(Box::new(knight.clone())),
            }
        };
        let mut b = Bout::new(
            arena_field(),
            vec![
                Fighter::new("k", &knight, 0, 100, 1),
                Fighter::new("trogg", &trogg, 110, 100, -1),
            ],
        );
        // A hundred and ten away is `TroggChop`'s range, and it is the chop
        // the controller hands over, not the swing the one button would give.
        let intent = b.monster_intent(1, 0, pick, true);
        assert!(!intent.attack, "the order carries the attack, not the intent");
        let order = b.fighters[1].ordered.clone().expect("the controller said nothing");
        assert_eq!(order.attack, Some(Attack::Chop));
        assert_eq!(order.script, "chop");
        b.step_with(pick, &[Intent::default(), intent]);
        assert_eq!(b.fighters[1].state, State::Attack);
        assert_eq!(b.fighters[1].attack, Some(Attack::Chop));
        // And the cooldown it set is in the fingerprint, so two machines
        // running the same fight agree about when it may strike again.
        assert_eq!(b.fighters[1].brain.cooldown, 10);
        let mut other = b.clone();
        assert_eq!(other.state_hash(), b.state_hash());
        other.fighters[1].brain.cooldown = 3;
        assert_ne!(other.state_hash(), b.state_hash(), "a brain is state");
    }

    /// The two branches of `ControlTrogg` the controller itself does not take,
    /// because they are raised by a blow rather than by an animation ending:
    /// `TroggStruck+3` (0x2f1c) zeroes `+0x4a` and `TroggHit+8` (0x2f55)
    /// writes ten into it. Neither calls `FaceKnight`.
    #[test]
    fn a_trogg_forgets_its_cooldown_when_struck_and_restarts_it_when_it_lands() {
        use crate::combat::tests::depth_def;
        let mut trogg = depth_def();
        trogg.controller = "trogg".into();
        trogg.approach = 100;
        trogg.back_off = 90;
        trogg.depth_tolerance = 5;
        let knight = depth_def();
        let pick = |name: &str| -> &ActorDef {
            match name {
                "trogg" => Box::leak(Box::new(trogg.clone())),
                _ => Box::leak(Box::new(knight.clone())),
            }
        };
        let mut b = Bout::new(
            arena_field(),
            vec![
                Fighter::new("k", &knight, 100, 100, 1),
                Fighter::new("trogg", &trogg, 130, 100, -1),
            ],
        );
        b.fighters[1].brain.cooldown = 7;
        b.fighters[1].facing = -1;
        // The knight swings and connects: the trogg is struck.
        let mut hits = 0;
        for _ in 0..40 {
            let ev = b.step_with(pick, &[Intent { dx: 0, dy: 0, attack: true }, Intent::default()]);
            hits += ev.len();
            if hits > 0 {
                break;
            }
        }
        assert!(hits > 0, "the swing landed");
        assert_eq!(b.fighters[1].brain.cooldown, 0, "TroggStruck+3: mov byte [di+0x4a], 0");
        assert_eq!(b.fighters[1].facing, -1, "TroggStruck never turns it");

        // And the other way round: the trogg's blow lands on the knight.
        let mut b = Bout::new(
            arena_field(),
            vec![
                Fighter::new("k", &knight, 100, 100, 1),
                Fighter::new("trogg", &trogg, 130, 100, -1),
            ],
        );
        b.fighters[1].brain.cooldown = 3;
        let mut hits = 0;
        for _ in 0..40 {
            let ev = b.step_with(pick, &[Intent::default(), Intent { dx: 0, dy: 0, attack: true }]);
            hits += ev.len();
            if hits > 0 {
                break;
            }
        }
        assert!(hits > 0, "the trogg's swing landed");
        assert_eq!(b.fighters[1].brain.cooldown, 10, "TroggHit+8: mov byte [di+0x4a], 0xa");
    }

    /// `DeCapFLAG` is a word of the bout's, not a reading off the corpse.
    /// `TroggAttack+0x3b` (0x2e9f) sets it the moment the trogg decides to
    /// go for a fallen knight's head, before anything has landed, and
    /// `TroggAttack+0x27` (0x2e8b) refuses a second try while it is set.
    #[test]
    fn the_decap_flag_is_raised_on_the_decision_and_stops_a_second_finisher() {
        use crate::combat::tests::depth_def;
        let mut trogg = depth_def();
        trogg.controller = "trogg".into();
        trogg.approach = 100;
        trogg.back_off = 90;
        trogg.depth_tolerance = 5;
        let knight = depth_def();
        let pick = |name: &str| -> &ActorDef {
            match name {
                "trogg" => Box::leak(Box::new(trogg.clone())),
                _ => Box::leak(Box::new(knight.clone())),
            }
        };
        let mut b = Bout::new(
            arena_field(),
            vec![
                Fighter::new("k", &knight, 100, 100, 1),
                Fighter::new("trogg", &trogg, 195, 100, -1),
                Fighter::new("trogg", &trogg, 5, 100, 1),
            ],
        );
        b.fighters[0].health = 0;
        b.fighters[0].state = State::Dead;
        assert!(!b.decap, "InitCombat (0x307) zeroes it");
        b.monster_intent(1, 0, pick, true);
        let first = b.fighters[1].ordered.clone().expect("an order");
        assert_eq!(first.attack, Some(Attack::Swing), "TroggAttack+0x41: jmp TroggSwing");
        assert!(b.decap, "TroggAttack+0x3b: mov word ptr [DeCapFLAG], 1");
        // The second trogg, inside a hundred with its count at zero, is
        // refused by the flag alone.
        b.fighters[2].brain.cooldown = 0;
        b.monster_intent(2, 0, pick, true);
        let second = b.fighters[2].ordered.clone().expect("an order");
        assert_eq!(second.state, State::Idle, "TroggAttack+0x2c: jne 02e9c");
        assert_eq!(second.attack, None);
    }

    /// `ControlClaw` never calls `CalcDamage`: the dragon's forelimbs take a
    /// blow and are unmoved by it.
    #[test]
    fn a_dragons_claw_takes_no_harm_from_anything() {
        use crate::combat::tests::depth_def;
        let mut claw = depth_def();
        claw.controller = "claw".into();
        let mut f = Fighter::new("claw", &claw, 0, 100, 1);
        f.struck(&claw, 40, Some(Attack::Swing));
        assert_eq!(f.health, claw.health, "a claw is not whittled down");
        assert_eq!(f.state, State::Hurt, "but it does flinch");
    }

    #[test]
    fn a_bout_holds_as_many_fighters_as_it_is_given() {
        let b = four();
        assert_eq!(b.alive_count(), 4);
        assert_eq!(b.winner(), None, "nobody has won while four are standing");
    }

    #[test]
    fn one_swing_cannot_cut_down_two_people() {
        let d = def();
        // Two targets stacked on the same spot, both inside one swing's line.
        let mut b = Bout::new(
            arena_field(),
            vec![
                Fighter::new("k", &d, 100, 100, 1),
                Fighter::new("k", &d, 125, 100, -1),
                Fighter::new("k", &d, 128, 100, -1),
            ],
        );
        let mut hits = Vec::new();
        for _ in 0..8 {
            let intents = [Intent { dx: 0, dy: 0, attack: true }, Intent::default(), Intent::default()];
            hits.extend(b.step(&d, &intents));
            if b.fighters[0].player.finished {
                break;
            }
        }
        assert_eq!(hits.len(), 1, "a single strike must land on one target");
    }

    #[test]
    fn the_last_one_standing_wins() {
        let d = def();
        let mut b = Bout::new(
            arena_field(),
            vec![
                Fighter::new("k", &d, 100, 100, 1),
                Fighter::new("k", &d, 160, 100, -1),
            ],
        );
        b.fighters[1].take_hit(1000);
        assert_eq!(b.winner(), Some(0));
        assert!(b.settled());
    }

    #[test]
    fn a_fighter_aims_at_the_nearest_living_opponent() {
        let mut b = four();
        assert_eq!(b.nearest_foe(0), Some(1));
        b.fighters[1].take_hit(1000);
        assert_eq!(b.nearest_foe(0), Some(2), "the dead are not targets");
    }

    /// The property everything networked rests on. Two independent bouts fed the
    /// same inputs must agree tick for tick, or lockstep and rollback are both
    /// impossible.
    #[test]
    fn identical_inputs_produce_identical_simulations() {
        let d = def();
        let script = |t: usize, i: usize| Intent {
            dx: if (t / 7 + i) % 3 == 0 { 1 } else { -1 },
            dy: if (t / 11 + i) % 4 == 0 { 1 } else { 0 },
            attack: (t / 5 + i) % 4 == 0,
        };
        let run = || {
            let mut b = four();
            let mut hashes = Vec::new();
            for t in 0..600 {
                let intents: Vec<Intent> = (0..4).map(|i| script(t, i)).collect();
                b.step(&d, &intents);
                hashes.push(b.state_hash());
            }
            hashes
        };
        assert_eq!(run(), run());
    }

    /// Snapshots have to restore exactly, or a player joining a bout in progress
    /// desynchronises immediately.
    #[test]
    fn a_bout_survives_a_round_trip_through_serialization() {
        let d = def();
        let mut b = four();
        for t in 0..90 {
            let intents: Vec<Intent> = (0..4)
                .map(|i| Intent { dx: 1, dy: 0, attack: (t + i) % 6 == 0 })
                .collect();
            b.step(&d, &intents);
        }

        let json = serde_json::to_string(&b).unwrap();
        let mut restored: Bout = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, b);
        assert_eq!(restored.state_hash(), b.state_hash());

        // And it must keep agreeing once both are simulated onward.
        for t in 0..120 {
            let intents: Vec<Intent> = (0..4)
                .map(|i| Intent { dx: -1, dy: 0, attack: (t + i) % 5 == 0 })
                .collect();
            b.step(&d, &intents);
            restored.step(&d, &intents);
        }
        assert_eq!(restored.state_hash(), b.state_hash());
    }

    /// The same round trip, for fighters run by the task VM. The interpreter's
    /// state is part of the fingerprint, so a snapshot that restored the
    /// fighters but not their scripts would be caught here.
    #[test]
    fn a_scripted_bout_survives_a_round_trip_through_serialization() {
        let d = crate::combat::tests::scripted_def();
        let mut b = Bout::new(
            arena_field(),
            vec![
                Fighter::new("k", &d, 100, 100, 1),
                Fighter::new("k", &d, 130, 100, -1),
                Fighter::new("k", &d, 200, 104, 1),
            ],
        );
        let script = |t: usize, i: usize| Intent {
            dx: if (t / 5 + i) % 3 == 0 { 1 } else { -1 },
            dy: 0,
            attack: (t / 4 + i) % 3 == 0,
        };
        let mut hits = 0;
        for t in 0..60 {
            let intents: Vec<Intent> = (0..3).map(|i| script(t, i)).collect();
            hits += b.step(&d, &intents).len();
        }
        assert!(hits > 0, "the weapon cels connect with something");

        let json = serde_json::to_string(&b).unwrap();
        let mut restored: Bout = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, b);
        assert_eq!(restored.state_hash(), b.state_hash());
        for t in 60..200 {
            let intents: Vec<Intent> = (0..3).map(|i| script(t, i)).collect();
            b.step(&d, &intents);
            restored.step(&d, &intents);
            assert_eq!(restored.state_hash(), b.state_hash(), "tick {t}");
        }
    }
}

#[cfg(test)]
mod depth {
    use super::*;
    use crate::combat::tests::depth_def;
    use crate::taskvm::{field, part_flags};

    /// One arena's ground. The tree line is put high enough that every
    /// fighter in these tests stands below it and is free to walk.
    fn arena_field() -> Field {
        Field::new(vec![Border { left: 0, right: 319, bottom: 60, top: 10 }])
    }

    /// Two knights thirty pixels apart, facing each other.
    fn pair(d: &ActorDef) -> Bout {
        Bout::new(
            arena_field(),
            vec![Fighter::new("k", d, 100, 100, 1), Fighter::new("k", d, 130, 100, -1)],
        )
    }

    fn swing() -> Intent {
        Intent { dx: 0, dy: 0, attack: true }
    }

    /// Down and back, for a knight facing left, is right and down.
    fn block_for(f: &Fighter) -> Intent {
        Intent { dx: -f.facing, dy: 1, attack: true }
    }

    /// `CheckBlock`: a block held against a swing from the front stops it.
    /// Nobody is hurt, the parry is reported, and the attacker's swing is
    /// cut short by his recovery.
    #[test]
    fn a_block_absorbs_a_swing_from_the_front() {
        let d = depth_def();
        let mut b = pair(&d);
        let mut hits = 0;
        let mut parries = 0;
        // Three ticks: the wind-up, the blade, and the recovery it bounces
        // into. Held longer, he would swing again.
        for _ in 0..3 {
            let intents = [swing(), block_for(&b.fighters[1])];
            hits += b.step(&d, &intents).len();
            parries += b.parries.len();
        }
        assert_eq!(hits, 0, "nothing landed");
        assert_eq!(parries, 1, "one swing, one parry");
        assert_eq!(b.fighters[0].state, State::Recover, "the swing bounced off");
        assert_eq!(b.fighters[1].health, 100);
        assert_eq!(b.fighters[1].state, State::Guard, "still holding it");
        assert!(b.fighters[0].struck, "the swing was spent on the shield");
    }

    /// The same block with the defender facing away is no block: `CheckBlock`
    /// clears `blockflag` when the two face the same way.
    #[test]
    fn a_block_does_not_stop_a_blow_from_behind() {
        let d = depth_def();
        let mut b = pair(&d);
        b.fighters[1].facing = 1;
        let mut hits = 0;
        for _ in 0..3 {
            let intents = [swing(), block_for(&b.fighters[1])];
            hits += b.step(&d, &intents).len();
        }
        assert_eq!(hits, 1);
        assert!(b.parries.is_empty());
        assert_eq!(b.fighters[1].health, 75);
    }

    /// The table says which guard stops which blow: a block stops a swing
    /// and not a chop; an evade stops a chop, once, and walking gives it
    /// back.
    #[test]
    fn the_block_table_and_the_evades_one_use() {
        let d = depth_def();
        let chop = Intent { dx: 0, dy: -1, attack: true };
        let evade = Intent { dx: 0, dy: 1, attack: true };

        let mut b = pair(&d);
        let mut hits = 0;
        for _ in 0..2 {
            let intents = [chop, block_for(&b.fighters[1])];
            hits += b.step(&d, &intents).len();
        }
        assert_eq!(hits, 1, "a block is the wrong guard for a chop");
        assert_eq!(b.fighters[1].health, 50, "and a chop is twice a swing");

        let mut b = pair(&d);
        let mut log = Vec::new();
        // Three chops, spaced so the defender is on his feet for each, with
        // a step to the side between the second and third.
        for t in 0..40 {
            let attacker = if matches!(t, 0 | 12 | 30) { chop } else { Intent::default() };
            let defender = if t == 22 { Intent { dx: 0, dy: 1, attack: false } } else { evade };
            let intents = [attacker, defender];
            let hits = b.step(&d, &intents);
            if !hits.is_empty() {
                log.push((t, "hit"));
            }
            if !b.parries.is_empty() {
                assert_eq!(b.parries[0].with, Attack::Evade);
                log.push((t, "evaded"));
            }
        }
        let kinds: Vec<&str> = log.iter().map(|(_, k)| *k).collect();
        assert_eq!(kinds, ["evaded", "hit", "evaded"], "{log:?}");
    }

    /// `CalcDamage` in a bout: the swing's table entry plus the sheet's
    /// strength and sword, and a chop doubled after the additions. At the
    /// original's scale a starting knight's swing is `4 + 1`, his chop
    /// `(4 + 1) * 2`; here the same with a bonus of three: seven, and
    /// fourteen, against the chop table's `8 + 3 * 2`.
    #[test]
    fn strength_and_the_sword_are_added_to_every_blow_and_a_chop_doubles_them() {
        let d = depth_def();
        let land = |bonus: i32, attack: Intent| {
            let mut b = pair(&d);
            b.damage = 4;
            b.fighters[0].bonus = bonus;
            let mut dealt = None;
            for _ in 0..8 {
                let hits = b.step(&d, &[attack, Intent::default()]);
                if let Some(h) = hits.first() {
                    dealt = Some(h.damage);
                    break;
                }
            }
            dealt.expect("the blow landed")
        };
        let chop = Intent { dx: 0, dy: -1, attack: true };
        assert_eq!(land(0, swing()), 4, "the table alone");
        assert_eq!(land(1, swing()), 5, "a new knight: four and his strength of one");
        assert_eq!(land(3, swing()), 7);
        assert_eq!(land(0, chop), 8, "the chop is twice the swing");
        assert_eq!(land(1, chop), 10, "(4 + 1) * 2, as CalcDamage doubles after adding");
        assert_eq!(land(3, chop), 14);
    }

    /// Two bouts that differ only in a sheet's strength differ in their
    /// fingerprint, so the sheet is part of what two machines agree on.
    #[test]
    fn the_bonus_is_part_of_the_fingerprint() {
        let d = depth_def();
        let (mut a, mut b) = (pair(&d), pair(&d));
        b.fighters[0].bonus = 2;
        assert_ne!(a.state_hash(), b.state_hash());
        a.fighters[0].bonus = 2;
        assert_eq!(a.state_hash(), b.state_hash());
        b.fighters[1].cursed = true;
        assert_ne!(a.state_hash(), b.state_hash());
    }

    /// `ControlKnight` on the cursed knight: left is right and up is down,
    /// and fire is fire. A cursed knight told to walk away walks in; told
    /// to block, back and down, he lunges, forward and down reversed.
    #[test]
    fn a_cursed_knights_joystick_is_reversed() {
        let d = depth_def();
        let mut b = pair(&d);
        b.fighters[1].cursed = true;
        let away = Intent { dx: 1, dy: 0, attack: false };
        let before = b.fighters[1].x;
        b.step(&d, &[Intent::default(), away]);
        assert!(b.fighters[1].x < before, "told right, went left");
        assert_eq!(b.fighters[1].facing, -1);

        // Forward and up with fire is the up thrust; reversed it is back and
        // down, the block.
        let mut b = pair(&d);
        b.fighters[1].cursed = true;
        let up_thrust = Intent { dx: b.fighters[1].facing, dy: -1, attack: true };
        b.step(&d, &[Intent::default(), up_thrust]);
        assert_eq!(b.fighters[1].attack, Some(Attack::Block), "forward and up came out back and down");
        assert_eq!(b.fighters[1].state, State::Guard);
        let mut sound = pair(&d);
        sound.step(&d, &[Intent::default(), up_thrust]);
        assert_ne!(sound.fighters[1].state, State::Guard, "an uncursed knight does not");
    }

    /// The thrown dagger: `KnifeThrow` takes one off the belt and spawns a
    /// task that flies until it touches somebody, connects once, and is
    /// gone. With no dagger the script goes back to the stance and nothing
    /// is thrown.
    #[test]
    fn a_dagger_flies_and_connects_once() {
        let d = depth_def();
        let mut b = Bout::new(
            arena_field(),
            vec![Fighter::new("k", &d, 100, 100, 1), Fighter::new("k", &d, 220, 100, -1)],
        );
        b.fighters[0].record.set(field::DAGGERS, 2);
        let throw = Intent { dx: -1, dy: -1, attack: true };
        let mut hits = Vec::new();
        let mut flew = Vec::new();
        for t in 0..20 {
            // Fire for the two frames of the throw, then let go: held, it
            // would throw again the moment the script ends.
            let me = if t < 2 { throw } else { Intent::default() };
            hits.extend(b.step(&d, &[me, Intent::default()]));
            if let Some(m) = b.missiles.first() {
                flew.push(m.task.x);
            }
        }
        assert_eq!(b.fighters[0].record.get(field::DAGGERS), 1, "one thrown");
        assert_eq!(flew.len() > 2, true, "it was in the air for a while: {flew:?}");
        assert!(flew.windows(2).all(|w| w[1] > w[0]), "and moving forward: {flew:?}");
        assert_eq!(hits.len(), 1, "it connected once: {hits:?}");
        assert_eq!(hits[0].damage, 18, "the knife's 3 against the swing's 4, of 25");
        assert_eq!(hits[0].attacker, 0, "and the blow is the thrower's");
        assert!(b.missiles.is_empty(), "and is gone once it has");
        assert_eq!(b.fighters[1].health, 82);

        // No dagger, no throw.
        let mut empty = Bout::new(arena_field(), vec![Fighter::new("k", &d, 100, 100, 1)]);
        empty.fighters[0].record.set(field::DAGGERS, 0);
        for _ in 0..6 {
            empty.step(&d, &[throw]);
        }
        assert!(empty.missiles.is_empty());
        assert_eq!(empty.fighters[0].record.get(field::DAGGERS), 0);
    }

    /// A dagger cannot be blocked: its blow lands through `KnightStruck1`,
    /// which never asks `CheckBlock`.
    #[test]
    fn a_dagger_is_not_stopped_by_a_block() {
        let d = depth_def();
        let mut b = Bout::new(
            arena_field(),
            vec![Fighter::new("k", &d, 100, 100, 1), Fighter::new("k", &d, 200, 100, -1)],
        );
        b.fighters[0].record.set(field::DAGGERS, 1);
        let throw = Intent { dx: -1, dy: -1, attack: true };
        let mut hits = 0;
        for _ in 0..20 {
            let intents = [throw, block_for(&b.fighters[1])];
            hits += b.step(&d, &intents).len();
        }
        assert_eq!(hits, 1);
        assert!(b.parries.is_empty());
    }

    /// Kill a knight and swing at the body while it kneels.
    fn kneel_and_strike(bloodless: bool) -> Bout {
        let d = depth_def();
        let mut b = pair(&d);
        b.bloodless = bloodless;
        b.fighters[1].health = 1;
        for _ in 0..40 {
            b.step(&d, &[swing(), Intent::default()]);
        }
        b
    }

    /// The blow-taken script's own `TASKDEAD` picks the death, and a swing on
    /// the kneeling body is the decapitation, whose gated parts show only
    /// with the gore on. Bloodless, the same swing lands, and the script's
    /// `TASKSKIP` turns it into the collapse.
    #[test]
    fn gore_parts_are_gated_by_the_switch() {
        let d = depth_def();
        let gory = kneel_and_strike(false);
        let body = &gory.fighters[1];
        assert_eq!(body.state, State::Dead);
        assert!(!body.alive());
        assert_eq!(body.script, "decap", "a swing on the kneeling body takes the head");
        let shown = &body.task.as_ref().unwrap().shown;
        assert!(shown.iter().any(|p| p.is(part_flags::GATED)), "the gore is drawn: {shown:?}");

        let clean = kneel_and_strike(true);
        let body = &clean.fighters[1];
        assert_eq!(body.state, State::Dead);
        let t = body.task.as_ref().unwrap();
        assert_eq!(t.pc.script, "collapse", "TASKSKIP diverts the decapitation");
        assert!(t.shown.iter().all(|p| !p.is(part_flags::GATED)), "and nothing gated is drawn");
    }

    /// `AddBlood`: a blow on a creature that bleeds spawns `Blood1` at the
    /// strike point on bank table 4, and every part of it is gated.
    #[test]
    fn blood_is_spawned_on_a_bleeder_and_gated() {
        let mut d = depth_def();
        d.bleeds = true;
        for bloodless in [false, true] {
            let mut b = pair(&d);
            b.bloodless = bloodless;
            let mut spawned = None;
            for _ in 0..6 {
                b.step(&d, &[swing(), Intent::default()]);
                if let Some(m) = b.missiles.iter().find(|m| m.attack.is_none()) {
                    spawned = Some(m.clone());
                }
            }
            let m = spawned.expect("a spray");
            assert_eq!(m.task.table, 4, "on the blood bank");
            assert_eq!(m.task.pc.script, "Blood1");
            assert_eq!(!m.task.shown.is_empty(), !bloodless, "bloodless {bloodless}: {:?}", m.task.shown);
            assert!(b.missiles.is_empty(), "and it killed itself");
        }
    }

    /// A body with no `BODY` parts left cannot be struck, and a corpse is
    /// finished once: after the decapitation the task is killed and draws
    /// nothing, as the original's `TASKKILLTASK` takes it out of the table.
    #[test]
    fn a_body_is_finished_once() {
        let d = depth_def();
        let mut b = kneel_and_strike(false);
        let script = b.fighters[1].script.clone();
        for _ in 0..30 {
            b.step(&d, &[swing(), Intent::default()]);
        }
        assert_eq!(b.fighters[1].script, script, "the finish was not restarted");
        assert!(!b.fighters[1].finishable(&d));
    }

    /// The missiles are part of the fingerprint and survive a snapshot: a
    /// bout saved with a dagger in the air agrees with itself afterwards.
    #[test]
    fn a_bout_with_a_dagger_in_flight_survives_a_round_trip() {
        let d = depth_def();
        let mut b = Bout::new(
            arena_field(),
            vec![Fighter::new("k", &d, 60, 100, 1), Fighter::new("k", &d, 260, 100, -1)],
        );
        b.fighters[0].record.set(field::DAGGERS, 3);
        let throw = Intent { dx: -1, dy: -1, attack: true };
        for _ in 0..4 {
            b.step(&d, &[throw, Intent::default()]);
        }
        assert!(!b.missiles.is_empty(), "a dagger is in the air");
        let json = serde_json::to_string(&b).unwrap();
        let mut restored: Bout = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, b);
        assert_eq!(restored.state_hash(), b.state_hash());
        for t in 0..30 {
            let intents = [throw, Intent { dx: -1, dy: 0, attack: false }];
            b.step(&d, &intents);
            restored.step(&d, &intents);
            assert_eq!(restored.state_hash(), b.state_hash(), "tick {t}");
        }
    }
}

#[cfg(test)]
mod mixed {
    use super::*;
    use crate::combat::tests::scripted_def;

    /// One arena's ground. The tree line is put high enough that every
    /// fighter in these tests stands below it and is free to walk.
    fn arena_field() -> Field {
        Field::new(vec![Border { left: 0, right: 319, bottom: 60, top: 10 }])
    }

    /// A troll and a knight in one bout: each fighter is stepped and struck by
    /// its own definition. The creature's blow is its own number; the knight,
    /// whose definition leaves the figure at zero, still deals the bout's.
    #[test]
    fn each_fighter_is_judged_by_its_own_definition() {
        let knight = scripted_def();
        let mut troll = scripted_def();
        troll.health = 300;
        troll.damage = 7;
        troll.depth_tolerance = 20;
        let def_of = |name: &str| if name == "troll" { &troll } else { &knight };

        let mut b = Bout::new(
            arena_field(),
            vec![
                Fighter::new("knight", &knight, 100, 100, 1),
                Fighter::new("troll", &troll, 130, 100, -1),
            ],
        );
        assert_eq!(b.fighters[1].max_health, 300, "built from its own definition");
        assert_eq!(b.fighters[1].damage, 7);

        let mut dealt = std::collections::BTreeMap::new();
        for _ in 0..40 {
            let both = [Intent { dx: 0, dy: 0, attack: true }; 2];
            for e in b.step_with(def_of, &both) {
                dealt.insert(e.attacker, e.damage);
            }
        }
        assert_eq!(dealt.get(&1), Some(&7), "the troll's blow is the troll's");
        assert_eq!(dealt.get(&0), Some(&b.damage), "the knight's is the bout's");
    }

    /// The old one-definition entry point is the same machine with the same
    /// definition handed back for everyone, so nothing that used it changes.
    #[test]
    fn one_definition_for_all_is_the_same_as_the_same_answer_for_each() {
        let d = scripted_def();
        let mk = || {
            Bout::new(
                arena_field(),
                vec![Fighter::new("k", &d, 100, 100, 1), Fighter::new("k", &d, 130, 100, -1)],
            )
        };
        let (mut a, mut b) = (mk(), mk());
        for t in 0..80 {
            let intents = [Intent { dx: 0, dy: 0, attack: t % 3 == 0 }; 2];
            a.step(&d, &intents);
            b.step_with(|_| &d, &intents);
            assert_eq!(a.state_hash(), b.state_hash(), "tick {t}");
        }
    }
}
