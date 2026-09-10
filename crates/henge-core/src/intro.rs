//! The intro sequence, out of `INTR.EXE`.
//!
//! `INTR.EXE` is the game's own engine with a different program on top. It
//! unpacks the way `MAIN.EXE` does and `tools/symbolmap.py` reads it unchanged:
//! four source modules and 319 named symbols, module 2 `_TASK` symbol for
//! symbol, module 0 `GFX` with a tile engine beside the text engine.
//!
//! **The image that tool writes is still packed, and that was the whole
//! blockage.** Its tail is Microsoft EXEPACK's run-length stream, so every
//! zero-filled span of the program is four bytes standing for hundreds. That is
//! why the code addresses looked as though they needed a fitted seven-step
//! correction, and why the data addresses looked 14,911 bytes out. Expand the
//! stream and there is no correction of any kind: `TextASCII` is at 44,634 and
//! `MesFILE` at 46,007, exactly where the symbol table says. `henge_formats::introexe`
//! does the expanding, and with it these came out:
//!
//! - **`.STI` is a tile map, and the opening is a vertical pan.** `FindTile`
//!   cuts tile *n* out of a sheet at `((n % 10) * 32, (n / 10) * 25)`, the same
//!   32x25 grid ten across a `CMP` uses, and the routine at `0x0e6f` walks the
//!   map ten big-endian words to a row, dividing each by 80 to choose between
//!   three loaded sheets. `INTRO.STI`'s 960 bytes are therefore 48 rows: a 320
//!   by 1200 panorama out of `bg1a`, `bg1c` and `bg1b`, which is what the three
//!   `panfile` symbols are for. The pan is a 200-tall window moving down it
//!   from 0 to 1000, at the speeds in [`PAN_SPEEDS`].
//! - **The captions have coordinates after all.** They are ordinary ten-byte
//!   `[string][x][y][flags][next]` records, the same chain the game's message
//!   system uses; the flag's bit 0 centres the line, which is why every x in
//!   the intro is zero. [`Line`] carries the recovered `y`.
//! - **The credits are the loading screens.** A seven-entry table at `DS:0x152`
//!   is stepped once per file loaded during the opening, so the pairing that
//!   this project would not guess at is simply read off: `Programmed by` goes
//!   with Anthony Mack and Nicholas Snape, `Music and Sound by` with Audio
//!   Visual Magic. `Richard Joseph` and `Kevin Hoare` are strings in the image
//!   that no record points at, so they are left out rather than placed.
//! - **The story cards are not drawn over a plate at all.** `0x36c4` loads
//!   `MESSAGE.PIV` over the screen first, which is the same stone-circle box
//!   the game's own messages use. The black bands this project used to cut
//!   through the artwork were an invention, and there is nothing to invent.
//! - **The cast animates**, on the intro's own scripts. They are in its
//!   `DGROUP` like the game's, and the intro's `INITTASK` fills one more
//!   handler slot than the game's, so `TASKGOSUB` is `0x9a` here and `0x98`
//!   there. Eighteen scripts drive the plates.
//! - **Which scripts a scene spawns is a table, and reading it corrected the
//!   stone circle.** `0x34b` and `0x31f` step a word table at `[0x13ad]` for
//!   `[0x13ab]` entries, spawning one and then running `16 - [0x13af]` frames.
//!   The circle's table at `DS:0x12d3` is `2ec3, 2ec3, 30c3, 30c3` and its
//!   walkers are torches out of `DA1.CEL`, seen from above like the plate; the
//!   forest's at `DS:0x12db` is ten of `2927` and the dolmen's at `DS:0x1303`
//!   five of `26e3`.
//! - **A figure whose script runs out leaves.** The frame builder emits a part
//!   for every record it walks past and a script pointer sitting on `ff ff`
//!   walks past none, so nothing is drawn. Holding the last frame instead is
//!   what left a druid standing at the left edge of the forest.
//! - **The captions' colours are the artwork's.** `BOLD.F` draws every glyph in
//!   five indices, 5 the ring round the letter and 9 to 12 its face, and the
//!   intro writes those five palette entries itself. See [`CAPTION_INK`].
//!
//! **The intro is the first half of `INTR.EXE` and the ending is the second.**
//! The program reads its command tail at `PSP:0x82` and jumps to a different
//! sequence when it is given one, and that half is the one with `The End`,
//! `And so, the tale of the Moonstone...`, `co.sti` and the plates `bg5`, `bg7`
//! and `bg8`; `ColourMoonstone` reads the same argument to colour the stone.
//! So those three plates are deliberately **not** in the sequence below: they
//! are not part of the intro.
//!
//! **Ours, and marked so below:** how long the publisher's logo and each of the
//! seven credit screens is held, because in the original that is however long a
//! floppy takes; and the rounding of the intro's own frame rate onto this
//! engine's tick.

