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
//! - `INSTRUCTMESSAGE` at 36631 takes a chain, shows it, and then writes
//!   **six words into the picture's own palette** before fading it in:
//!
//!   ```text
//!   0x8f38  mov si, 0x80bb                 ; the loaded picture's palette
//!   0x8f3b  mov word ptr [si + 2], 0x800   ; entry 1
//!   0x8f40  mov word ptr [si + 4], 0x600   ; entry 2
//!   0x8f45  mov word ptr [si + 6], 0x400   ; entry 3
//!   0x8f4a  mov word ptr [si + 8], 0       ; entry 4
//!   0x8f4f  mov word ptr [si + 0xa], 0x200 ; entry 5
//!   0x8f54  mov word ptr [si + 0xc], 0x100 ; entry 6
//!   0x8f59  call 0x5a3e                    ; the page flip
//!   0x8f5c  mov si, 0x80bb; call 0x5b44    ; the fade in, from that palette
//!   ```
//!
//!   so it comes up in a different colour from the other two: `MESSAGE.PIV`'s
//!   four purples at 1 to 4 and the two entries after them go red. Its callers
//!   are `KnightProtection`, `CheckLairClear`, `FightDemon`, `Henge`, `bac` and
//!   the stone circle's own `noswap`. `0x8761`, which every one of the three
//!   calls first through `0x8e90`, reads the palette back out of the picture's
//!   header, so the ramp lasts one message.
//!
//! So the three are not three formats; they are one format shown three ways,
//! and that is what is built here.
//!
//! **What takes the box down is the caller's business, not the routine's.**
//! None of the three waits: each blits the picture, walks the chain, flips the
//! page and fades in, and returns. Scanning the image for every call gives two
//! shapes of caller and no third:
//!
//! ```text
//! 0x108b  Henge+56          call INSTRUCTMESSAGE   ; HengeInstruct
//! 0x108e  Henge+59          call 0x8251            ; WaitFIRE: fire down, then up
//! 0x1091  Henge+62          call 0x5b65            ; the sixteen-step fade out
//!
//! 0x8e2f  LoadWasteBack+166 call OCCURMESSAGE      ; WelHigh1a, `Waterdeep`
//! 0x8e35                    call 0x875e            ; load the city's picture
//! 0x8e3b                    call 0x5b65            ; the fade out
//! ```
//!
//! `KnightProtection+51`, `CheckLairClear+64` (the game over at 0x617),
//! `Valley+19`, `FightDemon+71` and `KnightWonGame+75` are the first shape:
//! the box stays until fire. `WAITMESSAGE`'s four callers, the two city
//! welcomes and `noswap+31` (`HengeWait`, straight into loading `Hen1.p`) are
//! the second: the box covers a disk read and goes out when the read is done,
//! and fire does nothing to it. [`Until`] is that difference, carried on the
//! message because the routine that shows it cannot tell.
//!
//! **Every chain here is quoted from the image** with its own x, y and flags:
//! the fourteen wait chains, the two city welcomes, `HengeInstruct` (image
//! 0x129f9), `HengeWait` (0x1f10c), `SCR_PRO` (0x12965), `NextDayMes` (0x1b19a)
//! and `TitleMes` (0x1b17c). `SHMES1`..`SHMES8` (0x12a35 onwards) are not
//! records but the strings `HengeInstruct` and `VICTORY` point at; `VICTORY`,
//! `GameOverMes`, `NoKeysMessage` and `ValleyEnter` are in [`crate::quest`].

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

/// What takes the box down again.
///
/// The routine that shows a chain does not wait, so this is read off what the
/// caller does next (see the module note): `call 0x8251` or a load.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Until {
    /// `WaitFIRE` at image 0x8251: `call 0x81ec; test bx, 0x10; je` until fire
    /// is down, then the same until it is up again. Then the fade out.
    Fire,
    /// A disk read, which nothing here does. The box goes out on the fade
    /// the loader ends on, and fire does nothing to it.
    Loaded,
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

/// A chain, which routine shows it, and what takes it down.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub kind: Kind,
    pub until: Until,
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

