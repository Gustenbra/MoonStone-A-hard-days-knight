//! The two screens in front of the game.
//!
//! All the rules are in `henge_core::shell`; this file knows only where the
//! pixels go.
//!
//! **The title.** Its artwork was hiding in the font.
//!
//! `BOLD.F` has 76 frames and the glyph map only ever used 66 of them. The last
//! three are not glyphs at all: frame 73 is the 305 by 54 `Moonstone / A Hard
//! Days Knight` wordmark, frame 74 the copyright line and frame 75 `All rights
//! reserved`. That is the original's title screen, and this project had decoded
//! it on the first day and never drawn it.
//!
//! What it was drawn *over* is still unknown. `LOADTITLE` is a name in the
//! symbol list and `TitleMes` and its neighbours are text records, but the
//! picture behind them lives in `INTR.EXE`, which has never been examined, so
//! ours goes over one of the eleven intro plates that are on disk. The option
//! list is recovered: `DoOptions` has four rows, a player count of one to four,
//! a gore switch and two ways to start, and the arrow is `SEL.CEL` frame 0 at
//! x 50, which is the `ARX` the original uses.
//!
//! **Attract mode** cycles the other ten plates. Those eleven screens have been
//! sitting in the pack unused since the baker first decoded them; showing them
//! is the whole of it.
//!
//! **The select.** `CH.PIV` is the backdrop, `SEL.CEL` the art: frame 0 an
//! arrow, frame 1 a hollow border and frames 2 to 5 the four knights. The
//! knights stand at y 80, which is `ChooseRefresh`'s own `cx`, and the border
//! goes round whichever is highlighted. A knight already taken is not drawn at
//! all, which is also `ChooseRefresh`: it only draws the bits still set in
//! `choose_knight`.
//!
//! **The colours are recovered and the palette is not.** `CH.PIV` carries
//! sixteen colours and the portraits index up to twenty-eight, so the top half
//! of the select palette comes from `SelectPAL`, whose bytes did not survive
//! into our unpacked image. What did survive is better: `KnightGlowColours`
//! holds each knight's three shades, so the top sixteen entries are built as
//! four four-step ramps from those, and each portrait is drawn through a
//! substitution into its own ramp. The knight who is blue in the original is
//! blue here because the original says so, not because the palette happened to
//! have a blue in it.

use crate::framebuffer::Framebuffer;
use crate::sprite;
use crate::status;
use crate::text::Font;
use henge_assets::{recolour, Lut, Registry};
use henge_core::item::Items;
use henge_core::knight::Knights;
use henge_core::shell::{Row, Select, Title, SEATS};
use henge_core::{SCREEN_H, SCREEN_W};

/// The select art, and the arrow the title borrows from it.
const SEL: &str = "bank.sel";
const ARROW: usize = 0;
const BORDER: usize = 1;
const FIRST_PORTRAIT: usize = 2;

/// The title artwork, at the end of the bold font's bank where the glyph map
/// stops.
const TITLE_BANK: &str = "bank.bold";
const LOGO: usize = 73;
const COPYRIGHT: usize = 74;
const RESERVED: usize = 75;

/// The plate the title is drawn over, and the ones attract mode cycles.
const TITLE_PLATE: &str = "scene.bg2a";
const ATTRACT: [&str; 10] = [
    "scene.bg1a", "scene.bg1b", "scene.bg1c", "scene.bg2", "scene.bg3",
    "scene.bg4", "scene.bg5", "scene.bg5a", "scene.bg7", "scene.bg8",
];

/// Ticks of nobody touching anything before the title gives up and starts
/// showing off, and how long each plate stays.
const ATTRACT_AFTER: u32 = 420;
const PLATE_TICKS: u32 = 220;

/// `ARX` in the original. The arrow's left edge on the option list.
const ARROW_X: i32 = 50;
const FIRST_ROW_Y: i32 = 100;
const ROW_STEP: i32 = 18;

pub struct TitleScene {
    pub state: Title,
    /// Ticks since anyone pressed anything.
    idle: u32,
}

impl Default for TitleScene {
    fn default() -> TitleScene {
        TitleScene { state: Title::default(), idle: 0 }
    }
}

impl TitleScene {
    pub fn touched(&mut self) {
        self.idle = 0;
    }

