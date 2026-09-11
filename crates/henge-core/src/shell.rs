//! The two screens in front of the game: the title's option list, and choosing
//! a knight.
//!
//! Both are state machines with no pixels in them, which is why they are here
//! rather than in the renderer. A test drives them exactly as a keyboard does,
//! and so, later, will a network peer.
//!
//! **Recovered.** `DoOptions`, `Selection` and `Adjplayers` in `MOON` give the
//! title menu: four rows, the first of which is a number of players between one
//! and four that left and right adjust, the second a gore switch, and the last
//! two the two ways to start. Up and down clamp rather than wrap.
//!
//! **One row of the title is ours**, appended after the recovered four:
//! [`Row::Online`], which opens the lobby. The original's list is four records
//! long and `OptionKeys` clamps `optmode` to three, so a fifth row is an
//! addition and is marked as one; nothing about the four above it changed.
//!
//! `ChooseKnight`, `ChooseRefresh`, `FindChosen` and `ChooseFIRE` give the
//! select: a bitmask of the knights still free (`choose_knight`), a highlight
//! (`Chosen`), and one pass per player (`choose_loop`). Left and right step over
//! knights already taken and stop at the ends rather than wrapping, which is the
//! original's own behaviour and not a simplification of it.
//!
//! **And fire does not finish a seat's turn; it starts the typing.**
//! `ChooseFIRE` at image 0x16be writes the chosen knight's name buffer into
//! `NAMEy` and calls `TypeName` at 0x13f0, and only when that returns does it
//! clear the knight's bit in `choose_knight`. `ChooseKnight`'s loop then
//! decrements `choose_loop`, steps `choose_player` on by 0x62 and calls
//! `FindChosen`. So the knight being named is still drawn, still free and still
//! framed while the name is being typed, and [`Select::take`] and
//! [`Select::name_done`] are those two halves.

use serde::{Deserialize, Serialize};

/// How many knights there are to choose between, and therefore how many can
/// play. `PLAYER1`..`PLAYER4` in the original.
pub const SEATS: usize = 4;

/// A row of the title's option list, in the original's own order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// How many people are playing.
    Players,
    /// Whether blood is drawn. `GOREOPT`.
    Gore,
    /// `StartPractice`: one bout, no map.
    Practice,
    /// `StartMoonQuest`: the game.
    Quest,
    /// **Ours, and the only row here that is.** The original's list is four
    /// records long: `DoOptions` walks `OPT1a`'s chain, `ARR` at `DS:0x706` is
    /// four words, and `OptionKeys` clamps `optmode` to three. There was nothing
    /// to put on a fifth row in 1991, because the four joysticks were all in the
    /// same room.
    ///
    /// Nothing about the recovered four changes: this is appended after them, it
    /// starts nothing they start, and the player count above it still means
    /// people at *this* keyboard. See `henge_net`.
    Online,
}

impl Row {
    pub const ALL: [Row; 5] = [
        Row::Players,
        Row::Gore,
        Row::Practice,
        Row::Quest,
        Row::Online,
    ];

    /// The four the original has, for anything that wants to say which rows are
    /// recovered and which is not.
    pub const RECOVERED: usize = 4;
}

/// What taking an option asked for. Adjusting a setting asks for nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Start {
    Practice,
    Quest,
    /// Ours: the lobby, and then a quest driven from more than one machine.
    Online,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Title {
    /// Which row is highlighted. `optmode`.
    pub row: usize,
    /// One to four. `NUM_PLAYERS`.
    pub players: usize,
    pub gore: bool,
}

impl Default for Title {
    fn default() -> Title {
        Title {
            row: 0,
            players: 1,
            gore: true,
        }
    }
}

impl Title {
    pub fn selected(&self) -> Row {
        Row::ALL[self.row.min(Row::ALL.len() - 1)]
    }

    /// Move the highlight. Clamps at both ends: the original sets a flag and
    /// stops rather than wrapping round.
    pub fn move_by(&mut self, delta: i32) {
        let last = Row::ALL.len() as i32 - 1;
        self.row = (self.row as i32 + delta).clamp(0, last) as usize;
    }

    /// Left and right. On the player count they add and subtract; anywhere else
    /// they are `OSWITCHES`, which only ever touches the gore row.
    pub fn adjust(&mut self, delta: i32) {
        match self.selected() {
            Row::Players => {
                self.players = (self.players as i32 + delta).clamp(1, SEATS as i32) as usize;
            }
            Row::Gore => self.gore = !self.gore,
            _ => {}
        }
    }

