//! The ending, which is the second half of `INTR.EXE`.
//!
//! The program reads its command tail at `PSP:0x82` and runs a different
//! sequence when it is given one:
//!
//! ```text
//! 0x000c  mov ax, es:[0x82]      ; the two characters of the command tail
//! 0x0010  sub ax, 0x3131         ; both are digits
//! 0x0013  cmp al, 3; ja          ; and both are '1' to '4'
//! 0x0017  cmp ah, 3; ja
//! 0x001c  add ax, 0x101
//! 0x001f  mov [0x12d1], ax
//! 0x0063  cmp [0x12d1], 0; je 0x6d      ; no tail: the intro
//! 0x006a  jmp 0xfa                      ; a tail: the ending
//! ```
//!
//! and the byte is `MAIN.EXE`'s exit code, which `MOON:KnightWonGame` builds
//! with the moon in the low nibble and the winning seat in the high one. So
//! `al` is the moon and `ah` the knight, and [`crate::quest::Tally::code`] is
//! the byte that carries them here.
//!
//! The sequence is `0x00fa`:
//!
//! ```text
//! 0x00fa  mov si, 0x13b9; call 0x381b   ; the CEREMONY card over MESSAGE.PIV
//! 0x0100  call 0x3a00                   ; the four plates and the six banks
//! 0x0103  mov [0x4105], 2               ; tune 2
//! 0x0109  call 0x547                    ; the moonstone, then the circle
//! 0x010c  call 0x628                    ; the dubbing
//! 0x010f  call 0x6db                    ; three more
//! 0x0112  call 0x3ae7                   ; bg7 and bg8 load here, not earlier
//! 0x0115  call 0x778                    ; the rise, then bg8 and the tale
//! 0x0118  mov si, 2; call 0x381b        ; `The End`, over MESSAGE.PIV again
//! 0x011e  mov ax, 0x64; call 0xfae      ; a hundred retraces
//! 0x0124  call 0x10c5                   ; out
//! 0x0127  jmp 0xd4                      ; and the program ends
//! ```
//!
//! A scene routine is always the same five things: blacken, clear the task
//! list, copy one of the loaded screens into the work page (`0x8d2`), point
//! `[0x40fb]` at that screen's own palette, spawn one or more scripts, and run
//! the loop at `0x129` until `[0x4101]` goes up. `[0x4101]` is set by `0x1e1`,
//! which the end-of-animation table at `DS:0x40cd` holds in slot 0, and every
//! spawn is made with `dl = 0`, so **a scene lasts exactly as long as the first
//! of its scripts to reach `ff ff`**. That is where every frame count below
//! comes from: it is a script's own length, not a choice.
//!
//! **What the two digits colour.** `0x3b9d` branches on `ah` and writes four
//! twelve-bit words at `si + 0x10`, which is palette entries 8 to 11, into each
//! of the four plates the loader hands it; `0x3b23` branches on `al` and writes
//! three at `si + 0x18`, `si + 0x1e` and `si + 0x2e`, which are entries 12, 15
//! and 23, into `bg2a`'s alone, along with three glow targets at `DS:0x4489`.
//! See [`knight_ink`] and [`moonstone_ink`].
//!
//! **What is ours:** how long the `CEREMONY` card is up, because in the
//! original that is however long four plates and six banks take to come off a
//! floppy, and the rounding of this program's 9.1033 frames a second onto the
//! engine's tick. The retrace counts are no longer rounded at all: the engine's
//! tick *is* a retrace now. Everything else below is the image's.

use crate::intro::{Line, Spawn, TICKS_PER_FRAME};
use serde::{Deserialize, Serialize};

/// What is behind a scene.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Back {
    /// `MESSAGE.PIV`, which `0x381b` puts up before it walks a chain. The two
    /// cards the ending opens and closes on are drawn over it, like the
    /// intro's story card and every message in the game.
    Message,
    /// One of the loaded plates, copied whole into the work page by `0x8d2`.
    Plate(&'static str),
    /// The camera rising off `bg7` up `CO.STI`'s panorama, from `from` to `to`.
    Rise { from: u32, to: u32 },
}