    pub fn tick(&mut self) {
        self.idle = self.idle.saturating_add(1);
    }

    /// Is the title showing off rather than waiting?
    pub fn attracting(&self) -> bool {
        self.idle >= ATTRACT_AFTER
    }

    fn plate(&self) -> &'static str {
        if !self.attracting() {
            return TITLE_PLATE;
        }
        let n = ((self.idle - ATTRACT_AFTER) / PLATE_TICKS) as usize;
        ATTRACT[n % ATTRACT.len()]
    }

    pub fn render(&self, reg: &mut Registry, fb: &mut Framebuffer, fonts: &Fonts) {
        let plate = self.plate();
        show(reg, fb, plate);
        let (dark, light) = status::extremes(fb);
        let faint = status::faint(fb);

        // The wordmark, on a plate of its own so it reads over eleven different
        // pictures rather than over one. It is drawn as a silhouette with a halo
        // under it, for the reason every other sprite over a foreign palette is:
        // its own indices mean nothing here.
        let (lw, lh) = sprite::size(reg, TITLE_BANK, LOGO);
        let lx = (SCREEN_W as i32 - lw) / 2;
        fb.rect(0, 6, SCREEN_W as i32, lh + 10, dark);
        for (ox, oy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            sprite::draw_mask(reg, fb, TITLE_BANK, LOGO, lx + ox, 10 + oy, faint);
        }
        sprite::draw_mask(reg, fb, TITLE_BANK, LOGO, lx, 10, light);

        if self.attracting() {
            if let Some(small) = fonts.small {
                small.draw_centred(reg, fb, "Press fire", 180, light);
            }
            return;
        }

        // The option list. A plate behind it for the same reason. The arrow's
        // left edge is `ARX`, which the original sets to 50; the words follow
        // it.
        let rows = self.rows();
        let (aw, _) = sprite::size(reg, SEL, ARROW);
        let text_x = ARROW_X + aw + 6;
        // Down to the credit band, so no strip of picture is left between them.
        fb.rect(ARROW_X - 8, FIRST_ROW_Y - 8, 308 - ARROW_X, 184 - FIRST_ROW_Y, dark);
        let Some(bold) = fonts.bold else { return };
        for (i, label) in rows.iter().enumerate() {
            let y = FIRST_ROW_Y + i as i32 * ROW_STEP;
            let on = i == self.state.row;
            if on {
                sprite::draw_mask(reg, fb, SEL, ARROW, ARROW_X, y + 3, light);
            }
            bold.draw(reg, fb, label, text_x, y, if on { light } else { faint });
        }

        // The original's own two credit lines, which are frames 74 and 75 of the
        // same bank. They belong on this screen because it is this screen they
        // were drawn for, and they are the accurate statement of whose game the
        // artwork on it is.
        fb.rect(0, 176, SCREEN_W as i32, 24, dark);
        for (cel, y) in [(COPYRIGHT, 180), (RESERVED, 190)] {
            let (w, _) = sprite::size(reg, TITLE_BANK, cel);
            sprite::draw_mask(reg, fb, TITLE_BANK, cel, (SCREEN_W as i32 - w) / 2, y, light);
        }
    }

    fn rows(&self) -> Vec<String> {
        Row::ALL
            .iter()
            .map(|r| match r {
                Row::Players => format!("Players  {}", self.state.players),
                Row::Gore => format!("Gore  {}", if self.state.gore { "on" } else { "off" }),
                Row::Practice => "Practice combat".to_string(),
                Row::Quest => "Moon quest".to_string(),
            })
            .collect()
    }
}

/// Whichever fonts the packs happen to hold. Both are optional everywhere else
/// in this codebase and stay optional here.
pub struct Fonts<'a> {
    pub bold: Option<&'a Font>,
    pub small: Option<&'a Font>,
}

fn show(reg: &mut Registry, fb: &mut Framebuffer, scene: &str) {
    if let Some(p) = reg.palette(&format!("palette.{scene}")).map(|r| r.value.clone()) {
        fb.set_palette(&p);
    }
    match reg.image(scene) {
        Ok(img) if img.width == SCREEN_W && img.height == SCREEN_H => {
            fb.pixels.copy_from_slice(&img.pixels)
        }
        _ => fb.clear(0),
    }
}

