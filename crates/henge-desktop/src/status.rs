//! The status panel: what a knight is, drawn.
//!
//! One view, and it is a whole screen of its own.
//!
//! **There is no in-fight readout, and there never was one.** This file used to
//! draw a plate per fighter across the deepest thirty two rows of every arena,
//! with a name, a health bar, a `have/most` number and life pips, and all of it
//! was ours. `MOON:Combat`, image `0x351`, is the whole fight loop and it is ten
//! calls long: set the frame's tick target off the BIOS counter at `0:046c`
//! (`0x96e1`), wait for vertical retrace on port `0x3da` (`0x5a24`), shake the
//! screen and run `VBLQUE` (`0x4988`), copy two hundred rows of the back buffer
//! to the page (`0x5a66`), run the ten task slots through `PerformCOMMAND`
//! (`0x9702`), place them and their shadows (`0x975b`), flip the page by writing
//! CRTC register `0x0c` (`0x5a3e`), test collisions (`0x9f1d`), `KnightGlowOn`
//! (`0x8f8`), and wait for the tick (`0x96f1`). Not one of them draws a glyph or
//! a rectangle; the only thing `VBLQUE` ever holds is `ADDCOL`'s palette upload
//! at `0x4a3f`. The knight's numbers live on the sheet this file draws, and
//! `DisplayKnight` (`0xc0c2`) has exactly three callers, `ReDisplay` and the two
//! arms of `_displayknight`, all three of them in `_STATUS` and none of them
//! reachable from a bout. A sweep of all 2,223 symbols for energy, health, bar,
//! hud, strip, gauge, meter, score, life and pip returns one name,
//! `_MAP:HealLife`, which is the healer.
//!
//! So the strip is gone, and the arena is the whole 320x200 screen it always
//! was: `AddKnight` stands an arrival a quarter, a half or three quarters of the
//! way from the deepest impassable rectangle down to row 200, and `CheckBorder`
//! lets a fighter walk his feet to row 198.
//!
//! **The sheet here is the original's whole screen**, and all of it is
//! recovered. `_STATUS:DisplayPillars` clears the screen to index 0 and falls
//! into `StatusSetup`, which walks a table of eight byte records
//! `[cel][x][y][mirror]` until the first word is negative and blits each one.
//! There are two such tables and they run into each other: `TradingData` at
//! `DS:0xf37c` has seven records and no terminator, so drawing it draws
//! `SingleData`'s eight as well and the `ff ff` at the end of the second stops
//! both. `DisplayPillars` picks between them on the status type, and only the
//! two types in `OffsetValues` (9 and 3) take `SingleData` alone with the
//! knight's own numbers shifted right by `0x4a`. The plain sheet takes both.
//!
//! What those fifteen records draw is **two ivied stone arches on three
//! pillars**: `KI.CEL` cel 26 is a 25 by 117 pillar, at x 0, 148 and 296; cels
//! 34, 35 and 36 are the column, the corner and the arch head, each placed once
//! and once mirrored. The knight's numbers sit inside the left arch and there is
//! a whole second arch on the right, which is where the menu goes.
//!
//! `_STATUS:ABorders` is a third such table, six records, and it is the row
//! labels: cels 38, 39 and 40 at x 41 on rows 35, 42 and 49, and cels 41, 42
//! and 43 at x 89 on the same three rows. They read `STR :`, `CON :`, `END :`,
//! `XP :`, `GOLD :` and `HIT :`, which is how the three ability rows are known
//! to be strength, constitution and endurance in that order, and the right
//! column to be experience, gold and health.
//!
//! `DisplayKnight` puts the rest on it:
//!
//! ```text
//! name          (60, 24)
//! STR CON END   number at x 63, rows y 35, 42, 49  (SINDEX 0x2e.., SROW += 7)
//! XP  GOLD HIT  number at x 112, same three rows
//! life points   (41, 59), one figure every 19, cel StatACEL + 0x11
//! daggers       (38, 78), one every 10, cel 0x15
//! weapon        (43, 95), cel knight[0x40]
//! armour        (29, 112), cel knight[0x42]
//! keys          (76, 94, 112, 130) on row 111, cels 5 to 8
//! ```
//!
//! **The life point figure is a knight, not a toad.** `_displayknight` sets
//! `StatACEL` to 1 and `DisplayKnight` draws cel `StatACEL + 0x11`, which is 18;
//! it adds two when `knight[0x3a]` is set, and `KI.CEL` 17 and 18 are a helmed
//! head while 19 and 20 are a frog. This project drew 19, so a knight's five
//! lives came up as five frogs whether he had been cursed or not.
//!
//! **And the palette is the original's.** `_STATUS:STAPAL` is twenty eight
//! twelve bit words and `STAKNP` the four after it, which together are the
//! screen's thirty two colours; `ColourStatus` overwrites the first two of
//! `STAKNP` with the knight's own pair, chosen on `knight[0x20]`. With that
//! loaded every cel on the screen can be blitted with its own indices, which is
//! what makes the ivy green, the pillars stone and the labels gold. Nothing here
//! is a silhouette any more and nothing here draws a box.