/// `[0x1581]` and `[0x13b7]`: whether `0x7f9` stamps `OV1.CEL` over the plate
/// every frame, and how much of it.
///
/// `0x7f9` blits four cels of the bank at `DS:0x4485`, which is `ov1.cel`, at
/// fixed places; the middle two are skipped when `[0x13b7]` is 2. The only
/// scene that turns the overlay on sets `[0x13b7]` to 2 first and clears both
/// when it is done, so **cels 1 and 2 are never drawn in the shipped program**
/// and the table below carries them anyway, because they are what the code
/// says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overlay {
    None,
    /// Cels 0 and 3 only, which is `[0x13b7] == 2`.
    Near,
}

/// One cel of the overlay: `ax` is the cel, `bx + 0xa0` the x and `cx + 0x64`
/// the y, so the table is written about the middle of the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverlayCel {
    pub cel: usize,
    pub x: i32,
    pub y: i32,
    /// False for the two `[0x13b7] == 2` leaves out.
    pub near: bool,
}

/// `0x7f9`, read straight off the registers it loads.
///
/// ```text
/// ax = 0   cl = 0xd4 + 0x64 -> 56    bx = 0xff60 + 0xa0 -> 0
/// ax = 3   cx = 0x22 + 0x64 -> 134   bx = 0xff93 + 0xa0 -> 51
/// ax = 1   cx = 0x04 + 0x64 -> 104   bx = 0xfffd + 0xa0 -> 157
/// ax = 2   cl = 0xee + 0x64 -> 82    bx = 0x0089 + 0xa0 -> 297
/// ```
// Hand-aligned: one cel of the overlay per row, in the order `0x7f9` draws them.
#[rustfmt::skip]
pub const OVERLAY: [OverlayCel; 4] = [
    OverlayCel { cel: 0, x:   0, y:  56, near: true  },
    OverlayCel { cel: 3, x:  51, y: 134, near: true  },
    OverlayCel { cel: 1, x: 157, y: 104, near: false },
    OverlayCel { cel: 2, x: 297, y:  82, near: false },
];

/// The bank `0x7f9` stamps, which is `ovfile1`, `ov1.cel`.
pub const OVERLAY_BANK: &str = "bank.ov1";

/// One scene.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scene {
    pub back: Back,
    pub lines: &'static [Line],
    pub cast: &'static [Spawn],
    pub overlay: Overlay,
    /// `[0x40c1]` and `[0x40c3]`, which `0x1ef` and `0x207` add to the z every
    /// task they start is placed at. Only one scene sets them.
    pub z: (i32, i32),
    /// How many of the program's own frames the scene lasts, which is the
    /// length of the script that ends it. Zero means the scene is held for a
    /// count of vertical retraces instead, and [`scene_ticks`] says which.
    pub frames: u32,
}

const NO_LINES: &[Line] = &[];
const NO_CAST: &[Spawn] = &[];

// --------------------------------------------------------------- the colours

/// `0x3b9d`, the routine `MOON:KnightWonGame`'s high nibble reaches.
///
/// `add si, 0x10` then four words, so palette entries 8, 9, 10 and 11 of
/// whichever plate's palette it was handed. The loader at `0x3a00` hands it
/// `bg5`, `bg5a`, `bg3` and `bg2a`, in that order, and nothing hands it `bg7`
/// or `bg8`, which are loaded later and never recoloured.
///
/// `ah` is 1 for seat 3, 2 for seat 0, 3 for seat 1 and 4 for seat 2, and
/// `BNAME`..`RNAME` make those blue, gold, emerald and red, so the four words
/// are the winning knight's own: **four for four**.
// Hand-aligned: one knight per row, `ah` ascending.
#[rustfmt::skip]
pub const KNIGHT_INK: [[u16; 4]; 4] = [
    [0xe00, 0x900, 0x600, 0x300],  // ah = 1, seat 3, red
    [0x05d, 0x028, 0x016, 0x003],  // ah = 2, seat 0, blue
    [0xfa0, 0xb40, 0x930, 0x710],  // ah = 3, seat 1, gold
    [0x0c5, 0x082, 0x061, 0x040],  // ah = 4, seat 2, green
];

