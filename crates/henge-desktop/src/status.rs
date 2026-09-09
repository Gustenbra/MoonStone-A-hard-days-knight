//! The status panel: what a knight is, drawn, and what a knight can do with it.
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
//! reachable from a bout.
//!
//! **The panel has no menu, and the arch that used to hold one is not there.**
//! This file carried eleven rows of nine pixels in the right hand arch, windowed
//! on a cursor, with `Increase` gadgets and a line per carried item, and its own
//! comment admitted the rows were this project's. Two readings removed them
//! both:
//!
//! * `DisplayPillars` (`0xc02e`) picks its furniture table on the status type.
//!   `OffsetValues` is `{9, 3}`, and for those two it walks `SingleData` alone
//!   with `StatsOffset` set to `0x4a`. The plain character sheet is type 9. So
//!   **the sheet is one arch, centred**, on two pillars at x 74 and 222, and the
//!   three pillars and two arches of `TradingData` plus `SingleData` belong to
//!   the screens that have two parties on them: a trade, a lair, the merchant,
//!   the dragon. Drawing the trading furniture on the plain sheet is what left
//!   an empty arch on the right and invited a menu into it.
//! * `SetUpStatus` (`0xcf45`) and `SetUpID` (`0xd2b9`) are the menu's
//!   replacement, and they are not a menu. Every icon on the screen is a gadget
//!   (`PlaceIcons` blits it and `AddIconGadget` registers it in the same
//!   breath), its rectangle is the icon's own cel header, and the line it says
//!   is `Response1[STID >> 1]`, a ten-byte text record with `x` 0, `y` 3 and
//!   flags 3, which is **centred across the top of the screen in the bold
//!   face**. Fire over an icon runs the operation in its payload. So the sheet
//!   is pointed at, not scrolled, and the eleven rows had nowhere to be.
//!
//!   The tables themselves are in [`henge_core::status`].
//!
//! **The furniture is recovered.** `StatusSetup` (`0xc06c`) walks eight byte
//! records `[cel][x][y][mirror]` until the first word is negative, adds
//! `StatsOffset` to each `x`, and blits. `TradingData` at `DS:0xf37c` has seven
//! records and no terminator, so walking it walks `SingleData`'s eight as well
//! and the `ff ff` at the end of the second stops both. It also calls
//! `CreateExit` whenever the cel is `0x1a`, so **every pillar is the way out**:
//! a 25 by 117 gadget with id 7, which is the first thing `HotGadget` tests.
//!
//! What those records draw is ivied stone arches: `KI.CEL` cel 26 is the pillar
//! and cels 34, 35 and 36 are the column, the corner and the arch head, each
//! placed once and once mirrored.
//!
//! `_STATUS:ABorders` is a third such table, six records, and it is the row
//! labels: cels 38, 39 and 40 at x 41 and 41, 42 and 43 at x 89, on rows 35, 42
//! and 49. They read `STR :`, `CON :`, `END :`, `XP :`, `GOLD :` and `HIT :`,
//! which is how the three ability rows are known to be strength, constitution
//! and endurance in that order.
//!
//! `DisplayKnight` puts the rest on it, and the four `PlaceIcons` calls it opens
//! with are the labels again with gadget ids on them, 0, 2, 4 and 8:
//!
//! ```text
//! name          (60, 24)
//! STR CON END   number at x 63, rows y 35, 42, 49  (SINDEX 0x2e.., SROW += 7)
//! XP  GOLD HIT  number at x 112, same three rows
//! life points   (41, 59), one every 19, cel StatACEL + 0x11
//! daggers       (38, 78), one every 10, cel 0x15
//! weapon        (43, 95), cel knight[0x40]
//! armour        (29, 112), cel knight[0x42]
//! ```
//!
//! and then it ends on `push word ptr [si + 0x44]; pop word ptr [Address]` and
//! **falls through into `DisplayMagic`** at `0xc38e`, which is why the potions,
//! the gems, the rings, the talismans, the five scrolls, the four keys and the
//! moonstones are all part of every knight's sheet and not a page of their own.
//! Those were missing here entirely.
//!
//! **The life point figure is a knight, not a toad, and on the plain sheet it is
//! cel 17 rather than 18.** `DisplayKnight` draws `StatACEL + 0x11` and adds two
//! when `knight[0x3a]` says the wizard has made a toad of him; `ReDisplay` sets
//! `StatACEL` to 0 for the knight whose sheet it is and `_displayknight` sets it
//! to 1 for the second knight in the other arch. `KI.CEL` 17 and 18 are a helmed
//! head and 19 and 20 are a frog.
//!
//! **And the palette is the original's.** `_STATUS:STAPAL` is twenty eight
//! twelve bit words and `STAKNP` the four after it, which together are the
//! screen's thirty two colours; `ColourStatus` overwrites the first two of
//! `STAKNP` with the knight's own pair, chosen on `knight[0x20]`. With that
//! loaded every cel on the screen draws in its own colours and nothing here is a
//! silhouette or a box.