/// Where the four stand. `ChooseRefresh` puts them at y 80 and takes the x from
/// a table whose bytes are lost, so they are spread evenly instead.
const PORTRAIT_Y: i32 = 80;
const PORTRAIT_W: i32 = 64;
const PORTRAIT_X: [i32; SEATS] = [8, 88, 168, 248];

pub struct SelectScene {
    pub state: Select,
    /// Sixteen from `CH.PIV` and sixteen built from the recovered knight
    /// colours.
    palette: Vec<u32>,
    /// One substitution per knight, into that knight's four entries.
    luts: [Lut; SEATS],
}

impl SelectScene {
    pub fn new(reg: &Registry, players: usize, knights: &Knights) -> SelectScene {
        let mut palette = reg
            .palette("palette.scene.ch")
            .map(|r| r.value.clone())
            .unwrap_or_default();
        palette.resize(16, 0);
        for i in 0..SEATS {
            let shade = knights.get(i).and_then(|k| k.shades.first().copied()).unwrap_or(0x808080);
            palette.extend_from_slice(&recolour::ramp(shade));
        }
        let luts = std::array::from_fn(|i| {
            let bucket: Vec<usize> = (0..4).map(|j| 16 + i * 4 + j).collect();
            recolour::map_into_bucket(&palette, &bucket)
        });
        SelectScene { state: Select::new(players), palette, luts }
    }

    pub fn render(
        &self, reg: &mut Registry, fb: &mut Framebuffer, fonts: &Fonts, knights: &Knights,
        items: &Items,
    ) {
        show(reg, fb, "scene.ch");
        fb.set_palette(&self.palette);
        let (dark, light) = status::extremes(fb);

        fb.rect(0, 12, SCREEN_W as i32, 24, dark);
        if let Some(bold) = fonts.bold {
            bold.draw_centred(reg, fb, "Choose your knight", 16, light);
        }
        if let Some(small) = fonts.small {
            let line = if self.state.done() {
                "Ride out".to_string()
            } else {
                format!("Player {}", self.state.seat + 1)
            };
            small.draw_centred(reg, fb, &line, 40, light);
        }

        for i in 0..SEATS {
            let x = PORTRAIT_X[i];
            if self.state.free(i) {
                sprite::draw_lut(reg, fb, SEL, FIRST_PORTRAIT + i, x, PORTRAIT_Y, &self.luts[i]);
            } else {
                // Taken, so not drawn: `ChooseRefresh` only ever draws the bits
                // still set. Who took them goes in the empty slot instead.
                if let Some(small) = fonts.small {
                    let seat = (0..SEATS).find(|s| self.state.taken_by(*s) == Some(i));
                    if let Some(s) = seat {
                        let line = format!("Player {}", s + 1);
                        let w = small.width(reg, &line);
                        small.draw(reg, fb, &line, x + (PORTRAIT_W - w) / 2, PORTRAIT_Y + 32, light);
                    }
                }
            }
            let colour = (16 + i * 4 + 3) as u8;
            if i == self.state.cursor && !self.state.done() {
                // In the brightest colour the screen has rather than the
                // knight's own: every portrait already carries a coloured frame
                // of its own, so a highlight in one more colour would be one
                // frame among five instead of the answer to which is chosen.
                sprite::draw_mask(reg, fb, SEL, BORDER, x, PORTRAIT_Y, light);
            }
            if let (Some(small), Some(k)) = (fonts.small, knights.get(i)) {
                let w = small.width(reg, &k.name);
                small.draw(reg, fb, &k.name, x + (PORTRAIT_W - w) / 2, PORTRAIT_Y + 80, colour);
            }
        }

        // The highlighted knight's block, along the bottom. Four stat blocks at
        // once would not fit under a 64-pixel portrait, and this is the one the
        // player is deciding about.
        let Some(small) = fonts.small else { return };
        let Some(k) = knights.get(self.state.cursor) else { return };
        let live = henge_core::knight::Knight::from_def(k, self.state.cursor);
        let line = format!(
            "Str {}   Con {}   End {}   {} health   {} gold   {}",
            k.strength,
            k.constitution,
            k.endurance,
            live.max_health(items),
            k.gold,
            live.weapon_name(items),
        );
        fb.rect(0, 174, SCREEN_W as i32, 14, dark);
        small.draw_centred(reg, fb, &line, 178, light);
    }
}

