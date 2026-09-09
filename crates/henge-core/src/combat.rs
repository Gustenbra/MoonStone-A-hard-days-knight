//! One-on-one combat: state machine, movement, and positional hit resolution.
//!
//! No rendering, no assets, no platform. Everything here is driven by content
//! data, so retuning the feel of the game is editing JSON rather than editing
//! Rust, and swapping in our own artwork later changes nothing in this file.

use crate::anim::{Player, Sequence};
use crate::arena::{dir, Field, Step, GLOBAL};
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
        Attack::Lunge,
        Attack::Swing,
        Attack::Knife,
        Attack::Block,
        Attack::RThrust,
        Attack::UThrust,
        Attack::Evade,
        Attack::Chop,
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
    /// What the sheet adds to every blow: `CalcDamage`'s `cl = [si+0x2e]`,
    /// the strength, plus two, three or five for the three better swords.
    /// In the same units as `damage`, so a caller fighting at another scale
    /// moves it with the rest. Zero for a creature, which has no sheet.
    #[serde(default)]
    pub bonus: i32,
    /// The joystick reversed: `ControlKnight` on the knight `KnightCursed`
    /// names while the backfire flag is up, `xor ax, 0xc` when up or down is
    /// held and `xor ax, 3` when left or right is. Fire is left alone.
    #[serde(default)]
    pub cursed: bool,
    /// The controller state of a creature that has one of its own: the
    /// cooldown, timer and flags the original keeps in the actor record at
    /// `+0x0a`, `+0x0b`, `+0x48`, `+0x49` and `+0x4a`. See [`crate::monster`].
    #[serde(default)]
    pub brain: crate::monster::Brain,
    /// The script this fighter's own controller handed it, which is the
    /// original's `DS:0x783a` and `+0x28`. A creature does not press a button:
    /// its routine names the script and the kind outright, so an order is what
    /// a creature has where a person has an [`Intent`].
    #[serde(default)]
    pub ordered: Option<Order>,
    /// The standing order the controller last gave, kept because a controller
    /// only runs on the frame its animation ended (the original's `task+1`),
    /// while the simulation ticks six times for each of those frames. The
    /// original keeps the same thing in `+0x26`.
    #[serde(default)]
    pub drive: Intent,
    /// Something has hold of this fighter: the mudman's arms
    /// (`MudmenEntangle`). Held, its own input is ignored until it breaks free.
    #[serde(default)]
    pub holder: Option<usize>,
    /// Off the board, and not drawn: `KnightOFF`, which the demon's zap calls
    /// before `KnightON` puts him back beside it.
    #[serde(default)]
    pub hidden: bool,
    /// Which of the four directions the borders refused on the last tick this
    /// fighter tried to walk, in the bits of [`crate::arena::dir`].
    ///
    /// The original recomputes the whole byte at `+0x26` every frame and keeps
    /// nothing between them; this is the same byte, kept so that a trace can
    /// say which edge a fighter is standing against and so that two machines
    /// have one more thing to disagree about if they ever do.
    #[serde(default)]
    pub blocked: u8,
}

/// What a creature's own controller told it to do, as against what a joystick
/// would have said. `state` and `script` are `[0x783a]`; `attack` is `+0x28`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Order {
    pub state: State,
    pub script: String,
    pub attack: Option<Attack>,
}

impl Fighter {
    /// A zeroed definition, only for building a Fighter before the real
    /// definitions are in hand. Never simulated.
    pub fn placeholder_def() -> ActorDef {
        ActorDef {
            sheet: String::new(),
            health: 1,
            speed_x: 0,
            speed_y: 0,
            reach: 0,
            depth_tolerance: 0,
            attack_cooldown: 0,
            bounty: 0,
            girth: 0,
            body: [0; 4],
            sequences: Default::default(),
            ..ActorDef::default()
        }
    }

