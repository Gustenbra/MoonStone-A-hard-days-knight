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

/// `K_WalkRValue` (`0x77fe`), four words back to back with the two tables
/// below it: the x-speed the **person-controlled** knight steps by while
/// walking right or left, one entry per walk-cycle frame, in pixels per
/// frame rather than per tick. `KnightWalkRight` (0x4048) is called once for
/// right held and once for left, looks this same table up by `[di+0xa]`
/// (`Fighter::cycle` here) and negates it facing left; right and left are a
/// sign flip of the one table, not two tables.
pub const KNIGHT_WALK_R_VALUE: [i32; 4] = [25, 3, 23, 4];

/// `K_WalkUpValue` (`0x7808`): the z-speed walking up. `KnightWalkUp`
/// (0x4067) looks this up and **always** negates it, unconditionally — a
/// genuinely different table from the down one below, not its mirror.
const KNIGHT_WALK_UP_VALUE: [i32; 4] = [2, 9, 2, 9];

/// `K_WalkDownValue` (`0x7810`): the z-speed walking down. `KnightWalkDown`
/// (0x4080) looks this up and **never** negates it.
const KNIGHT_WALK_DOWN_VALUE: [i32; 4] = [8, 2, 9, 2];

/// The step a person-controlled knight's walk takes on one displayed frame,
/// `KnightWalkRight`/`KnightWalkUp`/`KnightWalkDown` (0x4048/0x4067/0x4080)
/// translated: right/left write `x` from the one shared table, sign only;
/// up/down write `z` from their own separate tables, one always negated and
/// the other never. Both axes are independent `test`s in the original
/// (0x3f5d/0x3f68/0x3f71/0x3f80), not an if/else chain, so a diagonal held
/// sets both from the SAME `idx` — the one counter driving all three tables
/// in lockstep.
///
/// `idx` is taken modulo the tables' own length rather than the walk
/// script's row length: the original's mask is a literal `and byte [di+0xa],
/// 3`, a fixed 4, independent of how many script names any row happens to
/// have (they are 4 too, confirmed by
/// `henge_formats::tables::tests::the_knight_tables_read_as_set_up_knight_writes_them`,
/// but that is a second, separate fact, not the source of this one).
fn knight_walk_step(intent: Intent, idx: usize) -> (i32, i32) {
    let idx = idx % KNIGHT_WALK_R_VALUE.len();
    let x = if intent.dx > 0 {
        KNIGHT_WALK_R_VALUE[idx]
    } else if intent.dx < 0 {
        -KNIGHT_WALK_R_VALUE[idx]
    } else {
        0
    };
    let y = if intent.dy > 0 {
        KNIGHT_WALK_DOWN_VALUE[idx]
    } else if intent.dy < 0 {
        -KNIGHT_WALK_UP_VALUE[idx]
    } else {
        0
    };
    (x, y)
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
    /// The rows this encounter's `InitKnightvs*` routine wrote over the
    /// knight's `*Att` table (the record's `+0x16`, `KnightAttSw`), by the
    /// name of the attack kind: see [`crate::bout::Bout::knight_att_rows`] for
    /// which fight writes what and [`Fighter::attack_script`] for the read.
    /// Empty for everyone else and for every fight that writes none.
    #[serde(default)]
    pub att_rows: std::collections::BTreeMap<String, String>,
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
    /// The direction held on the last tick, as `dx` and `dy` signs: the walk
    /// bits of `+0x26` before the borders take any away, which is what
    /// `ControlKnight` (0x3fd6) and `TroggMove` (0x2e2b) choose the walk row
    /// by. See [`ActorDef::walk_row`].
    #[serde(default)]
    pub heading: [i32; 2],
    /// `[bx+8]` of the knight's magic record (`+0x44`): how many Talismans of
    /// the Wyrm he carries, which `TalismanWrym` (0x43f4) halves the dragon's
    /// blows by. Zero for anything that has no magic record.
    #[serde(default)]
    pub talismans: i32,
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
            att_rows: std::collections::BTreeMap::new(),
            brain: crate::monster::Brain::default(),
            ordered: None,
            drive: Intent::default(),
            holder: None,
            hidden: false,
            blocked: 0,
            heading: [0, 0],
            talismans: 0,
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

    pub(crate) fn enter(&mut self, state: State) {
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
    pub(crate) fn enter_on(&mut self, state: State, script: String) {
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
        self.step_among(def, intent, field, bloodless, &[])
    }

    /// The same tick, with the other bodies in the arena.
    ///
    /// `others` is the task table `TASKWALKCOLLIDE` (0x9e06) walks: every
    /// other actor that has been drawn and still has hit points. A fighter
    /// may not step into one of them, and the routine decides in which
    /// directions; see [`crate::arena::walk_collide`].
    pub fn step_among(
        &mut self,
        def: &ActorDef,
        intent: Intent,
        field: &Field,
        bloodless: bool,
        others: &[crate::arena::Occupant],
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
        self.heading = [intent.dx.signum(), intent.dy.signum()];
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
            match self.attack_script(def, wanted) {
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
            // **Everybody moves once a displayed frame, by a table.** A
            // controller runs once per frame and moves once: `ControlKnight`
            // through `KnightWalkRight`/`Up`/`Down` (0x4048, 0x4067, 0x4080)
            // off `K_WalkRValue` and its two siblings, and every creature
            // through `MoveL`/`MoveR`/`MoveU`/`MoveD` (0x4dd5, 0x4e09,
            // 0x4e3d, 0x4e64), which index their caller's own table by the
            // same walk cycle. `ControlBlackKnight` is not the exception it
            // was read as: its `M0$`..`M3$` (0x4be6 to 0x4c0a) load
            // `BKnightWALKR`/`U`/`D`, which hold the knight's own numbers.
            //
            // Nothing in the image moves by a flat step applied every tick,
            // and applying one was what stuck the spear trogg a second time
            // after the borders let him go: six sub-ticks of two pixels put
            // him twelve pixels further along between one decision and the
            // next, and his attack window (`SetTroggSpTables` 0x2220: inside
            // approach 0x82, outside back off 0x78) is ten pixels wide. He
            // stepped over it every time, in both directions, for ever. His
            // own table is 0, 7 then 23, and those small steps are what land
            // him inside it.
            if let Some(idx) = self.walk_pulse(def) {
                let step = self.walk_step(def, intent, idx);
                let (moved, blocked) = self.walk(def, field, step, others);
                self.blocked = blocked;
                // `A4$`: a frame in which nothing moved winds the walk
                // cycle back and plays the stance instead, which is what
                // makes a man held up by a tree stand still rather than
                // walk on the spot.
                if !moved {
                    self.enter(State::Idle);
                }
            }
            // A sub-tick that is not this frame's one pulse: the original
            // has no such tick at all, so nothing here moves, is blocked, or
            // falls back to idle on it.
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
    /// The original runs the border half of this on the person's own knight
    /// and on nobody else: `CheckBorder` (0x40d0) and `SBORD` (0x4552) have
    /// one caller each and it is `ControlKnight` (0x3fb3, 0x3fc1). A creature
    /// walks through the tree line, and off the edge of the arena, as happily
    /// as ours used to not. See the block below it for the whole argument.
    fn walk(
        &mut self,
        def: &ActorDef,
        field: &Field,
        step: (i32, i32),
        others: &[crate::arena::Occupant],
    ) -> (bool, u8) {
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
        let (bl, bt, br, bb) = self.body(def);
        // `TASKWALKCOLLIDE`, which `ControlKnight+217` (0x3f9d),
        // `MonsterWalk+19` (0x4e9e) and `MudmenMove+65` (0x53c0) all call
        // before `CheckBorder` and `SBORD`, and whose answer they `and` into
        // `+0x26` exactly as this does.
        if !others.is_empty() {
            let me = crate::arena::Occupant {
                x: self.x,
                depth: self.y,
                box_left: bl,
                box_right: br,
                box_top: bt,
                box_bottom: bb,
            };
            let facing = if self.facing < 0 { 3 } else { 1 };
            wanted &= crate::arena::walk_collide(&me, facing, dx, others);
        }
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
        // **The borders are the person's knight's alone.** `MonsterWalk`
        // (0x4e8b) is the whole of a creature's gate: `NextWalk`, then
        // `TASKWALKCOLLIDE` at 0x4e9e, then `and [si+0x26], ax` and, if
        // nothing survived, the walk cycle reset and the stance (0x4eaa to
        // 0x4eb1). It calls neither `CheckBorder` (0x40d0) nor `SBORD`
        // (0x4552). `ControlBlackKnight` jumps into that same routine
        // (0x4c10), so a computer knight is not gated either. Only
        // `ControlKnight` asks the borders, at 0x3fb3 and 0x3fc1.
        //
        // Running every fighter through them was ours, and it is what stuck
        // the spear trogg: he backs away when the knight closes (`TrackBack`,
        // 0x5763), runs out of arena, is refused every direction, and the
        // `!moved` arm above drops him into his stance to stand there. The
        // seats settle it beyond argument: `TroggTABLE` puts one creature at
        // x -50 and another at 360, and a creature the borders held could
        // never walk in from either.
        let person =
            def.controller == "knight" && self.brain.flags & crate::monster::flag::DRIVEN == 0;
        let ok = if person {
            let ok = field.allow(&mut probe, wanted);
            // `CheckBorder` writes the column back before anything is added
            // to it, which it only does for the knight it gates.
            self.x = probe.x;
            ok
        } else {
            wanted
        };
        let mut moved = false;
        if ok & (dir::UP | dir::DOWN) != 0 {
            self.y += dy;
            moved = true;
        }
        if ok & (dir::LEFT | dir::RIGHT) != 0 {
            self.x += dx;
            moved = true;
        }
        // **Nothing is clamped after the add.** `ControlKnight` adds its step
        // at 0x3fdc, 0x3feb, 0x3ffa and 0x4009 and then falls straight out;
        // `MonsterWalk` adds at 0x4eeb and 0x4ef1 and jumps to 0x2d52.
        // `CheckBorder`'s own write-back (0x40ed, 0x40fb) is the whole of the
        // knight's bound, and it is taken *before* the add, off the probe, by
        // `field.allow` above. Exactly two routines in the image bound an
        // actor's column at all -- `CheckBorder` and `KnightON` (0x5289, on
        // arrival) -- and the four movement sites that write one
        // (`MonsterWalk` 0x4eeb, `DemonMove` 0x5004, `MudmenMove` 0x53fb,
        // `MudmenAppear` 0x5492) all `add` without a bound.
        //
        // Our clamp here was the other half of the spear trogg's freeze: it
        // pinned him at column 10 while he was still asking to retreat, so he
        // played his walk on the spot and the script frame carried him back.
        // Note too that `CheckBorder` writes back only the column, never the
        // depth: the two depth tests at 0x4100 and 0x410a clear direction
        // bits and nothing more.
        (moved, wanted & !ok)
    }

    /// **Person-controlled knight only.** The walk-speed table index this
    /// tick's step should use, or `None` on a tick nothing should move on.
    ///
    /// `ControlKnight` runs once per displayed frame: every frame a
    /// direction is held, `[di+0xa]` (`Fighter::cycle`) is incremented and
    /// masked mod 4 *before* `KnightWalkRight`/`Up`/`Down` look anything up,
    /// so the index the speed tables read is always the one the frame about
    /// to be drawn uses. This engine subdivides one displayed frame into
    /// `ActorDef::script_ticks` sub-ticks (below, `run_task`'s own
    /// `step_now`/cycle-advance below only fires once every that many
    /// calls), so a step computed every sub-tick — the flat
    /// `def.speed_x`/`speed_y` this replaces reads that way — applies the
    /// same table entry `script_ticks` times over rather than once, and a
    /// step computed off `self.cycle` as it stands is one whole displayed
    /// frame stale on the sub-tick the frame changes: the flat step used to
    /// run, and would still run, strictly before `run_task` updates
    /// `self.cycle` this same call, not after.
    ///
    /// So this predicts what `run_task` is about to set `self.cycle` to,
    /// using the exact conditions its own match arms below gate that same
    /// decision on (a fresh task, a restart, or `script_tick` about to wrap
    /// with the current script already finished) — never reading
    /// `self.cycle` as it stands once the walk is already under way, only
    /// what it is one tick away from becoming.
    /// This frame's step, from whichever walk-speed table this fighter's own
    /// mover reads.
    ///
    /// The person's knight is the one actor whose tables the image keeps as
    /// three separate rows of words rather than as `(x, z)` pairs, because
    /// `ControlKnight` reads them itself instead of going through the shared
    /// movers; `knight_walk_step` is that reading. Everyone else, the black
    /// knight included, indexes a pair table, and an actor with no table for
    /// an axis falls back to `def.speed_x`/`def.speed_y` for it, which is the
    /// flat literal its own controller writes -- the troll's `+/-5` depth
    /// (`ControlTroll` 0x5620, 0x5635), the mudmen's `+/-2`
    /// (`MudmenMoveU`/`MudmenMoveD` 0x5447, 0x5451), the demon's `+/-5` both
    /// ways (`DemonMove` 0x4fe2 through 0x5001).
    fn walk_step(&self, def: &ActorDef, intent: Intent, idx: usize) -> (i32, i32) {
        let person =
            def.controller == "knight" && self.brain.flags & crate::monster::flag::DRIVEN == 0;
        if person {
            knight_walk_step(intent, idx)
        } else {
            def.walk_speed
                .step(intent.dx, intent.dy, idx, (def.speed_x, def.speed_y))
        }
    }

    pub(crate) fn walk_pulse(&self, def: &ActorDef) -> Option<usize> {
        let names_len = def.walk_row(self.heading[0], self.heading[1]).len().max(1);
        match &self.task {
            // A fresh task, or a restart: `run_task` below plays `names[0]`
            // outright in both cases, matching `self.cycle`'s own reset to 0
            // on every state change (`Fighter::enter`), so no advance to
            // predict — the pulse is this tick, at the index already there.
            None => Some(self.cycle % names_len),
            Some(_) if self.restart => Some(self.cycle % names_len),
            // Continuing an already-running walk: `run_task` advances
            // `self.cycle` exactly when `script_tick` is about to wrap AND
            // the current script has already finished running, the same
            // `self.script_tick + 1 >= def.script_ticks.max(1)` and
            // `!t.running` this mirrors below.
            Some(t) => {
                let wrapping = self.script_tick + 1 >= def.script_ticks.max(1);
                if wrapping && !t.running {
                    Some((self.cycle + 1) % names_len)
                } else {
                    None
                }
            }
        }
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
        // A walk is drawn on the row the direction picks (`ActorDef::walk_row`).
        let names: Vec<String> = if !self.script.is_empty() {
            vec![self.script.clone()]
        } else if self.state == State::Walk {
            def.walk_row(self.heading[0], self.heading[1]).to_vec()
        } else {
            def.scripts_for(self.state.sequence_name()).to_vec()
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
                        // One connect per script, not one per state. The
                        // original has no such flag: `TaskCol_MainLoop`
                        // writes `+0xc` on the striker and `+0xe` on the
                        // struck (0x9f81) and the controller clears them on
                        // its next pass, so a creature whose walk cycle is
                        // its attack is armed again on every frame of it.
                        self.struck = false;
                    }
                }
            }
        }

        let Some(task) = self.task.as_mut() else {
            return Vec::new();
        };
        task.x = self.x + ox;
        task.y = self.y + oy;
        // `+4`, the height, which `perdone` (0x99c6) copies into the task
        // every frame and `TASKRIGHT`/`TASKLEFT` add to the depth when they
        // place a part. This engine keeps the depth in `Fighter::y` and so in
        // `Task::y`, which leaves `Task::z` for exactly this; nothing else
        // reads it, since a script's own `TASKMOVE` z is thrown away here.
        task.z = self.brain.height;
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
            // Whatever the script moved, the fighter moved -- unbounded. The
            // task VM never writes an actor's column in the original: of the
            // ten sites that write `[si+2]` as a coordinate, none sits in the
            // VM's range, and a scripted lunge moves the *task*, whose anchor
            // the controller owns. So there is nothing here to clamp against,
            // and clamping it was ours. It is what pulled a fighter who had
            // walked past the edge back two pixels every script frame, the
            // visible half of the spear trogg standing and twitching at the
            // arena wall. `CheckBorder` (0x40d0) is the only bound on the
            // person's knight and it is applied in `walk` above, before the
            // add, off the probe.
            self.x = task.x - ox;
            self.y = task.y - oy;
        }

        // **No state test.** `TaskCol_MainLoop` (0x9f26) walks the task's
        // `WeoponPile` at `+0x1e` against every other task's `BodyPile` at
        // `+0x20`, and neither pile knows what state anybody is in: a part is
        // on the weapon pile because its own record is flagged `WEAPON` and
        // for no other reason. Six scripts in the shipped game carry a weapon
        // part outside an attack, and every one of them is a creature that is
        // meant to hurt you with it: the beast's four run frames and its turn,
        // Balok's hop, the ratman's four leap frames and the two it hangs out
        // of a tree on. Gating this on `State::Attack` is what kept the beast
        // from ever touching anybody, since its run *is* its attack.
        if self.struck {
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

    /// `KnightAttSw[kind]` as this fight has it: the row the actor's own
    /// `Set*Tables` routine wrote, unless this encounter's `InitKnightvs*`
    /// wrote over it.
    ///
    /// **Recovered.** `KnightAttack` reads the table at the record's `+0x16`
    /// by the kind the joystick picked and hands the task what it finds; the
    /// kind is unchanged, only the script. Three fights write rows over it
    /// before the first frame — see [`crate::bout::Bout::knight_att_rows`] —
    /// and because the table is indexed by the kind asked for, the override is
    /// read at that kind and not at whatever [`ActorDef::attack_for`]'s
    /// fallback would have settled on. The knight has all nine rows, so the
    /// two never disagree for him.
    pub fn attack_script(&self, def: &ActorDef, wanted: Attack) -> Option<(String, Attack)> {
        if let Some(script) = self.att_rows.get(wanted.name()) {
            return Some((script.clone(), wanted));
        }
        def.attack_for(wanted)
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

    /// The task killed and the record freed: the routine at 0x96c9, which
    /// `DragonHit2+21` (0x3aea) calls on the knight the bite has closed on,
    /// and what `TASKHANDLE` does with a `[0x783a]` of zero, which is
    /// `CLAWS_DEAD` (0x3b61). Nothing is drawn, nothing is hit, and nothing
    /// counts this fighter among the standing.
    pub(crate) fn vanish(&mut self) {
        self.health = 0;
        self.state = State::Dead;
        self.hidden = true;
        self.restart = false;
        self.ordered = None;
        if let Some(t) = self.task.as_mut() {
            t.active = false;
            t.running = false;
            t.shown.clear();
        }
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
        // `KnightDamSw` as `SetUpKnight` (0x17e4) writes it: four for the
        // swing and four for the chop, three for the knife.
        for (name, script, damage) in [
            ("swing", "swing", 4),
            ("chop", "chop", 4),
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

    /// A creature is bounded by nothing but the other fighters.
    ///
    /// `MonsterWalk` (0x4e8b) is the whole of a creature's gate: `NextWalk`,
    /// `TASKWALKCOLLIDE` at 0x4e9e, `and [si+0x26], ax`, then `add [si+2], ax`
    /// and `add [si+6], ax` at 0x4eeb and 0x4ef1 and out. No `CheckBorder`
    /// (0x40d0), no `SBORD` (0x4552), no clamp. `TroggTABLE` seats creatures
    /// at columns -50 and 360 and they have to be able to walk in from there.
    ///
    /// Gating every fighter on the borders, and clamping the write-back, was
    /// ours, and between them they froze the spear trogg: he retreats when the
    /// knight closes (`TrackBack`, 0x5763), ran out of the gate's idea of the
    /// arena, was refused every direction, and the `!moved` arm dropped him
    /// into his stance to stand there for the rest of the fight.
    #[test]
    fn a_creature_walks_past_the_edge_because_nothing_in_the_original_stops_him() {
        let d = scripted_def();
        assert_ne!(d.controller, "knight", "this def is a creature's");
        let mut f = Fighter::new("a", &d, 318, 100, 1);
        for _ in 0..8 {
            f.step(
                &d,
                Intent {
                    dx: 1,
                    dy: 0,
                    attack: false,
                },
                &field(),
            );
        }
        assert!(
            f.x > crate::arena::limit::X_HIGH,
            "walked clean past 320, not held at it: {}",
            f.x
        );
        // And the task the blit reads went with him, so no script frame pulls
        // him back. This is the jitter the old clamp caused.
        let ox = d.origin[0] as i32;
        assert_eq!(f.task.as_ref().map(|t| t.x - ox), Some(f.x));
    }

    /// The spear trogg's freeze, as the person reported it: "the spear man
    /// just stands there stuck not attacking, especially when we close the
    /// distance a bit".
    ///
    /// Inside `back_off` (0x78, 120) `MonsterTrack`'s `TrackBack` (0x5763)
    /// hands him a step away from the knight. Once that carried him out of the
    /// borders' idea of the arena our gate refused every direction, `moved`
    /// came back false, and the arm at 0x4eaa's translation put him in his
    /// stance, where he stayed. Nothing in `MonsterWalk` can refuse him, so he
    /// keeps walking and keeps his walk state.
    #[test]
    fn a_retreating_creature_at_the_edge_keeps_walking_instead_of_freezing() {
        let d = scripted_def();
        let mut f = Fighter::new("a", &d, crate::arena::limit::X_LOW, 100, 1);
        for _ in 0..6 {
            f.step(
                &d,
                Intent {
                    dx: -1,
                    dy: 0,
                    attack: false,
                },
                &field(),
            );
            assert_eq!(f.state, State::Walk, "he never drops into his stance");
        }
        assert!(
            f.x < crate::arena::limit::X_LOW,
            "and he is past the old wall: {}",
            f.x
        );
    }

    /// The person's own knight is the one `CheckBorder` holds, and it holds
    /// him by writing the probe's limit into his column *before* the step is
    /// added (0x40ed, 0x40fb), not by clamping afterwards.
    #[test]
    fn the_persons_knight_is_held_at_the_edge_by_checkborder_alone() {
        let mut d = scripted_def();
        d.controller = "knight".into();
        // A border that reaches well past `X_HIGH`, as `wa3` and its kin do,
        // so that only `CheckBorder`'s own limit can be what stops him.
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

    /// `ControlKnight`'s `A1$` to `A4$` (0x3fd6 to 0x4012): a step straight
    /// up is drawn on `KnightWalSw+0x10`, straight down on `+0x20`, and any
    /// horizontal step on row 0 whatever else is held, since the horizontal
    /// tests come last and overwrite the row. The frame counter is shared
    /// across the rows, so changing row does not restart the stride.
    #[test]
    fn the_walk_row_follows_the_direction_held() {
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
        let stop = Instr::EndFrame { end: End::Stop };
        for (i, n) in [
            "up1", "up2", "up3", "up4", "down1", "down2", "down3", "down4",
        ]
        .iter()
        .enumerate()
        {
            d.animation.insert(
                n.to_string(),
                Script::new(vec![body(1 + (i as u8 & 3)), stop.clone()]),
            );
        }
        d.scripts.insert(
            "walk_up".into(),
            ["up1", "up2", "up3", "up4"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        );
        d.scripts.insert(
            "walk_down".into(),
            ["down1", "down2", "down3", "down4"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        );
        let mut f = Fighter::new("k", &d, 100, 100, 1);
        let mut go = |dx: i32, dy: i32| {
            f.step(
                &d,
                Intent {
                    dx,
                    dy,
                    attack: false,
                },
                &field(),
            );
            f.task.as_ref().unwrap().pc.script.clone()
        };
        assert_eq!(go(0, -1), "up1", "straight up: the +0x10 row");
        assert_eq!(go(0, -1), "up2");
        assert_eq!(
            go(0, 1),
            "down3",
            "straight down: the +0x20 row, same frame count"
        );
        assert_eq!(go(1, 1), "walk4", "down and right: right wins, row 0");
        assert_eq!(go(-1, -1), "walk1", "up and left: left wins, row 0");
        assert_eq!(d.walk_row(0, 0), d.scripts_for("walk"));
        // A creature whose table has one row, as `TrollWal` and `MudmenWal`.
        let one = scripted_def();
        assert_eq!(one.walk_row(0, -1), one.scripts_for("walk"));
    }

    /// `K_WalkRValue`/`K_WalkUpValue`/`K_WalkDownValue`
    /// (0x77fe/0x7808/0x7810), `docs/COMPLETE.md`:589.
    ///
    /// The person-controlled knight's own walk, built from `scripted_def()`
    /// with `controller` set to the string only the real baked knight ever
    /// carries and `script_ticks` shortened to 3 so a test only has to hold
    /// a handful of ticks to see the pulse: one tick in three actually
    /// moves, the walk-cycle frame that `ControlKnight` (0x3ec4) itself
    /// would be showing right then, and the other two do not move at all.
    fn person_knight_def(script_ticks: u32) -> ActorDef {
        ActorDef {
            controller: "knight".into(),
            script_ticks,
            ..scripted_def()
        }
    }

    /// Right and left share `K_WalkRValue` (`[25, 3, 23, 4]`), sign only,
    /// and the table is read once a displayed frame — three ticks here —
    /// not once every tick: a naive per-tick read of the flat
    /// `def.speed_x`/`speed_y` this replaces would move every one of these
    /// twelve ticks; the original, and this, move on four of them.
    #[test]
    fn a_person_knight_walks_the_r_table_once_a_frame_right_and_left() {
        let d = person_knight_def(3);
        let mut f = Fighter::new("k", &d, 100, 100, 1);
        let mut xs = Vec::new();
        for _ in 0..12 {
            f.step(
                &d,
                Intent {
                    dx: 1,
                    dy: 0,
                    attack: false,
                },
                &field(),
            );
            xs.push(f.x);
        }
        assert_eq!(
            xs,
            [125, 125, 125, 128, 128, 128, 151, 151, 151, 155, 155, 155],
            "+25, then flat, then +3, flat, +23, flat, +4, flat: K_WalkRValue, \
             once every three ticks"
        );

        let mut f = Fighter::new("k", &d, 100, 100, -1);
        let mut xs = Vec::new();
        for _ in 0..12 {
            f.step(
                &d,
                Intent {
                    dx: -1,
                    dy: 0,
                    attack: false,
                },
                &field(),
            );
            xs.push(f.x);
        }
        assert_eq!(
            xs,
            [75, 75, 75, 72, 72, 72, 49, 49, 49, 45, 45, 45],
            "the same table negated, held left"
        );
    }

    /// `KnightWalkUp` (0x4067) always negates `K_WalkUpValue`
    /// (`[2, 9, 2, 9]`); `KnightWalkDown` (0x4080) never negates
    /// `K_WalkDownValue` (`[8, 2, 9, 2]`) — two separate tables, not one
    /// table mirrored by sign, and the asymmetry (2 and 9 swap places
    /// between them) is the point.
    #[test]
    fn a_person_knight_walks_the_up_and_down_tables_unmirrored() {
        let d = person_knight_def(3);
        let mut f = Fighter::new("k", &d, 100, 100, 1);
        let mut ys = Vec::new();
        for _ in 0..12 {
            f.step(
                &d,
                Intent {
                    dx: 0,
                    dy: -1,
                    attack: false,
                },
                &field(),
            );
            ys.push(f.y);
        }
        assert_eq!(
            ys,
            [98, 98, 98, 89, 89, 89, 87, 87, 87, 78, 78, 78],
            "-2, -9, -2, -9: K_WalkUpValue, always negated"
        );

        let mut f = Fighter::new("k", &d, 100, 100, 1);
        let mut ys = Vec::new();
        for _ in 0..12 {
            f.step(
                &d,
                Intent {
                    dx: 0,
                    dy: 1,
                    attack: false,
                },
                &field(),
            );
            ys.push(f.y);
        }
        assert_eq!(
            ys,
            [108, 108, 108, 110, 110, 110, 119, 119, 119, 121, 121, 121],
            "+8, +2, +9, +2: K_WalkDownValue, never negated"
        );
    }

    /// The four direction tests are independent in the original
    /// (0x3f5d/0x3f68/0x3f71/0x3f80), not an if/else chain, so a diagonal
    /// held sets both axes the same tick from the SAME table index — one
    /// counter driving all three tables in lockstep.
    #[test]
    fn a_person_knight_walking_diagonally_sets_both_axes_from_one_index() {
        let d = person_knight_def(3);
        let mut f = Fighter::new("k", &d, 100, 100, 1);
        let mut xy = Vec::new();
        for _ in 0..6 {
            f.step(
                &d,
                Intent {
                    dx: 1,
                    dy: 1,
                    attack: false,
                },
                &field(),
            );
            xy.push((f.x, f.y));
        }
        assert_eq!(
            xy,
            [
                (125, 108),
                (125, 108),
                (125, 108),
                (128, 110),
                (128, 110),
                (128, 110),
            ],
            "K_WalkRValue[idx] on x and K_WalkDownValue[idx] on y, the same \
             idx, the same tick"
        );
    }
    /// The spear trogg's attack window is ten pixels wide, and a flat speed
    /// stepped straight over it.
    ///
    /// `SetTroggSpTables` (0x2220) gives him approach 0x82 and back off 0x78,
    /// so he lunges only while the gap is 121 to 130. `TroggWALKR` (DS:0x7746)
    /// moves him 0, then 7, then 23, and those uneven steps are what put him
    /// inside it. A flat two pixels a tick over six sub-ticks moved him twelve
    /// a frame, from 118 to 130 and back for ever, and he never once stood
    /// where attacking was an option. That, not the borders, is why he still
    /// looked stuck after the border gate was lifted.
    #[test]
    fn the_troggs_table_lands_him_inside_his_attack_window_where_a_flat_speed_could_not() {
        let mut d = scripted_def();
        d.script_ticks = 6;
        d.speed_x = 0;
        d.speed_y = 0;
        // `TroggWALKR`, as the baker reads it out of the image.
        d.walk_speed = crate::content::WalkSpeed {
            right: vec![[0, -1], [7, 1], [23, 0]],
            ..crate::content::WalkSpeed::default()
        };
        let mut f = Fighter::new("t", &d, 0, 100, 1);
        let mut inside = 0;
        let mut seen = Vec::new();
        // Walk in from 250 away and count the frames spent at a gap he could
        // lunge from. The knight stands at 250, as the person's did on the
        // video.
        for _ in 0..240 {
            f.step(
                &d,
                Intent {
                    dx: 1,
                    dy: 0,
                    attack: false,
                },
                &field(),
            );
            let gap = 250 - f.x;
            if seen.last().is_none_or(|g| *g != gap) {
                seen.push(gap);
            }
            if (121..=130).contains(&gap) {
                inside += 1;
            }
        }
        assert!(
            inside > 0,
            "he never stood inside 121..=130; the gaps he stood at were {seen:?}"
        );
        // And the reason: his steps are uneven, so the positions he can stop
        // at are not all congruent modulo one stride.
        assert_eq!(
            &seen[..4],
            &[250, 243, 220, 213],
            "0, then 7, then 23, once a frame each"
        );
        // Thirty pixels every three frames, but not ten every frame: the
        // gaps he can stop at are `250 - k` for `k` in 0, 7, 30, 37, 60 ...,
        // which is two residues modulo the stride rather than one. Twelve a
        // frame gave him one, and 121..=130 held none of it.
        assert!(seen.contains(&130), "and 130 is one of them: {seen:?}");
    }

    /// **A computer knight walks a table too, and it is the same table.**
    ///
    /// This test used to assert the opposite, on the reading that
    /// `ControlBlackKnight` (0x4b79) "calls the same plain movers any
    /// ordinary scripted creature does". It does call them, and they are not
    /// plain: `MoveR` (0x4e0e..0x4e1e) indexes a table its caller chose by
    /// the walk cycle times four, and `ControlBlackKnight`'s own `M0$`..`M3$`
    /// choose `BKnightWALKU`, `BKnightWALKD` and `BKnightWALKR` (0x4be6,
    /// 0x4bf2, 0x4bfe, 0x4c0a).
    ///
    /// `BKnightWALKR` (DS:0x7bb6) is `(25,0) (3,0) (23,0) (4,0)`, which is
    /// `K_WalkRValue`'s `25 3 23 4` pair by pair, and its two siblings match
    /// `K_WalkUpValue` and `K_WalkDownValue` the same way. So both seats of
    /// the knight definition walk the same distances on the same frames; the
    /// difference between them is only which routine reads the numbers.
    #[test]
    fn a_driven_knight_walks_the_black_knights_table_which_is_the_knights_own() {
        let mut d = person_knight_def(3);
        // `BKnightWALKR`/`U`/`D`, as the baker reads them out of the image.
        d.walk_speed = crate::content::WalkSpeed {
            right: vec![[25, 0], [3, 0], [23, 0], [4, 0]],
            up: vec![[0, 2], [0, 9], [0, 2], [0, 9]],
            down: vec![[0, 8], [0, 2], [0, 9], [0, 2]],
        };
        d.speed_x = 0;
        d.speed_y = 0;
        let mut f = Fighter::new("k", &d, 100, 100, 1);
        f.brain.flags |= crate::monster::flag::DRIVEN;
        // A driven fighter has no joystick, so what puts it in `State::Walk`
        // is its own controller's order, not `Intent`; once given, it keeps
        // walking on its own until a fresh order says otherwise (`else if
        // self.brain.flags & flag::DRIVEN != 0` in `step_among`).
        f.ordered = Some(Order {
            state: State::Walk,
            script: String::new(),
            attack: None,
        });
        let mut xs = Vec::new();
        for _ in 0..12 {
            f.step(
                &d,
                Intent {
                    dx: 1,
                    dy: 0,
                    attack: false,
                },
                &field(),
            );
            xs.push(f.x);
        }
        // Once a frame, three ticks to a frame here, by the table entry: the
        // same 25, 3, 23, 4 the person's own knight walks.
        assert_eq!(
            xs,
            [125, 125, 125, 128, 128, 128, 151, 151, 151, 155, 155, 155],
            "BKnightWALKR once a frame, not a flat speed every tick"
        );
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
                perch: None,
                foe_blow: 0,
                head_health: None,
            };
            let mut seed = seed;
            let mut facing = 1;
            decide(
                &s,
                brain,
                &mut seed,
                &mut facing,
                &mut crate::monster::Shared::default(),
                &mut (0, 0),
            )
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
                perch: None,
                foe_blow: 0,
                head_health: None,
            };
            let mut brain = Brain::default();
            let mut seed = wary;
            let mut f = 1;
            decide(
                &s,
                &mut brain,
                &mut seed,
                &mut f,
                &mut crate::monster::Shared::default(),
                &mut (0, 0),
            )
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