/// The first entry `0x3b9d` writes: `si + 0x10` is the eighth word.
pub const KNIGHT_FIRST: usize = 8;

/// `0x3b23`, the routine the low nibble reaches, which is the stone itself.
///
/// Three words at `si + 0x18`, `si + 0x1e` and `si + 0x2e` of `bg2a`'s palette,
/// which are entries 12, 15 and 23, and three glow targets beside them at
/// `DS:0x4489`, `0x448b` and `0x448d`. `al` of 1 is not written at all: the
/// routine tests 3, 2 and 4 and falls through to its `ret`, and `al` can only
/// be 1 when `KnightWonGame` took none of its three branches, which cannot
/// happen on a win.
pub const MOONSTONE_AT: [usize; 3] = [12, 15, 23];

/// `al` 2, 3 and 4, which are the moons `0x2e`, `0x31` and `0x2d`: the three
/// words and the three they glow towards.
// Hand-aligned: the plate's three entries, then the three glow targets.
#[rustfmt::skip]
pub const MOONSTONE_INK: [([u16; 3], [u16; 3]); 3] = [
    ([0x000, 0x222, 0x444], [0x111, 0x333, 0x555]),  // al = 2
    ([0xf80, 0xc50, 0x920], [0xc50, 0x920, 0x700]),  // al = 3
    ([0xb40, 0xd60, 0xf80], [0xd60, 0xf80, 0xfa0]),  // al = 4
];

/// The four words for the knight in the high nibble of an exit byte, or nothing
/// when the nibble is not one of the four the routine tests.
pub fn knight_ink(code: u8) -> Option<[u16; 4]> {
    let ah = code >> 4;
    if (1..=4).contains(&ah) {
        Some(KNIGHT_INK[ah as usize - 1])
    } else {
        None
    }
}

/// The three words and their three glow targets for the moon in the low
/// nibble, or nothing for the `al = 1` the routine leaves alone.
pub fn moonstone_ink(code: u8) -> Option<([u16; 3], [u16; 3])> {
    let al = code & 0x0f;
    if (2..=4).contains(&al) {
        Some(MOONSTONE_INK[al as usize - 2])
    } else {
        None
    }
}

/// The plates `0x3a00` hands to `0x3b9d`, which are the four the knight's
/// colours go into.
pub const KNIGHT_PLATES: [&str; 4] = ["scene.bg5", "scene.bg5a", "scene.bg3", "scene.bg2a"];

/// The one plate `0x3b23` writes, which is the one the stone is on.
pub const MOONSTONE_PLATE: &str = "scene.bg2a";

/// `0x50d`, which `0x3531` gosubs on its third frame: three `COLOURGLOW`
/// records on the three entries `0x3b23` wrote, period one, repeating for
/// ever. The three handles are kept in `[0x13b1]`, `[0x13b3]` and `[0x13b5]`
/// and `0x547` zeroes all three through `0xa4a` when the scene ends, which is
/// how a glow is taken out.
pub const MOONSTONE_GLOW_FRAME: u32 = 2;
pub const MOONSTONE_GLOW_PERIOD: u16 = 1;

// ------------------------------------------------------------------ the rise

/// `CO.STI`, baked as a second panorama the way `INTRO.STI` is.
pub const RISE_SHEET: &str = "scene.copan";

/// The palette the rise is shown in: `0x778` points `[0x40fb]` at `0x448f`,
/// which `0x3ae7` filled from `bg7`.
pub const RISE_PALETTE: &str = "palette.scene.bg7";

/// Where the rise starts. `0x0db2` writes `[0x160] = 0x3e8`, and the bottom
/// eight of `CO.STI`'s forty eight rows are `bg7` whole, so the window opens on
/// exactly that picture.
pub const RISE_FROM: u32 = 1000;

