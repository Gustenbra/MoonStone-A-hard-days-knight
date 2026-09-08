//! The message system: three kinds of box, one record, one chain walk.
//!
//! **Recovered, and more completely than the record promised.** A message in
//! the original is a linked list of ten-byte records in `DGROUP`:
//!
//! ```text
//! +0 u16  the text, as a DGROUP offset; NUL terminated
//! +2 u16  x
//! +4 u16  y
//! +6 u16  flags
//! +8 u16  the next record, and 0 ends the chain
//! ```
//!
//! and the flags are read by `GFX:TextPTop`:
//!
//! ```text
//! bit 0  centre between TextLeftBorder and TextRightBorder
//! bit 2  right align to TextRightBorder
//! bit 3  the bold face's kerning: three pixels off every glyph's advance
//! ```
//!
//! The routine at image 0x7a86 sets `TextTABLE` to the record and falls into
//! `TextPTop`, which draws the line; `TextPDone` reads `[+8]` and goes round
//! again until it is zero. That is the whole of `MESSAGE`.
//!
//! **The three kinds are three routines in `_LOADER`**, and they differ by
//! exactly one thing each. All three blit `MESSAGE.PIV`, set the font to the
//! bold face, walk the chain and fade:
//!
//! - `WAITMESSAGE` at image 36524 takes **no argument**. It reads
//!   `WaitMES[WaitCOUNT]`, draws it, then `inc WaitCOUNT` and wraps at
//!   fourteen. Its callers are the four places that used to sit waiting on a
//!   disk: `PracticeCombat5`, `InitKnightvsDemon`, `SetUpDKL` and `LoadWizard`.
//! - `OCCURMESSAGE` at 36587 takes a chain and shows it. Its callers are the
//!   things that happen: arriving at Highwood or Waterdeep (`LoadWasteBack`
//!   twice, once per city), `Valley` with `NoKeysMessage`, and
//!   `KnightWonGame`.
//! - `INSTRUCTMESSAGE` at 36631 takes a chain, shows it, and then writes its
//!   **own six-word palette ramp** (0x800, 0x600, 0x400, 0, 0x200, 0x100) over
//!   the fade table before fading, so it comes up in a different colour from
//!   the other two. Its callers are `KnightProtection`, `CheckLairClear`,
//!   `FightDemon`, `Henge`, `bac` and the stone circle's own `noswap`.
//!
//! So the three are not three formats; they are one format shown three ways,
//! and that is what is built here.
//!
//! **What is verbatim and what is not.** The fourteen wait chains, the two city
//! welcomes and the stone circle's line are quoted from the image with their
//! own x, y and flags. `HengeInstruct`, `GameOverMes`, `NoKeysMessage` and
//! `SHMES1`..`SHMES8` all sit below `DS:0b5a`, inside the 2,906 bytes of
//! `DGROUP` the unpacked image carries as a stale copy of another region
//! (`docs/REVERSING.md`), so their text is not readable and none of it is
//! guessed at here.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Which of the three routines shows this one.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// `WAITMESSAGE`. Argumentless: it takes the next of the fourteen.
    Wait,
    /// `OCCURMESSAGE`. Something has happened.
    Occurrence,
    /// `INSTRUCTMESSAGE`. The same box in its own colour.
    Instruction,
}

/// `TextPTop`'s three alignments.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Align {
    Left,
    Centre,
    Right,
}

/// Flag bits on a record's `+6`, named.
pub const FLAG_CENTRE: u16 = 1;
pub const FLAG_RIGHT: u16 = 4;
pub const FLAG_BOLD: u16 = 8;

/// One record of a chain.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub text: String,
    /// Ignored when the line is centred, which every recovered one is.
    pub x: i32,
    pub y: i32,
    pub align: Align,
    /// `CheckBOLD`'s bit. The original ORs it in when the current font is the
    /// bold one; a record that carries it already was authored for that face.
    pub bold: bool,
}

