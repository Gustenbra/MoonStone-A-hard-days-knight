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
use henge_core::item::Items;
use henge_core::place::{Effect, PlaceDef, Places, Visit};
use henge_core::run::Run;
use henge_core::{SCREEN_H, SCREEN_W};

/// `DICE.CEL`, whose first six cels are the six faces.
const DICE_SHEET: &str = "bank.dice";

/// Loads whatever places the packs declare. A pack without them is not an
/// error: the map simply has nowhere to go, exactly as it did before.
pub fn load(reg: &Registry) -> Places {
    reg.read_data("data.places").unwrap_or_default()
}

/// Whatever goods the packs declare. A pack without items is not an error: the
/// stalls simply have nothing on them, which is what they had before.
pub fn load_items(reg: &Registry) -> Items {
    reg.read_data("data.items").unwrap_or_default()
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
        Ok(PlaceScene { visit: Visit::open_at(place, def), palette, pixels: img.pixels.clone() })
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
        font: Option<&Font>, run: &Run, items: &Items,
    ) -> anyhow::Result<()> {
        fb.set_palette(&self.palette);
        fb.pixels.copy_from_slice(&self.pixels);
        if def.dice {
            self.draw_dice(reg, fb);
        }
        let Some(font) = font else { return Ok(()) };
        self.draw_menu(reg, fb, def, font, run, items);
        if let Some(box_) = def.text {
            self.draw_text(reg, fb, box_, font);
        }
        self.draw_status(reg, fb, font, run);
        Ok(())
    }

    /// The three faces of the last throw, over `DICE.PIV`'s own picture of
    /// three dice on the wood.
    ///
    /// **Recovered.** `_TAVERN:RollDice` rolls the three bytes of `DDICE` and
    /// then blits `DICE.CEL` cel `DDICE[n]` three times, with the same
    /// `bx`, `cx` pair the moon is drawn with: (115, 15), (49, 38) and
    /// (75, 88). The faces are the cel numbers themselves, which is why a die
    /// is zero based everywhere in the simulation.
    fn draw_dice(&self, reg: &mut Registry, fb: &mut Framebuffer) {
        const AT: [(i32, i32); 3] = [(115, 15), (49, 38), (75, 88)];
        let Some(dice) = self.visit.dice else { return };
        for (n, (x, y)) in AT.iter().enumerate() {
            let Some(face) = dice.get(n).copied() else { continue };
            crate::sprite::draw(reg, fb, DICE_SHEET, face as usize, *x, *y, false);
        }
    }

    /// What the place said, in its own panel rather than under the menu.
    ///
    /// Several of the original's screens paint a slate or a plank for words
    /// and nothing else. Where a place names one, the paragraph goes there and
    /// the menu keeps to its own corner of the picture.
    fn draw_text(&self, reg: &mut Registry, fb: &mut Framebuffer, box_: [i32; 4], font: &Font) {
        if self.visit.said.is_empty() {
            return;
        }
        let [x, y, w, h] = box_;
        let ink = self.ink_for(fb, x, y, w, h);
        // Flooded with the colour the art already uses most inside the box, so
        // over a painted slate or plank nothing changes and over dithered
        // ground the words get a plate to sit on rather than a thicket.
        fb.rect(x, y, w, h, ink.ground);
        const PAD: i32 = 5;
        let mut cy = y + PAD;
        for line in wrap(reg, font, &self.visit.said, w - PAD * 2) {
            if cy + 7 > y + h {
                break;
            }
            font.draw(reg, fb, &line, x + PAD, cy, ink.text);
            cy += 8;
        }
    }

    fn draw_menu(&self, reg: &mut Registry, fb: &mut Framebuffer, def: &PlaceDef,
                 font: &Font, run: &Run, items: &Items) {
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
            // Two different kinds of "no". A shut door is dim because it is not
            // built; a potion you cannot afford is dim because of what is in
            // your purse, and it brightens the moment you can pay for it.
            let open = choice.effect.offered(items, run);
            // A price belongs to the goods, so the label never repeats it and
            // the two can never drift apart. It is written hard against the
            // right edge of the box, which is where a bill goes.
            let price = choice.effect.cost(items).map(|c| c.to_string());
            let pw = price.as_deref().map_or(0, |p| font.width(reg, p));
            if i == self.visit.cursor {
                // The highlight is a bar rather than a marker: the fonts have no
                // arrow glyph, and inverting a line reads at this size anyway.
                // A shut door highlights faintly, so the highlight never makes
                // an option look live that is not.
                let bar = if open { ink.text } else { ink.faint };
                fb.rect(x + 2, cy - 2, w - 4, STEP, bar);
                font.draw(reg, fb, &choice.label, x + PAD, cy, ink.ground);
                if let Some(p) = &price {
                    font.draw(reg, fb, p, x + w - PAD - pw, cy, ink.ground);
                }
            } else {
                let shade = if open { ink.text } else { ink.faint };
                font.draw(reg, fb, &choice.label, x + PAD, cy, shade);
                if let Some(p) = &price {
                    font.draw(reg, fb, p, x + w - PAD - pw, cy, shade);
                }
            }
            cy += STEP;
        }

        // What the place last said, wrapped into whatever width the box has.
        // Unless the place keeps a panel for words, in which case it goes there.
        if def.text.is_none() && !self.visit.said.is_empty() {
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
        fb.rect(0, 188, 320, 12, dark as u8);
        let left = format!("Day {}", run.day);
        font.draw(reg, fb, &left, 6, 191, light as u8);
        let right = format!("{} of {}", run.health.max(0), run.max_health);
        let w = font.width(reg, &right);
        font.draw(reg, fb, &right, 314 - w, 191, light as u8);
        // The purse goes in the middle, where a place has nothing else to put:
        // it is the number that changes when you buy something, so it has to be
        // on the screen you buy things on.
        let purse = format!("{} gold", run.gold);
        let pw = font.width(reg, &purse);
        font.draw(reg, fb, &purse, (SCREEN_W as i32 - pw) / 2, 191, light as u8);
    }
}

/// Where the menu's rows are, as boxes a pointer can be over.
///
/// The same arithmetic `draw_menu` uses, taken from one place so a row can
/// never be highlighted in one and hit in the other. This is what the
/// original's `ADDGADGET` does at the moment it draws each line, and the reason
/// it can: a gadget's rectangle is the rectangle of the thing drawn in it.
pub fn menu_rects(def: &PlaceDef) -> Vec<(usize, i32, i32, i32, i32)> {
    const PAD: i32 = 5;
    const STEP: i32 = 9;
    let [x, y, w, _] = def.menu;
    let top = y + PAD + STEP + 4;
    (0..def.options.len())
        .map(|i| (i, x + 2, top + i as i32 * STEP - 2, w - 4, STEP))
        .collect()
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
pub fn describe(def: &PlaceDef, visit: &Visit, items: &Items) -> String {
    let label = visit.selected(def).map_or("-", |c| c.label.as_str());
    let shut = visit
        .selected(def)
        .is_some_and(|c| matches!(c.effect, Effect::Closed { .. }));
    let price = visit
        .selected(def)
        .and_then(|c| c.effect.cost(items))
        .map_or(String::new(), |p| format!(" [{p}]"));
    format!("{:<12} > {}{}{}", def.name, label, price, if shut { " (shut)" } else { "" })
}
