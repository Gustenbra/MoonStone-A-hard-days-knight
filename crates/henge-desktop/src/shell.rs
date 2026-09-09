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
//! loaded with. **The copy is taken first**: the `rep movsw` at `0x8839` runs
//! before the three blits at `0x8850`, so what is kept is the bare picture.
//! `DoOptions` then calls `0x890c`, which loads `Sel.cel` and restores that
//! bare picture through `0x8e3f`, and `DisplaySelect` blits the wordmark
//! again at (5, 10) with the option list under it and nothing else. So the
//! option screen is the night sky, the wordmark and the four rows: the two
//! credit lines belong to the loading title only, which is the screen with
//! `created by`, `Rob Anderson` and `Loading...` on it. They were drawn here
//! too for a while, and `Select Knight` at its own y of 170 ran straight into
//! them, which is how the mistake showed itself.
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
//! `dc9`, `c95`, `832` at 9 to 12. So the wordmark and the four option rows
//! are blitted with their own indices, and the black bands that used to sit
//! behind them are gone.
//!
//! **There is no attract mode.** One was invented here and cycled ten of the
//! intro's files as though each were a picture; the note further down says why
//! it went and what it looked like. `DoOptions` sits on the title until
//! somebody presses something.
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
use crate::text::Font;
use henge_assets::Registry;
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
/// Frames 74 and 75, the copyright line and `All rights reserved`. The loader
/// puts them at (22, 181) and (110, 190) on the loading title, which henge
/// does not show: it has nothing to load. They are named so the numbers are
/// on record, and drawn nowhere.
#[allow(dead_code)]
const COPYRIGHT: usize = 74;
#[allow(dead_code)]
const RESERVED: usize = 75;

/// The plate the title is drawn over.
///
/// `_LOADER:MoonPic` is the string `CH.PIV`, and the routine at `0x87c3` loads
/// it, keeps a copy and draws the wordmark and the two credit lines on it.
const TITLE_PLATE: &str = "scene.ch";

// Ticks of nobody touching anything before the title gives up and starts
// showing off used to be a constant here.
//
// **There is no such thing.** `DoOptions` at `0x1241` sets its three counters
// up and then polls the input and dispatches, with no idle count, no timer and
// nowhere to go: the original's title screen simply sits there until somebody
// presses something. An attract mode was invented here and cycled ten of the
// intro's files as though each were a picture. Three of them are not pictures
// at all: `bg1a`, `bg1b` and `bg1c` are the tile sheets `INTRO.STI` arranges
// into the opening panorama, so showing one raw put half a moon above a row of
// trunks with a hard cut between them, which is what it looked like.
//
// Removed rather than repaired. Restoring it means a list of the seven plates
// that really are pictures and a counter, but it would still be ours.

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

        // No credit lines. `0x890c` restores the picture the loader copied
        // before it blitted them, and `DisplaySelect` adds only the wordmark,
        // the arrow and the six records.
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
/// seventy six. The width is the cel's own and nothing here needs it: the blit
/// takes it out of `SEL.CEL`'s header, and the boxes that used to be measured
/// against it were the gadgets this screen has none of.
const PORTRAIT_Y: i32 = 80;
const PORTRAIT_X: [i32; SEATS] = [12, 88, 164, 240];

/// `MOON:CRText`, the one message chain this screen puts up: `Select a Knight`,
/// flags 1, which is `TextPTop`'s centre bit, at y 5.
const HEADING: &str = "Select a Knight";
const SELECT_HEADING_Y: i32 = 5;

/// Where the name being typed over goes.
///
/// `ChooseRefresh` at 0x1697: `cmp word ptr [TypeFLAG], 0; je` past it, else
/// `mov si, [NAMEy]; mov ax, 0x32; mov bx, 0x32; xor cx, cx; call 0x7a70`.
/// That routine builds a one-record chain at `DS:0x7ff2` out of `si`, `ax`,
/// `bx` and `cx` and falls into the walker, so `ax` is the string's x, `bx` its
/// y and `cx` its flags: (50, 50), left aligned, in whatever font is current,
/// which on this screen is the bold one the heading is set in.
const NAME_AT: (i32, i32) = (50, 50);

pub struct SelectScene {
    pub state: Select,
    /// `SelectPAL`, the screen's own thirty two, out of the pack.
    palette: Vec<u32>,
}