impl Line {
    /// Build a line from the record as it stands in the image, so the tables
    /// below read as the bytes do.
    pub fn new(text: &str, x: i32, y: i32, flags: u16) -> Line {
        Line {
            text: text.to_string(),
            x,
            y,
            align: if flags & FLAG_CENTRE != 0 {
                Align::Centre
            } else if flags & FLAG_RIGHT != 0 {
                Align::Right
            } else {
                Align::Left
            },
            bold: flags & FLAG_BOLD != 0,
        }
    }
}

/// A chain, and which routine shows it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub kind: Kind,
    pub lines: Vec<Line>,
}

impl Message {
    /// The lines a screen with no disk behind it should draw.
    ///
    /// Every one of the fourteen ends on `Loading...` at y 182, because in the
    /// original the box is up while a disk is read. Nothing here loads from a
    /// disk, so that last line is a lie and is left out of the drawing rather
    /// than out of the data: the table stays the bytes, and the decision not to
    /// print it is made here, once, where it can be seen.
    pub fn shown(&self) -> impl Iterator<Item = &Line> {
        self.lines.iter().filter(|l| l.text.trim() != "Loading...")
    }

    /// Whether anything would be drawn at all.
    pub fn is_empty(&self) -> bool {
        self.shown().next().is_none()
    }
}

/// The recovered `Loading...` line, kept so the tables below are the bytes.
fn loading() -> Line {
    Line::new("Loading...", 0, 182, FLAG_CENTRE | FLAG_BOLD)
}

/// Every message the executable gives up, and the counter the wait messages
/// are taken off.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Messages {
    wait: Vec<Message>,
    named: BTreeMap<String, Message>,
}

impl Default for Messages {
    fn default() -> Messages {
        Messages::recovered()
    }
}

impl Messages {
    /// The fourteen wait chains in `WaitMES` order, and the named chains whose
    /// text survived into the unpacked image.
    ///
    /// `WaitMES` is fourteen pointers and they are **not** in the order the
    /// labels are written: it reads `WaitM3A`, `WaitM2A`, `WaitM1A`, then
    /// `WaitM4A` onwards. This is the pointer order.
    pub fn recovered() -> Messages {
        let wait = WAIT
            .iter()
            .map(|lines| {
                let mut lines: Vec<Line> = lines
                    .iter()
                    .enumerate()
                    .map(|(i, text)| Line::new(text, 0, 75 + i as i32 * 20, FLAG_CENTRE))
                    .collect();
                lines.push(loading());
                Message { kind: Kind::Wait, lines }
            })
            .collect();

        let mut named = BTreeMap::new();
        // `WelHigh1a`..`WelHigh1e`, whose fourth record's text is swapped
        // between `WEMES1E` and `WEMES1F` by the two callers in
        // `LoadWasteBack`: one city each, and everything else the same chain.
        for (key, city) in [("welcome.highwood", "Highwood"), ("welcome.waterdeep", "Waterdeep")] {
            named.insert(
                key.to_string(),
                Message {
                    kind: Kind::Occurrence,
                    lines: vec![
                        Line::new("Welcome", 0, 60, FLAG_CENTRE),
                        Line::new("to the", 0, 80, FLAG_CENTRE),
                        Line::new("City", 0, 100, FLAG_CENTRE),
                        Line::new("of", 0, 120, FLAG_CENTRE),
                        Line::new(city, 0, 140, FLAG_CENTRE),
                    ],
                },
            );
        }
        // `_TAVERN:HengeWait`, which `noswap` hands to `INSTRUCTMESSAGE` on the
        // way into the stone circle.
        named.insert(
            "henge.ritual".to_string(),
            Message {
                kind: Kind::Instruction,
                lines: vec![
                    Line::new("The druids prepare", 0, 75, FLAG_CENTRE),
                    Line::new("for the ritual", 0, 95, FLAG_CENTRE),
                ],
            },
        );
        // `_LOADER:TitleMes`, drawn with the plain `MESSAGE` rather than by one
        // of the three, and the only place the original names its author. Its
        // own `Loading...` record is at y 150, not the 182 the fourteen wait
        // chains put theirs at, so it is written out rather than shared.
        named.insert(
            "title.credit".to_string(),
            Message {
                kind: Kind::Occurrence,
                lines: vec![
                    Line::new("created by", 0, 90, FLAG_CENTRE | FLAG_BOLD),
                    Line::new("Rob Anderson", 0, 105, FLAG_CENTRE | FLAG_BOLD),
                    Line::new("Loading...", 0, 150, FLAG_CENTRE | FLAG_BOLD),
                ],
            },
        );
        Messages { wait, named }
    }