/// The speed, frame by frame.
///
/// `0x0dca` writes `[0x162] = 9`, `0x0e12` subtracts it from `[0x160]` once a
/// frame while `bx` has bit 3 set, and the script `0x3ddb` gosubs `0xe05` on
/// each of its first eight frames, which takes one off the speed. Its ninth
/// frame holds for forty and its tenth gosubs `0xdfe`, which sets `[0x1277]`
/// and makes `0xdda` return. So the camera lifts quickly, settles to a crawl
/// and stops eighty five pixels up.
// Hand-aligned: the eight decelerating frames, then the forty one at a crawl.
#[rustfmt::skip]
pub const RISE_SPEEDS: [u32; 49] = [
    9, 8, 7, 6, 5, 4, 3, 2,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1,
];

/// Where the rise ends, which is [`RISE_FROM`] less the whole schedule.
pub const RISE_TO: u32 = RISE_FROM - 85;

/// How far up the panorama the camera has got after `frames` of the rise.
pub fn rise_at(frames: u32) -> u32 {
    let mut moved = 0;
    for (i, step) in RISE_SPEEDS.iter().enumerate() {
        if i as u32 >= frames {
            break;
        }
        moved += step;
    }
    RISE_FROM - moved.min(RISE_FROM - RISE_TO)
}

// ------------------------------------------------------------------ the waits

/// `0xfae` counts **vertical retraces**, and the engine's tick is one vertical
/// retrace: the wait at image `0x5a24`, at the 320x200 VGA mode's 70.0863 Hz.
/// So a retrace count is a tick count and there is nothing to convert. This used
/// to be `n * 6 / 7`, the rounding onto a sixty tick engine, and it was the one
/// piece of arithmetic here that was not the image's.
pub const fn retraces(n: u32) -> u32 {
    n
}

/// **Ours.** How long the `CEREMONY` card is up. In the original it is however
/// long four plates and six sprite banks take to come off a floppy, which is
/// not a number that can be recovered or reproduced. The intro's own logo card
/// is held for the same reason and by the same kind of number.
pub const CEREMONY_TICKS: u32 = 130;

/// `0x07d7`: `ax = 0x64`, a hundred retraces of `bg8` before the chain goes on
/// it, and `0x07e9`: `ax = 0x1f4`, five hundred with it up. Both are on the
/// same plate, so they are one scene here and the chain appears part way
/// through it.
pub const TALE_BEFORE: u32 = retraces(0x64);
pub const TALE_AFTER: u32 = retraces(0x1f4);

/// `0x011e`: `ax = 0x64`, a hundred retraces of `The End`.
pub const THE_END_TICKS: u32 = retraces(0x64);

// --------------------------------------------------------------- the sequence

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

