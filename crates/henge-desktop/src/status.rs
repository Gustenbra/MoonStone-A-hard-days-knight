//! The status panel: what a knight is, drawn.
//!
//! Two views of the same sheet.
//!
//! **In a bout**, one plate per fighter along the bottom of the arena. Every
//! arena's walkable band stops between y 88 and y 134, so the foreground below
//! it is ground nobody can ever stand on, and the plates cost the fight nothing.
//! They replace four bare bars that named nobody.
//!
//! **The sheet itself**, over whatever is on screen, at the coordinates
//! `DisplayKnight` uses. Those are worth writing down, because they are the
//! panel:
//!
//! ```text
//! name          (60, 24)
//! STR CON END   label at x 41, number at x 63, rows y 35, 42, 49
//! XP GOLD       label at x 89, number at x 112, rows y 35, 42
//! health        (112, 49), printed as "have/most"
//! life points   (41, 59), one figure every 19 pixels
//! daggers       (38, 78), one every 10
//! weapon        (43, 95)
//! armour        (29, 112)
//! ```
//!
//! The furniture is the original's own. `KI.CEL` frame 19 is the small figure a
//! life point is drawn as, frame 21 the dagger, frames 22 to 25 the four swords
//! and frames 27 to 30 the four suits of armour, which is exactly what the
//! weapon and armour fields index: `+0x40` holds 0x16 to 0x19 and `+0x42` holds
//! 0x1b to 0x1e.
//!
//! They are drawn as silhouettes rather than in their own colours. Sheet pixels
//! are palette indices and nothing records which palette they were baked
//! against, so `KI.CEL` over an arena backdrop is noise. The fonts already solve
//! this the same way.
//!
//! **The five labels are not drawn from the artwork, and the words are still
//! the original's.** `KI.CEL` frames 38 to 42 read `STR :`, `CON :`, `END :`,
//! `XP :` and `GOLD :`, and `DisplayKnight` places exactly those cel numbers on
//! exactly those rows, which is how the three ability rows are known to be
//! strength, constitution and endurance in that order and the right column to be
//! experience and gold. They are five pixels tall and anti-aliased across seven
//! palette entries, so a silhouette of one is a smudge; what is set in type here
//! is what those sprites say.

use crate::framebuffer::Framebuffer;
use crate::sprite;
use crate::text::Font;
use henge_assets::Registry;
use henge_core::item::Items;
use henge_core::run::Run;
use henge_core::SCREEN_W;

/// The original's UI furniture.
const UI: &str = "bank.ki";

/// Cel numbers, straight out of `DisplayKnight`.
const PIP_LIFE: usize = 19;
const PIP_DAGGER: usize = 21;
/// `+0x40` holds 0x16 to 0x19; `+0x42` holds 0x1b to 0x1e.
const SWORDS: [&str; 4] = ["long_sword", "broad_sword", "claymore", "sword_of_sharpness"];
const SWORD_CEL: usize = 0x16;
const ARMOURS: [&str; 4] = ["padded_armour", "chain_mail", "plate_armour", "battle_armour"];
const ARMOUR_CEL: usize = 0x1b;

/// The four keys of the Valley, and where they go.
///
/// **Recovered.** `_STATUS:StatCheckKeys` reads byte `+0x14` of the item
/// record and draws one cel per bit set: bit 1 is cel 5 at x 0x4c, bit 2 cel 6
/// at 0x5e, bit 4 cel 7 at 0x70 and bit 8 cel 8 at 0x82, all on row 0x6f, each
/// through its own colour replacement (0x11, 0x21, 0x41, 0x81). So the x
/// spacing of eighteen and the slot order are the original's, and a slot left
/// empty is a key you have not found.
///
/// **The row is ours**, because henge's sheet is not laid out where the
/// original's is: y 0x6f runs the cels straight through the armour's name.
const KEY_CEL: usize = 5;
const KEY_X: i32 = 0x4c;
const KEY_STEP: i32 = 0x12;
const KEY_Y: i32 = 131;

fn luma(c: u32) -> i32 {
    (((c >> 16) & 0xff) * 2 + ((c >> 8) & 0xff) * 3 + (c & 0xff)) as i32
}

/// The darkest and brightest entries of whatever palette is loaded.
///
/// Every screen in this game brings its own 32 colours, so a panel that named
/// its ink would be legible in one arena and invisible in the next.
pub fn extremes(fb: &Framebuffer) -> (u8, u8) {
    let (mut dark, mut light) = (0usize, 0usize);
    for i in 1..32 {
        if luma(fb.palette[i]) < luma(fb.palette[dark]) {
            dark = i;
        }
        if luma(fb.palette[i]) > luma(fb.palette[light]) {
            light = i;
        }
    }
    (dark as u8, light as u8)
}