use crate::framebuffer::Framebuffer;
use crate::sprite;
use crate::text::Font;
use henge_assets::Registry;
use henge_core::knight::Knight;
use henge_core::pointer::Gadget;
use henge_core::run::Run;
use henge_core::status::{Hoard, Payload, Screen, Table, EXIT_ID, SCROLL_SLOTS};

/// The original's UI furniture.
const UI: &str = "bank.ki";

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

/// One record of `StatusSetup`'s tables, `[cel, x, y, mirror]`.
type Furniture = (usize, i32, i32, bool);

/// `_STATUS:TradingData` at `DS:0xf37c`: seven records, **no terminator**, which
/// is why walking it walks the next table too.
#[rustfmt::skip]
const TRADING_DATA: [Furniture; 7] = [
    (0x1a, 0x128, 0x53, false),
    (0x22, 0x09a, 0x17, false),
    (0x23, 0x0b5, 0x1d, false),
    (0x24, 0x0b5, 0x0a, false),
    (0x22, 0x120, 0x17, true),
    (0x23, 0x111, 0x1d, true),
    (0x24, 0x0eb, 0x0a, true),
];

/// `_STATUS:SingleData`, eight records and an `ff ff`. One arch on two pillars.
#[rustfmt::skip]
const SINGLE_DATA: [Furniture; 8] = [
    (0x1a, 0x000, 0x53, false),
    (0x1a, 0x094, 0x53, false),
    (0x22, 0x006, 0x17, false),
    (0x23, 0x021, 0x1d, false),
    (0x24, 0x021, 0x0a, false),
    (0x22, 0x08c, 0x17, true),
    (0x23, 0x07d, 0x1d, true),
    (0x24, 0x057, 0x0a, true),
];

/// `_STATUS:ABorders`, the six label cels, in the table's own order.
#[rustfmt::skip]
const ABORDERS: [Furniture; 6] = [
    (0x29, 0x59, 0x23, false),
    (0x2a, 0x59, 0x2a, false),
    (0x2b, 0x59, 0x31, false),
    (0x26, 0x29, 0x23, false),
    (0x27, 0x29, 0x2a, false),
    (0x28, 0x29, 0x31, false),
];

/// `StatusSetup`'s test: the pillar cel is the one that makes an exit gadget.
const PILLAR_CEL: usize = 0x1a;

/// `DisplayKnight`: where the name and the numbers go.
const NAME_AT: (i32, i32) = (0x3c, 0x18);
const ABILITY_X: i32 = 0x3f;
const COLUMN_X: i32 = 0x70;
const ROW_TOP: i32 = 0x23;
const ROW_STEP: i32 = 7;

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

/// One thing `PlaceIcons` drew and `AddIconGadget` registered, which in the
/// original is the same act.
#[derive(Clone, Debug)]
pub struct Icon {
    pub cel: usize,
    pub x: i32,
    pub y: i32,
    /// `STID`, which is twice the slot. [`EXIT_ID`] for a pillar.
    pub id: usize,
    /// `STRP` and `STPL`.
    pub payload: Payload,
}

/// A laid out panel: the furniture that is only drawn, the icons that are both
/// drawn and registered, and which array says what each of them is.
pub struct Panel {
    /// Drawn and not pointed at: the arches, the labels, a lair's chest.
    plain: Vec<Furniture>,
    icons: Vec<Icon>,
    /// `RESP`, which is `Response1` for the left party and `Response2` for the
    /// right. One panel can carry both, so the array rides on the icon's side.
    tables: Vec<Table>,
    /// Which entry of `tables` each icon reads.
    side: Vec<usize>,
    /// The three ability slots `SetUpID` 0xd39a relights with `Inc`'s own line
    /// and the `0x40` bit, because the experience covers the cost.
    raisable: [bool; 3],
    /// Lines the panel draws itself rather than blitting: `DisplayGold`'s
    /// `"%d gp."` and the knight's numbers.
    words: Vec<(String, i32, i32)>,
    /// Gadgets with a rectangle of their own rather than a cel's. There is one
    /// in the whole program: `DisplayAquire`'s, which `HotGadget` recognises by
    /// its text record being `NEXT` and not by any id.
    extra: Vec<Gadget>,
}