/// The sequence, scene by scene.
// Hand-aligned: the spawn tables are grouped the way the routines group them.
#[rustfmt::skip]
pub const SCENES: &[Scene] = &[
    // `0x00fa`: the card the ending opens on, over `MESSAGE.PIV`, while the
    // plates and the banks load.
    Scene {
        back: Back::Message,
        lines: crate::intro::CEREMONY,
        cast: NO_CAST,
        overlay: Overlay::None,
        z: (0, 0),
        frames: 0,
    },
    // `0x0547`: `bg2a`, the one plate the stone's own three entries are written
    // into, with the overlay on and `0x3531` the only script. Thirty four
    // frames is that script's own length.
    Scene {
        back: Back::Plate("scene.bg2a"),
        lines: NO_LINES,
        cast: &[r("3531", 0)],
        overlay: Overlay::Near,
        z: (0, 0),
        frames: 34,
    },
    // `0x05ab`: the overlay off, `bg3`, the ten standing druids and `0x3ceb`,
    // which is forty frames long and is what ends the scene.
    Scene {
        back: Back::Plate("scene.bg3"),
        lines: NO_LINES,
        cast: &[
            r("2e75", 0), r("2e99", 0), r("2e51", 0), r("309f", 0), r("3221", 0),
            lf("2e75", 0), lf("2e99", 0), lf("2e51", 0), lf("309f", 0), lf("3221", 0),
            r("3ceb", 0),
        ],
        overlay: Overlay::None,
        z: (0, 0),
        frames: 40,
    },
    // `0x0628`: `bg5`, two scripts that loop, and then the word table at
    // `DS:0x000c`, five of `0x35cf` sixteen frames apart, alternating right and
    // left the way `0x34b`'s toggle at `[0x13a5]` does. The scene is not ended
    // by a script: `0x34b` returns after the fifth and `0x0690` counts forty
    // one more frames itself, so eighty and forty one.
    Scene {
        back: Back::Plate("scene.bg5"),
        lines: NO_LINES,
        cast: &[
            r("3a49", 0), r("3b07", 0),
            r("35cf", 0), lf("35cf", 16), r("35cf", 32), lf("35cf", 48), r("35cf", 64),
        ],
        overlay: Overlay::None,
        z: (5, 0xf),
        frames: 121,
    },
    // `0x06bc`: the same plate, the task list cleared and three more scripts.
    // `0x393d` is forty three frames and ends it.
    Scene {
        back: Back::Plate("scene.bg5"),
        lines: NO_LINES,
        cast: &[r("3a49", 0), r("393d", 0), r("393d", 0)],
        overlay: Overlay::None,
        z: (0, 0),
        frames: 43,
    },
    // `0x06db`, `0x070d` and `0x0745`: three scenes of one script each.
    Scene {
        back: Back::Plate("scene.bg5a"),
        lines: NO_LINES,
        cast: &[r("2351", 0)],
        overlay: Overlay::None,
        z: (0, 0),
        frames: 29,
    },
    Scene {
        back: Back::Plate("scene.bg5"),
        lines: NO_LINES,
        cast: &[r("3a49", 0), r("3b1f", 0)],
        overlay: Overlay::None,
        z: (0, 0),
        frames: 15,
    },
    Scene {
        back: Back::Plate("scene.bg5a"),
        lines: NO_LINES,
        cast: &[r("3c75", 0)],
        overlay: Overlay::None,
        z: (0, 0),
        frames: 14,
    },
    // `0x0778`: the rise. `0xd8c` sets the panorama up at the bottom, `0xdda`
    // scrolls it up until `0x3ddb` stops it, and then `0x129` runs on to the
    // end of that script, which is ninety three frames from its start.
    Scene {
        back: Back::Rise { from: RISE_FROM, to: RISE_TO },
        lines: NO_LINES,
        cast: &[r("3ddb", 0)],
        overlay: Overlay::None,
        z: (0, 0),
        frames: 93,
    },
    // `0x07bd`: `bg8`, held, then the tale written over it. This is the one
    // chain in the ending that is **not** drawn over `MESSAGE.PIV`: `0x07e0`
    // calls `0x3129`, the bare chain walker, with the plate already up.
    Scene {
        back: Back::Plate("scene.bg8"),
        lines: crate::intro::TALE,
        cast: NO_CAST,
        overlay: Overlay::None,
        z: (0, 0),
        frames: 0,
    },
    // `0x0118`: `The End`, over `MESSAGE.PIV` like the card it opened on.
    Scene {
        back: Back::Message,
        lines: crate::intro::THE_END,
        cast: NO_CAST,
        overlay: Overlay::None,
        z: (0, 0),
        frames: 0,
    },
];

/// Which scene is `bg8` and the tale, so the chain can arrive part way through
/// it the way `0x0778` puts it there.
pub const TALE_SCENE: usize = 9;

/// How long a scene is, in this engine's ticks.
pub fn scene_ticks(n: usize) -> u32 {
    let Some(scene) = SCENES.get(n) else { return 0 };
    if scene.frames > 0 {
        return scene.frames * TICKS_PER_FRAME;
    }
    match scene.back {
        Back::Message if n == 0 => CEREMONY_TICKS,
        Back::Message => THE_END_TICKS,
        Back::Plate(_) => TALE_BEFORE + TALE_AFTER,
        Back::Rise { .. } => TICKS_PER_FRAME,
    }
}

/// Whether the tale's chain is up yet, which it is after the first hundred
/// retraces of the plate it goes on.
pub fn lines_showing(n: usize, held: u32) -> bool {
    n != TALE_SCENE || held >= TALE_BEFORE
}

// ------------------------------------------------------------------- the state

/// Where the ending has got to, and which exit byte it was given.
///
/// A state machine with no pixels in it, like the intro's, so a test drives it
/// exactly as the program does.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ending {
    /// The exit byte `MAIN.EXE` would have quit with: the moon in the low
    /// nibble, the winning seat in the high one.
    pub code: u8,
    pub card: usize,
    pub held: u32,
    /// Where the rise has got to down the panorama.
    pub pan: u32,
    pub done: bool,
}