impl SelectScene {
    pub fn new(reg: &Registry, players: usize) -> SelectScene {
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

    pub fn render(&self, reg: &mut Registry, fb: &mut Framebuffer, fonts: &Fonts) {
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
        // A whose-turn line, a name under each portrait, `Player N` in the gap
        // a taken knight left and a stat line along the bottom all used to be
        // here, and `ChooseRefresh` draws none of them. It clears the screen,
        // walks `CRText`, blits the portraits still free, blits the frame on
        // the chosen one, and draws the name being typed if `TypeFLAG` is set.
        // That is the whole routine: there is nothing else on this screen.
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
            }
            // A knight already taken leaves an empty space and nothing is put
            // in it: `ChooseRefresh`'s loop does `test byte ptr [choose_knight],
            // al; je` past the blit, and the routine has no second pass.
            if i == self.state.cursor && !self.state.done() {
                // The hollow frame, in its own pixels like everything else.
                // Every pixel of cel 1 is index 15, and `ChooseRefresh` blits
                // it at `CCOL[Chosen]` with the same `cx = 0x50` the portraits
                // get, so it lands exactly on the one it marks. This is the
                // entry `COLOURGLOW` breathes, and it is the only thing on the
                // screen that moves.
                sprite::draw(reg, fb, SEL, BORDER, x, PORTRAIT_Y, false);
            }
        }

        // The name being typed over, while `TypeFLAG` is set. One string at
        // (50, 50), left aligned, with `CURSOR` standing in the buffer at the
        // caret, which is what `Typing::shown` puts there.
        if let (Some(bold), Some(typing)) = (fonts.bold, self.state.typing.as_ref()) {
            bold.draw_own(reg, fb, &typing.shown(), NAME_AT.0, NAME_AT.1);
        }
    }
}

// Where the title's four rows and the select's four portraits are, as boxes a
// pointer can be over, used to be two functions here.
//
// **There are no such boxes.** Both screens had one per row so that a mouse
// could drive them, and neither screen in the original has a pointer on it at
// all: `DoOptions` at 0x1241 polls the stick itself (`test bx, 8` for up,
// `test bx, 4` for down, `test bx, 0x10` for fire) with no `CLEARGADGETS`, no
// `ADDGADGET` and no `CHECKGADGET` anywhere in it, and `ChooseLoop` at 0x15a0
// does the same. The six places that blit the pointer are listed on
// `draw_pointer`, and the title and the select are not among them.
//
// The y table and the portrait corners the boxes were built from are still
// above, where the drawing uses them.

/// The pointer, over whatever is drawn.
///
/// `PO.CEL` is one 16 by 18 frame. `SHOWPOINTER` is the routine at image 0xcf31:
///
/// ```text
/// push es
/// les si, ptr [0x892f]      ; PO.CEL's bank
/// sub ax, ax                ; cel 0
/// mov bx, [0xe492]          ; the pointer's x
/// mov cx, [0xe494]          ; and its y
/// call 0x5d7f               ; the cel blit every other sprite goes through
/// pop es
/// ret
/// ```
///
/// So it is **one plain blit of one cel in its own pixels**, at the pointer's own
/// coordinates with nothing subtracted, which is why its hot spot is its top
/// left corner and so is the point [`henge_core::pointer::Gadget::covers`]
/// tests. A flat silhouette with an eight-direction dark halo under it used to
/// stand here; `PO.CEL` is drawn artwork and the halo was invented to make a
/// flattened version of it read.
///
/// **And it is only on the screens that call this.** Scanning every call in the
/// image gives six: `MOON:WDLOOP+66` and `MOON:HWLOOP+66`, which are the two
/// town menus; `_TAVERN:TavernLoop+57`; `_WIZARD:DonateLoop+52`;
/// `_STATUS:StatLOOP+19`; and `_STATUS:FiDisplay+10`. `MovePointer` at 0xcead,
/// which is what moves it two pixels a tick, is called from `StatLOOP` alone.
/// The title and the select screens are not in either list.
pub fn draw_pointer(reg: &mut Registry, fb: &mut Framebuffer, p: &henge_core::pointer::Pointer) {
    if !p.woken {
        return;
    }
    sprite::draw(reg, fb, POINTER, 0, p.x, p.y, false);
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
/// **Nothing else is on it.** A day number, and one of the fourteen `WaitMES`
/// hints, used to be. The fourteen belong to `WAITMESSAGE`, which is a different
/// screen: `MESSAGE.PIV` with the chain over it, shown while a disk is read, and
/// its four callers are `PracticeCombat5`, `InitKnightvsDemon`, `SetUpDKL` and
/// `LoadWizard`. The routine at 0x8e5b walks `NextDayMes` and blits one cel, and
/// there is no second chain, no number and no note anywhere in it.
///
/// The heading is drawn in its own indices rather than flattened to one colour.
/// `TextP` hands a glyph to the same blitter every other cel goes through, and
/// `CH.PIV` carries the bold face's five entries: black at 5, then `fed`,
/// `dc9`, `c95`, `832` at 9 to 12. Flattened, `Next Day` came out as a row of
/// blobs with its counters closed.
const HEADING_Y: i32 = 95;

pub fn draw_interlude(
    reg: &mut Registry, fb: &mut Framebuffer, fonts: &Fonts, phase: henge_core::moon::Phase,
) {
    show(reg, fb, "scene.ch");
    sprite::draw(reg, fb, MOON_BANK, phase.cel(), MOON_AT.0, MOON_AT.1, false);
    if let Some(bold) = fonts.bold {
        bold.draw_own_centred(reg, fb, "Next Day", HEADING_Y);
    }
}
