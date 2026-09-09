//! Drawing a place: its backdrop, its menu, and what it just said to you.
//!
//! All of the rules live in `henge_core::place`. This file knows only where to
//! put pixels, which is why the same [`Visit`] could be driven by a server or a
//! test with no renderer at all.
//!
//! **Nothing here samples the art any more.** This file used to count the
//! colours inside each box, take the commonest as the ground and the hardest
//! contrast to it as the ink, on the argument that a sunlit town and a stone
//! circle at midnight have nothing in common. The original does not do that
//! anywhere. Every screen it paints a panel on paints it in a fixed index, and
//! `GFX:TextP` has no ink at all: it hands a glyph's own pixels to the same cel
//! blit every sprite goes through, and the screens that carry words reserve the
//! face's entries for it. Every one of the place palettes the baker writes has
//! index 0 black and index 1 a near white, which is that convention, so a panel
//! is [`PANEL`] and a line is [`Font::draw_own`] and neither depends on what is
//! behind it.
//!
//! **And there is no status bar.** A strip across the bottom with the day, the
//! purse and the wounds on it was this project's, the same invention that was
//! already taken off the map for the same reason: the original's place screens
//! are their own pictures, and what is drawn over one is what that screen's own
//! routine draws. `DonateLoop` writes `Your Gold` and the number at (2, 190) and
//! (20, 175) because `_WIZARD` says so; nothing writes a purse over a town.
//!
//! **A town draws nothing over its picture at all**, and what its five boxes
//! open is not a place: see [`crate::town`]. The stall, the tavern menu, the
//! dice room, the healer's and the mystic's menus and the temple's list that
//! this file used to draw over the town's own art are gone with the effects
//! that opened them.

use crate::framebuffer::Framebuffer;
use crate::text::Font;
use henge_assets::Registry;
use henge_core::item::Items;
use henge_core::place::{Effect, PlaceDef, Places, Visit};
use henge_core::pointer::Pointer;
use henge_core::{SCREEN_H, SCREEN_W};

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

/// The index a painted panel is filled with. `_STATUS:DisplayPillars` clears
/// its whole screen to 0 before it draws anything, and every palette the baker
/// writes has 0 black, so this is the game's own ground.
const PANEL: u8 = 0;

/// What a line that cannot be taken is written in. The original has no dim
/// state on a gadget -- a gadget with no permission bit simply does nothing --
/// but a keyboard list needs to say which rows are live, and 4 is a mid tone in
/// every one of the place palettes.
const SHUT: u8 = 4;

/// What a highlighted row's bar is filled with: index 1, which is the near
/// white every one of these palettes keeps there and the entry the fonts are
/// drawn in.
const BAR: u8 = 1;

impl PlaceScene {
    pub fn open(reg: &mut Registry, def: &PlaceDef, place: &str) -> anyhow::Result<PlaceScene> {
        let palette = reg
            .palette(&format!("palette.{}", def.scene))
            .map(|r| r.value.clone())
            .ok_or_else(|| anyhow::anyhow!("no palette for {}", def.scene))?;
        let img = reg.image(&def.scene)?;
        anyhow::ensure!(
            img.width == SCREEN_W && img.height == SCREEN_H,
            "{} is {}x{}, expected a full screen",
            def.scene,
            img.width,
            img.height
        );
        Ok(PlaceScene {
            visit: Visit::open_at(place, def),
            palette,
            pixels: img.pixels.clone(),
        })
    }