    pub fn new(actor: impl Into<String>, def: &ActorDef, x: i32, y: i32, facing: i32) -> Fighter {
        let mut f = Fighter {
            actor: actor.into(),
            x,
            y,
            facing,
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
            bonus: 0,
            cursed: false,
            brain: crate::monster::Brain::default(),
            ordered: None,
            drive: Intent::default(),
            holder: None,
            hidden: false,
            blocked: 0,
        };
        // The demon's own stance slot (`[di+0x10]`) is `Demon_Evolve`, so the
        // first thing it does is arrive. Nothing else has an entrance.
        if def.controller() == crate::monster::Controller::Demon {
            f.brain.flags |= crate::monster::flag::UNBORN;
        }
        // Run the first frame of the standing script now, so a fighter is
        // visible before anything has ticked. Without it a screenshot taken at
        // tick zero shows an empty arena, and a task that has never stepped has
        // nothing to draw.
        if def.scripted() {
            if let Some(first) = def.scripts_for(State::Idle.sequence_name()).first() {
                let facing = if facing < 0 {
                    FACING_LEFT
                } else {
                    FACING_RIGHT
                };
                let mut task = Task::new(
                    first,
                    x + def.origin[0] as i32,
                    y + def.origin[1] as i32,
                    facing,
                );
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
            && self
                .task
                .as_ref()
                .is_some_and(|t| t.active && t.shown.iter().any(|p| p.is(taskvm::part_flags::BODY)))
    }

    /// Holding a block or an evade.
    pub fn guarding(&self) -> Option<Attack> {
        if self.state == State::Guard {
            self.attack
        } else {
            None
        }
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
            return self.script.is_empty()
                && def.scripts_for(self.state.sequence_name()).is_empty();
        }
        match &self.task {
            Some(t) => !t.running,
            None => def.scripts_for(self.state.sequence_name()).is_empty(),
        }
    }

    /// Is this fighter's own controller due to run?
    ///
    /// The original's task loop calls a controller when the actor has hit
    /// something, been struck, or its animation has ended (`task+1` clear).
    /// A one-frame stance clears that every frame, so a standing creature is
    /// asked every frame and a swinging one only when the swing is over.
    pub fn ready(&self, def: &ActorDef) -> bool {
        !self.state.is_committed() || self.animation_done(def)
    }

    pub fn sequence<'a>(&self, def: &'a ActorDef) -> Option<&'a Sequence> {
        def.sequence(self.state.sequence_name())
    }

    /// One tick. Returns the hit line this fighter is sweeping, in world space,
    /// if the current frame carries one and it has not already connected.
    pub fn step(&mut self, def: &ActorDef, intent: Intent, field: &Field) -> Vec<(i32, i32)> {
        self.step_gated(def, intent, field, false)
    }

    /// One tick, with the gore switch. `bloodless` is the original's
    /// DS:0x700, which the title screen toggles: set, every part flagged
    /// `GATED` is dropped and every `TASKSKIP` is taken, which is how the
    /// decapitation turns into a collapse.
    pub fn step_gated(
        &mut self,
        def: &ActorDef,
        intent: Intent,
        field: &Field,
        bloodless: bool,
    ) -> Vec<(i32, i32)> {
        self.effects.clear();
        // The cursed knight's joystick, before anything reads it.
        let intent = if self.cursed {
            Intent {
                dx: -intent.dx,
                dy: -intent.dy,
                attack: intent.attack,
            }
        } else {
            intent
        };
        // The dead, and the dying. A scripted fighter whose hit points are
        // gone is still on his blow-taken script, whose own `TASKDEAD` picks
        // the death: the same recoil ends in a fall for a stab and a split
        // for a cut, because that is where each script's branch goes. The
        // state follows the script rather than the other way round.
        let dying = self.state == State::Hurt && self.health <= 0 && def.scripted();
        if self.state == State::Dead || dying {
            if def.scripted() {
                self.run_task(def, bloodless, false);
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

        // Held: `MudmenEntangle` takes the knight's control away entirely and
        // reads only fire and down, which is the struggle. Everything else he
        // presses does nothing until he is free or dead.
        if self.holder.is_some() {
            self.ordered = None;
            // `MudmenEntangle`: `test bx, 0x10` and `test bx, 4`, fire and
            // down, and only both together tear him loose.
            if intent.attack && intent.dy > 0 {
                self.holder = None;
                self.hidden = false;
            } else {
                if def.scripted() {
                    return self.run_task(def, bloodless, false);
                }
                return Vec::new();
            }
        }

        // A committed action runs to completion before input is looked at
        // again. It is looked at on the tick the action ends, the way the
        // original's controller runs on the frame `task+1` clears, which is
        // what lets a held block stay up with no gap in it.
        // A finished action falls back to standing, unless this fighter's own
        // controller has already named what comes next, which is what the
        // original's task loop does on the frame `task+1` clears.
        // A fighter its own controller is driving keeps showing the last frame
        // of whatever it was given until that controller speaks again, which
        // is what `DS:0x783a` holding 0xffff means. Only a fighter nobody is
        // driving falls back to standing the moment its animation ends.
        if self.state.is_committed()
            && self.animation_done(def)
            && self.ordered.is_none()
            && self.brain.flags & crate::monster::flag::DRIVEN == 0
        {
            self.enter(State::Idle);
        }
        let busy = self.state.is_committed() && !self.animation_done(def);
        if busy {
            // still busy
        } else if let Some(order) = self.ordered.take() {
            // A creature's own controller names the script and the kind
            // outright, the way the original writes `DS:0x783a` and `+0x28`,
            // rather than pressing a button and letting `KnightAttack`
            // choose. Everything else about the state is the same.
            //
            // A walk does not turn a creature. `MoveL` and `MoveR` (0x4dd5,
            // 0x4e09) never touch `+8`; the only thing they ask of the facing
            // is `MoveBACK` (0x5783), which runs the walk cycle backwards
            // when the step goes against it. The facing itself is whatever
            // the controller wrote, which for everything that tracks is
            // `FaceKnight`. Turning to the step here is what had the trogg
            // give ground rightward and then swing to the right, away from
            // the knight it had just backed away from.
            if order.state == State::Walk {
                // `M0$` (0x4bdc): `and byte ptr [si + 0x48], 0x7f`.
                self.evaded = false;
            }
            if order.script.is_empty() {
                self.enter(order.state);
            } else {
                // `REPLACEANIM`: the same script handed over again means play
                // it again. Without that a hold whose animation is shorter
                // than the hold drops to a stance between frames.
                if self.state == order.state && self.script == order.script {
                    self.state = State::Idle;
                    self.script.clear();
                }
                self.enter_on(order.state, order.script);
            }
            self.attack = order.attack;
        } else if self.brain.flags & crate::monster::flag::DRIVEN != 0 {
            // A creature with a controller of its own has no joystick to read.
            // Between one order and the next it carries on with what it was
            // given, which is `DS:0x783a` left at 0xffff.
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
                    let state = if kind.is_guard() {
                        State::Guard
                    } else {
                        State::Attack
                    };
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
            // `ControlKnight`, the only place the knight's own facing is
            // decided, and it is decided by the direction held, before the
            // borders get to refuse the step:
            //
            //   03f71  test byte ptr [di+0x26], 1
            //   03f75  je  03f80
            //   03f77  mov byte ptr [di+8], 1     ; right held: face right
            //   03f7b  call KnightWalkRight
            //   03f7e  jmp 03f8d
            //   03f80  test byte ptr [di+0x26], 2
            //   03f84  je  03f8d
            //   03f86  mov byte ptr [di+8], 3     ; left held: face left
            //   03f8a  call KnightWalkRight
            //
            // Up and down alone (0x3f5d, 0x3f68) leave `+8` as it is. The
            // task takes the new value on the same frame, through `dh` at
            // the tail (0x2d63) and `TASKHANDLE` (0x9741).
            if intent.dx > 0 {
                self.facing = 1;
            } else if intent.dx < 0 {
                self.facing = -1;
            }
            self.enter(State::Walk);
            // Walking is what gives the evade its one use back.
            self.evaded = false;
        } else {
            self.enter(State::Idle);
        }

        // Free movement only while not committed. Committed frames may still
        // carry the actor via their own dx/dy.
        //
        // This is `ControlKnight`'s own order: the walk routines put a step in
        // `DS:0x783d` and `0x783f` without moving anybody, `CheckBorder` and
        // `SBORD` clear whichever direction bits that step would break, and
        // only then is the step added, one axis at a time, to the axes whose
        // bit survived. Nothing is clamped, and a refused direction leaves the
        // other three alone.
        if self.state == State::Walk {
            let step = (intent.dx * def.speed_x, intent.dy * def.speed_y);
            let (moved, blocked) = self.walk(def, field, step);
            self.blocked = blocked;
            // `A4$`: a frame in which nothing moved winds the walk cycle back
            // and plays the stance instead, which is what makes a man held up
            // by a tree stand still rather than walk on the spot.
            if !moved {
                self.enter(State::Idle);
            }
        } else {
            self.blocked = 0;
        }

        if def.scripted() {
            // `MoveBACK` (0x5783), for a creature `MonsterWalk` moves: the
            // walk cycle runs backwards when the step goes against the
            // facing, and the facing itself is not touched. Translated as
            // `monster::move_back`, with the listing beside it. The knight's
            // own controller has no such thing: `ControlKnight+0x91`
            // (0x3f55) is `add byte ptr [di+0xa], 1` whichever way he goes.
            let back = self.brain.flags & crate::monster::flag::DRIVEN != 0
                && self.state == State::Walk
                && crate::monster::move_back(if self.facing < 0 { -1 } else { 1 }, intent.dx) < 0;
            return self.run_task(def, bloodless, back);
        }

        let Some(seq) = def.sequence(self.state.sequence_name()) else {
            return Vec::new();
        };
        let frame = self.player.current(seq).cloned();
        self.player.advance(seq);

        let Some(frame) = frame else {
            return Vec::new();
        };
        if frame.dx != 0 || frame.dy != 0 {
            let (x, y) = GLOBAL.clamp(
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

    /// One walking step, gated by the arena's borders.
    ///
    /// This is `ControlKnight`'s tail, `A1$` to `A5$`: the direction bits are
    /// whatever the step asked for, `CheckBorder` and `SBORD` take some of them
    /// away, and each axis is then added only if its own bit is still there.
    /// Returns whether anything moved, and which directions were refused.
    ///
    /// The original runs this on the knight and on nobody else: `SBORD` has one
    /// caller and `MonsterWalk` is not it, so in the DOS game a creature walks
    /// through the tree line as happily as our knight used to. Running every
    /// fighter through the same gate is ours, and it is the only sense in which
    /// this is not a transcription.
    fn walk(&mut self, def: &ActorDef, field: &Field, step: (i32, i32)) -> (bool, u8) {
        let (dx, dy) = step;
        let mut wanted = 0u8;
        if dx > 0 {
            wanted |= dir::RIGHT;
        } else if dx < 0 {
            wanted |= dir::LEFT;
        }
        if dy > 0 {
            wanted |= dir::DOWN;
        } else if dy < 0 {
            wanted |= dir::UP;
        }
        let (bl, _, br, bb) = self.body(def);
        let mut probe = Step {
            x: self.x,
            y: self.y,
            facing: if self.facing < 0 { -1 } else { 1 },
            dx,
            dy,
            box_left: bl,
            box_right: br,
            box_bottom: bb,
        };
        let ok = field.allow(&mut probe, wanted);
        // `CheckBorder` writes the column back before anything is added to it.
        self.x = probe.x;
        let mut moved = false;
        if ok & (dir::UP | dir::DOWN) != 0 {
            self.y += dy;
            moved = true;
        }
        if ok & (dir::LEFT | dir::RIGHT) != 0 {
            self.x += dx;
            moved = true;
        }
        // `CheckBorder` (0x40d0) holds the column inside `X_LOW`..`X_HIGH` as
        // well as inside the arena's own rectangles, and `run_task` below
        // clamps every fighter to the same pair when the script's position is
        // carried back out. An arena whose border rectangle reaches past 320
        // let a step land outside them, and the clamp then pulled it back on
        // the next script frame: a creature seated off the right edge walked
        // two pixels out and ten back for the rest of the fight instead of
        // standing. The two gates answer the same question, so they use the
        // same limits.
        let (cx, cy) = GLOBAL.clamp(self.x, self.y);
        self.x = cx;
        self.y = cy;
        (moved, wanted & !ok)
    }

    /// One tick of the task VM, for an actor animated by the recovered scripts.
    ///
    /// The task's position is the fighter's, shifted by the actor's origin,
    /// because the original places parts against a point near the top of the
    /// figure and this engine positions everything by the feet. Anything the
    /// script does to that position with `TASKMOVE` is carried back out, so a
    /// script that walks itself walks the fighter.
    ///
    /// `back` is `MoveBACK`'s `bp = -1`: the walk cycle is stepped backwards,
    /// which is `NextWalk` (0x4ef7) doing `add byte ptr [si+0xa], al` with
    /// `al` negative.
    fn run_task(&mut self, def: &ActorDef, bloodless: bool, back: bool) -> Vec<(i32, i32)> {
        // The state's own list, or the one script the state was entered on.
        let names: Vec<String> = if self.script.is_empty() {
            def.scripts_for(self.state.sequence_name()).to_vec()
        } else {
            vec![self.script.clone()]
        };
        if names.is_empty() {
            return Vec::new();
        }
        let facing = if self.facing < 0 {
            FACING_LEFT
        } else {
            FACING_RIGHT
        };
        let (ox, oy) = (def.origin[0] as i32, def.origin[1] as i32);

        // When the task's own facing (`task+0x14`) is taken from the record's
        // `+8`, and only then:
        //
        //   ADDTASK      0968a  mov al, [di+8]; 0968d mov [bx+0x14], al
        //   TASKHANDLE   09741  mov [di+0x14], dh   (dh from NOTEND+20)
        //
        // that is, when the task is made and each time a controller has run
        // and handed over a script. In between, the task keeps what it has,
        // which is what lets `TASK_FLIP` inside `Knight_SwDeath` or
        // `Beast_TurnAround` stay turned: the handler at 0x9a6d writes
        // `task+0x14` and copies it to `[actor+8]` (0x9a85), and nothing
        // writes it back the other way until the controller next runs.
        // Copying the record in on every tick undid the flip a tick later.
        //
        // The other direction runs every frame. `perdone` (0x99bd), the end
        // of `PerformCOMMAND`, writes the task back into the record:
        //
        //   099c0  mov ax, [di+4]; mov [bx+2], ax     ; x
        //   099c6  mov ax, [di+6]; mov [bx+4], ax     ; y
        //   099cc  mov ax, [di+8]; mov [bx+6], ax     ; z
        //   099d2  mov al, [di+0x14]; mov [bx+8], al  ; facing
        //
        // so `[actor+8]` always reads as the task's facing by the time a
        // controller looks at it. That is `self.facing` following `flipped`
        // below, and the position being carried back out.
        let mut step_now = false;
        match &mut self.task {
            Some(t) if self.restart => {
                // The new state's script, `REPLACEANIM`: the VM state is
                // zeroed and the first frame is stepped now.
                t.replace(names[0].clone());
                t.table = def.bank_table;
                t.facing = facing;
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
                        // `NextWalk` 0x4ef7: `add byte ptr [si+0xa], al`
                        // with `al` the `bp` `MoveBACK` chose, then
                        // `and byte ptr [si+0xa], 7`, which is the wrap.
                        self.cycle = if back {
                            (self.cycle + names.len() - 1) % names.len()
                        } else {
                            (self.cycle + 1) % names.len()
                        };
                        let next = names[self.cycle].clone();
                        t.replace(next);
                        t.facing = facing;
                    }
                }
            }
        }

        let Some(task) = self.task.as_mut() else {
            return Vec::new();
        };
        task.x = self.x + ox;
        task.y = self.y + oy;
        if step_now {
            self.record.set_health(self.health);
            // `[actor+8]` as the script sees it. A `TASK_FLIP` writes it
            // (0x9a85), and what it wrote is the fighter's facing from then
            // on, exactly as the next controller run would read it.
            self.record.set(taskvm::field::FACING, facing as i32);
            let frame = task.step(&def.animation, &mut self.record, bloodless);
            let flipped = self.record.get(taskvm::field::FACING);
            if flipped != facing as i32 {
                self.facing = if flipped & 2 != 0 { -1 } else { 1 };
            }
            self.effects = frame.effects;
            // Whatever the script moved, the fighter moved.
            let (nx, ny) = GLOBAL.clamp(task.x - ox, task.y - oy);
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
    /// **The weapon pile is recovered.** `COLLIDE.HIT` carries a polyline for
    /// every cel of every weapon bank, and `COLCHK` (0x9fcd) walks exactly
    /// that list: `CHECKL` (0xa0da) reads one point, `NOWID1` (0xa0ed) adds
    /// the weapon record's x and 0xa108 its y, and the mirrored case is
    /// `neg ax; add ax, [WIDTH]` at 0xa0e7 with `WIDTH` the cel's own width.
    /// So a swing is tested along the blade, and a cel the file gives no line
    /// for cannot hit anything (`RIGHTON`, 0xa022).
    ///
    /// What is still approximated is the *body* pile. The original places the
    /// target's `BODY` parts on `BodyPile` too and tests each weapon point
    /// against the body cel's own pixel mask (`CBITLP`, 0xa190); here the
    /// swept line is tested against the authored body box instead, so the
    /// resolution on that side is one rectangle rather than a mask. A bank
    /// with no line in the file keeps the old cel rectangle, so an actor the
    /// original never had still fights.
    fn hit_line(&self, def: &ActorDef) -> Vec<(i32, i32)> {
        let Some(task) = self.task.as_ref() else {
            return Vec::new();
        };
        let mirror = task.mirror();
        let mut out = Vec::new();
        for p in task
            .shown
            .iter()
            .filter(|p| p.is(taskvm::part_flags::WEAPON))
        {
            let Some(bank) = def.bank(p.table, p.bank) else {
                continue;
            };
            let Some(r) = taskvm::place(p, bank, (task.x, task.y, task.z), task.mirror()) else {
                continue;
            };
            match bank.hit_line(p.cel) {
                // 0a0da..0a10d, one sample of the blade at a time.
                Some(points) => out.extend(points.iter().map(|[px, py]| {
                    let x = if mirror {
                        r.x + r.w as i32 - *px as i32
                    } else {
                        r.x + *px as i32
                    };
                    (x, r.y + *py as i32)
                })),
                // No block in `COLLIDE.HIT` for this bank at all, which is
                // every actor that is not the original's. The cel's own
                // rectangle stands in, as it did for all of them before.
                None => {
                    let (l, t) = (r.x, r.y);
                    let (rr, b) = (r.x + r.w as i32, r.y + r.h as i32);
                    out.extend([(l, t), (rr, t), (rr, b), (l, b), (l, t)]);
                }
            }
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
        let hurt = if def.scripted() {
            def.hurt_for(by)
        } else {
            None
        };
        // `ControlClaw` never subtracts: the dragon's forelimbs take a blow
        // and are unmoved by it.
        if def.controller().takes_damage() {
            self.health -= damage;
        }
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
        let which = if by == Some(Attack::Swing) || by_own_kind {
            "decap"
        } else {
            "collapse"
        };
        let Some(script) = def.finishes.get(which).cloned() else {
            return false;
        };
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
        let wanted = def
            .blocks
            .get(attack.name())
            .and_then(|g| Attack::from_name(g));
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
            let Some(bank) = def.bank(p.table, p.bank) else {
                continue;
            };
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
    line.windows(2)
        .any(|w| segment_hits_rect(w[0], w[1], l, t, r, b))
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
            Instr::Part(Part {
                table: 1,
                bank: 0,
                cel,
                x: -8,
                y: 0,
                flags: part_flags::BODY,
            })
        };
        let stop = Instr::EndFrame { end: End::Stop };
        let next = Instr::EndFrame { end: End::Next };
        let mut animation = ScriptSet::new();
        animation.insert("stance".into(), Script::new(vec![body(0), stop.clone()]));
        for (i, n) in ["walk1", "walk2", "walk3", "walk4"].iter().enumerate() {
            animation.insert(
                n.to_string(),
                Script::new(vec![body(1 + i as u8), stop.clone()]),
            );
        }
        animation.insert(
            "swing".into(),
            Script::new(vec![
                Instr::Sound { sample: 0x0b },
                Instr::Gosub {
                    routine: "KnightGruntSound".into(),
                },
                body(6),
                next.clone(),
                body(7),
                Instr::Part(Part {
                    table: 1,
                    bank: 0,
                    cel: 8,
                    x: 10,
                    y: 20,
                    flags: part_flags::WEAPON,
                }),
                next.clone(),
                Instr::Goto {
                    mode: 0,
                    target: "stance".into(),
                },
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
                Instr::Dead {
                    target: "fall".into(),
                },
                body(9),
                stop.clone(),
            ]),
        );
        animation.insert(
            "fall".into(),
            Script::new(vec![
                Instr::Part(Part {
                    table: 1,
                    bank: 0,
                    cel: 10,
                    x: -20,
                    y: 40,
                    flags: 0,
                }),
                stop.clone(),
            ]),
        );

        let mut cels = vec![[16u16, 52u16]; 11];
        cels[8] = [30, 10]; // the blade
        let mut scripts = BTreeMap::new();
        scripts.insert("idle".into(), vec!["stance".to_string()]);
        scripts.insert(
            "walk".into(),
            ["walk1", "walk2", "walk3", "walk4"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
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
            banks: BTreeMap::from([(
                1u8,
                vec![Bank {
                    sheet: "test".into(),
                    base: 0,
                    cels,
                    hit: Vec::new(),
                }],
            )]),
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
            Instr::Part(Part {
                table: 1,
                bank: 0,
                cel,
                x: -8,
                y: 0,
                flags: part_flags::BODY,
            })
        };
        let blade = Instr::Part(Part {
            table: 1,
            bank: 0,
            cel: 8,
            x: 10,
            y: 20,
            flags: part_flags::WEAPON,
        });
        let stop = Instr::EndFrame { end: End::Stop };
        let next = Instr::EndFrame { end: End::Next };
        let a = &mut d.animation;
        // One frame with the blade out, held for two, so a chop can be told
        // from a swing by its script and lands on its first frame.
        a.insert(
            "chop".into(),
            Script::new(vec![
                Instr::Hold { count: 2 },
                body(6),
                blade.clone(),
                stop.clone(),
            ]),
        );
        a.insert("block".into(), Script::new(vec![body(9), stop.clone()]));
        a.insert("evade".into(), Script::new(vec![body(9), stop.clone()]));
        a.insert("recover".into(), Script::new(vec![body(9), stop.clone()]));
        // The throw: nothing without a dagger, else the call that spawns one.
        a.insert(
            "throw".into(),
            Script::new(vec![
                Instr::TestEq {
                    mode: 1,
                    field: field::DAGGERS,
                    target: "stance".into(),
                },
                body(6),
                next.clone(),
                Instr::Gosub {
                    routine: "KnifeThrow".into(),
                },
                body(7),
                stop.clone(),
            ]),
        );
        a.insert(
            "SpeedKnife".into(),
            Script::new(vec![
                Instr::Sound { sample: 0x0b },
                Instr::Move {
                    flags: 0x01,
                    x: 5,
                    y: 0,
                    z: 0,
                },
                blade.clone(),
                stop.clone(),
            ]),
        );
        a.insert(
            "Knife".into(),
            Script::new(vec![
                Instr::Move {
                    flags: 0x01,
                    x: 20,
                    y: 0,
                    z: 0,
                },
                blade.clone(),
                stop.clone(),
            ]),
        );
        // The death kneels with a body on it for ten frames, then lies flat
        // with none, the way `Knight_SwDeath` does before its collapse.
        a.insert(
            "fall".into(),
            Script::new(vec![
                Instr::Hold { count: 10 },
                Instr::Part(Part {
                    table: 1,
                    bank: 0,
                    cel: 10,
                    x: -20,
                    y: 20,
                    flags: part_flags::BODY,
                }),
                next.clone(),
                Instr::Part(Part {
                    table: 1,
                    bank: 0,
                    cel: 10,
                    x: -20,
                    y: 40,
                    flags: 0,
                }),
                stop.clone(),
            ]),
        );
        a.insert(
            "decap".into(),
            Script::new(vec![
                Instr::Skip {
                    target: "collapse".into(),
                },
                Instr::Part(Part {
                    table: 1,
                    bank: 0,
                    cel: 5,
                    x: 0,
                    y: -10,
                    flags: part_flags::GATED,
                }),
                Instr::Part(Part {
                    table: 1,
                    bank: 0,
                    cel: 10,
                    x: -20,
                    y: 40,
                    flags: 0,
                }),
                stop.clone(),
            ]),
        );
        a.insert(
            "collapse".into(),
            Script::new(vec![
                Instr::Part(Part {
                    table: 1,
                    bank: 0,
                    cel: 10,
                    x: -20,
                    y: 40,
                    flags: 0,
                }),
                stop.clone(),
            ]),
        );
        a.insert(
            "Blood1".into(),
            Script::new(vec![
                Instr::Part(Part {
                    table: 4,
                    bank: 4,
                    cel: 0,
                    x: -5,
                    y: -5,
                    flags: part_flags::GATED,
                }),
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
            d.attacks.insert(
                name.into(),
                AttackDef {
                    script: script.into(),
                    damage,
                },
            );
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
        let blood = Bank {
            sheet: "blood".into(),
            base: 0,
            cels: vec![[12, 12]; 4],
            hit: Vec::new(),
        };
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
                    hit: if hits {
                        vec![[10, 30], [40, 30]]
                    } else {
                        vec![]
                    },
                    ..Frame::default()
                })
                .collect();
            (
                name.to_string(),
                Sequence {
                    name: name.into(),
                    frames,
                    end,
                },
            )
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

    /// One arena's ground. The tree line is put high enough that every
    /// fighter in these tests stands below it and is free to walk.
    fn field() -> Field {
        Field::new(vec![crate::arena::Border {
            left: 0,
            right: 319,
            bottom: 60,
            top: 10,
        }])
    }

    /// The step gate and the write-back agree about the edge.
    ///
    /// `CheckBorder` (0x40d0) holds the column inside `X_LOW`..`X_HIGH`, and
    /// `run_task` clamps to the same pair when the script's position is
    /// carried out. Four of the shipped arena headers name a rectangle that
    /// reaches past 320, and while the two gates disagreed a fighter walking
    /// into that strip stepped two pixels out on every tick and was pulled
    /// ten back on every script frame, which is a creature seated off the
    /// right edge jittering there for the rest of the fight rather than
    /// standing at the edge.
    #[test]
    fn a_step_never_lands_outside_the_bounds_the_task_is_clamped_to() {
        let d = scripted_def();
        // A border that reaches well past `X_HIGH`, as `wa3` and its kin do.
        let wide = Field::new(vec![crate::arena::Border {
            left: 0,
            right: 400,
            bottom: 60,
            top: 10,
        }]);
        let mut f = Fighter::new("a", &d, 318, 100, 1);
        for _ in 0..8 {
            f.step(
                &d,
                Intent {
                    dx: 1,
                    dy: 0,
                    attack: false,
                },
                &wide,
            );
        }
        assert_eq!(f.x, crate::arena::limit::X_HIGH, "held at the edge");
        // And the task the blit reads is at the same column, so nothing is
        // pulled back on the next script frame.
        let ox = d.origin[0] as i32;
        assert_eq!(f.task.as_ref().map(|t| t.x - ox), Some(f.x));
    }

    #[test]
    fn walking_moves_and_turns_the_fighter() {
        let d = def();
        let mut f = Fighter::new("a", &d, 100, 100, 1);
        f.step(
            &d,
            Intent {
                dx: -1,
                dy: 0,
                attack: false,
            },
            &field(),
        );
        assert_eq!(f.facing, -1);
        assert_eq!(f.x, 98);
        assert_eq!(f.state, State::Walk);
    }

    #[test]
    fn an_attack_commits_and_ignores_input_until_it_finishes() {
        let d = def();
        let mut f = Fighter::new("a", &d, 100, 100, 1);
        f.step(
            &d,
            Intent {
                dx: 0,
                dy: 0,
                attack: true,
            },
            &field(),
        );
        assert_eq!(f.state, State::Attack);
        // Trying to walk away mid-swing does nothing.
        f.step(
            &d,
            Intent {
                dx: 1,
                dy: 0,
                attack: false,
            },
            &field(),
        );
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
            if !f
                .step(
                    &d,
                    Intent {
                        dx: 0,
                        dy: 0,
                        attack: true,
                    },
                    &field(),
                )
                .is_empty()
            {
                lines += 1;
                f.struck = true; // what the arena does once a hit lands
            }
            if f.player.finished {
                break;
            }
        }
        assert_eq!(
            lines, 1,
            "two frames carry a line but only one connect is allowed"
        );
    }

    /// A swing that misses must not disarm the rest of the swing.
    #[test]
    fn a_missed_swing_keeps_offering_its_line() {
        let d = def();
        let mut f = Fighter::new("a", &d, 100, 100, 1);
        let mut lines = 0;
        for _ in 0..8 {
            if !f
                .step(
                    &d,
                    Intent {
                        dx: 0,
                        dy: 0,
                        attack: true,
                    },
                    &field(),
                )
                .is_empty()
            {
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
            if !f
                .step(
                    &d,
                    Intent {
                        dx: 0,
                        dy: 0,
                        attack: true,
                    },
                    &field(),
                )
                .is_empty()
            {
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
            let l = right.step(
                &d,
                Intent {
                    dx: 0,
                    dy: 0,
                    attack: true,
                },
                &field(),
            );
            if !l.is_empty() {
                break l;
            }
        };
        let b = loop {
            let l = left.step(
                &d,
                Intent {
                    dx: 0,
                    dy: 0,
                    attack: true,
                },
                &field(),
            );
            if !l.is_empty() {
                break l;
            }
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
        assert_eq!(
            f.task.as_ref().unwrap().pc.script,
            "stance",
            "standing before a tick"
        );
        f.step(
            &d,
            Intent {
                dx: 1,
                dy: 0,
                attack: false,
            },
            &field(),
        );
        assert_eq!(f.task.as_ref().unwrap().pc.script, "walk1");
        f.step(
            &d,
            Intent {
                dx: 1,
                dy: 0,
                attack: false,
            },
            &field(),
        );
        assert_eq!(
            f.task.as_ref().unwrap().pc.script,
            "walk2",
            "the cycle advances"
        );
    }

    /// The walk is four one-frame scripts, and the cycle wraps rather than
    /// stopping on the last of them.
    #[test]
    fn a_walk_cycles_through_all_of_its_scripts_and_wraps() {
        let d = scripted_def();
        let mut f = Fighter::new("k", &d, 100, 100, 1);
        let seen: Vec<String> = (0..5)
            .map(|_| {
                f.step(
                    &d,
                    Intent {
                        dx: 1,
                        dy: 0,
                        attack: false,
                    },
                    &field(),
                );
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
            .map(|_| {
                f.step(
                    &d,
                    Intent {
                        dx: 0,
                        dy: 0,
                        attack: true,
                    },
                    &field(),
                )
                .len()
            })
            .collect();
        assert_eq!(
            lines,
            vec![0, 5, 0],
            "one frame of three, and a rectangle is five points"
        );
    }

    /// The blade is placed by the same arithmetic that draws it, mirror term
    /// included, so what you see is what can hit you.
    #[test]
    fn the_hit_shape_is_the_weapon_cel_where_it_is_drawn() {
        let d = scripted_def();
        let mut right = Fighter::new("k", &d, 100, 100, 1);
        right.step(
            &d,
            Intent {
                dx: 0,
                dy: 0,
                attack: true,
            },
            &field(),
        );
        let a = right.step(
            &d,
            Intent {
                dx: 0,
                dy: 0,
                attack: true,
            },
            &field(),
        );
        // The task origin is 52 above the feet; the cel is 30 by 10 at (10, 20).
        assert_eq!(a[0], (110, 68), "left, top");
        assert_eq!(a[2], (140, 78), "right, bottom");

        let mut left = Fighter::new("k", &d, 100, 100, -1);
        left.step(
            &d,
            Intent {
                dx: 0,
                dy: 0,
                attack: true,
            },
            &field(),
        );
        let b = left.step(
            &d,
            Intent {
                dx: 0,
                dy: 0,
                attack: true,
            },
            &field(),
        );
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
            f.step(&d, Intent::default(), &field());
            assert_eq!(f.state, State::Hurt, "still reeling at tick {t}");
        }
        f.step(&d, Intent::default(), &field());
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
            f.step(&d, Intent::default(), &field());
        }
        assert_eq!(f.task.as_ref().unwrap().pc.script, "fall");
    }

    /// Sounds and calls into the original's code are reported, never performed.
    #[test]
    fn a_scripted_swing_reports_its_cues_without_playing_them() {
        use crate::taskvm::{Effect, GosubKind};
        let d = scripted_def();
        let mut f = Fighter::new("k", &d, 100, 100, 1);
        f.step(
            &d,
            Intent {
                dx: 0,
                dy: 0,
                attack: true,
            },
            &field(),
        );
        assert_eq!(
            f.effects,
            vec![
                Effect::Sound { sample: 0x0b },
                Effect::Gosub {
                    routine: "KnightGruntSound".into(),
                    kind: GosubKind::Sound
                },
            ]
        );
        f.step(
            &d,
            Intent {
                dx: 0,
                dy: 0,
                attack: true,
            },
            &field(),
        );
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
            .map(|_| {
                !f.step(
                    &d,
                    Intent {
                        dx: 0,
                        dy: 0,
                        attack: true,
                    },
                    &field(),
                )
                .is_empty()
            })
            .collect();
        assert_eq!(
            lines,
            [false, false, false, true, true, true, false, false, false]
        );
    }

    /// `Rjoystick` and `Ljoystick`, as one table over forward and back. The
    /// nine cells are the recovered ones; fire alone is the one cell the
    /// original leaves at the stance.
    #[test]
    fn the_direction_held_with_fire_picks_the_attack() {
        use Attack::*;
        // Hand-aligned: the nine cells are a three by three grid of the joystick,
        // read as forward/back across and up/down the screen down.
        #[rustfmt::skip]
        let table = [
            ((1, -1), Some(UThrust)), ((1, 0), Some(Swing)), ((1, 1), Some(Lunge)),
            ((0, -1), Some(Chop)), ((0, 0), None), ((0, 1), Some(Evade)),
            ((-1, -1), Some(Knife)), ((-1, 0), Some(RThrust)), ((-1, 1), Some(Block)),
        ];
        for ((fwd, dy), want) in table {
            assert_eq!(
                Attack::for_direction(fwd, dy),
                want,
                "forward {fwd}, dy {dy}"
            );
        }
        assert_eq!(
            Attack::Chop.kind(),
            0x10,
            "the kinds are KnightAttSw offsets"
        );
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
        left.step(
            &d,
            Intent {
                dx: -1,
                dy: 0,
                attack: true,
            },
            &field(),
        );
        assert_eq!(left.attack, Some(Attack::Swing));
        assert_eq!(left.task.as_ref().unwrap().pc.script, "swing");
        assert_eq!(left.facing, -1, "attacking does not turn him");

        let mut right = Fighter::new("k", &d, 100, 100, 1);
        right.step(
            &d,
            Intent {
                dx: -1,
                dy: 1,
                attack: true,
            },
            &field(),
        );
        assert_eq!(right.state, State::Guard, "back and down is the block");
        assert_eq!(right.attack, Some(Attack::Block));
        assert_eq!(right.task.as_ref().unwrap().pc.script, "block");

        let mut up = Fighter::new("k", &d, 100, 100, 1);
        up.step(
            &d,
            Intent {
                dx: 0,
                dy: -1,
                attack: true,
            },
            &field(),
        );
        assert_eq!(up.attack, Some(Attack::Chop));
        assert_eq!(up.task.as_ref().unwrap().pc.script, "chop");

        // Fire alone is the stance in the original; here it is the swing, so
        // that one button still fights.
        let mut plain = Fighter::new("k", &d, 100, 100, 1);
        plain.step(
            &d,
            Intent {
                dx: 0,
                dy: 0,
                attack: true,
            },
            &field(),
        );
        assert_eq!(plain.attack, Some(Attack::Swing));

        // An actor with fewer attacks than the table falls back to its own.
        let mut one = scripted_def();
        one.attacks.insert(
            "lunge".into(),
            crate::content::AttackDef {
                script: "swing".into(),
                damage: 3,
            },
        );
        one.attack = "lunge".into();
        let mut f = Fighter::new("t", &one, 100, 100, 1);
        f.step(
            &one,
            Intent {
                dx: 0,
                dy: -1,
                attack: true,
            },
            &field(),
        );
        assert_eq!(
            f.attack,
            Some(Attack::Lunge),
            "no chop of its own, so its one attack"
        );
    }

    /// A held guard has no gap in it: the block script is one frame ending
    /// on `ff ff`, and the original's controller reads the joystick on the
    /// frame it ends, so the kind stays a block for as long as fire is held.
    #[test]
    fn a_held_block_stays_up_between_frames() {
        let d = depth_def();
        let mut f = Fighter::new("k", &d, 100, 100, 1);
        for t in 0..8 {
            f.step(
                &d,
                Intent {
                    dx: -1,
                    dy: 1,
                    attack: true,
                },
                &field(),
            );
            assert_eq!(f.state, State::Guard, "tick {t}");
            assert_eq!(f.guarding(), Some(Attack::Block), "tick {t}");
        }
        f.step(&d, Intent::default(), &field());
        assert_eq!(f.state, State::Idle, "and it comes down when fire does");
        assert_eq!(f.attack, None);
    }

    /// What stood here was `simple_ai`, a hand written opponent with a rhythm
    /// chosen for playability: two thirds speed, a step back after every
    /// swing, and a hesitation every couple of seconds. Item 37 replaced that
    /// with the recovered tracker, but the decision on top of it was still
    /// ours: close, swing, and a flat cooldown of twenty.
    ///
    /// `ControlBlackKnight` (0x4b79) is now translated, so this tests that
    /// instead, and it tests more than the old claim did: the opponent still
    /// closes and then strikes, and it also picks its attack by range,
    /// declines to play the same one twice running, and blocks or ducks what
    /// is coming at it. The flat twenty is gone because the original has no
    /// such number: `BKnightAttack` zeroes `+0x4a` and never sets it.
    #[test]
    fn the_computer_knight_closes_then_picks_an_attack_by_range() {
        use crate::monster::{decide, percent, Act, Brain, Sight, PROGRESSION};
        let d = ActorDef {
            controller: "knight".into(),
            approach: 100,
            back_off: 80,
            ..def()
        };
        let mut me = Fighter::new("a", &d, 0, 100, 1);
        me.brain.flags |= crate::monster::flag::DRIVEN;
        let look = |me: &Fighter, foe: &Fighter, brain: &mut Brain, seed: u16| {
            let s = Sight {
                me,
                foe,
                def: &d,
                bounds: GLOBAL,
                gore: true,
                body: false,
                decapped: false,
                progression: 0,
            };
            let mut seed = seed;
            let mut facing = 1;
            decide(&s, brain, &mut seed, &mut facing)
        };
        // `BKBlock` spends one roll before `BKAttack` spends its own, so the
        // seed that makes the knight strike is one whose *second* roll clears
        // the day's figure. Day zero is `Progression[0]`, twenty.
        let keen = (1..=u16::MAX)
            .find(|&s| {
                let mut x = s;
                percent(&mut x);
                percent(&mut x) > PROGRESSION[0]
            })
            .expect("some seed rolls over twenty");
        let shy = (1..=u16::MAX)
            .find(|&s| {
                let mut x = s;
                percent(&mut x);
                percent(&mut x) <= PROGRESSION[0]
            })
            .expect("some seed rolls under twenty");

        let mut brain = Brain::default();
        let far = Fighter::new("b", &d, 200, 100, -1);
        assert!(
            matches!(look(&me, &far, &mut brain, keen), Act::Walk { dx: 1, .. }),
            "walks toward a distant foe"
        );
        // Ninety across, which is `K0$`'s `cmp bx, 0x5a`.
        let near = Fighter::new("b", &d, 90, 100, -1);
        assert_eq!(
            look(&me, &near, &mut brain, keen),
            Act::Attack {
                kind: Attack::Swing,
                spawn: None
            },
            "inside ninety, and nothing ordered yet: the swing"
        );
        assert_eq!(brain.att, Some(Attack::Swing), "+0x28 keeps what it chose");
        assert_eq!(
            brain.cooldown, 0,
            "BKnightAttack has no cooldown of its own"
        );
        // `cmp word [ATT], 4; je KK1$`: the same swing again is refused and
        // the overhead chop is taken instead.
        assert_eq!(
            look(&me, &near, &mut brain, keen),
            Act::Attack {
                kind: Attack::Chop,
                spawn: None
            },
            "never the same attack twice running"
        );
        // And again, inside ninety, with the chop now in `ATT`: the swing is
        // free once more, so the two alternate. This is why the computer
        // knight does not stand there repeating one blow.
        assert_eq!(
            look(&me, &near, &mut brain, keen),
            Act::Attack {
                kind: Attack::Swing,
                spawn: None
            },
            "inside ninety it alternates the swing and the chop"
        );
        // `K2$`, ninety eight across: past ninety five, inside a hundred, and
        // with no daggers on the belt `cmp byte [si+0x34], 0` takes the lunge
        // whatever `ATT` holds.
        let mid = Fighter::new("b", &d, 98, 100, -1);
        let mut b1 = Brain::default();
        assert_eq!(
            look(&me, &mid, &mut b1, keen),
            Act::Attack {
                kind: Attack::Lunge,
                spawn: None
            },
            "between ninety five and a hundred with no daggers: the lunge"
        );
        // `K3$`: a hundred and ten away, and a dagger to throw.
        let out = Fighter::new("b", &d, 110, 100, -1);
        let mut armed = me.clone();
        armed.record.set(crate::taskvm::field::DAGGERS, 3);
        let mut b2 = Brain::default();
        assert_eq!(
            look(&armed, &out, &mut b2, keen),
            Act::Attack {
                kind: Attack::Knife,
                spawn: None
            },
            "out of reach with daggers left: it throws one"
        );
        // The same place with an empty belt closes instead: `K4$`.
        let mut b3 = Brain::default();
        assert!(
            matches!(look(&me, &out, &mut b3, keen), Act::Walk { .. } | Act::Idle),
            "out of reach with none: it closes"
        );
        // `BKAttack`'s own roll under the day's figure gives ground.
        let mut b4 = Brain::default();
        assert_eq!(
            look(&me, &near, &mut b4, shy),
            Act::Idle,
            "a roll under the day's figure hesitates instead of striking"
        );
    }

    /// `BKBlock` (0x4c40), which the invented AI had nothing of: the computer
    /// knight answers what the other one is doing.
    #[test]
    fn the_computer_knight_blocks_a_swing_and_ducks_a_chop() {
        use crate::monster::{decide, percent, Act, Brain, Sight, PROGRESSION};
        let d = ActorDef {
            controller: "knight".into(),
            approach: 100,
            back_off: 80,
            ..def()
        };
        let me = Fighter::new("a", &d, 0, 100, 1);
        // `BKBlock`'s own roll has to reach the day's figure for it to look at
        // the other knight at all: `cmp al, [bx]; jl BKAttack`.
        let wary = (1..=u16::MAX)
            .find(|&s| {
                let mut x = s;
                percent(&mut x) >= PROGRESSION[0]
            })
            .expect("some seed rolls at or over twenty");
        let answer = |incoming, facing: i32| {
            let mut foe = Fighter::new("b", &d, 90, 100, facing);
            foe.state = State::Attack;
            foe.attack = Some(incoming);
            let s = Sight {
                me: &me,
                foe: &foe,
                def: &d,
                bounds: GLOBAL,
                gore: true,
                body: false,
                decapped: false,
                progression: 0,
            };
            let mut brain = Brain::default();
            let mut seed = wary;
            let mut f = 1;
            decide(&s, &mut brain, &mut seed, &mut f)
        };
        assert_eq!(
            answer(Attack::Swing, -1),
            Act::Attack {
                kind: Attack::Block,
                spawn: None
            },
            "a swing from the front is blocked"
        );
        assert_eq!(
            answer(Attack::Chop, -1),
            Act::Attack {
                kind: Attack::Evade,
                spawn: None
            },
            "an overhead chop is ducked"
        );
        assert_eq!(
            answer(Attack::Lunge, -1),
            Act::Attack {
                kind: Attack::Evade,
                spawn: None
            },
            "so is a lunge"
        );
        // 04c60: both facing the same way is his back turned, so there is
        // nothing to stop and it attacks instead.
        assert!(
            !matches!(
                answer(Attack::Swing, 1),
                Act::Attack {
                    kind: Attack::Block,
                    ..
                }
            ),
            "a swing from someone facing the same way is not blocked"
        );
    }
}
