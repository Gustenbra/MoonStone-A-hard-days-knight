//! The arena: backdrop, scenery and two fighters, drawn in one depth-sorted pass
//! and simulated by `henge_core::combat`.
//!
//! Nothing here knows where the pixels came from, so this file is unchanged when
//! the reference art is replaced by our own.

use crate::framebuffer::Framebuffer;
use henge_assets::Registry;
use henge_core::arena::GLOBAL;
use henge_core::battle_palette::{self, BattleColours, Sides};
use henge_core::bout::{Bout, HitEvent};
use henge_core::combat::{Fighter, Intent};
use henge_core::content::{
    ActorData, ActorDef, ArenaData, Arenas, Families, ORIGINAL_KNIGHT_HEALTH,
};
use henge_core::taskvm::{field, Bank, BankTables, Task};
use std::collections::BTreeMap;

const CELL_W: usize = 32;
const CELL_H: usize = 25;

/// Where a fighter's intent comes from. The bout cannot tell them apart, which
/// is the point: a network peer slots in here later without touching combat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    /// Reads a set of keys on this machine. Moonstone was a couch game.
    Local(usize),
    /// The creature's own controller, which lives in the simulation because
    /// the state it keeps is the actor record's. See `henge_core::monster`.
    Ai,
}

/// What the run's knight brings into the arena.
#[derive(Clone, Copy, Debug)]
pub struct Sheet {
    /// `10 * constitution + armour + rings + 10`.
    pub max_health: i32,
    /// Strength and the sword, `CalcDamage`'s additions.
    pub bonus: i32,
    /// The same for a knight fresh off `SetKnightEquipment`, which is what
    /// the other seats fight with: a strength of one and a long sword.
    pub fresh_bonus: i32,
    /// The backfire flag: the player's joystick reversed for this bout.
    pub cursed: bool,
    /// `+0x2e` and `+0x3c` on the knight record, which are the two things
    /// `AdjustLevel` (image `0x2824`) reads off him before it decides how many
    /// creatures a fight holds. See `henge_core::wave`.
    pub strength: i32,
    pub experience: i32,
}

pub struct World {
    arenas: Arenas,
    families: Families,
    actors: ActorData,
    order: Vec<String>,
    index: usize,
    pub bout: Bout,
    pub control: Vec<Control>,
    /// Hits from the last tick, for whoever wants to play a sound.
    pub events: Vec<HitEvent>,
    /// `BattlePal`'s tables: what a bout writes over the backdrop's palette
    /// for the knight, a second knight, each creature and the ground. See
    /// `henge_core::battle_palette`.
    colours: BattleColours,
    /// The second knight's banks: `HE1.OB` to `HE3.OB` and the shared `KN4`
    /// and `KN5`, which is the creature table as `InitKnightvsKnight` loads
    /// it. The same figure painted in indices 9 to 11 instead of 6 to 8, so a
    /// second knight is a different colour without a single pixel changing.
    second_banks: Option<Vec<Bank>>,
    /// `BattlePal` as last composed, for whoever names a fighter by one of
    /// its colours after the frame is drawn.
    palette: [u32; battle_palette::ENTRIES],
    /// What the player brings into the next bout. Wounds carry between fights,
    /// so this is not always full.
    player_health: Option<i32>,
    /// The player's sheet, once a run has chosen a knight: what he can bear
    /// and what `CalcDamage` adds to his blows. With a sheet the fight is at
    /// the original's own scale, a twenty point knight and a four point
    /// swing, and the other knights are what `SetKnightEquipment` makes
    /// them; without one the arena browser's hundred stands and everything
    /// is scaled to it.
    sheet: Option<Sheet>,
    /// Which knight is in which seat, so a fight is fought in the colours the
    /// select screen handed out.
    roster: Vec<usize>,
    /// What the seats not taken by a person are filled with, by actor id. A
    /// knight until the road says otherwise; `set_foe` and the family's own
    /// creature list say otherwise.
    foe: String,
    /// The creatures the pack knows, in a fixed order, for cycling through.
    bestiary: Vec<String>,
    /// The title screen's gore switch. On by default, as the original's
    /// DS:0x700 starts at zero, which its option row reads as `ON`.
    gore: bool,
    /// What the player's knight carries into the next bout, off the run's
    /// sheet. The other knights get the ten `SetKnightEquipment` hands out.
    player_daggers: Option<u32>,
    /// `AddCNT`, the counter `AddKnight` stands every arrival by.
    ///
    /// It lives here rather than on the bout because the original's lives in
    /// BSS and nothing resets it: the rotation carries on from bout to bout, so
    /// which of the three standing places the player's knight gets depends on
    /// how many fighters have stood up before him.
    arrivals: henge_core::arena::Arrivals,
    /// `TotalMonsters` handed over by a lair: `AdjustLevel` at image `0x287e`
    /// replaces the count its `InitKnightvs*` wrote with the lair record's own
    /// `+4`, which is `ForestLairs`'s head count. None on the road, where the
    /// creature's own count stands.
    heads: Option<i32>,
    /// Tonight's moon, as `moon::Phase::key` writes it.
    ///
    /// `SetRatmenTables` reads the phase every time it sets a ratman up, so
    /// what waits in an arena depends on the night the fight starts. The one
    /// creature that answers to it says so on its own definition, which is why
    /// this is a key and not a special case for ratmen.
    moon: String,
}

