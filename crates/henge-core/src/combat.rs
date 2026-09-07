//! One-on-one combat: state machine, movement, and positional hit resolution.
//!
//! No rendering, no assets, no platform. Everything here is driven by content
//! data, so retuning the feel of the game is editing JSON rather than editing
//! Rust, and swapping in our own artwork later changes nothing in this file.

use crate::anim::{Player, Sequence};
use crate::arena::Bounds;
use crate::content::ActorDef;
use crate::taskvm::{self, Effect, Task, TaskActor, FACING_LEFT, FACING_RIGHT};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Idle,
    Walk,
    Attack,
    Hurt,
    Dead,
    /// A block or an evade: an attack kind in the original's tables, played
    /// through `KnightAttack` like any other, but one that carries no weapon
    /// part and exists to be found by `CheckBlock`. Kept apart from `Attack`
    /// so that a held block is not announced as a swing every frame.
    Guard,
    /// The recovery script after a blow has landed on something. The
    /// original replaces the attacker's animation the moment his weapon pile
    /// touches a body (`KnightHitNormal` hands over `+0x12`), so a swing
    /// that connects is cut short rather than carried through.
    Recover,
}

impl State {
    pub fn sequence_name(self) -> &'static str {
        match self {
            State::Idle => "idle",
            State::Walk => "walk",
            State::Attack => "attack",
            State::Hurt => "hurt",
            State::Dead => "death",
            State::Guard => "guard",
            State::Recover => "recover",
        }
    }

    /// While these run, the fighter is committed: no turning, no new attack.
    /// Commitment is what gives a swing weight and makes spacing matter.
    pub fn is_committed(self) -> bool {
        matches!(
            self,
            State::Attack | State::Hurt | State::Dead | State::Guard | State::Recover
        )
    }
}

/// The eight things a knight can do with the button, as `KnightAttSw` holds
/// them: the offset into that table is the attack kind the original keeps in
/// the actor record at `+0x28`, indexes every `*Hit` and `*Dam` table with,
/// and compares in `CheckBlock`.
///
/// **Recovered**, from `SetKnightAnims`: 2 lunge, 4 swing, 6 knife, 8 block,
/// 0xa rear thrust, 0xc up thrust, 0xe evade, 0x10 chop. Slot 0 holds the
/// stance, which is what fire with no direction plays in the original.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Attack {
    Lunge,
    Swing,
    Knife,
    Block,
    RThrust,
    UThrust,
    Evade,
    Chop,
}

impl Attack {
    pub const ALL: [Attack; 8] = [
        Attack::Lunge, Attack::Swing, Attack::Knife, Attack::Block,
        Attack::RThrust, Attack::UThrust, Attack::Evade, Attack::Chop,
    ];

    /// The original's kind code: the byte offset into `KnightAttSw`.
    pub fn kind(self) -> u8 {
        match self {
            Attack::Lunge => 0x02,
            Attack::Swing => 0x04,
            Attack::Knife => 0x06,
            Attack::Block => 0x08,
            Attack::RThrust => 0x0a,
            Attack::UThrust => 0x0c,
            Attack::Evade => 0x0e,
            Attack::Chop => 0x10,
        }
    }

