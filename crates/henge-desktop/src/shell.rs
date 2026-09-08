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
//! **What it is drawn over is `CH.PIV`**, the same night sky the select screen
//! stands its knights against. `_LOADER:MoonPic` is the string `CH.PIV` and
//! `MoonFont` is `BOLD.F`; the routine at image `0x87c3` loads both, keeps a
//! copy of the picture in the segment at `DS:0x88fb`, and blits cel 0x49 at
//! (5, 20), cel 0x4a at (22, 181) and cel 0x4b at (110, 190). Those are the
//! wordmark and the two credit lines, at the coordinates the registers are
//! loaded with. `DoOptions` then calls `0x890c`, which loads `Sel.cel` and
//! restores that same picture through `0x8e3f`, and `DisplaySelect` blits the
//! wordmark again at (5, 10) with the option list under it. So the option
//! screen is the title screen with the wordmark ten pixels higher, and the
//! plate behind it was never an intro plate at all.
//!
//! The option list is recovered too, and now down to its words. `DoOptions` has
//! four rows, a player count of one to four, a gore switch and two ways to
//! start, and the arrow is `SEL.CEL` frame 0 at x 50, which is the `ARX`
//! `DoOptions` writes with `mov word ptr [ARX], 0x32`. Its four `y` values come
//! out of `MOON:ARR` at `DS:0x706`, which `DisplaySelect` indexes with
//! `optmode`: 85, 110, 148 and 168.
//!
//! The rows themselves are `MOON:OPT1a`, a chain of six ten-byte text records
//! that `DisplaySelect` hands to the message walker: `Players` and `Gore` in a
//! left column at x 86, the player count and `On`/`Off` in a right column at
//! x 214, and `Practice` and `Select Knight` centred below them. Both the
//! records and the strings were in the span of DGROUP the unpacker used to
//! leave stale. What stood here before was `Players N`, `Gore on`,
//! `Practice combat` and `Moon quest` on an even 18-pixel step, all four of
//! which were this project's own wording and this project's own spacing.
//!
//! **Nothing on it is flattened to one colour.** `CH.PIV` reserves the bold
//! face's five entries the way `MESSAGE.PIV` does: black at 5 and `dee`,
//! `dc9`, `c95`, `832` at 9 to 12. So the wordmark, the two credit lines and
//! the four option rows are all blitted with their own indices, and the black
//! bands that used to sit behind them are gone.
//!
//! **Attract mode** cycles the other ten plates. Those eleven screens have been
//! sitting in the pack unused since the baker first decoded them; showing them
//! is the whole of it.
//!
//! **The select, and it has no picture behind it.** This screen was drawn over
//! `CH.PIV` here for a long time and the original draws it over nothing at all.
//! `ChooseRefresh`'s first call, at `0x174e`, writes `0x0f02` to the sequencer
//! and then `rep stosw` of zero across `0x2000` words: it clears the screen to
//! palette entry 0, and the only picture the whole routine puts up is four
//! portraits. Then it loads `SelectPAL`, its own thirty two colours, out of the
//! data segment rather than out of a plate.
//!
//! `SEL.CEL` is the art: frame 0 an arrow, frame 1 a hollow frame and frames 2
//! to 5 the four knights. `ChooseRefresh` runs `bp` from 0 to 3, blits cel
//! `bp + 2` at `CCOL[bp]` with `cx = 0x50`, and then blits cel 1 at the chosen
//! knight's own x and the same y, so the frame lands exactly on the portrait it
//! marks. All five go through the ordinary cel blit at `0x5dc8`, with **no
//! colour substitution of any kind**: the portraits are painted in their own
//! indices, which is what `SelectPAL` is for. A knight already taken is not
//! drawn at all, which is also `ChooseRefresh`: it only draws the bits still
//! set in `choose_knight`.
//!
//! **The chosen knight glows, and nothing else does.** Cel 1 is 64 by 76 and
//! every one of its pixels is either transparent or index 15; no portrait
//! touches 15 and the cleared screen is entry 0, so on this screen entry 15 is
//! that frame and nothing else. `ChooseKnight` calls
//! `COLOURGLOW(0x0f, 0x088, 1, 0)` right after the fade, and `COLCON` swaps a
//! glow's two ends on arrival with repeat 0 meaning forever, so the frame
//! breathes between `SelectPAL`'s `0x066` and `0x088` for as long as the screen
//! is up. Ours used to glow the whole background instead, which is exactly what
//! happens when a recovered effect is aimed at a palette that is not the one it
//! was written for.