    /// How many wait messages there are. Fourteen, which is what `WaitCOUNT`
    /// wraps at.
    pub fn wait_len(&self) -> usize {
        self.wait.len()
    }

    /// `WaitMES[n]`, with the wrap `WAITMESSAGE` applies.
    pub fn wait(&self, n: usize) -> &Message {
        &self.wait[n % self.wait.len()]
    }

    pub fn named(&self, key: &str) -> Option<&Message> {
        self.named.get(key)
    }

    /// Which message a place should show on arrival, if any.
    ///
    /// **Recovered as a mapping, ours as a lookup.** The original hard-codes
    /// it: `LoadWasteBack` calls `OCCURMESSAGE` with the welcome chain on the
    /// way into either city, and `noswap` calls `INSTRUCTMESSAGE` with
    /// `HengeWait` on the way into the circle. Nothing else in the recovered
    /// code shows a chain on arrival anywhere.
    pub fn on_entering(&self, place: &str) -> Option<&Message> {
        let key = match place {
            "highwood" => "welcome.highwood",
            "waterdeep" => "welcome.waterdeep",
            // The pack's id for the stone circle. `_TAVERN:noswap` is the
            // routine, inside the module that also holds `Henge`.
            "stones" => "henge.ritual",
            _ => return None,
        };
        self.named(key)
    }

    /// Whether arriving here takes one off the wait pile instead.
    ///
    /// **Recovered.** `WAITMESSAGE` has four callers and one of them is
    /// `_WIZARD:LoadWizard`, so the tower is the one door on the map that puts
    /// up the Gods' fourteen rather than a chain of its own. The other three
    /// are `PracticeCombat5`, `InitKnightvsDemon` and `SetUpDKL`, which are
    /// the practice bout and the two set pieces.
    pub fn waits_on_entering(&self, place: &str) -> bool {
        place == "wizard"
    }
}