    pub fn render(
        &self,
        reg: &mut Registry,
        fb: &mut Framebuffer,
        def: &PlaceDef,
        font: Option<&Font>,
        pointer: &Pointer,
    ) -> anyhow::Result<()> {
        fb.set_palette(&self.palette);
        fb.pixels.copy_from_slice(&self.pixels);
        let Some(font) = font else { return Ok(()) };
        self.draw_menu(reg, fb, def, font);
        if let Some(box_) = def.text {
            self.draw_text(reg, fb, box_, font);
        }
        // On a screen whose options are boxes, the arrow is the only cursor
        // there is, which is what `HWLOOP` has: `MovePointer` and
        // `CHECKGADGET`, and nothing drawn to say where the choice is. A
        // pointer nobody has steered is parked on the highlighted box so the
        // keyboard still shows what fire would take.
        if def.boxes.is_some() && !pointer.woken {
            if let Some((_, x, y, _, h)) = menu_rects(def).get(self.visit.cursor).copied() {
                let parked = Pointer {
                    x: x + 4,
                    y: y + h / 2,
                    ..*pointer
                };
                crate::shell::draw_pointer_at(reg, fb, &parked);
            }
        }
        Ok(())
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
        // No plate. A place names this box precisely because the picture
        // already has furniture there for words. The original writes on it
        // and paints nothing under what it writes.
        const PAD: i32 = 5;
        let mut cy = y + PAD;
        for line in wrap(reg, font, &self.visit.said, w - PAD * 2) {
            if cy + 7 > y + h {
                break;
            }
            font.draw_own(reg, fb, &line, x + PAD, cy);
            cy += 8;
        }
    }

    fn draw_menu(&self, reg: &mut Registry, fb: &mut Framebuffer, def: &PlaceDef, font: &Font) {
        // A screen whose options are boxes over painted words draws nothing at
        // all: `InitHighWood` adds five gadgets with no text record and
        // `HWLOOP` writes nothing over the picture. See `PlaceDef::boxes`.
        if def.boxes.is_some() {
            return;
        }
        let [x, y, w, h] = def.menu;
        fb.rect(x, y, w, h, PANEL);

        const PAD: i32 = 5;
        const STEP: i32 = 9;
        let mut cy = y + PAD;
        font.draw_own(reg, fb, &def.name, x + PAD, cy);
        cy += STEP;
        fb.rect(x + PAD, cy, w - PAD * 2, 1, BAR);
        cy += 4;

        for (i, choice) in def.options.iter().enumerate() {
            // A shut door is dim because it is not built.
            let open = choice.effect.available();
            if i == self.visit.cursor {
                // The highlight is a bar rather than a marker: the fonts have no
                // arrow glyph, and inverting a line reads at this size anyway.
                // A shut door highlights faintly, so the highlight never makes
                // an option look live that is not.
                fb.rect(x + 2, cy - 2, w - 4, STEP, if open { BAR } else { SHUT });
            }
            // Every line goes down in the glyphs' own five indices, lit or
            // shut: `GFX:TextP` has no ink in it and the bar behind the
            // cursor is what says which line is which. Painting a shut line
            // in one colour flattened it to a silhouette, which is exactly
            // what `blit_mask` was for and why it is gone.
            font.draw_own(reg, fb, &choice.label, x + PAD, cy);
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
                font.draw_own(reg, fb, &line, x + PAD, cy);
                cy += 8;
            }
        }
    }
}

/// Where the menu's rows are, as boxes a pointer can be over.
///
/// The same arithmetic `draw_menu` uses, taken from one place so a row can
/// never be highlighted in one and hit in the other. This is what the
/// original's `ADDGADGET` does at the moment it draws each line, and the reason
/// it can: a gadget's rectangle is the rectangle of the thing drawn in it.
pub fn menu_rects(def: &PlaceDef) -> Vec<(usize, i32, i32, i32, i32)> {
    // A town's five are the original's own, out of `InitHighWood` and
    // `InitWaterDeep`, and they sit on words the picture already carries.
    if let Some(boxes) = def.boxes.as_ref() {
        return boxes
            .iter()
            .enumerate()
            .map(|(i, b)| (i, b[0], b[1], b[2], b[3]))
            .collect();
    }
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
        let candidate = if line.is_empty() {
            word.to_string()
        } else {
            format!("{line} {word}")
        };
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
    format!(
        "{:<12} > {}{}",
        def.name,
        label,
        if shut { " (shut)" } else { "" }
    )
}