    /// Fire. Starts the game on the last two rows; on the gore row it is the
    /// same switch left and right throw, and on the player count it does
    /// nothing, exactly as `OptionKeys` does.
    pub fn choose(&mut self) -> Option<Start> {
        match self.selected() {
            Row::Practice => Some(Start::Practice),
            Row::Quest => Some(Start::Quest),
            Row::Online => Some(Start::Online),
            Row::Gore => {
                self.gore = !self.gore;
                None
            }
            Row::Players => None,
        }
    }
}

/// How long a name may get. `TypeName`'s own limit:
/// `cmp word ptr [SPACE], 0xd; jl` accepts a character only while the caret is
/// below thirteen, and above it the routine beeps and drops the key.
pub const NAME_MAX: usize = 13;

/// `CURSOR`, the caret. `TypeName` writes 0x5c into `CURSOR` at 0x1405 and
/// `Cursor` at 0x14ac stamps it into the buffer at the caret before every
/// redraw. `GFX:TextASCII` maps 0x5c and 0x2f both to glyph 71, so it draws as
/// the stroke.
pub const CARET: char = '\\';

/// Typing a name over the knight's own. `MOON:TypeName` at image 0x13f0.
///
/// The original keeps a 21-byte buffer per knight (`BNAME`, `GNAME`, `ENAME`,
/// `RNAME`), a caret index in `SPACE`, and nothing else. On entry it finds the
/// caret by scanning the buffer for the first space; the default names are
/// stored with an underscore where their space goes, because `TextASCII` draws
/// 0x5f as the blank and the scan would otherwise stop in the middle of
/// `SIR GODBER`. The space bar types one of those underscores too:
/// `ASCIIT[0x39]` is 0x5f.
///
/// So the caret is always at the end of the live text and everything from it on
/// is blank. That is why this holds the live text alone: a caret kept as state
/// needs no scan, and `NameDone` writes a NUL at the caret, which is the same
/// truncation.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Typing {
    /// Whose name. `ChooseFIRE` picks the buffer `NAMEy` points at from the
    /// knight index, one branch each.
    pub knight: usize,
    /// Everything before the caret, which is the whole of the name.
    pub text: String,
}

impl Typing {
    /// `SPACE`: where the next character lands.
    pub fn caret(&self) -> usize {
        self.text.chars().count()
    }

    /// One character off `ASCIIKEY`. Dropped when the caret has reached
    /// thirteen, which is `ScanKEYS` taking the beep branch.
    pub fn type_char(&mut self, c: char) -> bool {
        if self.caret() >= NAME_MAX {
            return false;
        }
        self.text.push(c);
        true
    }

    /// `BACKSPACE` at image 0x1485: blank the cell, step the caret back, blank
    /// the cell it lands on. It stops at zero, so a name can be emptied.
    pub fn backspace(&mut self) {
        self.text.pop();
    }

    /// What `ChooseRefresh` draws: the buffer with `CURSOR` written in at the
    /// caret. Everything past the caret is blank, so nothing follows it.
    pub fn shown(&self) -> String {
        let mut s = self.text.clone();
        s.push(CARET);
        s
    }
}

/// Choosing knights, one player at a time.
///
/// **`ChooseKnight` is a hot seat**: `choose_loop` counts the players down,
/// `choose_player` says whose turn it is, and `ChooseLoop` reads one input
/// device, so exactly one seat can move the frame or press fire and the others
/// wait. That is what a game at one keyboard needs and it is what a game across
/// machines gets too: the only difference is where that seat's keys come from.
///
/// Every method that acts takes the seat acting, and does nothing unless it is
/// that seat's turn. At one keyboard the caller always passes
/// [`Select::seat`]; across machines it passes the same thing and reads that
/// seat's word off the wire, so a person pressing keys out of turn is ignored on
/// every machine alike.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Select {
    /// How many will choose. `choose_loop`.
    pub players: usize,
    /// Which player is choosing now, counting from zero. `choose_player`.
    pub seat: usize,
    /// The highlighted knight. `Chosen`.
    pub cursor: usize,
    /// Still unclaimed. `choose_knight`, which the original keeps as a bitmask.
    free: [bool; SEATS],
    /// Which knight each player took, in seat order.
    taken: [Option<usize>; SEATS],
    /// `TypeFLAG` and `NAMEy` together: the name being typed, while one is.
    pub typing: Option<Typing>,
}

