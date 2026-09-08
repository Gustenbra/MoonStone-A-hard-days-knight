//! The intro sequence, out of `INTR.EXE`.
//!
//! `INTR.EXE` had never been examined. It unpacks the same way `MAIN.EXE` does
//! (PKLITE outside, Microsoft EXEPACK inside), and `tools/symbolmap.py` reads
//! it without a change: a 56,128 byte load image, four source modules and
//! **319 named symbols**, 262 of them independently corroborated.
//!
//! What that gives, in order of how much it settles:
//!
//! - **The intro is the same engine.** Module 2 is `_TASK` again, symbol for
//!   symbol: `PerformCOMMAND`, `PerformLOOP`, `TaskGoto`, `TaskGosub`,
//!   `TaskCelBuf`, `TaskCommandTable`. Module 0 is `GFX`, with `TextPTop`,
//!   `TextP`, `TextASCII` and a tile engine (`PlaceTile`, `FindTile`,
//!   `Dump_Tile`, `TileScreen`). Module 1 is `_LOADER`, module 3 the DOS error
//!   table. So the intro runs the animation VM this project already has.
//! - **Its asset list, by name.** `picfile1`..`picfile8` are `bg4`, `bg5a`,
//!   `bg3`, `bg2`, `bg2a`, `bg5`, `bg7`, `bg8`; `panfile1`..`panfile3` are
//!   `bg1a`, `bg1c`, `bg1b`; the cast is `au1`, `li1`, `da1`, `dw1`, `ha1`,
//!   `ov1`, `co1`, `dg1` and `klift1`; the font is `bold.f`; and `MesFILE` is
//!   `message.piv`, the same box the message system uses. The packs have had
//!   every one of them decoded and unused.
//! - **Its words**, which is what makes an intro possible at all. The strings
//!   are plain in the image from offset 18,551: the credits, the two title
//!   lines, `The End`, and three story cards.
//!
//! **What is not recovered.** `intro.sti` and `co.sti` are 960 bytes each and
//! `intro1.sti` is 105; the `.STI` format is not decoded, and the tile engine
//! that reads them is not built. The cast's animation scripts live in the
//! intro's own `DGROUP` and are not extracted. The captions' own x and y are
//! not recovered: unlike the message chains, nothing in the image points at
//! these strings, so they are drawn by code that builds a `TextTemp` record
//! from registers, and finding it means disassembling the intro's main module.
//!
//! **So what is here is: the plates, recovered; the words, recovered; which
//! word goes over which plate, and for how long, ours.** The cards below say
//! which is which.

use serde::{Deserialize, Serialize};

/// One held plate with words over it.
///
/// A `&'static` table rather than saved state, so it is not serializable and
/// does not need to be: it is content, and the only thing that ever moves is
/// [`Intro`]'s three integers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Card {
    /// The asset id of the full-screen picture.
    pub plate: &'static str,
    /// What is written over it, top line first.
    pub lines: &'static [&'static str],
    /// Ticks it holds. Sixty to the second.
    pub ticks: u32,
}

/// How long a card holds unless it says otherwise.
pub const HOLD: u32 = 220;

/// The sequence.
///
/// **The plate order is the original's own numbering.** `picfile1`..`picfile8`
/// and `panfile1`..`panfile3` are two runs of consecutive symbols, and the
/// filenames they point at are not in alphabetical order, so the numbering is
/// a real ordering and not an artefact: the three `bg1` panels first, then
/// `bg4`, `bg5a`, `bg3`, `bg2`, `bg2a`, `bg5`, `bg7`, `bg8`.
///
/// **Which words go on which plate is ours**, chosen by what the picture shows:
/// the moon opens, the tale is told over the henge, the knight under the
/// constellation ends it. The words themselves are the executable's.
pub const CARDS: &[Card] = &[
    Card { plate: "scene.bg1a", lines: &["MINDSCAPE PRESENTS"], ticks: 160 },
    Card { plate: "scene.bg1c", lines: &["MOONSTONE"], ticks: 160 },
    Card {
        plate: "scene.bg1b",
        lines: &["The druids sent their", "best knights to Stonehenge"],
        ticks: HOLD,
    },
    Card {
        plate: "scene.bg4",
        lines: &["so they may be dubbed", "into the", "Quest for the ", "MOONSTONE"],
        ticks: HOLD,
    },
    Card { plate: "scene.bg5a", lines: &["The ceremony of the", "Moonstone", "is about to begin"], ticks: HOLD },
    Card { plate: "scene.bg3", lines: &[], ticks: 120 },
    Card { plate: "scene.bg2", lines: &[], ticks: 120 },
    Card { plate: "scene.bg2a", lines: &[], ticks: 120 },
    Card { plate: "scene.bg5", lines: &[], ticks: 120 },
    Card { plate: "scene.bg7", lines: &[], ticks: 120 },
    Card {
        plate: "scene.bg8",
        lines: &[
            "And so, the tale of the",
            "Moonstone and the courage",
            "of the knights that fought",
            "for it is passed on from",
            "one generation to the next",
        ],
        ticks: 300,
    },
    // The credits. The six names stand without their headings, for the reason
    // `HEADINGS` gives; the conversion credit keeps its own, because the image
    // groups those four under it and nothing else.
    Card {
        plate: "scene.bg1c",
        lines: &[
            "Rob Anderson", "Todd Prescott", "Dennis Turner",
            "Richard Joseph", "Kevin Hoare", "Steve Leney",
        ],
        ticks: 220,
    },
    Card {
        plate: "scene.bg1a",
        lines: &[
            "conversion by",
            "Images Software Ltd",
            "Anthony Mack",
            "Nicholas Snape",
            "Audio Visual Magic",
        ],
        ticks: 220,
    },
];

