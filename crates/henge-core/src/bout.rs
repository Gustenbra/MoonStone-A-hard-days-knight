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

use crate::arena::{Arrivals, Border, Field, Occupant, GLOBAL};
use crate::combat::{line_hits_body, Attack, Fighter, Intent, Order, State};
use crate::content::ActorDef;
use crate::monster::Controller;
use crate::sound::{self, SoundCall};
use crate::taskvm::{self, field, Effect, Task, TaskActor, FACING_LEFT, FACING_RIGHT};
use crate::wave::Wave;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
    /// This task is standing in for its owner while he is off the board, and
    /// it is over once it reaches this script.
    ///
    /// `BeastStruck1` (0x447b) hands `Beast_BackToss` to the **knight's** own
    /// task, and the script draws him out of the beast's bank tables, because
    /// `TASKCELBUF` chooses from a global `TaskCelTable` the loader filled and
    /// not from anything the actor carries. This engine looks a bank up on the
    /// actor's own definition, so the only way to draw the beast's cels is on
    /// the beast's definition: the toss runs as a task of its own out of that
    /// definition while the knight is on standby, which is the same picture
    /// and the same chain of scripts. It ends where the chain ends, at
    /// `Knight_SwStance`, and the knight comes back standing where it left
    /// him.
    #[serde(default)]
    pub until: String,
}