impl Select {
    pub fn new(players: usize) -> Select {
        Select {
            players: players.clamp(1, SEATS),
            seat: 0,
            cursor: 0,
            free: [true; SEATS],
            taken: [None; SEATS],
            typing: None,
        }
    }

    pub fn free(&self, knight: usize) -> bool {
        self.free.get(knight).copied().unwrap_or(false)
    }

    /// Whether this seat's keys do anything right now: it is a seat somebody is
    /// in, and it is its turn.
    pub fn acts(&self, seat: usize) -> bool {
        seat < self.players && seat == self.seat && !self.done()
    }

    /// Which knight a seat took, if it has chosen.
    pub fn taken_by(&self, seat: usize) -> Option<usize> {
        self.taken.get(seat).copied().flatten()
    }

    /// Every choice made so far, in seat order.
    pub fn chosen(&self) -> Vec<usize> {
        self.taken
            .iter()
            .take(self.players)
            .filter_map(|t| *t)
            .collect()
    }

    pub fn done(&self) -> bool {
        self.seat >= self.players
    }

    /// Step the highlight, skipping knights already taken.
    ///
    /// Stops at the ends rather than wrapping, and leaves the highlight where it
    /// was when there is nothing further to land on. That is what `ChooseLoop`
    /// does, and wrapping would put a highlight on a knight nobody can have.
    pub fn move_by(&mut self, seat: usize, delta: i32) {
        for _ in 0..delta.unsigned_abs() {
            self.step(seat, delta.signum());
        }
    }

    /// One press of left or right.
    fn step(&mut self, seat: usize, dir: i32) {
        if dir == 0 || !self.acts(seat) || self.typing.is_some() {
            return;
        }
        let mut at = self.cursor as i32;
        loop {
            at += dir;
            if at < 0 || at >= SEATS as i32 {
                return; // nothing further this way, so nothing moves
            }
            if self.free[at as usize] {
                self.cursor = at as usize;
                return;
            }
        }
    }

    /// Fire on the highlighted knight: `ChooseFIRE`.
    ///
    /// It does not finish the seat's turn. It writes the knight's name buffer
    /// into `NAMEy` and calls `TypeName`, so the knight is still free and still
    /// framed and the name is now being typed over. `default` is what the buffer
    /// holds, which is that knight's own name.
    ///
    /// Returns the knight whose name is being typed.
    pub fn take(&mut self, seat: usize, default: &str) -> Option<usize> {
        if !self.acts(seat) || self.typing.is_some() || !self.free[self.cursor] {
            return None;
        }
        let knight = self.cursor;
        // `TypeName` scans for the first space and the default names carry an
        // underscore where theirs goes, so the caret lands past the whole name
        // however long it is. Thirteen is as much as the routine will ever hold.
        let text: String = default.chars().take(NAME_MAX).collect();
        self.typing = Some(Typing { knight, text });
        Some(knight)
    }