impl World {
    pub fn load(reg: &Registry) -> anyhow::Result<World> {
        let arenas: Arenas = reg.read_data("data.arenas")?;
        let families: Families = reg.read_data("data.families")?;
        let actors: ActorData = reg.read_data("data.actors")?;
        // A pack without the tables fights in the backdrop's own colours,
        // which is wrong but visible, rather than refusing to fight.
        let colours: BattleColours = reg.read_data("data.battle_palette").unwrap_or_else(|e| {
            eprintln!("no battle palette in the pack, fighters keep the backdrop's colours: {e}");
            BattleColours::default()
        });
        let second_banks = reg
            .read_data::<BTreeMap<String, BankTables>>("data.banks")
            .ok()
            .and_then(|mut b| b.remove("hero"))
            .and_then(|mut tables| tables.remove(&2));
        let mut order: Vec<String> = arenas.keys().cloned().collect();
        order.sort();
        anyhow::ensure!(!order.is_empty(), "no arenas in the pack");
        anyhow::ensure!(
            actors.contains_key("knight"),
            "no knight definition in the pack"
        );

        let field = arenas[&order[0]].field();
        // Everyone the pack can field, the knight first so cycling from the
        // default goes straight to the creatures.
        let mut bestiary: Vec<String> = vec!["knight".into()];
        bestiary.extend(actors.keys().filter(|k| k.as_str() != "knight").cloned());
        let mut w = World {
            arenas,
            families,
            actors,
            order,
            index: 0,
            bout: Bout::new(field, Vec::new()),
            control: Vec::new(),
            events: Vec::new(),
            colours,
            second_banks,
            palette: [0; battle_palette::ENTRIES],
            player_health: None,
            sheet: None,
            roster: (0..4).collect(),
            foe: "knight".into(),
            bestiary,
            gore: true,
            player_daggers: None,
            arrivals: henge_core::arena::Arrivals::default(),
            heads: None,
            moon: String::new(),
        };
        // One person by default. Two would leave the second knight controlled by
        // a keyboard nobody is pressing: it never attacks, never closes, and a
        // bout with it in can never settle. Press 2 to take that seat.
        w.set_players(1);
        Ok(w)
    }

    /// Who this seat is fighting: a creature's opponent is the nearest
    /// knight, a knight's is the nearest creature. `ControlTrogg` and every
    /// other controller begin with `mov ax, [KnightTable]; mov [Opponent], ax`,
    /// so a creature never takes another creature for an enemy.
    fn opposed(&self, seat: usize) -> Option<usize> {
        let me = self.bout.fighters.get(seat)?;
        let want_knight = me.actor != "knight";
        let (x, y) = (me.x, me.y);
        let nearest = |alive_only: bool| {
            self.bout
                .fighters
                .iter()
                .enumerate()
                .filter(|(i, f)| *i != seat && (f.actor == "knight") == want_knight)
                // Not filtered on `hidden`: the original's `Opponent` is the
                // knight's record whatever is being done to him, and a mudman
                // that has hold of one must not lose sight of him for it.
                .filter(|(_, f)| !alive_only || f.alive())
                .min_by_key(|(_, f)| (f.x - x).abs() + (f.y - y).abs() * 2)
                .map(|(i, _)| i)
        };
        // A body is still an opponent: the finisher goes to whoever is down,
        // and a creature that has just killed the knight must not go looking
        // for another creature to fight.
        nearest(true).or_else(|| nearest(false))
    }

    /// The definition behind a seat. Every fighter in a bout was built from
    /// an actor the pack holds, so a miss here is a bug rather than a state.
    fn def_of(&self, actor: &str) -> &ActorDef {
        &self.actors[actor]
    }

    fn def_at(&self, seat: usize) -> &ActorDef {
        self.def_of(&self.bout.fighters[seat].actor)
    }

    /// Is this seat a knight, and so drawn in a knight's colours and named
    /// off the roster, or a creature, drawn as its sheet has it?
    pub fn is_knight(&self, seat: usize) -> bool {
        self.bout
            .fighters
            .get(seat)
            .is_none_or(|f| f.actor == "knight")
    }

    /// Field a particular actor as the opponent. An unknown id is refused and
    /// said so, rather than silently fielding a knight.
    pub fn set_foe(&mut self, actor: &str) -> bool {
        if !self.actors.contains_key(actor) {
            let known: Vec<&str> = self.bestiary.iter().map(String::as_str).collect();
            eprintln!(
                "no actor called {actor}. The pack has: {}",
                known.join(", ")
            );
            return false;
        }
        self.foe = actor.to_string();
        // A new opponent is a new fight, and only a lair hands a head count
        // over. Clearing it here means a raid's fourteen cannot leak into the
        // next ambush on the road; the raid sets it after naming the guardian.
        self.heads = None;
        self.reset();
        true
    }

    /// The next or previous creature in the pack, for the arena browser.
    pub fn step_foe(&mut self, delta: i32) {
        let n = self.bestiary.len() as i32;
        if n == 0 {
            return;
        }
        let at = self
            .bestiary
            .iter()
            .position(|b| *b == self.foe)
            .unwrap_or(0) as i32;
        let next = (((at + delta) % n) + n) % n;
        let id = self.bestiary[next as usize].clone();
        self.set_foe(&id);
    }

    /// The opponent the road produces on this family's ground: the family's
    /// own creature list, indexed by its turn counter, or a knight where the
    /// pack lists nothing.
    pub fn foe_for(&self, family: &str, pick: usize) -> String {
        self.families
            .get(family)
            .and_then(|f| f.creature(pick))
            .filter(|c| self.actors.contains_key(*c))
            .unwrap_or("knight")
            .to_string()
    }