/// The fourteen, in `WaitMES` pointer order, with the strings exactly as the
/// image holds them. Every one of them is followed by `Loading...`, which
/// [`Messages::recovered`] appends rather than repeating fourteen times.
const WAIT: [&[&str]; 14] = [
    &["Prepare yourself, for the ", "season of the Moonstones is", "upon you!"],
    &["The Gods pause for a moment", "to contemplate your fate..."],
    &["The Gods pause for a moment", " "],
    &[
        "Beware of the Ratmen",
        "during a full moon",
        "for they grow stronger",
        "as the moon gets fuller",
    ],
    &["Seek the knowledge", "of", "Mythral the Mystic"],
    &["Beware of the", "fierce Baloks", "of the", "Northern Wastelands"],
    &["Offer a magic item", "within Stonehenge", "and Danu will grant", "you a longer life"],
    &["Seek the wisdom of", "Math the wizard", "to aid you in your quest"],
    &["Visit your home village", "to restore lost lives."],
    &["The Gods turn their", "attentions away for", "a moment..."],
    &["The Gods pause for a moment"],
    &["The Gods await their", "new champion..."],
    &["Beware of the dreaded", "Black Knights", " "],
    &["Beware of the Dragon", "whose dark shadow", "sweeps the land"],
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_are_fourteen_wait_messages_and_the_counter_wraps() {
        let m = Messages::recovered();
        assert_eq!(m.wait_len(), 14, "WaitCOUNT wraps at fourteen");
        assert_eq!(m.wait(0).lines[0].text, "Prepare yourself, for the ");
        assert_eq!(m.wait(14), m.wait(0), "and fifteen is the first again");
        assert_eq!(m.wait(2).lines[0].text, "The Gods pause for a moment");
    }

    /// The order is `WaitMES`'s, not the labels'. Chain 2 is `WaitM1A`, which
    /// is the shortest of the three "pause for a moment" chains.
    #[test]
    fn the_wait_order_is_the_pointer_table_and_not_the_labels() {
        let m = Messages::recovered();
        assert_eq!(m.wait(0).lines[2].text, "upon you!", "WaitM3C");
        assert_eq!(m.wait(1).lines[1].text, "to contemplate your fate...", "WaitM2B");
        assert_eq!(m.wait(2).lines[1].text, " ", "WaitM1B, a single space");
    }

    #[test]
    fn every_wait_message_ends_on_the_loading_line_and_never_draws_it() {
        let m = Messages::recovered();
        for n in 0..m.wait_len() {
            let w = m.wait(n);
            assert_eq!(w.lines.last().unwrap().text, "Loading...");
            assert_eq!(w.lines.last().unwrap().y, 182);
            assert!(w.lines.last().unwrap().bold, "y 182 with flags 9");
            assert!(
                w.shown().all(|l| l.text != "Loading..."),
                "nothing here loads from a disk"
            );
        }
    }

    #[test]
    fn the_recovered_coordinates_are_twenty_apart_from_seventy_five() {
        let m = Messages::recovered();
        let w = m.wait(3);
        let ys: Vec<i32> = w.shown().map(|l| l.y).collect();
        assert_eq!(ys, vec![75, 95, 115, 135], "WaitM4A..WaitM4D");
        assert!(w.shown().all(|l| l.align == Align::Centre), "flag 1 is centre");
    }

    /// The three kinds are the point of the exercise: the right routine for
    /// the right event, chosen by what happened rather than by one special
    /// case.
    #[test]
    fn arriving_somewhere_chooses_the_right_kind_of_message() {
        let m = Messages::recovered();
        let high = m.on_entering("highwood").expect("a welcome for Highwood");
        assert_eq!(high.kind, Kind::Occurrence);
        assert_eq!(high.lines.last().unwrap().text, "Highwood");
        let deep = m.on_entering("waterdeep").expect("a welcome for Waterdeep");
        assert_eq!(deep.lines.last().unwrap().text, "Waterdeep");
        assert_eq!(deep.lines[0].text, "Welcome", "one chain, two cities");

        let henge = m.on_entering("stones").expect("the druids");
        assert_eq!(henge.kind, Kind::Instruction, "INSTRUCTMESSAGE, in its own colour");
        assert_eq!(henge.lines[0].text, "The druids prepare");

        assert!(m.on_entering("highwood.tavern").is_none(), "nothing greets a tavern");
        assert!(m.on_entering("nowhere").is_none());

        // The wizard is the odd one: `LoadWizard` calls the argumentless
        // routine, so his tower shows whichever of the fourteen is next.
        assert!(m.waits_on_entering("wizard"));
        assert!(!m.waits_on_entering("highwood"));
        assert!(m.on_entering("wizard").is_none(), "it has no chain of its own");
    }

    #[test]
    fn the_flag_word_decodes_the_way_textptop_reads_it() {
        assert_eq!(Line::new("x", 0, 0, FLAG_CENTRE).align, Align::Centre);
        assert_eq!(Line::new("x", 0, 0, FLAG_RIGHT).align, Align::Right);
        assert_eq!(Line::new("x", 0, 0, 0).align, Align::Left);
        assert!(Line::new("x", 0, 0, FLAG_CENTRE | FLAG_BOLD).bold);
        assert!(!Line::new("x", 0, 0, FLAG_CENTRE).bold);
    }

    #[test]
    fn messages_survive_serialization() {
        let m = Messages::recovered();
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<Messages>(&json).unwrap(), m);
    }
}