use crate::framebuffer::Framebuffer;
use crate::sprite;
use crate::text::Font;
use henge_assets::Registry;
use henge_core::run::Run;

/// The original's UI furniture.
const UI: &str = "bank.ki";

/// Cel numbers, straight out of `DisplayKnight`.
///
/// `StatACEL + 0x11` with `StatACEL` 1, which is the helmed head; two more when
/// `knight[0x3a]` says the wizard has made a toad of him, which is the frog.
const PIP_LIFE: usize = 18;
const PIP_TOAD: usize = 20;
const PIP_DAGGER: usize = 21;
/// `+0x40` holds 0x16 to 0x19; `+0x42` holds 0x1b to 0x1e.
const SWORDS: [&str; 4] = [
    "long_sword",
    "broad_sword",
    "claymore",
    "sword_of_sharpness",
];
const SWORD_CEL: usize = 0x16;
const ARMOURS: [&str; 4] = [
    "padded_armour",
    "chain_mail",
    "plate_armour",
    "battle_armour",
];
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
/// All four of those are the original's, row included: with the arches drawn
/// the armour cel sits at x 29 to 72 and the keys start at 76, so the row that
/// used to run through the armour's name has nothing in its way.
const KEY_CEL: usize = 5;
const KEY_X: i32 = 0x4c;
const KEY_STEP: i32 = 0x12;
const KEY_Y: i32 = 0x6f;

/// `_STATUS:STAPAL`, twenty eight words, and `STAKNP`, the four after it.
/// Amiga `0RGB` expanded the way every other palette in the game is, four bits
/// a channel times seventeen.
// Hand-aligned: four rows of eight, which is how the thirty-two entries are banked.
#[rustfmt::skip]
const STATUS_PALETTE: [u32; 32] = [
    0x000000, 0xffffff, 0xcccccc, 0xaaaaaa, 0x888888, 0x555555, 0x333333, 0xdd0000,
    0x552211, 0x884433, 0xcc7755, 0xffbb88, 0x555588, 0x7777aa, 0x88aadd, 0xaaddff,
    0x000000, 0xffffff, 0x777777, 0x000000, 0x66bb33, 0x228833, 0x005522, 0x002200,
    0xee7700, 0xbb4400, 0x882200, 0x551100, 0xcccccc, 0xdddddd, 0xeeeeee, 0xffffff,
];

/// `_STATUS:ColourStatus`, which writes two words into `STAKNP` chosen on
/// `knight[0x20]`: the seat, in the order the select screen offers them.
const KNIGHT_INK: [(u32, u32); 4] = [
    (0x0033ff, 0x002288),
    (0xffbb00, 0xbb6600),
    (0x44cc33, 0x116600),
    (0xff0000, 0x880000),
];

/// The first slot `ColourStatus` writes. The pair after it belongs to the
/// second knight on a trading screen, and stays as `STAKNP` has it.
const KNIGHT_SLOT: u8 = 28;

/// `_STATUS:StatusSetup` records, `[cel, x, y, mirror]`, in the order
/// `DisplayPillars` walks them for the plain sheet: `TradingData` and then
/// `SingleData`, because the first has no terminator of its own.
const FURNITURE: [(usize, i32, i32, bool); 15] = [
    (26, 296, 83, false),
    (34, 154, 23, false),
    (35, 181, 29, false),
    (36, 181, 10, false),
    (34, 288, 23, true),
    (35, 273, 29, true),
    (36, 235, 10, true),
    (26, 0, 83, false),
    (26, 148, 83, false),
    (34, 6, 23, false),
    (35, 33, 29, false),
    (36, 33, 10, false),
    (34, 140, 23, true),
    (35, 125, 29, true),
    (36, 87, 10, true),
];

