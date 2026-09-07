//! The arena: backdrop, scenery and two fighters, drawn in one depth-sorted pass
//! and simulated by `henge_core::combat`.
//!
//! Nothing here knows where the pixels came from, so this file is unchanged when
//! the reference art is replaced by our own.

use crate::framebuffer::Framebuffer;
use henge_assets::{player_colours, player_luts, recolour, Lut, Registry};
use henge_core::arena::Bounds;
use henge_core::bout::{Bout, HitEvent};
use henge_core::combat::{simple_ai, Fighter, Intent};
use henge_core::content::{ActorData, ActorDef, ArenaData, Arenas, Families, ORIGINAL_KNIGHT_HEALTH};
use henge_core::taskvm::{field, Task};

const CELL_W: usize = 32;
const CELL_H: usize = 25;

/// Where a fighter's intent comes from. The bout cannot tell them apart, which
/// is the point: a network peer slots in here later without touching combat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    /// Reads a set of keys on this machine. Moonstone was a couch game.
    Local(usize),
    Ai { cooldown: i32, clock: i32 },
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
    /// One colour substitution per seat, rebuilt when the arena changes because
    /// each arena brings its own palette.
    luts: [Lut; 4],
    lut_arena: Option<usize>,
    /// A representative colour per seat, for status bars.
    seat_colours: [u8; 4],
    /// What the player brings into the next bout. Wounds carry between fights,
    /// so this is not always full.
    player_health: Option<i32>,
    /// The health every knight in the arena is worth, once a run has chosen one.
    ///
    /// A knight's health is `10 * constitution + armour + 10`, so a new one is
    /// worth twenty, and the arenas here are authored against a hundred. Both
    /// scales take four blows to settle a fight, so adopting the recovered one
    /// means moving the damage with it and nothing else.
    sheet_health: Option<i32>,
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
}

impl World {
    pub fn load(reg: &Registry) -> anyhow::Result<World> {
        let arenas: Arenas = reg.read_data("data.arenas")?;
        let families: Families = reg.read_data("data.families")?;
        let actors: ActorData = reg.read_data("data.actors")?;
        let mut order: Vec<String> = arenas.keys().cloned().collect();
        order.sort();
        anyhow::ensure!(!order.is_empty(), "no arenas in the pack");
        anyhow::ensure!(actors.contains_key("knight"), "no knight definition in the pack");

        let bounds = arenas[&order[0]].bounds();
        // Everyone the pack can field, the knight first so cycling from the
        // default goes straight to the creatures.
        let mut bestiary: Vec<String> = vec!["knight".into()];
        bestiary.extend(actors.keys().filter(|k| k.as_str() != "knight").cloned());
        let mut w = World {
            arenas, families, actors, order, index: 0,
            bout: Bout::new(bounds, Vec::new()),
            control: Vec::new(),
            events: Vec::new(),
            luts: [henge_assets::recolour::IDENTITY; 4],
            lut_arena: None,
            seat_colours: [1; 4],
            player_health: None,
            sheet_health: None,
            roster: (0..4).collect(),
            foe: "knight".into(),
            bestiary,
            gore: true,
            player_daggers: None,
        };
        // One person by default. Two would leave the second knight controlled by
        // a keyboard nobody is pressing: it never attacks, never closes, and a
        // bout with it in can never settle. Press 2 to take that seat.
        w.set_players(1);
        Ok(w)
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
        self.bout.fighters.get(seat).map_or(true, |f| f.actor == "knight")
    }

    /// What a seat is called: the creature's name from its definition, or
    /// nothing for a knight, whose name is the roster's business.
    pub fn creature_name(&self, seat: usize) -> Option<String> {
        let f = self.bout.fighters.get(seat)?;
        if f.actor == "knight" {
            return None;
        }
        Some(self.def_of(&f.actor).display_name(&f.actor).to_string())
    }