    /// How many people are at the keyboard. The rest of the four are opponents.
    /// Moonstone was a four player game; this is that, with the seats not taken
    /// by a person filled in.
    /// Every seat filled: the arena browser's brawl, and what the 1 and 2 keys
    /// set up. Not what a game fight wants; see `set_seats`.
    pub fn set_players(&mut self, humans: usize) {
        self.set_seats(humans, 4 - humans.clamp(1, 4));
    }

    /// The people at this keyboard, plus this many computer opponents.
    ///
    /// A fight was always four knights, whatever asked for it, and a single
    /// person choosing practice was put up against three at once. At twenty
    /// health that is over in under a second, before the first key is read,
    /// and it does not practise anything. One person practises against one.
    pub fn set_seats(&mut self, humans: usize, foes: usize) {
        let humans = humans.clamp(1, 4);
        let total = (humans + foes).clamp(2, 4);
        self.control = (0..total)
            .map(|i| {
                if i < humans {
                    Control::Local(i)
                } else {
                    Control::Ai
                }
            })
            .collect();
        self.reset();
    }

    pub fn humans(&self) -> usize {
        self.control
            .iter()
            .filter(|c| matches!(c, Control::Local(_)))
            .count()
    }

    /// `TotalMonsters` for the next bout, which a lair hands over and the road
    /// does not: `AdjustLevel` at image `0x287e` writes the lair record's own
    /// `+4` straight over whatever the `InitKnightvs*` routine decided.
    ///
    /// How many of them are on the screen at once is never this number. That is
    /// `MaxMonsters`, which comes off the creature's own definition and through
    /// `AdjustLevel` with it.
    pub fn set_heads(&mut self, heads: Option<i32>) {
        self.heads = heads;
        self.reset();
    }

    /// What `AdjustLevel` reads off the knight, out of the sheet the run handed
    /// over and the swing the pack gives him.
    ///
    /// `CalcDamage` (`0x2d67`) with the swing's kind in `+0x28` is the swing's
    /// own `*Dam` entry plus strength plus the blade, which is exactly the
    /// table damage plus [`Sheet::bonus`]. Without a sheet there is no run and
    /// no knight to read, so a fresh one stands.
    fn level(&self) -> henge_core::wave::Level {
        let knight = &self.actors["knight"];
        let swing = knight.attacks.get(&knight.attack).map_or(4, |a| a.damage);
        let (strength, experience, bonus) = match self.sheet {
            Some(s) => (s.strength, s.experience, s.bonus),
            None => (1, 0, 1),
        };
        henge_core::wave::Level {
            strength,
            experience,
            swing: swing + bonus,
            players: self.humans().max(1) as i32,
        }
    }

    /// Set what the player enters the next bout with. Opponents are always
    /// fresh; the player is whatever the run has left them.
    pub fn set_player_health(&mut self, health: i32) {
        self.player_health = Some(health);
    }

    /// Tonight's moon, for the creatures whose numbers move with it. Nothing
    /// redraws: the phase is read when the next bout is set up, as
    /// `SetRatmenTables` reads it when the fight is built.
    pub fn set_moon(&mut self, phase: &str) {
        if self.moon != phase {
            self.moon = phase.to_string();
            self.reset();
        }
    }

    /// The daggers the player's knight throws from, off the run's sheet.
    pub fn set_player_daggers(&mut self, daggers: u32) {
        self.player_daggers = Some(daggers);
        if let Some(f) = self.bout.fighters.first_mut() {
            f.record.set(field::DAGGERS, daggers as i32);
        }
    }

    /// How many daggers a seat has left, for writing back to the sheet after
    /// a fight. A thrown dagger is a dagger gone.
    pub fn daggers_left(&self, seat: usize) -> u32 {
        self.bout
            .fighters
            .get(seat)
            .map_or(0, |f| f.record.get(field::DAGGERS).max(0) as u32)
    }

    /// The gore switch, from the title. Takes effect on the next reset, the
    /// way the original reads DS:0x700 when a fight is set up.
    pub fn set_gore(&mut self, on: bool) {
        self.gore = on;
        self.bout.bloodless = !on;
    }

    /// A finisher still playing on a fallen fighter, which the bout's end
    /// should wait for rather than cut off mid-fall.
    pub fn finishing(&self) -> bool {
        let actors = &self.actors;
        self.bout.finishing(|name| &actors[name])
    }

    /// Fight on the chosen knight's sheet, at the original's scale.
    ///
    /// The creatures' hit points and blows in the pack are at the scale of a
    /// twenty point knight, so with a sheet in hand nothing is scaled: a
    /// troll is forty, a swing is four plus the sheet's bonus, and buying
    /// constitution makes the knight harder to kill without making the
    /// troll harder to kill too.
    pub fn set_sheet(&mut self, sheet: Sheet) {
        self.sheet = Some(Sheet {
            max_health: sheet.max_health.max(1),
            ..sheet
        });
        self.reset();
    }

    /// The curse for the next bout. Takes effect on the next reset, as the
    /// original's `[0x930]` is read by `ControlKnight` during the fight.
    pub fn set_player_cursed(&mut self, cursed: bool) {
        if let Some(s) = self.sheet.as_mut() {
            s.cursed = cursed;
        }
        if let Some(f) = self.bout.fighters.first_mut() {
            f.cursed = cursed;
        }
    }

    /// Which knight sits in which seat. Colours follow it, so the knight chosen
    /// on the select screen is the one that walks into the arena.
    pub fn set_roster(&mut self, roster: Vec<usize>) {
        if !roster.is_empty() {
            self.roster = roster;
        }
    }