impl Panel {
    fn new() -> Panel {
        Panel {
            plain: Vec::new(),
            icons: Vec::new(),
            tables: Vec::new(),
            side: Vec::new(),
            raisable: [false; 3],
            words: Vec::new(),
            extra: Vec::new(),
        }
    }

    /// `StatusSetup`: blit each record and make an exit of every pillar.
    fn furniture(&mut self, table: &[Furniture], offset: i32) {
        for (cel, x, y, mirror) in table.iter().copied() {
            self.plain.push((cel, x + offset, y, mirror));
            if cel == PILLAR_CEL {
                // `CreateExit`: 0x19 by 0x75 at the pillar's own corner, id 7.
                self.icons.push(Icon {
                    cel,
                    x: x + offset,
                    y,
                    id: EXIT_ID,
                    payload: Payload::default(),
                });
                // `side` is read by icon index, so a pillar has to take a
                // place in it too or every icon after it reads the wrong
                // array. `EXIT` is its own record and `line` never gets this
                // far for one, so which side it is put on does not matter.
                self.side.push(0);
            }
        }
    }

    /// `PlaceIcons`: `cx` copies of `STCEL`, stepping `STI` in x, each one a
    /// gadget of its own.
    #[allow(clippy::too_many_arguments)]
    fn place(
        &mut self,
        side: usize,
        count: u32,
        cel: usize,
        mut x: i32,
        y: i32,
        step: i32,
        id: usize,
        payload: Payload,
    ) {
        for _ in 0..count {
            self.icons.push(Icon {
                cel,
                x,
                y,
                id,
                payload,
            });
            self.side.push(side);
            x += step;
        }
    }

    fn words(&mut self, line: impl Into<String>, x: i32, y: i32) {
        self.words.push((line.into(), x, y));
    }

    /// The boxes the pointer can be over, with the line each one says.
    ///
    /// The rectangle is the cel's own header, which is what `AddIconGadget`
    /// reads out of `es:[bx + si + 0xe]` and `+0x10`.
    pub fn gadgets(&self, reg: &mut Registry) -> Vec<Gadget> {
        let mut out = Vec::with_capacity(self.icons.len());
        for (n, icon) in self.icons.iter().enumerate() {
            let Some(rect) = reg
                .sheet(UI)
                .and_then(|s| s.value.frames.get(icon.cel).copied())
            else {
                continue;
            };
            let (label, lit) = self.line(n, icon);
            out.push(Gadget {
                id: icon.id,
                x: icon.x,
                y: icon.y,
                w: rect.w as i32,
                h: rect.h as i32,
                label,
                payload: icon.payload,
                lit,
            });
        }
        out.extend(self.extra.iter().cloned());
        out
    }

    /// `Response1[STID >> 1]` or `Response2[STID >> 1]`, with `SetUpID`'s two
    /// overrides on top: `Identify` carries no permission bit, and an ability
    /// row the experience can pay for takes `Inc`'s line and the `0x40` bit.
    fn line(&self, n: usize, icon: &Icon) -> (String, bool) {
        if icon.id == EXIT_ID {
            // `EXIT` is its own record, `Exit@` at `DS:0xed1a`.
            return ("Exit".to_string(), true);
        }
        let side = self.side.get(n).copied().unwrap_or(0);
        let Some(table) = self.tables.get(side).copied() else {
            return (String::new(), false);
        };
        let Some(slot) = henge_core::status::slot_of(icon.id) else {
            return (String::new(), false);
        };
        if slot < 3 && side == 0 && self.raisable[slot] {
            return (henge_core::status::INC[slot].to_string(), true);
        }
        let lit = table.acts() && slot >= 3;
        (table.line(slot).unwrap_or_default().to_string(), lit)
    }

    /// Paint it. Every cel in its own indices, which is what [`STATUS_PALETTE`]
    /// is loaded for.
    pub fn draw(&self, reg: &mut Registry, fb: &mut Framebuffer, font: Option<&Font>) {
        for (cel, x, y, mirror) in self.plain.iter().copied() {
            sprite::draw(reg, fb, UI, cel, x, y, mirror);
        }
        for icon in &self.icons {
            if icon.id == EXIT_ID {
                // The pillar is already in `plain`; `CreateExit` only adds the
                // gadget.
                continue;
            }
            sprite::draw(reg, fb, UI, icon.cel, icon.x, icon.y, false);
        }
        let Some(font) = font else { return };
        for (line, x, y) in &self.words {
            font.draw_own(reg, fb, line, *x, *y);
        }
    }
}