    /// The key the data tables use.
    pub fn name(self) -> &'static str {
        match self {
            Attack::Lunge => "lunge",
            Attack::Swing => "swing",
            Attack::Knife => "knife",
            Attack::Block => "block",
            Attack::RThrust => "rthrust",
            Attack::UThrust => "uthrust",
            Attack::Evade => "evade",
            Attack::Chop => "chop",
        }
    }

    pub fn from_name(name: &str) -> Option<Attack> {
        Attack::ALL.iter().copied().find(|a| a.name() == name)
    }

    /// A block or an evade: no blade out, and a thing `CheckBlock` looks for.
    pub fn is_guard(self) -> bool {
        matches!(self, Attack::Block | Attack::Evade)
    }

    /// The attack the joystick chooses when fire is held with a direction.
    ///
    /// **Recovered.** `KnightAttack` strips the fire bit from the input word,
    /// doubles what is left (1 right, 2 left, 4 down, 8 up) and reads
    /// `Rjoystick` or `Ljoystick` by the knight's facing; the two tables are
    /// each other's mirror, so this is one table over forward and back:
    ///
    /// ```text
    ///              up          level       down
    /// forward      up thrust   swing       lunge
    /// neither      chop        (nothing)   evade
    /// back         knife       rear thrust block
    /// ```
    ///
    /// `forward` is the direction held relative to the facing: 1 towards
    /// where the knight looks, -1 behind him, 0 neither. `dy` is 1 for down.
    /// Fire with no direction is slot 0 of `KnightAttSw`, the stance, so the
    /// original does nothing for it; that is the `None` here, and what a
    /// caller puts in its place is the caller's choice.
    pub fn for_direction(forward: i32, dy: i32) -> Option<Attack> {
        match (forward.signum(), dy.signum()) {
            (1, -1) => Some(Attack::UThrust),
            (1, 0) => Some(Attack::Swing),
            (1, 1) => Some(Attack::Lunge),
            (0, -1) => Some(Attack::Chop),
            (0, 0) => None,
            (0, 1) => Some(Attack::Evade),
            (-1, -1) => Some(Attack::Knife),
            (-1, 0) => Some(Attack::RThrust),
            (-1, 1) => Some(Attack::Block),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Intent {
    pub dx: i32,
    pub dy: i32,
    pub attack: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Fighter {
    pub actor: String,
    pub x: i32,
    pub y: i32,
    /// Facing right is +1, left is -1.
    pub facing: i32,
    pub state: State,
    pub player: Player,
    pub health: i32,
    pub max_health: i32,
    /// One connect per swing, however many frames carry a hit line.
    pub struck: bool,
    /// What this fighter's blow takes off, or zero for the bout's own figure.
    /// Copied from the definition so a caller fighting at another scale can
    /// move it with the health, the way it moves the bout's.
    #[serde(default)]
    pub damage: i32,
    /// The running task, for an actor animated by the recovered VM. `None`
    /// until the first tick of a state, and always for an actor with no scripts.
    #[serde(default)]
    pub task: Option<Task>,
    /// The actor record the VM reads and writes: hit points for `TASKDEAD`,
    /// and whatever `TASKSAVE` puts there.
    #[serde(default)]
    pub record: TaskActor,
    /// Which of the current state's scripts is playing. A walk is a cycle of
    /// four one-frame scripts, and this is the place in that cycle.
    #[serde(default)]
    pub cycle: usize,
    /// Ticks since the task last stepped, against `ActorDef::script_ticks`.
    #[serde(default)]
    pub script_tick: u32,
    /// What the script asked for on this tick and the interpreter would not do
    /// itself: sounds, spawns, and calls into the original's own code. Kept on
    /// the fighter so it serializes with everything else rather than being
    /// handed out through a channel the simulation would have to know about.
    #[serde(default)]
    pub effects: Vec<Effect>,
    /// The attack kind the fighter is doing, the original's `+0x28`. `None`
    /// while standing or walking, which the original keeps as a zero and
    /// `CheckBlock` reads as such.
    #[serde(default)]
    pub attack: Option<Attack>,
    /// The script the current state was entered on, where a state has a
    /// choice: the attack the joystick picked, the blow-taken script the
    /// attacker's kind picked, the finish a corpse was given. Empty for the
    /// states that cycle their own list.
    #[serde(default)]
    pub script: String,
    /// The evade's one use, bit 7 of `+0x48`: an evade stops one blow, and
    /// the bit stays set until the knight walks, which `ControlKnight` clears
    /// it on. Without this a held evade would be a wall.
    #[serde(default)]
    pub evaded: bool,
    /// The state changed and the task has not yet been handed the new
    /// state's script. The old frame stays on screen until it has, the way
    /// the original's `REPLACEANIM` swaps one script for the next between
    /// two draws; dropping the task instead left a fighter invisible for
    /// the one tick between a blow and the recoil.
    #[serde(default)]
    pub restart: bool,
}

impl Fighter {
    /// A zeroed definition, only for building a Fighter before the real
    /// definitions are in hand. Never simulated.
    pub fn placeholder_def() -> ActorDef {
        ActorDef {
            sheet: String::new(), health: 1, speed_x: 0, speed_y: 0,
            reach: 0, depth_tolerance: 0, attack_cooldown: 0,
            bounty: 0, girth: 0,
            body: [0; 4], sequences: Default::default(),
            ..ActorDef::default()
        }
    }

    pub fn new(actor: impl Into<String>, def: &ActorDef, x: i32, y: i32, facing: i32) -> Fighter {
        let mut f = Fighter {
            actor: actor.into(),
            x, y, facing,
            state: State::Idle,
            player: Player::default(),
            health: def.health,
            max_health: def.health,
            struck: false,
            damage: def.damage,
            task: None,
            record: TaskActor::with_health(def.health),
            cycle: 0,
            script_tick: 0,
            effects: Vec::new(),
            attack: None,
            script: String::new(),
            evaded: false,
            restart: false,
        };
        // Run the first frame of the standing script now, so a fighter is
        // visible before anything has ticked. Without it a screenshot taken at
        // tick zero shows an empty arena, and a task that has never stepped has
        // nothing to draw.
        if def.scripted() {
            if let Some(first) = def.scripts_for(State::Idle.sequence_name()).first() {
                let facing = if facing < 0 { FACING_LEFT } else { FACING_RIGHT };
                let mut task =
                    Task::new(first, x + def.origin[0] as i32, y + def.origin[1] as i32, facing);
                task.table = def.bank_table;
                task.step(&def.animation, &mut f.record, false);
                f.task = Some(task);
            }
        }
        f
    }

    /// Standing, or at least not yet down. A fighter whose hit points are
    /// gone is dead from the moment of the blow, even while the blow-taken
    /// script is still deciding how he falls.
    pub fn alive(&self) -> bool {
        self.state != State::Dead && self.health > 0
    }

    /// Down, but with a body still on the pile: the frame being shown carries
    /// `BODY` parts, so a blow can still find it. The knight kneels for twenty
    /// frames of `Knight_SwDeath` with two of them before the collapse takes
    /// them away, and that is the window a finisher has.
    pub fn finishable(&self, def: &ActorDef) -> bool {
        self.state == State::Dead
            && !def.finishes.is_empty()
            && self.task.as_ref().map_or(false, |t| {
                t.active && t.shown.iter().any(|p| p.is(taskvm::part_flags::BODY))
            })
    }

    /// Holding a block or an evade.
    pub fn guarding(&self) -> Option<Attack> {
        if self.state == State::Guard { self.attack } else { None }
    }

    fn enter(&mut self, state: State) {
        if self.state == state {
            return;
        }
        self.state = state;
        self.player.restart();
        self.struck = false;
        // A new state starts its script cycle from the beginning, and asks
        // for the task to be put on the new state's script at the next tick.
        self.cycle = 0;
        self.script_tick = 0;
        self.restart = true;
        self.script.clear();
        if !matches!(state, State::Attack | State::Guard) {
            self.attack = None;
        }
    }

    /// Enter a state on a particular script of it.
    fn enter_on(&mut self, state: State, script: String) {
        if self.state == state && self.script == script {
            // The same thing again: a held block is replayed frame by frame
            // in the original, and here it simply carries on.
            return;
        }
        if self.state == state {
            // A different script of the same state, which `enter` would
            // treat as nothing to do.
            self.state = State::Idle;
        }
        self.enter(state);
        self.script = script;
    }

    /// Has the animation of the current state run out?
    ///
    /// For a scripted actor this is the original's own `task+1`, which the end
    /// of frame handler clears on `ff ff`; for one animated from a frame list
    /// it is the player reaching the end. A committed state ends when this
    /// does, which is what makes a swing something you cannot walk out of.
    fn animation_done(&self, def: &ActorDef) -> bool {
        if !def.scripted() {
            return self.player.finished;
        }
        // A state entered from outside a tick, which is what taking a blow
        // is, has not had its animation begin, and something that has not
        // begun has not finished. Reading it as finished is what would make a
        // recoil last exactly one tick.
        if self.restart {
            return self.script.is_empty() && def.scripts_for(self.state.sequence_name()).is_empty();
        }
        match &self.task {
            Some(t) => !t.running,
            None => def.scripts_for(self.state.sequence_name()).is_empty(),
        }
    }

    pub fn sequence<'a>(&self, def: &'a ActorDef) -> Option<&'a Sequence> {
        def.sequence(self.state.sequence_name())
    }

    /// One tick. Returns the hit line this fighter is sweeping, in world space,
    /// if the current frame carries one and it has not already connected.
    pub fn step(&mut self, def: &ActorDef, intent: Intent, bounds: Bounds) -> Vec<(i32, i32)> {
        self.step_gated(def, intent, bounds, false)
    }

    /// One tick, with the gore switch. `bloodless` is the original's
    /// DS:0x700, which the title screen toggles: set, every part flagged
    /// `GATED` is dropped and every `TASKSKIP` is taken, which is how the
    /// decapitation turns into a collapse.
    pub fn step_gated(
        &mut self,
        def: &ActorDef,
        intent: Intent,
        bounds: Bounds,
        bloodless: bool,
    ) -> Vec<(i32, i32)> {
        self.effects.clear();
        // The dead, and the dying. A scripted fighter whose hit points are
        // gone is still on his blow-taken script, whose own `TASKDEAD` picks
        // the death: the same recoil ends in a fall for a stab and a split
        // for a cut, because that is where each script's branch goes. The
        // state follows the script rather than the other way round.
        let dying = self.state == State::Hurt && self.health <= 0 && def.scripted();
        if self.state == State::Dead || dying {
            if def.scripted() {
                self.run_task(def, bounds, bloodless);
                if dying {
                    let (branched, ended) = self
                        .task
                        .as_ref()
                        .map_or((false, true), |t| (t.pc.script != self.script, !t.running));
                    if branched {
                        // The script chose the death; keep the task on it.
                        self.state = State::Dead;
                    } else if ended {
                        // A recoil with no branch of its own: the plain death.
                        self.enter(State::Dead);
                    }
                }
            } else if let Some(seq) = self.sequence(def) {
                self.player.advance(seq);
            }
            return Vec::new();
        }

        // A committed action runs to completion before input is looked at
        // again. It is looked at on the tick the action ends, the way the
        // original's controller runs on the frame `task+1` clears, which is
        // what lets a held block stay up with no gap in it.
        if self.state.is_committed() && self.animation_done(def) {
            self.enter(State::Idle);
        }
        if self.state.is_committed() {
            // still busy
        } else if intent.attack {
            // `KnightAttack`: the direction held with fire, relative to the
            // facing, picks the attack. Fire alone is the stance in the
            // original, which is to say nothing; here it is the swing, so
            // that one button still fights, and so that the plain opponent,
            // which only knows one button, does too.
            let forward = intent.dx * self.facing;
            let wanted = Attack::for_direction(forward, intent.dy).unwrap_or(Attack::Swing);
            match def.attack_for(wanted) {
                Some((script, kind)) => {
                    let state = if kind.is_guard() { State::Guard } else { State::Attack };
                    self.enter_on(state, script);
                    self.attack = Some(kind);
                }
                None => {
                    // A frame-list actor: one attack, and no kinds to choose
                    // between.
                    self.enter(State::Attack);
                    self.attack = Some(Attack::Swing);
                }
            }
        } else if intent.dx != 0 || intent.dy != 0 {
            if intent.dx != 0 {
                self.facing = intent.dx.signum();
            }
            self.enter(State::Walk);
            // Walking is what gives the evade its one use back.
            self.evaded = false;
        } else {
            self.enter(State::Idle);
        }

        // Free movement only while not committed. Committed frames may still
        // carry the actor via their own dx/dy.
        if !self.state.is_committed() && self.state == State::Walk {
            let (x, y) = bounds.clamp(
                self.x + intent.dx * def.speed_x,
                self.y + intent.dy * def.speed_y,
            );
            self.x = x;
            self.y = y;
        }

        if def.scripted() {
            return self.run_task(def, bounds, bloodless);
        }

        let Some(seq) = def.sequence(self.state.sequence_name()) else {
            return Vec::new();
        };
        let frame = self.player.current(seq).cloned();
        self.player.advance(seq);

        let Some(frame) = frame else { return Vec::new() };
        if frame.dx != 0 || frame.dy != 0 {
            let (x, y) = bounds.clamp(
                self.x + frame.dx as i32 * self.facing,
                self.y + frame.dy as i32,
            );
            self.x = x;
            self.y = y;
        }

        if self.state != State::Attack || self.struck || frame.hit.is_empty() {
            return Vec::new();
        }
        frame
            .hit
            .iter()
            .map(|[hx, hy]| (self.x + *hx as i32 * self.facing, self.y - *hy as i32))
            .collect()
    }

    /// One tick of the task VM, for an actor animated by the recovered scripts.
    ///
    /// The task's position is the fighter's, shifted by the actor's origin,
    /// because the original places parts against a point near the top of the
    /// figure and this engine positions everything by the feet. Anything the
    /// script does to that position with `TASKMOVE` is carried back out, so a
    /// script that walks itself walks the fighter.
    fn run_task(&mut self, def: &ActorDef, bounds: Bounds, bloodless: bool) -> Vec<(i32, i32)> {
        // The state's own list, or the one script the state was entered on.
        let names: Vec<String> = if self.script.is_empty() {
            def.scripts_for(self.state.sequence_name()).to_vec()
        } else {
            vec![self.script.clone()]
        };
        if names.is_empty() {
            return Vec::new();
        }
        let facing = if self.facing < 0 { FACING_LEFT } else { FACING_RIGHT };
        let (ox, oy) = (def.origin[0] as i32, def.origin[1] as i32);

        let mut step_now = false;
        match &mut self.task {
            Some(t) if self.restart => {
                // The new state's script, `REPLACEANIM`: the VM state is
                // zeroed and the first frame is stepped now.
                t.replace(names[0].clone());
                t.table = def.bank_table;
                self.restart = false;
                self.script_tick = 0;
                step_now = true;
            }
            None => {
                let mut task = Task::new(&names[0], self.x + ox, self.y + oy, facing);
                task.table = def.bank_table;
                self.task = Some(task);
                self.restart = false;
                self.script_tick = 0;
                step_now = true;
            }
            Some(t) => {
                self.script_tick += 1;
                if self.script_tick >= def.script_ticks.max(1) {
                    self.script_tick = 0;
                    step_now = true;
                    // The script ended. A state with more than one script
                    // is a cycle, and this is where the next one is handed
                    // over, which is what `TASKHANDLE` does when it sees
                    // `task+1` cleared. A death is not handed anything: it
                    // ended, and a corpse that replays its fall is a corpse
                    // that will not lie still.
                    if !t.running && self.state != State::Dead {
                        self.cycle = (self.cycle + 1) % names.len();
                        let next = names[self.cycle].clone();
                        t.replace(next);
                    }
                }
            }
        }

        let Some(task) = self.task.as_mut() else { return Vec::new() };
        task.facing = facing;
        task.x = self.x + ox;
        task.y = self.y + oy;
        if step_now {
            self.record.set_health(self.health);
            let frame = task.step(&def.animation, &mut self.record, bloodless);
            self.effects = frame.effects;
            // Whatever the script moved, the fighter moved.
            let (nx, ny) = bounds.clamp(task.x - ox, task.y - oy);
            self.x = nx;
            self.y = ny;
            task.x = nx + ox;
            task.y = ny + oy;
        }

        if self.state != State::Attack || self.struck {
            return Vec::new();
        }
        self.hit_line(def)
    }

    /// The shape a swing sweeps, taken from the frame's own weapon parts.
    ///
    /// The original does not carry a hit line for a knight at all: it pushes
    /// every part flagged `WEAPON` onto `WeoponPile` and every part flagged
    /// `BODY` onto `BodyPile`, and the collision code walks one against the
    /// other. That distinction is in the data and it is sharper than a hand
    /// authored line: the knight's stance carries his sword as part of his
    /// body, and only the swing marks it a weapon, so a man standing still
    /// cannot cut you by holding a blade out.
    ///
    /// What is approximated here is the granularity. Rather than pile against
    /// pile, each weapon cel's own rectangle is swept as a polyline against the
    /// target's body box, which is the same shape at cel resolution.
    fn hit_line(&self, def: &ActorDef) -> Vec<(i32, i32)> {
        let Some(task) = self.task.as_ref() else { return Vec::new() };
        let mut out = Vec::new();
        for p in task.shown.iter().filter(|p| p.is(taskvm::part_flags::WEAPON)) {
            let Some(bank) = def.bank(p.table, p.bank) else { continue };
            let Some(r) = taskvm::place(p, bank, (task.x, task.y, task.z), task.mirror()) else {
                continue;
            };
            let (l, t) = (r.x, r.y);
            let (rr, b) = (r.x + r.w as i32, r.y + r.h as i32);
            out.extend([(l, t), (rr, t), (rr, b), (l, b), (l, t)]);
        }
        out
    }

    /// Body rectangle in world space: (left, top, right, bottom), screen axes.
    pub fn body(&self, def: &ActorDef) -> (i32, i32, i32, i32) {
        let [x0, y0, x1, y1] = def.body;
        let (a, b) = (x0 as i32 * self.facing, x1 as i32 * self.facing);
        (
            self.x + a.min(b),
            self.y - y1 as i32,
            self.x + a.max(b),
            self.y - y0 as i32,
        )
    }

    pub fn take_hit(&mut self, damage: i32) {
        if self.state == State::Dead {
            return;
        }
        self.health -= damage;
        if self.health <= 0 {
            self.health = 0;
            self.enter(State::Dead);
        } else {
            self.enter(State::Hurt);
        }
    }

    /// A blow of a particular kind, the way `KnightSAnim` and `TroggStruck`
    /// deal one: the blow-taken script is the `*Hit` entry for the attacker's
    /// kind, and that script's own `TASKDEAD` decides whether the fighter
    /// gets up or which way he goes down. A frame-list actor has neither
    /// table and takes the blow the plain way.
    pub fn struck(&mut self, def: &ActorDef, damage: i32, by: Option<Attack>) {
        if !self.alive() {
            return;
        }
        let hurt = if def.scripted() { def.hurt_for(by) } else { None };
        self.health -= damage;
        match hurt {
            Some(script) => {
                if self.health < 0 {
                    self.health = 0;
                }
                // Struck again while reeling: the script starts over, which
                // `enter_on` would decline to do for the same script.
                self.state = State::Idle;
                self.enter_on(State::Hurt, script);
            }
            None => {
                if self.health <= 0 {
                    self.health = 0;
                    self.enter(State::Dead);
                } else {
                    self.enter(State::Hurt);
                }
            }
        }
    }

    /// A blow on a body that is already down: the corpse is given the
    /// finish. `MudmenStruck1`, the path every creature's and a thrown
    /// dagger's blow on a fallen knight takes, decapitates for a swing and
    /// collapses him for anything else; `KnightKnightStruck1` decapitates
    /// for any blow, so a knight's is any blow from the same kind of
    /// fighter. Whether the head actually comes off is the script's own
    /// `TASKSKIP`, which the gore switch decides.
    pub fn finish(&mut self, def: &ActorDef, by: Option<Attack>, by_own_kind: bool) -> bool {
        if !self.finishable(def) {
            return false;
        }
        let which = if by == Some(Attack::Swing) || by_own_kind { "decap" } else { "collapse" };
        let Some(script) = def.finishes.get(which).cloned() else { return false };
        if let Some(t) = self.task.as_mut() {
            t.replace(script.clone());
        }
        self.script = script;
        true
    }

    /// `CheckBlock`. Does what the defender is holding stop this blow?
    ///
    /// **Recovered.** The defender's block table is read at the attacker's
    /// kind, and the entry has to equal the defender's own kind: `Block` for a
    /// swing, `Evade` for a chop, a lunge or a rear thrust. An evade stops one
    /// blow and is then spent until the defender walks; a block only works
    /// against a blow from the front, which is to say when the two are not
    /// facing the same way. A kind with no entry in the table is a zero, and
    /// a zero is also what a knight who is doing nothing holds, so an up
    /// thrust from the front is stopped by a knight standing still and lands
    /// only on one who is mid-swing or turned away. That reads as a quirk and
    /// is reproduced, because it is what the code does.
    ///
    /// Returns whether the blow was stopped, and marks the evade spent when
    /// it was the evade that stopped it. What the original does not do is
    /// consult the kind a reeling or recovering knight is left holding; here
    /// those states never block, which is a simplification.
    pub fn blocks(&mut self, def: &ActorDef, attacker_facing: i32, attack: Attack) -> bool {
        if def.blocks.is_empty() {
            return false;
        }
        let holding = match self.state {
            State::Guard | State::Attack => self.attack,
            State::Idle | State::Walk => None,
            _ => return false,
        };
        let wanted = def.blocks.get(attack.name()).and_then(|g| Attack::from_name(g));
        if wanted != holding {
            return false;
        }
        if wanted == Some(Attack::Evade) {
            if self.evaded {
                return false;
            }
            self.evaded = true;
            return true;
        }
        self.facing != attacker_facing
    }

    /// The moment the weapon touches something, the swing is over and the
    /// recovery script takes its place, unless the actor has none to give.
    /// `KnightHitNormal` keeps the swing going for an up thrust; `KnightHitKnight`
    /// for a blow on a knight who is evading, and for a swing on a fallen knight
    /// with the gore on, which is the swing that takes the head.
    pub fn recover(&mut self, def: &ActorDef) {
        if let Some(script) = def.scripts_for("recover").first().cloned() {
            self.enter_on(State::Recover, script);
        }
    }

    /// The body a blow can find on a fighter that is down: the union of the
    /// `BODY` parts of the frame being shown, placed where they are drawn.
    /// The original walks exactly this pile; the living use `body`, the
    /// authored box, because that is what makes a strike something to aim.
    pub fn corpse_body(&self, def: &ActorDef) -> Option<(i32, i32, i32, i32)> {
        let task = self.task.as_ref()?;
        let mut out: Option<(i32, i32, i32, i32)> = None;
        for p in task.shown.iter().filter(|p| p.is(taskvm::part_flags::BODY)) {
            let Some(bank) = def.bank(p.table, p.bank) else { continue };
            let Some(r) = taskvm::place(p, bank, (task.x, task.y, task.z), task.mirror()) else {
                continue;
            };
            let (l, t, rr, b) = (r.x, r.y, r.x + r.w as i32, r.y + r.h as i32);
            out = Some(match out {
                None => (l, t, rr, b),
                Some((ol, ot, or, ob)) => (ol.min(l), ot.min(t), or.max(rr), ob.max(b)),
            });
        }
        out
    }

    /// Depth key. Everything in an arena sorts by where its feet are.
    pub fn depth(&self) -> i32 {
        self.y
    }
}

/// Does a swept hit line cross a body rectangle?
pub fn line_hits_body(line: &[(i32, i32)], body: (i32, i32, i32, i32)) -> bool {
    let (l, t, r, b) = body;
    if line.is_empty() {
        return false;
    }
    // A single point still counts, which matters for a thrust.
    if line.len() == 1 {
        let (x, y) = line[0];
        return x >= l && x <= r && y >= t && y <= b;
    }
    line.windows(2).any(|w| segment_hits_rect(w[0], w[1], l, t, r, b))
}

fn segment_hits_rect(a: (i32, i32), b: (i32, i32), l: i32, t: i32, r: i32, bo: i32) -> bool {
    let inside = |(x, y): (i32, i32)| x >= l && x <= r && y >= t && y <= bo;
    if inside(a) || inside(b) {
        return true;
    }
    // Otherwise the segment must cross one of the four edges.
    let edges = [
        ((l, t), (r, t)),
        ((r, t), (r, bo)),
        ((r, bo), (l, bo)),
        ((l, bo), (l, t)),
    ];
    edges.iter().any(|(p, q)| segments_cross(a, b, *p, *q))
}

fn segments_cross(p1: (i32, i32), p2: (i32, i32), p3: (i32, i32), p4: (i32, i32)) -> bool {
    let d = |a: (i32, i32), b: (i32, i32), c: (i32, i32)| -> i64 {
        (b.0 as i64 - a.0 as i64) * (c.1 as i64 - a.1 as i64)
            - (b.1 as i64 - a.1 as i64) * (c.0 as i64 - a.0 as i64)
    };
    let (d1, d2, d3, d4) = (d(p3, p4, p1), d(p3, p4, p2), d(p1, p2, p3), d(p1, p2, p4));
    ((d1 > 0 && d2 < 0) || (d1 < 0 && d2 > 0)) && ((d3 > 0 && d4 < 0) || (d3 < 0 && d4 > 0))
}

/// A deliberately plain opponent, but one a person can fight.
///
/// The first version closed at full speed and swung on every cooldown, which
/// against a human on twenty health meant being pinned in place and cut down
/// without a chance to answer: equal speed meant no getting away, and every
/// hit froze the player for the next. This one has a rhythm that can be read
/// and exploited, which is what makes a fight a game rather than a countdown.
///
/// * It closes at two thirds of walking speed, so the player can always make
///   space by backing off.
/// * After a swing it steps back for a moment before coming in again.
/// * Every so often it hesitates, which is the opening to attack it.
///
/// `clock` is any counter that advances once a tick. Real behaviour, per
/// creature, comes with the bestiary; this exists so combat can be felt.
///
/// One thing here is the original's rather than ours: with the gore on, a
/// creature that has put a knight down comes in for one more blow while he
/// is still kneeling (`TroggAttack`, on `DeCapFLAG`, inside a hundred
/// pixels), which is the swing that takes the head. `gore` is that switch,
/// and `foe` may be a body for as long as it can still be struck.
pub fn simple_ai(
    me: &Fighter,
    foe: &Fighter,
    def: &ActorDef,
    cooldown: &mut i32,
    clock: i32,
    gore: bool,
) -> Intent {
    if !me.alive() {
        return Intent::default();
    }
    *cooldown = (*cooldown - 1).max(0);
    let dx = foe.x - me.x;
    let dy = foe.y - me.y;
    let reach = def.reach;
    let toward = dx.signum();
    let level = dy.abs() <= def.depth_tolerance;

    if !foe.alive() {
        // The finisher, or nothing.
        let body = foe.state == State::Dead
            && foe.task.as_ref().map_or(false, |t| {
                t.active && t.shown.iter().any(|p| p.is(taskvm::part_flags::BODY))
            });
        if !gore || !body || *cooldown > 0 {
            return Intent::default();
        }
        if dx.abs() <= reach && level {
            *cooldown = def.attack_cooldown;
            return Intent { dx: 0, dy: 0, attack: true };
        }
        return Intent { dx: if dx.abs() > reach - 4 { toward } else { 0 }, dy: if !level { dy.signum() } else { 0 }, attack: false };
    }

    // Recovering from a swing: back off, then wait it out.
    if *cooldown > 0 {
        let backing = *cooldown > def.attack_cooldown - 14;
        return Intent { dx: if backing { -toward } else { 0 }, dy: 0, attack: false };
    }

    if dx.abs() <= reach && level {
        *cooldown = def.attack_cooldown;
        return Intent { dx: 0, dy: 0, attack: true };
    }

    // A pause in the approach, every couple of seconds, that a watching
    // player can learn to use.
    if clock.rem_euclid(150) < 30 {
        return Intent::default();
    }

    Intent {
        dx: if dx.abs() > reach - 4 && clock % 3 != 0 { toward } else { 0 },
        dy: if !level { dy.signum() } else { 0 },
        attack: false,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::anim::{EndBehaviour, Frame};
    use crate::content::ActorDef;
    use crate::taskvm::{part_flags, Bank, End, Instr, Part, Script, ScriptSet};
    use std::collections::BTreeMap;

    /// An actor shaped like the baked knight: a stance, a four script walk
    /// cycle, a swing whose middle frame carries a weapon cel and which ends by
    /// going back to the stance, and a recoil that turns into a death when the
    /// hit points have run out.
    pub(crate) fn scripted_def() -> ActorDef {
        let body = |cel: u8| {
            Instr::Part(Part { table: 1, bank: 0, cel, x: -8, y: 0, flags: part_flags::BODY })
        };
        let stop = Instr::EndFrame { end: End::Stop };
        let next = Instr::EndFrame { end: End::Next };
        let mut animation = ScriptSet::new();
        animation.insert("stance".into(), Script::new(vec![body(0), stop.clone()]));
        for (i, n) in ["walk1", "walk2", "walk3", "walk4"].iter().enumerate() {
            animation.insert(n.to_string(), Script::new(vec![body(1 + i as u8), stop.clone()]));
        }
        animation.insert(
            "swing".into(),
            Script::new(vec![
                Instr::Sound { sample: 0x0b },
                Instr::Gosub { routine: "KnightGruntSound".into() },
                body(6),
                next.clone(),
                body(7),
                Instr::Part(Part {
                    table: 1, bank: 0, cel: 8, x: 10, y: 20, flags: part_flags::WEAPON,
                }),
                next.clone(),
                Instr::Goto { mode: 0, target: "stance".into() },
                body(6),
                stop.clone(),
            ]),
        );
        animation.insert(
            "recoil".into(),
            Script::new(vec![
                Instr::Hold { count: 4 },
                body(9),
                next.clone(),
                Instr::Dead { target: "fall".into() },
                body(9),
                stop.clone(),
            ]),
        );
        animation.insert(
            "fall".into(),
            Script::new(vec![
                Instr::Part(Part { table: 1, bank: 0, cel: 10, x: -20, y: 40, flags: 0 }),
                stop.clone(),
            ]),
        );

        let mut cels = vec![[16u16, 52u16]; 11];
        cels[8] = [30, 10]; // the blade
        let mut scripts = BTreeMap::new();
        scripts.insert("idle".into(), vec!["stance".to_string()]);
        scripts.insert(
            "walk".into(),
            ["walk1", "walk2", "walk3", "walk4"].iter().map(|s| s.to_string()).collect(),
        );
        scripts.insert("attack".into(), vec!["swing".to_string()]);
        scripts.insert("hurt".into(), vec!["recoil".to_string()]);
        scripts.insert("death".into(), vec!["fall".to_string()]);

        ActorDef {
            sheet: "test".into(),
            health: 100,
            speed_x: 2,
            speed_y: 1,
            reach: 40,
            depth_tolerance: 6,
            attack_cooldown: 30,
            body: [-9, 0, 9, 52],
            origin: [0, -52],
            script_ticks: 1,
            animation,
            scripts,
            banks: BTreeMap::from([(1u8, vec![Bank { sheet: "test".into(), base: 0, cels }])]),
            ..ActorDef::default()
        }
    }

    /// The scripted actor with the combat depth tables on it: the eight
    /// attacks by kind, a block table, a recovery, a kneeling death with a
    /// body still on it, the two finishes, the dagger's scripts and a blood
    /// spray on bank table 4. Shaped like the baked knight, in miniature.
    pub(crate) fn depth_def() -> ActorDef {
        use crate::content::AttackDef;
        use crate::taskvm::field;
        let mut d = scripted_def();
        let body = |cel: u8| {
            Instr::Part(Part { table: 1, bank: 0, cel, x: -8, y: 0, flags: part_flags::BODY })
        };
        let blade = Instr::Part(Part {
            table: 1, bank: 0, cel: 8, x: 10, y: 20, flags: part_flags::WEAPON,
        });
        let stop = Instr::EndFrame { end: End::Stop };
        let next = Instr::EndFrame { end: End::Next };
        let a = &mut d.animation;
        // One frame with the blade out, held for two, so a chop can be told
        // from a swing by its script and lands on its first frame.
        a.insert("chop".into(), Script::new(vec![Instr::Hold { count: 2 }, body(6), blade.clone(), stop.clone()]));
        a.insert("block".into(), Script::new(vec![body(9), stop.clone()]));
        a.insert("evade".into(), Script::new(vec![body(9), stop.clone()]));
        a.insert("recover".into(), Script::new(vec![body(9), stop.clone()]));
        // The throw: nothing without a dagger, else the call that spawns one.
        a.insert(
            "throw".into(),
            Script::new(vec![
                Instr::TestEq { mode: 1, field: field::DAGGERS, target: "stance".into() },
                body(6),
                next.clone(),
                Instr::Gosub { routine: "KnifeThrow".into() },
                body(7),
                stop.clone(),
            ]),
        );
        a.insert(
            "SpeedKnife".into(),
            Script::new(vec![
                Instr::Sound { sample: 0x0b },
                Instr::Move { flags: 0x01, x: 5, y: 0, z: 0 },
                blade.clone(),
                stop.clone(),
            ]),
        );
        a.insert(
            "Knife".into(),
            Script::new(vec![Instr::Move { flags: 0x01, x: 20, y: 0, z: 0 }, blade.clone(), stop.clone()]),
        );
        // The death kneels with a body on it for ten frames, then lies flat
        // with none, the way `Knight_SwDeath` does before its collapse.
        a.insert(
            "fall".into(),
            Script::new(vec![
                Instr::Hold { count: 10 },
                Instr::Part(Part { table: 1, bank: 0, cel: 10, x: -20, y: 20, flags: part_flags::BODY }),
                next.clone(),
                Instr::Part(Part { table: 1, bank: 0, cel: 10, x: -20, y: 40, flags: 0 }),
                stop.clone(),
            ]),
        );
        a.insert(
            "decap".into(),
            Script::new(vec![
                Instr::Skip { target: "collapse".into() },
                Instr::Part(Part { table: 1, bank: 0, cel: 5, x: 0, y: -10, flags: part_flags::GATED }),
                Instr::Part(Part { table: 1, bank: 0, cel: 10, x: -20, y: 40, flags: 0 }),
                stop.clone(),
            ]),
        );
        a.insert(
            "collapse".into(),
            Script::new(vec![Instr::Part(Part { table: 1, bank: 0, cel: 10, x: -20, y: 40, flags: 0 }), stop.clone()]),
        );
        a.insert(
            "Blood1".into(),
            Script::new(vec![
                Instr::Part(Part { table: 4, bank: 4, cel: 0, x: -5, y: -5, flags: part_flags::GATED }),
                next.clone(),
                Instr::KillTask,
                stop.clone(),
            ]),
        );
        for (name, script, damage) in [
            ("swing", "swing", 4),
            ("chop", "chop", 8),
            ("knife", "throw", 3),
            ("block", "block", 0),
            ("evade", "evade", 0),
        ] {
            d.attacks.insert(name.into(), AttackDef { script: script.into(), damage });
        }
        d.attack = "swing".into();
        d.scripts.insert("recover".into(), vec!["recover".into()]);
        d.blocks = [("swing", "block"), ("chop", "evade"), ("lunge", "evade")]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        d.blockable = true;
        d.finishes = [("decap", "decap"), ("collapse", "collapse")]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let blood = Bank { sheet: "blood".into(), base: 0, cels: vec![[12, 12]; 4] };
        d.banks.insert(4, vec![blood; 5]);
        assert_eq!(d.validate(), Ok(()));
        d
    }

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
            sheet: "test".into(),
            health: 100,
            speed_x: 2,
            speed_y: 1,
            reach: 40,
            depth_tolerance: 6,
            attack_cooldown: 30,
            bounty: 0,
            girth: 0,
            body: [-9, 0, 9, 52],
            sequences,
            ..ActorDef::default()
        }
    }

    fn bounds() -> Bounds {
        Bounds { left: 0, right: 319, top: 10, bottom: 114 }
    }

    #[test]
    fn walking_moves_and_turns_the_fighter() {
        let d = def();
        let mut f = Fighter::new("a", &d, 100, 100, 1);
        f.step(&d, Intent { dx: -1, dy: 0, attack: false }, bounds());
        assert_eq!(f.facing, -1);
        assert_eq!(f.x, 98);
        assert_eq!(f.state, State::Walk);
    }

    #[test]
    fn an_attack_commits_and_ignores_input_until_it_finishes() {
        let d = def();
        let mut f = Fighter::new("a", &d, 100, 100, 1);
        f.step(&d, Intent { dx: 0, dy: 0, attack: true }, bounds());
        assert_eq!(f.state, State::Attack);
        // Trying to walk away mid-swing does nothing.
        f.step(&d, Intent { dx: 1, dy: 0, attack: false }, bounds());
        assert_eq!(f.state, State::Attack);
        assert_eq!(f.x, 100);
    }

    /// Both attack frames carry a hit line. Once the arena reports a connect,
    /// the rest of that swing must stay inert, or a single strike would deal
    /// damage twice.
    #[test]
    fn a_swing_connects_once_however_many_frames_carry_the_line() {
        let d = def();
        let mut f = Fighter::new("a", &d, 100, 100, 1);
        let mut lines = 0;
        // One swing only: stop as soon as the sequence completes.
        for _ in 0..8 {
            if !f.step(&d, Intent { dx: 0, dy: 0, attack: true }, bounds()).is_empty() {
                lines += 1;
                f.struck = true; // what the arena does once a hit lands
            }
            if f.player.finished {
                break;
            }
        }
        assert_eq!(lines, 1, "two frames carry a line but only one connect is allowed");
    }

    /// A swing that misses must not disarm the rest of the swing.
    #[test]
    fn a_missed_swing_keeps_offering_its_line() {
        let d = def();
        let mut f = Fighter::new("a", &d, 100, 100, 1);
        let mut lines = 0;
        for _ in 0..8 {
            if !f.step(&d, Intent { dx: 0, dy: 0, attack: true }, bounds()).is_empty() {
                lines += 1; // nothing was hit, so `struck` stays false
            }
            if f.player.finished {
                break;
            }
        }
        assert_eq!(lines, 2);
    }

    /// Releasing and pressing again is a new swing, which may connect again.
    #[test]
    fn a_second_swing_can_connect_again() {
        let d = def();
        let mut f = Fighter::new("a", &d, 100, 100, 1);
        let mut swings = 0;
        for _ in 0..24 {
            if !f.step(&d, Intent { dx: 0, dy: 0, attack: true }, bounds()).is_empty() {
                swings += 1;
                f.struck = true;
            }
        }
        assert!(swings >= 2, "holding attack keeps swinging, got {swings}");
    }

    #[test]
    fn the_hit_line_mirrors_with_facing() {
        let d = def();
        let mut right = Fighter::new("a", &d, 100, 100, 1);
        let mut left = Fighter::new("a", &d, 100, 100, -1);
        let a = loop {
            let l = right.step(&d, Intent { dx: 0, dy: 0, attack: true }, bounds());
            if !l.is_empty() { break l; }
        };
        let b = loop {
            let l = left.step(&d, Intent { dx: 0, dy: 0, attack: true }, bounds());
            if !l.is_empty() { break l; }
        };
        assert_eq!(a[0], (110, 70));
        assert_eq!(b[0], (90, 70));
    }

    #[test]
    fn a_line_crossing_a_body_counts_even_with_both_ends_outside() {
        let body = (90, 50, 110, 100);
        assert!(line_hits_body(&[(60, 70), (140, 70)], body));
        assert!(line_hits_body(&[(100, 70)], body));
        assert!(!line_hits_body(&[(60, 20), (140, 20)], body));
        assert!(!line_hits_body(&[], body));
    }

    #[test]
    fn enough_damage_kills_and_death_is_final() {
        let d = def();
        let mut f = Fighter::new("a", &d, 0, 0, 1);
        f.take_hit(60);
        assert_eq!(f.state, State::Hurt);
        f.take_hit(60);
        assert_eq!(f.state, State::Dead);
        assert_eq!(f.health, 0);
        f.take_hit(60);
        assert_eq!(f.health, 0);
    }

    /// The state machine, running the recovered scripts instead of frame lists.
    #[test]
    fn a_scripted_fighter_shows_the_script_its_state_names() {
        let d = scripted_def();
        let mut f = Fighter::new("k", &d, 100, 100, 1);
        assert_eq!(f.task.as_ref().unwrap().pc.script, "stance", "standing before a tick");
        f.step(&d, Intent { dx: 1, dy: 0, attack: false }, bounds());
        assert_eq!(f.task.as_ref().unwrap().pc.script, "walk1");
        f.step(&d, Intent { dx: 1, dy: 0, attack: false }, bounds());
        assert_eq!(f.task.as_ref().unwrap().pc.script, "walk2", "the cycle advances");
    }

    /// The walk is four one-frame scripts, and the cycle wraps rather than
    /// stopping on the last of them.
    #[test]
    fn a_walk_cycles_through_all_of_its_scripts_and_wraps() {
        let d = scripted_def();
        let mut f = Fighter::new("k", &d, 100, 100, 1);
        let seen: Vec<String> = (0..5)
            .map(|_| {
                f.step(&d, Intent { dx: 1, dy: 0, attack: false }, bounds());
                f.task.as_ref().unwrap().pc.script.clone()
            })
            .collect();
        assert_eq!(seen, ["walk1", "walk2", "walk3", "walk4", "walk1"]);
    }

    /// The hit shape comes out of the frame's own weapon parts, so only the
    /// frame that draws a blade can cut, and the frames either side cannot.
    #[test]
    fn only_a_frame_carrying_a_weapon_cel_can_connect() {
        let d = scripted_def();
        let mut f = Fighter::new("k", &d, 100, 100, 1);
        let lines: Vec<usize> = (0..3)
            .map(|_| f.step(&d, Intent { dx: 0, dy: 0, attack: true }, bounds()).len())
            .collect();
        assert_eq!(lines, vec![0, 5, 0], "one frame of three, and a rectangle is five points");
    }

    /// The blade is placed by the same arithmetic that draws it, mirror term
    /// included, so what you see is what can hit you.
    #[test]
    fn the_hit_shape_is_the_weapon_cel_where_it_is_drawn() {
        let d = scripted_def();
        let mut right = Fighter::new("k", &d, 100, 100, 1);
        right.step(&d, Intent { dx: 0, dy: 0, attack: true }, bounds());
        let a = right.step(&d, Intent { dx: 0, dy: 0, attack: true }, bounds());
        // The task origin is 52 above the feet; the cel is 30 by 10 at (10, 20).
        assert_eq!(a[0], (110, 68), "left, top");
        assert_eq!(a[2], (140, 78), "right, bottom");

        let mut left = Fighter::new("k", &d, 100, 100, -1);
        left.step(&d, Intent { dx: 0, dy: 0, attack: true }, bounds());
        let b = left.step(&d, Intent { dx: 0, dy: 0, attack: true }, bounds());
        assert_eq!(b[0], (60, 68), "task_x - (x + cel_width) = 100 - (10 + 30)");
        assert_eq!(b[2], (90, 78));
    }

    /// A blow is dealt outside a tick, so the recoil's task does not exist yet
    /// when the next tick asks whether the animation has finished. Reading a
    /// task that has not started as one that has ended made a recoil last
    /// exactly one tick and a knight unflinching.
    #[test]
    fn a_recoil_lasts_as_long_as_its_script_and_not_one_tick() {
        let d = scripted_def();
        let mut f = Fighter::new("k", &d, 100, 100, 1);
        f.take_hit(10);
        assert_eq!(f.state, State::Hurt);
        for t in 0..5 {
            f.step(&d, Intent::default(), bounds());
            assert_eq!(f.state, State::Hurt, "still reeling at tick {t}");
        }
        f.step(&d, Intent::default(), bounds());
        assert_eq!(f.state, State::Idle, "and then he has his feet again");
    }

    /// `TASKDEAD` in the middle of the recoil. The script itself decides, from
    /// the hit points in the actor record, whether a blow was the last one.
    #[test]
    fn a_recoil_turns_into_a_death_when_the_hit_points_are_gone() {
        let d = scripted_def();
        let mut f = Fighter::new("k", &d, 100, 100, 1);
        f.take_hit(10);
        f.health = 0;
        // Four ticks of the held frame, then the fifth reaches TASKDEAD.
        for _ in 0..5 {
            f.step(&d, Intent::default(), bounds());
        }
        assert_eq!(f.task.as_ref().unwrap().pc.script, "fall");
    }

    /// Sounds and calls into the original's code are reported, never performed.
    #[test]
    fn a_scripted_swing_reports_its_cues_without_playing_them() {
        use crate::taskvm::{Effect, GosubKind};
        let d = scripted_def();
        let mut f = Fighter::new("k", &d, 100, 100, 1);
        f.step(&d, Intent { dx: 0, dy: 0, attack: true }, bounds());
        assert_eq!(
            f.effects,
            vec![
                Effect::Sound { sample: 0x0b },
                Effect::Gosub { routine: "KnightGruntSound".into(), kind: GosubKind::Sound },
            ]
        );
        f.step(&d, Intent { dx: 0, dy: 0, attack: true }, bounds());
        assert!(f.effects.is_empty(), "and not again on the next frame");
    }

    /// Script frames last `script_ticks` ticks, and the blade keeps threatening
    /// for the whole of its frame rather than on the one tick it was stepped.
    #[test]
    fn a_script_frame_holds_for_its_ticks_and_the_blade_stays_out() {
        let mut d = scripted_def();
        d.script_ticks = 3;
        let mut f = Fighter::new("k", &d, 100, 100, 1);
        let lines: Vec<bool> = (0..9)
            .map(|_| !f.step(&d, Intent { dx: 0, dy: 0, attack: true }, bounds()).is_empty())
            .collect();
        assert_eq!(lines, [false, false, false, true, true, true, false, false, false]);
    }

    /// `Rjoystick` and `Ljoystick`, as one table over forward and back. The
    /// nine cells are the recovered ones; fire alone is the one cell the
    /// original leaves at the stance.
    #[test]
    fn the_direction_held_with_fire_picks_the_attack() {
        use Attack::*;
        let table = [
            ((1, -1), Some(UThrust)), ((1, 0), Some(Swing)), ((1, 1), Some(Lunge)),
            ((0, -1), Some(Chop)), ((0, 0), None), ((0, 1), Some(Evade)),
            ((-1, -1), Some(Knife)), ((-1, 0), Some(RThrust)), ((-1, 1), Some(Block)),
        ];
        for ((fwd, dy), want) in table {
            assert_eq!(Attack::for_direction(fwd, dy), want, "forward {fwd}, dy {dy}");
        }
        assert_eq!(Attack::Chop.kind(), 0x10, "the kinds are KnightAttSw offsets");
        assert_eq!(Attack::Lunge.kind(), 2);
        assert_eq!(Attack::from_name("rthrust"), Some(RThrust));
    }

    /// The same key is a different attack depending on which way the knight
    /// faces: `KnightAttack` picks the table by `+8`. Left held by a knight
    /// facing left is forward, and a swing; by one facing right it is back.
    #[test]
    fn the_attack_is_chosen_relative_to_the_facing() {
        let d = depth_def();
        let mut left = Fighter::new("k", &d, 100, 100, -1);
        left.step(&d, Intent { dx: -1, dy: 0, attack: true }, bounds());
        assert_eq!(left.attack, Some(Attack::Swing));
        assert_eq!(left.task.as_ref().unwrap().pc.script, "swing");
        assert_eq!(left.facing, -1, "attacking does not turn him");

        let mut right = Fighter::new("k", &d, 100, 100, 1);
        right.step(&d, Intent { dx: -1, dy: 1, attack: true }, bounds());
        assert_eq!(right.state, State::Guard, "back and down is the block");
        assert_eq!(right.attack, Some(Attack::Block));
        assert_eq!(right.task.as_ref().unwrap().pc.script, "block");

        let mut up = Fighter::new("k", &d, 100, 100, 1);
        up.step(&d, Intent { dx: 0, dy: -1, attack: true }, bounds());
        assert_eq!(up.attack, Some(Attack::Chop));
        assert_eq!(up.task.as_ref().unwrap().pc.script, "chop");

        // Fire alone is the stance in the original; here it is the swing, so
        // that one button still fights.
        let mut plain = Fighter::new("k", &d, 100, 100, 1);
        plain.step(&d, Intent { dx: 0, dy: 0, attack: true }, bounds());
        assert_eq!(plain.attack, Some(Attack::Swing));

        // An actor with fewer attacks than the table falls back to its own.
        let mut one = scripted_def();
        one.attacks.insert(
            "lunge".into(),
            crate::content::AttackDef { script: "swing".into(), damage: 3 },
        );
        one.attack = "lunge".into();
        let mut f = Fighter::new("t", &one, 100, 100, 1);
        f.step(&one, Intent { dx: 0, dy: -1, attack: true }, bounds());
        assert_eq!(f.attack, Some(Attack::Lunge), "no chop of its own, so its one attack");
    }

    /// A held guard has no gap in it: the block script is one frame ending
    /// on `ff ff`, and the original's controller reads the joystick on the
    /// frame it ends, so the kind stays a block for as long as fire is held.
    #[test]
    fn a_held_block_stays_up_between_frames() {
        let d = depth_def();
        let mut f = Fighter::new("k", &d, 100, 100, 1);
        for t in 0..8 {
            f.step(&d, Intent { dx: -1, dy: 1, attack: true }, bounds());
            assert_eq!(f.state, State::Guard, "tick {t}");
            assert_eq!(f.guarding(), Some(Attack::Block), "tick {t}");
        }
        f.step(&d, Intent::default(), bounds());
        assert_eq!(f.state, State::Idle, "and it comes down when fire does");
        assert_eq!(f.attack, None);
    }

    #[test]
    fn the_opponent_closes_distance_then_swings() {
        let d = def();
        let me = Fighter::new("a", &d, 0, 100, 1);
        let far = Fighter::new("b", &d, 200, 100, -1);
        let mut cd = 0;
        assert_eq!(simple_ai(&me, &far, &d, &mut cd, 31, true).dx, 1, "walks toward a distant foe");
        assert_eq!(simple_ai(&me, &far, &d, &mut cd, 33, true).dx, 0, "but not every tick: slower than a person");
        assert_eq!(simple_ai(&me, &far, &d, &mut cd, 10, true).dx, 0, "and it hesitates now and then");

        let near = Fighter::new("b", &d, 20, 100, -1);
        let mut cd = 0;
        assert!(simple_ai(&me, &near, &d, &mut cd, 31, true).attack, "swings when in reach");
        assert_eq!(cd, d.attack_cooldown);
        let after = simple_ai(&me, &near, &d, &mut cd, 32, true);
        assert!(!after.attack, "one swing, then it recovers");
        assert_eq!(after.dx, -1, "and gives ground while it does");
    }
}
