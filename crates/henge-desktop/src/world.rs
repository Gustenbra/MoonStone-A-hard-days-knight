//! The arena: backdrop, scenery and two fighters, drawn in one depth-sorted pass
//! and simulated by `henge_core::combat`.
//!
//! Nothing here knows where the pixels came from, so this file is unchanged when
//! the reference art is replaced by our own.

use crate::framebuffer::Framebuffer;
use henge_assets::Registry;
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
        };
        w.set_players(2);
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
        for c in self.control.iter_mut() {
            if let Control::Ai { cooldown } = c {
                *cooldown = 20;
            }
        }
        self.events.clear();
    }

    pub fn settled_for(&self) -> u32 { self.bout.settled_for }

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

    pub fn render(&self, reg: &mut Registry, fb: &mut Framebuffer) -> anyhow::Result<()> {
        let arena = self.arena();
        let family = self
            .families
            .get(&arena.family)
            .ok_or_else(|| anyhow::anyhow!("unknown arena family {}", arena.family))?;
        let (sheet_id, backdrop_id) = (family.sheet.clone(), family.backdrop.clone());

        // The backdrop owns the palette everything else is drawn in, which is how
        // the original recoloured the same creature per region for free.
        if let Some(p) = reg.palette(&format!("palette.{backdrop_id}")).map(|r| r.value.clone()) {
            fb.set_palette(&p);
        }
        match reg.image(&backdrop_id) {
            Ok(img) if img.width == 320 && img.height == 200 => {
                fb.pixels.copy_from_slice(&img.pixels)
            }
            _ => fb.clear(0),
        }

        enum Item<'a> { Prop(&'a henge_core::arena::Prop), Fighter(usize) }
        let mut items: Vec<(i32, Item)> = arena
            .terrain
            .placements
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
        fb.blit(&px, w, h, x, y, flip);

        // Four knights in identical armour are impossible to tell apart. The
        // original recoloured them per player; until that palette remap is
        // written, a marker above the head does the same job honestly.
        if f.alive() {
            let shade = Self::marker_shade(fb, index);
            fb.rect(f.x - 3, y - 6, 6, 2, shade);
        }
        Ok(())
    }

    /// Pick four shades that are as far apart as this arena's palette allows.
    /// Arena palettes have no fixed slots, so anything hardcoded turns invisible
    /// in one region and garish in the next.
    fn marker_shade(fb: &Framebuffer, index: usize) -> u8 {
        let luma = |c: u32| ((c >> 16) & 0xff) * 2 + ((c >> 8) & 0xff) * 3 + (c & 0xff);
        let mut ranked: Vec<usize> = (1..32).collect();
        ranked.sort_by_key(|i| std::cmp::Reverse(luma(fb.palette[*i])));
        ranked[(index * ranked.len() / 5).min(ranked.len() - 1)] as u8
    }

    /// Arena palettes have no fixed slots, so pick the darkest and brightest
    /// entries at draw time. That way the bars read on every backdrop instead of
    /// turning pink in one arena and vanishing in the next.
    /// One bar per fighter, each tinted to match the marker above its knight.
    fn draw_health(&self, fb: &mut Framebuffer) {
        let luma = |c: u32| ((c >> 16) & 0xff) * 2 + ((c >> 8) & 0xff) * 3 + (c & 0xff);
        let mut dark = 0usize;
        for i in 1..32 {
            if luma(fb.palette[i]) < luma(fb.palette[dark]) { dark = i; }
        }
        let n = self.bout.fighters.len().max(1) as i32;
        let w = (312 / n - 6).clamp(20, 82);
        for (i, f) in self.bout.fighters.iter().enumerate() {
            let x0 = 4 + i as i32 * (w + 6);
            let frac = f.health.max(0) * (w - 4) / f.max_health.max(1);
            fb.rect(x0, 6, w, 8, dark as u8);
            fb.rect(x0 + 2, 8, frac, 4, Self::marker_shade(fb, i));
        }
    }
}

impl FighterExt for Fighter {}
pub trait FighterExt {}