/// Where the title's four rows and the select's four portraits are, as boxes a
/// pointer can be over.
///
/// The numbers are the same ones the drawing uses, taken from one place so a
/// row can never be lit in one and hit in the other.
pub fn title_rects() -> Vec<(usize, i32, i32, i32, i32)> {
    (0..Row::ALL.len())
        .map(|i| (i, ARROW_X, FIRST_ROW_Y + i as i32 * ROW_STEP - 2, 260 - ARROW_X, ROW_STEP))
        .collect()
}

pub fn select_rects() -> Vec<(usize, i32, i32, i32, i32)> {
    (0..SEATS).map(|i| (i, PORTRAIT_X[i], PORTRAIT_Y, PORTRAIT_W, 80)).collect()
}

/// The pointer, over whatever is drawn.
///
/// `PO.CEL` is one 16 by 18 frame, and the packs have carried it decoded and
/// unused since the first day; `SHOWPOINTER` blits it at the pointer's own
/// coordinates with nothing subtracted, so its hot spot is its top left corner
/// and so is the point [`henge_core::pointer::Gadget::covers`] tests.
///
/// Drawn as a silhouette with a halo under it, for the reason every other
/// sprite over a foreign palette is: the pointer has to read over a sunlit town
/// and over a stone circle at midnight, and its own indices mean nothing in
/// either.
pub fn draw_pointer(reg: &mut Registry, fb: &mut Framebuffer, p: &henge_core::pointer::Pointer) {
    if !p.woken {
        return;
    }
    let (dark, light) = status::extremes(fb);
    // Outlined in the darkest colour the screen has rather than a middle one:
    // the arrow is sixteen pixels wide and has to read over a knight in white
    // armour as well as over a night sky.
    for (ox, oy) in [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, 1), (-1, 1), (1, -1)] {
        sprite::draw_mask(reg, fb, POINTER, 0, p.x + ox, p.y + oy, dark);
    }
    sprite::draw_mask(reg, fb, POINTER, 0, p.x, p.y, light);
}

/// `PO.CEL`, the original's pointer.
const POINTER: &str = "bank.po";

/// `MESSAGE.PIV`: the stone circle in silhouette against a night sky, black
/// under it, which is what all three kinds of message are drawn over.
const MESSAGE_PLATE: &str = "scene.message";

/// One message, over the original's own message picture.
///
/// **Recovered:** the picture, the coordinates, the centring, and the bold face
/// every one of them is set in (`les si, ptr [0x8915]` before the chain walk,
/// which is the bold font pointer).
///
/// **Ours:** the colour an instruction message comes up in. `INSTRUCTMESSAGE`
/// installs its own six-word palette ramp (0x800, 0x600, 0x400, 0, 0x200,
/// 0x100) before it fades, and the fade machinery those words drive is not
/// built, so the difference between the kinds is made here instead: an
/// instruction is written in the picture's own brightest colour and the other
/// two in white, which is the same distinction the ramp draws.
pub fn draw_message(
    reg: &mut Registry, fb: &mut Framebuffer, fonts: &Fonts, msg: &henge_core::message::Message,
) {
    use henge_core::message::{Align, Kind};
    show(reg, fb, MESSAGE_PLATE);
    let (_, light) = status::extremes(fb);
    // An instruction message is the one that arrives in another colour.
    let ink = match msg.kind {
        Kind::Instruction => warmest(fb),
        _ => light,
    };
    // Every message is set in the bold face, because every one of the three
    // routines does `les si, ptr [0x8915]` into the current font pointer before
    // it walks the chain, and `[0x8915]` is `BOLD.F`. The record's own bit 3 is
    // `CheckBOLD`'s kerning, not a choice of face.
    let Some(font) = fonts.bold.or(fonts.small) else { return };
    for line in msg.shown() {
        match line.align {
            Align::Centre => font.draw_centred(reg, fb, &line.text, line.y, ink),
            Align::Right => {
                let w = font.width(reg, &line.text);
                font.draw(reg, fb, &line.text, TEXT_RIGHT - w, line.y, ink);
            }
            Align::Left => {
                font.draw(reg, fb, &line.text, line.x, line.y, ink);
            }
        }
    }
}

/// `TextRightBorder`, which the alignment is measured against.
const TEXT_RIGHT: i32 = SCREEN_W as i32;