/// Something between the two extremes, for a menu line that is there but not
/// the one you are on. A shut option and an unlit one both need to read as
/// present rather than as absent.
pub fn faint(fb: &Framebuffer) -> u8 {
    let (dark, light) = extremes(fb);
    let midpoint = (luma(fb.palette[dark as usize]) + luma(fb.palette[light as usize])) / 2;
    (1..32)
        .min_by_key(|i| (luma(fb.palette[*i]) - midpoint).abs())
        .unwrap_or(light as usize) as u8
}

/// One fighter's line on the in-bout strip.
pub struct Plate {
    pub name: String,
    /// The seat's colour in the loaded palette, so the plate names the knight on
    /// screen rather than a number.
    pub colour: u8,
    pub health: i32,
    pub max_health: i32,
    pub lives: i32,
}

/// Where the strip sits. Above it is arena; below it is ground no arena's
/// walkable band reaches.
const STRIP_Y: i32 = 168;
const STRIP_H: i32 = 32;

/// One plate per fighter, along the bottom of the arena.
pub fn draw_plates(reg: &mut Registry, fb: &mut Framebuffer, font: Option<&Font>, plates: &[Plate]) {
    if plates.is_empty() {
        return;
    }
    let (dark, light) = extremes(fb);
    fb.rect(0, STRIP_Y, SCREEN_W as i32, STRIP_H, dark);

    let n = plates.len() as i32;
    let step = SCREEN_W as i32 / n;
    let w = step - 3;
    let faint = faint(fb);
    for (i, p) in plates.iter().enumerate() {
        let x = 2 + i as i32 * step;
        // A rule in the knight's own colour, so a plate is matched to a figure
        // by hue and not by counting from the left.
        fb.rect(x, STRIP_Y + 2, w, 1, p.colour);
        let Some(font) = font else { continue };

        // A fallen knight goes dim rather than disappearing: who was in the
        // fight is worth knowing after they are out of it.
        let down = p.health <= 0;
        let ink = if down { faint } else { light };
        font.draw(reg, fb, &p.name, x + 2, STRIP_Y + 6, if down { faint } else { p.colour });

        // Health, as a bar and as a number, because a bar says how bad it is and
        // a number says how bad exactly.
        let bar = w - 4;
        let frac = p.health.max(0) * bar / p.max_health.max(1);
        fb.rect(x + 2, STRIP_Y + 15, bar, 5, dark);
        fb.rect(x + 2, STRIP_Y + 15, frac, 5, p.colour);
        fb.rect(x + 2, STRIP_Y + 14, bar, 1, ink);
        fb.rect(x + 2, STRIP_Y + 20, bar, 1, ink);

        let line = format!("{}/{}", p.health.max(0), p.max_health);
        font.draw(reg, fb, &line, x + 2, STRIP_Y + 23, ink);

        // Life points, right-aligned. Small blocks rather than the panel's own
        // figure: five of those are sixty-five pixels wide and a plate is
        // seventy-seven, which would leave nowhere for the number.
        let pips = p.lives.max(0).min(8);
        for k in 0..pips {
            fb.rect(x + w - 2 - (k + 1) * 5, STRIP_Y + 24, 3, 5, if down { faint } else { p.colour });
        }
    }
}