    /// The knight in a seat, for whoever is drawing a name or a plate.
    pub fn knight_at(&self, seat: usize) -> usize {
        self.roster.get(seat).copied().unwrap_or(seat % 4)
    }

    /// The seat drawn as the first knight, in entries 6 to 8. Seat zero
    /// whenever a knight is in it, which is every bout but the browser's.
    fn main_knight_seat(&self) -> Option<usize> {
        self.bout.fighters.iter().position(|f| f.actor == "knight")
    }

    /// The first knight who is not the main one. He is drawn from the second
    /// knight's banks, in entries 9 to 11, as `Colour2ndKnight` colours him.
    fn second_knight_seat(&self) -> Option<usize> {
        let main = self.main_knight_seat()?;
        self.bout
            .fighters
            .iter()
            .enumerate()
            .position(|(i, f)| i != main && f.actor == "knight")
    }

    /// Whether a seat draws through the second knight's banks. Every knight
    /// but the main one does: the original never fields more than two, and
    /// the palette has room for two, so a third and fourth in the browser's
    /// brawl wear the second's colours.
    fn draws_as_second(&self, seat: usize) -> bool {
        self.is_knight(seat) && self.main_knight_seat() != Some(seat)
    }

    /// The creature whose block is in entries 9 upwards: the first fighter
    /// the tables know, which in every bout the road produces is the one
    /// kind of creature in it.
    fn creature_in_palette(&self) -> Option<&str> {
        self.bout
            .fighters
            .iter()
            .map(|f| f.actor.as_str())
            .find(|a| self.colours.creatures.contains_key(*a))
    }