/// `_STATUS:ABorders`, the six label cels.
const LABELS: [(usize, i32, i32); 6] = [
    (41, 89, 35),
    (42, 89, 42),
    (43, 89, 49),
    (38, 41, 35),
    (39, 41, 42),
    (40, 41, 49),
];

/// `DisplayKnight`: the name, the three ability numbers down `SROW`, and the
/// right hand column.
const NAME_AT: (i32, i32) = (60, 24);
const ABILITY_X: i32 = 63;
const COLUMN_X: i32 = 112;
const ROW_TOP: i32 = 35;
const ROW_STEP: i32 = 7;

/// The original's whole status screen, and in the right hand arch the menu
/// this project puts there: the `Increase` gadgets and a line for each thing
/// carried, which is where a scroll is cast.
///
/// `run` supplies the numbers that move; the knight on it supplies the rest.
/// `menu` is each line and whether it is lit; `cursor` the highlighted one,
/// or none when the sheet is only being shown.
pub fn draw_sheet(
    reg: &mut Registry,
    fb: &mut Framebuffer,
    font: Option<&Font>,
    run: &Run,
    menu: &[(String, bool)],
    cursor: Option<usize>,
) {
    // `DisplayPillars` clears the screen and `ColourStatus` colours it. The
    // sheet owns the screen the way the original's does; nothing behind it is
    // meant to show through.
    fb.clear(0);
    let seat = run.knight.seat % KNIGHT_INK.len();
    let mut palette = STATUS_PALETTE;
    palette[KNIGHT_SLOT as usize] = KNIGHT_INK[seat].0;
    palette[KNIGHT_SLOT as usize + 1] = KNIGHT_INK[seat].1;
    fb.set_palette(&palette);

    for (cel, x, y, mirror) in FURNITURE {
        sprite::draw(reg, fb, UI, cel, x, y, mirror);
    }
    for (cel, x, y) in LABELS {
        sprite::draw(reg, fb, UI, cel, x, y, false);
    }

    // Life points, daggers, what is held and what is worn, each the cel its own
    // field indexes and each in its own colours, which is what the palette
    // above is for.
    let toad = run.is_toad();
    for i in 0..run.lives.max(0) {
        let cel = if toad { PIP_TOAD } else { PIP_LIFE };
        sprite::draw(reg, fb, UI, cel, 41 + i * 19, 59, false);
    }
    for i in 0..(run.knight.daggers.min(12) as i32) {
        sprite::draw(reg, fb, UI, PIP_DAGGER, 38 + i * 10, 78, false);
    }
    let k = &run.knight;
    if let Some(cel) = SWORDS.iter().position(|s| *s == k.weapon) {
        sprite::draw(reg, fb, UI, SWORD_CEL + cel, 43, 95, false);
    }
    if let Some(cel) = ARMOURS.iter().position(|s| *s == k.armour) {
        sprite::draw(reg, fb, UI, ARMOUR_CEL + cel, 29, 112, false);
    }
    // The keys of the Valley, one slot each, in the original's own order and
    // spacing. Four filled slots is the Valley open. The slot is the bit, not
    // the order the keys are planted in: `StatCheckKeys` tests bit 1 first and
    // gives it cel 5 and the leftmost x, so the glade's key is the left hand
    // slot and the forest's the right.
    for key in run.keys_held() {
        let slot = key.bit().trailing_zeros() as i32;
        sprite::draw(
            reg,
            fb,
            UI,
            KEY_CEL + slot as usize,
            KEY_X + slot * KEY_STEP,
            KEY_Y,
            false,
        );
    }

    let Some(font) = font else { return };
    // Every line in its own indices. `TextP` hands a glyph to the same blitter
    // every cel above goes through, and `SMALL.FON` is one plane in index 1,
    // which `STAPAL` has as white.
    let name = if run.knight.named() {
        run.knight.name.clone()
    } else {
        "No knight".to_string()
    };
    font.draw_own(reg, fb, &name, NAME_AT.0, NAME_AT.1);

    for (i, value) in [k.strength, k.constitution, k.endurance].iter().enumerate() {
        let y = ROW_TOP + i as i32 * ROW_STEP;
        font.draw_own(reg, fb, &value.to_string(), ABILITY_X, y);
    }
    // `XP`, `GOLD` and `HIT`, whose labels are cels 41, 42 and 43 above.
    font.draw_own(reg, fb, &run.experience.to_string(), COLUMN_X, ROW_TOP);
    font.draw_own(reg, fb, &run.gold.to_string(), COLUMN_X, ROW_TOP + ROW_STEP);
    let health = format!("{}/{}", run.health.max(0), run.max_health);
    font.draw_own(reg, fb, &health, COLUMN_X, ROW_TOP + 2 * ROW_STEP);

    draw_menu(reg, fb, font, menu, cursor);
}