use crate::framebuffer::Framebuffer;
use crate::sprite;
use crate::status;
use crate::text::Font;
use henge_assets::Registry;
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
///
/// `_LOADER:MoonPic` is the string `CH.PIV`, and the routine at `0x87c3` loads
/// it, keeps a copy and draws the wordmark and the two credit lines on it.
const TITLE_PLATE: &str = "scene.ch";

/// Ticks of nobody touching anything before the title gives up and starts
/// showing off.
///
/// **There is no such thing.** `DoOptions` at `0x1241` sets its three counters
/// up and then polls the input and dispatches, with no idle count, no timer and
/// nowhere to go: the original's title screen simply sits there until somebody
/// presses something. An attract mode was invented here and cycled ten of the
/// intro's files as though each were a picture. Three of them are not pictures
/// at all: `bg1a`, `bg1b` and `bg1c` are the tile sheets `INTRO.STI` arranges
/// into the opening panorama, so showing one raw put half a moon above a row of
/// trunks with a hard cut between them, which is what it looked like.
///
/// Removed rather than repaired. Restoring it means a list of the seven plates
/// that really are pictures and a counter, but it would still be ours.

/// `ARX` in the original. The arrow's left edge on the option list.
const ARROW_X: i32 = 50;
/// `MOON:ARR` at `DS:0x706`. `DisplaySelect` does
/// `mov si, 0x706; mov ax, [optmode]; shl ax, 1; add si, ax; mov ax, [si]` and
/// blits `SEL.CEL` cel 0 at `ARX` with that as `cx`, so these four words are
/// the arrow's y on the four rows. They were in the span of DGROUP the unpacker
/// used to leave stale and are readable now.
const ARROW_Y: [i32; 4] = [85, 110, 148, 168];

/// The option list itself: `MOON:OPT1a`, six ten-byte text records chained
/// through their last word, which `DisplaySelect` hands to the message walker
/// at image `0x7a86`.
///
/// **Recovered, all of it**, out of the same span. A record is
/// `{text, x, y, flags, next}`, which is what that walker reads:
/// `mov dx, [bx]` is the string, `[bx+2]` and `[bx+4]` go into `TextX` and
/// `TextY`, `[bx+6]` is tested for bit 0 (centre between `TextLeftBorder` 0 and
/// `TextRightBorder` 320) and bit 2 (right-align), and `[bx+8]` is the next
/// record or zero.
///
/// ```text
/// OPT1a    Sel1     Players         x  86  y  83  flags 2
/// OPT1b    Sel2     Gore            x  86  y 108  flags 2
/// OPT1f    Sel5     Practice        x   0  y 150  flags 3
/// OPT1g    Sel6     Select Knight   x   0  y 170  flags 3
/// OPT1h    NPLAYER  "1          "   x 214  y  83  flags 2
/// GOREOPT  TEXTON   On              x 214  y 108  flags 2
/// ```
///
/// `DisplaySelect` prints the player count into `NPLAYER`'s buffer with the
/// decimal routine at `0x7d7f` and points `GOREOPT`'s first word at `TEXTOFF`
/// or `TEXTON` before it walks the chain, which is the whole of how the two
/// values on the right get there. Bit 1, which the left-hand four set, is not
/// read anywhere in the walker.
const ROW_LABEL: [&str; 4] = ["Players", "Gore", "Practice", "Select Knight"];
/// The labels are in `optmode` order, which is the order `Row::ALL` is in.
const _: () = assert!(ROW_LABEL.len() == Row::ALL.len());
const ROW_Y: [i32; 4] = [83, 108, 150, 170];
/// Flag bit 0. The bottom two rows are centred and their `x` is ignored.
const ROW_CENTRED: [bool; 4] = [false, false, true, true];
/// `OPT1a`/`OPT1b` `x`, and `OPT1h`/`GOREOPT` `x`: label column and value
/// column.
const LABEL_X: i32 = 86;
const VALUE_X: i32 = 214;
/// `MOON:TEXTON` and `MOON:TEXTOFF`. `DisplaySelect` writes `TEXTOFF` in and
/// then puts `TEXTON` back if the gore word is zero, so gore starts on, which
/// is what the word in the image holds.
const GORE_ON: &str = "On";
const GORE_OFF: &str = "Off";