    /// Field a particular actor as the opponent. An unknown id is refused and
    /// said so, rather than silently fielding a knight.
    pub fn set_foe(&mut self, actor: &str) -> bool {
        if !self.actors.contains_key(actor) {
            let known: Vec<&str> = self.bestiary.iter().map(String::as_str).collect();
            eprintln!("no actor called {actor}. The pack has: {}", known.join(", "));
            return false;
        }
        self.foe = actor.to_string();
        self.reset();
        true
    }

    /// The next or previous creature in the pack, for the arena browser.
    pub fn step_foe(&mut self, delta: i32) {
        let n = self.bestiary.len() as i32;
        if n == 0 {
            return;
        }
        let at = self.bestiary.iter().position(|b| *b == self.foe).unwrap_or(0) as i32;
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
            .map(|i| if i < humans { Control::Local(i) } else { Control::Ai { cooldown: 20 + i as i32 * 17, clock: i as i32 * 37 } })
            .collect();
        self.reset();
    }

    pub fn humans(&self) -> usize {
        self.control.iter().filter(|c| matches!(c, Control::Local(_))).count()
    }

    /// Set what the player enters the next bout with. Opponents are always
    /// fresh; the player is whatever the run has left them.
    pub fn set_player_health(&mut self, health: i32) {
        self.player_health = Some(health);
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
        self.bout.fighters.get(seat).map_or(0, |f| f.record.get(field::DAGGERS).max(0) as u32)
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

    /// Fight at the chosen knight's scale rather than the arena's.
    ///
    /// Everyone in an arena is a knight, so one number does for all four. The
    /// blow is moved by the same ratio, which keeps a bout the same number of
    /// swings long as it was: without that, a knight worth twenty health would
    /// be cut down by a single hit authored against a hundred.
    pub fn set_sheet_health(&mut self, max_health: i32) {
        self.sheet_health = Some(max_health.max(1));
        self.reset();
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

    /// The colour that seat's knight wears in the loaded palette.
    pub fn seat_colour(&self, seat: usize) -> u8 {
        self.seat_colours[self.knight_at(seat) % 4]
    }

    pub fn reset(&mut self) {
        let b = self.bounds();
        // Near the front of the walkable band: the band runs from the horizon
        // down, so its bottom edge is the ground closest to the viewer.
        let ground = b.bottom - 8;
        let knight = self.actors["knight"].clone();
        let foe = self.actors.get(&self.foe).cloned().unwrap_or_else(|| knight.clone());
        let n = self.control.len().max(2) as i32;
        let span = b.right - b.left - 100;
        let fighters = (0..n)
            .map(|i| {
                // Spread them across the arena, alternating which way they face
                // so nobody starts with their back to the fight.
                let x = b.left + 50 + span * i / (n - 1).max(1);
                let y = ground - (i % 2) * 10;
                let facing = if i % 2 == 0 { 1 } else { -1 };
                // People are knights. The seats the machine fills are whatever
                // the road, or the browser, asked for.
                match self.control.get(i as usize) {
                    Some(Control::Ai { .. }) if self.foe != "knight" => {
                        Fighter::new(self.foe.as_str(), &foe, x, y, facing)
                    }
                    _ => Fighter::new("knight", &knight, x, y, facing),
                }
            })
            .collect();
        self.bout = Bout::new(b, fighters);
        self.bout.bloodless = !self.gore;
        // The scale every fight is at: a knight's health, and the blow that
        // takes a quarter of it. Every knight in an arena is worth the same,
        // so one number does for all of them, and the creatures, whose hit
        // points and blows are at the original's scale of a twenty-point
        // knight, are moved by the same ratio so a troll is as many swings
        // deep at a hundred as it is at twenty.
        let knight_max = self.sheet_health.unwrap_or(knight.health).max(1);
        if let Some(max) = self.sheet_health {
            let base = knight.health.max(1);
            self.bout.damage = (self.bout.damage * max / base).max(1);
        }
        for f in self.bout.fighters.iter_mut() {
            if f.actor == "knight" {
                f.max_health = knight_max;
                f.health = knight_max;
                // `SetKnightEquipment`: ten daggers on the belt.
                f.record.set(field::DAGGERS, 10);
            } else {
                let scale = |v: i32| (v * knight_max / ORIGINAL_KNIGHT_HEALTH).max(1);
                f.max_health = scale(f.max_health);
                f.health = f.max_health;
                f.damage = scale(f.damage);
                f.record.set_health(f.health);
            }
        }
        if let (Some(h), Some(f)) = (self.player_health, self.bout.fighters.first_mut()) {
            f.health = h.clamp(1, f.max_health);
        }
        if let (Some(d), Some(f)) = (self.player_daggers, self.bout.fighters.first_mut()) {
            f.record.set(field::DAGGERS, d as i32);
        }
        for c in self.control.iter_mut() {
            if let Control::Ai { cooldown, .. } = c {
                *cooldown = 20;
            }
        }
        self.events.clear();
    }

    pub fn settled_for(&self) -> u32 { self.bout.settled_for }

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

    /// How many visibly different knights this arena's palette can support.
    pub fn distinct_players(&self, palette: &[u32]) -> usize {
        recolour::max_distinct_players(palette)
    }

    pub fn arena(&self) -> &ArenaData { &self.arenas[&self.order[self.index]] }
    pub fn name(&self) -> &str { &self.order[self.index] }
    pub fn bounds(&self) -> Bounds { self.arena().bounds() }

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
        let names = self.families.get(family).map(|f| f.arenas.clone()).unwrap_or_default();
        let wanted = names
            .get(pick % names.len().max(1))
            .and_then(|n| self.order.iter().position(|o| o == n));
        // A family with no rotation in the pack still has to put the fight
        // somewhere, so fall back to any arena that claims the family.
        let fallback = || {
            self.order.iter().position(|n| self.arenas[n].family == family)
        };
        if let Some(i) = wanted.or_else(fallback) {
            self.index = i;
        }
        self.reset();
    }

    pub fn family(&self) -> &str { &self.arena().family }

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

        for i in 0..self.bout.fighters.len() {
            match self.control.get(i).copied() {
                Some(Control::Local(slot)) => {
                    intents[i] = local.get(slot).copied().unwrap_or_default();
                }
                Some(Control::Ai { .. }) => {
                    // Somebody standing, or failing that a body still worth
                    // one more blow, which is the finisher with the gore on.
                    let actors = &self.actors;
                    let target = self
                        .bout
                        .nearest_foe(i)
                        .or_else(|| self.bout.nearest_body(i, |name| &actors[name]));
                    if let Some(target) = target {
                        let (me, foe) = (self.bout.fighters[i].clone(), self.bout.fighters[target].clone());
                        // The opponent judges spacing by its own reach, so a
                        // troll swings from where a troll's club lands.
                        let def = self.def_of(&me.actor).clone();
                        let gore = self.gore;
                        if let Some(Control::Ai { cooldown, clock }) = self.control.get_mut(i) {
                            *clock += 1;
                            intents[i] = simple_ai(&me, &foe, &def, cooldown, *clock, gore);
                        }
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

        // The backdrop owns the palette everything else is drawn in, which is how
        // the original recoloured the same creature per region for free.
        if let Some(p) = reg.palette(&format!("palette.{backdrop_id}")).map(|r| r.value.clone()) {
            fb.set_palette(&p);
            self.refresh_luts(&p);
        }
        match reg.image(&backdrop_id) {
            Ok(img) if img.width == 320 && img.height == 200 => {
                fb.pixels.copy_from_slice(&img.pixels)
            }
            _ => fb.clear(0),
        }

        enum Item<'a> { Prop(&'a henge_core::arena::Prop), Fighter(usize), Missile(usize) }
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
                    // The knight's own dagger is in his colours; the blood is
                    // the blood bank's own.
                    let lut = if m.attack.is_some() && self.is_knight(m.owner) {
                        self.luts[self.knight_at(m.owner) % 4]
                    } else {
                        henge_assets::recolour::IDENTITY
                    };
                    let def = self.def_of(&m.actor);
                    Self::draw_parts(reg, fb, def, &m.task, &lut)?;
                }
            }
        }
        Ok(())
    }

    fn draw_prop(&self, reg: &mut Registry, fb: &mut Framebuffer, sheet: &str,
                 p: &henge_core::arena::Prop) {
        let Ok(img) = reg.image(sheet) else { return };
        let per_row = img.width / CELL_W;
        if per_row == 0 { return; }
        let (sx, sy) = ((p.cell as usize % per_row) * CELL_W,
                        (p.cell as usize / per_row) * CELL_H);
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

    /// Rebuild the seat colours when the arena, and therefore the palette,
    /// changes. Recomputing every frame would be wasteful and pointless.
    pub fn refresh_luts(&mut self, palette: &[u32]) {
        if self.lut_arena == Some(self.index) {
            return;
        }
        self.luts = player_luts(palette);
        self.seat_colours = player_colours(palette);
        self.lut_arena = Some(self.index);
    }

    fn draw_fighter(&self, reg: &mut Registry, fb: &mut Framebuffer, index: usize)
        -> anyhow::Result<()> {
        let f = &self.bout.fighters[index];
        let def = self.def_at(index);
        if def.scripted() {
            return self.draw_task(reg, fb, index);
        }
        let Some(seq) = f.sequence(def) else { return Ok(()) };
        let Some(frame) = f.player.current(seq) else { return Ok(()) };
        let Some(rect) = reg
            .sheet(&def.sheet)
            .and_then(|r| r.value.frames.get(frame.sprite as usize).copied())
        else { return Ok(()) };

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
        fb.blit_lut(&px, w, h, x, y, flip, &self.luts[self.knight_at(index) % 4]);
        Ok(())
    }

    /// A fighter animated by the task VM: several cels a frame, each named by a
    /// bank slot and placed by the interpreter, drawn in script order so a
    /// later part covers an earlier one.
    ///
    /// The simulation decided all of this. Nothing here chooses a frame or a
    /// position; it resolves a bank slot to a sheet and blits, with the mirror
    /// term the original's `TASKLEFT` applies.
    fn draw_task(&self, reg: &mut Registry, fb: &mut Framebuffer, index: usize)
        -> anyhow::Result<()> {
        let f = &self.bout.fighters[index];
        let def = self.def_at(index);
        let Some(task) = f.task.as_ref() else { return Ok(()) };
        // A knight wears his seat's colours. A creature is drawn as its sheet
        // has it: the arena's palette already recolours it per region, which
        // is how the original got a swamp trogg and a forest trogg for free.
        let lut = if self.is_knight(index) {
            self.luts[self.knight_at(index) % 4]
        } else {
            henge_assets::recolour::IDENTITY
        };
        Self::draw_parts(reg, fb, def, task, &lut)
    }

    /// The parts of one task, whoever's it is. A task that has killed itself
    /// draws nothing, as in the original, where it is no longer in the table.
    fn draw_parts(reg: &mut Registry, fb: &mut Framebuffer, def: &ActorDef, task: &Task, lut: &Lut)
        -> anyhow::Result<()> {
        if !task.active {
            return Ok(());
        }
        let at = (task.x, task.y, task.z);
        for part in &task.shown {
            let Some(bank) = def.bank(part.table, part.bank) else { continue };
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
            fb.blit_lut(&px, w, h, placed.x, placed.y, placed.mirror, lut);
        }
        Ok(())
    }
}

impl FighterExt for Fighter {}
pub trait FighterExt {}