/// The fields `DisplayKnight` reads off `[si]`, whichever record `si` is:
/// the run's own, for the knight whose sheet this is, or a computer
/// knight's, for the second one on a trading screen.
struct KnightSheet<'a> {
    knight: &'a Knight,
    experience: u32,
    gold: u32,
    health: i32,
    max_health: i32,
    lives: i32,
    /// `[si + 0x3a] > 0`, read as a `cel + 2` rather than a day count here.
    toad: bool,
    /// `[si + 0x44]`, already a `Hoard` on a computer knight's own record;
    /// `Run` keeps it as an inventory instead, so `Hoard::of` builds one.
    hoard: Hoard,
}

impl<'a> KnightSheet<'a> {
    fn of_run(run: &'a Run) -> KnightSheet<'a> {
        KnightSheet {
            knight: &run.knight,
            experience: run.experience,
            gold: run.gold,
            health: run.health,
            max_health: run.max_health,
            lives: run.lives,
            toad: run.is_toad(),
            hoard: Hoard::of(&run.kit, run.knight.weapon == "sword_of_sharpness"),
        }
    }

    fn of_rival(r: &'a henge_core::rival::Rival) -> KnightSheet<'a> {
        KnightSheet {
            knight: &r.knight,
            experience: r.experience,
            gold: r.gold,
            health: r.health,
            max_health: r.max_health,
            lives: r.lives,
            toad: r.toad > 0,
            hoard: r.hoard,
        }
    }
}

/// `DisplayKnight` at `0xc0c2`, and `DisplayMagic` after it because it falls
/// through.
///
/// `acel` is `StatACEL`: 0 for the knight whose sheet this is and 1 for the
/// second knight in the other arch (`_displayknight`'s second arm).
#[allow(clippy::too_many_arguments)]
fn display_knight(
    panel: &mut Panel,
    side: usize,
    offset: i32,
    sheet: &KnightSheet,
    acel: usize,
    name: &str,
) {
    let k = sheet.knight;
    // The name, `[si + 0x4c]`, at (0x3c + StatsOffset, 0x18).
    panel.words(name, NAME_AT.0 + offset, NAME_AT.1);

    // The four label gadgets, which are the same cels `ABorders` blits at the
    // same places with ids on them. `PlaceIcons` is called with `cx` holding
    // the value of the field, so the original stacks that many copies with
    // `STI` zero; one is all that can be seen and all that can be hit.
    //
    // `STPL` 0x2e, 0x2f, 0x30 at rows 0x23, 0x2a, 0x31: strength,
    // constitution, endurance, which is slots 0, 1 and 2.
    for (slot, cel, row, field) in [
        (0usize, 0x26usize, 0x23i32, 0x2eu16),
        (2, 0x28, 0x31, 0x30),
        (1, 0x27, 0x2a, 0x2f),
    ] {
        panel.place(
            side,
            1,
            cel,
            0x29 + offset,
            row,
            0,
            slot * 2,
            Payload::new(3, field),
        );
    }
    // `GOLD :`, cel 0x2a at (0x59, 0x2a), id 8, `STRP` 2, `STPL` 0x32.
    panel.place(
        side,
        1,
        0x2a,
        0x59 + offset,
        0x2a,
        0,
        8,
        Payload::new(2, 0x32),
    );
    // `ABorders`, walked by `StatusSetup` so it takes `StatsOffset` too.
    panel.furniture(&ABORDERS, offset);

    // `[si + 0x36]` at (0x70, 0x23), `[si + 0x32]` at (0x70, 0x2a), then the
    // three ability bytes down `SROW` from 0x23 in steps of seven at 0x3f, and
    // `[si+0x38]/[si+0x3c]` at (0x70, 0x31).
    panel.words(sheet.experience.to_string(), COLUMN_X + offset, ROW_TOP);
    panel.words(
        sheet.gold.to_string(),
        COLUMN_X + offset,
        ROW_TOP + ROW_STEP,
    );
    for (i, value) in [k.strength, k.constitution, k.endurance].iter().enumerate() {
        panel.words(
            value.to_string(),
            ABILITY_X + offset,
            ROW_TOP + i as i32 * ROW_STEP,
        );
    }
    panel.words(
        format!("{}/{}", sheet.health.max(0), sheet.max_health),
        COLUMN_X + offset,
        ROW_TOP + 2 * ROW_STEP,
    );

    // Life points: cel `StatACEL + 0x11`, two more for a toad, at (0x29, 0x3b)
    // every 0x13. `cl` is `[si + 0x31]` and a negative one is taken as none.
    let life = sheet.lives.max(0) as u32;
    panel.place(
        side,
        life,
        0x11 + acel + if sheet.toad { 2 } else { 0 },
        0x29 + offset,
        0x3b,
        0x13,
        6,
        Payload::new(3, 0x31),
    );
    // Daggers: cel 0x15 at (0x26, 0x4e) every 0xa.
    panel.place(
        side,
        k.daggers,
        0x15,
        0x26 + offset,
        0x4e,
        0xa,
        0xa,
        Payload::new(3, 0x34),
    );
    // The weapon, cel `[si + 0x40]` at (0x2b, 0x5f), whose id is
    // `(cel - 0x16) * 2 + 0x14`. The sword of sharpness is the one that takes
    // `STRP` 1 and `STPL` 4, because it lives in the magic record.
    if let Some(n) = SWORDS.iter().position(|s| *s == k.weapon) {
        let cel = SWORD_CEL + n;
        let payload = if cel == 0x19 {
            Payload::new(1, 4)
        } else {
            Payload::new(2, 0x40)
        };
        panel.place(side, 1, cel, 0x2b + offset, 0x5f, 0, n * 2 + 0x14, payload);
    }
    // The armour, cel `[si + 0x42]` at (0x1d, 0x70), id `(cel - 0x1b) * 2 + 0xc`.
    if let Some(n) = ARMOURS.iter().position(|s| *s == k.armour) {
        panel.place(
            side,
            1,
            ARMOUR_CEL + n,
            0x1d + offset,
            0x70,
            0,
            n * 2 + 0xc,
            Payload::new(2, 0x42),
        );
    }
    // `push [si + 0x44]; pop [Address]` and fall into `DisplayMagic`.
    display_magic(panel, side, offset, &sheet.hoard);
}

/// `DisplayMagic` at `0xc38e` and `StatCheckKeys` after it, which it also falls
/// into.
///
/// Each of the four counted kinds clamps its own way and the clamps are not the
/// same: the gems and the talismans take `cmp cx, 4; jbe` and so show four, the
/// rings take `cmp cx, 4; jb` with `mov cx, 3` and so show three.
fn display_magic(panel: &mut Panel, side: usize, offset: i32, hoard: &Hoard) {
    // Gems: cel 9 at (0x57, 0xa4) every 0x10, id 0x26, `STRP` 5, `STPL` 2.
    panel.place(
        side,
        u32::from(hoard.gems).min(4),
        9,
        0x57 + offset,
        0xa4,
        0x10,
        0x26,
        Payload::new(5, 2),
    );
    // Rings: cel 3 at (0x21, 0x95) every 0xc, id 0x28, `STRP` 1, `STPL` 6.
    let rings = u32::from(hoard.rings);
    panel.place(
        side,
        if rings >= 4 { 3 } else { rings },
        3,
        0x21 + offset,
        0x95,
        0xc,
        0x28,
        Payload::new(1, 6),
    );
    // Talismans: cel 0xa at (0x45, 0x85) every 0x14, id 0x2a, `STRP` 1.
    panel.place(
        side,
        u32::from(hoard.talismans).min(4),
        0xa,
        0x45 + offset,
        0x85,
        0x14,
        0x2a,
        Payload::new(1, 8),
    );
    stat_check_keys(panel, side, offset, hoard);
    // Potions: cel 4 at (0x1f, 0xa5) every 0xd, id 0x24, `STRP` 5, `STPL` 0.
    panel.place(
        side,
        u32::from(hoard.potions).min(4),
        4,
        0x1f + offset,
        0xa5,
        0xd,
        0x24,
        Payload::new(5, 0),
    );
    stat_place_scrolls(panel, side, offset, hoard);
}

/// `StatCheckKeys` at `0xc44e`: the four keys of the Valley and then the
/// moonstones, one slot per bit.
///
/// The keys are cels 5 to 8 at x 0x4c, 0x5e, 0x70 and 0x82, eighteen apart, all
/// on row **0x6f**, one above the armour's 0x70, and each carries its own bit in
/// the high nibble of `STRP`. A slot left empty is a key you have not found.
///
/// The moonstones are all at x 0x67 on the same row, because a knight holds one
/// at a time: bit 1 is cel 2 and id 0x1c, bit 2 cel 1 and id 0x1e, bit 4 cel 0
/// and id 0x20, and bit 8 is cel 0 again with id 0x1c, which is the fourth stone
/// sharing the new moon's name. That is the same gap `moon::Moonstone` has.
fn stat_check_keys(panel: &mut Panel, side: usize, offset: i32, hoard: &Hoard) {
    for (bit, cel, x) in [
        (1u8, 5usize, 0x4c),
        (2, 6, 0x5e),
        (4, 7, 0x70),
        (8, 8, 0x82),
    ] {
        if hoard.keys & bit == 0 {
            continue;
        }
        panel.place(
            side,
            1,
            cel,
            x + offset,
            0x6f,
            0,
            0x22,
            Payload::new(u16::from(bit) << 4 | 1, 0x14),
        );
    }
    for (bit, cel, id) in [
        (1u8, 2usize, 0x1c),
        (2, 1, 0x1e),
        (4, 0, 0x20),
        (8, 0, 0x1c),
    ] {
        if hoard.moonstones & bit == 0 {
            continue;
        }
        panel.place(
            side,
            1,
            cel,
            0x67 + offset,
            0x6f,
            0,
            id,
            Payload::new(u16::from(bit) << 4 | 1, 0x16),
        );
    }
}

/// The five scroll fields and `StatPlaceScroll` at `0xc633`.
///
/// `SCROLLX` starts at 0x1e and `SCROLLCEL` at 0xb, and the walk is five fields
/// from `[si + 0xa]` upwards with `STID` from 0x2c stepping by two, which is
/// slots 22 to 26. **`SCROLLX` only advances when a scroll is actually drawn**,
/// because the `add word ptr [SCROLLX], 0x19` is inside `StatPlaceScroll`, so
/// the scrolls pack leftward along row 0xb7 rather than keeping fixed slots.
///
/// A second scroll of a kind is not a second scroll: `StatPlaceScroll` clamps
/// the count to two and draws cel 0x10, a four pixel sliver, eleven to the
/// right of the first, which is a stack seen edge on.
fn stat_place_scrolls(panel: &mut Panel, side: usize, offset: i32, hoard: &Hoard) {
    const SCROLL_Y: i32 = 0xb7;
    const SLIVER_CEL: usize = 0x10;
    let mut x = 0x1e;
    for (n, slot) in SCROLL_SLOTS.into_iter().enumerate() {
        let count = u32::from(hoard.scrolls[n]);
        if count == 0 {
            continue;
        }
        let payload = Payload::new(5, 0xa + n as u16 * 2);
        // `STID` starts at 0x2c and steps by two, which is twice the slot.
        let id = slot * 2;
        panel.place(side, 1, 0xb + n, x + offset, SCROLL_Y, 0, id, payload);
        // `cmp bp, 3; jb; mov bp, 2`: one or two, never three.
        if count >= 2 {
            panel.place(
                side,
                1,
                SLIVER_CEL,
                x + 0xb + offset,
                SCROLL_Y,
                4,
                id,
                payload,
            );
        }
        x += 0x19;
    }
}

/// `DisplayMSword` at `0xc70c`: the hoard's own sword of sharpness, cel 0x19 at
/// (0x2b, 0x5f), id 0x1a, which is slot 13.
fn display_msword(panel: &mut Panel, side: usize, offset: i32, hoard: &Hoard) {
    if !hoard.magic_sword {
        return;
    }
    panel.place(
        side,
        1,
        0x19,
        0x2b + offset,
        0x5f,
        0,
        0x1a,
        Payload::new(1, 4),
    );
}

/// `DisplayGold` at `0xc743`: cel 0x25 at (0x57, 0x40), id 8, and then
/// `"<n> gp."` at (0x5c, 0x5b). The four bytes after the number are `20 67 70
/// 2e 00`, a space, `g`, `p` and a full stop.
fn display_gold(panel: &mut Panel, side: usize, offset: i32, gold: u32) {
    panel.place(
        side,
        1,
        0x25,
        0x57 + offset,
        0x40,
        0,
        8,
        Payload::new(2, 0x32),
    );
    panel.words(format!("{gold} gp."), 0x5c + offset, 0x5b);
}

/// `DisplayMerchant` at `0xc7ad`: what is on the stall.
///
/// Five single goods and a row of thirteen daggers, all with `STPL` on the field
/// the purchase writes and `STRP`'s low nibble 0xa, which is `BuyGoods`. The
/// high nibble picks which of the three armours or two swords it is.
///
/// ```text
/// 0c7ad  cel 0x1c at (0x17, 0x91)  id 0x0e  STRP 0x1a  STPL 0x42   chain mail
/// 0c7d5  cel 0x1d at (0x40, 0x91)  id 0x10  STRP 0x2a  STPL 0x42   plate
/// 0c7fd  cel 0x1e at (0x6a, 0x90)  id 0x12  STRP 0x4a  STPL 0x42   battle
/// 0c825  cel 0x17 at (0x32, 0x6b)  id 0x16  STRP 0x1a  STPL 0x40   broad sword
/// 0c84d  cel 0x18 at (0x2f, 0x7d)  id 0x18  STRP 0x2a  STPL 0x40   claymore
/// 0c875  cel 0x15 at (0x20, 0x59)  id 0x0a  STRP 0x0a  STPL 0x34   thirteen
///        daggers, STI 9
/// ```
///
/// The ids are twice the slots whose `Purchase` lines carry the prices
/// `BuyArmour`, `BuyWeapon` and `BuyDagger` charge, so the line over the
/// gadget and the coin taken agree without a table between them.
pub fn display_merchant(panel: &mut Panel, side: usize, offset: i32) {
    for (cel, x, y, id, strp, field) in [
        (0x1cusize, 0x17i32, 0x91i32, 0x0eusize, 0x1au16, 0x42u16),
        (0x1d, 0x40, 0x91, 0x10, 0x2a, 0x42),
        (0x1e, 0x6a, 0x90, 0x12, 0x4a, 0x42),
        (0x17, 0x32, 0x6b, 0x16, 0x1a, 0x40),
        (0x18, 0x2f, 0x7d, 0x18, 0x2a, 0x40),
    ] {
        panel.place(
            side,
            1,
            cel,
            x + offset,
            y,
            0,
            id,
            Payload::new(strp, field),
        );
    }
    // Thirteen daggers, cel 0x15 at (0x20, 0x59) every nine, id 0xa.
    panel.place(
        side,
        13,
        0x15,
        0x20 + offset,
        0x59,
        9,
        0xa,
        Payload::new(0xa, 0x34),
    );
}

/// `DisplayLair` at `0xc67f`: three cels of a hoard on the floor, in the right
/// arch, and then the gold, the magic sword and the magic on top of them.
pub fn display_lair(panel: &mut Panel, side: usize, offset: i32, hoard: &Hoard, gold: u32) {
    for (cel, x, y) in [
        (0x1fusize, 0x3ai32, 0x2ei32),
        (0x20, 0x4c, 0x21),
        (0x21, 0x3a, 0x3c),
    ] {
        panel.plain.push((cel, x + offset, y, false));
    }
    if gold > 0 {
        display_gold(panel, side, offset, gold);
    }
    display_msword(panel, side, offset, hoard);
    display_magic(panel, side, offset, hoard);
}

/// `DisplayDragon` at `0xc6ea`: the hoard, the sword, and the gold if there is
/// any. No furniture of its own.
pub fn display_dragon(panel: &mut Panel, side: usize, offset: i32, hoard: &Hoard, gold: u32) {
    display_magic(panel, side, offset, hoard);
    display_msword(panel, side, offset, hoard);
    if gold > 0 {
        display_gold(panel, side, offset, gold);
    }
}

/// `DisplayAquire` at `0xc8ce`: one cel of the thing picked up at (0x92, 0x47)
/// and a twenty by twenty gadget over it whose text record is `NEXT`.
///
/// `HotGadget` recognises it by that record rather than by an id, so it is the
/// one gadget on the panel identified by what it says.
pub const ACQUIRE_AT: (i32, i32) = (0x92, 0x47);
pub const ACQUIRE_CEL: usize = 0x2c;
/// `NextKn` at `DS:0xed1f`, which is what `NEXT`'s record points at.
pub const NEXT_LINE: &str = "Select next knight";

/// The id this one is given here. `DisplayAquire` leaves `+0xe` zero and
/// `HotGadget` matches on the record instead, which a `String` cannot be matched
/// on as cheaply; the label is still `NEXT`'s own.
pub const NEXT_ID: usize = 0xffff;

pub fn display_acquire(panel: &mut Panel) {
    panel
        .plain
        .push((ACQUIRE_CEL, ACQUIRE_AT.0, ACQUIRE_AT.1, false));
    // `mov word ptr [si + 4], 0x14; mov word ptr [si + 6], 0x14`, twenty by
    // twenty, with `NEXT` as its text record.
    panel.extra.push(Gadget {
        id: NEXT_ID,
        x: ACQUIRE_AT.0,
        y: ACQUIRE_AT.1,
        w: 0x14,
        h: 0x14,
        label: NEXT_LINE.to_string(),
        payload: Payload::default(),
        lit: true,
    });
}

/// Lay the whole panel out, which is `ReDisplay` at `0xbe6b`.
///
/// `DisplayPillars` first, then `DisplayKnight` with `StatsOffset` at 0 or
/// `0x4a`, then `StatsOffset` to `0x96` and the type's own page in the other
/// arch. The plain sheet, the lair's floor, the merchant and the temple are
/// reached; the rest are here because they are one routine each and the
/// panel is the same panel.
pub fn lay_out(run: &Run, screen: Screen, other: Option<&Other>) -> Panel {
    let mut panel = Panel::new();
    panel.tables.push(screen.left_table());

    // `DisplayPillars`: `SingleData` alone for the two types in `OffsetValues`,
    // both tables for everything else, and `StatsOffset` with it.
    let offset = if screen.single_arch() {
        henge_core::status::SINGLE_OFFSET
    } else {
        0
    };
    if screen.single_arch() {
        panel.furniture(&SINGLE_DATA, offset);
    } else {
        panel.furniture(&TRADING_DATA, 0);
        panel.furniture(&SINGLE_DATA, 0);
    }

    // `SetUpID` 0xd39a: the three ability rows take `Inc`'s line and the 0x40
    // bit when `[ARR+18]` is no more than the knight's experience and the value
    // is under five. `Run::can_level` is that test.
    let can = run.can_level();
    let k = &run.knight;
    for (slot, value) in [k.strength, k.constitution, k.endurance].iter().enumerate() {
        let slot = match slot {
            0 => 0,
            1 => 1,
            _ => 2,
        };
        panel.raisable[slot] = can && *value < 5;
    }

    let name = if run.knight.named() {
        run.knight.name.clone()
    } else {
        "No knight".to_string()
    };
    display_knight(&mut panel, 0, offset, &KnightSheet::of_run(run), 0, &name);

    // `ReDisplay` 0xbeb6: `RESP` becomes `Response2` and `StatsOffset` 0x96
    // before the type's own page. The branches are the routine's, in its order.
    panel
        .tables
        .push(screen.right_table(other.is_some_and(|o| o.scouted)));
    let right = henge_core::status::RIGHT_OFFSET;
    let hoard = other.map_or_else(Hoard::default, |o| o.hoard);
    let gold = other.map_or(0, |o| o.gold);
    match screen {
        Screen::Lair => display_lair(&mut panel, 1, right, &hoard, gold),
        Screen::Dragon => display_dragon(&mut panel, 1, right, &hoard, gold),
        Screen::Merchant => display_merchant(&mut panel, 1, right),
        // `ReDisplay` 0xbf66: `mov word ptr [Address], 0xed96`, which is the
        // temple's own stock, then `DisplayMagic` and `DisplayMSword` on it.
        Screen::Temple => {
            display_magic(&mut panel, 1, right, &run.temple);
            display_msword(&mut panel, 1, right, &run.temple);
        }
        Screen::Acquire | Screen::AcquirePair => display_acquire(&mut panel),
        // `_displayknight`'s second arm: the loser of a knight fight, in the
        // other arch, `StatACEL` 1. `other.loser` names his record, off the
        // run's own `rivals`, since a duel's loser is always a computer
        // knight (`Run::challenge` never lets two of them fight).
        Screen::Trade => {
            if let Some(r) = other
                .and_then(|o| o.loser)
                .and_then(|idx| run.rivals.get(idx.wrapping_sub(1)))
            {
                let name = other
                    .and_then(|o| o.loser)
                    .map_or_else(String::new, |idx| run.record_name(idx));
                display_knight(&mut panel, 1, right, &KnightSheet::of_rival(r), 1, &name);
            }
        }
        // The stone circle's page is one arch and draws nothing on the right.
        Screen::Henge | Screen::Sheet => {}
    }
    panel
}

/// Whatever is in the other arch: a lair's floor, a dragon's hoard, the
/// mystic's stock, or a second knight. `StatMAGIC2` and the record's own
/// purse.
pub struct Other {
    pub hoard: Hoard,
    pub gold: u32,
    /// `EffectFLAG+4`, which `_STATUS:Paper2` at 0xd3ed reads before it picks
    /// the right arch's array: a lair looked at from the air gets `Identify`
    /// and so carries no permission bit, and nothing on the floor can be
    /// taken. See `henge_core::lair::Page`.
    pub scouted: bool,
    /// The trade page's second knight, by record index: `_displayknight`
    /// wants a second record, and now there is one. `None` for every other
    /// screen, none of which puts a second knight in the other arch.
    pub loser: Option<usize>,
}

/// The original's whole status screen, laid out and painted.
///
/// `said` is what the gadget under the pointer says, which `GadgetHit` draws as
/// a ten-byte record with `x` 0, `y` 3 and flags 3: centred, in the bold face.
pub fn draw_sheet(
    reg: &mut Registry,
    fb: &mut Framebuffer,
    font: Option<&Font>,
    bold: Option<&Font>,
    run: &Run,
    panel: &Panel,
    said: Option<&str>,
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

    panel.draw(reg, fb, font);

    // `GadgetHit` 0xd4f2: `mov si, es:[si + 8]` and the text record walker at
    // 0x7a86. `SetUpID` built every one of those records with x 0, y 3 and
    // flags 3, so the line is centred across the top of the screen in the bold
    // face and nowhere else.
    if let (Some(said), Some(bold)) = (said, bold.or(font)) {
        let w = bold.width(reg, said);
        bold.draw_own(reg, fb, said, (henge_core::SCREEN_W as i32 - w) / 2, 3);
    }
}