    /// `NameDone` at 0x14bb and what `ChooseFIRE` does after it: the NUL at the
    /// caret, `and word ptr [choose_knight], ...`, and then `ChooseKnight`'s
    /// own `sub word ptr [choose_loop], 1` and `FindChosen`.
    ///
    /// Returns the knight and the name that was typed.
    pub fn name_done(&mut self, seat: usize) -> Option<(usize, String)> {
        if seat != self.seat {
            return None;
        }
        let typing = self.typing.take()?;
        let knight = typing.knight;
        self.free[knight] = false;
        self.taken[self.seat] = Some(knight);
        self.seat += 1;
        if !self.done() {
            if let Some(next) = (0..SEATS).find(|i| self.free[*i]) {
                self.cursor = next;
            }
        }
        Some((knight, typing.text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_title_list_clamps_rather_than_wrapping() {
        let mut t = Title::default();
        t.move_by(-1);
        assert_eq!(t.selected(), Row::Players, "already at the top");
        t.move_by(9);
        assert_eq!(t.selected(), Row::Online, "and stops at the bottom");
    }

    #[test]
    fn the_player_count_runs_one_to_four() {
        let mut t = Title::default();
        assert_eq!(t.players, 1);
        t.adjust(-1);
        assert_eq!(t.players, 1, "nobody plays with none");
        for _ in 0..9 {
            t.adjust(1);
        }
        assert_eq!(t.players, SEATS, "and there are only four knights");
    }

    #[test]
    fn left_and_right_only_touch_the_row_they_are_on() {
        let mut t = Title::default();
        t.move_by(1);
        let players = t.players;
        t.adjust(1);
        assert!(!t.gore, "the gore row is a switch");
        assert_eq!(t.players, players, "and the count is on another row");
        t.move_by(1);
        t.adjust(1);
        assert!(!t.gore, "the practice row adjusts nothing");
    }

    #[test]
    fn only_the_last_three_rows_start_anything() {
        let mut t = Title::default();
        assert_eq!(t.choose(), None);
        t.move_by(2);
        assert_eq!(t.choose(), Some(Start::Practice));
        t.move_by(1);
        assert_eq!(t.choose(), Some(Start::Quest));
        t.move_by(1);
        assert_eq!(t.choose(), Some(Start::Online));
    }

    /// The four recovered rows are the first four and behave as they did: the
    /// fifth is appended, and adding it moved nothing.
    #[test]
    fn the_recovered_rows_are_the_first_four_and_are_unchanged() {
        assert_eq!(
            &Row::ALL[..Row::RECOVERED],
            &[Row::Players, Row::Gore, Row::Practice, Row::Quest]
        );
        assert_eq!(Row::ALL[Row::RECOVERED], Row::Online);
        let mut t = Title::default();
        // The row the original's `OptionKeys` clamps to is still reachable, and
        // the one past it is ours.
        t.move_by(Row::RECOVERED as i32 - 1);
        assert_eq!(t.selected(), Row::Quest);
        // Left and right do nothing on it, like the two rows above it.
        t.move_by(1);
        let (players, gore) = (t.players, t.gore);
        t.adjust(1);
        t.adjust(-1);
        assert_eq!((t.players, t.gore), (players, gore));
    }

    #[test]
    fn one_player_takes_one_knight_and_the_screen_is_done() {
        let mut s = Select::new(1);
        assert!(!s.done());
        s.move_by(0, 2);
        assert_eq!(s.cursor, 2);
        // Fire starts the typing and leaves the seat where it is, which is
        // `ChooseFIRE` calling `TypeName` before it clears the knight's bit.
        assert_eq!(s.take(0, "SIR JEFFREY"), Some(2));
        assert!(
            !s.done(),
            "the seat is still choosing while the name is typed"
        );
        assert_eq!(
            s.take(0, "SIR JEFFREY"),
            None,
            "and fire again does nothing"
        );
        assert_eq!(s.name_done(0), Some((2, "SIR JEFFREY".into())));
        assert!(s.done());
        assert_eq!(s.chosen(), vec![2]);
        assert_eq!(
            s.take(0, "SIR JEFFREY"),
            None,
            "and nothing more can be taken"
        );
    }

    /// `ChooseLoop`: the highlight steps over knights already spoken for.
    #[test]
    fn the_highlight_steps_over_a_knight_already_taken() {
        let mut s = Select::new(3);
        s.move_by(0, 1);
        s.take(0, "SIR RICHARD"); // knight 1 goes
        s.name_done(0);
        // `FindChosen`: the next seat opens on the lowest knight still free.
        assert_eq!(s.seat, 1);
        assert_eq!(s.cursor, 0, "the lowest still free");
        s.move_by(1, 1);
        assert_eq!(s.cursor, 2, "one is gone, so right lands on two");
    }

    /// `ChooseFIRE` clears `choose_knight` only after `TypeName` returns, so
    /// the knight being named is still drawn and still framed.
    #[test]
    fn the_knight_being_named_is_still_free_until_the_name_is_done() {
        let mut s = Select::new(2);
        s.take(0, "SIR GODBER");
        assert!(s.free(0), "still a bit set in choose_knight");
        assert_eq!(s.cursor, 0, "and still the one the frame is on");
        // And nothing moves: `TypeName` owns the input until Enter or fire.
        s.move_by(0, 2);
        assert_eq!(s.cursor, 0);
        s.name_done(0);
        assert!(!s.free(0));
    }

    /// `TypeName`, `ScanKEYS`, `BACKSPACE` and `NameDone`, end to end.
    #[test]
    fn a_name_is_typed_over_the_knights_own() {
        let mut s = Select::new(1);
        s.take(0, "SIR GODBER");
        let t = s.typing.as_mut().expect("TypeFLAG is set");
        // The caret sits past the whole default name, because the original's
        // buffer holds an underscore where its space goes and the scan for the
        // first real space runs past it.
        assert_eq!(t.caret(), 10);
        assert_eq!(t.shown(), "SIR GODBER\\");
        // Ten backspaces empty it, and an eleventh does nothing: the original
        // clamps `SPACE` at zero.
        for _ in 0..11 {
            t.backspace();
        }
        assert_eq!(t.caret(), 0);
        assert_eq!(t.shown(), "\\");
        for c in "SIR ALAN".chars() {
            assert!(t.type_char(c));
        }
        assert_eq!(t.shown(), "SIR ALAN\\");
        assert_eq!(s.name_done(0), Some((0, "SIR ALAN".into())));
        assert!(s.typing.is_none(), "TypeFLAG is clear again");
    }

    /// `cmp word ptr [SPACE], 0xd; jl`: thirteen characters and no more.
    #[test]
    fn a_name_stops_at_thirteen_characters() {
        let mut s = Select::new(1);
        s.take(0, "SIR GODBER");
        let t = s.typing.as_mut().unwrap();
        for c in "XYZ".chars() {
            assert!(t.type_char(c));
        }
        assert_eq!(t.caret(), NAME_MAX);
        assert!(!t.type_char('!'), "the routine beeps and drops the key");
        assert_eq!(t.text, "SIR GODBERXYZ");
        assert_eq!(t.caret(), NAME_MAX);
        // A default longer than the field is cut to it rather than overflowing.
        let mut s = Select::new(1);
        s.take(0, "SIR CHRISTOPHER");
        assert_eq!(s.typing.as_ref().unwrap().text, "SIR CHRISTOPH");
    }

    #[test]
    fn the_highlight_stops_at_the_ends_rather_than_wrapping() {
        let mut s = Select::new(2);
        s.move_by(0, -1);
        assert_eq!(s.cursor, 0);
        s.move_by(0, 3);
        assert_eq!(s.cursor, 3);
        s.move_by(0, 1);
        assert_eq!(s.cursor, 3, "nothing beyond the last knight");
    }

    /// With three of the four taken, moving cannot land on any of them, and the
    /// highlight must stay where it legally is rather than sliding onto one.
    #[test]
    fn a_move_with_nowhere_to_go_changes_nothing() {
        let mut s = Select::new(4);
        for _ in 0..3 {
            // Each of the three in turn, because the screen is a hot seat: only
            // the seat `choose_player` names can act.
            let seat = s.seat;
            s.take(seat, "SIR GODBER");
            s.name_done(seat);
        }
        assert_eq!(s.seat, 3, "the last one is up");
        assert_eq!(s.cursor, 3);
        s.move_by(3, -1);
        assert_eq!(s.cursor, 3, "the three below are taken");
        s.move_by(3, 1);
        assert_eq!(s.cursor, 3);
    }

    #[test]
    fn four_players_take_one_each_and_none_twice() {
        let mut s = Select::new(4);
        let mut picked = Vec::new();
        while !s.done() {
            let seat = s.seat;
            s.move_by(seat, 1);
            picked.push(s.take(seat, "SIR GODBER").expect("a free knight to take"));
            s.name_done(seat);
        }
        picked.sort();
        assert_eq!(picked, vec![0, 1, 2, 3]);
        assert_eq!(s.chosen().len(), 4);
    }

    /// A seat pressing keys when it is not its turn does nothing at all.
    ///
    /// `ChooseLoop` reads one input device, so at one keyboard this cannot even
    /// arise. Across machines all four seats' keys are on the wire every tick,
    /// and the three that are waiting must be ignored on every machine alike or
    /// the screens come apart.
    #[test]
    fn a_seat_that_is_not_up_does_nothing() {
        let mut s = Select::new(2);
        assert!(s.acts(0));
        assert!(!s.acts(1), "seat one waits its turn");
        s.move_by(1, 3);
        assert_eq!(s.cursor, 0, "its keys do not move the frame");
        assert_eq!(s.take(1, "SIR RICHARD"), None, "nor take a knight");
        assert_eq!(s.name_done(1), None, "nor finish one");
        // Seat zero takes its turn, and then it is the other's.
        s.take(0, "SIR GODBER");
        s.name_done(0);
        assert!(!s.acts(0), "and now seat zero is the one that waits");
        assert!(s.acts(1));
        s.move_by(1, 1);
        assert_eq!(s.cursor, 2, "one is taken, so right lands on two");
    }

    #[test]
    fn a_select_survives_serialization() {
        let mut s = Select::new(2);
        s.move_by(0, 1);
        s.take(0, "SIR RICHARD");
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Select>(&json).unwrap(), s);
    }
}