/// The sheet's menu, in the right hand arch: eleven rows of nine pixels,
/// windowed on the cursor when there are more lines than that.
///
/// The arch the original draws on the right of its status screen has an
/// interior of x 181 to 288 below the corner cels at y 41, and that is the box
/// these rows sit in. The rows themselves are this project's: the original's
/// own menu is a system of gadgets and string tables in `SetUpStatus` that this
/// screen does not yet run.
const MENU_X: i32 = 184;
const MENU_TOP: i32 = 44;
const MENU_ROWS: usize = 11;
const MENU_STEP: i32 = 9;

/// The sheet's menu rows, as boxes a pointer can be over.
///
/// The same `X`, `TOP`, row height and scroll `draw_menu` uses, and the
/// scrolling matters: the sheet shows eleven rows of a list that can be longer,
/// so a gadget has to be registered for the row that is actually on screen.
/// This is the screen the original's gadgets belong to: `AddIconGadget`,
/// `HotGadget` and `GadgetSlot` are all in `_STATUS`.
pub fn sheet_menu_rects(len: usize, cursor: Option<usize>) -> Vec<(usize, i32, i32, i32, i32)> {
    if len == 0 {
        return Vec::new();
    }
    let at = cursor.unwrap_or(0);
    let first = at
        .saturating_sub(MENU_ROWS - 1)
        .min(len.saturating_sub(MENU_ROWS));
    (first..len.min(first + MENU_ROWS))
        .enumerate()
        .map(|(row, i)| {
            (
                i,
                MENU_X - 8,
                MENU_TOP + row as i32 * MENU_STEP - 1,
                296 - (MENU_X - 8),
                MENU_STEP,
            )
        })
        .collect()
}

/// The two entries of `STAPAL` a menu line that is not simply lit takes: 5 is
/// the grey the bold face rings its letters in and 28 the first of the two
/// words `ColourStatus` writes for this knight.
const MENU_SHUT: u8 = 5;

fn draw_menu(
    reg: &mut Registry,
    fb: &mut Framebuffer,
    font: &Font,
    menu: &[(String, bool)],
    cursor: Option<usize>,
) {
    if menu.is_empty() {
        return;
    }
    let at = cursor.unwrap_or(0);
    let first = at
        .saturating_sub(MENU_ROWS - 1)
        .min(menu.len().saturating_sub(MENU_ROWS));
    for (row, (i, (line, lit))) in menu
        .iter()
        .enumerate()
        .skip(first)
        .take(MENU_ROWS)
        .enumerate()
    {
        let y = MENU_TOP + row as i32 * MENU_STEP;
        if cursor == Some(i) {
            fb.rect(MENU_X - 8, y + 2, 4, 3, KNIGHT_SLOT);
        }
        // A shut door is drawn in the palette's own grey; anything a player can
        // still take is drawn in the glyphs' own white.
        if *lit {
            font.draw_own(reg, fb, line, MENU_X, y);
        } else {
            font.draw(reg, fb, line, MENU_X, y, MENU_SHUT);
        }
    }
    // Eleven rows is a lot of sheet; say when there is more below than fits.
    if first + MENU_ROWS < menu.len() {
        font.draw(
            reg,
            fb,
            "...",
            MENU_X,
            MENU_TOP + MENU_ROWS as i32 * MENU_STEP,
            MENU_SHUT,
        );
    }
}