/// The most saturated colour the palette has, for an instruction message.
fn warmest(fb: &Framebuffer) -> u8 {
    (1..32)
        .max_by_key(|i| {
            let c = fb.palette[*i];
            let (r, g, b) = ((c >> 16) & 0xff, (c >> 8) & 0xff, c & 0xff);
            let hi = r.max(g).max(b);
            let lo = r.min(g).min(b);
            (hi - lo) * 4 + hi
        })
        .unwrap_or(1) as u8
}

/// The intro, as `INTR.EXE`'s own main module plays it.
///
/// All of it is the original's now: the publisher's logo, the vertical pan
/// down the panorama its `.STI` tile map describes, the seven credit screens
/// the loader steps through, the plates in the order the scene routines hand
/// them to the blitter, the cast animating on the intro's own scripts, and
/// every caption at the `y` its own ten-byte record carries. `henge_core::intro`
/// says what was recovered and what little is ours.
///
/// Nothing here draws a box behind a caption. It used to, because the
/// coordinates were invented and a line had to be made readable wherever it
/// landed; the recovered story cards are drawn over `MESSAGE.PIV`, which is
/// what the original puts behind them.
pub fn draw_intro(
    reg: &mut Registry, fb: &mut Framebuffer, fonts: &Fonts, intro: &henge_core::intro::Intro,
    cast: Option<&henge_core::content::IntroCast>,
) {
    use henge_core::intro::Backdrop;
    let Some(step) = intro.showing() else { return };
    match step.back {
        Backdrop::Logo => show(reg, fb, LOGO_PLATE),
        Backdrop::Message => show(reg, fb, MESSAGE_PLATE),
        Backdrop::Plate(plate) => show(reg, fb, plate),
        Backdrop::Pan { .. } => draw_pan(reg, fb, intro.pan as i32),
    }

    if let Some(cast) = cast {
        draw_cast(reg, fb, intro, cast);
    }

    // `0x0c6a` blits `BOLD.F`'s wordmark cel before it draws the publisher's
    // card, at the literal (9, 60) the registers are loaded with.
    if step.wordmark {
        sprite::draw(
            reg, fb, TITLE_BANK, henge_core::intro::WORDMARK_CEL,
            henge_core::intro::WORDMARK_AT.0, henge_core::intro::WORDMARK_AT.1, false,
        );
    }

    if step.lines.is_empty() {
        return;
    }
    let (dark, light) = status::extremes(fb);
    // The intro sets its captions in the bold face, as the message system does.
    let Some(font) = fonts.bold.or(fonts.small) else { return };
    for line in step.lines {
        // **Ours: the outline.** The original's glyphs keep their own shading,
        // and `0x0cb0` turns the four entries that shading uses pure white
        // while a caption is up and dims them to a grey ramp afterwards. This
        // engine draws a glyph as a silhouette in one colour on purpose, so a
        // white caption over the white moon these are drawn on would be
        // unreadable. Ringing it in the picture's own darkest entry is what
        // stands in for the shading, and it is the same trick the pointer uses.
        for (ox, oy) in [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, 1), (-1, 1), (1, -1)] {
            let w = font.width(reg, line.text);
            font.draw(reg, fb, line.text, (SCREEN_W as i32 - w) / 2 + ox, line.y + oy, dark);
        }
        font.draw_centred(reg, fb, line.text, line.y, light);
    }
}

/// `MINDSCAP`, the publisher's logo. A PIV with no extension, which is the
/// only reason nothing had ever baked it.
const LOGO_PLATE: &str = "scene.mindscap";

/// The panorama, 200 rows of it.
///
/// The original never holds this picture anywhere: it stamps 32x25 tiles
/// straight into video memory and scrolls what is already there, two tile rows
/// at a time. The picture is composited whole at bake time instead, because a
/// framebuffer this engine can blit from costs 384KB and a tile engine over
/// mode X costs a mode X.
fn draw_pan(reg: &mut Registry, fb: &mut Framebuffer, top: i32) {
    if let Some(p) = reg.palette("palette.scene.intropan").map(|r| r.value.clone()) {
        fb.set_palette(&p);
    }
    let Ok(img) = reg.image(PAN_SHEET) else {
        fb.clear(0);
        return;
    };
    let top = top.clamp(0, (img.height as i32 - SCREEN_H as i32).max(0)) as usize;
    for row in 0..SCREEN_H {
        let src = (top + row) * img.width;
        let dst = row * SCREEN_W;
        if src + SCREEN_W <= img.pixels.len() {
            fb.pixels[dst..dst + SCREEN_W].copy_from_slice(&img.pixels[src..src + SCREEN_W]);
        }
    }
}