use serde::{Deserialize, Serialize};

/// One line of a caption, at the `y` its record carries.
///
/// Every intro caption sets bit 0 of its flag word, and `TextPTop` reads that
/// as "centre this line", computing x from `TextRightBorder - TextLeftBorder`
/// less the measured width. So the x in the record is zero and is not used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Line {
    pub text: &'static str,
    pub y: i32,
}

const fn l(text: &'static str, y: i32) -> Line {
    Line { text, y }
}

/// One figure the intro puts on a plate: which script it runs, which of the
/// intro's own frames it starts on, and whether it faces left.
///
/// The two starters are `0x1ef` and `0x207`: the first puts a task at x 160
/// facing right, the second at x 120 facing left, both at y 0 and z 100.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spawn {
    pub script: &'static str,
    pub at: u32,
    pub left: bool,
}

const fn r(script: &'static str, at: u32) -> Spawn {
    Spawn {
        script,
        at,
        left: false,
    }
}
const fn lf(script: &'static str, at: u32) -> Spawn {
    Spawn {
        script,
        at,
        left: true,
    }
}

/// Where a task starts. `0x1ef` and `0x207` differ only in x and facing.
pub const SPAWN_RIGHT_X: i32 = 160;
pub const SPAWN_LEFT_X: i32 = 120;
pub const SPAWN_Y: i32 = 0;
pub const SPAWN_Z: i32 = 100;

/// What is behind a step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backdrop {
    /// `MINDSCAP`, the publisher's logo, which is a `PIV` with no extension and
    /// had never been baked.
    Logo,
    /// The 320x1200 panorama, showing the 200 rows at `from`, moving to `to`.
    Pan { from: u32, to: u32 },
    /// A full-screen plate.
    Plate(&'static str),
    /// `MESSAGE.PIV`, the box the game's own messages go over.
    Message,
}

/// One step of the sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    pub back: Backdrop,
    /// Whether `BOLD.F`'s wordmark cel is blitted over it, which the opening
    /// does at (9, 60).
    pub wordmark: bool,
    pub lines: &'static [Line],
    pub cast: &'static [Spawn],
    /// How many of the intro's own frames the step lasts. Zero means the step
    /// runs until its pan reaches the far end.
    pub frames: u32,
}

const NO_CAST: &[Spawn] = &[];
const NO_LINES: &[Line] = &[];

/// Where the wordmark goes: `BOLD.F` cel 73 at (9, 60), which is what
/// `0x0c6a` blits before it draws the publisher's card.
pub const WORDMARK_AT: (i32, i32) = (9, 60);
pub const WORDMARK_CEL: usize = 73;

// ------------------------------------------------------------- the captions

/// The publisher's card, drawn over the moon plate during the opening.
pub const PRESENTS: &[Line] = &[l("MINDSCAPE PRESENTS", 20), l("copyright 1992", 165)];

/// The seven credit screens, in the order the table at `DS:0x152` steps
/// through them, one per file the opening loads.
// Hand-aligned: one credit screen per line.
#[rustfmt::skip]
pub const CREDITS: [&[Line]; 7] = [
    &[l("conversion by", 85), l("Images Software Ltd", 105)],
    &[l("created by", 85), l("Rob Anderson", 105)],
    &[l("Programmed by", 75), l("Anthony Mack", 95), l("Nicholas Snape", 115)],
    &[l("Artwork by", 75), l("Rob Anderson", 95), l("Dennis Turner", 115)],
    &[l("Music and Sound by", 85), l("Audio Visual Magic", 105)],
    &[l("Additional Art by", 85), l("Steve Leney", 105)],
    &[l("Design by", 75), l("Rob Anderson", 95), l("Todd Prescott", 115)],
];

/// The story card the intro ends on, at `DS:0x141d`.
pub const DRUIDS: &[Line] = &[
    l("The druids sent their", 55),
    l("best knights to Stonehenge", 75),
    l("so they may be dubbed", 95),
    l("into the", 115),
    l("Quest for the ", 135),
    l("MOONSTONE", 175),
];