impl Missile {
    /// The blow of the frame being shown: the weapon parts, as rectangles.
    fn hit_line(&self, def: &ActorDef) -> Vec<(i32, i32)> {
        let mut out = Vec::new();
        for p in self
            .task
            .shown
            .iter()
            .filter(|p| p.is(taskvm::part_flags::WEAPON))
        {
            let Some(bank) = def.bank(p.table, p.bank) else {
                continue;
            };
            let t = &self.task;
            let Some(r) = taskvm::place(p, bank, (t.x, t.y, t.z), t.mirror()) else {
                continue;
            };
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
    /// What asked to be heard on the last tick, in the order the scripts asked.
    /// Output, like the parries: the simulation never reads it back, and nothing
    /// here knows what a sound id means.
    ///
    /// `TASKSOUND` (0x92, handler `0x9b38`) and the sound routines the scripts
    /// call through `TASKGOSUB` are the only things in the original that fill
    /// this, because they are the only things in the original that make a noise
    /// during a fight. See [`crate::sound`].
    #[serde(default)]
    pub sounds: Vec<SoundCall>,
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
    /// `ShakeCOUNT` (DS:0x78b8): the screen shake is due when this counts
    /// down to nought.
    ///
    /// `ShakeADD` (0x493f) is two instructions of substance -- if the count
    /// is already set, leave it; otherwise `mov word [0x78b8], 1` -- and it
    /// has exactly two callers in the game: `BalokJumping+12` (0x374b), when
    /// the Balok lands, and the script `Troll_Chop`, which gosubs it. Nothing
    /// else in the image shakes the screen.
    ///
    /// `COLCON` (0x4988) is what spends it, once per combat loop pass:
    /// `cmp word [0x78b8], 0; je; dec word [0x78b8]; jne; call ShakeScreen`.
    /// So the count is set to one and the shake runs on the very next pass.
    /// `InitCombat` (0x33e) clears it at the start of a fight.
    ///
    /// It lives here, in the simulation, because it is a small deterministic
    /// integer and both peers of a lockstep fight must agree on it. What the
    /// shake *looks* like does not: `ShakeScreen`'s fifteen random offsets
    /// are the renderer's, and are not rolled from this crate's seed.
    #[serde(default)]
    pub shake_count: u32,
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
    /// `DS:0x5b1`, the day count the computer knight's nerve is read off.
    ///
    /// `InitGameStart+60` (0x1c49) writes zero and `EncounterFini+35` (0x1167)
    /// adds one every fourth encounter, in step with `MoonCount`; `BKBlock`
    /// (0x4c45) and `BKAttack` (0x4cc8) are its only readers, and they index
    /// [`crate::monster::PROGRESSION`] with it. Zero is the practice duel,
    /// which `PracticeCombat5` reaches straight out of `InitGameStart`.
    #[serde(default)]
    pub progression: i32,
    /// The words a fight keeps for a whole species: `RatFLAGS` (DS:`0x779c`),
    /// `HitDelay` (`0x779e`), `BalokFLAGS` (`0x7794`) and the two scratch
    /// words the jumps leave behind. See [`crate::monster::Shared`].
    #[serde(default)]
    pub shared: crate::monster::Shared,
    /// The tree `InitKnightvsRatmen+82` (0x236f) stands in the middle of a
    /// ratmen fight, which is the only thing `RatmanLeap` (0x31c7) aims a
    /// first leap at.
    #[serde(default)]
    pub perch: Option<crate::monster::Perch>,
    /// `StopCombat` (0x231) has run: DS:0x897d is down and the fight is over
    /// but for the thirty five frames DS:0x8987 counts out.
    #[serde(default)]
    pub stopped: bool,
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
            sounds: Vec::new(),
            rng: default_rng(),
            decap: false,
            // `InitCombat+13` (0x33e): every fight opens with it clear.
            shake_count: 0,
            wave: Wave::default(),
            arrivals: Arrivals::default(),
            progression: 0,
            shared: crate::monster::Shared::default(),
            perch: None,
            stopped: false,
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
        // `StopCombat` itself, from whichever script called it: the dragon's
        // fight has no wave and two claws that never take a hit point, so
        // its two endings, `Dragon_Dead` (0x4030) and the knight's own
        // death scripts, are the only things that can end it.
        if self.stopped {
            return true;
        }
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
        let y = self.field.standing_row(self.arrivals.next_place());
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
                && f.task.as_ref().is_some_and(|t| t.active && t.running)
        })
    }

    /// One tick, with every fighter the same kind. `intents` is indexed to
    /// match `fighters`; a short slice is treated as idle for the rest, which
    /// keeps a caller honest without panicking mid-fight.
    pub fn step(&mut self, def: &ActorDef, intents: &[Intent]) -> Vec<HitEvent> {
        self.step_with(|_| def, intents)
    }

    /// What one fighter's blow of one kind takes off.
    ///
    /// The `*Dam` entry for the kind, which is the fighter's own figure, or
    /// the bout's where the actor leaves it at zero, scaled by the attack's
    /// entry against the default attack's. For a blow that goes through
    /// `CalcDamage` (0x2d67) the sheet's strength and sword are then added
    /// and a chop is doubled after the addition:
    ///
    /// ```text
    /// 02d75  mov ax, [di]              ; *Dam[kind]
    /// 02d78  mov cl, [si+0x2e]; add ax, cx   ; strength
    /// 02d7d  cmp word [si+0x40], 0x17 ... add ax, 2 / 3 / 5   ; the sword
    /// 02d98  cmp dx, 0x10; jne; shl ax, 1    ; a chop, doubled last
    /// ```
    ///
    /// Only two strikers reach it: a knight, through every creature's
    /// `*Struck` and through `KnightStruck1` (0x44b3), and a mudman, whose
    /// `MudmenStruck1` is `KnightStruck1`. Every other creature's `*Struck1`
    /// subtracts its own number and never calls it, so a trogg's chop is the
    /// table's three and not six.
    fn blow(&self, attacker: usize, def: &ActorDef, attack: Attack) -> i32 {
        let f = &self.fighters[attacker];
        let base = match f.damage {
            0 => self.damage,
            d => d,
        };
        let (num, den) = def.blow_ratio(attack);
        let table = base * num / den;
        let dealt = match def.controller() {
            Controller::Knight | Controller::Mudman => {
                let added = table + f.bonus;
                if attack == Attack::Chop {
                    added * 2
                } else {
                    added
                }
            }
            _ => table,
        };
        dealt.max(1)
    }

    /// Put a task in the arena and show its first frame at once, so it is on
    /// screen the tick it appears.
    ///
    /// That frame is a frame like any other, so a script whose first command is
    /// a `TASKSOUND` is heard now: `SpeedKnife` is, and the swish of a thrown
    /// dagger would otherwise be the one sound in the game nothing played.
    fn launch(&mut self, mut m: Missile, scripts: &taskvm::ScriptSet) {
        let frame = m.task.step(scripts, &mut m.record, self.bloodless);
        let owner = m.owner;
        for e in &frame.effects {
            if let Effect::Sound { sample } = e {
                self.sounds.push(SoundCall {
                    who: owner,
                    id: *sample,
                });
            }
        }
        self.missiles.push(m);
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
        let m = Missile {
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
            until: String::new(),
        };
        // Its first frame now, so it is on screen the tick it leaves the hand,
        // and `SpeedKnife` opens with the `TASKSOUND` for the throw.
        self.launch(m, &def.animation);
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
        let m = Missile {
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
            until: String::new(),
        };
        self.launch(m, &def.animation);
    }

    /// `AddDragonFIRE`, image 0x3afc: the high breath's fire, a task of its
    /// own.
    ///
    /// ```text
    /// 03afc  mov si, 0x6964; mov word [si+0x14], ControlMisc  ; kind 0x14's controller
    /// 03b04  mov di, 0x6e26                    ; the head
    /// 03b07  mov si, Dragon_Fire               ; the script
    /// 03b0a  mov bp, 0x8949                    ; the creature bank table
    /// 03b0d  mov ax, [di+2]                    ; x
    /// 03b10  mov bx, 0                         ; height 0
    /// 03b13  mov cx, [di+6]; add cx, 5         ; z, five rows deeper
    /// 03b19  mov dh, 1                         ; facing right
    /// 03b1b  add ax, 0x37                      ; fifty five pixels along
    /// 03b1e  mov dl, 0x14                      ; kind 0x14
    /// 03b20  call 0x3e7d                       ; FindTABLE and ADDTASK
    /// ```
    ///
    /// Kind 0x14 is `DragonFire1` in `StruckTable` (0x43ce), so what it
    /// touches is burned for thirty: see [`Bout::dragon_blow`]. `ControlMisc`
    /// (0x3ead) clears the task's `+0xc` and `+0xe` and carries on, so the
    /// fire is not put out by touching him; here a missile that connects is
    /// spent, and the knight it burned has no `BODY` parts to be found by
    /// while `Knight_Burn` plays, which comes to one burn per breath either
    /// way.
    fn breathe(&mut self, owner: usize, script: &str, def: &ActorDef) {
        if !def.animation.contains_key(script) {
            return;
        }
        let f = &self.fighters[owner];
        let Some(t) = f.task.as_ref() else { return };
        // 03b1b  add ax, 0x37; 03b16 add cx, 5; 03b19 mov dh, 1; 03b10 mov bx, 0
        let mut task = Task::new(script, t.x + 0x37, t.y + 5, FACING_RIGHT);
        task.table = def.bank_table;
        task.z = 0;
        let m = Missile {
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
            until: String::new(),
        };
        self.launch(m, &def.animation);
    }

    /// `AddBlood`: a spray at the strike point, facing the way the knight
    /// does, on bank table 4, which is `BLO.CEL` five times over. Every part
    /// of `Blood1` is gated, so with the gore off the task runs its five
    /// frames drawing nothing and kills itself.
    fn add_blood(&mut self, owner: usize, at: (i32, i32), depth: i32, actor: &str, def: &ActorDef) {
        if !def.animation.contains_key("Blood1") || !def.banks.contains_key(&4) {
            return;
        }
        let facing = if self.fighters[owner].facing < 0 {
            FACING_LEFT
        } else {
            FACING_RIGHT
        };
        let mut blood = Task::new("Blood1", at.0, at.1, facing);
        blood.table = 4;
        let m = Missile {
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
            until: String::new(),
        };
        self.launch(m, &def.animation);
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

    /// `RatmanHit`, image 0x34f0: the ratman's own `+0xc` branch, and the
    /// two things it does that nothing else in the game does.
    ///
    /// ```text
    /// 034f0  mov si, [di+0xc]                 ; whoever it hit
    /// 034f3  cmp byte [si+0x35], 0x12
    /// 034f7  jne 034fc; jmp ControlRatCollide ; another rat: nothing
    /// 034fc  test byte [di+0x48], 1;  jne RatLeapHit    ; it was in the air
    /// 03502  test byte [di+0x48], 8;  jne RatTailHit    ; it was in the tree
    /// 03508  mov word [0x783a], 0xffff        ; carry the claw through
    /// 0350e  mov word [di+0x48], 0
    /// 03513  mov word [HitDelay], 0xf         ; and no rat claws for fifteen
    /// 03519  mov al, [si+8]; cmp al, [di+8]   ; FlipKnight, see below
    /// ```
    ///
    /// `+0x48 & 0x80`, the short hop, is **not** tested here, so a hop that
    /// connects takes the ordinary path and the `mov word [di+0x48], 0` at
    /// 0x350e ends the hop where it stands. The jump's slot is left occupied
    /// in the original and nothing steps it again; dropping it is the same
    /// thing.
    ///
    /// `FlipKnight` (0x3d13) writes the *task*'s `+0x14` and lets `perdone`
    /// (0x99d2) carry it into the record, so the fighter and the task are both
    /// turned here; leaving the task alone would have shown the old mirror
    /// until the next hand-over. See [`crate::monster::ratman_flips`].
    fn ratman_hit(&mut self, attacker: usize, target: usize, def: &ActorDef) -> bool {
        if def.controller() != Controller::Ratman {
            return false;
        }
        // 034f3: a rat's claw does nothing at all to another rat.
        if self.fighters[attacker].actor == self.fighters[target].actor {
            return false;
        }
        let flags = self.fighters[attacker].brain.flags;
        // 034fc  test byte ptr [di + 0x48], 1
        if flags & crate::monster::flag::LEAPING != 0 {
            return self.rat_leap_hit(attacker, target, def);
        }
        // 03502  test byte ptr [di + 0x48], 8
        if flags & crate::monster::flag::IN_TREE != 0 {
            return self.rat_tail_hit(attacker, target, def);
        }
        // 0350e / 03513.
        self.clear_rat_flags(attacker);
        self.shared.hit_delay = 0xf;
        let (victim, ratman) = (self.fighters[target].facing, self.fighters[attacker].facing);
        if !crate::monster::ratman_flips(false, false, victim, ratman) {
            // 03508  mov word ptr [0x783a], 0xffff: carry the claw through.
            return true;
        }
        let f = &mut self.fighters[target];
        f.facing = -victim;
        if let Some(t) = f.task.as_mut() {
            // 03d26  xor byte ptr [si + 0x14], 2
            t.facing ^= 2;
        }
        true
    }

    /// `+0x48` and `+0x49` put down together, which four of the ratman's
    /// branches do with one `mov word [di+0x48], 0`.
    fn clear_rat_flags(&mut self, who: usize) {
        use crate::monster::flag;
        let f = &mut self.fighters[who];
        f.brain.flags &= !(flag::LEAPING
            | flag::TREE_BOUND
            | flag::IN_TREE
            | flag::GOUGING
            | flag::ON_HEAD
            | flag::SHORT_HOP
            | flag::RELEASING
            | flag::HANGING);
        f.brain.jump = None;
        f.brain.height = 0;
    }

    /// `RatLeapHit`, image 0x353f: a leap that lands on somebody puts the rat
    /// on his head for as long as his endurance holds out.
    ///
    /// ```text
    /// 0353f  cmp byte [si+0x35], 0x12; jne; jmp RatmanLeaping
    /// 03548  test word [RatFLAGS], 0x20
    /// 0354e  jne RatmanLeaping                ; one is up there already
    /// 03553  mov word [di+0x48], 0; or byte [di+0x48], 0x20
    /// 0355c  or  word [RatFLAGS], 0x20
    /// 03561  mov al, [si+0x30]; add al, 6     ; his endurance, plus six
    /// 03567  mov [di+0x4a], ax
    /// 0356a  mov word [0x783a], Ratman_SitOnHead
    /// 03570  mov word [di+4], 0               ; and it is on the ground again
    /// 03575  mov ax, si; call TASKSTANDBY     ; he is drawn inside its frame
    /// ```
    fn rat_leap_hit(&mut self, attacker: usize, target: usize, def: &ActorDef) -> bool {
        use crate::monster::{flag, rat_flag};
        // 03548: one rat at a time, and the rest carry on falling.
        if self.shared.rat & rat_flag::ON_HEAD != 0 {
            return true;
        }
        self.clear_rat_flags(attacker);
        self.shared.rat |= rat_flag::ON_HEAD;
        let endurance = self.fighters[target]
            .record
            .get(crate::monster::ENDURANCE)
            .clamp(0, 255);
        let script = def.scripts_for("sit").first().cloned().unwrap_or_default();
        if script.is_empty() {
            return false;
        }
        let f = &mut self.fighters[attacker];
        f.brain.flags |= flag::ON_HEAD;
        f.brain.cooldown = endurance + 6;
        f.ordered = Some(Order {
            state: State::Attack,
            script,
            attack: None,
        });
        self.fighters[target].holder = Some(attacker);
        self.fighters[target].hidden = true;
        true
    }

    /// `RatTailHit`, image 0x357d: the tail a rat hangs into the arena from
    /// its tree, and what it catches with it.
    ///
    /// ```text
    /// 0357d  mov word [di+0x48], 0
    /// 03582  or  byte [di+0x49], 4
    /// 03586  or  word [RatFLAGS], 8
    /// 0358b  mov word [0x783a], Ratman_SnagKnight
    /// 03591  mov ax, si; call TASKSTANDBY
    /// ```
    fn rat_tail_hit(&mut self, attacker: usize, target: usize, def: &ActorDef) -> bool {
        use crate::monster::{flag, rat_flag};
        let script = def.scripts_for("snag").first().cloned().unwrap_or_default();
        if script.is_empty() {
            return false;
        }
        let height = self.fighters[attacker].brain.height;
        self.clear_rat_flags(attacker);
        self.shared.rat |= rat_flag::HANGING;
        let f = &mut self.fighters[attacker];
        // The rat stays where it was: only `+0x48`/`+0x49` change, and it is
        // still up the tree while it holds him.
        f.brain.height = height;
        f.brain.flags |= flag::HANGING;
        f.ordered = Some(Order {
            state: State::Attack,
            script,
            attack: None,
        });
        self.fighters[target].holder = Some(attacker);
        self.fighters[target].hidden = true;
        true
    }

    /// `BalokHit`, image 0x377a, and `BalokGrabbed` (0x379b): the grab that
    /// connects takes hold of the knight rather than staggering him.
    ///
    /// ```text
    /// 0377a  mov di, [si+0xc]
    /// 0377d  cmp word [si+0x28], 0x10        ; the grab
    /// 03781  je  BalokGrabbed
    /// 03783  cmp word [si+0x28], 4           ; the uppercut
    /// 03787  jne 03792
    /// 03789  mov word [0x783a], Balok_SlapRecover
    /// 03792  mov ax, [si+0x12]               ; anything else: Balok_Recover
    /// BalokGrabbed:
    /// 0379b  mov ax, di; call TASKSTANDBY
    /// 037a0  mov word [0x783a], Balok_GrabKnight
    /// 037a6  or  word [BalokFLAGS], 1
    /// ```
    fn balok_hit(&mut self, attacker: usize, target: usize, def: &ActorDef) -> bool {
        if def.controller() != Controller::Balok {
            return false;
        }
        // 0377d  cmp word ptr [si + 0x28], 0x10: the grab and nothing else.
        // The uppercut's own branch is `Balok_SlapRecover` and everything
        // else's is `+0x12`, which is what [`Fighter::recover`] already gives.
        if self.fighters[attacker].attack != Some(Attack::Chop) {
            return false;
        }
        if !def.animation.contains_key("Balok_GrabKnight") {
            return false;
        }
        self.shared.balok |= crate::monster::balok_flag::HELD;
        // `BalokGrabbed` writes `[0x783a]` itself, so this is the controller's
        // own answer rather than an order it can be talked out of: the flag is
        // what `ControlBalok` reads on its next pass, and the script is what
        // this frame turns into.
        self.fighters[attacker].brain.flags |= crate::monster::flag::GRABBED;
        self.fighters[attacker].ordered = Some(Order {
            state: State::Attack,
            script: "Balok_GrabKnight".into(),
            attack: None,
        });
        self.fighters[target].holder = Some(attacker);
        self.fighters[target].hidden = true;
        true
    }

    /// `KnightGotStruck` (0x4267): the facing its table writes into whoever
    /// took the blow, which is a demon's slap and a dragon's claw and nothing
    /// else. See [`crate::monster::struck_facing`].
    ///
    /// The write is `mov [di+8], al` on the record, and the task takes it on
    /// the same frame because the blow-taken script is a hand-over: the
    /// original's `TASKHANDLE` (0x9741) and this engine's `restart` both put
    /// the new script and the new facing on the task together.
    fn turn_struck(&mut self, attacker: usize, target: usize, a_def: &ActorDef) {
        let kind = self.fighters[attacker].attack;
        let facing = self.fighters[attacker].facing;
        if let Some(turned) = crate::monster::struck_facing(a_def.controller(), kind, facing) {
            self.fighters[target].facing = turned;
        }
    }

    /// The task loop's `+0xe` branch: `TroggStruck` (0x2f19) zeroes `+0x4a`
    /// (0x2f1c) beside picking the blow-taken script, which
    /// [`Fighter::struck`] already did. No `FaceKnight` on this path either.
    fn got_struck(&mut self, target: usize, def: &ActorDef, by: Option<Attack>) {
        if matches!(def.controller(), Controller::Trogg | Controller::TroggSpear) {
            crate::monster::trogg_struck(&mut self.fighters[target].brain);
        }
        if def.controller() == Controller::Ratman {
            self.ratman_struck(target, def, by);
        }
    }

    /// `RatmanStruck` (0x3465) and `KnightStruckRatInAir` (0x34b5): what a
    /// blow does to a ratman, which depends on what it was doing when the
    /// blow arrived.
    ///
    /// ```text
    /// 03465  mov si, [di+0xe]                  ; whoever struck it
    /// 03468  cmp byte [si+0x35], 6
    /// 0346c  je 03471; jmp ControlRatCollide   ; not a joystick knight: nothing
    /// 03471  test byte [di+0x48], 1
    /// 03475  jne KnightStruckRatInAir
    /// 03477  cmp word [si+0x28], 8;    je 03483    ; a block
    /// 0347d  cmp word [si+0x28], 0xe; jne 0349b    ; an evade
    /// 03483  mov word [0x783a], Ratman_HitOnHead
    /// 03489  mov word [di+0x48], 0
    /// 0348e  mov word [di+4], 0
    /// 03493  mov word [di+0x38], 0xffff          ; and that is the end of it
    /// 0349b  the ordinary blow: CalcDamage and the `*Hit` row
    /// KnightStruckRatInAir:
    /// 034b5  cmp word [si+0x28], 0xc; je 034d8    ; the up thrust
    /// 034bb  CalcDamage; mov word [di+0x48], 0; mov word [di+4], 0
    /// 034cf  mov word [0x783a], Ratman_Stabbed
    /// 034d8  mov word [0x783a], Ratman_KnockDead
    /// 034de  mov word [di+0x48], 0; mov word [di+4], 0
    /// 034e8  mov word [di+0x38], 0xffff
    /// ```
    ///
    /// So a block or an evade **kills** a ratman outright, and so does an up
    /// thrust that catches one in the air: three of the eight attack kinds end
    /// it whatever its hit points say.
    ///
    /// The kind test at 0x3468 is read and **not** reproduced. `+0x35` 6 is
    /// `ControlKnight`, a knight with a joystick, so in the original a rat is
    /// unhurt by another rat and by the computer knight alike; this engine has
    /// already taken the damage off by the time the branch is reached, and
    /// undoing it here would be a worse lie than leaving the gate out of a
    /// fight the player is always in.
    fn ratman_struck(&mut self, target: usize, def: &ActorDef, by: Option<Attack>) {
        let airborne = self.fighters[target].brain.flags & crate::monster::flag::LEAPING != 0;
        // The three branches, and whether each one is the end of it.
        let (script, fatal) = match (airborne, by) {
            // 034b5  cmp word ptr [si + 0x28], 0xc; 034e8 hit points to -1
            (true, Some(Attack::UThrust)) => ("Ratman_KnockDead", true),
            // 034bb: anything else that catches it in the air takes its
            // damage the ordinary way, which `Fighter::struck` has done.
            (true, _) => ("Ratman_Stabbed", false),
            // 03477 / 0347d: a block or an evade, and 03493 hit points to -1.
            (false, Some(Attack::Block | Attack::Evade)) => ("Ratman_HitOnHead", true),
            _ => return,
        };
        if !def.animation.contains_key(script) {
            return;
        }
        self.clear_rat_flags(target);
        let f = &mut self.fighters[target];
        if fatal {
            // `mov word ptr [di+0x38], 0xffff`.
            f.health = 0;
        }
        f.state = State::Idle;
        f.enter_on(if fatal { State::Dead } else { State::Hurt }, script.into());
    }

    /// A task in the arena that fights nobody: the ratmen's tree.
    ///
    /// `InitKnightvsRatmen+82` (0x236f) builds it out of `SetDecapFLAG+7`
    /// (0x3e7d), which is the general "put an actor here on this script"
    /// routine, with kind `0x14` and `ControlMisc` (0x3ead) for a controller,
    /// and `ControlMisc` does nothing at all but keep `[0x783a]` at `0xffff`.
    /// Here it is a task with no fighter behind it, which is the same thing
    /// with nothing left over.
    pub fn stand_scenery(
        &mut self,
        actor: &str,
        def: &ActorDef,
        row: &str,
        at: crate::monster::Perch,
    ) {
        let Some(script) = def.scripts_for(row).first().cloned() else {
            return;
        };
        let mut task = Task::new(&script, at.x, at.y + def.origin[1] as i32, FACING_RIGHT);
        task.table = def.bank_table;
        task.z = at.height;
        let m = Missile {
            owner: 0,
            actor: actor.to_string(),
            task,
            record: TaskActor::default(),
            depth: at.y,
            attack: None,
            flight: String::new(),
            script_tick: 0,
            spent: false,
            follow: false,
            until: String::new(),
        };
        let scripts = def.animation.clone();
        self.launch(m, &scripts);
    }

    /// `BeastStruck1`, image 0x4430: the two things a beast does to whoever it
    /// runs into, which are the only blows in the game that pick the animation
    /// off which way the two are *facing*.
    ///
    /// ```text
    /// 04430  sub word [di+0x38], 5
    /// 04434  cmp word [di+0x38], 0; jg 0447b        ; he lived: a toss
    /// 0443a  cmp word [GORESWITCH], 0; jne 0447b    ; gore off: a toss
    /// 04441  mov word [0x7834], 1
    /// 04447  mov si, [di+0xe]                       ; the beast
    /// 0444a  mov al, [di+8]; cmp al, [si+8]
    /// 04450  je  04457
    /// 04452  mov si, Beast_ImpaleChest              ; facing each other
    /// 04457  mov si, Beast_ImpaleBack               ; facing the same way
    /// 0445a  mov di, [di+0xe]; call REPLACEANIM     ; on the beast's own task
    /// 04466  call StopCombat
    /// 04469  call KillKnight
    /// 0446c  mov word [0x783a], 0
    /// 04472  mov word [0x788b], 1
    /// 0447b  mov si, [di+0xe]
    /// 0447e  mov al, [di+8]; cmp al, [si+8]; je 0448f
    /// 04486  mov word [0x783a], Beast_ChestToss     ; facing each other
    /// 0448f  mov word [0x783a], Beast_BackToss      ; facing the same way
    /// ```
    ///
    /// The impale is played by the **beast**, the toss by the **knight**, and
    /// both draw out of the beast's bank tables; see [`Missile::until`] for
    /// how the toss reaches the screen here. The five hit points at 0x4430 are
    /// the beast's `*Dam` figure and are already off by the time this runs.
    fn beast_tosses(
        &mut self,
        attacker: usize,
        target: usize,
        a_def: &ActorDef,
        t_def: &ActorDef,
    ) -> bool {
        if a_def.controller() != Controller::Beast {
            return false;
        }
        // 0444a / 0447e: the same comparison twice, on `+8` either side.
        let same_way = self.fighters[target].facing == self.fighters[attacker].facing;
        // 04434 / 0443a: the impale wants him dead and the gore switch on.
        if self.fighters[target].health <= 0 && !self.bloodless {
            let script = if same_way {
                "Beast_ImpaleBack"
            } else {
                "Beast_ImpaleChest"
            };
            if !a_def.animation.contains_key(script) {
                return false;
            }
            self.fighters[target].hidden = true;
            self.fighters[target].holder = Some(attacker);
            self.fighters[attacker].ordered = Some(Order {
                state: State::Attack,
                script: script.to_string(),
                attack: None,
            });
            return true;
        }
        if self.fighters[target].health <= 0 {
            return false;
        }
        let script = if same_way {
            "Beast_BackToss"
        } else {
            "Beast_ChestToss"
        };
        if !a_def.animation.contains_key(script) || !a_def.animation.contains_key("Knight_SwStance")
        {
            return false;
        }
        let (x, y, facing) = {
            let f = &self.fighters[target];
            match f.task.as_ref() {
                Some(t) => (t.x, t.y, t.facing),
                None => (f.x, f.y + t_def.origin[1] as i32, 1),
            }
        };
        let mut task = Task::new(script, x, y, facing);
        task.table = a_def.bank_table;
        let depth = self.fighters[target].y;
        let m = Missile {
            owner: target,
            actor: self.fighters[attacker].actor.clone(),
            task,
            record: TaskActor::with_health(self.fighters[target].health),
            depth,
            attack: None,
            flight: String::new(),
            script_tick: 0,
            spent: false,
            follow: false,
            until: "Knight_SwStance".into(),
        };
        self.fighters[target].hidden = true;
        let scripts = a_def.animation.clone();
        self.launch(m, &scripts);
        true
    }

    /// What a blow in the dragon's fight takes off, on either side of it.
    ///
    /// The knight's side is `StruckTable` (DS:0x7843) at the three kinds
    /// `SetKnightStruckTable` (0x4172) fills for this fight, and none of the
    /// three calls `CalcDamage`:
    ///
    /// ```text
    /// DragonStruck1:                          ; kind 0xa, the head
    /// 043ad  mov ax, [si+6]; mov [di+6], ax
    /// 043b3  sub word [di+6], 1              ; see dragon_struck_knight
    /// 043b7  cmp word [si+0x28], 2
    /// 043bb  jne DragonFire1
    /// 043bd  mov ax, 0x14                     ; the bite is twenty
    /// 043c0  jmp DragonDamage
    /// DragonFire1:                            ; kind 0x14, the fire task
    /// 043c2  mov ax, 0x1e                     ; the fire is thirty
    /// 043c5  mov word [si+0x28], 0x10
    /// DragonDamage:
    /// 043ca  call TalismanWrym
    /// 043cd  sub word [di+0x38], ax
    /// 043d0  jmp KnightSAnim
    /// ClawStruck1:                            ; kind 0x16, a claw
    /// 043d3  mov ax, 0xa                      ; the claw is ten
    /// 043d6  call TalismanWrym
    /// 043d9  sub word [di+0x38], ax
    /// ```
    ///
    /// The twenty, the thirty and the ten are the `attacks` rows the pack
    /// carries for the dragon and the claw, so what comes in as `damage` is
    /// already that number; `DragonDam` (DS:0x6aac), which
    /// `InitKnightvsDragon+37` (0x245c) also fills, is never read for a
    /// knight, because his `StruckTable` entries do not go through
    /// `CalcDamage`. [`crate::monster::talisman_wrym`] is the one thing left
    /// to do to it.
    ///
    /// The dragon's side is `DragonStruck` (0x3a73), which takes `CalcDamage`
    /// for a knight's blow (0x3aa2) and a flat three for a knife (0x3acf:
    /// `sub word [di+0x38], 3`).
    fn dragon_blow(
        &self,
        target: usize,
        damage: i32,
        kind: Option<Attack>,
        a_def: &ActorDef,
        t_def: &ActorDef,
    ) -> i32 {
        // 043ca / 043d6  call TalismanWrym
        if matches!(a_def.controller(), Controller::Dragon | Controller::Claw) {
            return crate::monster::talisman_wrym(damage, self.fighters[target].talismans);
        }
        // 03a7c  cmp byte [si+0x35], 0x1a; 03acf sub word [di+0x38], 3
        if t_def.controller() == Controller::Dragon && kind == Some(Attack::Knife) {
            return 3;
        }
        damage
    }

    /// `DragonStruck`, image 0x3a73: the dragon's own `+0xe` branch.
    ///
    /// ```text
    /// 03a73  mov si, [di+0xe]                 ; whoever struck it
    /// 03a76  cmp byte [si+0x35], 6; je 03a9c  ; a knight
    /// 03a7c  cmp byte [si+0x35], 0x1a; je 03ac0   ; a knife
    /// 03a82  mov ax, 0x6e26; call 0x96a2      ; anything else: its own task
    /// 03a8a  cmp byte [di+1], 0; jne 03a93
    /// 03a90  jmp DragonMove                   ; idle: as if nothing happened
    /// 03a93  mov word [0x783a], 0xffff        ; busy: carry on
    /// 03a99  jmp NOTEND+3
    /// 03a9c  or  word [DragonFLAGS], 0x80     ; a knight: struck
    /// 03aa2  call CalcDamage; sub [di+0x38], ax
    /// 03aa8  mov ax, [si+0x28]; mov di, [di+0x14]; ... mov ax, [di+ax]
    /// 03ab3  mov [0x783a], ax                 ; the `DragonHit` row: Dragon_Hit
    /// 03ab7  mov si, 0x6e26; call AddBlood
    /// 03abd  jmp NOTEND+3
    /// 03ac0  or  word [DragonFLAGS], 0x80     ; a knife: struck
    /// 03ac6  mov si, [di+0x14]; mov ax, [si+6]; mov [0x783a], ax   ; Dragon_Hit
    /// 03acf  sub word [di+0x38], 3
    /// 03ad3  jmp 03ab7                        ; and the blood
    /// ```
    ///
    /// The damage, the `*Hit` row and the blood are [`Fighter::struck`] and
    /// [`Bout::add_blood`] with the dragon's `bleeds`; the bit is the one
    /// thing left, and it is raised for a knight's blow or a knife's and for
    /// nothing else.
    fn dragon_struck(
        &mut self,
        missile: bool,
        kind: Option<Attack>,
        a_def: &ActorDef,
        t_def: &ActorDef,
    ) {
        if t_def.controller() != Controller::Dragon {
            return;
        }
        // 03a76  cmp byte ptr [si + 0x35], 6
        // 03a7c  cmp byte ptr [si + 0x35], 0x1a
        let knight = !missile && a_def.controller() == Controller::Knight;
        let knife = missile && kind == Some(Attack::Knife);
        if knight || knife {
            // 03a9c / 03ac0  or word ptr [DragonFLAGS], 0x80
            self.shared.dragon |= crate::monster::dragon_flag::STRUCK;
        }
    }

    /// The rows `InitKnightvsDragon` (0x2441) writes over the knight's own
    /// `*Hit` table (`+0x14`) for this fight, which `KnightSAnim` (0x44b9)
    /// then indexes with the striker's `+0x28`:
    ///
    /// ```text
    /// 02441  mov di, [0x77e8]; mov si, [di+0x14]
    /// 02448  mov word [si+2], Knight_SwShoulderHit   ; kind 2, the bite
    /// 0244d  mov word [si+4], Knight_Burn            ; kind 4, the low breath
    /// 02452  mov word [si+0x10], Knight_Burn         ; kind 0x10, the high
    /// 02457  mov word [si+0xa], Knight_SwSlapped     ; kind 0xa, a claw
    /// ```
    ///
    /// `DragonFire1` (0x43c5) writes 0x10 into the fire task's `+0x28`
    /// before `KnightSAnim` reads it, so the fire is the burn too. And
    /// `DragonStruck1` (0x43ad) first moves him: `mov ax, [si+6]; mov
    /// [di+6], ax; sub word [di+6], 1`, the head's own row less one, which
    /// is why a knight the head reaches is on its row from then on. The fire
    /// task's entry starts past that write, so the fire does not move him.
    fn dragon_struck_knight(
        &mut self,
        attacker: usize,
        target: usize,
        body: bool,
        kind: Option<Attack>,
        a_def: &ActorDef,
        t_def: &ActorDef,
    ) {
        if t_def.controller() != Controller::Knight
            || !matches!(a_def.controller(), Controller::Dragon | Controller::Claw)
        {
            return;
        }
        // 043ad  mov ax, [si+6]; mov [di+6], ax; 043b3 sub word [di+6], 1
        if body && a_def.controller() == Controller::Dragon {
            let row = self.fighters[attacker].y - 1;
            let (_, ny) = GLOBAL.clamp(self.fighters[target].x, row);
            self.fighters[target].y = ny;
        }
        let script = match (a_def.controller(), kind) {
            // 02448  kind 2
            (Controller::Dragon, Some(Attack::Lunge)) => "Knight_SwShoulderHit",
            // 0244d, 02452  kinds 4 and 0x10, and the fire's 0x10
            (Controller::Dragon, Some(Attack::Swing) | Some(Attack::Chop)) => "Knight_Burn",
            // 02457  kind 0xa, and `ClawStruck1+9` (0x43dc) `mov byte
            // [SLAP], 1` with it: the claw always bats him rightward,
            // whichever side of him it is. `ClawHit+9` (0x3b9f) writes the
            // same 1 from the claw's own controller on the same blow, so one
            // write here stands for both. `ClawStruck1+26` (0x43ed) leaves
            // him facing *left* — that is [`crate::monster::struck_facing`] —
            // and `KnightSLAP+19` (0x44e5) turns him back rightward on the
            // next frame of the script. Both are built; the later one wins.
            (Controller::Claw, Some(Attack::RThrust)) => {
                self.shared.slap = 1;
                // 03ba5 / 043e1  mov word [SLAPY], BalokSLAP, the same pair of
                // stores on the same blow and for the same reason.
                self.shared.slap_y = crate::monster::slap_y::BALOK_SLAP;
                "Knight_SwSlapped"
            }
            _ => return,
        };
        self.hit_row(target, script, t_def);
    }

    /// The one row `InitKnightvsBalok` (0x257c) writes over the knight's own
    /// `*Hit` table (`+0x14`) for the Balok fight:
    ///
    /// ```text
    /// 02585  mov di, [0x77ea]; mov di, [di+0x14]
    /// 0258c  mov word [di+4], 0x1596     ; kind 4: Knight_SwSlapped
    /// ```
    ///
    /// Kind 4 is the uppercut — `ControlBalok`'s branch at 0x3634 writes 4
    /// into the Balok's `+0x28` — so the uppercut throws him by the very same
    /// script a claw does, on the direction 0x3639 put in `SLAP`. The grab is
    /// kind 0x10 and has no row of its own, so it keeps whatever
    /// `Fighter::struck` chose.
    fn balok_struck_knight(
        &mut self,
        target: usize,
        kind: Option<Attack>,
        a_def: &ActorDef,
        t_def: &ActorDef,
    ) {
        if t_def.controller() != Controller::Knight
            || a_def.controller() != Controller::Balok
            || kind != Some(Attack::Swing)
        {
            return;
        }
        self.hit_row(target, "Knight_SwSlapped", t_def);
    }

    /// The two rows `InitKnightvsBeast` (0x2280) writes over the knight's own
    /// `*Hit` table (`+0x14`), which are not the row it wrote a moment earlier
    /// over his `*Att` table:
    ///
    /// ```text
    /// 002274  mov di, [0x77e8]
    /// 002278  mov si, [di+0x16]           ; the *Att* table, KnightAttSw
    /// 00227b  mov word [si+0xe], 0x11ce   ; kind 0xe: Knight_SwEvade
    /// 002280  mov si, [di+0x14]           ; si RELOADED: the *Hit* table
    /// 002283  mov word [si+0x10], 0x31d0  ; kind 0x10: Beast_BackToss
    /// 002288  mov word [si+0xe], 0x31d0   ; kind 0xe:  Beast_BackToss
    /// ```
    ///
    /// The reload at 0x2280 is the whole point: `KnightAttSw` is at DS:`0x69b0`
    /// and `KnightHitSw` at DS:`0x69c2` (`SetKnightSwTables`, 0x1f6a), so the
    /// writes at `+0xe` either side of it land in different tables and the
    /// second does not overwrite the first. See
    /// [`Bout::knight_att_rows`] for the `*Att` half.
    ///
    /// `ControlBeast+17` (0x2fb2) writes 0x10 into `+0x28` on every tick of
    /// the beast's controller and nothing in a beast fight ever writes 0xe, so
    /// the second row cannot be reached by the shipped game; it is built
    /// because it is what the routine writes.
    ///
    /// `Beast_BackToss` is a knight script kept in the beast's namespace: it
    /// opens `TASKCELBUF 2` to draw the tossed knight out of the beast's own
    /// banks and `Beast_ChestToss` closes with `TASKCELBUF 1` and
    /// `TASKGOTO Knight_GetUp`, which is the knight's.
    fn beast_struck_knight(
        &mut self,
        target: usize,
        kind: Option<Attack>,
        a_def: &ActorDef,
        t_def: &ActorDef,
    ) {
        if t_def.controller() != Controller::Knight || a_def.controller() != Controller::Beast {
            return;
        }
        // 02283 kind 0x10, 02288 kind 0xe.
        if !matches!(kind, Some(Attack::Chop) | Some(Attack::Evade)) {
            return;
        }
        self.hit_row(target, "Beast_BackToss", t_def);
    }

    /// The one row `InitKnightvsTroll` (0x26b3) writes over the knight's own
    /// `*Hit` table (`+0x14`):
    ///
    /// ```text
    /// 0026b3  mov di, [0x77ea]            ; Opponent
    /// 0026b7  mov di, [di+0x14]
    /// 0026ba  mov word [di+4], 0x1596     ; kind 4: Knight_SwSlapped
    /// ```
    ///
    /// Kind 4 is the club: `TrollBunt` (0x5689) writes 4 into the troll's
    /// `+0x28`, and the two instructions after it write `SLAP` off the troll's
    /// own `+8` and `SLAPY` off `BalokSLAP` — so the bunt throws the knight
    /// the way a Balok's uppercut does, which is [`crate::monster::troll`].
    /// The overhead chop is kind 0x10 and has no row of its own.
    ///
    /// **The global this routine reaches the knight through is `Opponent`
    /// (DS:`0x77ea`), not `[0x77e8]`**, and the common setup preamble never
    /// refreshes it: `SetKnightCombat` (0x2962) writes `[0x77e8]` into
    /// `KnightTable` and `[0x8979]` and leaves `Opponent` alone.
    /// `InitGameStart+0` (0x1c55) points it at DS:`0x6c9e`, the first of the
    /// four player records it goes on to fill, and after that only the
    /// creatures' own controllers write it — twelve stores between 0x2dee and
    /// 0x55d8, of which nine write `KnightTable`, one writes `[0x897b]` (the
    /// challenged knight of a duel) and three write the record a target search
    /// returned. Every one of those is a knight record, and all four player
    /// records are given the same `+0x14` by `SetKnightSwTables` (0x1f6f,
    /// `KnightHitSw` at DS:`0x69c2`), so the write lands on the one global hit
    /// table whichever knight `Opponent` happens to name. It is the knight's
    /// row either way, and `InitKnightvsBalok` (0x2585) reaches it the same
    /// way.
    fn troll_struck_knight(
        &mut self,
        target: usize,
        kind: Option<Attack>,
        a_def: &ActorDef,
        t_def: &ActorDef,
    ) {
        if t_def.controller() != Controller::Knight
            || a_def.controller() != Controller::Troll
            || kind != Some(Attack::Swing)
        {
            return;
        }
        self.hit_row(target, "Knight_SwSlapped", t_def);
    }

    /// The one row `InitKnightvsDemon` (0x275e) writes over the knight's own
    /// `*Hit` table (`+0x14`):
    ///
    /// ```text
    /// 00275e  mov di, [0x77e8]
    /// 002762  mov di, [di+0x14]
    /// 002765  mov word [di+0x10], 0x1596  ; kind 0x10: Knight_SwSlapped
    /// 00276a  call 0x2a12                 ; a new di: the demon's own record
    /// 002771  mov word [di+0x10], 0x5d1a  ; Demon_Evolve, its stance slot
    /// 002776  mov word [di+0x12], 0x5f28  ; Demon_Stance1, its recovery
    /// ```
    ///
    /// Kind 0x10 is the slap — `DemonAttack`'s branch at 0x504e writes it —
    /// so **a demon's slap throws the knight**, on the direction and table
    /// `DemonAttack` put in `SLAP` and `SLAPY` five instructions later
    /// ([`crate::monster::demon`]). `DemonSlap` (0x437f), the struck side of
    /// the same blow, only turns him ([`crate::monster::struck_facing`]);
    /// reading that as the slap throwing nobody was wrong, and this row is
    /// what settles it. The whip is kind 2 and keeps
    /// `Knight_SwShoulderHit`; the zap is kind 4 and keeps the waist.
    ///
    /// The demon's own two slots at 0x2771 and 0x2776 are built already: they
    /// are `Fighter::new`'s `UNBORN` flag and the recovery the pack carries.
    fn demon_struck_knight(
        &mut self,
        target: usize,
        kind: Option<Attack>,
        a_def: &ActorDef,
        t_def: &ActorDef,
    ) {
        if t_def.controller() != Controller::Knight
            || a_def.controller() != Controller::Demon
            || kind != Some(Attack::Chop)
        {
            return;
        }
        self.hit_row(target, "Knight_SwSlapped", t_def);
    }

    /// The rows an encounter's `InitKnightvs*` routine writes over the
    /// knight's `*Att` table, the `KnightAttSw` at his record's `+0x16`, by
    /// the name of the attack kind each row is.
    ///
    /// **Recovered.** Three of the thirteen routines write there, and all
    /// three write only the two guard kinds — 8, the block, and 0xe, the
    /// evade — because `SetUpKnight` (0x17ab, 0x17b0) is what put
    /// `Knight_SwEvade` and `Knight_SwBlock` in them:
    ///
    /// ```text
    /// InitKnightvsTroggSpear:
    /// 0021d1  mov di, [0x77e8]; mov si, [di+0x16]
    /// 0021d8  mov word [si+0xe], 0x11ce   ; Knight_SwEvade (the row already there)
    /// 0021dd  mov word [si+8],   0x11ce   ; Knight_SwEvade over the block
    ///
    /// InitKnightvsBeast:
    /// 002274  mov di, [0x77e8]; mov si, [di+0x16]
    /// 00227b  mov word [si+0xe], 0x11ce   ; Knight_SwEvade (the row already there)
    ///
    /// InitKnightvsRatmen:
    /// 002326  mov di, [0x978]; mov si, [di+0x16]    ; KnightTable, the knight
    /// 00232d  mov word [si+8],   0x115e   ; Knight_SwOThrust over the block
    /// 002332  mov word [si+0xe], 0x10ba   ; Knight_SwDThrust over the evade
    /// ```
    ///
    /// So against the spear trogg the block plays the evade as well, against
    /// the ratmen the block becomes an overhead thrust and the evade a
    /// downward one, and the beast's single write puts back the script that
    /// was there — the table is global and `SetKnightAnims` refills it for
    /// every fight, so it changes nothing and is kept because it is what the
    /// routine does.
    ///
    /// `Knight_SwOThrust` (DS:`0x115e`) and `Knight_SwDThrust` (DS:`0x10ba`)
    /// are in no other table in the image: the ratmen fight is the only place
    /// either is reachable.
    ///
    /// **What these rows are not** is the block table. That is `KnightBloSw`
    /// at the record's `+0x1e` (`SetKnightSwTables`, 0x1f7e), its rows hold
    /// guard *kinds* rather than script addresses, and no `InitKnightvs*`
    /// touches it — so a spear trogg's swing is still stopped by the block and
    /// its lunge by the evade ([`crate::combat::Fighter::blocks`]); only what
    /// the knight is shown doing changes. `+0x16` is `KnightAttSw`, which is
    /// why these writes are indexed by the knight's own guard kinds and hold
    /// script addresses.
    pub fn knight_att_rows(foe: Controller) -> BTreeMap<String, String> {
        let rows: &[(Attack, &str)] = match foe {
            // 0021d8, 0021dd
            Controller::TroggSpear => &[
                (Attack::Evade, "Knight_SwEvade"),
                (Attack::Block, "Knight_SwEvade"),
            ],
            // 00227b
            Controller::Beast => &[(Attack::Evade, "Knight_SwEvade")],
            // 00232d, 002332
            Controller::Ratman => &[
                (Attack::Block, "Knight_SwOThrust"),
                (Attack::Evade, "Knight_SwDThrust"),
            ],
            _ => &[],
        };
        rows.iter()
            .map(|(a, s)| (a.name().to_string(), s.to_string()))
            .collect()
    }

    /// `SetKnightCombat` (0x2962) and the `*Att` writes of the
    /// `InitKnightvs*` that follows it: the rows go on every knight in the
    /// arena, because the table they are written into is one global that every
    /// knight record's `+0x16` points at.
    ///
    /// The routine the original runs is chosen by the encounter; here the
    /// encounter is who the knight is standing against, which is the first
    /// fighter in the bout that is not a knight. A fight with no creature in
    /// it — `InitKnightvsKnight`, the practice duel — writes nothing, and
    /// neither does this.
    ///
    /// A script a knight's definition has not got is left out, so a pack that
    /// ships no `Knight_SwOThrust` keeps the row it had rather than attacking
    /// with nothing.
    pub fn init_knight_att<'a, F>(&mut self, def_of: F)
    where
        F: Fn(&str) -> &'a ActorDef,
    {
        let foe = self
            .fighters
            .iter()
            .map(|f| def_of(&f.actor).controller())
            .find(|c| *c != Controller::Knight);
        let Some(foe) = foe else { return };
        let rows = Bout::knight_att_rows(foe);
        if rows.is_empty() {
            return;
        }
        for f in &mut self.fighters {
            let def = def_of(&f.actor);
            if def.controller() != Controller::Knight {
                continue;
            }
            f.att_rows = rows
                .iter()
                .filter(|(_, s)| def.animation.contains_key(*s))
                .map(|(k, s)| (k.clone(), s.clone()))
                .collect();
        }
    }

    /// `KnightSAnim` 0x44c5: `mov [0x783a], ax`, over whatever
    /// `Fighter::struck` chose off the knight's ordinary row.
    /// `ShakeADD`, image 0x493f: ask for a shake, unless one is already due.
    pub fn shake(&mut self) {
        if self.shake_count == 0 {
            self.shake_count = 1;
        }
    }

    /// `COLCON`'s own three lines (0x4988), once per combat loop pass: spend
    /// a pending shake and say whether this is the pass it runs on.
    ///
    /// ```text
    /// 04988  cmp word ptr [0x78b8], 0
    /// 0498d  je  04998
    /// 0498f  dec word ptr [0x78b8]
    /// 04993  jne 04998
    /// 04995  call ShakeScreen
    /// ```
    pub fn take_shake(&mut self) -> bool {
        if self.shake_count == 0 {
            return false;
        }
        self.shake_count -= 1;
        self.shake_count == 0
    }

    fn hit_row(&mut self, target: usize, script: &str, t_def: &ActorDef) {
        if !t_def.animation.contains_key(script) {
            return;
        }
        let f = &mut self.fighters[target];
        if f.state == State::Hurt && f.script != script {
            f.state = State::Idle;
            f.enter_on(State::Hurt, script.to_string());
        }
    }

    /// `KnightSLAP`, image 0x44d2, and its rightward twin `KnightSLAPR`
    /// (0x4522): the knockback that flings a knight across the arena when a
    /// dragon's claw or a Balok's uppercut has landed on him.
    ///
    /// ```text
    /// KnightSLAP:
    /// 044d2  add word [SLAPCNT], 1
    /// 044d7  mov [0x77e8], di             ; current actor = this knight
    /// 044db  mov si, di
    /// 044dd  mov word [si+0x26], 0        ; clear the direction bits
    /// 044e2  mov al, [SLAP]
    /// 044e5  mov byte [si+8], al          ; his facing becomes the slap's
    /// 044e8  cmp byte [SLAP], 1
    /// 044ed  je KnightSLAPR               ; 1 = flung right
    /// 044ef  or byte [si+0x26], 2         ; otherwise flung left
    /// 044f3  call CheckBorder
    /// 044f6  test byte [si+0x26], 2
    /// 044fa  je 04521                     ; the border refused it: nothing moves
    /// 044fc  mov ax, [0x77e8]; call 0x986f; mov si, ax    ; his own task record
    /// 04504  bx = SLAPY[SLAPCNT]
    /// 04513  sub word [si+4], bx          ; and the clamp, see `slap_move`
    /// 04521  ret
    /// KnightSLAPR:
    /// 04522  or word [si+0x26], 1
    /// 04526  call CheckBorder
    /// 04529  test word [si+0x26], 1
    /// 0452e  je 04551                     ; refused: nothing moves
    /// 04530  bx = SLAPY[SLAPCNT], the same six instructions
    /// 04547  add word [si+4], bx          ; and no clamp: see `slap_move`
    /// 04551  ret
    /// ```
    ///
    /// `di` on entry is the actor whose script gosubbed it, and the script is
    /// `Knight_SwSlapped` — the knight's own blow-taken animation — so the
    /// caller *is* the victim. That is why this is keyed on `me` and not on a
    /// foe the way `KnightOFF` is.
    ///
    /// Three things about the shape are the original's and are kept:
    ///
    /// * one direction bit is set and `CheckBorder` alone is asked, never
    ///   `SBORD`, so a knight can be batted through the tree line but not off
    ///   the board ([`crate::arena::check_border`]);
    /// * `CheckBorder`'s own write-back of the column limit into `[si+2]`
    ///   (0x40ed, 0x40fb) is dropped on the floor here because it is dropped
    ///   there too: `PerformCOMMAND` (0x97fb) never copies the record's column
    ///   into the task, and `perdone` (0x99c0) copies the task's `+4` back
    ///   into the record's `+2` at the end of every frame, so whatever
    ///   `CheckBorder` wrote is overwritten before anything can read it;
    /// * the facing is taken from `SLAP` (0x44e5), which overrides the left
    ///   that `ClawStruck1+26` gave him one frame earlier.
    ///
    /// The two halves disagree about operand size on the direction byte — the
    /// leftward path is `or byte`/`test byte` at 0x44ef/0x44f6 and the
    /// rightward one `or word`/`test word` at 0x4522/0x4529, and 0x44dd clears
    /// the whole word — and it makes no difference: a scan of the image for
    /// every byte- or word-sized access with a displacement of `0x27` on any
    /// base register finds none at all, so `+0x27` is a pad and the word
    /// stores cannot reach a field.
    ///
    /// The original moves the task's own column and lets `perdone` carry it
    /// into the record; this engine keeps the position on the fighter and
    /// mirrors it into the task at the top of each `run_task`, so the column
    /// moved here is the fighter's.
    fn knight_slap(&mut self, me: usize, def: &ActorDef) {
        // 044d2  add word [SLAPCNT], 1
        self.shared.slap_cnt += 1;
        let slap = self.shared.slap;
        // 044e2  mov al, [SLAP]; 044e5 mov byte [si+8], al
        self.fighters[me].facing = if slap & 2 != 0 { -1 } else { 1 };
        // 044dd  mov word [si+0x26], 0, and then the one bit this throw wants:
        // 04522 `or 1` rightward, 044ef `or 2` leftward.
        let wanted = if slap == 1 {
            crate::arena::dir::RIGHT
        } else {
            crate::arena::dir::LEFT
        };
        let f = &self.fighters[me];
        // `CheckBorder` reads only `+2`, `+8` and `+6`; the box fields are
        // `SBORD`'s and this call never reaches it.
        let (bl, _, br, bb) = f.body(def);
        let mut probe = crate::arena::Step {
            x: f.x,
            y: f.y,
            facing: f.facing,
            dx: 0,
            dy: 0,
            box_left: bl,
            box_right: br,
            box_bottom: bb,
        };
        // 044f3 / 04526  call CheckBorder, and 044f6 / 04529 the bit it may
        // have taken away. Nothing moves if it is gone.
        if crate::arena::check_border(&mut probe, wanted) & wanted == 0 {
            return;
        }
        // 04504 / 04538  bx = SLAPY[SLAPCNT], from whichever word of
        // `BalokSLAP` the striker pointed `SLAPY` at.
        let Some(entry) = crate::monster::slap_entry(self.shared.slap_y, self.shared.slap_cnt)
        else {
            return;
        };
        // 04513 / 04547, with the clamp on the leftward side only.
        let x = self.fighters[me].x;
        self.fighters[me].x = crate::monster::slap_move(x, slap, entry);
    }

    /// `TrackKnight` (0x3be8) as a script calls it: `Dragon_HighBreath`
    /// (0x4448) and `Dragon_LowBreath` (0x4130) run it by `TASKGOSUB` on
    /// every one of their five loops, on the head's fixed record against
    /// `KnightTable`, and it writes `+2`, `+6` and `+8` where it stands and
    /// copies the first two into the task (0x3c6a, 0x3c70).
    fn track_knight<'a, F>(&mut self, me: usize, def_of: &F)
    where
        F: Fn(&str) -> &'a ActorDef,
    {
        let def = def_of(&self.fighters[me].actor);
        if def.controller() != Controller::Dragon {
            return;
        }
        // 03bf4  mov ax, [KnightTable]; mov [Opponent], ax
        let Some(foe) = self
            .fighters
            .iter()
            .position(|f| def_of(&f.actor).controller() == Controller::Knight)
        else {
            return;
        };
        let mut facing = self.fighters[me].facing;
        let mut at = (self.fighters[me].x, self.fighters[me].y);
        let mut shared = self.shared;
        crate::monster::track_knight(&self.fighters[foe], def, &mut shared, &mut facing, &mut at);
        self.shared = shared;
        // Unclamped, for the reason given in `monster_intent`: `TrackKnight`
        // carries its own column limits (0x3c31 `cmp bx, 0x64`, 0x3c44
        // `cmp bx, 0x1e`) and its depth writes at 0x3c51 and 0x3c5a have none
        // at all.
        let (nx, ny) = (at.0, at.1);
        let f = &mut self.fighters[me];
        f.facing = facing;
        f.x = nx;
        f.y = ny;
        // 03c6a  mov ax, [di+2]; mov [si+4], ax; 03c70 mov ax, [di+6]; mov [si+8], ax
        if let Some(t) = f.task.as_mut() {
            t.x = nx + def.origin[0] as i32;
            t.y = ny + def.origin[1] as i32;
        }
    }

    /// `DragonHit2`, image 0x3ad5: the dragon's own `+0xc` branch.
    ///
    /// ```text
    /// 03ad5  mov si, [di+0xc]                 ; what it hit
    /// 03ad8  cmp word [di+0x28], 2
    /// 03adc  je  03ae7
    /// 03ade  mov word [0x783a], 0xffff        ; a breath: carry on
    /// 03ae4  jmp NOTEND+3
    /// 03ae7  mov ax, [di+0xc]; call 0x96c9    ; the bite: his task killed
    /// 03aed  mov word [dragonbodge3], 1       ; and his record freed
    /// 03af3  mov word [0x783a], Dragon_BitKnight
    /// 03af9  jmp NOTEND+3
    /// ```
    ///
    /// `Dragon_BitKnight` (0x46f8) is the chewing: six loops of the head with
    /// him in its jaws and `DragonKnightSound`, then `KillKnight`, six of
    /// `DragonChewSound`, and `StopCombat`. He is drawn inside it out of the
    /// fire bank's cels 31 to 35, so his own task is gone: [`Fighter::vanish`].
    /// Answers whether it took the frame over, which for a bite that lands
    /// is always.
    fn dragon_bites(
        &mut self,
        attacker: usize,
        target: usize,
        kind: Option<Attack>,
        a_def: &ActorDef,
    ) -> bool {
        if a_def.controller() != Controller::Dragon {
            return false;
        }
        // 03ad8  cmp word ptr [di + 0x28], 2; jne
        if kind != Some(Attack::Lunge) {
            return false;
        }
        if !a_def.animation.contains_key("Dragon_BitKnight") {
            return false;
        }
        // 03aea  call 0x96c9: the task killed, the record freed.
        self.fighters[target].vanish();
        self.fighters[target].holder = Some(attacker);
        // 03aed  mov word ptr [dragonbodge3], 1
        self.shared.dragon_bodge[2] = 1;
        // 03af3  mov word ptr [0x783a], Dragon_BitKnight. `DragonHit2` is
        // the controller's own answer on the frame after the touch, which
        // `TASKHANDLE` runs it for because `+0xc` is set whether or not the
        // bite has ended, and `REPLACEANIM` puts the chewing on in the
        // bite's place there and then. So it goes on here and then, and not
        // as an order the next controller pass could talk it out of.
        let f = &mut self.fighters[attacker];
        f.state = State::Idle;
        f.enter_on(State::Attack, "Dragon_BitKnight".to_string());
        f.attack = None;
        f.ordered = None;
        true
    }

    /// `TrollStruck1` (0x438a) and `TrollOHead` (0x4397): the troll's own
    /// finisher, which is not a blow on a corpse but the blow that makes one.
    ///
    /// ```text
    /// TrollStruck1:
    /// 0438a  sub word [di+0x38], 7
    /// 0438e  cmp word [si+0x28], 0x10   ; the overhead chop
    /// 04392  je  TrollOHead
    /// 04394  jmp KnightSAnim            ; anything else is an ordinary blow
    /// TrollOHead:
    /// 04397  mov di, [0x77e8]
    /// 0439b  cmp word [di+0x38], 0
    /// 0439f  jle 043a4
    /// 043a1  jmp KnightSAnim            ; still standing: an ordinary blow
    /// 043a4  mov word [0x783a], Knight_Explode
    /// ```
    ///
    /// So a troll's chop that takes the last of a knight's hit points does not
    /// lay him down, it bursts him. Called after [`Fighter::struck`] has taken
    /// the damage off, which is where `[di+0x38]` stands when `TrollOHead`
    /// reads it.
    fn troll_explodes(
        &mut self,
        target: usize,
        kind: Option<Attack>,
        a_def: &ActorDef,
        t_def: &ActorDef,
    ) {
        // 0438e  cmp word [si+0x28], 0x10; 04392 je TrollOHead
        if a_def.controller() != Controller::Troll || kind != Some(Attack::Chop) {
            return;
        }
        // 0439b  cmp word [di+0x38], 0; 0439f jle 043a4
        if self.fighters[target].health > 0 {
            return;
        }
        let Some(script) = t_def.finishes.get("explode").cloned() else {
            return;
        };
        // 043a4  mov word [0x783a], Knight_Explode
        let f = &mut self.fighters[target];
        if let Some(t) = f.task.as_mut() {
            t.replace(script.clone());
        }
        f.script = script;
    }

    /// The rest of `TroggHit` (0x2f59), which is the spear's alone: it picks a
    /// dead knight up on the point and throws him.
    ///
    /// ```text
    /// 02f59  mov si, [di+0xc]          ; what I hit
    /// 02f5c  cmp byte [si+0x35], 6     ; a knight a person is playing
    /// 02f60  jne TroggStart
    /// 02f65  cmp word [si+0xc], 0      ; and he has not hit anything himself
    /// 02f69  jne TroggDone
    /// 02f6b  cmp word [si+0x38], 0
    /// 02f6f  jg  TroggDone             ; still standing: nothing
    /// 02f71  mov word [0x783a], 0xffff ; carry on rather than recover
    /// 02f77  cmp byte [di+0x35], 0x10  ; and only the spear
    /// 02f7b  jne TroggDone
    /// 02f7d  cmp word [0x700], 0
    /// 02f82  jne TroggDone             ; gore off: nothing
    /// 02f86  call (the knight's task is taken away)
    /// 02f89  mov word [0x783a], TroggSpear_Toss
    /// TroggDone:
    /// 02f92  cmp byte [di+0x35], 0x10
    /// 02f98  mov ax, [di+0x10]         ; the spear stands rather than
    /// 02f9b  mov [0x783a], ax          ; playing the recovery
    /// ```
    ///
    /// The corpse is drawn inside `TroggSpear_Toss` from here on, which is
    /// what the removed task means, so it is hidden the way the mudman's
    /// entangled knight is. `[si+0xc]`, "he has not hit anything himself on
    /// this frame", has no equivalent in this engine and is not tested.
    fn trogg_spear_toss(
        &mut self,
        attacker: usize,
        target: usize,
        a_def: &ActorDef,
        t_def: &ActorDef,
    ) {
        // 02f77  cmp byte [di+0x35], 0x10; 02f5c cmp byte [si+0x35], 6
        if a_def.controller() != Controller::TroggSpear
            || t_def.controller() != Controller::Knight
            || self.fighters[target].brain.flags & crate::monster::flag::DRIVEN != 0
        {
            return;
        }
        // 02f6b  cmp word [si+0x38], 0; 02f6f jg TroggDone
        // 02f7d  cmp word [0x700], 0; 02f82 jne TroggDone
        if self.fighters[target].health > 0 || self.bloodless {
            return;
        }
        let Some(script) = a_def
            .animation
            .contains_key("TroggSpear_Toss")
            .then(|| "TroggSpear_Toss".to_string())
        else {
            return;
        };
        // 02f86: the knight's own task goes, and he is drawn inside the toss.
        self.fighters[target].hidden = true;
        // 02f89  mov word [0x783a], TroggSpear_Toss
        self.fighters[attacker].ordered = Some(Order {
            state: State::Attack,
            script,
            attack: None,
        });
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
    pub fn monster_intent<'a, F>(
        &mut self,
        me: usize,
        target: usize,
        def_of: F,
        gore: bool,
    ) -> Intent
    where
        F: Fn(&str) -> &'a ActorDef,
    {
        use crate::monster::{decide, Act, Sight};
        // `DragonMoveClaw1` (0x3bb4): every path out of the head's controller
        // pins the two forelimbs to its depth, `mov ax, [si+6]; add ax, 0xa;
        // mov [bx+6], ax` for claw one and `sub ax, 0x1e; mov [bp+6], ax` for
        // claw two. Done on the claw's own pass, off the head's record as it
        // stands, which is the same two numbers on the same frame. The head
        // is the fixed record at DS:0x6e26, dead or alive: `ControlClaw+43`
        // (0x3b4e) reads its hit points off that address, and that is what
        // [`Sight::head_health`] carries.
        let head = self
            .fighters
            .iter()
            .position(|f| def_of(&f.actor).controller() == Controller::Dragon);
        if def_of(&self.fighters[me].actor).controller() == Controller::Claw {
            if let Some(h) = head {
                let (y, off) = (self.fighters[h].y, self.fighters[me].brain.timer);
                let (_, ny) = GLOBAL.clamp(self.fighters[me].x, y + off);
                self.fighters[me].y = ny;
            }
        }
        let head_health = head.map(|h| self.fighters[h].health);
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
        // `CalcDamage` (0x2d67) with the opponent in `si`, taken before the
        // fighters are borrowed: what his own blow takes off.
        let foe_blow = {
            let t_def = def_of(&self.fighters[target].actor);
            let kind = self.fighters[target].attack.unwrap_or(Attack::Swing);
            self.blow(target, t_def, kind)
        };
        let (act, mut intent) = {
            let def = def_of(&self.fighters[me].actor);
            let foe = &self.fighters[target];
            let t_def = def_of(&foe.actor);
            let sight = Sight {
                me: &self.fighters[me],
                foe,
                def,
                gore,
                body: foe.finishable(t_def),
                decapped,
                progression: self.progression,
                perch: self.perch,
                // `CalcDamage` with the opponent in `si`: what his own blow
                // takes off, which `RatHangKnight` is the one caller of.
                foe_blow,
                head_health,
            };
            let mut brain = self.fighters[me].brain;
            let mut seed = self.rng;
            // The record's `+8` goes in as it stands and comes back as the
            // controller left it: `FaceKnight` and its kin write it inside
            // the controller, and `TASKHANDLE` (0x9741) takes `dh`, which
            // `NOTEND+20` loaded from `[di+8]`, into the task on the way
            // out. A creature's facing is set here and nowhere else.
            let mut facing = self.fighters[me].facing;
            let mut shared = self.shared;
            // And `+2` and `+6` the same way, for `TrackKnight` (0x3c36,
            // 0x3c49, 0x3c51, 0x3c5a), which writes them where it stands.
            let mut at = (self.fighters[me].x, self.fighters[me].y);
            let act = decide(
                &sight,
                &mut brain,
                &mut seed,
                &mut facing,
                &mut shared,
                &mut at,
            );
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
            // **Nothing clamps what a controller wrote**, and this was the
            // other half of the borders that used to hold every fighter.
            // `MonsterWalk` (0x4e8b) adds its step at 0x4eeb and 0x4ef1 and
            // jumps straight to `NOTEND`; the controllers that write `+2`
            // themselves -- `TrackKnight` (0x3c36, 0x3c49), `BeastCharge`
            // (0x2ff3, 0x3004), `MudmenAppear` (0x5492) -- write their own
            // literals, and no common limit is applied to any of them on the
            // way out. `CheckBorder` (0x40d0) is the person's knight's alone
            // and `Fighter::walk` applies it there.
            //
            // `at` starts as the fighter's own position, so this clamp did not
            // bound what a controller wrote: it bounded every creature in the
            // arena, once a frame. Letting them out of the walk gate and then
            // pulling them back here is why they still stopped dead on the
            // knight's own wall after the gate was fixed.
            self.fighters[me].x = at.0;
            self.fighters[me].y = at.1;
            self.rng = seed;
            self.shared = shared;
            // `BalokJumping+12` (0x374b) calls `ShakeADD` where it stands, so
            // the controller's answer is taken here rather than waiting for a
            // script effect. Cleared as it is spent, since the call happens
            // once.
            if self.shared.shake {
                self.shared.shake = false;
                self.shake();
            }
            (act, Intent::default())
        };
        let order = |state, script: String, attack| {
            Some(Order {
                state,
                script,
                attack,
            })
        };
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
                match self.fighters[me].attack_script(def, kind) {
                    Some((script, k)) => {
                        let state = if k.is_guard() {
                            State::Guard
                        } else {
                            State::Attack
                        };
                        order(state, script, Some(k))
                    }
                    None => order(State::Attack, String::new(), Some(kind)),
                }
            }
            Act::Stand(script) => order(State::Idle, script, None),
            // `CLAWS_DEAD` (0x3b61): `[0x783a]` left at zero, which
            // `TASKHANDLE` takes as the task killed and the record freed.
            Act::Vanish => {
                self.fighters[me].vanish();
                None
            }
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
            // `RatmanLeaping` (0x3276) and `BalokJumping` (0x36df): the arc
            // writes `+2` and `+6` itself and names the frame to draw. The
            // height is `+4` and lives on the brain, which the controller has
            // already written.
            Act::Fly { x, y, script } => {
                let b = GLOBAL;
                let (nx, ny) = b.clamp(x, y);
                let f = &mut self.fighters[me];
                f.x = nx;
                f.y = ny;
                if script.is_empty() {
                    order(State::Idle, String::new(), None)
                } else {
                    order(State::Attack, script, None)
                }
            }
            // One frame of a hold, from the holder's side: see `Act::Grip`.
            Act::Grip {
                script,
                damage,
                cost,
                fatal,
                hold,
                victim,
            } => {
                let t_def = def_of(&self.fighters[target].actor);
                if hold {
                    self.fighters[target].holder = Some(me);
                    self.fighters[target].hidden = true;
                } else {
                    self.fighters[target].holder = None;
                    self.fighters[target].hidden = false;
                }
                // `sub word [di+0x38], n` on the one held: hit points off,
                // with no blow-taken script of his own, because he is drawn
                // inside the creature's animation while this lasts.
                if fatal {
                    let left = self.fighters[target].health.max(1);
                    self.fighters[target].struck(t_def, left, None);
                } else if damage > 0 {
                    let f = &mut self.fighters[target];
                    f.health -= damage;
                    if f.health <= 0 {
                        f.health = 0;
                        f.struck(t_def, 0, None);
                    }
                }
                // `REPLACEANIM` on the one held: `Knight_Explode` under
                // Balok's landing, `Knight_GetUp` off a dead ratman's grip.
                if !victim.is_empty() && t_def.animation.contains_key(&victim) {
                    let f = &mut self.fighters[target];
                    f.state = State::Idle;
                    f.enter_on(State::Dead, victim);
                } else if !hold && self.fighters[target].alive() {
                    self.fighters[target].enter(State::Idle);
                }
                // `sub word [di+0x38], ax` on the creature itself:
                // `RatHangKnight+32` puts the knight's own blow through it.
                if cost > 0 {
                    let def = def_of(&self.fighters[me].actor);
                    self.fighters[me].struck(def, cost, None);
                }
                if script.is_empty() {
                    order(State::Idle, String::new(), None)
                } else {
                    order(State::Attack, script, None)
                }
            }
            Act::Strike {
                script,
                damage,
                fatal,
                victim,
            } => {
                let t_def = def_of(&self.fighters[target].actor);
                self.fighters[target].holder = None;
                self.fighters[target].hidden = false;
                if fatal {
                    let left = self.fighters[target].health.max(1);
                    self.fighters[target].struck(t_def, left, None);
                } else if damage > 0 {
                    self.fighters[target].struck(t_def, damage, None);
                }
                // `mov si, 0x1596; call REPLACEANIM` on the victim's own
                // record, which is what the demon's two whip follows do
                // (0x50de, 0x5119): the script is named outright and his
                // `*Hit` row is not consulted. Taken after the blow, because
                // the blow is what puts him in `State::Hurt` for `hit_row`
                // to replace -- and a blow that killed him is left alone,
                // since a corpse that plays a thrown script is a corpse
                // that gets up.
                if let Some(v) = victim {
                    if self.fighters[target].alive() {
                        self.hit_row(target, &v, t_def);
                    }
                }
                order(State::Attack, script, None)
            }
        };
        // The standing order is the walk, never the press: a struggle is
        // read on the frame it is made and not held down for six ticks.
        self.fighters[me].brain.rest = def_of(&self.fighters[me].actor).script_ticks.max(1) as i32;
        self.fighters[me].drive = Intent {
            attack: false,
            ..intent
        };
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
        self.sounds.clear();
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
            // `TASKWALKCOLLIDE`'s own loop over the task table (0x9e1e): every
            // slot that is filled, is not this actor, has a body box
            // (`[di+0x22]` non-zero, which is to say it has been drawn at
            // least once) and has hit points left, so the dead are no longer
            // in the way.
            //
            // A fighter somebody has hold of is left out here and is not left
            // out there: `TASKSTANDBY` takes his *task* off the draw list and
            // his record keeps whatever box it last had, so in the original he
            // is still a body while a mudman or Balok holds him. He is drawn
            // inside his holder's animation, standing where his holder stands,
            // and leaving him in would be a second body on the same ground.
            let others: Vec<Occupant> = (0..self.fighters.len())
                .filter(|k| *k != i)
                .filter(|k| self.fighters[*k].alive() && !self.fighters[*k].hidden)
                .map(|k| {
                    let f = &self.fighters[k];
                    let (l, t, r, b) = f.body(def_of(&f.actor));
                    Occupant {
                        x: f.x,
                        depth: f.y,
                        box_left: l,
                        box_right: r,
                        box_top: t,
                        box_bottom: b,
                    }
                })
                .collect();
            let line = self.fighters[i].step_among(def, intent, &self.field, bloodless, &others);
            if !line.is_empty() {
                let f = &self.fighters[i];
                blows.push(Blow {
                    attacker: i,
                    missile: None,
                    line,
                    attack: f.attack,
                    depth: f.y,
                });
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
                        for t in 0..self.fighters.len() {
                            if t == i || !self.fighters[t].hidden {
                                continue;
                            }
                            // `KnightON`, 0x5289: the arrival column is held
                            // inside 1..0x13f and the depth is not touched at
                            // all, which is the only bound in the image on
                            // anything but the walking knight.
                            //
                            //   05289  cmp bx, 0x140; jl 05292; mov bx, 0x13f
                            //   05292  or  bx, bx;    jge 05299; mov bx, 1
                            let nx = (x + 0x89 * facing).clamp(1, 0x13f);
                            let f = &mut self.fighters[t];
                            f.x = nx;
                            f.y = y;
                            f.facing = -facing;
                            f.hidden = false;
                        }
                    }
                    // `InitSLAP` (0x44cb): `mov word [SLAPCNT], 0xffff`, the
                    // two instructions the whole routine is. The first
                    // `KnightSLAP` of the throw then steps the index to zero.
                    Effect::Gosub { routine, .. } if routine == "InitSLAP" => {
                        self.shared.slap_cnt = -1;
                    }
                    // `KnightSLAP` (0x44d2). Unlike `KnightOFF`, the fighter
                    // whose effects carry this gosub is the one it throws:
                    // `Knight_SwSlapped` is the knight's own script.
                    Effect::Gosub { routine, .. } if routine == "KnightSLAP" => {
                        self.knight_slap(i, def);
                    }
                    // `SetDecapFLAG` (0x3e76): `mov word ptr [DeCapFLAG], 1`.
                    // `ShakeADD` (0x493f), which `Troll_Chop` gosubs and
                    // `BalokJumping+12` (0x374b) calls when the Balok lands:
                    //
                    //   04946  cmp word [ShakeCOUNT], 0
                    //   0494b  jne  <out>          ; one shake at a time
                    //   0494d  mov word [ShakeCOUNT], 1
                    //
                    // so a second chop while one is pending does nothing.
                    Effect::Gosub { routine, .. } if routine == "ShakeADD" => {
                        self.shake();
                    }
                    Effect::Gosub { routine, .. } if routine == "SetDecapFLAG" => {
                        self.decap = true;
                    }
                    // `TrackKnight` (0x3be8), which `Dragon_HighBreath`
                    // (0x4448) and `Dragon_LowBreath` (0x4130) call on each
                    // of their five loops: the head keeps after him while the
                    // fire is out, on the ranges of two and one the breath
                    // bit puts in. See `crate::monster::track_knight`.
                    Effect::Gosub { routine, .. } if routine == "TrackKnight" => {
                        self.track_knight(i, &def_of);
                    }
                    // `DrDropHead` (0x3bd2): `mov ax, 0x6e26; call 0x96a2;
                    // add word ptr [di + 6], 0x26` on the head's own task,
                    // whose `+6` is the height. The dead head drops thirty
                    // eight rows down the screen, and stays there.
                    Effect::Gosub { routine, .. } if routine == "DrDropHead" => {
                        self.fighters[i].brain.height += 0x26;
                    }
                    // `DrDropClaws` (0x3be1): `mov word ptr [DEAD_CLAWS],
                    // 0xffff`, which `ControlClaw` reads on its next pass.
                    Effect::Gosub { routine, .. } if routine == "DrDropClaws" => {
                        self.shared.dead_claws = -1;
                    }
                    // `StopCombat` (0x231): the combat flag at DS:0x897d down
                    // and thirty five more frames on DS:0x8987. Every one of
                    // the knight's deaths calls it at its end, `Dragon_Dead`
                    // (0x4030) and `Dragon_BitKnight` (0x4880) call it, and
                    // `CountTheDead` falls into it. See [`Bout::settled`].
                    Effect::Gosub { routine, .. } if routine == "StopCombat" => {
                        self.stopped = true;
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
                    // `KillKnight` (0xab2): `mov si, [0x8979]; mov word
                    // [si+0x38], 0xffff`, the player's own record and no
                    // other, whoever called it. The dragon's chewing calls
                    // it, Balok's bite and squeeze do, and so does
                    // `Knight_BurnDeath` (0x198c) from the knight's own task.
                    // Whoever this fighter has hold of is that knight when
                    // there is one, since a held fighter is off the board;
                    // otherwise it is the first knight in the fight, which is
                    // seat zero. A creature it happens to be facing is never
                    // the one it means.
                    Effect::Gosub { routine, .. } if routine == "KillKnight" => {
                        let held = self.fighters.iter().position(|f| f.holder == Some(i));
                        let player = self
                            .fighters
                            .iter()
                            .position(|f| def_of(&f.actor).controller() == Controller::Knight);
                        if let Some(t) = held.or(player) {
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
                            let m = Missile {
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
                                until: String::new(),
                            };
                            self.launch(m, &def.animation);
                        }
                    }
                    // `TASKSOUND`: the script named a sample at this exact
                    // frame. Nothing decides anything about it here; the id
                    // goes out as the handler at 0x9b38 hands it to `PLAY_SFX`.
                    Effect::Sound { sample } => {
                        self.sounds.push(SoundCall { who: i, id: sample });
                    }
                    // And the 23 routines the scripts call that play one: each
                    // is transcribed in `crate::sound`, and the three that are
                    // silent in the shipped image are silent here.
                    Effect::Gosub {
                        routine,
                        kind: taskvm::GosubKind::Sound,
                    } => {
                        let x = self.fighters[i].task.as_ref().map_or(0, |t| t.x);
                        let mut ids = Vec::new();
                        sound::gosub(&routine, x, &mut self.rng, &mut ids);
                        for id in ids {
                            self.sounds.push(SoundCall { who: i, id });
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
                    Some(f) => (
                        f.alive(),
                        f.task.as_ref().map(|t| (t.x, t.y, t.z, t.facing)),
                    ),
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
                let frame = m.task.step(&def.animation, &mut m.record, bloodless);
                // A thrown dagger has a script of its own, and `SpeedKnife`
                // asks for the swish on its first frame.
                let owner = m.owner;
                for e in &frame.effects {
                    if let Effect::Sound { sample } = e {
                        self.sounds.push(SoundCall {
                            who: owner,
                            id: *sample,
                        });
                    }
                }
            }
            if !m.task.active {
                m.spent = true;
            }
            // A stand-in that has reached the end of its chain: the fighter it
            // stood in for comes back where it left him.
            if !m.until.is_empty() && (m.task.pc.script == m.until || !m.task.active) {
                m.spent = true;
                let (owner, x) = (m.owner, m.task.x);
                if let Some(f) = self.fighters.get_mut(owner) {
                    f.hidden = false;
                    f.holder = None;
                    // The column the chain left him at. The depth is his own:
                    // a task's `y` here is the anchor row, and turning it back
                    // into a feet row wants the actor's origin, which is the
                    // fighter's rather than the stand-in's.
                    let (nx, ny) = GLOBAL.clamp(x, f.y);
                    f.x = nx;
                    f.y = ny;
                    if f.alive() {
                        f.enter(State::Idle);
                    }
                }
                continue;
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
                    (match f.damage {
                        0 => self.damage,
                        d => d,
                    }) + f.bonus
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
                            self.parries.push(Parry {
                                attacker,
                                target,
                                with,
                            });
                            // `BKnightStruck` (0x4d78): a computer knight
                            // that stops a blow has its evade's one use given
                            // back on the spot. See
                            // [`crate::monster::black_knight_blocked`].
                            if crate::monster::black_knight_blocked(t_def.controller())
                                && self.fighters[target].brain.flags & crate::monster::flag::DRIVEN
                                    != 0
                            {
                                self.fighters[target].evaded = false;
                            }
                            self.fighters[attacker].recover(a_def);
                            self.hit_something(attacker, a_def);
                            break;
                        }
                    }
                    let evading = self.fighters[target].guarding() == Some(Attack::Evade);
                    // `StruckTable` (DS:0x7843) for the dragon's three kinds
                    // and the dragon's own `DragonStruck`: what each side of
                    // a blow in this fight costs, before it is dealt.
                    let damage = self.dragon_blow(target, damage, blow.attack, a_def, t_def);
                    self.fighters[target].struck(t_def, damage, blow.attack);
                    self.got_struck(target, t_def, blow.attack);
                    if blow.missile.is_none() {
                        self.troll_explodes(target, blow.attack, a_def, t_def);
                        self.trogg_spear_toss(attacker, target, a_def, t_def);
                    }
                    // `KnightGotStruck` (0x4267) dispatches on the striker's
                    // own kind, so it is the blow of a body and not of a
                    // thrown thing: a knife has a task of its own and never
                    // reaches this table.
                    if blow.missile.is_none() {
                        self.turn_struck(attacker, target, a_def);
                    }
                    // `DragonStruck1`, `DragonFire1` and `ClawStruck1`: the
                    // rows `InitKnightvsDragon` wrote over the knight's own
                    // `*Hit` table for this fight, and the row the bite drags
                    // him onto. And `DragonStruck`: what a blow on the head
                    // leaves behind.
                    self.dragon_struck_knight(
                        attacker,
                        target,
                        blow.missile.is_none(),
                        blow.attack,
                        a_def,
                        t_def,
                    );
                    // `InitKnightvsBalok+0x10` (0x258c): the uppercut's own row
                    // over the knight's `*Hit` table, which is the same
                    // `Knight_SwSlapped` and so the same throw.
                    self.balok_struck_knight(target, blow.attack, a_def, t_def);
                    // The same thing again for the three fights whose rows had
                    // been read off the image but not built: the beast's toss
                    // (`InitKnightvsBeast+21`, 0x2283 and 0x2288), the troll's
                    // club (`InitKnightvsTroll+16`, 0x26ba) and the demon's
                    // slap (`InitKnightvsDemon+40`, 0x2765).
                    self.beast_struck_knight(target, blow.attack, a_def, t_def);
                    self.troll_struck_knight(target, blow.attack, a_def, t_def);
                    self.demon_struck_knight(target, blow.attack, a_def, t_def);
                    self.dragon_struck(blow.missile.is_some(), blow.attack, a_def, t_def);
                    let fatal = !self.fighters[target].alive();
                    events.push(HitEvent {
                        attacker,
                        target,
                        damage,
                        fatal,
                    });
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
                    // exceptions the striker's own `+0xc` branch makes.
                    // `KnightHit1` (0x4123) keeps it going for an up thrust
                    // and for a blow on a knight who is evading;
                    // `BKnightHit` (0x4da8) is the computer knight's own
                    // branch and keeps it going for its chop and its lunge as
                    // well. `flag::DRIVEN` is the original's `+0x35`: kind 8
                    // runs `ControlBlackKnight`, kind 6 reads a joystick.
                    let driven = |f: &Fighter| f.brain.flags & crate::monster::flag::DRIVEN != 0;
                    // The three creatures whose `+0xc` branch names a script
                    // of its own rather than falling into a recovery:
                    // `RatmanHit` (0x34f0) with `RatLeapHit` and `RatTailHit`
                    // under it, `BalokHit` (0x377a) with `BalokGrabbed`, and
                    // `BeastStruck1` (0x4430). Each answers whether it took
                    // the frame over, which is what `carries_on` then reads.
                    let taken = if blow.missile.is_none() {
                        self.hit_something(attacker, a_def);
                        let rat = self.ratman_hit(attacker, target, a_def);
                        let balok = self.balok_hit(attacker, target, a_def);
                        let beast = self.beast_tosses(attacker, target, a_def, t_def);
                        let dragon = self.dragon_bites(attacker, target, blow.attack, a_def);
                        rat || balok || beast || dragon
                    } else {
                        false
                    };
                    let carries_on = if a_def.controller() == Controller::Knight
                        && driven(&self.fighters[attacker])
                    {
                        crate::monster::black_knight_carries_on(
                            blow.attack,
                            t_def.controller() == Controller::Knight
                                && !driven(&self.fighters[target]),
                            evading,
                        )
                    } else {
                        blow.attack == Some(Attack::UThrust) || evading || taken
                    };
                    if blow.missile.is_none() && !carries_on {
                        self.fighters[attacker].recover(a_def);
                    }
                    break;
                }
                // Down, but still a body while the kneel lasts.
                let Some(body) = self.fighters[target].corpse_body(t_def) else {
                    continue;
                };
                if !self.fighters[target].finishable(t_def) || !line_hits_body(&blow.line, body) {
                    continue;
                }
                let own_kind = self.fighters[attacker].actor == self.fighters[target].actor;
                // `KnightGotStruck` (0x4267) picks the handler by the
                // striker's `+0x35`, so the striker's record kind goes in.
                let striker = a_def.record_kind;
                if self.fighters[target].finish(t_def, blow.attack, own_kind, striker) {
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

        if self.settled() {
            self.settled_for += 1;
        }
        events
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
            mix(f.brain.height as i64);
            if let Some(j) = f.brain.jump {
                for v in [
                    j.steps, j.yvel, j.grav, j.xvel, j.zvel, j.xpos, j.zpos, j.ypos,
                ] {
                    mix(v as i64);
                }
            } else {
                mix(-1);
            }
            mix(f.holder.map_or(-1, |h| h as i64));
            mix(f.hidden as i64);
            mix(f.talismans as i64);
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
        mix(self.stopped as i64);
        mix(self.shared.dragon as i64);
        mix(self.shared.ddis as i64);
        for v in self.shared.dragon_bodge {
            mix(v as i64);
        }
        mix(self.shared.dead_claws as i64);
        if let Some((a, b)) = self.shared.dragon_ranges {
            mix(a as i64);
            mix(b as i64);
        }
        h
    }
}

