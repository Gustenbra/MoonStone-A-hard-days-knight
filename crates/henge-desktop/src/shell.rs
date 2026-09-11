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
const ARROW_Y: [i32; 5] = [85, 110, 148, 168, 184];

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
const ROW_LABEL: [&str; 5] = [
    "Players",
    "Gore",
    "Practice",
    "Select Knight",
    // Ours, the fifth row: see `henge_core::shell::Row::Online`.
    "Play Online",
];
/// The labels are in `optmode` order, which is the order `Row::ALL` is in.
const _: () = assert!(ROW_LABEL.len() == Row::ALL.len());
/// **Four recovered and one ours.** `ARR` and the records give 83, 108, 150 and
/// 170, and the arrow's own four are two above the top pair and two below the
/// bottom pair. The fifth row follows the bottom pair's spacing, twenty pixels
/// under `Select Knight`, and its arrow two above it like the rest of the lower
/// half, as far down as a line of the bold face fits: its glyphs are twelve
/// tall and the screen is two hundred, so 186 is the last row that is not cut
/// off at the bottom.
const ROW_Y: [i32; 5] = [83, 108, 150, 170, 186];
/// Flag bit 0. The bottom rows are centred and their `x` is ignored.
const ROW_CENTRED: [bool; 5] = [false, false, true, true, true];
/// Which of the rows above came out of the image, for the test that says so.
const ROWS_RECOVERED: usize = Row::RECOVERED;
const _: () = assert!(ROWS_RECOVERED == 4);
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
        bold.draw_own(
            reg,
            fb,
            if self.state.gore { GORE_ON } else { GORE_OFF },
            VALUE_X,
            ROW_Y[1],
        );

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
    if let Some(p) = reg
        .palette(&format!("palette.{scene}"))
        .map(|r| r.value.clone())
    {
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

/// **Ours**: where the whose-turn line goes, under the heading and above the
/// name being typed, which are the original's own two lines at 5 and 50.
const TURN_Y: i32 = 32;

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
        SelectScene {
            state: Select::new(players),
            palette,
        }
    }

    /// `mine` is the seat at this keyboard, when there is a game across machines,
    /// and `who` is what each seat calls itself. Both are only for the line that
    /// says whose turn it is, which is ours and is drawn only in such a game.
    pub fn render(
        &self,
        reg: &mut Registry,
        fb: &mut Framebuffer,
        fonts: &Fonts,
        mine: Option<usize>,
        who: &[String],
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
        // A whose-turn line, a name under each portrait, `Player N` in the gap
        // a taken knight left and a stat line along the bottom all used to be
        // here, and `ChooseRefresh` draws none of them. It clears the screen,
        // walks `CRText`, blits the portraits still free, blits the frame on
        // the chosen one, and draws the name being typed if `TypeFLAG` is set.
        // That is the whole routine.
        for (i, &x) in PORTRAIT_X.iter().enumerate() {
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

        // **Ours, and only in a game across machines**: whose turn it is.
        //
        // `ChooseRefresh` draws nothing of the sort, and at one keyboard it does
        // not need to: the four are in the same room and can see whose hands are
        // on the keys. On four machines the three who are waiting have no way of
        // knowing that they are waiting, and a frame that will not move reads as
        // a game that has hung, which is exactly how it read.
        if let (Some(small), Some(mine)) = (fonts.small.or(fonts.bold), mine) {
            if !self.state.done() {
                let seat = self.state.seat;
                let line = if seat == mine {
                    "YOUR TURN".to_string()
                } else {
                    match who.get(seat).filter(|n| !n.is_empty()) {
                        Some(name) => format!("{} IS CHOOSING", name.to_uppercase()),
                        None => format!("PLAYER {} IS CHOOSING", seat + 1),
                    }
                };
                small.draw_own_centred(reg, fb, &line, TURN_Y);
            }
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
    draw_pointer_at(reg, fb, p);
}

/// The same blit, without the "has it been steered" test: for the screens whose
/// only cursor is the arrow, which park it on the highlighted box.
pub fn draw_pointer_at(reg: &mut Registry, fb: &mut Framebuffer, p: &henge_core::pointer::Pointer) {
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
/// same call clocks out to the DAC three bytes at a time, ninety six of them:
///
/// ```text
/// 08f38  mov si, 0x80bb
/// 08f3b  mov word ptr [si + 2], 0x800      ; entry 1
/// 08f40  mov word ptr [si + 4], 0x600      ; entry 2
/// 08f45  mov word ptr [si + 6], 0x400      ; entry 3
/// 08f4a  mov word ptr [si + 8], 0          ; entry 4
/// 08f4f  mov word ptr [si + 0xa], 0x200    ; entry 5
/// 08f54  mov word ptr [si + 0xc], 0x100    ; entry 6
/// 08f59  call 0x5a3e                       ; page flip
/// 08f5c  mov si, 0x80bb; call 0x5b44       ; fade in from that palette
/// ```
///
/// So the words are palette entries 1 to 6, and an instruction repaints
/// `MESSAGE.PIV`'s four purples and the font's own black outline as a red ramp.
/// The picture goes red, which is the difference a player sees; nothing about
/// the lettering changes but the colour of the ring round it. It lasts one
/// message: `0x8761`, which every one of the three calls first, reads the
/// palette back out of the picture's own header.
pub const INSTRUCT_RAMP: [u32; 6] = [0x880000, 0x660000, 0x440000, 0x000000, 0x220000, 0x110000];

pub fn draw_message(
    reg: &mut Registry,
    fb: &mut Framebuffer,
    fonts: &Fonts,
    msg: &henge_core::message::Message,
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
    let Some(font) = fonts.bold.or(fonts.small) else {
        return;
    };
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
    reg: &mut Registry,
    fb: &mut Framebuffer,
    fonts: &Fonts,
    intro: &henge_core::intro::Intro,
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
            reg,
            fb,
            TITLE_BANK,
            henge_core::intro::WORDMARK_CEL,
            henge_core::intro::WORDMARK_AT.0,
            henge_core::intro::WORDMARK_AT.1,
            false,
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
    let Some(font) = fonts.bold.or(fonts.small) else {
        return;
    };
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
    if let Some(p) = reg
        .palette("palette.scene.intropan")
        .map(|r| r.value.clone())
    {
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

/// The ending, which is `INTR.EXE`'s other half.
///
/// The same engine, the same scene loop and the same cast machinery as the
/// intro above; what differs is the plates, the six sprite banks the loader at
/// `0x3a00` fills the table with, `CO.STI` in place of `INTRO.STI`, and the two
/// recolourings the exit byte drives. `henge_core::ending` says where every
/// number came from.
pub fn draw_ending(
    reg: &mut Registry,
    fb: &mut Framebuffer,
    fonts: &Fonts,
    ending: &henge_core::ending::Ending,
    cast: Option<&henge_core::content::IntroCast>,
) {
    use henge_core::ending::{Back, Overlay};
    let Some(scene) = ending.showing() else {
        return;
    };
    match scene.back {
        Back::Message => show(reg, fb, MESSAGE_PLATE),
        Back::Plate(plate) => show(reg, fb, plate),
        Back::Rise { .. } => draw_rise(reg, fb, ending.pan as i32),
    }
    ink_ending(fb, scene.back, ending.code);

    if let Some(cast) = cast {
        draw_ending_cast(reg, fb, ending, cast);
    }

    // `0x129` calls `0x7f9` after the task draw when `[0x1581]` is up, so the
    // overlay goes over the cast, not under it.
    if scene.overlay == Overlay::Near {
        for cel in henge_core::ending::OVERLAY {
            if !cel.near {
                continue;
            }
            sprite::draw(
                reg,
                fb,
                henge_core::ending::OVERLAY_BANK,
                cel.cel,
                cel.x,
                cel.y,
                false,
            );
        }
    }

    let lines = ending.lines();
    if lines.is_empty() {
        return;
    }
    // `MESSAGE.PIV` carries the bold face's own five entries and so does `bg8`,
    // so both chains are drawn in the glyphs' own indices like every other
    // line in the game. The intro writes the five itself over the panorama,
    // which the ending never needs because neither of its two chains goes over
    // a picture that does not already reserve them.
    let Some(font) = fonts.bold.or(fonts.small) else {
        return;
    };
    for line in lines {
        // The `Loading ...` record on the opening card is kept in the data and
        // left off the screen, for the reason `henge_core::message` gives: it
        // is the original telling you a floppy is turning, and nothing here
        // turns one.
        if line.text.trim_end_matches([' ', '.']) == "Loading" {
            continue;
        }
        font.draw_own_centred(reg, fb, line.text, line.y);
    }
}

/// `ColourKnight` at `0x3b9d` and `ColourMoonstone` at `0x3b23`, applied to the
/// plate that is up.
///
/// Both write into a plate's stored palette at load time, so by the time a
/// scene puts that plate on the screen the entries are already the winner's.
/// Doing it here instead is the same thing in the same order: the picture is
/// up, and then its palette is corrected.
fn ink_ending(fb: &mut Framebuffer, back: henge_core::ending::Back, code: u8) {
    use henge_core::ending::{self, Back};
    let plate = match back {
        Back::Plate(p) => p,
        // `bg7` is behind the rise and `0x3ae7` hands neither it nor `bg8` to
        // either routine, so the rise and `MESSAGE.PIV` keep their own colours.
        _ => return,
    };
    if ending::KNIGHT_PLATES.contains(&plate) {
        if let Some(words) = ending::knight_ink(code) {
            for (i, w) in words.iter().enumerate() {
                fb.palette[ending::KNIGHT_FIRST + i] = henge_assets::palette::from12(*w);
            }
        }
    }
    if plate == ending::MOONSTONE_PLATE {
        if let Some((words, _)) = ending::moonstone_ink(code) {
            for (at, w) in ending::MOONSTONE_AT.iter().zip(words) {
                fb.palette[*at] = henge_assets::palette::from12(w);
            }
        }
    }
}

/// The stone circle's set piece, `HengeLOOP`.
///
/// `Hen1.p` with its own palette, the winner's armour written into entries 8 to
/// 11 by `ColourEn4Knight`, and the two tasks `0xb3a6` and `0xb3bc` start,
/// drawn in the order they were added. `henge_core::stones` says where each
/// number came from, including the one branch of `ColourEn4Knight` that reads
/// the wrong register and is reproduced rather than fixed.
pub fn draw_stones(
    reg: &mut Registry,
    fb: &mut Framebuffer,
    stones: &henge_core::stones::Stones,
    banks: Option<&henge_core::taskvm::BankTables>,
) {
    use henge_core::stones;
    show(reg, fb, stones::PLATE);
    if let Some(words) = stones::knight_ink(stones.knight) {
        for (i, w) in words.iter().enumerate() {
            fb.palette[stones::KNIGHT_FIRST + i] = henge_assets::palette::from12(*w);
        }
    }
    let Some(banks) = banks else { return };
    for task in [&stones.torches, &stones.lift] {
        draw_task(reg, fb, task, banks);
    }
}

/// The hand over the dice table, `TavernLoop`'s one task.
///
/// `_TAVERN:ShakeDice` at 0xb18a adds it at (0xa0, 0, 0x64) facing 1 on
/// `dice.cel` in `DiceHANDLE`, and the screen behind it is `dice.piv`, which
/// the place has already drawn. `henge_core::dice` says the rest.
pub fn draw_dice_hand(
    reg: &mut Registry,
    fb: &mut Framebuffer,
    table: &henge_core::dice::Table,
    banks: Option<&henge_core::taskvm::BankTables>,
) {
    if let Some(banks) = banks {
        draw_task(reg, fb, &table.task, banks);
    }
}

/// One task's last frame, placed the way `TASKRIGHT` places it.
pub(crate) fn draw_task(
    reg: &mut Registry,
    fb: &mut Framebuffer,
    task: &henge_core::taskvm::Task,
    banks: &henge_core::taskvm::BankTables,
) {
    if !task.active {
        return;
    }
    let at = (task.x, task.y, task.z);
    for part in &task.shown {
        let Some(bank) = banks
            .get(&part.table)
            .and_then(|t| t.get(part.bank as usize))
        else {
            continue;
        };
        let Some(placed) = henge_core::taskvm::place(part, bank, at, task.mirror()) else {
            continue;
        };
        let Some(cut) = sprite::cut(reg, &bank.sheet, placed.frame as usize) else {
            continue;
        };
        fb.blit(&cut.pixels, cut.w, cut.h, placed.x, placed.y, placed.mirror);
    }
}

/// `CO.STI`'s panorama, the 200 rows the camera is over.
///
/// The same window `draw_pan` moves down `INTRO.STI`, moving up this one
/// instead: `0xe12` adds `[0x162]` to `[0x160]` for the intro and subtracts it
/// for the ending, and that is the whole difference.
fn draw_rise(reg: &mut Registry, fb: &mut Framebuffer, top: i32) {
    use henge_core::ending::{RISE_PALETTE, RISE_SHEET};
    if let Some(p) = reg.palette(RISE_PALETTE).map(|r| r.value.clone()) {
        fb.set_palette(&p);
    }
    let Ok(img) = reg.image(RISE_SHEET) else {
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

/// The ending's cast, placed the way the intro's is.
///
/// `0x1ef` and `0x207` are the same two starters, with one thing added: they
/// add `[0x40c1]` and `[0x40c3]` to the z, and one scene sets those to 5 and
/// 15.
fn draw_ending_cast(
    reg: &mut Registry,
    fb: &mut Framebuffer,
    ending: &henge_core::ending::Ending,
    cast: &henge_core::content::IntroCast,
) {
    use henge_core::intro::{SPAWN_LEFT_X, SPAWN_RIGHT_X, SPAWN_Y, SPAWN_Z};
    let Some(scene) = ending.showing() else {
        return;
    };
    let now = ending.frame();
    for spawn in scene.cast {
        if now < spawn.at {
            continue;
        }
        let Some(frame) = cast.frame_at(spawn.script, now - spawn.at) else {
            continue;
        };
        let (ox, z) = if spawn.left {
            (SPAWN_LEFT_X, SPAWN_Z + scene.z.1)
        } else {
            (SPAWN_RIGHT_X, SPAWN_Z + scene.z.0)
        };
        for part in &frame.parts {
            let Some(sheet) = cast.banks.get(part.bank as usize).cloned() else {
                continue;
            };
            let Some(cut) = sprite::cut(reg, &sheet, part.cel as usize) else {
                continue;
            };
            let x = if spawn.left {
                ox - (part.x as i32 + cut.w as i32)
            } else {
                ox + part.x as i32
            };
            let y = SPAWN_Y + z + part.y as i32;
            fb.blit(&cut.pixels, cut.w, cut.h, x, y, spawn.left);
        }
    }
}

/// Everything the step has put on the plate.
///
/// A figure is placed the way the task VM places one: x from the task's own,
/// or mirrored about it, and y from the task's y plus its z. The two starters
/// differ only in x and facing, and their numbers are `henge_core::intro`'s.
fn draw_cast(
    reg: &mut Registry,
    fb: &mut Framebuffer,
    intro: &henge_core::intro::Intro,
    cast: &henge_core::content::IntroCast,
) {
    use henge_core::intro::{SPAWN_LEFT_X, SPAWN_RIGHT_X, SPAWN_Y, SPAWN_Z};
    let Some(step) = intro.showing() else { return };
    let now = intro.frame();
    for spawn in step.cast {
        if now < spawn.at {
            continue;
        }
        let Some(frame) = cast.frame_at(spawn.script, now - spawn.at) else {
            continue;
        };
        let ox = if spawn.left {
            SPAWN_LEFT_X
        } else {
            SPAWN_RIGHT_X
        };
        for part in &frame.parts {
            let Some(sheet) = cast.banks.get(part.bank as usize).cloned() else {
                continue;
            };
            let Some(cut) = sprite::cut(reg, &sheet, part.cel as usize) else {
                continue;
            };
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
/// **Recovered:** the backdrop, the moon and its corner, and the chain. The
/// routine at 0x8e5b is:
///
/// ```text
/// 08e5b  call 0x5b65                    ; fade out
/// 08e5e  call 0x8e3f                    ; CH.PIV back onto the page
/// 08e61  call 0x5a66                    ; and onto the screen
/// 08e64  les si, [0x8915]; mov [0x8981], si; mov [0x8983], es   ; BOLD.F
/// 08e70  mov si, 0x8dea; call 0x7a86    ; NextDayMes, the chain walk
/// 08e76  les si, [0x891f]               ; KI.CEL
/// 08e7a  mov ax, [0x8989]               ; Moons[MoonCount], tonight's cel
/// 08e7d  mov bx, 0x77; mov cx, 0xc      ; at (119, 12)
/// 08e83  call 0x5d7f                    ; the cel blit
/// 08e86  call 0x5a3e                    ; page flip
/// 08e89  mov si, 0x80bb; call 0x5b44    ; fade in
/// ```
///
/// `NextDayMes` (image 0x1b19a) is **two** records: `Next Day` centred at y
/// 95 and then the shared `Press fire to continue` record at DS:0x5dd, y 182,
/// which is the same record `HengeInstruct` ends on. `henge_core::message`
/// holds it as `next.day`, and `NextWHICH+35` is `WaitFIRE`.
///
/// **Nothing else is on it.** A day number, and one of the fourteen `WaitMES`
/// hints, used to be. The fourteen belong to `WAITMESSAGE`, which is a different
/// screen: `MESSAGE.PIV` with the chain over it, shown while a disk is read, and
/// its four callers are `PracticeCombat5`, `InitKnightvsDemon`, `SetUpDKL` and
/// `LoadWizard`.
///
/// The lines are drawn in their own indices, because `TextP` hands a glyph to
/// the same blitter every other cel goes through, and `CH.PIV` carries the
/// bold face's five entries: black at 5, then `fed`, `dc9`, `c95`, `832` at 9
/// to 12.
pub fn draw_interlude(
    reg: &mut Registry,
    fb: &mut Framebuffer,
    fonts: &Fonts,
    phase: henge_core::moon::Phase,
    chain: Option<&henge_core::message::Message>,
) {
    show(reg, fb, "scene.ch");
    if let (Some(bold), Some(chain)) = (fonts.bold, chain) {
        for line in chain.shown() {
            bold.draw_own_centred(reg, fb, &line.text, line.y);
        }
    }
    sprite::draw(reg, fb, MOON_BANK, phase.cel(), MOON_AT.0, MOON_AT.1, false);
}

// ------------------------------------------------------------------- the lobby

/// The lobby, drawn over the title's own plate.
///
/// **Ours**, like the screen it draws: see [`crate::online`]. It borrows the
/// title's plate, the title's wordmark and the arrow out of `SEL.CEL`, and it
/// stands on the title's own grid: the arrow at [`ARROW_X`], the labels at
/// [`LABEL_X`] and the values at [`VALUE_X`], which are `ARX`, `OPT1a`'s x and
/// `OPT1h`'s x. So the two screens line up column for column and this one adds
/// no artwork of its own.
///
/// The page is one block, laid out from the top down rather than at fixed
/// coordinates, because the pages are different heights: a lobby with four
/// people in it is taller than an empty one, and a list of open games is taller
/// than either. Everything is measured from [`LOBBY_TOP`] and the note always
/// sits on the last line.
pub fn draw_online(
    reg: &mut Registry,
    fb: &mut Framebuffer,
    fonts: &Fonts,
    screen: &crate::online::Online,
) {
    use crate::online::{Page, Row};
    show(reg, fb, TITLE_PLATE);
    sprite::draw(reg, fb, TITLE_BANK, LOGO, LOGO_AT.0, LOGO_AT.1, false);
    let Some(bold) = fonts.bold else { return };
    let small = fonts.small.unwrap_or(bold);

    // A heading, so a person always knows which of the five pages they are on.
    let heading = match screen.page {
        Page::Menu => "Play Online",
        Page::Create => "Create game",
        Page::Browse => "Lobby",
        Page::Join => "Join a game",
        Page::Waiting => screen.roster.name.as_str(),
    };
    // The note is wrapped before anything else is placed, because how many
    // lines it takes is how much room the rows above it have. A two line note
    // used to be drawn over the last row rather than under it.
    let note = if screen.note.is_empty() {
        &screen.reachable
    } else {
        &screen.note
    };
    let note_lines = if note.is_empty() {
        Vec::new()
    } else {
        wrapped(reg, small, &note.to_uppercase(), LOBBY_NOTE_LINES)
    };
    let note_top = LOBBY_NOTE_Y - (note_lines.len().max(1) as i32 - 1) * LOBBY_NOTE_STEP;
    // The floor every other line is kept above, with a pixel between the last of
    // them and the note so a descender does not sit on it.
    let floor = note_top - LOBBY_STEP - 2;

    // Where the block starts. Normally the title's own y, but a lobby with four
    // people in it and a note that took two lines is taller than the room under
    // the wordmark, so it slides up rather than losing its last row. The heading
    // goes with it, and neither ever reaches the wordmark.
    let roster_lines = if screen.page == Page::Waiting {
        screen.roster.players.len() as i32
    } else {
        0
    };
    let row_lines = screen.rows().len() as i32;
    let lines = (roster_lines + row_lines - 1).max(0) * LOBBY_STEP;
    // The gap between the roster and the rows is the first thing given up when
    // the block is taller than the room: four people and a note that took two
    // lines is six pixels more than there is, and a gap is worth less than the
    // last row.
    let mut gap = if roster_lines > 0 { LOBBY_GAP } else { 0 };
    let short = LOBBY_MIN_TOP - (floor - lines - gap);
    if short > 0 {
        gap = (gap - short).max(0);
    }
    let mut y = (floor - lines - gap).clamp(LOBBY_MIN_TOP, LOBBY_TOP + LOBBY_HEAD);
    bold.draw_own_centred(reg, fb, heading, y - LOBBY_HEAD);
    // The round trip between the two ends of the worst line, beside the heading
    // because it belongs to the game and not to any one seat, and because a line
    // of its own is a line the rows need. Each seat's own number is a different
    // measurement: that one is its leg to the relay, this is the whole path
    // between two players, and it is what the input delay is chosen off.
    if screen.page == Page::Waiting {
        if let Some(ms) = screen.roster.between {
            let line = format!("GAME  {ms}MS");
            let w = small.width(reg, &line);
            small.draw_own(reg, fb, &line, ROSTER_RIGHT - w, y - LOBBY_HEAD + 3);
        }
    }

    // The roster, in its own block above the rows: who is here, whether they
    // are ready, and how far each of them is from the list server, which is also
    // the relay that carries a game neither end can host. No knight is shown,
    // because the lobby does not settle that: every seat picks one on
    // `ChooseKnight` once the game begins, the same screen a game at one
    // keyboard uses. The measurements are in the small face because they are
    // measurements beside a name and not things to be chosen.
    if screen.page == Page::Waiting {
        for p in &screen.roster.players {
            // The name in the bold face, because it is the thing being read,
            // and everything else about that seat as one small line set against
            // the right edge. Two draws, no columns to collide: a thirteen
            // character name in the bold face is a hundred and sixty pixels of
            // a three hundred and twenty pixel screen, so columns across it do
            // not fit.
            bold.draw_own(reg, fb, &p.name, LABEL_X, y);
            // A tick beside the name the moment that seat says it is ready, so
            // the thing a person is waiting to see sits where they are already
            // looking rather than at the far edge of the line.
            if p.ready {
                let (cel, w, h) = tick_cel();
                let after = LABEL_X + bold.width(reg, &p.name) + 4;
                fb.blit(
                    &cel,
                    w,
                    h,
                    after,
                    y + (bold.line_height - h as i32) / 2,
                    false,
                );
            }
            let mut about: Vec<String> = Vec::new();
            if Some(p.seat) == screen.seat {
                about.push("YOU".into());
            }
            // How far that machine is from the relay, which is what it puts
            // into everybody's lag while the relay is carrying the game. A lobby
            // is where somebody should find out their line is bad, not a minute
            // into a fight.
            if let Some(ms) = p.ms {
                about.push(format!("{ms}MS"));
            }
            if !about.is_empty() {
                let line = about.join("  ");
                let w = small.width(reg, &line);
                small.draw_own(reg, fb, &line, ROSTER_RIGHT - w, y + 3);
            }
            y += LOBBY_STEP;
        }
        y += gap;
    }

    // The rows of the page that is up, and the arrow against the one chosen.
    let rows = screen.rows();
    let first = y;
    let chosen = screen.row.min(rows.len().saturating_sub(1));
    sprite::draw(
        reg,
        fb,
        SEL,
        ARROW,
        ARROW_X,
        first + chosen as i32 * LOBBY_STEP - 2,
        false,
    );
    for (i, row) in rows.iter().enumerate() {
        let at = first + i as i32 * LOBBY_STEP;
        if at > floor {
            break;
        }
        // A game on the list is a line of its own rather than a label and a
        // value: the name on the left, and on the right how full it is, a star
        // if it wants a word, and a tilde if the list server is carrying it
        // because its host cannot be reached directly.
        if let Row::Game(n) = row {
            if let Some(g) = screen.games.get(*n) {
                bold.draw_own(reg, fb, &g.name, LABEL_X, at);
                // Words rather than marks, for the same reason the roster uses
                // them: the game's font has no star, no tilde and no slash, and
                // draws what it does not know as a blank.
                let mut about = vec![format!("{} OF {}", g.players, g.seats)];
                if g.locked {
                    about.push("LOCKED".into());
                }
                if g.relayed() {
                    about.push("CARRIED".into());
                }
                let line = about.join("  ");
                let w = small.width(reg, &line);
                small.draw_own(reg, fb, &line, ROSTER_RIGHT - w, at + 3);
            }
            continue;
        }
        bold.draw_own(reg, fb, row.label(), LABEL_X, at);
        let value = match row {
            Row::LobbyName | Row::PlayerName | Row::Address | Row::Password => screen.shown(*row),
            Row::Ready => (if screen.ready { GORE_ON } else { GORE_OFF }).to_string(),
            _ => String::new(),
        };
        if !value.is_empty() {
            // A typed field can outgrow its column, so it is drawn in the small
            // face when the bold one would run off the screen. An address is
            // the case that needs it.
            let w = bold.width(reg, &value);
            if VALUE_X + w < henge_core::SCREEN_W as i32 - 4 {
                bold.draw_own(reg, fb, &value, VALUE_X, at);
            } else {
                small.draw_own(reg, fb, &value, VALUE_X, at + 3);
            }
        }
    }

    // The note, at the bottom, wrapped above. What the router said, who joined,
    // why the last attempt failed. The small face, because it is a sentence and
    // not a label.
    for (i, line) in note_lines.iter().enumerate() {
        small.draw_own_centred(reg, fb, line, note_top + i as i32 * LOBBY_NOTE_STEP);
    }
}

/// Break a sentence into at most `most` lines that fit the screen.
///
/// Public because the message boxes need it too: a notice is written by whatever
/// happened and some of them are longer than three hundred and twenty pixels.
///
/// **Ours**, like the screen it is for. Broken on spaces, because the small face
/// is proportional and a break in the middle of a word reads as a typing
/// mistake; a single word too wide to fit is cut with a tail, which is the only
/// case that can lose anything, and the notes that hit it are addresses.
fn wrapped(reg: &mut Registry, font: &Font, text: &str, most: usize) -> Vec<String> {
    // The measuring is handed over so the breaking itself can be tested without
    // a font, which needs the whole asset pack behind it.
    let mut width = |s: &str| font.width(reg, s);
    wrap_to(text, most, SCREEN_W as i32 - 8, &mut width)
}

pub fn wrap_to(
    text: &str,
    most: usize,
    room: i32,
    width: &mut impl FnMut(&str) -> i32,
) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut dropped = false;
    for word in text.split_whitespace() {
        let mut wider = line.clone();
        if !wider.is_empty() {
            wider.push(' ');
        }
        wider.push_str(word);
        // A word that will not fit even on a line of its own is kept anyway and
        // cut at the end, because a cut address is still recognisable and a
        // missing one is not.
        if width(&wider) <= room || line.is_empty() {
            line = wider;
            continue;
        }
        if lines.len() + 1 == most {
            // This is the last line there is room for, so the rest is lost.
            dropped = true;
            break;
        }
        lines.push(std::mem::take(&mut line));
        line = word.to_string();
    }
    if !line.is_empty() {
        lines.push(line);
    }
    // A note that lost its tail says so, and so does a word wider than the
    // screen. Both end the same way, and the tail is put on before the trimming
    // so that the three characters it costs are counted.
    if let Some(last) = lines.last_mut() {
        if dropped {
            last.push_str("...");
        }
        while width(last) > room && last.chars().count() > 4 {
            last.truncate(last.char_indices().nth_back(3).map_or(0, |(i, _)| i));
            last.push_str("...");
        }
    }
    lines
}

/// The lobby's own layout. Ours, and the only numbers in this file that are not
/// out of the image. The columns are the title's (`ARROW_X`, `LABEL_X`,
/// `VALUE_X`); only the vertical rhythm is new, and it is chosen so that the
/// tallest page, a four-seat lobby, still leaves the note its line.
const LOBBY_TOP: i32 = 84;
/// The drop from the heading to the first line under it.
const LOBBY_HEAD: i32 = 18;
/// One line, whether it is a roster entry or a row.
const LOBBY_STEP: i32 = 13;
/// The gap between the roster and the rows, so the two blocks read as two.
const LOBBY_GAP: i32 = 8;
/// As high as the block will ever go, which is clear of the wordmark: the
/// wordmark is `SEL.CEL`'s 54 rows blitted at y 10, so it ends at 64.
const LOBBY_MIN_TOP: i32 = 86;
/// Where the note's last line sits, and the floor every other line is kept
/// above.
const LOBBY_NOTE_Y: i32 = 190;
/// How many lines a note may take, and the step between them. Two, because that
/// is what fits between the last row and the bottom of the screen.
const LOBBY_NOTE_LINES: usize = 2;
const LOBBY_NOTE_STEP: i32 = 8;
/// The right edge everything secondary is set against: what a seat is, and how
/// full and how reachable a listed game is.
const ROSTER_RIGHT: i32 = 300;

/// The tick drawn beside a name that is ready.
///
/// **Ours**, and a cel rather than a letter because there is no letter for it:
/// the bold and small faces carry A to Z, 0 to 9, space and `. , ! ?`, and
/// `GFX:TextP` draws anything else as nothing at all, so `*`, `>` and `~` come
/// out blank. The art is the face; the outline around it is worked out from the
/// face at draw time, which is how the bold glyphs in `CH.PIV` are built too.
const TICK_ART: [&str; TICK_H] = [
    "........#",
    ".......##",
    "......##.",
    "#....##..",
    "##..##...",
    ".####....",
    "..##.....",
];
const TICK_W: usize = 9;
const TICK_H: usize = 7;
/// The bold face's own two inks out of `CH.PIV`: black at 5, the lightest face
/// colour at 9. The tick is the same ink as the lettering it sits beside, not a
/// colour of its own.
const TICK_OUTLINE: u8 = 5;
const TICK_FACE: u8 = 9;
/// One pixel of outline on every side, so the cel is the art grown by one.
const CEL_W: usize = TICK_W + 2;
const CEL_H: usize = TICK_H + 2;

/// Builds the tick: the art in [`TICK_FACE`], and every cell touching it in
/// [`TICK_OUTLINE`].
fn tick_cel() -> ([u8; CEL_W * CEL_H], usize, usize) {
    let mut face = [0u8; CEL_W * CEL_H];
    for (row, art) in TICK_ART.iter().enumerate() {
        for (col, ink) in art.bytes().enumerate() {
            if ink == b'#' {
                face[(row + 1) * CEL_W + col + 1] = TICK_FACE;
            }
        }
    }
    let mut cel = face;
    for y in 0..CEL_H as i32 {
        for x in 0..CEL_W as i32 {
            if face[y as usize * CEL_W + x as usize] != 0 {
                continue;
            }
            let touches = (-1..=1).any(|dy| {
                (-1..=1).any(|dx| {
                    let (ny, nx) = (y + dy, x + dx);
                    (0..CEL_H as i32).contains(&ny)
                        && (0..CEL_W as i32).contains(&nx)
                        && face[ny as usize * CEL_W + nx as usize] != 0
                })
            });
            if touches {
                cel[y as usize * CEL_W + x as usize] = TICK_OUTLINE;
            }
        }
    }
    (cel, CEL_W, CEL_H)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Six pixels a character, which is near enough the small face and is a
    /// number a reader of the test can do in their head.
    fn six(s: &str) -> i32 {
        s.chars().count() as i32 * 6
    }

    /// A note is broken between words and keeps its whole meaning when it fits.
    ///
    /// The note that made this necessary is the one a game behind a router that
    /// will not open a port gets, and it is the difference between a game that
    /// works and one that does not, so losing its tail to a truncation was the
    /// wrong answer.
    #[test]
    fn a_note_is_broken_between_words_and_not_through_them() {
        let mut w = six;
        let long =
            "YOUR ROUTER WOULD NOT OPEN THE PORT, SO THE GAME IS BEING CARRIED BY THE LIST SERVER";
        let lines = wrap_to(long, 2, 312, &mut w);
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|l| six(l) <= 312), "{lines:?}");
        assert!(!lines.iter().any(|l| l.ends_with(' ') || l.starts_with(' ')));
        // Nothing was lost: put back together it is the sentence that went in.
        assert_eq!(lines.join(" "), long);
        // A short one stays one line and is not padded out to two.
        assert_eq!(wrap_to("IN SEAT 2", 2, 312, &mut w), vec!["IN SEAT 2"]);
    }

    /// What will not fit in the lines there are loses its tail and says so, and
    /// so does a single word wider than the screen. Those are addresses, and an
    /// address that is cut is still recognisable.
    #[test]
    fn a_note_too_long_for_its_lines_is_cut_with_a_tail() {
        let mut w = six;
        let lines = wrap_to("ONE TWO THREE FOUR FIVE SIX SEVEN", 1, 60, &mut w);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].ends_with("..."), "{lines:?}");
        assert!(six(&lines[0]) <= 60);
        let one = wrap_to("SUPERCALIFRAGILISTIC", 2, 60, &mut w);
        assert!(one.last().unwrap().ends_with("..."), "{one:?}");
        assert!(one.iter().all(|l| six(l) <= 60), "{one:?}");
    }

    /// The tick is drawn in the bold face's own two inks, and every face pixel
    /// is surrounded, so it reads on the title plate rather than only against it.
    #[test]
    fn the_tick_is_the_face_inside_its_own_outline() {
        let (cel, w, h) = tick_cel();
        assert_eq!((w, h), (TICK_W + 2, TICK_H + 2));
        assert!(cel.contains(&TICK_FACE), "it has a face");
        assert!(cel.contains(&TICK_OUTLINE), "and an outline");
        assert!(
            cel.iter()
                .all(|p| *p == 0 || *p == TICK_FACE || *p == TICK_OUTLINE),
            "and nothing else: a third ink would not be the lettering's"
        );
        // The art sits one pixel in on every side, so no face pixel is on the
        // edge and every one of them has eight neighbours inside the cel.
        for y in 0..h {
            for x in 0..w {
                if cel[y * w + x] != TICK_FACE {
                    continue;
                }
                assert!(
                    y > 0 && y + 1 < h && x > 0 && x + 1 < w,
                    "the face touches the edge at {x},{y} and would be drawn unoutlined"
                );
                for (dy, dx) in [
                    (-1i32, -1i32),
                    (-1, 0),
                    (-1, 1),
                    (0, -1),
                    (0, 1),
                    (1, -1),
                    (1, 0),
                    (1, 1),
                ] {
                    let n = cel[(y as i32 + dy) as usize * w + (x as i32 + dx) as usize];
                    assert_ne!(n, 0, "a gap beside the face at {x},{y}");
                }
            }
        }
    }
}
