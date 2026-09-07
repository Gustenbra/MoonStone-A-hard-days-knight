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
//! `ChooseKnight`, `ChooseRefresh`, `FindChosen` and `ChooseFIRE` give the
//! select: a bitmask of the knights still free (`choose_knight`), a highlight
//! (`Chosen`), and one pass per player (`choose_loop`). Left and right step over
//! knights already taken and stop at the ends rather than wrapping, which is the
//! original's own behaviour and not a simplification of it.

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
}

impl Row {
    pub const ALL: [Row; 4] = [Row::Players, Row::Gore, Row::Practice, Row::Quest];
}

/// What taking an option asked for. Adjusting a setting asks for nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Start {
    Practice,
    Quest,
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
        Title { row: 0, players: 1, gore: true }
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
            Row::Gore => {
                self.gore = !self.gore;
                None
            }
            Row::Players => None,
        }
    }
}

/// Choosing knights, one player at a time.
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
}

impl Select {
    pub fn new(players: usize) -> Select {
        Select {
            players: players.clamp(1, SEATS),
            seat: 0,
            cursor: 0,
            free: [true; SEATS],
            taken: [None; SEATS],
        }
    }

    pub fn free(&self, knight: usize) -> bool {
        self.free.get(knight).copied().unwrap_or(false)
    }

    /// Which knight a seat took, if it has chosen.
    pub fn taken_by(&self, seat: usize) -> Option<usize> {
        self.taken.get(seat).copied().flatten()
    }

    /// Every choice made so far, in seat order.
    pub fn chosen(&self) -> Vec<usize> {
        self.taken.iter().take(self.players).filter_map(|t| *t).collect()
    }

    pub fn done(&self) -> bool {
        self.seat >= self.players
    }

    /// Step the highlight, skipping knights already taken.
    ///
    /// Stops at the ends rather than wrapping, and leaves the highlight where it
    /// was when there is nothing further to land on. That is what `ChooseLoop`
    /// does, and wrapping would put a highlight on a knight nobody can have.
    pub fn move_by(&mut self, delta: i32) {
        for _ in 0..delta.unsigned_abs() {
            self.step(delta.signum());
        }
    }

    /// One press of left or right.
    fn step(&mut self, dir: i32) {
        if dir == 0 || self.done() {
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

    /// Take the highlighted knight for the current player.
    ///
    /// Returns the knight taken. Afterwards the highlight sits on the lowest
    /// still-free knight, which is `FindChosen`.
    pub fn take(&mut self) -> Option<usize> {
        if self.done() || !self.free[self.cursor] {
            return None;
        }
        let knight = self.cursor;
        self.free[knight] = false;
        self.taken[self.seat] = Some(knight);
        self.seat += 1;
        if !self.done() {
            if let Some(next) = (0..SEATS).find(|i| self.free[*i]) {
                self.cursor = next;
            }
        }
        Some(knight)
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
        assert_eq!(t.selected(), Row::Quest, "and stops at the bottom");
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
    fn only_the_last_two_rows_start_anything() {
        let mut t = Title::default();
        assert_eq!(t.choose(), None);
        t.move_by(2);
        assert_eq!(t.choose(), Some(Start::Practice));
        t.move_by(1);
        assert_eq!(t.choose(), Some(Start::Quest));
    }

    #[test]
    fn one_player_takes_one_knight_and_the_screen_is_done() {
        let mut s = Select::new(1);
        assert!(!s.done());
        s.move_by(2);
        assert_eq!(s.cursor, 2);
        assert_eq!(s.take(), Some(2));
        assert!(s.done());
        assert_eq!(s.chosen(), vec![2]);
        assert_eq!(s.take(), None, "and nothing more can be taken");
    }

    /// `ChooseLoop`: the highlight steps over knights already spoken for.
    #[test]
    fn the_highlight_steps_over_a_knight_already_taken() {
        let mut s = Select::new(3);
        s.move_by(1);
        s.take(); // knight 1 goes
        assert_eq!(s.cursor, 0, "the lowest still free");
        s.move_by(1);
        assert_eq!(s.cursor, 2, "one is gone, so right lands on two");
    }

    #[test]
    fn the_highlight_stops_at_the_ends_rather_than_wrapping() {
        let mut s = Select::new(2);
        s.move_by(-1);
        assert_eq!(s.cursor, 0);
        s.move_by(3);
        assert_eq!(s.cursor, 3);
        s.move_by(1);
        assert_eq!(s.cursor, 3, "nothing beyond the last knight");
    }

    /// With three of the four taken, moving cannot land on any of them, and the
    /// highlight must stay where it legally is rather than sliding onto one.
    #[test]
    fn a_move_with_nowhere_to_go_changes_nothing() {
        let mut s = Select::new(4);
        s.take(); // 0
        s.take(); // 1
        s.take(); // 2
        assert_eq!(s.cursor, 3);
        s.move_by(-1);
        assert_eq!(s.cursor, 3, "the three below are taken");
        s.move_by(1);
        assert_eq!(s.cursor, 3);
    }

    #[test]
    fn four_players_take_one_each_and_none_twice() {
        let mut s = Select::new(4);
        let mut picked = Vec::new();
        while !s.done() {
            s.move_by(1);
            picked.push(s.take().expect("a free knight to take"));
        }
        picked.sort();
        assert_eq!(picked, vec![0, 1, 2, 3]);
        assert_eq!(s.chosen().len(), 4);
    }

    #[test]
    fn a_select_survives_serialization() {
        let mut s = Select::new(2);
        s.move_by(1);
        s.take();
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Select>(&json).unwrap(), s);
    }
}