impl Ending {
    /// Start the ending with the byte `MOON:KnightWonGame` put in `al`.
    pub fn new(code: u8) -> Ending {
        Ending {
            code,
            pan: RISE_FROM,
            ..Ending::default()
        }
    }

    pub fn showing(&self) -> Option<&'static Scene> {
        if self.done {
            return None;
        }
        SCENES.get(self.card)
    }

    /// Which of the program's own frames this scene is on.
    pub fn frame(&self) -> u32 {
        self.held / TICKS_PER_FRAME
    }

    /// Whether this scene's chain is up.
    pub fn lines(&self) -> &'static [Line] {
        match self.showing() {
            Some(s) if lines_showing(self.card, self.held) => s.lines,
            _ => &[],
        }
    }

    pub fn tick(&mut self) {
        let Some(scene) = self.showing() else {
            self.done = true;
            return;
        };
        if let Back::Rise { .. } = scene.back {
            self.pan = rise_at(self.frame());
        }
        self.held += 1;
        if self.held >= scene_ticks(self.card) {
            self.next();
        }
    }

    pub fn next(&mut self) {
        self.held = 0;
        self.card += 1;
        if self.card >= SCENES.len() {
            self.done = true;
            return;
        }
        if let Some(Back::Rise { from, .. }) = self.showing().map(|s| s.back) {
            self.pan = from;
        }
    }

    /// Fire. `0x00ea` tests the button every frame of the intro's own scene
    /// loop and the ending runs the same loop, so the ending skips too.
    pub fn skip(&mut self) {
        self.done = true;
    }

    pub fn card_count(&self) -> usize {
        SCENES.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// As in [`crate::intro`]: these are counts of **vertical retraces** off
    /// `0xfae`, not of the 54.6204 Hz timer `Combat` waits on, so moving the
    /// arena onto the timer must leave every one of them where it is.
    #[test]
    fn the_retrace_counts_are_the_recovered_ones() {
        assert_eq!(CEREMONY_TICKS, 130);
        assert_eq!(TALE_BEFORE, 100);
        assert_eq!(TALE_AFTER, 500);
        assert_eq!(THE_END_TICKS, 100);
        // `retraces` is the identity, because the engine's tick on every screen
        // but the arena *is* one retrace.
        assert_eq!(retraces(0x1f4), 0x1f4);
    }

    #[test]
    fn every_scene_holds_for_a_while_and_names_something_to_draw() {
        for (n, s) in SCENES.iter().enumerate() {
            assert!(scene_ticks(n) > 0, "scene {n} would flash past");
            if let Back::Plate(p) = s.back {
                assert!(p.starts_with("scene.bg"), "{p} is an ending plate");
            }
        }
    }

    /// The plates are the ones the ending's own loader and scene routines name,
    /// and `bg7` is behind the rise rather than blitted as a plate.
    #[test]
    fn the_plates_are_the_recovered_ones() {
        let plates: Vec<&str> = SCENES
            .iter()
            .filter_map(|s| match s.back {
                Back::Plate(p) => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(
            plates,
            vec![
                "scene.bg2a",
                "scene.bg3",
                "scene.bg5",
                "scene.bg5",
                "scene.bg5a",
                "scene.bg5",
                "scene.bg5a",
                "scene.bg8",
            ]
        );
        // `bg5`, `bg7` and `bg8` are the three the intro deliberately leaves
        // out, and two of them are here.
        assert!(plates.contains(&"scene.bg8"));
        assert_eq!(
            SCENES
                .iter()
                .filter(|s| matches!(s.back, Back::Rise { .. }))
                .count(),
            1
        );
    }

    /// The exit byte is read the way the entry reads it: `al` the moon, `ah`
    /// the knight, and the four seats four for four.
    #[test]
    fn the_exit_byte_colours_the_knight_and_the_stone() {
        // `KnightWonGame` folds seat 3 to 1, 0 to 2, 1 to 3 and 2 to 4.
        assert_eq!(knight_ink(0x12), Some([0xe00, 0x900, 0x600, 0x300]));
        assert_eq!(knight_ink(0x24), Some([0x05d, 0x028, 0x016, 0x003]));
        assert_eq!(knight_ink(0x33), Some([0xfa0, 0xb40, 0x930, 0x710]));
        assert_eq!(knight_ink(0x42), Some([0x0c5, 0x082, 0x061, 0x040]));
        assert_eq!(knight_ink(0x04), None, "no seat, no colours");
        assert_eq!(knight_ink(0x54), None);
        // The three moons `0x3b23` tests, and the one it leaves alone.
        assert_eq!(
            moonstone_ink(0x22),
            Some(([0x000, 0x222, 0x444], [0x111, 0x333, 0x555]))
        );
        assert_eq!(
            moonstone_ink(0x23),
            Some(([0xf80, 0xc50, 0x920], [0xc50, 0x920, 0x700]))
        );
        assert_eq!(
            moonstone_ink(0x24),
            Some(([0xb40, 0xd60, 0xf80], [0xd60, 0xf80, 0xfa0]))
        );
        assert_eq!(moonstone_ink(0x21), None, "`al = 1` is never written");
    }

    /// Every byte `quest::Tally::code` can produce lands on a knight and a
    /// moon the ending knows how to colour, which is the whole of the wiring
    /// between the two executables.
    #[test]
    fn every_tally_byte_the_game_can_quit_with_is_understood() {
        for seat in 0..4u8 {
            let nibble = match seat {
                3 => 0x10,
                0 => 0x20,
                1 => 0x30,
                _ => 0x40,
            };
            for moon in [2u8, 4, 3] {
                let code = nibble | moon;
                assert!(knight_ink(code).is_some(), "{code:#04x}");
                assert!(moonstone_ink(code).is_some(), "{code:#04x}");
            }
        }
    }

    /// The rise goes up, reaches its end and stops there.
    #[test]
    fn the_rise_lifts_and_settles() {
        assert_eq!(rise_at(0), RISE_FROM);
        assert_eq!(rise_at(1), RISE_FROM - 9, "the first frame is the fastest");
        assert_eq!(rise_at(8), RISE_FROM - 44);
        assert_eq!(rise_at(RISE_SPEEDS.len() as u32), RISE_TO);
        assert_eq!(rise_at(1_000), RISE_TO, "and it does not run past the end");
        assert_eq!(RISE_TO, 915);
    }

    #[test]
    fn no_figure_is_spawned_after_its_scene_ends() {
        for (n, s) in SCENES.iter().enumerate() {
            for spawn in s.cast {
                assert!(
                    s.frames > 0 && spawn.at < s.frames,
                    "scene {n}: {} starts too late",
                    spawn.script
                );
            }
        }
    }

    #[test]
    fn it_runs_through_and_stops() {
        let mut e = Ending::new(0x24);
        let total: u32 = (0..SCENES.len()).map(scene_ticks).sum();
        for _ in 0..total {
            assert!(!e.done);
            e.tick();
        }
        assert!(e.done, "it ends rather than looping");
        assert!(e.showing().is_none());
    }

    #[test]
    fn the_tale_arrives_part_way_through_its_plate() {
        let mut e = Ending::new(0x24);
        while e.card < TALE_SCENE && !e.done {
            e.next();
        }
        assert_eq!(e.card, TALE_SCENE);
        assert!(e.lines().is_empty(), "the plate is up on its own first");
        for _ in 0..TALE_BEFORE {
            e.tick();
        }
        assert_eq!(e.lines(), crate::intro::TALE);
    }

    #[test]
    fn fire_cuts_it_short() {
        let mut e = Ending::new(0x24);
        e.tick();
        e.skip();
        assert!(e.done);
    }

    /// Every line of both cards is on the screen, the way the intro's are.
    #[test]
    fn every_line_is_on_the_screen() {
        for s in SCENES {
            for l in s.lines {
                assert!(l.y >= 0 && l.y < 200, "{l:?} is off the screen");
                assert!(!l.text.is_empty());
            }
        }
    }
}