/// The card the ending opens on, at `DS:0x13b9`. Not part of the intro; kept
/// because it is recovered and the ending will want it.
pub const CEREMONY: &[Line] = &[
    l("The ceremony of the", 75),
    l("Moonstone", 95),
    l("is about to begin", 115),
    l("Loading ...", 180),
];

/// The card the ending closes on, at `DS:0x1459`, and `The End` at `DS:0x0002`.
pub const TALE: &[Line] = &[
    l("And so, the tale of the", 55),
    l("Moonstone and the courage", 75),
    l("of the knights that fought", 95),
    l("for it is passed on from", 115),
    l("one generation to the next", 135),
];
pub const THE_END: &[Line] = &[l("The End", 95)];

/// The font's own five palette entries, and what `0x0cfb` sets them to.
///
/// `BOLD.F`'s glyphs are not silhouettes. Every one of them is drawn in
/// exactly five indices: **5 is the outline**, which rings the letter and
/// fills its counters, and **9, 10, 11 and 12 are the letter face**, a thin
/// bright stroke shaded across four steps inside that outline. Flatten all
/// five to one colour and the outline and the face become the same colour, so
/// every counter closes and the line reads as a row of blobs.
///
/// The intro reserves those five and writes them itself. `0x0cb0` sets all
/// five to white while a caption goes up, and `0x0cfb` puts them back to the
/// values below: the outline black and the face a warm ramp. `MESSAGE.PIV`,
/// the plate every message in the game is written over, carries exactly these
/// five words in its own palette, which is what makes the game's text legible
/// on it; the panorama's palette leaves 9 to 12 unused entirely, so writing
/// them over the moon costs the picture nothing. That is the whole of the
/// caption's colour, and it is the artwork's, so nothing has to be invented
/// for it.
pub const CAPTION_INK: [(u8, u32); 5] = [
    (5, 0x000000),
    (9, 0xffeedd),
    (10, 0xddcc99),
    (11, 0xbb9955),
    (12, 0x884422),
];

// ------------------------------------------------------------------ the pan

/// The pan's speed schedule: the first threshold the position has not passed
/// chooses the speed beside it, so the camera accelerates to six pixels a
/// frame and eases back to one. Eleven words at `DS:0x124` and eleven at
/// `DS:0x13a`.
pub const PAN_STOPS: [u16; 11] = [10, 18, 33, 65, 115, 900, 920, 940, 960, 980, 1200];
pub const PAN_SPEEDS: [u8; 11] = [1, 2, 3, 4, 5, 6, 5, 4, 3, 2, 1];

/// How far down the panorama the pan ends: 48 rows of 25 less the 200 on show.
pub const PAN_END: u32 = 1000;

/// The panorama's own size, which is the tile map's.
pub const PAN_W: u32 = 320;
pub const PAN_H: u32 = 1200;

/// The speed at a position, straight off the two tables.
pub fn pan_speed(at: u32) -> u32 {
    for (i, stop) in PAN_STOPS.iter().enumerate() {
        if at <= *stop as u32 {
            return PAN_SPEEDS[i] as u32;
        }
    }
    PAN_SPEEDS[PAN_SPEEDS.len() - 1] as u32
}

// ------------------------------------------------------------- the sequence

/// **Ours, but only the rounding is.** The intro's scene loop waits two BIOS
/// ticks a frame: `INTR.EXE` at `0x021f` reads the BIOS counter at `0000:046c`,
/// adds two and stores the target, and `0x022f` spins until the counter reaches
/// it. Two of 18.2065 Hz is 9.1033 frames a second. This engine ticks at the
/// game's own 70.0863 Hz retrace, which is 7.70 ticks to the intro's frame, and
/// eight is that rounded. It was seven while the engine ticked at sixty.
pub const TICKS_PER_FRAME: u32 = 8;

/// **Ours.** How long the logo and a credit screen are held. In the original
/// each is up for exactly as long as the next file takes to come off a floppy,
/// which is not a number that can be recovered or reproduced.
pub const LOGO_TICKS: u32 = 130;
pub const CREDIT_TICKS: u32 = 105;

/// The story card is held for 420 vertical retraces, which is the one wait in
/// the intro measured in retraces rather than in scene frames. This engine's
/// tick *is* a retrace now, so the recovered number is the number: 420, which at
/// 70.0863 Hz is the six seconds it always was. It was scaled to 360 while the
/// engine ticked at sixty.
pub const MESSAGE_TICKS: u32 = 420;