/// The whole sheet, at `DisplayKnight`'s own coordinates, and beside it the
/// menu the original's status screen carries: the `Increase` gadgets and a
/// line for each thing carried, which is where a scroll is cast.
///
/// `run` supplies the numbers that move; the knight on it supplies the rest.
/// `menu` is each line and whether it is lit; `cursor` the highlighted one,
/// or none when the sheet is only being shown.
pub fn draw_sheet(
    reg: &mut Registry, fb: &mut Framebuffer, font: Option<&Font>, run: &Run, items: &Items,
    colour: u8, menu: &[(String, bool)], cursor: Option<usize>,
) {
    let (dark, light) = extremes(fb);
    let faint = faint(fb);
    // The original's panel is a column; the menu takes the rest of the width,
    // set to its own left edge so a long line never runs into the pips.
    let wide = !menu.is_empty();
    let width = if wide { 300 } else { 212 };
    // The panel grows a row when there is a key on it, because the four cels
    // are sixteen pixels tall and the original's own row for them runs through
    // where henge puts the armour.
    let keys = run.keys_held();
    let height = if keys.is_empty() { 132 } else { 156 };
    fb.rect(20, 12, width, height, dark);
    fb.rect(20, 12, width, 1, colour);
    fb.rect(20, 11 + height, width, 1, colour);
    let Some(font) = font else { return };
    draw_menu(reg, fb, font, menu, cursor, light, faint, colour);

    let name = if run.knight.named() { run.knight.name.clone() } else { "No knight".to_string() };
    font.draw(reg, fb, &name, 60, 24, colour);

    // Left column: the three abilities, on the rows the original puts them, with
    // the number a little further right than its own 63 so it clears a label set
    // in type rather than in five-pixel sprites.
    let k = &run.knight;
    for (row, label, value) in [
        (35, "Str", k.strength),
        (42, "Con", k.constitution),
        (49, "End", k.endurance),
    ] {
        font.draw(reg, fb, label, 41, row - 2, light);
        font.draw(reg, fb, &value.to_string(), 74, row - 2, light);
    }

    // Right column: what a run accumulates.
    const RIGHT: i32 = 122;
    font.draw(reg, fb, "XP", 89, 33, light);
    font.draw(reg, fb, &run.experience.to_string(), RIGHT, 33, light);
    font.draw(reg, fb, "Gold", 89, 40, light);
    font.draw(reg, fb, &run.gold.to_string(), RIGHT, 40, light);
    // Health has no label of its own on the original's panel either: the slash
    // between the two numbers is what says which pair they are.
    let health = format!("{}/{}", run.health.max(0), run.max_health);
    font.draw(reg, fb, &health, 89, 47, light);

    // Life points and daggers, at their own spacings.
    for i in 0..run.lives.max(0) {
        sprite::draw_mask(reg, fb, UI, PIP_LIFE, 41 + i * 19, 59, colour);
    }
    for i in 0..(run.knight.daggers.min(12) as i32) {
        sprite::draw_mask(reg, fb, UI, PIP_DAGGER, 38 + i * 10, 78, light);
    }

    // What is held and what is worn, each drawn as the cel its field indexes and
    // named in words beside it. Dimmer than the text: a silhouette of a suit of
    // armour is a solid shape, and in the brightest colour on screen it reads as
    // a hole rather than as a breastplate.
    if let Some(cel) = SWORDS.iter().position(|s| *s == k.weapon) {
        sprite::draw_mask(reg, fb, UI, SWORD_CEL + cel, 43, 95, faint);
    }
    font.draw(reg, fb, &k.weapon_name(items), 43, 104, light);
    if let Some(cel) = ARMOURS.iter().position(|s| *s == k.armour) {
        sprite::draw_mask(reg, fb, UI, ARMOUR_CEL + cel, 29, 112, faint);
    }
    font.draw(reg, fb, &k.armour_name(items), 75, 122, light);

    // The keys of the Valley, one slot each, in the original's own order and
    // spacing. Four filled slots is the Valley open.
    for key in keys {
        // The slot is the bit, not the order the keys are planted in: the
        // original tests bit 1 first and gives it cel 5 and the leftmost x, so
        // the glade's key is the left hand slot and the forest's the right.
        let slot = key.bit().trailing_zeros() as i32;
        sprite::draw_mask(
            reg, fb, UI, KEY_CEL + slot as usize,
            KEY_X + slot * KEY_STEP, KEY_Y, colour,
        );
    }
}

/// The sheet's menu, down the right hand side: eleven rows of seven pixels
/// between the top rule and the bottom one, windowed on the cursor when there
/// are more lines than that. An unlit line is drawn faint, like a shut door.
/// The sheet's menu rows, as boxes a pointer can be over.
///
/// The same `X`, `TOP`, row height and scroll `draw_menu` uses, and the
/// scrolling matters: the sheet shows eleven rows of a list that can be longer,
/// so a gadget has to be registered for the row that is actually on screen.
/// This is the screen the original's gadgets belong to: `AddIconGadget`,
/// `HotGadget` and `GadgetSlot` are all in `_STATUS`.
pub fn sheet_menu_rects(len: usize, cursor: Option<usize>) -> Vec<(usize, i32, i32, i32, i32)> {
    const ROWS: usize = 11;
    const X: i32 = 176;
    const TOP: i32 = 28;
    if len == 0 {
        return Vec::new();
    }
    let at = cursor.unwrap_or(0);
    let first = at.saturating_sub(ROWS - 1).min(len.saturating_sub(ROWS));
    (first..len.min(first + ROWS))
        .enumerate()
        .map(|(row, i)| (i, X - 8, TOP + row as i32 * 9 - 1, 300 - (X - 8) + 20, 9))
        .collect()
}

fn draw_menu(
    reg: &mut Registry, fb: &mut Framebuffer, font: &Font, menu: &[(String, bool)],
    cursor: Option<usize>, light: u8, faint: u8, colour: u8,
) {
    if menu.is_empty() {
        return;
    }
    const ROWS: usize = 11;
    const X: i32 = 176;
    const TOP: i32 = 28;
    let at = cursor.unwrap_or(0);
    let first = at.saturating_sub(ROWS - 1).min(menu.len().saturating_sub(ROWS));
    for (row, (i, (line, lit))) in menu.iter().enumerate().skip(first).take(ROWS).enumerate() {
        let y = TOP + row as i32 * 9;
        let ink = if !lit { faint } else if cursor == Some(i) { colour } else { light };
        if cursor == Some(i) {
            fb.rect(X - 8, y + 2, 4, 3, colour);
        }
        font.draw(reg, fb, line, X, y, ink);
    }
    // Nine rows of ninety four pixels is a lot of sheet; say when there is
    // more below than fits.
    if first + ROWS < menu.len() {
        font.draw(reg, fb, "...", X, TOP + ROWS as i32 * 9, faint);
    }
}