    /// Who the palette is composed for.
    fn sides(&self) -> Sides<'_> {
        let main = self.main_knight_seat();
        Sides {
            main_knight: main.map_or_else(|| self.knight_at(0), |s| self.knight_at(s)),
            second_knight: self.second_knight_seat().map(|s| self.knight_at(s)),
            creature: self.creature_in_palette(),
            family: self.family(),
        }
    }

    /// `BattlePal` for the bout that is up: the backdrop's own thirty two
    /// colours with the fighters' written over them, in the original's order.
    pub fn battle_palette(&self, backdrop: &[u32]) -> [u32; battle_palette::ENTRIES] {
        let mut base = [0u16; battle_palette::ENTRIES];
        for (slot, c) in base.iter_mut().zip(backdrop) {
            *slot = battle_palette::narrow(*c);
        }
        let pal = self.colours.compose(&base, &self.sides());
        let mut out = [0u32; battle_palette::ENTRIES];
        for (slot, w) in out.iter_mut().zip(pal) {
            *slot = battle_palette::widen(w);
        }
        out
    }

    /// The glow a knight's entries walk towards below ten health, and the
    /// entries: `KnightGlowOn` (0x8f8) installs three `COLOURGLOW`s on 6, 7
    /// and 8 for the main knight, the first every other frame and the other
    /// two every frame, and the same on 9, 10 and 11 for a second knight,
    /// all three every frame. None for a creature, and none for a third or
    /// fourth knight in the browser's brawl: the original has handles for two.
    pub fn knight_glow(&self, seat: usize) -> Vec<henge_assets::Glow> {
        let main = self.main_knight_seat();
        if main != Some(seat) && self.second_knight_seat() != Some(seat) {
            return Vec::new();
        }
        let Some(target) = self.colours.glow_for(self.knight_at(seat)) else {
            return Vec::new();
        };
        let (first, slow) = if self.draws_as_second(seat) {
            (battle_palette::SECOND_AT as u8, 1)
        } else {
            (battle_palette::MAIN_KNIGHT_AT as u8, 2)
        };
        target
            .iter()
            .enumerate()
            .map(|(k, t)| henge_assets::Glow {
                index: first + k as u8,
                target: *t,
                period: if k == 0 { slow } else { 1 },
                repeat: 0,
            })
            .collect()
    }

    /// `KnightGlowOn`'s test: `cmp ax, 0xa; jg` on the knight's health, so
    /// ten or fewer of the original's twenty, dead included, which is why a
    /// fallen knight keeps breathing until the bout is over. Without a sheet
    /// the browser's knight stands at a hundred, and the line moves with him.
    pub fn knight_is_low(&self, seat: usize) -> bool {
        let Some(f) = self.bout.fighters.get(seat) else {
            return false;
        };
        if !self.is_knight(seat) {
            return false;
        }
        let threshold = match self.sheet {
            Some(_) => 10,
            None => 10 * self.actors["knight"].health / ORIGINAL_KNIGHT_HEALTH,
        };
        f.health <= threshold
    }

    pub fn reset(&mut self) {
        // `AddCNT` is a word of BSS nothing resets, so the rotation comes back
        // off the bout that is ending, arrivals it made mid-fight included, and
        // carries on into the one being built.
        self.arrivals = self.bout.arrivals;
        let mut field = self.arena().field();
        // `InitKnightvsDemon` calls `SETDEMONBORD` before it stands anybody
        // up, so the demon's own rectangle is the ground the standing places
        // are measured from as well as the ground the fight is fought on.
        if let Some(g) = self
            .actors
            .get(&self.foe)
            .and_then(|d| d.ground())
            .filter(|g| g.is_sane())
        {
            field.narrow_to(g);
        }
        // **The opening layout is the original's.** Each arrival takes its x and
        // its facing out of its own seat record, through `ActorDef::seat`, and
        // its depth out of `AddKnight`'s rotation: three quarters, one quarter,
        // one half of the way from the deepest border down to row 200, in that
        // order, off a counter nothing ever resets. All three places are usable
        // now that nothing is drawn over the foot of the screen.
        //
        // A knight gets the knight's two records, `SetKnightCombat` at 0x2962
        // for the first and `InitKnightvsKnight` at 0x206b for the second; a
        // creature gets its own table from `first_seat` on. Fighters are built
        // in seat order because that is the order the original stands them up:
        // `SetUpDKL` calls `SetKnightCombat` and then each `InitKnightvs*` walks
        // its table.
        let knight = self.actors["knight"].clone();
        let foe = self
            .actors
            .get(&self.foe)
            .cloned()
            .unwrap_or_else(|| knight.clone());
        // **How many creatures, and how many at once, is the original's.**
        // `InitKnightvs*` writes `MaxMonsters`, `TotalMonsters` and
        // `NumberInCombat`, `AdjustLevel` (0x2824) moves all three by what the
        // knight has become, and `SetMonsterCombat` (0x27e4) then stands
        // `MaxMonsters` of them up and no more. The rest arrive one at a time as
        // the ones in front of you fall; see `henge_core::wave`.
        //
        // A knight has no wave, and neither has a creature the pack gives no
        // counts for: for those the seats are whatever the caller asked for,
        // which is what the arena browser's brawl is.
        let level = self.level();
        let mut wave = if self.foe != "knight" && foe.wave.max > 0 {
            henge_core::wave::Wave::open(&foe.wave, self.heads, &level)
        } else {
            henge_core::wave::Wave::default()
        };
        let humans = self.humans().max(1);
        let n = if wave.max > 0 {
            humans + wave.max as usize
        } else {
            self.control.len().max(2)
        };
        if wave.max > 0 {
            self.control = (0..n)
                .map(|i| {
                    if i < humans {
                        Control::Local(i)
                    } else {
                        Control::Ai
                    }
                })
                .collect();
        }
        let mut fighters: Vec<Fighter> = Vec::with_capacity(n);
        let mut nth: BTreeMap<&str, usize> = BTreeMap::new();
        for i in 0..n {
            // People are knights. The seats the machine fills are whatever the
            // road, the wave, or the browser asked for.
            let creature = matches!(self.control.get(i), Some(Control::Ai)) && self.foe != "knight";
            let (id, def) = if creature {
                (self.foe.as_str(), &foe)
            } else {
                ("knight", &knight)
            };
            let seat = nth.entry(id).or_insert(0);
            // A creature in a wave takes the record `SetMonsterCombat`'s own
            // pointer walk names, or the one `SIDE` names where the fight opens
            // through `INITMO` instead. Everything else keeps `first_seat`: two
            // knights is as many as the original fields, so a third and a fourth
            // in the browser's brawl take the same two records again. The depths
            // differ anyway, because the rotation never repeats inside three.
            let (x, facing) = if creature && wave.max > 0 {
                let record = wave.opening_seat(&foe.wave, *seat);
                // `InitNewMO`'s own `add word [NumberInCombat], 1`, 0x27ee.
                wave.arrived();
                def.seat_at(record)
            } else {
                def.seat(*seat)
            }
            .unwrap_or((GLOBAL.left + 50, 1));
            *seat += 1;
            let y = field.standing_row(self.arrivals.next_place());
            fighters.push(Fighter::new(id, def, x, y, facing));
        }
        // **The dragon's set piece.** `InitKnightvsDragon` does not put a
        // dragon in an ordinary bout: the head goes in at x 80 like any other
        // arrival, and then it builds two more actors of its own, `Claw1TABLE`
        // and `Claw2TABLE`, both at x 5, each of which goes through `AddPlayer`
        // and so through the same rotation. `DragonMoveClaw1` then pins them
        // either side of the head. Nothing else in the game is set up this way.
        if self.foe == "dragon" && self.actors.contains_key("dragon_claw") {
            let claw = self.actors["dragon_claw"].clone();
            fighters.retain(|f| f.actor == "knight" || f.actor == "dragon");
            fighters.truncate(2);
            let head = fighters
                .iter()
                .find(|f| f.actor == "dragon")
                .map_or(0, |f| f.y);
            // `DragonMoveClaw1`: claw one ten rows in front of the head, claw
            // two twenty behind it, and both follow it.
            for (seat, dz) in [10, -20].into_iter().enumerate() {
                let (x, _) = claw.seat(seat).unwrap_or((5, 1));
                let (x, y) = GLOBAL.clamp(x, head + dz);
                let mut f = Fighter::new("dragon_claw", &claw, x, y, 1);
                f.brain.timer = dz;
                self.arrivals.next_place();
                fighters.push(f);
            }
            self.control = (0..fighters.len())
                .map(|i| {
                    if i == 0 {
                        Control::Local(0)
                    } else {
                        Control::Ai
                    }
                })
                .collect();
        }
        self.bout = Bout::new(field, fighters);
        self.bout.bloodless = !self.gore;
        self.bout.wave = wave;
        self.bout.arrivals = self.arrivals;
        // `SETDEMONBORD`: an actor may narrow the ground the fight is fought
        // on, and one does.
        {
            let actors = &self.actors;
            self.bout.apply_actor_borders(|name| &actors[name]);
        }
        // The scale every fight is at. With a sheet it is the original's:
        // the knight's swing is its `*Dam` entry and the creatures, whose
        // hit points and blows are at the scale of a twenty point knight,
        // are left as they are. Without one the arena browser's hundred
        // point knight stands and everything is moved by the same ratio, so
        // a troll is as many swings deep at a hundred as it is at twenty.
        let scale_to = match self.sheet {
            Some(_) => ORIGINAL_KNIGHT_HEALTH,
            None => knight.health.max(1),
        };
        // What each creature in the bout is worth on tonight's moon, read off
        // the definitions before the fighters are taken mutably.
        let moon: std::collections::BTreeMap<String, (i32, i32)> = self
            .bout
            .fighters
            .iter()
            .map(|f| {
                (
                    f.actor.clone(),
                    self.def_of(&f.actor).under_moon(&self.moon),
                )
            })
            .collect();
        if self.sheet.is_some() {
            self.bout.damage = knight
                .attacks
                .get(&knight.attack)
                .map_or(4, |a| a.damage)
                .max(1);
        }
        for (i, f) in self.bout.fighters.iter_mut().enumerate() {
            if f.actor == "knight" {
                // Seat zero fights on the run's sheet; every other knight is
                // fresh off `SetKnightEquipment`, twenty health and one of
                // strength, because nothing tracks what they have been through.
                let (max, bonus) = match self.sheet {
                    Some(s) if i == 0 => (s.max_health, s.bonus),
                    Some(s) => (ORIGINAL_KNIGHT_HEALTH, s.fresh_bonus),
                    None => (knight.health, 0),
                };
                f.max_health = max;
                f.health = max;
                f.bonus = bonus;
                // `SetKnightEquipment`: ten daggers on the belt.
                f.record.set(field::DAGGERS, 10);
            } else {
                let scale = |v: i32| (v * scale_to / ORIGINAL_KNIGHT_HEALTH).max(1);
                // What the moon makes of it, before anything is scaled: a
                // ratman is five points and a slash of one most nights, seven
                // and three on the full moon and twelve and five on the new.
                let (health, damage) = moon
                    .get(&f.actor)
                    .copied()
                    .unwrap_or((f.max_health, f.damage));
                f.max_health = scale(health);
                f.health = f.max_health;
                f.damage = scale(damage);
                f.record.set_health(f.health);
            }
        }
        if let (Some(s), Some(f)) = (self.sheet, self.bout.fighters.first_mut()) {
            f.cursed = s.cursed;
        }
        if let (Some(h), Some(f)) = (self.player_health, self.bout.fighters.first_mut()) {
            f.health = h.clamp(1, f.max_health);
        }
        if let (Some(d), Some(f)) = (self.player_daggers, self.bout.fighters.first_mut()) {
            f.record.set(field::DAGGERS, d as i32);
        }
        // What a creature arriving later is fielded with. `InitNewMO` calls the
        // same `INITANIM` for every one of them, so a reinforcement is the same
        // creature as the one it replaces; the numbers are read back off a
        // fighter already standing rather than worked out twice.
        if self.bout.wave.max > 0 {
            let foe = self.foe.clone();
            if let Some(f) = self.bout.fighters.iter().find(|f| f.actor == foe) {
                let (health, damage) = (f.max_health, f.damage);
                self.bout.wave.health = health;
                self.bout.wave.damage = damage;
            }
        }
        self.events.clear();
    }

    pub fn settled_for(&self) -> u32 {
        self.bout.settled_for
    }

    /// What the fallen were carrying, for whoever is left standing.
    ///
    /// Every fighter but the player's own seat is somebody else's, and each is
    /// worth the bounty its actor definition names, so what a fight pays is a
    /// property of who you fought and lives in the pack. Seat zero is excluded
    /// because a man does not loot himself.
    pub fn purse(&self) -> u32 {
        self.bout
            .fighters
            .iter()
            .enumerate()
            .filter(|(i, f)| *i != 0 && !f.alive())
            .map(|(_, f)| self.def_of(&f.actor).bounty)
            .sum()
    }

    /// What the fallen were worth in experience, the same way.
    pub fn experience(&self) -> u32 {
        self.bout
            .fighters
            .iter()
            .enumerate()
            .filter(|(i, f)| *i != 0 && !f.alive())
            .map(|(_, f)| self.def_of(&f.actor).experience)
            .sum()
    }

    pub fn arena(&self) -> &ArenaData {
        &self.arenas[&self.order[self.index]]
    }
    pub fn name(&self) -> &str {
        &self.order[self.index]
    }
    /// How many arenas a family's rotation has in it. Eight, in every family
    /// the original ships; asked for rather than assumed so a pack can differ.
    pub fn rotation_len(&self, family: &str) -> usize {
        self.families.get(family).map_or(0, |f| f.arenas.len())
    }

    /// Pick an arena belonging to a family, so an encounter in the swamp is
    /// fought in a swamp.
    ///
    /// `pick` is the family's own turn counter, kept on the run, because the
    /// original chooses by rotation and not by chance: `Table[counter]`, then
    /// `inc counter` and `and counter, 7`. The eight arenas of a family come
    /// round in order, and the six lair layouts, which are not in that table,
    /// never come up on the road at all.
    pub fn set_family(&mut self, family: &str, pick: usize) {
        let names = self
            .families
            .get(family)
            .map(|f| f.arenas.clone())
            .unwrap_or_default();
        let wanted = names
            .get(pick % names.len().max(1))
            .and_then(|n| self.order.iter().position(|o| o == n));
        // A family with no rotation in the pack still has to put the fight
        // somewhere, so fall back to any arena that claims the family.
        let fallback = || {
            self.order
                .iter()
                .position(|n| self.arenas[n].family == family)
        };
        if let Some(i) = wanted.or_else(fallback) {
            self.index = i;
        }
        self.reset();
    }

    /// Put the fight in one named arena, whatever family it belongs to.
    ///
    /// The lair layouts are not in any family's rotation, so a raid cannot ask
    /// for one by turn counter the way the road does. It names it instead.
    pub fn set_arena(&mut self, name: &str) -> bool {
        let Some(i) = self.order.iter().position(|o| o == name) else {
            return false;
        };
        self.index = i;
        self.reset();
        true
    }

    pub fn family(&self) -> &str {
        &self.arena().family
    }

    pub fn step_arena(&mut self, delta: i32) {
        let n = self.order.len() as i32;
        self.index = (((self.index as i32 + delta) % n + n) % n) as usize;
        self.reset();
    }

    /// One tick. `local` holds the intents of whoever is at the keyboard, in
    /// seat order; opponents are filled in here. The bout itself cannot tell
    /// which is which.
    pub fn update(&mut self, local: &[Intent]) {
        let mut intents = vec![Intent::default(); self.bout.fighters.len()];

        for (i, intent) in intents.iter_mut().enumerate() {
            // A creature the wave walked in mid-fight has no seat in the control
            // list, because the list was fixed when the bout was built. It is
            // the machine's, like every other creature: `FindTABLE` hands
            // `InitNewMO` a free actor slot and `CONTROLTABLE` decides what runs
            // it off its kind, not off where it sits.
            match self.control.get(i).copied().or(Some(Control::Ai)) {
                Some(Control::Local(slot)) => {
                    *intent = local.get(slot).copied().unwrap_or_default();
                }
                Some(Control::Ai) => {
                    // Somebody standing, or failing that a body still worth
                    // one more blow, which is the finisher with the gore on.
                    // The creature's own controller does the rest, inside the
                    // simulation, where the state it keeps belongs.
                    let actors = &self.actors;
                    // Every one of the original's controllers opens by
                    // writing `KnightTable` into `Opponent`: a creature
                    // fights the knight and nothing else, which is also what
                    // keeps the dragon from taking its own claws for a foe.
                    let target = self
                        .opposed(i)
                        .or_else(|| self.bout.nearest_foe(i))
                        .or_else(|| self.bout.nearest_body(i, |name| &actors[name]));
                    if let Some(target) = target {
                        let gore = self.gore;
                        // The bout needs a definition per actor and a mutable
                        // hold on itself at the same time, and both live on
                        // this struct. Lending the definitions out and taking
                        // them back is the cheap way to say that; the map is
                        // moved, not cloned.
                        let actors = std::mem::take(&mut self.actors);
                        *intent = self
                            .bout
                            .monster_intent(i, target, |name| &actors[name], gore);
                        self.actors = actors;
                    }
                }
                None => {}
            }
        }
        let actors = &self.actors;
        self.events = self.bout.step_with(|name| &actors[name], &intents);
    }

    pub fn render(&mut self, reg: &mut Registry, fb: &mut Framebuffer) -> anyhow::Result<()> {
        let family_name = self.arena().family.clone();
        let family = self
            .families
            .get(&family_name)
            .ok_or_else(|| anyhow::anyhow!("unknown arena family {family_name}"))?;
        let (sheet_id, backdrop_id) = (family.sheet.clone(), family.backdrop.clone());
        // Which sheet each placement draws from. Recovered: the byte is 4 for
        // the shared `FO2` sheet and the family's own sheet otherwise, and
        // compositing an arena either way shows only that reading makes a
        // coherent picture.
        let tiles = family.tiles.clone();

        // The backdrop's own palette is the base, and `BattlePal` is that with
        // the fighters written over it: the knight in 6 to 8, the creature or
        // the second knight from 9, the ground, black at 0 and red at 15.
        if let Some(p) = reg
            .palette(&format!("palette.{backdrop_id}"))
            .map(|r| r.value.clone())
        {
            self.palette = self.battle_palette(&p);
            fb.set_palette(&self.palette);
        }
        match reg.image(&backdrop_id) {
            Ok(img) if img.width == 320 && img.height == 200 => {
                fb.pixels.copy_from_slice(&img.pixels)
            }
            _ => fb.clear(0),
        }

        enum Item<'a> {
            Prop(&'a henge_core::arena::Prop),
            Fighter(usize),
            Missile(usize),
        }
        let props: Vec<henge_core::arena::Prop> = self.arena().terrain.placements.clone();
        let mut items: Vec<(i32, Item)> = props
            .iter()
            .map(|p| (p.y as i32 + CELL_H as i32, Item::Prop(p)))
            .collect();
        for (i, f) in self.bout.fighters.iter().enumerate() {
            items.push((f.depth(), Item::Fighter(i)));
        }
        // A dagger in flight sorts by the feet it left from, a spray of blood
        // just in front of the body it came off.
        for (k, m) in self.bout.missiles.iter().enumerate() {
            items.push((m.depth + 1, Item::Missile(k)));
        }
        items.sort_by_key(|(d, _)| *d);

        for (_, item) in items {
            match item {
                Item::Prop(p) => {
                    let sheet = tiles.get(&p.sheet).unwrap_or(&sheet_id).clone();
                    self.draw_prop(reg, fb, &sheet, p)
                }
                Item::Fighter(i) => self.draw_fighter(reg, fb, i)?,
                Item::Missile(k) => {
                    let m = &self.bout.missiles[k];
                    // A dagger comes out of the banks of whoever threw it,
                    // so a second knight's is in his table; the blood is the
                    // blood bank's own.
                    let banks = if m.attack.is_some() && self.draws_as_second(m.owner) {
                        self.second_banks.as_deref()
                    } else {
                        None
                    };
                    let def = self.def_of(&m.actor);
                    Self::draw_parts(reg, fb, def, &m.task, banks)?;
                }
            }
        }
        Ok(())
    }

    fn draw_prop(
        &self,
        reg: &mut Registry,
        fb: &mut Framebuffer,
        sheet: &str,
        p: &henge_core::arena::Prop,
    ) {
        let Ok(img) = reg.image(sheet) else { return };
        let per_row = img.width / CELL_W;
        if per_row == 0 {
            return;
        }
        let (sx, sy) = (
            (p.cell as usize % per_row) * CELL_W,
            (p.cell as usize / per_row) * CELL_H,
        );
        let mut cell = vec![0u8; CELL_W * CELL_H];
        for row in 0..CELL_H {
            let src = (sy + row) * img.width + sx;
            if src + CELL_W <= img.pixels.len() {
                cell[row * CELL_W..(row + 1) * CELL_W]
                    .copy_from_slice(&img.pixels[src..src + CELL_W]);
            }
        }
        fb.blit(&cell, CELL_W, CELL_H, p.x as i32, p.y as i32, false);
    }

    fn draw_fighter(
        &self,
        reg: &mut Registry,
        fb: &mut Framebuffer,
        index: usize,
    ) -> anyhow::Result<()> {
        let f = &self.bout.fighters[index];
        let def = self.def_at(index);
        if def.scripted() {
            return self.draw_task(reg, fb, index);
        }
        let Some(seq) = f.sequence(def) else {
            return Ok(());
        };
        let Some(frame) = f.player.current(seq) else {
            return Ok(());
        };
        let Some(rect) = reg
            .sheet(&def.sheet)
            .and_then(|r| r.value.frames.get(frame.sprite as usize).copied())
        else {
            return Ok(());
        };

        let img = reg.image(&def.sheet)?;
        let (w, h) = (rect.w as usize, rect.h as usize);
        let mut px = vec![0u8; w * h];
        for row in 0..h {
            let src = (rect.y as usize + row) * img.width + rect.x as usize;
            if src + w <= img.pixels.len() {
                px[row * w..(row + 1) * w].copy_from_slice(&img.pixels[src..src + w]);
            }
        }
        let flip = f.facing < 0;
        // ox/oy anchor the frame to the feet; mirroring has to mirror the anchor.
        let ox = if flip { -(rect.ox + w as i32) } else { rect.ox };
        let x = f.x + ox + frame.offset_x as i32;
        let y = f.y + rect.oy + frame.offset_y as i32;
        fb.blit(&px, w, h, x, y, flip);
        Ok(())
    }

    /// A fighter animated by the task VM: several cels a frame, each named by a
    /// bank slot and placed by the interpreter, drawn in script order so a
    /// later part covers an earlier one.
    ///
    /// The simulation decided all of this. Nothing here chooses a frame or a
    /// position; it resolves a bank slot to a sheet and blits, with the mirror
    /// term the original's `TASKLEFT` applies.
    fn draw_task(
        &self,
        reg: &mut Registry,
        fb: &mut Framebuffer,
        index: usize,
    ) -> anyhow::Result<()> {
        let f = &self.bout.fighters[index];
        let def = self.def_at(index);
        let Some(task) = f.task.as_ref() else {
            return Ok(());
        };
        // Everyone is drawn in their own pixel indices, and the palette says
        // what those are this bout: the knight's 6 to 8 hold his colours, a
        // creature's 9 upwards hold its block for this ground, which is how
        // the original got a swamp trogg and a forest trogg from one sheet.
        // A second knight is the one figure drawn from other banks, the
        // `HE*.OB` set painted in 9 to 11.
        let banks = if self.draws_as_second(index) {
            self.second_banks.as_deref()
        } else {
            None
        };
        Self::draw_parts(reg, fb, def, task, banks)
    }

    /// The parts of one task, whoever's it is. A task that has killed itself
    /// draws nothing, as in the original, where it is no longer in the table.
    ///
    /// `banks`, when given, stands in for the actor's own starting table, the
    /// way the creature table stands in for the knight's when a second knight
    /// is loaded into it.
    fn draw_parts(
        reg: &mut Registry,
        fb: &mut Framebuffer,
        def: &ActorDef,
        task: &Task,
        banks: Option<&[Bank]>,
    ) -> anyhow::Result<()> {
        if !task.active {
            return Ok(());
        }
        let at = (task.x, task.y, task.z);
        for part in &task.shown {
            let swapped = banks
                .filter(|_| part.table == def.bank_table)
                .and_then(|b| b.get(part.bank as usize))
                .filter(|b| !b.cels.is_empty());
            let Some(bank) = swapped.or_else(|| def.bank(part.table, part.bank)) else {
                continue;
            };
            let Some(placed) = henge_core::taskvm::place(part, bank, at, task.mirror()) else {
                continue;
            };
            let Some(rect) = reg
                .sheet(&bank.sheet)
                .and_then(|r| r.value.frames.get(placed.frame as usize).copied())
            else {
                continue;
            };
            let img = reg.image(&bank.sheet)?;
            let (w, h) = (rect.w as usize, rect.h as usize);
            let mut px = vec![0u8; w * h];
            for row in 0..h {
                let src = (rect.y as usize + row) * img.width + rect.x as usize;
                if src + w <= img.pixels.len() {
                    px[row * w..(row + 1) * w].copy_from_slice(&img.pixels[src..src + w]);
                }
            }
            fb.blit(&px, w, h, placed.x, placed.y, placed.mirror);
        }
        Ok(())
    }
}