/// The sequence, as `INTR.EXE`'s own main module runs it.
///
/// `0x007c` loads, `0x007f` pans, and `0x008f` onwards is one call per scene:
/// `0x291`, `0x2c2`, `0x240`, `0x386` twice, `0x3e7`, `0x465`, `0x432`, and
/// then the story card. Which plate a scene shows is which of the eight screen
/// slots it hands to the blitter, and the three `CopyPals3` does at `0x0095`
/// are what moves `bg4`, `bg5a` and `bg3` into the slots the pan was using.
// Hand-aligned: one scene per line where it fits, so the seven identical credit
// steps read as seven rows differing only in which card they carry, and a scene's
// cast is grouped the way the original's own tables group it.
#[rustfmt::skip]
pub const STEPS: &[Step] = &[
    // The publisher's logo, its own screen, before anything else is loaded.
    Step { back: Backdrop::Logo, wordmark: false, lines: NO_LINES, cast: NO_CAST, frames: 0 },
    // The moon at the top of the panorama, with the wordmark and the card.
    Step {
        back: Backdrop::Pan { from: 0, to: 0 },
        wordmark: true,
        lines: PRESENTS,
        cast: NO_CAST,
        frames: 0,
    },
    // Seven credit screens over the same view, one per file loaded. The
    // wordmark is not on these: `0x0cb0` redraws the tile rows under the
    // caption and does not blit it again, so it is painted over.
    Step { back: Backdrop::Pan { from: 0, to: 0 }, wordmark: false, lines: CREDITS[0], cast: NO_CAST, frames: 0 },
    Step { back: Backdrop::Pan { from: 0, to: 0 }, wordmark: false, lines: CREDITS[1], cast: NO_CAST, frames: 0 },
    Step { back: Backdrop::Pan { from: 0, to: 0 }, wordmark: false, lines: CREDITS[2], cast: NO_CAST, frames: 0 },
    Step { back: Backdrop::Pan { from: 0, to: 0 }, wordmark: false, lines: CREDITS[3], cast: NO_CAST, frames: 0 },
    Step { back: Backdrop::Pan { from: 0, to: 0 }, wordmark: false, lines: CREDITS[4], cast: NO_CAST, frames: 0 },
    Step { back: Backdrop::Pan { from: 0, to: 0 }, wordmark: false, lines: CREDITS[5], cast: NO_CAST, frames: 0 },
    Step { back: Backdrop::Pan { from: 0, to: 0 }, wordmark: false, lines: CREDITS[6], cast: NO_CAST, frames: 0 },
    // The pan itself: down the whole panorama, on the recovered speed ramp.
    Step {
        back: Backdrop::Pan { from: 0, to: PAN_END },
        wordmark: false,
        lines: NO_LINES,
        cast: NO_CAST,
        frames: 0,
    },
    // `0x291`: ten of the same script, twelve frames apart, over the view the
    // pan came to rest on.
    Step {
        back: Backdrop::Pan { from: PAN_END, to: PAN_END },
        wordmark: false,
        lines: NO_LINES,
        cast: &[
            r("2927", 0), r("2927", 12), r("2927", 24), r("2927", 36), r("2927", 48),
            r("2927", 60), r("2927", 72), r("2927", 84), r("2927", 96), r("2927", 108),
        ],
        frames: 120,
    },
    // `0x2c2`: five, eight frames apart, on `bg2`.
    Step {
        back: Backdrop::Plate("scene.bg2"),
        wordmark: false,
        lines: NO_LINES,
        cast: &[r("26e3", 0), r("26e3", 8), r("26e3", 16), r("26e3", 24), r("26e3", 32)],
        frames: 40,
    },
    // `0x240`: the six standing figures of `0x4ab`, then the four of the table
    // at `DS:0x12d3` alternating right and left sixteen frames apart, then the
    // one that ends the scene.
    //
    // That table is `2ec3, 2ec3, 30c3, 30c3`, and reading it rather than
    // assuming is what puts the right figures on this plate: the circle is seen
    // from above, and what walks into it is two torches out of `DA1.CEL`, not
    // the forest's side-on walker.
    Step {
        back: Backdrop::Plate("scene.bg3"),
        wordmark: false,
        lines: NO_LINES,
        cast: &[
            r("2e75", 0), r("2e99", 0), r("2dc5", 0),
            lf("2e75", 0), lf("2e99", 0), lf("2ce5", 0),
            r("2ec3", 0), lf("2ec3", 16), r("30c3", 32), lf("30c3", 48),
            r("338f", 64),
        ],
        frames: 102,
    },
    // `0x386`, both halves: `bg4` then `bg5a`.
    Step {
        back: Backdrop::Plate("scene.bg4"),
        wordmark: false,
        lines: NO_LINES,
        cast: &[r("1583", 0)],
        frames: 24,
    },
    Step {
        back: Backdrop::Plate("scene.bg5a"),
        wordmark: false,
        lines: NO_LINES,
        cast: &[r("1e29", 0)],
        frames: 31,
    },
    // `0x3e7`.
    Step {
        back: Backdrop::Plate("scene.bg2a"),
        wordmark: false,
        lines: NO_LINES,
        cast: &[r("2669", 0)],
        frames: 31,
    },
    // `0x465`: the ten of `0x4d0`, and the pair that follow them.
    Step {
        back: Backdrop::Plate("scene.bg3"),
        wordmark: false,
        lines: NO_LINES,
        cast: &[
            r("2e75", 0), r("2e99", 0), r("2e51", 0), r("309f", 0), r("3221", 0),
            lf("2e75", 0), lf("2e99", 0), lf("2e51", 0), lf("309f", 0), lf("3221", 0),
            r("348f", 0), r("349b", 0),
        ],
        frames: 31,
    },
    // `0x432`.
    Step {
        back: Backdrop::Plate("scene.bg5a"),
        wordmark: false,
        lines: NO_LINES,
        cast: &[r("1739", 0)],
        frames: 27,
    },
    // `0x00b6`: the story card, over `MESSAGE.PIV`, held for 420 retraces.
    Step {
        back: Backdrop::Message,
        wordmark: false,
        lines: DRUIDS,
        cast: NO_CAST,
        frames: 0,
    },
];