/// `DisplaySelect`: `mov ax, 0x49; mov bx, 5; mov cx, 0xa`. The wordmark is
/// not centred; its left edge is five pixels in.
const LOGO_AT: (i32, i32) = (5, 10);
/// `0x8850` onwards, on the screen the same picture is first put up on:
/// `mov ax, 0x4a; mov bx, 0x16; mov cx, 0xb5` and then `0x4b` at (110, 190).
const COPYRIGHT_AT: (i32, i32) = (22, 181);
const RESERVED_AT: (i32, i32) = (110, 190);

#[derive(Default)]
pub struct TitleScene {
    pub state: Title,
}

impl TitleScene {
    fn plate(&self) -> &'static str {
        TITLE_PLATE
    }

    pub fn render(&self, reg: &mut Registry, fb: &mut Framebuffer, fonts: &Fonts) {
        let plate = self.plate();
        show(reg, fb, plate);
        // An attract plate is one of the intro's, and its palette owes the bold
        // face nothing. The intro itself solves that: `0x0cfb` writes the five
        // entries a glyph is drawn in before it puts a caption up, and those
        // are the five words in `CAPTION_INK`. `CH.PIV` already carries them,
        // so on the title proper this changes nothing.
        if plate != TITLE_PLATE {
            for (i, rgb) in henge_core::intro::CAPTION_INK {
                fb.palette[i as usize] = rgb;
            }
        }
        let (_, light) = status::extremes(fb);

        // In its own pixels, at its own corner. The wordmark is artwork with a
        // drawn outline and a shaded face; flattened to one colour all of that
        // went, and the black band and the four way outline were both invented
        // to make the silhouette read.
        sprite::draw(reg, fb, TITLE_BANK, LOGO, LOGO_AT.0, LOGO_AT.1, false);

        // The option list, at the coordinates its own records carry. The
        // arrow's left edge is `ARX`, which `DoOptions` sets to 50, and its y
        // is `ARR[optmode]`, which is a table of its own rather than the row's
        // own y: the top two rows sit two pixels above their arrow and the
        // bottom two two below it.
        let Some(bold) = fonts.bold else { return };
        // `Sel.cel` frame 0 in its own colours, the way `DisplaySelect` blits
        // it: one `call` with `ax` the cel and nothing said about colour.
        let row = self.state.row.min(ROW_LABEL.len() - 1);
        sprite::draw(reg, fb, SEL, ARROW, ARROW_X, ARROW_Y[row], false);
        for (i, label) in ROW_LABEL.iter().enumerate() {
            // In the glyphs' own five indices. The row the arrow is against is
            // the one that is chosen, which is the whole of how the original
            // says so; a second colour for it would be one more than the
            // original has.
            if ROW_CENTRED[i] {
                bold.draw_own_centred(reg, fb, label, ROW_Y[i]);
            } else {
                bold.draw_own(reg, fb, label, LABEL_X, ROW_Y[i]);
            }
        }
        // `OPT1h` and `GOREOPT`, the two records whose text `DisplaySelect`
        // rewrites before it walks the chain.
        bold.draw_own(reg, fb, &self.state.players.to_string(), VALUE_X, ROW_Y[0]);
        bold.draw_own(reg, fb, if self.state.gore { GORE_ON } else { GORE_OFF }, VALUE_X, ROW_Y[1]);

        // The original's own two credit lines, frames 74 and 75 of the same
        // bank, at the corners `0x8860` and `0x8870` load: (22, 181) and
        // (110, 190). In their own pixels like everything else on the screen.
        let _ = light;
        sprite::draw(reg, fb, TITLE_BANK, COPYRIGHT, COPYRIGHT_AT.0, COPYRIGHT_AT.1, false);
        sprite::draw(reg, fb, TITLE_BANK, RESERVED, RESERVED_AT.0, RESERVED_AT.1, false);
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

/// Where the four stand. `ChooseRefresh` loads `cx` with `0x50` for every one
/// of them and takes the x out of `MOON:CCOL`, four words at `DS:0x8d2`.
///
/// Both are recovered. `CCOL` is in the bottom of DGROUP, which the unpacker
/// used to leave stale and now does not, and it holds 12, 88, 164 and 240: a
/// twelve pixel margin on the left, sixty four wide portraits and a step of
/// seventy six.
const PORTRAIT_Y: i32 = 80;
const PORTRAIT_W: i32 = 64;
const PORTRAIT_X: [i32; SEATS] = [12, 88, 164, 240];

/// `MOON:CRText`, the one message chain this screen puts up: `Select a Knight`,
/// flags 1, which is `TextPTop`'s centre bit, at y 5.
const HEADING: &str = "Select a Knight";
const SELECT_HEADING_Y: i32 = 5;

pub struct SelectScene {
    pub state: Select,
    /// `SelectPAL`, the screen's own thirty two, out of the pack.
    palette: Vec<u32>,
}

impl SelectScene {
    pub fn new(reg: &Registry, players: usize, _knights: &Knights) -> SelectScene {
        // `palette.select` is `MOON:SelectPAL`, baked out of the executable.
        // A pack made before the baker could read it has none, and then the
        // screen falls back to the plate it used to stand on rather than
        // coming up in whatever the last screen was using.
        let palette = reg
            .palette("palette.select")
            .or_else(|| reg.palette("palette.scene.ch"))
            .map(|r| r.value.clone())
            .unwrap_or_default();
        SelectScene { state: Select::new(players), palette }
    }

    pub fn render(
        &self, reg: &mut Registry, fb: &mut Framebuffer, fonts: &Fonts, knights: &Knights,
        items: &Items,
    ) {
        // `ChooseRefresh`'s own first act: `mov ax, 0xf02; out dx, ax` to the
        // sequencer's map mask and `rep stosw` of zero over 0x2000 words, which
        // is every plane of every pixel set to palette entry 0. There is no
        // backdrop on this screen; there never was.
        fb.clear(0);
        fb.set_palette(&self.palette);

        // `MOON:CRText`, in the glyphs' own five indices. `SelectPAL` reserves
        // them the way `CH.PIV` and `MESSAGE.PIV` do: black at 5 and a warm
        // ramp at 9 to 12, which is why a bold line reads on a black screen.
        if let Some(bold) = fonts.bold {
            bold.draw_own_centred(reg, fb, HEADING, SELECT_HEADING_Y);
        }
        // **Ours**, and the only things on this screen that are: whose turn it
        // is, the name under each portrait, the `Player N` left where a taken
        // knight was, and the stat line along the bottom. The original draws
        // none of them. What it draws once a knight is taken is that knight's
        // name at (50, 50) out of `NAMEy`, which is a different thing again.
        if let Some(small) = fonts.small {
            let line = if self.state.done() {
                "Ride out".to_string()
            } else {
                format!("Player {}", self.state.seat + 1)
            };
            small.draw_own_centred(reg, fb, &line, 40);
        }

        for i in 0..SEATS {
            let x = PORTRAIT_X[i];
            if self.state.free(i) {
                // In its own pixels. `ChooseRefresh` puts the cel number in
                // `ax` and the corner in `bx` and `cx` and calls the same
                // blitter every other cel goes through; there is no ink and no
                // substitution anywhere in it. The four portraits are already
                // painted blue, gold, emerald and red in `SelectPAL`, and a
                // recolour on top of that was colouring coloured artwork.
                sprite::draw(reg, fb, SEL, FIRST_PORTRAIT + i, x, PORTRAIT_Y, false);
            } else {
                // Taken, so not drawn: `ChooseRefresh` only ever draws the bits
                // still set. Who took them goes in the empty slot instead.
                if let Some(small) = fonts.small {
                    let seat = (0..SEATS).find(|s| self.state.taken_by(*s) == Some(i));
                    if let Some(s) = seat {
                        let line = format!("Player {}", s + 1);
                        let w = small.width(reg, &line);
                        small.draw_own(reg, fb, &line, x + (PORTRAIT_W - w) / 2, PORTRAIT_Y + 32);
                    }
                }
            }
            if i == self.state.cursor && !self.state.done() {
                // The hollow frame, in its own pixels like everything else.
                // Every pixel of cel 1 is index 15, and `ChooseRefresh` blits
                // it at `CCOL[Chosen]` with the same `cx = 0x50` the portraits
                // get, so it lands exactly on the one it marks. This is the
                // entry `COLOURGLOW` breathes, and it is the only thing on the
                // screen that moves.
                sprite::draw(reg, fb, SEL, BORDER, x, PORTRAIT_Y, false);
            }
            if let (Some(small), Some(k)) = (fonts.small, knights.get(i)) {
                let w = small.width(reg, &k.name);
                small.draw_own(reg, fb, &k.name, x + (PORTRAIT_W - w) / 2, PORTRAIT_Y + 80);
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
        small.draw_own_centred(reg, fb, &line, 178);
    }
}

/// Where the title's four rows and the select's four portraits are, as boxes a
/// pointer can be over.
///
/// The numbers are the same ones the drawing uses, taken from one place so a
/// row can never be lit in one and hit in the other.
pub fn title_rects() -> Vec<(usize, i32, i32, i32, i32)> {
    // A row's box starts at the arrow that marks it and runs the width of the
    // list, tall enough for one line of the bold face. `ARR` and the records
    // put the four rows 25, 40 and 20 pixels apart, so the boxes do not meet.
    (0..ROW_LABEL.len())
        .map(|i| {
            let top = ROW_Y[i].min(ARROW_Y[i]) - 1;
            (i, ARROW_X, top, 260 - ARROW_X, 19)
        })
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
pub const MESSAGE_PLATE: &str = "scene.message";

/// One message, over the original's own message picture.
///
/// **Recovered:** the picture, the coordinates, the centring, and the bold face
/// every one of them is set in (`les si, ptr [0x8915]` before the chain walk,
/// which is the bold font pointer).
///
/// **Recovered too: the colour an instruction message comes up in.** The six
/// words `INSTRUCTMESSAGE` writes at `0x8f3b` go to `DS:0x80bb + 2` onwards,
/// and `DS:0x80bb` is the 32-entry palette the fade routine at the end of the
/// same call clocks out to the DAC three bytes at a time, ninety six of them.
/// So the words are palette entries 1 to 6, and an instruction repaints
/// `MESSAGE.PIV`'s four purples and the font's own black outline as a red ramp.
/// The picture goes red, which is the difference a player sees; nothing about
/// the lettering changes but the colour of the ring round it.
pub const INSTRUCT_RAMP: [u32; 6] = [0x880000, 0x660000, 0x440000, 0x000000, 0x220000, 0x110000];

pub fn draw_message(
    reg: &mut Registry, fb: &mut Framebuffer, fonts: &Fonts, msg: &henge_core::message::Message,
) {
    use henge_core::message::{Align, Kind};
    show(reg, fb, MESSAGE_PLATE);
    if msg.kind == Kind::Instruction {
        for (i, rgb) in INSTRUCT_RAMP.iter().enumerate() {
            fb.palette[i + 1] = *rgb;
        }
    }
    // Every message is set in the bold face, because every one of the three
    // routines does `les si, ptr [0x8915]` into the current font pointer before
    // it walks the chain, and `[0x8915]` is `BOLD.F`. The record's own bit 3 is
    // `CheckBOLD`'s kerning, not a choice of face.
    //
    // In the glyphs' own five indices, because `TextP` blits a glyph through
    // the same routine every other cel goes through and `MESSAGE.PIV` carries
    // `000`, `fed`, `dc9`, `b95`, `842` at exactly 5 and 9 to 12 and uses none
    // of them in its own picture. Flattened to one colour the ring round each
    // letter and the face inside it become the same colour, every counter fills
    // in, and a line reads as a row of blobs.
    let Some(font) = fonts.bold.or(fonts.small) else { return };
    for line in msg.shown() {
        match line.align {
            Align::Centre => font.draw_own_centred(reg, fb, &line.text, line.y),
            Align::Right => {
                let w = font.width(reg, &line.text);
                font.draw_own(reg, fb, &line.text, TEXT_RIGHT - w, line.y);
            }
            Align::Left => {
                font.draw_own(reg, fb, &line.text, line.x, line.y);
            }
        }
    }
}

/// `TextRightBorder`, which the alignment is measured against.
const TEXT_RIGHT: i32 = SCREEN_W as i32;


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
    // **Recovered, and the outline this used to draw is gone.** `BOLD.F`'s
    // glyphs are drawn in five indices: 5 is the ring round each letter and
    // its counters, 9 to 12 the bright face inside it. A silhouette paints the
    // ring and the face the same colour, closing every counter, which is why a
    // caption needed a halo to be read at all.
    //
    // The intro reserves those five entries and writes them itself: `0x0cfb`
    // puts the ring back to black and the face to the ramp
    // `0xfed, 0xdc9, 0xb95, 0x842`. `MESSAGE.PIV` carries the same five words
    // in its own palette, which is what makes the game's messages legible over
    // it, and the panorama the credits go over never uses 9 to 12 at all, so
    // writing them there changes nothing but the lettering.
    for (i, rgb) in henge_core::intro::CAPTION_INK {
        fb.palette[i as usize] = rgb;
    }
    // The intro sets its captions in the bold face, as the message system does.
    let Some(font) = fonts.bold.or(fonts.small) else { return };
    for line in step.lines {
        font.draw_own_centred(reg, fb, line.text, line.y);
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
/// the heading takes its recovered 95 and the hint follows underneath, because
/// both cannot have y 95 and a bold line is nineteen pixels tall. The
/// `Loading...` line every one of the fourteen ends on is left out, because
/// nothing here loads.
///
/// Every line is drawn in its own indices rather than flattened to one colour.
/// `TextP` hands a glyph to the same blitter every other cel goes through, and
/// `CH.PIV` carries the bold face's five entries: black at 5, then `fed`,
/// `dc9`, `c95`, `832` at 9 to 12. Flattened, `Next Day` came out as a row of
/// blobs with its counters closed.
const HEADING_Y: i32 = 95;
const DAY_Y: i32 = 118;
const HINT_Y: i32 = 136;
const HINT_STEP: i32 = 14;

pub fn draw_interlude(
    reg: &mut Registry, fb: &mut Framebuffer, fonts: &Fonts, day: u32, phase: henge_core::moon::Phase,
    hint: &henge_core::message::Message, note: Option<&str>,
) {
    show(reg, fb, "scene.ch");
    sprite::draw(reg, fb, MOON_BANK, phase.cel(), MOON_AT.0, MOON_AT.1, false);
    if let Some(bold) = fonts.bold {
        bold.draw_own_centred(reg, fb, "Next Day", HEADING_Y);
    }
    let Some(small) = fonts.small else { return };
    small.draw_own_centred(reg, fb, &format!("Day {day}   {}", phase.name()), DAY_Y);
    let mut y = HINT_Y;
    // A day the run had no say in says so, in the hint's place: being turned
    // into a toad and losing three turns is the sort of thing a player has to
    // be told about, and this is the screen those three days go past on.
    if let Some(note) = note {
        small.draw_own_centred(reg, fb, note, y);
        return;
    }
    for line in hint.shown() {
        small.draw_own_centred(reg, fb, &line.text, y);
        y += HINT_STEP;
    }
}