/// The record at DS:0x5dd (image 0x1298d), `Press fire to continue` centred,
/// which `SCR_PRO`, `HengeInstruct` and `NextDayMes` all end on at y 182, and
/// `Vee7` (0x12b10) repeats at y 180 for `ValleyEnter`, `NoKeysMessage` and
/// `GameOverMes`. The string itself is `promes4`, DS:0x632.
pub fn press_fire(y: i32) -> Line {
    Line::new("Press fire to continue", 0, y, FLAG_CENTRE)
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
                Message {
                    kind: Kind::Wait,
                    until: Until::Loaded,
                    lines,
                }
            })
            .collect();

        let mut named = BTreeMap::new();
        // `WelHigh1a`..`WelHigh1e`, whose fourth record's text is swapped
        // between `WEMES1E` and `WEMES1F` by the two callers in
        // `LoadWasteBack`: one city each, and everything else the same chain.
        for (key, city) in [
            ("welcome.highwood", "Highwood"),
            ("welcome.waterdeep", "Waterdeep"),
        ] {
            named.insert(
                key.to_string(),
                Message {
                    kind: Kind::Occurrence,
                    // `0x8e35 call 0x875e` loads the city's picture straight
                    // after, and `0x8e3b` fades out.
                    until: Until::Loaded,
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
        // `MOON:HengeInstruct`, image 0x129f9: what `Henge+56` (0x1088) hands
        // to `INSTRUCTMESSAGE` when the moon is not yours, before the offering
        // page. Four records of its own, then the shared `Press fire to
        // continue` record at DS:0x5dd (image 0x1298d), and `Henge+59` is
        // `WaitFIRE`.
        //
        // ```text
        // 129f9  text 0685 x 0 y  75 flags 1 next 0653   To be granted a
        // 12a03  text 0695 x 0 y  95 flags 1 next 065d   longer life you must
        // 12a0d  text 06aa x 0 y 115 flags 1 next 0667   offer an item of
        // 12a17  text 06bc x 0 y 135 flags 1 next 05dd   magical nature to Danu
        // 1298d  text 0632 x 0 y 182 flags 1 next 0000   Press fire to continue
        // ```
        //
        // The four strings are `SHMES1`..`SHMES4` at image 0x12a35, 0x12a45,
        // 0x12a5a and 0x12a6c. `offer an item of ` carries a trailing space in
        // the image and keeps it here.
        named.insert(
            "henge.instruct".to_string(),
            Message {
                kind: Kind::Instruction,
                until: Until::Fire,
                lines: vec![
                    Line::new("To be granted a", 0, 75, FLAG_CENTRE),
                    Line::new("longer life you must", 0, 95, FLAG_CENTRE),
                    Line::new("offer an item of ", 0, 115, FLAG_CENTRE),
                    Line::new("magical nature to Danu", 0, 135, FLAG_CENTRE),
                    press_fire(182),
                ],
            },
        );
        // `_TAVERN:HengeWait`, image 0x1f10c, which `noswap+31` (0xb375)
        // hands to `INSTRUCTMESSAGE` once something has been offered, and
        // then loads `Hen1.p` straight over: `0xb37d mov dx, HengeFILE1; call
        // 0x875e`. The strings are `SHMES5` and `SHMES6`, in `_TAVERN`.
        named.insert(
            "henge.ritual".to_string(),
            Message {
                kind: Kind::Instruction,
                until: Until::Loaded,
                lines: vec![
                    Line::new("The druids prepare", 0, 75, FLAG_CENTRE),
                    Line::new("for the ritual", 0, 95, FLAG_CENTRE),
                ],
            },
        );
        // `_LOADER:NextDayMes`, image 0x1b19a: `Next Day` at y 95 and then the
        // same shared `Press fire to continue` record at y 182 that
        // `HengeInstruct` ends on. The routine at 0x8e5b walks it over `CH.PIV`
        // rather than over `MESSAGE.PIV`, and `NextWHICH+35` is `WaitFIRE`.
        named.insert(
            "next.day".to_string(),
            Message {
                kind: Kind::Occurrence,
                until: Until::Fire,
                lines: vec![Line::new("Next Day", 0, 95, FLAG_CENTRE), press_fire(182)],
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
                until: Until::Loaded,
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
            // The pack's id for the stone circle. `Henge+56` (0x1088) puts
            // `HengeInstruct` up before the offering page; `HengeWait` comes
            // later, from `noswap`, and only for an offering the druids took.
            "stones" => "henge.instruct",
            _ => return None,
        };
        self.named(key)
    }

    /// `MOON:SCR_PRO`, image 0x12965: what `KnightProtection+48` (0x518)
    /// hands to `INSTRUCTMESSAGE` when a challenged knight has a scroll of
    /// protection, and `+51` is `WaitFIRE`.
    ///
    /// ```text
    /// 00502  mov si, 0x5e7            ; promes0, an eighteen-space buffer
    /// 00505  mov bx, [di+0x4c]        ; the knight's name
    /// 00508  mov al, [bx]; mov [si], al; inc si; inc bx; or al, al; jne 00508
    /// 00515  mov si, 0x5b5            ; SCR_PRO
    /// 00518  call INSTRUCTMESSAGE
    /// 0051b  call WaitFIRE
    ///
    /// 12965  text 05e7 x 0 y  55 flags 1 next 05bf   promes0, the name copied in
    /// 1296f  text 05fa x 0 y  75 flags 1 next 05c9   may use their
    /// 12979  text 0608 x 0 y  95 flags 1 next 05d3   Scroll of protection
    /// 12983  text 061d x 0 y 115 flags 1 next 05dd   to avoid this battle
    /// 1298d  text 0632 x 0 y 182 flags 1 next 0000   Press fire to continue
    /// ```
    ///
    /// So the first line is the knight's own name, copied NUL and all into
    /// `promes0`, which is why it takes the name here.
    pub fn scroll_of_protection(name: &str) -> Message {
        Message {
            kind: Kind::Instruction,
            until: Until::Fire,
            lines: vec![
                Line::new(name, 0, 55, FLAG_CENTRE),
                Line::new("may use their", 0, 75, FLAG_CENTRE),
                Line::new("Scroll of protection", 0, 95, FLAG_CENTRE),
                Line::new("to avoid this battle", 0, 115, FLAG_CENTRE),
                press_fire(182),
            ],
        }
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
// Hand-aligned: one of the fourteen messages per line, in pointer order.
#[rustfmt::skip]
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
        assert_eq!(
            m.wait(1).lines[1].text,
            "to contemplate your fate...",
            "WaitM2B"
        );
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
        assert!(
            w.shown().all(|l| l.align == Align::Centre),
            "flag 1 is centre"
        );
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
        assert_eq!(
            henge.kind,
            Kind::Instruction,
            "INSTRUCTMESSAGE, in its own colour"
        );
        assert_eq!(
            henge.lines[0].text, "To be granted a",
            "HengeInstruct, SHMES1"
        );
        assert_eq!(henge.until, Until::Fire, "Henge+59 is WaitFIRE");

        assert!(
            m.on_entering("highwood.tavern").is_none(),
            "nothing greets a tavern"
        );
        assert!(m.on_entering("nowhere").is_none());

        // The wizard is the odd one: `LoadWizard` calls the argumentless
        // routine, so his tower shows whichever of the fourteen is next.
        assert!(m.waits_on_entering("wizard"));
        assert!(!m.waits_on_entering("highwood"));
        assert!(
            m.on_entering("wizard").is_none(),
            "it has no chain of its own"
        );
    }

    #[test]
    fn the_flag_word_decodes_the_way_textptop_reads_it() {
        assert_eq!(Line::new("x", 0, 0, FLAG_CENTRE).align, Align::Centre);
        assert_eq!(Line::new("x", 0, 0, FLAG_RIGHT).align, Align::Right);
        assert_eq!(Line::new("x", 0, 0, 0).align, Align::Left);
        assert!(Line::new("x", 0, 0, FLAG_CENTRE | FLAG_BOLD).bold);
        assert!(!Line::new("x", 0, 0, FLAG_CENTRE).bold);
    }

    /// `HengeInstruct` as the walker at 0x7a86 reads it out of image 0x129f9:
    /// four records twenty apart from y 75, then the shared record at
    /// DS:0x5dd, every one of them with flags 1.
    #[test]
    fn henge_instruct_is_the_five_records_at_0x129f9() {
        let m = Messages::recovered();
        let h = m.named("henge.instruct").expect("HengeInstruct");
        let texts: Vec<&str> = h.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "To be granted a",
                "longer life you must",
                "offer an item of ",
                "magical nature to Danu",
                "Press fire to continue",
            ]
        );
        let ys: Vec<i32> = h.lines.iter().map(|l| l.y).collect();
        assert_eq!(ys, [75, 95, 115, 135, 182]);
        assert!(h.lines.iter().all(|l| l.align == Align::Centre && !l.bold));
        assert_eq!(h.kind, Kind::Instruction, "0x108b calls 0x8f17");
        assert_eq!(h.until, Until::Fire, "0x108e calls 0x8251");
        assert!(h.shown().count() == 5, "no Loading... line to drop");
    }

    /// `HengeWait` is the only chain the circle shows twice over: it belongs
    /// to `noswap`, goes up when something has been offered, and is covered
    /// by the load of `Hen1.p` rather than waited on.
    #[test]
    fn henge_wait_covers_a_load_and_is_not_the_arrival_message() {
        let m = Messages::recovered();
        let w = m.named("henge.ritual").expect("HengeWait");
        assert_eq!(w.lines[0].text, "The druids prepare", "SHMES5");
        assert_eq!(w.lines[1].text, "for the ritual", "SHMES6");
        assert_eq!((w.lines[0].y, w.lines[1].y), (75, 95));
        assert_eq!(w.kind, Kind::Instruction, "0xb375 calls 0x8f17");
        assert_eq!(
            w.until,
            Until::Loaded,
            "0xb37d loads HengeFILE1 straight after"
        );
        assert_ne!(m.on_entering("stones"), Some(w));
    }

    /// `SCR_PRO` with the name `KnightProtection` copies into `promes0`.
    #[test]
    fn the_scroll_of_protection_chain_starts_with_the_knights_name() {
        let s = Messages::scroll_of_protection("SIR_GODBER");
        let texts: Vec<&str> = s.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "SIR_GODBER",
                "may use their",
                "Scroll of protection",
                "to avoid this battle",
                "Press fire to continue",
            ]
        );
        let ys: Vec<i32> = s.lines.iter().map(|l| l.y).collect();
        assert_eq!(ys, [55, 75, 95, 115, 182]);
        assert_eq!((s.kind, s.until), (Kind::Instruction, Until::Fire));
    }

    /// `NextDayMes` is two records, not one: the heading and the same `Press
    /// fire to continue` record `HengeInstruct` ends on.
    #[test]
    fn next_day_is_two_records_and_waits_for_fire() {
        let m = Messages::recovered();
        let n = m.named("next.day").expect("NextDayMes");
        assert_eq!(n.lines.len(), 2);
        assert_eq!((n.lines[0].text.as_str(), n.lines[0].y), ("Next Day", 95));
        assert_eq!(
            (n.lines[1].text.as_str(), n.lines[1].y),
            ("Press fire to continue", 182)
        );
        assert_eq!(n.until, Until::Fire, "NextWHICH+35 is WaitFIRE");
    }

    /// The fourteen and the welcomes cover a load; nothing waits on fire
    /// there, because nothing in the original does.
    #[test]
    fn load_messages_are_not_waited_on() {
        let m = Messages::recovered();
        for n in 0..m.wait_len() {
            assert_eq!(m.wait(n).until, Until::Loaded);
        }
        assert_eq!(m.named("welcome.highwood").unwrap().until, Until::Loaded);
        assert_eq!(m.named("welcome.waterdeep").unwrap().until, Until::Loaded);
        assert_eq!(m.named("title.credit").unwrap().until, Until::Loaded);
    }

    #[test]
    fn messages_survive_serialization() {
        let m = Messages::recovered();
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<Messages>(&json).unwrap(), m);
    }
}