/// How long a step is, in ticks.
pub fn step_ticks(n: usize) -> u32 {
    let Some(step) = STEPS.get(n) else { return 0 };
    if step.frames > 0 {
        return step.frames * TICKS_PER_FRAME;
    }
    match step.back {
        Backdrop::Logo => LOGO_TICKS,
        // A plate with no frames of its own would not be drawn at all; the
        // sequence has none, and the check is here so a new one cannot.
        Backdrop::Plate(_) => TICKS_PER_FRAME,
        Backdrop::Message => MESSAGE_TICKS,
        Backdrop::Pan { from, to } if from == to => CREDIT_TICKS,
        Backdrop::Pan { from, to } => {
            let (mut at, mut ticks) = (from, 0);
            while at < to {
                at = (at + pan_speed(at)).min(to);
                ticks += 1;
            }
            ticks
        }
    }
}

// ------------------------------------------------------------------- state

/// Where the sequence has got to.
///
/// A state machine with no pixels in it, like the title and the select, so a
/// test drives it exactly as a keyboard does.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Intro {
    /// Which step is up.
    pub card: usize,
    /// Ticks this step has been up.
    pub held: u32,
    /// Where the pan has got to down the panorama.
    pub pan: u32,
    /// Finished, whether it ran out or was cut short.
    pub done: bool,
}

impl Intro {
    pub fn new() -> Intro {
        Intro::default()
    }

