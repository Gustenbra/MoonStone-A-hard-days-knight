//! Drawing a place: its backdrop, its menu, and what it just said to you.
//!
//! All of the rules live in `henge_core::place`. This file knows only where to
//! put pixels, which is why the same [`Visit`] could be driven by a server or a
//! test with no renderer at all.
//!
//! The one idea worth naming is how it picks colours. A place is drawn in the
//! backdrop's own 32 colours, and those 32 differ wildly between a sunlit town
//! and a stone circle at midnight, so nothing can be hardcoded. Instead the
//! menu takes the colour the art already uses most inside its own box and
//! writes on it in whatever contrasts hardest. Over Highwood's painted panel
//! that lands on parchment and ink, and the live menu replaces the painted one
//! without a seam.

use crate::framebuffer::Framebuffer;
use crate::text::Font;
use henge_assets::Registry;
use henge_core::place::{Effect, PlaceDef, Places, Visit};
use henge_core::run::Run;
use henge_core::{SCREEN_H, SCREEN_W};

/// Loads whatever places the packs declare. A pack without them is not an
/// error: the map simply has nowhere to go, exactly as it did before.
pub fn load(reg: &Registry) -> Places {
    reg.read_data("data.places").unwrap_or_default()
}

pub struct PlaceScene {
    pub visit: Visit,
    palette: Vec<u32>,
    pixels: Vec<u8>,
}

/// A palette read for writing on: the ground colour, the strongest contrast to
/// it, and something halfway for saying a thing is there but shut.
struct Ink {
    ground: u8,
    text: u8,
    faint: u8,
}

fn luma(c: u32) -> i32 {
    (((c >> 16) & 0xff) * 2 + ((c >> 8) & 0xff) * 3 + (c & 0xff)) as i32
}

impl PlaceScene {
    pub fn open(reg: &mut Registry, def: &PlaceDef, place: &str) -> anyhow::Result<PlaceScene> {
        let palette = reg
            .palette(&format!("palette.{}", def.scene))
            .map(|r| r.value.clone())
            .ok_or_else(|| anyhow::anyhow!("no palette for {}", def.scene))?;
        let img = reg.image(&def.scene)?;
        anyhow::ensure!(
            img.width == SCREEN_W && img.height == SCREEN_H,
            "{} is {}x{}, expected a full screen", def.scene, img.width, img.height
        );
        Ok(PlaceScene { visit: Visit::open(place), palette, pixels: img.pixels.clone() })
    }

    /// The colour the backdrop uses most inside a box, and what to write on it.
    fn ink_for(&self, fb: &Framebuffer, x: i32, y: i32, w: i32, h: i32) -> Ink {
        let mut counts = [0u32; 32];
        for yy in y.max(0)..(y + h).min(SCREEN_H as i32) {
            for xx in x.max(0)..(x + w).min(SCREEN_W as i32) {
                counts[(self.pixels[yy as usize * SCREEN_W + xx as usize] & 0x1f) as usize] += 1;
            }
        }
        let ground = counts
            .iter()
            .enumerate()
            .max_by_key(|(i, n)| (**n, std::cmp::Reverse(*i)))
            .map(|(i, _)| i)
            .unwrap_or(0);
        Self::ink_over(fb, ground as u8)
    }

    /// The two colours that read best over a given one.
    fn ink_over(fb: &Framebuffer, ground: u8) -> Ink {
        let base = luma(fb.palette[ground as usize & 0x1f]);
        let text = (0..32)
            .max_by_key(|i| (luma(fb.palette[*i]) - base).abs())
            .unwrap_or(0);
        let midpoint = (base + luma(fb.palette[text])) / 2;
        let faint = (0..32)
            .min_by_key(|i| (luma(fb.palette[*i]) - midpoint).abs())
            .unwrap_or(0);
        Ink { ground, text: text as u8, faint: faint as u8 }
    }

    pub fn render(
        &self, reg: &mut Registry, fb: &mut Framebuffer, def: &PlaceDef,
        font: Option<&Font>, run: &Run,
    ) -> anyhow::Result<()> {
        fb.set_palette(&self.palette);
        fb.pixels.copy_from_slice(&self.pixels);
        let Some(font) = font else { return Ok(()) };
        self.draw_menu(reg, fb, def, font);
        self.draw_status(reg, fb, font, run);
        Ok(())
    }

    fn draw_menu(&self, reg: &mut Registry, fb: &mut Framebuffer, def: &PlaceDef, font: &Font) {
        let [x, y, w, h] = def.menu;
        let ink = self.ink_for(fb, x, y, w, h);
        fb.rect(x, y, w, h, ink.ground);

        const PAD: i32 = 5;
        const STEP: i32 = 9;
        let mut cy = y + PAD;
        font.draw(reg, fb, &def.name, x + PAD, cy, ink.text);
        cy += STEP;
        fb.rect(x + PAD, cy, w - PAD * 2, 1, ink.text);
        cy += 4;

        for (i, choice) in def.options.iter().enumerate() {
            let open = choice.effect.available();
            if i == self.visit.cursor {
                // The highlight is a bar rather than a marker: the fonts have no
                // arrow glyph, and inverting a line reads at this size anyway.
                // A shut door highlights faintly, so the highlight never makes
                // an option look live that is not.
                let bar = if open { ink.text } else { ink.faint };
                fb.rect(x + 2, cy - 2, w - 4, STEP, bar);
                font.draw(reg, fb, &choice.label, x + PAD, cy, ink.ground);
            } else {
                font.draw(reg, fb, &choice.label, x + PAD, cy,
                          if open { ink.text } else { ink.faint });
            }
            cy += STEP;
        }

        // What the place last said, wrapped into whatever width the box has.
        if !self.visit.said.is_empty() {
            cy += 5;
            for line in wrap(reg, font, &self.visit.said, w - PAD * 2) {
                if cy + 7 > y + h {
                    break;
                }
                font.draw(reg, fb, &line, x + PAD, cy, ink.text);
                cy += 8;
            }
        }
    }

    /// The same strip the map draws, so the day and your wounds are in the same
    /// place whether you are walking or standing in a doorway.
    fn draw_status(&self, reg: &mut Registry, fb: &mut Framebuffer, font: &Font, run: &Run) {
        let (mut dark, mut light) = (0usize, 0usize);
        for i in 1..32 {
            if luma(fb.palette[i]) < luma(fb.palette[dark]) { dark = i; }
            if luma(fb.palette[i]) > luma(fb.palette[light]) { light = i; }
        }
        fb.rect(0, 178, 320, 22, dark as u8);
        let left = format!("Day {}", run.day);
        font.draw(reg, fb, &left, 6, 186, light as u8);
        let right = format!("{} of {}", run.health.max(0), run.max_health);
        let w = font.width(reg, &right);
        font.draw(reg, fb, &right, 314 - w, 186, light as u8);
    }
}

/// Greedy word wrap. Rendering, not rules, so it lives here.
fn wrap(reg: &Registry, font: &Font, text: &str, width: i32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
        if font.width(reg, &candidate) <= width || line.is_empty() {
            line = candidate;
        } else {
            lines.push(std::mem::take(&mut line));
            line = word.to_string();
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// A one-line summary for the headless trace.
pub fn describe(def: &PlaceDef, visit: &Visit) -> String {
    let label = visit.selected(def).map_or("-", |c| c.label.as_str());
    let shut = visit
        .selected(def)
        .is_some_and(|c| matches!(c.effect, Effect::Closed { .. }));
    format!("{:<12} > {}{}", def.name, label, if shut { " (shut)" } else { "" })
}
