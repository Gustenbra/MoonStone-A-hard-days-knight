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
use henge_core::content::{ActorData, ArenaData, Arenas, Families};

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
        };
        // One person by default. Two would leave the second knight controlled by
        // a keyboard nobody is pressing: it never attacks, never closes, and a
        // bout with it in can never settle. Press 2 to take that seat.
        w.set_players(1);
        Ok(w)
    }

    fn def(&self) -> &henge_core::content::ActorDef {
        &self.actors["knight"]
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
        let def = self.actors["knight"].clone();
        let n = self.control.len().max(2) as i32;
        let span = b.right - b.left - 100;
        let fighters = (0..n)
            .map(|i| {
                // Spread them across the arena, alternating which way they face
                // so nobody starts with their back to the fight.
                let x = b.left + 50 + span * i / (n - 1).max(1);
                let y = ground - (i % 2) * 10;
                Fighter::new("knight", &def, x, y, if i % 2 == 0 { 1 } else { -1 })
            })
            .collect();
        self.bout = Bout::new(b, fighters);
        if let Some(max) = self.sheet_health {
            let base = def.health.max(1);
            for f in self.bout.fighters.iter_mut() {
                f.max_health = max;
                f.health = max;
            }
            self.bout.damage = (self.bout.damage * max / base).max(1);
        }
        if let (Some(h), Some(f)) = (self.player_health, self.bout.fighters.first_mut()) {
            f.health = h.clamp(1, f.max_health);
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
        let bounty = self.def().bounty;
        self.bout
            .fighters
            .iter()
            .enumerate()
            .filter(|(i, f)| *i != 0 && !f.alive())
            .map(|_| bounty)
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
        let def = self.actors["knight"].clone();
        let mut intents = vec![Intent::default(); self.bout.fighters.len()];

        for i in 0..self.bout.fighters.len() {
            match self.control.get(i).copied() {
                Some(Control::Local(slot)) => {
                    intents[i] = local.get(slot).copied().unwrap_or_default();
                }
                Some(Control::Ai { .. }) => {
                    if let Some(target) = self.bout.nearest_foe(i) {
                        let (me, foe) = (self.bout.fighters[i].clone(), self.bout.fighters[target].clone());
                        if let Some(Control::Ai { cooldown, clock }) = self.control.get_mut(i) {
                            *clock += 1;
                            intents[i] = simple_ai(&me, &foe, &def, cooldown, *clock);
                        }
                    }
                }
                None => {}
            }
        }
        self.events = self.bout.step(&def, &intents);
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

        enum Item<'a> { Prop(&'a henge_core::arena::Prop), Fighter(usize) }
        let props: Vec<henge_core::arena::Prop> = self.arena().terrain.placements.clone();
        let mut items: Vec<(i32, Item)> = props
            .iter()
            .map(|p| (p.y as i32 + CELL_H as i32, Item::Prop(p)))
            .collect();
        for (i, f) in self.bout.fighters.iter().enumerate() {
            items.push((f.depth(), Item::Fighter(i)));
        }
        items.sort_by_key(|(d, _)| *d);

        for (_, item) in items {
            match item {
                Item::Prop(p) => {
                    let sheet = tiles.get(&p.sheet).unwrap_or(&sheet_id).clone();
                    self.draw_prop(reg, fb, &sheet, p)
                }
                Item::Fighter(i) => self.draw_fighter(reg, fb, i)?,
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
        let def = self.def();
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

}

impl FighterExt for Fighter {}
pub trait FighterExt {}