    /// The step being shown, or nothing once it is over.
    pub fn showing(&self) -> Option<&'static Step> {
        if self.done {
            return None;
        }
        STEPS.get(self.card)
    }

    /// Which of the intro's own frames this step is on.
    pub fn frame(&self) -> u32 {
        self.held / TICKS_PER_FRAME
    }

    /// One tick.
    pub fn tick(&mut self) {
        let Some(step) = self.showing() else {
            self.done = true;
            return;
        };
        if let Backdrop::Pan { to, .. } = step.back {
            if self.pan < to {
                self.pan = (self.pan + pan_speed(self.pan)).min(to);
            }
        }
        self.held += 1;
        if self.held >= step_ticks(self.card) {
            self.next();
        }
    }

    /// Move on. The last step ends the sequence.
    pub fn next(&mut self) {
        self.held = 0;
        self.card += 1;
        if self.card >= STEPS.len() {
            self.done = true;
            return;
        }
        if let Some(Backdrop::Pan { from, .. }) = self.showing().map(|s| s.back) {
            self.pan = from.max(self.pan);
        }
    }

    /// Fire. The original's intro can be cut short and so can this one:
    /// `0x00ea` tests the fire button every frame and jumps to the exit.
    pub fn skip(&mut self) {
        self.done = true;
    }

    /// How many steps there are, for anything that wants to draw progress.
    pub fn card_count(&self) -> usize {
        STEPS.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// These are counts of **vertical retraces**, and the intro is one of the
    /// two loops in the game that genuinely waits on them: `INTR.EXE` paces its
    /// scenes with `0xfae` (`mov cx, ax; call 0xf9e; loop`), the retrace wait.
    /// They are not counts of the 54.6204 Hz timer the fight is paced by, so
    /// putting the arena on that clock must not move a single one of them.
    #[test]
    fn the_retrace_counts_are_the_recovered_ones() {
        assert_eq!(TICKS_PER_FRAME, 8);
        assert_eq!(LOGO_TICKS, 130);
        assert_eq!(CREDIT_TICKS, 105);
        // `0x00b6`: `ax = 0x1a4`, four hundred and twenty retraces.
        assert_eq!(MESSAGE_TICKS, 420);
    }

    #[test]
    fn every_step_holds_for_a_while_and_names_something_to_draw() {
        for (n, s) in STEPS.iter().enumerate() {
            assert!(step_ticks(n) > 0, "step {n} would flash past");
            if let Backdrop::Plate(p) = s.back {
                assert!(p.starts_with("scene.bg"), "{p} is an intro plate");
            }
        }
    }

    /// The plates are the ones the intro's own scene routines hand to the
    /// blitter, which is five of the eleven: `bg5`, `bg7` and `bg8` belong to
    /// the ending, and `bg1a`, `bg1b` and `bg1c` are the panorama's tiles.
    #[test]
    fn the_plates_are_the_recovered_ones() {
        let plates: Vec<&str> = STEPS
            .iter()
            .filter_map(|s| match s.back {
                Backdrop::Plate(p) => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(
            plates,
            vec![
                "scene.bg2",
                "scene.bg3",
                "scene.bg4",
                "scene.bg5a",
                "scene.bg2a",
                "scene.bg3",
                "scene.bg5a"
            ]
        );
    }

    /// The pan's schedule is the two tables, and it reaches the far end.
    #[test]
    fn the_pan_accelerates_and_eases_off() {
        assert_eq!(pan_speed(0), 1);
        assert_eq!(pan_speed(500), 6);
        assert_eq!(pan_speed(1000), 1);
        let mut at = 0;
        let mut ticks = 0;
        while at < PAN_END {
            at += pan_speed(at);
            ticks += 1;
            assert!(ticks < 10_000, "the pan does not end");
        }
        assert!(at >= PAN_END);
    }

    /// Every caption sits inside the screen, which is what makes the recovered
    /// coordinates worth having: nothing has to be invented to place them.
    #[test]
    fn every_caption_is_on_the_screen() {
        for step in STEPS {
            for line in step.lines {
                assert!(line.y >= 0 && line.y < 200, "{:?} is off the screen", line);
                assert!(!line.text.is_empty());
            }
        }
        for c in CREDITS {
            assert!(!c.is_empty());
        }
    }

    #[test]
    fn it_runs_through_and_stops() {
        let mut i = Intro::new();
        let total: u32 = (0..STEPS.len()).map(step_ticks).sum();
        for _ in 0..total {
            assert!(!i.done);
            i.tick();
        }
        assert!(i.done, "it ends rather than looping");
        assert!(i.showing().is_none());
    }

    #[test]
    fn fire_cuts_it_short() {
        let mut i = Intro::new();
        i.tick();
        i.skip();
        assert!(i.done);
    }

    #[test]
    fn every_step_walks_forward_one_at_a_time() {
        let mut i = Intro::new();
        for (n, step) in STEPS.iter().enumerate() {
            assert_eq!(i.card, n);
            assert_eq!(i.showing().map(|s| s.back), Some(step.back));
            i.next();
        }
        assert!(i.done);
    }

    /// The cast is spawned within the step it belongs to, so nothing is
    /// scheduled for a frame the step never reaches.
    #[test]
    fn no_figure_is_spawned_after_its_scene_ends() {
        for (n, step) in STEPS.iter().enumerate() {
            for s in step.cast {
                assert!(s.at < step.frames, "step {n}: {} starts too late", s.script);
            }
        }
    }
}
