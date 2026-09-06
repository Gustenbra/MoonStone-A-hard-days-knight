//! The arena: backdrop, scenery and two fighters, drawn in one depth-sorted pass
//! and simulated by `henge_core::combat`.
//!
//! Nothing here knows where the pixels came from, so this file is unchanged when
//! the reference art is replaced by our own.

use crate::framebuffer::Framebuffer;
use henge_assets::Registry;
use henge_core::arena::Bounds;
use henge_core::combat::{line_hits_body, simple_ai, Fighter, Intent};
use henge_core::content::{ActorData, ArenaData, Arenas, Families};

const CELL_W: usize = 32;
const CELL_H: usize = 25;
const DAMAGE: i32 = 25;

pub struct World {
    arenas: Arenas,
    families: Families,
    actors: ActorData,
    order: Vec<String>,
    index: usize,
    pub player: Fighter,
    pub foe: Fighter,
    ai_cooldown: i32,
    pub over_for: i32,
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

        let mut w = World {
            arenas, families, actors, order, index: 0,
            player: Fighter::new("knight", &Fighter::placeholder_def(), 0, 0, 1),
            foe: Fighter::new("knight", &Fighter::placeholder_def(), 0, 0, -1),
            ai_cooldown: 0,
            over_for: 0,
        };
        w.reset();
        Ok(w)
    }

    fn def(&self) -> &henge_core::content::ActorDef {
        &self.actors["knight"]
    }

    pub fn reset(&mut self) {
        let b = self.bounds();
        // Near the front of the walkable band: the band runs from the horizon
        // down, so its bottom edge is the ground closest to the viewer.
        let ground = b.bottom - 8;
        let def = self.actors["knight"].clone();
        self.player = Fighter::new("knight", &def, b.left + 70, ground, 1);
        self.foe = Fighter::new("knight", &def, b.right - 70, ground, -1);
        self.ai_cooldown = 20;
        self.over_for = 0;
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

    pub fn update(&mut self, intent: Intent) {
        let def = self.actors["knight"].clone();
        let bounds = self.bounds();

        let ai = simple_ai(&self.foe, &self.player, &def, &mut self.ai_cooldown);

        let player_line = self.player.step(&def, intent, bounds);
        let foe_line = self.foe.step(&def, ai, bounds);

        // Depth has to line up as well as reach. Two fighters at different
        // depths pass through each other's swings, which is what makes the
        // vertical axis worth using.
        if !player_line.is_empty() && self.foe.alive() {
            if (self.player.y - self.foe.y).abs() <= def.depth_tolerance
                && line_hits_body(&player_line, self.foe.body(&def))
            {
                self.player.struck = true;
                self.foe.take_hit(DAMAGE);
            }
        }
        if !foe_line.is_empty() && self.player.alive() {
            if (self.player.y - self.foe.y).abs() <= def.depth_tolerance
                && line_hits_body(&foe_line, self.player.body(&def))
            {
                self.foe.struck = true;
                self.player.take_hit(DAMAGE);
            }
        }

        self.separate(&def, bounds);

        if !self.player.alive() || !self.foe.alive() {
            self.over_for += 1;
        }
    }

    /// Two fighters at the same depth cannot occupy the same ground. Pushing
    /// them apart rather than blocking movement keeps a scrappy close-quarters
    /// fight readable instead of jamming the pair into a stalemate.
    fn separate(&mut self, def: &henge_core::content::ActorDef, bounds: Bounds) {
        if !self.player.alive() || !self.foe.alive() {
            return;
        }
        if (self.player.y - self.foe.y).abs() > def.depth_tolerance {
            return;
        }
        let min_gap = (def.body[2] - def.body[0]) as i32;
        let gap = self.foe.x - self.player.x;
        if gap.abs() >= min_gap {
            return;
        }
        let push = (min_gap - gap.abs() + 1) / 2;
        let dir = if gap >= 0 { 1 } else { -1 };
        let (px, _) = bounds.clamp(self.player.x - push * dir, self.player.y);
        let (fx, _) = bounds.clamp(self.foe.x + push * dir, self.foe.y);
        self.player.x = px;
        self.foe.x = fx;
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

        enum Item<'a> { Prop(&'a henge_core::arena::Prop), Fighter(&'a Fighter) }
        let mut items: Vec<(i32, Item)> = arena
            .terrain
            .placements
            .iter()
            .map(|p| (p.y as i32 + CELL_H as i32, Item::Prop(p)))
            .collect();
        items.push((self.player.depth(), Item::Fighter(&self.player)));
        items.push((self.foe.depth(), Item::Fighter(&self.foe)));
        items.sort_by_key(|(d, _)| *d);

        for (_, item) in items {
            match item {
                Item::Prop(p) => self.draw_prop(reg, fb, &sheet_id, p),
                Item::Fighter(f) => self.draw_fighter(reg, fb, f)?,
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

    fn draw_fighter(&self, reg: &mut Registry, fb: &mut Framebuffer, f: &Fighter)
        -> anyhow::Result<()> {
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
        Ok(())
    }

    /// Arena palettes have no fixed slots, so pick the darkest and brightest
    /// entries at draw time. That way the bars read on every backdrop instead of
    /// turning pink in one arena and vanishing in the next.
    fn draw_health(&self, fb: &mut Framebuffer) {
        let luma = |c: u32| ((c >> 16) & 0xff) * 2 + ((c >> 8) & 0xff) * 3 + (c & 0xff);
        let (mut dark, mut light) = (0usize, 0usize);
        for i in 1..32 {
            if luma(fb.palette[i]) < luma(fb.palette[dark]) { dark = i; }
            if luma(fb.palette[i]) > luma(fb.palette[light]) { light = i; }
        }
        let (dark, light) = (dark as u8, light as u8);
        for (f, x0) in [(&self.player, 8i32), (&self.foe, 320 - 8 - 82)] {
            let frac = f.health.max(0) * 78 / f.max_health.max(1);
            fb.rect(x0, 6, 82, 8, dark);
            fb.rect(x0 + 2, 8, frac, 4, light);
        }
    }
}

impl FighterExt for Fighter {}
pub trait FighterExt {}