/// Where a blow landed, for the blood: `CXx` and `CY`.
///
/// **Recovered.** `COLCHK` walks the weapon's polyline point by point, and the
/// point that lands is the answer, not the middle of anything:
///
/// ```text
/// 0a128  sub cx, cx
/// 0a12a  mov cl, [bx-2]            ; the point's own x, as CHECKL left it
/// 0a12d  add cx, [di+6]            ; plus the weapon record's x
/// 0a130  mov [CXx], cx
/// 0a137  sub dx, dx
/// 0a139  mov dl, [bx-1]            ; and its y
/// 0a13c  add dx, [di+8]
/// 0a13f  mov [CY], dx
/// ```
///
/// and `TaskCol_MainLoop` then writes the pair into the struck actor's `+0x58`
/// and `+0x5a` (0x9f87, 0x9f8d). The walk stops at the first point inside the
/// body cel whose mask bit is set, so this takes the first point of the sweep
/// that is inside the body.
///
/// Where no point of the sweep is inside, which this engine can reach because
/// a bank with no line in `COLLIDE.HIT` still sweeps its cel corners, the
/// first segment that meets the body is clamped into it instead. The original
/// has no such case: every point it tests is a sample of the blade.
fn strike_point(line: &[(i32, i32)], body: (i32, i32, i32, i32)) -> (i32, i32) {
    let (l, t, r, b) = body;
    let inside = |(x, y): &(i32, i32)| *x >= l && *x <= r && *y >= t && *y <= b;
    // 0a12a..0a13f: the point that landed.
    if let Some(p) = line.iter().find(|p| inside(p)) {
        return *p;
    }
    for w in line.windows(2) {
        if crate::combat::line_hits_body(w, body) {
            return (w[0].0.clamp(l, r), w[0].1.clamp(t, b));
        }
    }
    ((l + r) / 2, (t + b) / 2)
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
    fn arena_field() -> Field {
        Field::new(vec![Border {
            left: 0,
            right: 319,
            bottom: 60,
            top: 10,
        }])
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

    /// `TrollStruck1` into `TrollOHead` (0x438a, 0x4397): the troll's overhead
    /// chop, and only that, bursts a knight it kills. Nothing built this
    /// before; `Knight_Explode` was named in the scripts and never played.
    #[test]
    fn a_trolls_chop_that_kills_bursts_the_knight() {
        let mut troll = def();
        troll.controller = "troll".into();
        let mut knight = def();
        knight
            .finishes
            .insert("explode".into(), "Knight_Explode".into());
        let mut b = Bout::new(
            arena_field(),
            vec![
                Fighter::new("t", &troll, 100, 100, 1),
                Fighter::new("k", &knight, 140, 100, -1),
            ],
        );
        // 0439b: still standing, so `KnightSAnim` and no burst.
        b.fighters[1].health = 5;
        b.troll_explodes(1, Some(Attack::Chop), &troll, &knight);
        assert_ne!(b.fighters[1].script, "Knight_Explode");
        // 0438e: the swing is not the chop, so no burst either.
        b.fighters[1].health = 0;
        b.troll_explodes(1, Some(Attack::Swing), &troll, &knight);
        assert_ne!(b.fighters[1].script, "Knight_Explode");
        // 043a4: the chop, and nothing left.
        b.troll_explodes(1, Some(Attack::Chop), &troll, &knight);
        assert_eq!(b.fighters[1].script, "Knight_Explode");
        // And no other creature does it: the table entry is the troll's.
        let mut other = def();
        other.controller = "trogg".into();
        b.fighters[1].script.clear();
        b.troll_explodes(1, Some(Attack::Chop), &other, &knight);
        assert_eq!(b.fighters[1].script, "");
    }

    /// `TroggHit+12` (0x2f59): the spear trogg picks a dead knight up on the
    /// point. The axe and the hammer do not, and neither does anyone with the
    /// gore switched off.
    #[test]
    fn the_spear_trogg_tosses_a_corpse_and_only_with_the_gore_on() {
        let mut spear = def();
        spear.controller = "trogg_spear".into();
        spear
            .animation
            .insert("TroggSpear_Toss".into(), Default::default());
        let knight = def();
        let mut b = Bout::new(
            arena_field(),
            vec![
                Fighter::new("s", &spear, 100, 100, 1),
                Fighter::new("k", &knight, 140, 100, -1),
            ],
        );
        // 02f6b: still standing.
        b.fighters[1].health = 5;
        b.trogg_spear_toss(0, 1, &spear, &knight);
        assert_eq!(b.fighters[0].ordered, None);
        // 02f7d: gore off.
        b.fighters[1].health = 0;
        b.bloodless = true;
        b.trogg_spear_toss(0, 1, &spear, &knight);
        assert_eq!(b.fighters[0].ordered, None);
        // 02f89, with the gore on.
        b.bloodless = false;
        b.trogg_spear_toss(0, 1, &spear, &knight);
        assert_eq!(
            b.fighters[0].ordered.as_ref().map(|o| o.script.as_str()),
            Some("TroggSpear_Toss")
        );
        assert!(b.fighters[1].hidden, "the corpse is drawn inside the toss");
        // 02f77: the axe trogg runs the same routine and takes `TroggDone`.
        let mut axe = def();
        axe.controller = "trogg".into();
        axe.animation
            .insert("TroggSpear_Toss".into(), Default::default());
        b.fighters[0].ordered = None;
        b.trogg_spear_toss(0, 1, &axe, &knight);
        assert_eq!(b.fighters[0].ordered, None);
    }

    /// `RatLeapHit` (0x353f) and `RatTailHit` (0x357d): the two things a rat's
    /// blow does that no other creature's does, and the two branches
    /// `RatmanHit` (0x34fc, 0x3502) takes before the ordinary claw.
    #[test]
    fn a_ratmans_blow_from_the_air_takes_his_head_and_one_from_a_tree_snags_him() {
        use crate::monster::{flag, rat_flag};
        let mut rat = def();
        rat.controller = "ratman".into();
        for row in ["sit", "snag"] {
            let name = if row == "sit" {
                "Ratman_SitOnHead"
            } else {
                "Ratman_SnagKnight"
            };
            rat.scripts.insert(row.into(), vec![name.into()]);
            rat.animation.insert(name.into(), Default::default());
        }
        let knight = def();
        let field = arena_field();
        let bout = || {
            Bout::new(
                field.clone(),
                vec![
                    Fighter::new("r", &rat, 100, 100, 1),
                    Fighter::new("k", &knight, 140, 100, -1),
                ],
            )
        };
        // 034fc: the leap. The knight goes off the board and the rat sits on
        // him for his endurance plus six.
        let mut b = bout();
        b.fighters[1].record.set(crate::monster::ENDURANCE, 3);
        b.fighters[0].brain.flags |= flag::LEAPING;
        b.fighters[0].brain.height = -30;
        assert!(b.ratman_hit(0, 1, &rat), "the frame is the rat's own");
        assert_eq!(b.shared.rat & rat_flag::ON_HEAD, rat_flag::ON_HEAD);
        assert!(b.fighters[0].brain.flags & flag::ON_HEAD != 0);
        assert_eq!(b.fighters[0].brain.cooldown, 9, "03561: endurance plus six");
        assert_eq!(b.fighters[0].brain.height, 0, "03570: mov [di+4], 0");
        assert!(b.fighters[1].hidden && b.fighters[1].holder == Some(0));
        assert_eq!(
            b.fighters[0].ordered.as_ref().map(|o| o.script.as_str()),
            Some("Ratman_SitOnHead")
        );
        // 03548: and a second rat finds the head taken and falls past.
        let mut second = bout();
        second.shared.rat |= rat_flag::ON_HEAD;
        second.fighters[0].brain.flags |= flag::LEAPING;
        assert!(second.ratman_hit(0, 1, &rat));
        assert!(!second.fighters[1].hidden, "one rat at a time");
        // 03502: the tail from the tree, which leaves it in the tree.
        let mut b = bout();
        b.fighters[0].brain.flags |= flag::IN_TREE;
        b.fighters[0].brain.height = -88;
        assert!(b.ratman_hit(0, 1, &rat));
        assert_eq!(b.shared.rat & rat_flag::HANGING, rat_flag::HANGING);
        assert!(b.fighters[0].brain.flags & flag::HANGING != 0);
        assert_eq!(b.fighters[0].brain.height, -88, "it is still up the tree");
        assert!(b.fighters[1].hidden && b.fighters[1].holder == Some(0));
        // 03508: an ordinary claw, which buys every rat fifteen frames.
        let mut b = bout();
        assert!(b.ratman_hit(0, 1, &rat));
        assert_eq!(b.shared.hit_delay, 0xf, "03513: mov [HitDelay], 0xf");
        assert!(!b.fighters[1].hidden);
        // 034f3: and a rat's claw does nothing at all to another rat.
        let mut b = Bout::new(
            field,
            vec![
                Fighter::new("r", &rat, 100, 100, 1),
                Fighter::new("r", &rat, 140, 100, -1),
            ],
        );
        assert!(!b.ratman_hit(0, 1, &rat));
        assert_eq!(b.shared.hit_delay, 0);
    }

    /// `BeastStruck1` (0x4430): the toss when the knight lives, the impale
    /// when he does not and the gore switch is on, and which of each pair by
    /// which way the two are facing.
    #[test]
    fn the_beast_tosses_a_knight_who_lives_and_impales_one_who_does_not() {
        let mut beast = def();
        beast.controller = "beast".into();
        for name in [
            "Beast_BackToss",
            "Beast_ChestToss",
            "Beast_ImpaleBack",
            "Beast_ImpaleChest",
            "Knight_SwStance",
        ] {
            beast.animation.insert(name.into(), Default::default());
        }
        let knight = def();
        let field = arena_field();
        let bout = || {
            Bout::new(
                field.clone(),
                vec![
                    Fighter::new("b", &beast, 100, 100, 1),
                    Fighter::new("k", &knight, 140, 100, -1),
                ],
            )
        };
        // 0447e: facing each other is the chest, and he is thrown as a task of
        // the beast's own so that the beast's bank tables can draw him.
        let mut b = bout();
        b.fighters[1].health = 5;
        assert!(b.beast_tosses(0, 1, &beast, &knight));
        assert_eq!(b.missiles.len(), 1);
        assert_eq!(b.missiles[0].task.pc.script, "Beast_ChestToss");
        assert_eq!(b.missiles[0].until, "Knight_SwStance");
        assert!(b.fighters[1].hidden, "he is drawn inside the toss");
        // 0448f: facing the same way is the back.
        let mut b = bout();
        b.fighters[1].health = 5;
        b.fighters[1].facing = 1;
        assert!(b.beast_tosses(0, 1, &beast, &knight));
        assert_eq!(b.missiles[0].task.pc.script, "Beast_BackToss");
        // 04434 and 0443a: nothing left and the gore on is the impale, which
        // the beast plays itself.
        let mut b = bout();
        b.fighters[1].health = 0;
        assert!(b.beast_tosses(0, 1, &beast, &knight));
        assert!(b.missiles.is_empty());
        assert_eq!(
            b.fighters[0].ordered.as_ref().map(|o| o.script.as_str()),
            Some("Beast_ImpaleChest")
        );
        assert!(b.fighters[1].hidden && b.fighters[1].holder == Some(0));
        // 0443a: with the gore off it is a toss like any other.
        let mut b = bout();
        b.fighters[1].health = 0;
        b.bloodless = true;
        assert!(!b.beast_tosses(0, 1, &beast, &knight), "and he stays down");
        assert!(b.missiles.is_empty());
        assert_eq!(b.fighters[0].ordered, None);
    }

    /// The dragon's set piece in miniature: a scripted dragon whose `*Hit`
    /// row dies into a death that calls what `Dragon_Dead` (0x3e40) calls,
    /// two claws on `ControlClaw`, and a knight with the rows
    /// `InitKnightvsDragon` writes over his table.
    fn dragon_set_piece() -> (ActorDef, ActorDef, ActorDef) {
        use crate::combat::tests::depth_def;
        use crate::content::AttackDef;
        use crate::taskvm::{End, Instr, Part, Script};
        let stop = Instr::EndFrame { end: End::Stop };
        let next = Instr::EndFrame { end: End::Next };
        let gosub = |r: &str| Instr::Gosub { routine: r.into() };
        let head = |cel: u8, flags: u8| {
            Instr::Part(Part {
                table: 1,
                bank: 0,
                cel,
                x: 0,
                y: 0,
                flags,
            })
        };
        let mut dragon = depth_def();
        dragon.controller = "dragon".into();
        dragon.health = 200;
        dragon.damage = 20;
        dragon.approach = 60;
        dragon.back_off = 20;
        dragon.depth_tolerance = 5;
        dragon.bleeds = false;
        dragon.attacks.clear();
        for (kind, script, damage) in [
            ("lunge", "Dragon_HighBite", 20),
            ("swing", "Dragon_LowBreath", 30),
            ("chop", "Dragon_HighBreath", 30),
        ] {
            dragon.attacks.insert(
                kind.into(),
                AttackDef {
                    script: script.into(),
                    damage,
                },
            );
        }
        dragon.attack = "lunge".into();
        for name in ["Dragon_HighBite", "Dragon_LowBreath", "Dragon_HighBreath"] {
            // One frame with a weapon part at the head and the bite's tooth.
            dragon.animation.insert(
                name.into(),
                Script::new(vec![
                    head(
                        8,
                        crate::taskvm::part_flags::BODY | crate::taskvm::part_flags::WEAPON,
                    ),
                    next.clone(),
                    head(0, crate::taskvm::part_flags::BODY),
                    stop.clone(),
                ]),
            );
        }
        // `Dragon_Fire` (0x48ca): a weapon part and a `TASKKILLTASK`.
        dragon.animation.insert(
            "Dragon_Fire".into(),
            Script::new(vec![
                gosub("DrFireSnd"),
                head(8, crate::taskvm::part_flags::WEAPON),
                next.clone(),
                Instr::KillTask,
                stop.clone(),
            ]),
        );
        // `Dragon_BitKnight` (0x46f8): the chewing, `KillKnight` and
        // `StopCombat`.
        dragon.animation.insert(
            "Dragon_BitKnight".into(),
            Script::new(vec![
                gosub("DragonKnightSound"),
                head(0, 0),
                next.clone(),
                gosub("KillKnight"),
                head(0, 0),
                next.clone(),
                gosub("StopCombat"),
                head(0, 0),
                stop.clone(),
            ]),
        );
        // `Dragon_Dead` (0x3e40): `DrDropHead`, then `StopCombat`, then
        // `DrDropClaws`, then the task killed.
        dragon.animation.insert(
            "Dragon_Dead".into(),
            Script::new(vec![
                gosub("DrDropHead"),
                head(0, 0),
                next.clone(),
                gosub("StopCombat"),
                head(0, 0),
                next.clone(),
                gosub("DrDropClaws"),
                head(0, 0),
                next.clone(),
                Instr::KillTask,
                stop.clone(),
            ]),
        );
        // `Dragon_Hit` (0x4542): `TASKDEAD Dragon_Dead`.
        dragon.animation.insert(
            "Dragon_Hit".into(),
            Script::new(vec![
                Instr::Dead {
                    target: "Dragon_Dead".into(),
                },
                head(0, 0),
                stop.clone(),
            ]),
        );
        dragon.hurt_by.clear();
        dragon
            .scripts
            .insert("hurt".into(), vec!["Dragon_Hit".into()]);
        dragon
            .scripts
            .insert("death".into(), vec!["Dragon_Dead".into()]);
        dragon
            .scripts
            .insert("recover".into(), vec!["stance".into()]);
        dragon.scripts.insert("lift".into(), vec!["stance".into()]);
        dragon.scripts.insert("lower".into(), vec!["stance".into()]);
        for name in ["Dragon_Stance", "Dragon_HighStance"] {
            dragon.animation.insert(
                name.into(),
                Script::new(vec![head(0, crate::taskvm::part_flags::BODY), stop.clone()]),
            );
        }
        let mut claw = dragon.clone();
        claw.controller = "claw".into();
        claw.depth_tolerance = 10;
        claw.attacks.clear();
        claw.attacks.insert(
            "rthrust".into(),
            AttackDef {
                script: "Dragon_ClawSlap".into(),
                damage: 10,
            },
        );
        claw.attack = "rthrust".into();
        claw.damage = 10;
        for name in ["Dragon_Claw", "Dragon_ClawSlap", "Dragon_ClawDead"] {
            claw.animation.insert(
                name.into(),
                Script::new(vec![head(0, crate::taskvm::part_flags::BODY), stop.clone()]),
            );
        }
        let mut knight = depth_def();
        for name in ["Knight_SwShoulderHit", "Knight_Burn", "Knight_SwSlapped"] {
            knight.animation.insert(
                name.into(),
                Script::new(vec![Instr::Hold { count: 4 }, head(9, 0), stop.clone()]),
            );
        }
        (dragon, claw, knight)
    }

    /// The set piece stood up as `InitKnightvsDragon` stands it: the head at
    /// x 80 and the two claws at x 5, ten rows either side, with a knight in
    /// front of it.
    fn dragon_bout(knight_x: i32) -> Bout {
        let (dragon, claw, knight) = dragon_set_piece();
        let mut k = Fighter::new("k", &knight, knight_x, 100, -1);
        k.health = 20;
        k.max_health = 20;
        let mut d = Fighter::new("d", &dragon, 80, 100, 1);
        d.brain.height = -40;
        let mut c1 = Fighter::new("c", &claw, 5, 110, 1);
        c1.brain.timer = 10;
        let mut c2 = Fighter::new("c", &claw, 5, 80, 1);
        c2.brain.timer = -20;
        Bout::new(arena_field(), vec![k, d, c1, c2])
    }

    fn dragon_defs() -> std::collections::BTreeMap<String, ActorDef> {
        let (dragon, claw, knight) = dragon_set_piece();
        [("k", knight), ("d", dragon), ("c", claw)]
            .into_iter()
            .map(|(n, d)| (n.to_string(), d))
            .collect()
    }

    /// `DragonStruck1` (0x43ad), `ClawStruck1` (0x43d3) and `DragonFire1`
    /// (0x43c2): what the dragon's three kinds do to a knight, through
    /// `TalismanWrym`, onto the rows `InitKnightvsDragon` wrote, and where
    /// the head's own blows leave him.
    #[test]
    fn the_dragons_blows_burn_bite_and_slap_through_the_talisman() {
        let defs = dragon_defs();
        let (dragon, claw, knight) = (&defs["d"], &defs["c"], &defs["k"]);
        // 043bd: the bite is twenty, and 043ad: he is on the head's row less one.
        let mut b = dragon_bout(150);
        b.fighters[0].y = 108;
        b.fighters[1].y = 104;
        assert_eq!(
            b.dragon_blow(0, 20, Some(Attack::Lunge), dragon, knight),
            20
        );
        b.fighters[0].struck(knight, 20, Some(Attack::Lunge));
        b.dragon_struck_knight(1, 0, true, Some(Attack::Lunge), dragon, knight);
        assert_eq!(b.fighters[0].y, 103, "043b3: the head's row, less one");
        assert_eq!(
            b.fighters[0].script, "Knight_SwShoulderHit",
            "02448: kind 2"
        );
        // 043c2: the fire task is thirty, and it does not move him.
        let mut b = dragon_bout(150);
        b.fighters[0].health = 20;
        assert_eq!(b.dragon_blow(0, 30, Some(Attack::Chop), dragon, knight), 30);
        b.fighters[0].struck(knight, 30, Some(Attack::Chop));
        b.dragon_struck_knight(1, 0, false, Some(Attack::Chop), dragon, knight);
        assert_eq!(
            b.fighters[0].y, 100,
            "DragonFire1 enters past the row write"
        );
        assert_eq!(b.fighters[0].script, "Knight_Burn", "02452: kind 0x10");
        // 0244d: the low breath is the burn as well, off the head's own parts.
        let mut b = dragon_bout(150);
        b.fighters[0].struck(knight, 30, Some(Attack::Swing));
        b.dragon_struck_knight(1, 0, true, Some(Attack::Swing), dragon, knight);
        assert_eq!(b.fighters[0].script, "Knight_Burn", "0244d: kind 4");
        assert_eq!(b.fighters[0].y, 99, "and the head's row less one");
        // 043d3: a claw is ten, and 02457 the slap.
        let mut b = dragon_bout(60);
        assert_eq!(
            b.dragon_blow(0, 10, Some(Attack::RThrust), claw, knight),
            10
        );
        b.fighters[0].struck(knight, 10, Some(Attack::RThrust));
        b.dragon_struck_knight(2, 0, true, Some(Attack::RThrust), claw, knight);
        assert_eq!(b.fighters[0].script, "Knight_SwSlapped", "02457: kind 0xa");
        assert_eq!(b.fighters[0].y, 100, "a claw's blow leaves his row alone");
        // 043ca / 043d6: TalismanWrym on every one of them.
        let mut b = dragon_bout(150);
        b.fighters[0].talismans = 1;
        assert_eq!(b.dragon_blow(0, 30, Some(Attack::Chop), dragon, knight), 15);
        assert_eq!(
            b.dragon_blow(0, 20, Some(Attack::Lunge), dragon, knight),
            10
        );
        assert_eq!(b.dragon_blow(0, 10, Some(Attack::RThrust), claw, knight), 5);
        b.fighters[0].talismans = 3;
        assert_eq!(
            b.dragon_blow(0, 30, Some(Attack::Chop), dragon, knight),
            5,
            "04403: the floor"
        );
        // 03acf: a knife takes three off the dragon, whatever it is worth.
        assert_eq!(b.dragon_blow(1, 9, Some(Attack::Knife), knight, dragon), 3);
        assert_eq!(
            b.dragon_blow(1, 9, Some(Attack::Swing), knight, dragon),
            9,
            "03aa2: CalcDamage"
        );
        // 03a9c / 03ac0: a knight's blow or a knife raises bit 7; a claw's
        // touch or the fire's does not.
        b.dragon_struck(false, Some(Attack::Swing), knight, dragon);
        assert_eq!(b.shared.dragon & crate::monster::dragon_flag::STRUCK, 0x80);
        b.shared.dragon = 0;
        b.dragon_struck(true, Some(Attack::Knife), knight, dragon);
        assert_eq!(b.shared.dragon & crate::monster::dragon_flag::STRUCK, 0x80);
        b.shared.dragon = 0;
        b.dragon_struck(false, Some(Attack::RThrust), claw, dragon);
        b.dragon_struck(true, Some(Attack::Chop), dragon, dragon);
        assert_eq!(
            b.shared.dragon, 0,
            "03a82: anything else is not a blow it marks"
        );
    }

    /// `InitSLAP` (0x44cb) and `KnightSLAP` (0x44d2): the knockback, entry by
    /// entry off `BalokSLAP` (DS:0x7818), with the count starting at -1 so the
    /// first step reads entry zero.
    #[test]
    fn the_slap_flings_the_knight_along_the_table() {
        use crate::monster::BALOK_SLAP;
        let defs = dragon_defs();
        let knight = &defs["k"];
        let mut b = dragon_bout(100);
        // 044cb  mov word [SLAPCNT], 0xffff, which is what the `InitSLAP`
        // gosub arm above does.
        b.shared.slap_cnt = -1;
        // 03b9f / 043dc: a claw's slap, rightward.
        b.shared.slap = 1;
        b.fighters[0].facing = -1;
        let mut x = 100;
        for (i, entry) in BALOK_SLAP.iter().enumerate() {
            b.knight_slap(0, knight);
            assert_eq!(b.shared.slap_cnt, i as i32, "044d2: add word [SLAPCNT], 1");
            x += entry;
            assert_eq!(
                b.fighters[0].x, x,
                "04547: entry {i} of the table is {entry}"
            );
            assert_eq!(b.fighters[0].facing, 1, "044e5: the facing is SLAP's");
        }
        // 30, 25, 20, 20, 20, then the rebound and the rest: fifty five of the
        // hundred and four is in the first two frames, which is all the script
        // ever asks for.
        assert_eq!(x, 204);
        // Past the table's eleventh word the original reads `SLAPY` itself;
        // nothing in the image can reach it and nothing is built for it, so
        // the step is skipped. See `monster::slap_entry`.
        assert_eq!(
            crate::monster::slap_entry(crate::monster::slap_y::BALOK_SLAP, 11),
            None
        );
        b.knight_slap(0, knight);
        assert_eq!(b.fighters[0].x, x, "off the end of the table: no step");
        assert_eq!(b.shared.slap_cnt, 11, "and the count still went up");
    }

    /// **The clamp at the end of the throw is on one side only, and that is
    /// the shipped game.** `KnightSLAP`'s leftward tail floors the column at
    /// ten:
    ///
    /// ```text
    /// 04516  83 7c 04 0a        cmp word [si+4], 0xa
    /// 0451a  7d 05              jge 04521            ; over five bytes
    /// 0451c  c7 44 04 0a 00     mov word [si+4], 0xa
    /// 04521  c3                 ret
    /// ```
    ///
    /// `KnightSLAPR`'s does not. The compare is there and its body is not:
    ///
    /// ```text
    /// 0454a  81 7c 04 40 01     cmp word [si+4], 0x140
    /// 0454f  7e 00              jle 04551            ; a displacement of zero
    /// 04551  c3                 ret
    /// ```
    ///
    /// `7e 00` jumps to the very next instruction, so the taken and the
    /// untaken branch land on the same `ret` and there is no clamp to skip
    /// over. A knight batted rightward has no upper bound of his own. Built
    /// that way on purpose; do not make it symmetric.
    #[test]
    fn the_rightward_slap_has_no_clamp_of_its_own() {
        let defs = dragon_defs();
        let knight = &defs["k"];
        // Rightward from the last column `CheckBorder` still allows — 295,
        // whose probe is exactly 320 — and thirty on top of it.
        let mut b = dragon_bout(295);
        b.shared.slap_cnt = -1;
        b.shared.slap = 1;
        b.knight_slap(0, knight);
        assert_eq!(
            b.fighters[0].x, 325,
            "0454f: 7e 00, a jle with a zero displacement, so nothing clamps"
        );
        assert!(b.fighters[0].x > 0x140, "past the column the compare names");
        // Leftward from the first column it still allows — 35, whose probe is
        // exactly 10 — and the same thirty, which this side does clamp.
        let mut b = dragon_bout(35);
        b.shared.slap_cnt = -1;
        b.shared.slap = 3;
        b.knight_slap(0, knight);
        assert_eq!(b.fighters[0].x, 0xa, "0451c: mov word [si+4], 0xa");
    }

    /// `CheckBorder` inside the throw (0x44f3 and 0x4526) and the test of the
    /// bit it may have taken away (0x44fa, 0x452e): the step is skipped whole,
    /// though the count and the facing are already spent.
    #[test]
    fn the_border_refuses_a_slap_and_nothing_moves() {
        let defs = dragon_defs();
        let knight = &defs["k"];
        // Leftward at column 30: the probe is 5, under `X_LOW`, so 040e9
        // clears bit 1 and 044fa finds it gone.
        let mut b = dragon_bout(30);
        b.shared.slap_cnt = -1;
        b.shared.slap = 3;
        b.knight_slap(0, knight);
        assert_eq!(b.fighters[0].x, 30, "044fa: the border refused it");
        assert_eq!(b.fighters[0].facing, -1, "044e5 ran before the border did");
        assert_eq!(b.shared.slap_cnt, 0, "so did 044d2");
        // Rightward at column 300: the probe is 325, over `X_HIGH`, so 040f7
        // clears bit 0.
        let mut b = dragon_bout(300);
        b.shared.slap_cnt = -1;
        b.shared.slap = 1;
        b.knight_slap(0, knight);
        assert_eq!(b.fighters[0].x, 300, "0452e: the border refused it");
        assert_eq!(b.fighters[0].facing, 1);
    }

    /// Who writes `SLAP`: `ClawStruck1+9` (0x43dc) and `ClawHit+9` (0x3b9f)
    /// always 1, and `ControlBalok`'s uppercut branch (0x3639) the Balok's own
    /// `+8`. So a claw bats him rightward from either side of him, and the
    /// Balok throws him the way it is itself facing.
    #[test]
    fn the_claw_slaps_rightward_and_the_balok_by_its_own_facing() {
        let defs = dragon_defs();
        let (claw, knight) = (&defs["c"], &defs["k"]);
        // The claws stand at x 5; the knight to the right of them and then to
        // the left, and the direction is 1 both times.
        for knight_x in [60, 2] {
            let mut b = dragon_bout(knight_x);
            b.shared.slap = 0;
            b.fighters[0].struck(knight, 10, Some(Attack::RThrust));
            b.dragon_struck_knight(2, 0, true, Some(Attack::RThrust), claw, knight);
            assert_eq!(b.fighters[0].script, "Knight_SwSlapped", "02457");
            assert_eq!(b.shared.slap, 1, "043dc: mov byte [SLAP], 1");
        }
        // And the Balok: the uppercut branch, reached at a distance inside
        // 0x50 and outside 0x46, with the knight on each side in turn.
        let mut balok = def();
        balok.controller = "balok".into();
        let plain = def();
        for (foe_x, want) in [(175, 1), (25, 3)] {
            let b = Bout::new(
                arena_field(),
                vec![
                    Fighter::new("b", &balok, 100, 100, 1),
                    Fighter::new("k", &plain, foe_x, 100, -1),
                ],
            );
            let mut brain = b.fighters[0].brain;
            let mut shared = b.shared;
            let mut facing = 1;
            let mut seed = 1u16;
            let sight = crate::monster::Sight {
                me: &b.fighters[0],
                foe: &b.fighters[1],
                def: &balok,
                gore: true,
                body: false,
                decapped: false,
                progression: 0,
                perch: None,
                foe_blow: 0,
                head_health: None,
            };
            let act = crate::monster::decide(
                &sight,
                &mut brain,
                &mut seed,
                &mut facing,
                &mut shared,
                &mut (0, 0),
            );
            assert_eq!(
                act,
                crate::monster::Act::Attack {
                    kind: Attack::Swing,
                    spawn: None
                },
                "0362e: the uppercut"
            );
            assert_eq!(shared.slap, want, "03639: mov al, [si+8]; mov [SLAP], al");
        }
        // `InitKnightvsBalok+0x10` (0x258c): kind 4 is the same slapped script
        // the claw's kind 0xa is, and the grab's kind 0x10 is not.
        let mut b = dragon_bout(60);
        let mut balok_def = defs["d"].clone();
        balok_def.controller = "balok".into();
        b.fighters[0].struck(knight, 5, Some(Attack::Swing));
        b.balok_struck_knight(0, Some(Attack::Swing), &balok_def, knight);
        assert_eq!(b.fighters[0].script, "Knight_SwSlapped", "0258c: kind 4");
        let mut b = dragon_bout(60);
        b.fighters[0].struck(knight, 5, Some(Attack::Chop));
        b.balok_struck_knight(0, Some(Attack::Chop), &balok_def, knight);
        assert_ne!(
            b.fighters[0].script, "Knight_SwSlapped",
            "the grab has no row of its own"
        );
    }

    /// **A controller's write-back is not clamped, and creatures do not stop
    /// on the knight's wall.**
    ///
    /// `CheckBorder` (0x40d0) has three callers and all three are the
    /// knight's: `ControlKnight+0xef` and the two halves of `KnightSLAP`.
    /// `MonsterWalk` (0x4e8b), which is a creature's whole gate, calls
    /// `TASKWALKCOLLIDE` at 0x4e9e and then adds its step at 0x4eeb and
    /// 0x4ef1 with nothing between. `TroggTABLE` seats one creature at -50
    /// and another at 360, which a border at 320 could never let walk in.
    ///
    /// `Fighter::walk`'s gate is only half of it. This pass used to end
    /// `x = GLOBAL.clamp(...)`, and `at` starts as the fighter's own
    /// position, so every creature took the knight's wall one controller
    /// pass after the gate had correctly let it out.
    #[test]
    fn a_creature_is_not_pulled_back_to_the_knights_wall() {
        let mut trogg = def();
        trogg.controller = "trogg".into();
        trogg.approach = 100;
        trogg.back_off = 90;
        let knight = def();
        let defs: BTreeMap<String, ActorDef> = [
            ("t".to_string(), trogg.clone()),
            ("k".to_string(), knight.clone()),
        ]
        .into_iter()
        .collect();
        // Out where `TroggTABLE`'s second seat stands one, and further.
        let mut b = Bout::new(
            arena_field(),
            vec![
                Fighter::new("t", &trogg, 380, 100, -1),
                Fighter::new("k", &knight, 200, 100, 1),
            ],
        );
        b.monster_intent(0, 1, |a| &defs[a], true);
        assert_eq!(
            b.fighters[0].x, 380,
            "the seat's own column, not `CheckBorder`'s 320"
        );
        // And the same on the other side, where the first seat is.
        let mut b = Bout::new(
            arena_field(),
            vec![
                Fighter::new("t", &trogg, -50, 100, 1),
                Fighter::new("k", &knight, 200, 100, -1),
            ],
        );
        b.monster_intent(0, 1, |a| &defs[a], true);
        assert_eq!(b.fighters[0].x, -50, "and not `CheckBorder`'s 10");
    }

    /// `BeastCharge` at its edge (0x2fec, 0x2ff3): the turn writes the
    /// record's own column out to `0x17c`, and the bout writes what the
    /// controller left. Clamped, the beast turned in full view at the edge of
    /// the picture instead of running off it.
    #[test]
    fn the_beasts_turn_sets_it_down_off_the_board() {
        let mut beast = def();
        beast.controller = "beast".into();
        beast.approach = 2;
        beast.back_off = 1;
        let knight = def();
        let defs: BTreeMap<String, ActorDef> = [
            ("b".to_string(), beast.clone()),
            ("k".to_string(), knight.clone()),
        ]
        .into_iter()
        .collect();
        let mut b = Bout::new(
            arena_field(),
            vec![
                Fighter::new("b", &beast, 0x154, 100, 1),
                Fighter::new("k", &knight, 160, 100, -1),
            ],
        );
        b.monster_intent(0, 1, |a| &defs[a], true);
        assert_eq!(
            b.fighters[0].x, 0x17c,
            "02ff3: mov word ptr [di + 2], 0x17c"
        );
        assert_eq!(b.fighters[0].facing, -1, "02ff8: mov byte ptr [di + 8], 3");
    }

    /// A knight who has the scripts the five remaining `InitKnightvs*`
    /// routines hand him, and the creature definitions those fights field.
    fn override_defs() -> BTreeMap<String, ActorDef> {
        use crate::taskvm::{part_flags, End, Instr, Part, Script};
        let mut defs = dragon_defs();
        let knight = defs.get_mut("k").expect("the knight");
        for name in ["Beast_BackToss", "Knight_SwOThrust", "Knight_SwDThrust"] {
            knight.animation.insert(
                name.into(),
                Script::new(vec![
                    Instr::Hold { count: 4 },
                    Instr::Part(Part {
                        table: 1,
                        bank: 0,
                        cel: 9,
                        x: -8,
                        y: 0,
                        flags: part_flags::BODY,
                    }),
                    Instr::EndFrame { end: End::Stop },
                ]),
            );
        }
        for (id, controller) in [
            ("beast", "beast"),
            ("troll", "troll"),
            ("demon", "demon"),
            ("spear", "trogg_spear"),
            ("rat", "ratman"),
        ] {
            let mut c = defs["d"].clone();
            c.controller = controller.into();
            defs.insert(id.to_string(), c);
        }
        defs
    }

    /// The rows the three `*Att` writers put over `KnightAttSw`, and the two
    /// things they are not: they do not change the kind the knight is holding,
    /// so `CheckBlock` still stops what it stopped, and they are not the block
    /// table, which is `KnightBloSw` at `+0x1e` and which no `InitKnightvs*`
    /// touches.
    ///
    /// `SetUpKnight` (0x17ab, 0x17b0) fills `KnightAttSw[8]` with
    /// `Knight_SwBlock` and `[0xe]` with `Knight_SwEvade`, so of the five
    /// writes only three change anything; all five are built because all five
    /// are what the routines do.
    #[test]
    fn each_fight_writes_its_own_guard_scripts_over_the_knights_att_table() {
        let row = |c: Controller, k: Attack| Bout::knight_att_rows(c).get(k.name()).cloned();
        // 0021d8, 0021dd
        assert_eq!(
            row(Controller::TroggSpear, Attack::Evade).as_deref(),
            Some("Knight_SwEvade"),
            "0021d8: the row already there"
        );
        assert_eq!(
            row(Controller::TroggSpear, Attack::Block).as_deref(),
            Some("Knight_SwEvade"),
            "0021dd: the block plays the evade"
        );
        // 00227b
        assert_eq!(
            row(Controller::Beast, Attack::Evade).as_deref(),
            Some("Knight_SwEvade"),
            "00227b: the one *Att write the beast makes"
        );
        // 00232d, 002332
        assert_eq!(
            row(Controller::Ratman, Attack::Block).as_deref(),
            Some("Knight_SwOThrust"),
            "00232d: the overhead thrust"
        );
        assert_eq!(
            row(Controller::Ratman, Attack::Evade).as_deref(),
            Some("Knight_SwDThrust"),
            "002332: the downward thrust"
        );
        // The other ten routines write no `*Att` row at all.
        for c in [
            Controller::Trogg,
            Controller::Troll,
            Controller::Mudman,
            Controller::Balok,
            Controller::Demon,
            Controller::Dragon,
            Controller::Claw,
            Controller::Knight,
        ] {
            assert!(
                Bout::knight_att_rows(c).is_empty(),
                "{c:?} writes nothing over the *Att table"
            );
        }
        // And no writer touches anything but the two guard kinds.
        for c in [
            Controller::TroggSpear,
            Controller::Beast,
            Controller::Ratman,
        ] {
            for (k, _) in Bout::knight_att_rows(c) {
                let kind = Attack::from_name(&k).expect("an attack kind");
                assert!(
                    kind.is_guard(),
                    "{c:?} wrote over {k}, which is not a guard"
                );
            }
        }
        // `KnightAttack` reads the row at the kind it asked for, and the kind
        // it reports is unchanged: the override is the script only.
        let defs = override_defs();
        let knight = &defs["k"];
        let mut f = Fighter::new("k", knight, 100, 100, 1);
        assert_eq!(
            f.attack_script(knight, Attack::Block),
            Some(("block".to_string(), Attack::Block)),
            "the knight's own row before any fight writes over it"
        );
        f.att_rows = Bout::knight_att_rows(Controller::Ratman);
        assert_eq!(
            f.attack_script(knight, Attack::Block),
            Some(("Knight_SwOThrust".to_string(), Attack::Block)),
            "00232d: the script changes and the kind does not"
        );
        assert_eq!(
            f.attack_script(knight, Attack::Swing),
            Some(("swing".to_string(), Attack::Swing)),
            "a kind no row was written for is the knight's own"
        );
        // `CheckBlock` reads `KnightBloSw` at `+0x1e`, which none of the
        // thirteen routines writes, against the kind the knight is holding —
        // and the kind is still the block, so it still stops a swing.
        f.state = State::Guard;
        f.attack = Some(Attack::Block);
        f.facing = 1;
        assert!(
            f.blocks(knight, -1, Attack::Swing),
            "the ratmen's row does not move KnightBloSw"
        );
    }

    /// `SetKnightCombat` and the `*Att` half of the `InitKnightvs*` after it:
    /// the rows go on every knight in the arena, because the table is one
    /// global that every knight record's `+0x16` points at, and a script the
    /// definition has not got is left out rather than handed over.
    #[test]
    fn the_att_rows_go_on_every_knight_and_only_for_scripts_he_has() {
        let defs = override_defs();
        let mut b = Bout::new(
            arena_field(),
            vec![
                Fighter::new("k", &defs["k"], 100, 100, 1),
                Fighter::new("k", &defs["k"], 200, 100, -1),
                Fighter::new("rat", &defs["rat"], 50, 100, 1),
            ],
        );
        b.init_knight_att(|n| &defs[n]);
        for i in [0, 1] {
            assert_eq!(
                b.fighters[i].att_rows,
                Bout::knight_att_rows(Controller::Ratman),
                "both knights read the one table"
            );
        }
        assert!(
            b.fighters[2].att_rows.is_empty(),
            "a creature has a table of its own and no rows written over it"
        );
        // A pack with no `Knight_SwOThrust` keeps the row it had.
        let mut bare = defs.clone();
        let k = bare.get_mut("k").expect("the knight");
        k.animation.remove("Knight_SwOThrust");
        let mut b = Bout::new(
            arena_field(),
            vec![
                Fighter::new("k", &bare["k"], 100, 100, 1),
                Fighter::new("rat", &bare["rat"], 50, 100, 1),
            ],
        );
        b.init_knight_att(|n| &bare[n]);
        assert_eq!(
            b.fighters[0].att_rows.get("block"),
            None,
            "a script the definition has not got is left out"
        );
        assert_eq!(
            b.fighters[0].att_rows.get("evade").map(String::as_str),
            Some("Knight_SwDThrust"),
            "and the other row still goes on"
        );
        // A fight with no creature in it writes nothing: that is
        // `InitKnightvsKnight`, which has no `*Att` writes of its own.
        let mut duel = Bout::new(
            arena_field(),
            vec![
                Fighter::new("k", &defs["k"], 100, 100, 1),
                Fighter::new("k", &defs["k"], 200, 100, -1),
            ],
        );
        duel.init_knight_att(|n| &defs[n]);
        assert!(duel.fighters[0].att_rows.is_empty());
    }

    /// **`InitKnightvsBeast` writes two tables and not one.** `si` is loaded
    /// from `[di+0x16]` for the first write and reloaded from `[di+0x14]` for
    /// the next two, and the two writes at `+0xe` either side of that reload
    /// land in different tables — `KnightAttSw` is DS:`0x69b0` and
    /// `KnightHitSw` DS:`0x69c2`. Fold them together either way round and this
    /// test fails.
    #[test]
    fn the_beast_writes_the_knights_att_table_and_his_hit_table_separately() {
        let defs = override_defs();
        let (knight, beast) = (&defs["k"], &defs["beast"]);
        let att = Bout::knight_att_rows(Controller::Beast);
        // 00227b went into the *Att* table: the evade, and it is the evade.
        assert_eq!(
            att.get("evade").map(String::as_str),
            Some("Knight_SwEvade"),
            "00227b is an *Att row and holds Knight_SwEvade, not the toss"
        );
        assert_eq!(
            att.get("chop"),
            None,
            "002283 is a *Hit row: folding the tables would have put it here"
        );
        assert_eq!(att.len(), 1, "one *Att write and no more");
        // 002283 and 002288 went into the *Hit* table: both the toss.
        for (kind, site) in [(Attack::Chop, "002283"), (Attack::Evade, "002288")] {
            let mut b = dragon_bout(60);
            b.fighters[0].struck(knight, 5, Some(kind));
            b.beast_struck_knight(0, Some(kind), beast, knight);
            assert_eq!(
                b.fighters[0].script, "Beast_BackToss",
                "{site}: the *Hit row is the toss and not the evade"
            );
        }
        // And the two do not reach into each other: a knight with the beast's
        // `*Att` row on him, on the toss, still guards with the evade.
        let mut b = dragon_bout(60);
        b.fighters[0].att_rows = att;
        b.fighters[0].struck(knight, 5, Some(Attack::Evade));
        b.beast_struck_knight(0, Some(Attack::Evade), beast, knight);
        assert_eq!(b.fighters[0].script, "Beast_BackToss");
        assert_eq!(
            b.fighters[0].attack_script(knight, Attack::Evade),
            Some(("Knight_SwEvade".to_string(), Attack::Evade)),
            "the *Att row is still the evade"
        );
        // The beast writes no other row: its charge is 0x10 and nothing in a
        // beast fight writes 0xe, but the lunge, the swing and the rest keep
        // whatever `Fighter::struck` chose.
        for kind in [Attack::Lunge, Attack::Swing, Attack::RThrust] {
            let mut b = dragon_bout(60);
            b.fighters[0].struck(knight, 5, Some(kind));
            b.beast_struck_knight(0, Some(kind), beast, knight);
            assert_ne!(
                b.fighters[0].script, "Beast_BackToss",
                "{kind:?} has no row"
            );
        }
    }

    /// `InitKnightvsTroll+16` (0x26ba) and `InitKnightvsDemon+40` (0x2765):
    /// the club and the slap throw the knight by the very same
    /// `Knight_SwSlapped` a claw and an uppercut do, and the other kinds each
    /// creature has keep the rows `SetUpKnight` wrote.
    ///
    /// The demon's is the row that settles an earlier mistake: `DemonSlap`
    /// (0x437f) writes neither `SLAP` nor `SLAPY` and was read as a demon's
    /// slap throwing nobody. It is the struck side of the blow; this row is
    /// what puts him on the script, and `DemonAttack` (0x5059..0x5062) is what
    /// writes the direction and the table.
    #[test]
    fn the_trolls_club_and_the_demons_slap_throw_the_knight() {
        let defs = override_defs();
        let knight = &defs["k"];
        // 0026ba: kind 4, the bunt.
        let mut b = dragon_bout(60);
        b.fighters[0].struck(knight, 5, Some(Attack::Swing));
        b.troll_struck_knight(0, Some(Attack::Swing), &defs["troll"], knight);
        assert_eq!(b.fighters[0].script, "Knight_SwSlapped", "0026ba: kind 4");
        // The overhead chop is kind 0x10 and has no row.
        let mut b = dragon_bout(60);
        b.fighters[0].struck(knight, 5, Some(Attack::Chop));
        b.troll_struck_knight(0, Some(Attack::Chop), &defs["troll"], knight);
        assert_ne!(
            b.fighters[0].script, "Knight_SwSlapped",
            "the chop has no row of its own"
        );
        // 002765: kind 0x10, the slap.
        let mut b = dragon_bout(60);
        b.fighters[0].struck(knight, 5, Some(Attack::Chop));
        b.demon_struck_knight(0, Some(Attack::Chop), &defs["demon"], knight);
        assert_eq!(
            b.fighters[0].script, "Knight_SwSlapped",
            "002765: kind 0x10, so a demon's slap does throw him"
        );
        // The whip is kind 2 and the zap kind 4, and neither has a row.
        for kind in [Attack::Lunge, Attack::Swing] {
            let mut b = dragon_bout(60);
            b.fighters[0].struck(knight, 5, Some(kind));
            b.demon_struck_knight(0, Some(kind), &defs["demon"], knight);
            assert_ne!(
                b.fighters[0].script, "Knight_SwSlapped",
                "{kind:?} keeps the row SetUpKnight wrote"
            );
        }
        // And none of the three rows is written for anybody but the knight, or
        // by anybody but the creature whose routine wrote it.
        let mut b = dragon_bout(60);
        b.fighters[0].struck(knight, 5, Some(Attack::Chop));
        b.troll_struck_knight(0, Some(Attack::Chop), &defs["demon"], knight);
        b.demon_struck_knight(0, Some(Attack::Swing), &defs["troll"], knight);
        b.beast_struck_knight(0, Some(Attack::Chop), &defs["troll"], knight);
        assert_ne!(b.fighters[0].script, "Knight_SwSlapped");
        assert_ne!(b.fighters[0].script, "Beast_BackToss");
    }

    /// `TrollBunt` (0x5694..0x569a) and `DemonAttack` (0x5059..0x5062 and
    /// 0x509e..0x50a4): the two controllers that were read as writing no
    /// `SLAP` at all, and the whip's own table.
    ///
    /// `SLAPY` is a pointer, and the image's six stores give it two values:
    /// `BalokSLAP` itself and `DemonWHIP`, which is `BalokSLAP + 10`. So it is
    /// an index into [`crate::monster::BALOK_SLAP`] here, not a constant.
    #[test]
    fn the_bunt_and_the_slap_write_the_throws_direction_and_its_table() {
        use crate::monster::{slap_entry, slap_y, Act, Shared, Sight};
        let plain = def();
        let run = |controller: &str, facing: i32, gap: i32| -> (Act, i32, i32) {
            let mut me_def = def();
            me_def.controller = controller.into();
            // `+0x52`, wide enough that `MonsterTrack` is inside its approach
            // and hands the controller the attack branch rather than a step;
            // the distance the branch is chosen by is `FindDistance`'s, which
            // is `gap`.
            me_def.approach = 400;
            let mut me = Fighter::new("m", &me_def, 100, 100, facing);
            // `Fighter::new` gives a demon the `UNBORN` flag, and its first
            // script is the arrival; the branch under test is the one after it.
            me.brain.flags &= !crate::monster::flag::UNBORN;
            let foe = Fighter::new("k", &plain, 100 + gap * facing, 100, -facing);
            let (mut brain, mut shared, mut seed, mut f) =
                (me.brain, Shared::default(), 1u16, facing);
            let sight = Sight {
                me: &me,
                foe: &foe,
                def: &me_def,
                gore: true,
                body: false,
                decapped: false,
                progression: 0,
                perch: None,
                foe_blow: 0,
                head_health: None,
            };
            let act = crate::monster::decide(
                &sight,
                &mut brain,
                &mut seed,
                &mut f,
                &mut shared,
                &mut (0, 0),
            );
            (act, shared.slap, shared.slap_y)
        };
        // 05689: the bunt is kind 4 inside a hundred, and 05697 writes the
        // troll's own `+8` — 1 facing right, 3 facing left.
        let (act, slap, y) = run("troll", 1, 60);
        assert_eq!(
            act,
            Act::Attack {
                kind: Attack::Swing,
                spawn: None
            },
            "05689: the bunt"
        );
        assert_eq!(slap, 1, "05697: mov [SLAP], al with al = 1");
        assert_eq!(y, slap_y::BALOK_SLAP, "0569a: mov [SLAPY], BalokSLAP");
        let (_, slap, _) = run("troll", -1, 60);
        assert_eq!(slap, 3, "and 3 facing left");
        // 0504e: the demon's slap is kind 0x10 inside a hundred, and 0505c
        // and 0505f write the same pair.
        let (act, slap, y) = run("demon", 1, 60);
        assert_eq!(
            act,
            Act::Attack {
                kind: Attack::Chop,
                spawn: None
            },
            "0504e: the slap"
        );
        assert_eq!(slap, 1, "0505c: mov [SLAP], al");
        assert_eq!(y, slap_y::BALOK_SLAP, "0505f: mov [SLAPY], BalokSLAP");
        // 0508e: the whip is kind 2 out to a hundred and forty, and 050a4
        // points `SLAPY` at `DemonWHIP` instead — the same storage, five
        // words in, so every entry is negative and the knight is dragged in.
        let (act, slap, y) = run("demon", 1, 138);
        assert_eq!(
            act,
            Act::Attack {
                kind: Attack::Lunge,
                spawn: None
            },
            "0508e: the whip"
        );
        assert_eq!(slap, 1, "050a1: mov [SLAP], al");
        assert_eq!(y, slap_y::DEMON_WHIP, "050a4: mov [SLAPY], DemonWHIP");
        // 04504: and the read is `SLAPY[SLAPCNT]`, from whichever word that is.
        let four = |base: i32| -> Vec<i32> { (0..4).filter_map(|c| slap_entry(base, c)).collect() };
        assert_eq!(four(slap_y::BALOK_SLAP), vec![30, 25, 20, 20], "a throw");
        assert_eq!(four(slap_y::DEMON_WHIP), vec![-7, -3, -1, 0], "a drag");
    }

    /// The script's own gosubs, end to end. `Knight_SwSlapped` (DS:0x1596) is
    /// two frames, each `TASKHOLD 02` with the gosub after the hold, and
    /// `TASKHOLD`'s resume point (0x9ac5: `add word [di+2], 2`) is the
    /// instruction after itself — so the gosub runs on both shows of each
    /// frame, `KnightSLAP` runs four times, and the throw is 30 + 25 + 20 + 20
    /// before `TASKGOTO Knight_GetUp`. Words 4, 9 and 10 of `BalokSLAP` are
    /// never read by anything: a throw takes 0 to 3 and the demon's whip,
    /// whose `SLAPY` is `DemonWHIP`, takes 5 to 8.
    #[test]
    fn the_slap_script_runs_the_slap_four_times_and_no_more() {
        use crate::taskvm::{End, Instr, Part, Script};
        let (dragon, claw, mut knight) = dragon_set_piece();
        let gosub = |r: &str| Instr::Gosub { routine: r.into() };
        let part = Instr::Part(Part {
            table: 1,
            bank: 0,
            cel: 17,
            x: 0,
            y: 0,
            flags: 0,
        });
        // 1596..15cc, with the second frame stopping where the original goes
        // to `Knight_GetUp`: the two sounds and the five parts are not what is
        // under test, the two gosubs are.
        knight.animation.insert(
            "Knight_SwSlapped".into(),
            Script::new(vec![
                gosub("InitSLAP"),
                Instr::Hold { count: 2 },
                gosub("KnightSLAP"),
                part.clone(),
                Instr::EndFrame { end: End::Next },
                Instr::Hold { count: 2 },
                gosub("KnightSLAP"),
                part,
                Instr::EndFrame { end: End::Stop },
            ]),
        );
        let defs: std::collections::BTreeMap<String, ActorDef> =
            [("k", knight), ("d", dragon), ("c", claw)]
                .into_iter()
                .map(|(n, d)| (n.to_string(), d))
                .collect();
        let mut b = dragon_bout(100);
        b.shared.slap_cnt = 0;
        b.shared.slap = 1;
        b.fighters[0].state = State::Idle;
        b.fighters[0].enter_on(State::Hurt, "Knight_SwSlapped".to_string());
        for _ in 0..40 {
            b.step_with(|n| &defs[n], &[Intent::default(); 4]);
            if b.fighters[0].script != "Knight_SwSlapped" {
                break;
            }
        }
        assert_eq!(b.shared.slap_cnt, 3, "four KnightSLAPs and no fifth");
        assert_eq!(b.fighters[0].x, 195, "30 + 25 + 20 + 20");
    }

    /// `DragonHit2` (0x3ad5): the bite that closes takes the knight's task
    /// away and puts the chewing on in the bite's place, and `KillKnight`
    /// inside it kills him and nobody else.
    #[test]
    fn the_bite_that_closes_is_the_chewing_and_the_chewing_is_the_end() {
        let defs = dragon_defs();
        let dragon = &defs["d"];
        let mut b = dragon_bout(150);
        b.fighters[1].enter_on(State::Attack, "Dragon_HighBite".into());
        b.fighters[1].attack = Some(Attack::Lunge);
        // 03ade: a breath that touches him carries on.
        assert!(!b.dragon_bites(1, 0, Some(Attack::Chop), dragon));
        assert!(!b.fighters[0].hidden);
        // 03ae7: the bite.
        assert!(b.dragon_bites(1, 0, Some(Attack::Lunge), dragon));
        assert!(b.fighters[0].hidden, "03aea: his task is killed");
        assert_eq!(b.fighters[0].holder, Some(1));
        assert!(!b.fighters[0].alive(), "and his record freed");
        assert_eq!(b.shared.dragon_bodge[2], 1, "03aed: dragonbodge3");
        assert_eq!(b.fighters[1].script, "Dragon_BitKnight", "03af3");
        assert_eq!(b.fighters[1].state, State::Attack);
        // The chewing runs: `KillKnight` on him and `StopCombat` at the end,
        // and the claws stay where the head is.
        let mut ticks = 0;
        while !b.settled() {
            b.step_with(|n| &defs[n], &[Intent::default(); 4]);
            ticks += 1;
            assert!(ticks < 100, "the chewing never ended");
        }
        assert!(b.stopped, "04880: StopCombat");
        assert!(
            b.fighters[1].alive(),
            "KillKnight (0xab2) is [0x8979], the knight, and not the caller's foe"
        );
        assert_eq!(
            b.fighters[2].y,
            b.fighters[1].y + 10,
            "03bc5: claw one ten deeper"
        );
        assert_eq!(
            b.fighters[3].y,
            b.fighters[1].y - 20,
            "03bcb: claw two twenty nearer"
        );
    }

    /// `Dragon_Dead` (0x3e40) as the bout runs it: `DrDropHead` (0x3bd2)
    /// puts the head thirty eight rows down the screen, `StopCombat` ends
    /// the fight, and `DrDropClaws` (0x3be1) has both claws kill their own
    /// tasks on their next pass, having held `Dragon_ClawDead` since the
    /// head's hit points went.
    #[test]
    fn the_dead_dragon_drops_its_head_and_then_its_claws() {
        let defs = dragon_defs();
        let mut b = dragon_bout(150);
        // The last blow: the head onto `Dragon_Hit`, whose `TASKDEAD` is the
        // death.
        b.fighters[1].struck(&defs["d"], 200, Some(Attack::Swing));
        assert!(!b.fighters[1].alive());
        let height = b.fighters[1].brain.height;
        // Two claw passes with the head down: `Dragon_ClawDead`, and no slap
        // however close he stands.
        b.fighters[0].x = 60;
        for claw in [2, 3] {
            b.monster_intent(claw, 0, |n| &defs[n], true);
            assert_eq!(
                b.fighters[claw].ordered.as_ref().map(|o| o.script.as_str()),
                Some("Dragon_ClawDead"),
                "03b58"
            );
        }
        let mut ticks = 0;
        let mut dropped_at = None;
        while b.fighters[1].task.as_ref().is_some_and(|t| t.active) {
            b.step_with(|n| &defs[n], &[Intent::default(); 4]);
            ticks += 1;
            if dropped_at.is_none() && b.fighters[1].brain.height != height {
                dropped_at = Some(ticks);
            }
            assert!(ticks < 100, "Dragon_Dead never ended");
        }
        assert_eq!(
            b.fighters[1].brain.height,
            height + 0x26,
            "03bdb: add word [di+6], 0x26"
        );
        assert!(dropped_at.is_some());
        assert!(b.stopped, "04030: StopCombat");
        assert_eq!(b.shared.dead_claws, -1, "03be3: DEAD_CLAWS");
        // And the claws, on their next pass, are gone: no task, no body, not
        // among the standing.
        for claw in [2, 3] {
            b.fighters[claw].brain.rest = 0;
            b.monster_intent(claw, 0, |n| &defs[n], true);
            assert!(!b.fighters[claw].alive(), "03b61: CLAWS_DEAD");
            assert!(b.fighters[claw].task.as_ref().is_some_and(|t| !t.active));
        }
        assert!(b.settled());
        assert_eq!(b.winner(), Some(0));
    }

    /// `AddDragonFIRE` (0x3afc): the fire is a task of its own fifty five
    /// pixels along and five rows deeper than the head, at no height, facing
    /// right, on the creature's banks, and it lands as thirty.
    #[test]
    fn the_high_breaths_fire_is_a_task_of_its_own_beside_the_head() {
        let defs = dragon_defs();
        let mut b = dragon_bout(150);
        b.breathe(1, "Dragon_Fire", &defs["d"]);
        assert_eq!(b.missiles.len(), 1);
        let m = &b.missiles[0];
        let head = b.fighters[1].task.as_ref().unwrap();
        assert_eq!(m.task.x, head.x + 0x37, "03b1b: add ax, 0x37");
        assert_eq!(m.task.y, head.y + 5, "03b16: add cx, 5");
        assert_eq!(m.task.z, 0, "03b10: mov bx, 0");
        assert_eq!(m.task.facing, FACING_RIGHT, "03b19: mov dh, 1");
        assert_eq!(m.depth, b.fighters[1].y + 5);
        assert_eq!(
            m.attack,
            Some(Attack::Chop),
            "kind 0x14 is DragonFire1: thirty"
        );
        assert_eq!(m.owner, 1);
    }

    /// `TrackKnight` by `TASKGOSUB` (0x4448): the head moves while its own
    /// breath plays, on the ranges of two and one the breath bit puts in.
    #[test]
    fn the_breath_scripts_track_the_knight_mid_frame() {
        let defs = dragon_defs();
        let mut b = dragon_bout(300);
        b.fighters[1].x = 90;
        b.shared.dragon = crate::monster::dragon_flag::BREATHING;
        b.track_knight(1, &|n| &defs[n]);
        assert_eq!(b.fighters[1].x, 95, "03c36: five to the right");
        assert_eq!(b.fighters[1].facing, 1, "03c1f");
        assert_eq!(
            b.shared.dragon_ranges,
            Some((60, 60)),
            "03c84: DCL was +0x52"
        );
        let t = b.fighters[1].task.as_ref().unwrap();
        assert_eq!(t.x, 95 + defs["d"].origin[0] as i32, "03c6a: the task's x");
        // Nobody but the dragon runs it.
        b.fighters[0].x = 200;
        b.track_knight(0, &|n| &defs[n]);
        assert_eq!(b.fighters[0].x, 200);
    }

    /// `BalokHit` (0x377a) and `BalokGrabbed` (0x379b): the grab takes hold of
    /// the knight, and `ControlBalok`'s own flag word carries the three frames
    /// after it.
    #[test]
    fn baloks_grab_takes_hold_and_the_shake_and_the_release_follow() {
        use crate::monster::{balok_flag, flag};
        let mut balok = def();
        balok.controller = "balok".into();
        for name in [
            "Balok_GrabKnight",
            "Balok_ShakeKnight",
            "Balok_BiteKnight",
            "Balok_SqueezeKnight",
            "Balok_Recover",
        ] {
            balok.animation.insert(name.into(), Default::default());
        }
        let knight = def();
        let mut b = Bout::new(
            arena_field(),
            vec![
                Fighter::new("b", &balok, 100, 100, 1),
                Fighter::new("k", &knight, 160, 100, -1),
            ],
        );
        // 0377d: the uppercut's blow is an ordinary one.
        b.fighters[0].attack = Some(Attack::Swing);
        assert!(!b.balok_hit(0, 1, &balok));
        assert!(!b.fighters[1].hidden);
        // The grab, kind 0x10.
        b.fighters[0].attack = Some(Attack::Chop);
        assert!(b.balok_hit(0, 1, &balok));
        assert_eq!(b.shared.balok & balok_flag::HELD, balok_flag::HELD);
        assert!(b.fighters[0].brain.flags & flag::GRABBED != 0);
        assert!(b.fighters[1].hidden && b.fighters[1].holder == Some(0));
        // And what the controller makes of that, frame by frame: the grab,
        // the shake, and then the release while he still lives.
        let mut brain = b.fighters[0].brain;
        let mut shared = b.shared;
        let mut seed = 1u16;
        let mut facing = 1;
        let held = crate::monster::Sight {
            me: &b.fighters[0],
            foe: &b.fighters[1],
            def: &balok,
            gore: true,
            body: false,
            decapped: false,
            progression: 0,
            perch: None,
            foe_blow: 0,
            head_health: None,
        };
        let grip = |a: &crate::monster::Act| match a {
            crate::monster::Act::Grip { script, hold, .. } => Some((script.clone(), *hold)),
            _ => None,
        };
        let first = crate::monster::decide(
            &held,
            &mut brain,
            &mut seed,
            &mut facing,
            &mut shared,
            &mut (0, 0),
        );
        assert_eq!(
            grip(&first),
            Some(("Balok_GrabKnight".to_string(), true)),
            "0x37a0"
        );
        let second = crate::monster::decide(
            &held,
            &mut brain,
            &mut seed,
            &mut facing,
            &mut shared,
            &mut (0, 0),
        );
        assert_eq!(
            grip(&second),
            Some(("Balok_ShakeKnight".to_string(), true)),
            "ControlBalokGrab, 0x37ae"
        );
        assert_eq!(shared.balok & balok_flag::RELEASING, balok_flag::RELEASING);
        let third = crate::monster::decide(
            &held,
            &mut brain,
            &mut seed,
            &mut facing,
            &mut shared,
            &mut (0, 0),
        );
        assert_eq!(
            grip(&third),
            Some(("Balok_Recover".to_string(), false)),
            "ControlBalokRelease, 0x37fa: he lived, so it lets go"
        );
        // 037f8: and if he had not, it would be eating him instead.
        let mut dead = b.fighters[1].clone();
        dead.health = 0;
        let done = crate::monster::Sight { foe: &dead, ..held };
        let mut shared = crate::monster::Shared {
            balok: balok_flag::RELEASING,
            ..Default::default()
        };
        let bite = crate::monster::decide(
            &done,
            &mut brain,
            &mut seed,
            &mut facing,
            &mut shared,
            &mut (0, 0),
        );
        assert_eq!(
            grip(&bite),
            Some(("Balok_BiteKnight".to_string(), true)),
            "ControlBalokBite, 0x37c8"
        );
        let mut shared = crate::monster::Shared {
            balok: balok_flag::RELEASING,
            balok_bite: 1,
            ..Default::default()
        };
        let squeeze = crate::monster::decide(
            &done,
            &mut brain,
            &mut seed,
            &mut facing,
            &mut shared,
            &mut (0, 0),
        );
        assert_eq!(
            grip(&squeeze),
            Some(("Balok_SqueezeKnight".to_string(), true)),
            "ControlBalokCrush, 0x37dc: every other one"
        );
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
            vec![
                Fighter::new("k", &def, 200, 100, -1),
                Fighter::new("m", &def, -50, 100, 1),
            ],
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
            vec![
                Fighter::new("k", &d, 200, 100, -1),
                Fighter::new("m", &d, 40, 100, 1),
            ],
        );
        let wave_def = WaveDef {
            max: 1,
            heads: 10,
            reinforced: true,
            ..WaveDef::default()
        };
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
        assert_eq!(
            b.field, arena,
            "an ordinary fight is fought on the arena's own ground"
        );

        let mut b = Bout::new(
            arena.clone(),
            vec![
                Fighter::new("k", &plain, 40, 100, 1),
                Fighter::new("demon", &demon, 200, 100, -1),
            ],
        );
        b.apply_actor_borders(pick);
        assert_eq!(
            b.field.borders,
            vec![Border {
                left: 0,
                right: 309,
                bottom: 99,
                top: 10
            }],
            "the demon's record replaces the arena's list rather than joining it"
        );
        assert_eq!(
            b.field.floor(),
            99,
            "and the ground begins fifteen rows higher"
        );

        let mut b = Bout::new(
            arena.clone(),
            vec![
                Fighter::new("k", &plain, 40, 100, 1),
                Fighter::new("ugly", &ugly, 200, 100, -1),
            ],
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
        assert!(
            !intent.attack,
            "the order carries the attack, not the intent"
        );
        let order = b.fighters[1]
            .ordered
            .clone()
            .expect("the controller said nothing");
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

    /// `RatmanHit` (0x34f0) calling `FlipKnight` (0x3d13): a ratman's claw on
    /// someone facing the same way turns him round, task and record together,
    /// so the mirror the blit reads turns with him.
    ///
    /// The three ways out of it are covered by
    /// `monster::ratman_flips`'s own test; this is the wiring.
    #[test]
    fn a_ratmans_claw_spins_a_knight_caught_facing_away() {
        use crate::combat::tests::depth_def;
        let mut rat = depth_def();
        rat.controller = "ratman".into();
        let knight = depth_def();
        let pick = |name: &str| -> &ActorDef {
            match name {
                "rat" => Box::leak(Box::new(rat.clone())),
                _ => Box::leak(Box::new(knight.clone())),
            }
        };
        // The rat is behind him: both face right, so its claw comes at his
        // back and the two `+8` bytes are equal.
        let mut b = Bout::new(
            arena_field(),
            vec![
                Fighter::new("k", &knight, 100, 100, 1),
                Fighter::new("rat", &rat, 70, 100, 1),
            ],
        );
        b.fighters[1].facing = 1;
        let mut landed = false;
        for _ in 0..60 {
            let ev = b.step_with(
                pick,
                &[
                    Intent::default(),
                    Intent {
                        dx: 0,
                        dy: 0,
                        attack: true,
                    },
                ],
            );
            if ev.iter().any(|e| e.attacker == 1 && e.target == 0) {
                landed = true;
                break;
            }
        }
        assert!(landed, "the claw landed");
        assert_eq!(
            b.fighters[0].facing, -1,
            "FlipKnight: 03d26 xor byte [si+0x14], 2, 03d31 into the record"
        );
        assert_eq!(
            b.fighters[0].task.as_ref().map(|t| t.mirror()),
            Some(true),
            "and the blit's mirror turned with it"
        );

        // Facing each other, `0351f jne 03524` skips the call and nothing
        // about the knight's facing changes.
        let mut b = Bout::new(
            arena_field(),
            vec![
                Fighter::new("k", &knight, 100, 100, -1),
                Fighter::new("rat", &rat, 70, 100, 1),
            ],
        );
        b.fighters[1].facing = 1;
        for _ in 0..60 {
            let ev = b.step_with(
                pick,
                &[
                    Intent::default(),
                    Intent {
                        dx: 0,
                        dy: 0,
                        attack: true,
                    },
                ],
            );
            if ev.iter().any(|e| e.attacker == 1 && e.target == 0) {
                break;
            }
        }
        assert_eq!(
            b.fighters[0].facing, -1,
            "no flip when they face each other"
        );
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
            let ev = b.step_with(
                pick,
                &[
                    Intent {
                        dx: 0,
                        dy: 0,
                        attack: true,
                    },
                    Intent::default(),
                ],
            );
            hits += ev.len();
            if hits > 0 {
                break;
            }
        }
        assert!(hits > 0, "the swing landed");
        assert_eq!(
            b.fighters[1].brain.cooldown, 0,
            "TroggStruck+3: mov byte [di+0x4a], 0"
        );
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
            let ev = b.step_with(
                pick,
                &[
                    Intent::default(),
                    Intent {
                        dx: 0,
                        dy: 0,
                        attack: true,
                    },
                ],
            );
            hits += ev.len();
            if hits > 0 {
                break;
            }
        }
        assert!(hits > 0, "the trogg's swing landed");
        assert_eq!(
            b.fighters[1].brain.cooldown, 10,
            "TroggHit+8: mov byte [di+0x4a], 0xa"
        );
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
        assert_eq!(
            first.attack,
            Some(Attack::Swing),
            "TroggAttack+0x41: jmp TroggSwing"
        );
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
            let intents = [
                Intent {
                    dx: 0,
                    dy: 0,
                    attack: true,
                },
                Intent::default(),
                Intent::default(),
            ];
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
            dx: if (t / 7 + i).is_multiple_of(3) { 1 } else { -1 },
            dy: if (t / 11 + i).is_multiple_of(4) { 1 } else { 0 },
            attack: (t / 5 + i).is_multiple_of(4),
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
                .map(|i| Intent {
                    dx: 1,
                    dy: 0,
                    attack: (t + i) % 6 == 0,
                })
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
                .map(|i| Intent {
                    dx: -1,
                    dy: 0,
                    attack: (t + i) % 5 == 0,
                })
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
            dx: if (t / 5 + i).is_multiple_of(3) { 1 } else { -1 },
            dy: 0,
            attack: (t / 4 + i).is_multiple_of(3),
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

    /// The demon's whip drags the knight in, end to end.
    ///
    /// `DemonOWhipFollow` (0x50de) hands him `Knight_SwSlapped` outright --
    /// `mov si, 0x1596; call REPLACEANIM` on the knight's own record -- and
    /// that script is the only one in the image that gosubs `InitSLAP` and
    /// `KnightSLAP`. Without the hand-off the knight took the blow, stood
    /// where he was, and `SLAPY`'s `DemonWHIP` was written every whip and
    /// read by nobody.
    #[test]
    fn the_whip_puts_the_knight_on_the_thrown_script_and_drags_him_in() {
        let (_, _, knight) = dragon_set_piece();
        let mut demon = crate::combat::tests::depth_def();
        demon.controller = "demon".into();
        demon.damage = 4;
        let any = demon
            .animation
            .values()
            .next()
            .cloned()
            .expect("the fixture has scripts");
        for name in ["Demon_OWhipHit", "Demon_OWhipKnight"] {
            demon.animation.insert(name.into(), any.clone());
        }
        demon.damage = 4;
        let mut d = Fighter::new("d", &demon, 100, 100, 1);
        d.brain.flags |= crate::monster::flag::CAUGHT | crate::monster::flag::DRIVEN;
        // Mid-chain: phase 2 is `DemonOWhipFollow`.
        d.brain.phase = 2;
        d.brain.flags &= !crate::monster::flag::UNBORN;
        let k = Fighter::new("k", &knight, 230, 100, -1);
        let mut b = Bout::new(arena_field(), vec![d, k]);
        b.shared.slap_y = crate::monster::slap_y::DEMON_WHIP;
        let pick = |n: &str| -> &ActorDef {
            if n == "d" {
                &demon
            } else {
                &knight
            }
        };
        for _ in 0..40 {
            // The controller runs on the frame a script ended, which is what
            // the original's task loop does; `step_with` is the tick.
            let i = b.monster_intent(0, 1, pick, true);
            b.step_with(pick, &[i, Intent::default()]);
            if b.fighters[1].script == "Knight_SwSlapped" {
                break;
            }
        }
        assert_eq!(
            b.fighters[1].script, "Knight_SwSlapped",
            "050de: the knight is handed the thrown script, not his own hurt row",
        );
        assert_eq!(
            b.fighters[0].brain.flags & crate::monster::flag::CAUGHT,
            0,
            "050e5: and the whip lets him go",
        );
    }

    /// The screen shake: `ShakeADD` (0x493f) sets `ShakeCOUNT` to one unless
    /// one is already pending, and `COLCON` (0x4988) spends it on the next
    /// pass. Two callers in the whole game, `Troll_Chop`'s gosub and
    /// `BalokJumping+12` (0x374b).
    #[test]
    fn a_shake_is_asked_for_once_and_spent_on_the_next_pass() {
        let d = def();
        let mut b = Bout::new(arena_field(), vec![Fighter::new("k", &d, 100, 100, 1)]);
        // 0x33e: `InitCombat` clears it.
        assert_eq!(b.shake_count, 0);
        assert!(!b.take_shake(), "nothing pending, nothing shakes");
        b.shake();
        assert_eq!(b.shake_count, 1, "0494d");
        // 04946: a second ask while one is pending changes nothing.
        b.shake();
        assert_eq!(b.shake_count, 1);
        // 0498f and 04993: the decrement reaches nought, so this is the pass.
        assert!(b.take_shake());
        assert_eq!(b.shake_count, 0);
        assert!(!b.take_shake(), "and only that pass");
    }

    /// The beast's toss, the two things that kept it off the screen.
    ///
    /// `InitKnightvsBeast` (0x2283, 0x2288) puts `Beast_BackToss` on the
    /// knight's own `*Hit` rows for kinds 0x10 and 0xe. Reaching it needs
    /// both of these, and each was missing on its own:
    ///
    /// * the beast's kind. `ControlBeast+17` (0x2fb2) writes 0x10 into
    ///   `+0x28` on **every tick, before any branch**, because the charge is
    ///   its walk and it never enters an attack state at all. A kind written
    ///   only on an attack order is a kind the beast never has.
    /// * the banks. The script opens `TASKCELBUF 2`, and table 2 is the
    ///   encounter's loaded creature for everybody in the arena, the knight
    ///   included, which is `World::encounter_banks`.
    #[test]
    fn the_beast_keeps_its_kind_through_a_walk_so_its_toss_row_is_reachable() {
        let mut beast = def();
        beast.controller = "beast".into();
        let mut knight = def();
        knight
            .animation
            .insert("Beast_BackToss".into(), Default::default());
        let mut f = Fighter::new("b", &beast, 100, 100, 1);
        // What the controller wrote, before any order.
        f.brain.kind = Some(Attack::Chop);
        f.ordered = Some(Order {
            state: State::Walk,
            script: String::new(),
            attack: None,
        });
        f.step(
            &beast,
            Intent {
                dx: 1,
                dy: 0,
                attack: false,
            },
            &arena_field(),
        );
        assert_eq!(
            f.attack,
            Some(Attack::Chop),
            "02fb2: the walk order does not clear it",
        );
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
        Field::new(vec![Border {
            left: 0,
            right: 319,
            bottom: 60,
            top: 10,
        }])
    }

    /// Two knights thirty pixels apart, facing each other.
    fn pair(d: &ActorDef) -> Bout {
        Bout::new(
            arena_field(),
            vec![
                Fighter::new("k", d, 100, 100, 1),
                Fighter::new("k", d, 130, 100, -1),
            ],
        )
    }

    fn swing() -> Intent {
        Intent {
            dx: 0,
            dy: 0,
            attack: true,
        }
    }

    /// Down and back, for a knight facing left, is right and down.
    fn block_for(f: &Fighter) -> Intent {
        Intent {
            dx: -f.facing,
            dy: 1,
            attack: true,
        }
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
        let chop = Intent {
            dx: 0,
            dy: -1,
            attack: true,
        };
        let evade = Intent {
            dx: 0,
            dy: 1,
            attack: true,
        };

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
            let attacker = if matches!(t, 0 | 12 | 30) {
                chop
            } else {
                Intent::default()
            };
            let defender = if t == 22 {
                Intent {
                    dx: 0,
                    dy: 1,
                    attack: false,
                }
            } else {
                evade
            };
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
    /// fourteen. `KnightDamSw[0x10]` is four, the same as the swing, and the
    /// doubling is `CalcDamage`'s own `shl ax, 1` (0x2d9d).
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
        let chop = Intent {
            dx: 0,
            dy: -1,
            attack: true,
        };
        assert_eq!(land(0, swing()), 4, "the table alone");
        assert_eq!(
            land(1, swing()),
            5,
            "a new knight: four and his strength of one"
        );
        assert_eq!(land(3, swing()), 7);
        assert_eq!(land(0, chop), 8, "the chop is twice the swing");
        assert_eq!(
            land(1, chop),
            10,
            "(4 + 1) * 2, as CalcDamage doubles after adding"
        );
        assert_eq!(land(3, chop), 14);
    }

    /// `TroggStruck1+36` (0x42e7): `mov ax, [bx]; sub [di+0x38], ax` with
    /// `bx` the trogg's own `*Dam` table at the kind. No `CalcDamage`, so no
    /// strength and no doubling: a creature's chop is its table entry as it
    /// stands, where a knight's is doubled after his sheet is added.
    #[test]
    fn a_creatures_chop_is_its_table_entry_and_is_not_doubled() {
        let mut d = depth_def();
        d.controller = "trogg".into();
        // Six for the chop against four for the swing, so the answer tells
        // the chop's own entry from the swing's, from the sheet, and from a
        // doubling.
        d.attacks.get_mut("chop").unwrap().damage = 6;
        let mut b = pair(&d);
        b.damage = 4;
        b.fighters[0].bonus = 3;
        // The plain controller is bypassed by driving the intent directly,
        // so the fighter is a knight-shaped actor filed as a trogg.
        b.fighters[0].brain.flags &= !crate::monster::flag::DRIVEN;
        let chop = Intent {
            dx: 0,
            dy: -1,
            attack: true,
        };
        let mut dealt = None;
        for _ in 0..8 {
            let hits = b.step(&d, &[chop, Intent::default()]);
            if let Some(h) = hits.first() {
                dealt = Some(h.damage);
                break;
            }
        }
        assert_eq!(
            dealt,
            Some(6),
            "the table's six, neither added to nor doubled"
        );
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
        let away = Intent {
            dx: 1,
            dy: 0,
            attack: false,
        };
        let before = b.fighters[1].x;
        b.step(&d, &[Intent::default(), away]);
        assert!(b.fighters[1].x < before, "told right, went left");
        assert_eq!(b.fighters[1].facing, -1);

        // Forward and up with fire is the up thrust; reversed it is back and
        // down, the block.
        let mut b = pair(&d);
        b.fighters[1].cursed = true;
        let up_thrust = Intent {
            dx: b.fighters[1].facing,
            dy: -1,
            attack: true,
        };
        b.step(&d, &[Intent::default(), up_thrust]);
        assert_eq!(
            b.fighters[1].attack,
            Some(Attack::Block),
            "forward and up came out back and down"
        );
        assert_eq!(b.fighters[1].state, State::Guard);
        let mut sound = pair(&d);
        sound.step(&d, &[Intent::default(), up_thrust]);
        assert_ne!(
            sound.fighters[1].state,
            State::Guard,
            "an uncursed knight does not"
        );
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
            vec![
                Fighter::new("k", &d, 100, 100, 1),
                Fighter::new("k", &d, 220, 100, -1),
            ],
        );
        b.fighters[0].record.set(field::DAGGERS, 2);
        let throw = Intent {
            dx: -1,
            dy: -1,
            attack: true,
        };
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
        assert!(flew.len() > 2, "it was in the air for a while: {flew:?}");
        assert!(
            flew.windows(2).all(|w| w[1] > w[0]),
            "and moving forward: {flew:?}"
        );
        assert_eq!(hits.len(), 1, "it connected once: {hits:?}");
        assert_eq!(
            hits[0].damage, 18,
            "the knife's 3 against the swing's 4, of 25"
        );
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
            vec![
                Fighter::new("k", &d, 100, 100, 1),
                Fighter::new("k", &d, 200, 100, -1),
            ],
        );
        b.fighters[0].record.set(field::DAGGERS, 1);
        let throw = Intent {
            dx: -1,
            dy: -1,
            attack: true,
        };
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
        let gory = kneel_and_strike(false);
        let body = &gory.fighters[1];
        assert_eq!(body.state, State::Dead);
        assert!(!body.alive());
        assert_eq!(
            body.script, "decap",
            "a swing on the kneeling body takes the head"
        );
        let shown = &body.task.as_ref().unwrap().shown;
        assert!(
            shown.iter().any(|p| p.is(part_flags::GATED)),
            "the gore is drawn: {shown:?}"
        );

        let clean = kneel_and_strike(true);
        let body = &clean.fighters[1];
        assert_eq!(body.state, State::Dead);
        let t = body.task.as_ref().unwrap();
        assert_eq!(t.pc.script, "collapse", "TASKSKIP diverts the decapitation");
        assert!(
            t.shown.iter().all(|p| !p.is(part_flags::GATED)),
            "and nothing gated is drawn"
        );
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
            assert_eq!(
                !m.task.shown.is_empty(),
                !bloodless,
                "bloodless {bloodless}: {:?}",
                m.task.shown
            );
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
            vec![
                Fighter::new("k", &d, 60, 100, 1),
                Fighter::new("k", &d, 260, 100, -1),
            ],
        );
        b.fighters[0].record.set(field::DAGGERS, 3);
        let throw = Intent {
            dx: -1,
            dy: -1,
            attack: true,
        };
        for _ in 0..4 {
            b.step(&d, &[throw, Intent::default()]);
        }
        assert!(!b.missiles.is_empty(), "a dagger is in the air");
        let json = serde_json::to_string(&b).unwrap();
        let mut restored: Bout = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, b);
        assert_eq!(restored.state_hash(), b.state_hash());
        for t in 0..30 {
            let intents = [
                throw,
                Intent {
                    dx: -1,
                    dy: 0,
                    attack: false,
                },
            ];
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
        Field::new(vec![Border {
            left: 0,
            right: 319,
            bottom: 60,
            top: 10,
        }])
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
        assert_eq!(
            b.fighters[1].max_health, 300,
            "built from its own definition"
        );
        assert_eq!(b.fighters[1].damage, 7);

        let mut dealt = std::collections::BTreeMap::new();
        for _ in 0..40 {
            let both = [Intent {
                dx: 0,
                dy: 0,
                attack: true,
            }; 2];
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
                vec![
                    Fighter::new("k", &d, 100, 100, 1),
                    Fighter::new("k", &d, 130, 100, -1),
                ],
            )
        };
        let (mut a, mut b) = (mk(), mk());
        for t in 0..80 {
            let intents = [Intent {
                dx: 0,
                dy: 0,
                attack: t % 3 == 0,
            }; 2];
            a.step(&d, &intents);
            b.step_with(|_| &d, &intents);
            assert_eq!(a.state_hash(), b.state_hash(), "tick {t}");
        }
    }
}