/// The credit strings, verbatim and in the order the image holds them.
///
/// Every one of these is a string in `INTR.EXE` from offset 18,580, and they
/// are the names of the people who made the game, which is why they are quoted
/// exactly.
///
/// **The pairing is not recovered.** Six headings run `created by`, `Design
/// by`, `Artwork by`, `Programmed by`, `Music and Sound by`, `Additional Art
/// by`, and six names run `Rob Anderson`, `Todd Prescott`, `Dennis Turner`,
/// `Richard Joseph`, `Kevin Hoare`, `Steve Leney`, in two adjacent blocks.
/// Reading them off positionally would put the composer on the programming
/// line, so the order of the two blocks is plainly not the order they are drawn
/// in, and which heading goes with which name is in code this project has not
/// disassembled. So the six names are shown without headings rather than
/// under guessed ones. The conversion credit is different: `conversion by` is
/// followed by four names and no second heading, so that grouping is the
/// image's own.
pub const HEADINGS: &[&str] = &[
    "created by",
    "Design by",
    "Artwork by",
    "Programmed by",
    "Music and Sound by",
    "Additional Art by",
];
pub const NAMES: &[&str] = &[
    "Rob Anderson",
    "Todd Prescott",
    "Dennis Turner",
    "Richard Joseph",
    "Kevin Hoare",
    "Steve Leney",
];

/// Where the sequence has got to.
///
/// A state machine with no pixels in it, like the title and the select, so a
/// test drives it exactly as a keyboard does.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Intro {
    /// Which card is up.
    pub card: usize,
    /// Ticks this card has been up.
    pub held: u32,
    /// Finished, whether it ran out or was cut short.
    pub done: bool,
}

impl Intro {
    pub fn new() -> Intro {
        Intro::default()
    }

    /// The card being shown, or nothing once it is over.
    pub fn showing(&self) -> Option<&'static Card> {
        if self.done {
            return None;
        }
        CARDS.get(self.card)
    }

    /// One tick.
    pub fn tick(&mut self) {
        let Some(card) = self.showing() else {
            self.done = true;
            return;
        };
        self.held += 1;
        if self.held >= card.ticks {
            self.next();
        }
    }

    /// Move on. The last card ends the sequence.
    pub fn next(&mut self) {
        self.held = 0;
        self.card += 1;
        if self.card >= CARDS.len() {
            self.done = true;
        }
    }

    /// Fire. The original's intro can be cut short and so can this one:
    /// nobody wants to sit through it twice.
    pub fn skip(&mut self) {
        self.done = true;
    }

    /// How far through, in eighths, for anything that wants to draw progress.
    pub fn card_count(&self) -> usize {
        CARDS.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_card_names_a_plate_and_holds_for_a_while() {
        for c in CARDS {
            assert!(c.plate.starts_with("scene.bg"), "{} is an intro plate", c.plate);
            assert!(c.ticks > 0, "{} would flash past", c.plate);
        }
        assert_eq!(CARDS.len(), 13, "eleven plates, all eleven used, two of them twice");
    }

    /// The order is the original's own file numbering, which is not
    /// alphabetical and so is worth pinning.
    #[test]
    fn the_plate_order_is_the_recovered_numbering() {
        let order: Vec<&str> = CARDS.iter().map(|c| c.plate).collect();
        assert_eq!(
            &order[..11],
            &[
                "scene.bg1a", "scene.bg1c", "scene.bg1b", "scene.bg4", "scene.bg5a",
                "scene.bg3", "scene.bg2", "scene.bg2a", "scene.bg5", "scene.bg7", "scene.bg8",
            ],
            "panfile1..3 then picfile1..8"
        );
    }

    #[test]
    fn it_runs_through_and_stops() {
        let mut i = Intro::new();
        let total: u32 = CARDS.iter().map(|c| c.ticks).sum();
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
    fn every_card_walks_forward_one_at_a_time() {
        let mut i = Intro::new();
        for n in 0..CARDS.len() {
            assert_eq!(i.card, n);
            assert_eq!(i.showing().map(|c| c.plate), Some(CARDS[n].plate));
            i.next();
        }
        assert!(i.done);
    }
}