const PAN_SHEET: &str = "scene.intropan";

/// Everything the step has put on the plate.
///
/// A figure is placed the way the task VM places one: x from the task's own,
/// or mirrored about it, and y from the task's y plus its z. The two starters
/// differ only in x and facing, and their numbers are `henge_core::intro`'s.
fn draw_cast(
    reg: &mut Registry, fb: &mut Framebuffer, intro: &henge_core::intro::Intro,
    cast: &henge_core::content::IntroCast,
) {
    use henge_core::intro::{SPAWN_LEFT_X, SPAWN_RIGHT_X, SPAWN_Y, SPAWN_Z};
    let Some(step) = intro.showing() else { return };
    let now = intro.frame();
    for spawn in step.cast {
        if now < spawn.at {
            continue;
        }
        let Some(frame) = cast.frame_at(spawn.script, now - spawn.at) else { continue };
        let ox = if spawn.left { SPAWN_LEFT_X } else { SPAWN_RIGHT_X };
        for part in &frame.parts {
            let Some(sheet) = cast.banks.get(part.bank as usize).cloned() else { continue };
            let Some(cut) = sprite::cut(reg, &sheet, part.cel as usize) else { continue };
            let x = if spawn.left {
                ox - (part.x as i32 + cut.w as i32)
            } else {
                ox + part.x as i32
            };
            let y = SPAWN_Y + SPAWN_Z + part.y as i32;
            fb.blit(&cut.pixels, cut.w, cut.h, x, y, spawn.left);
        }
    }
}

/// `KI.CEL`, whose cels 0x2d to 0x31 are the five moons.
const MOON_BANK: &str = "bank.ki";
/// Where the `Next Day` routine at image 0x8e5b blits tonight's moon:
/// `mov bx, 0x77; mov cx, 0xc`.
const MOON_AT: (i32, i32) = (119, 12);

/// The screen between one day and the next.
///
/// **Recovered:** the backdrop, the moon and its corner, and the heading. The
/// routine at 0x8e5b draws `NextDayMes`, whose string is `NDM` (`Next Day`) at
/// y 95, and then blits cel `[0x8989]` of `KI.CEL` at (119, 12) over the night
/// sky the select screen also uses. `[0x8989]` is `Moons[MoonCount]`, which is
/// tonight's phase.
///
/// **Ours:** the day number under the heading, and the hint below it. The hint
/// is one of `WAITMESSAGE`'s fourteen, taken through `henge_core::message`, and
/// the original's own hint screen has its lines at y 75, 95, 115 and 135; here
/// the heading keeps its recovered 95 and the hint takes the same four slots
/// starting one below it, because both cannot have y 95. The `Loading...` line
/// every one of the fourteen ends on is left out, because nothing here loads.
pub fn draw_interlude(
    reg: &mut Registry, fb: &mut Framebuffer, fonts: &Fonts, day: u32, phase: henge_core::moon::Phase,
    hint: &henge_core::message::Message, note: Option<&str>,
) {
    show(reg, fb, "scene.ch");
    sprite::draw(reg, fb, MOON_BANK, phase.cel(), MOON_AT.0, MOON_AT.1, false);
    let (_, light) = status::extremes(fb);
    if let Some(bold) = fonts.bold {
        bold.draw_centred(reg, fb, "Next Day", 88, light);
    }
    let Some(small) = fonts.small else { return };
    small.draw_centred(reg, fb, &format!("Day {day}   {}", phase.name()), 112, light);
    let mut y = 132;
    // A day the run had no say in says so, in the hint's place: being turned
    // into a toad and losing three turns is the sort of thing a player has to
    // be told about, and this is the screen those three days go past on.
    if let Some(note) = note {
        small.draw_centred(reg, fb, note, y, light);
        return;
    }
    for line in hint.shown() {
        small.draw_centred(reg, fb, &line.text, y, light);
        y += 14;
    }
}
