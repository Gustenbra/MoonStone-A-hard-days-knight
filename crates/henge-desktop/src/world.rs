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
    Ai { cooldown: i32 },
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
    pub fn set_players(&mut self, humans: usize) {
        let humans = humans.clamp(1, 4);
        self.control = (0..4)
            .map(|i| if i < humans { Control::Local(i) } else { Control::Ai { cooldown: 20 + i as i32 * 17 } })
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
        if let (Some(h), Some(f)) = (self.player_health, self.bout.fighters.first_mut()) {
            f.health = h.clamp(1, f.max_health);
        }
        for c in self.control.iter_mut() {
            if let Control::Ai { cooldown } = c {
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

    /// Pick an arena belonging to a family, so an encounter in the swamp is
    /// fought in a swamp. Falls back to any arena if the family is unknown.
    pub fn set_family(&mut self, family: &str, pick: u32) {
        let matching: Vec<usize> = self
            .order
            .iter()
            .enumerate()
            .filter(|(_, name)| self.arenas[*name].family == family)
            .map(|(i, _)| i)
            .collect();
        if !matching.is_empty() {
            self.index = matching[pick as usize % matching.len()];
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
                        if let Some(Control::Ai { cooldown }) = self.control.get_mut(i) {
                            intents[i] = simple_ai(&me, &foe, &def, cooldown);
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
                Item::Prop(p) => self.draw_prop(reg, fb, &sheet_id, p),
                Item::Fighter(i) => self.draw_fighter(reg, fb, i)?,
            }
        }
        self.draw_health(fb);
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
        fb.blit_lut(&px, w, h, x, y, flip, &self.luts[index % 4]);
        Ok(())
    }

    /// The colour to fill this seat's status bar with: the hue that seat's
    /// knight wears, taken from the same ranked hues the recolour uses.
    fn seat_status_shade(&self, index: usize) -> u8 {
        self.seat_colours[index % 4]
    }

    /// Whichever palette extreme stands out most against a given colour, so a
    /// bar is readable whatever the arena and whatever the knight.
    fn contrast_with(palette: &[u32], colour: u8) -> u8 {
        let l = |i: usize| palette.get(i).map_or(0i32, |c| {
            (((c >> 16 & 0xff) * 2 + (c >> 8 & 0xff) * 3 + (c & 0xff)) / 6) as i32
        });
        let n = palette.len().min(32);
        let (mut dark, mut light) = (1usize, 1usize);
        for i in 1..n {
            if l(i) < l(dark) { dark = i; }
            if l(i) > l(light) { light = i; }
        }
        let c = l(colour as usize);
        if (c - l(dark)).abs() >= (c - l(light)).abs() { dark as u8 } else { light as u8 }
    }

    /// Arena palettes have no fixed slots, so pick the darkest and brightest
    /// entries at draw time. That way the bars read on every backdrop instead of
    /// turning pink in one arena and vanishing in the next.
    /// One bar per fighter, each tinted to match the marker above its knight.
    fn draw_health(&self, fb: &mut Framebuffer) {
        let palette: Vec<u32> = fb.palette.to_vec();
        let n = self.bout.fighters.len().max(1) as i32;
        let w = (312 / n - 6).clamp(20, 82);
        for (i, f) in self.bout.fighters.iter().enumerate() {
            let x0 = 4 + i as i32 * (w + 6);
            let fill = self.seat_status_shade(i);
            let frame = Self::contrast_with(&palette, fill);
            let frac = f.health.max(0) * (w - 4) / f.max_health.max(1);
            fb.rect(x0, 6, w, 8, frame);
            fb.rect(x0 + 2, 8, frac, 4, fill);
        }
    }
}

impl FighterExt for Fighter {}
pub trait FighterExt {}
