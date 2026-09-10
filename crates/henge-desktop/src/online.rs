//! The lobby screen: a state machine with no pixels in it.
//!
//! **Ours, all of it**, like everything in `henge_net`: the original has no
//! screen between the title and the select, because its four players were
//! already in the room. What is *not* ours is the way it is driven. It reads the
//! same five bits every other screen reads, it takes fire as "do this row", and
//! a text field on it behaves exactly like `TypeName`: fire starts the typing,
//! fire or Enter ends it, backspace walks back and nothing past thirteen
//! characters is accepted. So a person who can work the knight select can work
//! this without being told anything.
//!
//! It sits here rather than in `henge_core` because it is shell, and beside
//! `shell.rs` rather than inside it because the recovered screens in there
//! should stay recovered.

use henge_core::shell::NAME_MAX;
use henge_net::proto::{Lobby, LOBBY_NAME_MAX, SEATS};

/// Which page of the lobby is up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    /// Host a game, or join one.
    Menu,
    /// Naming a game before opening it.
    Create,
    /// The address of a game to join.
    Join,
    /// In a lobby, waiting for the rest.
    Waiting,
}

/// A row of whichever page is up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// The name of the game being hosted.
    LobbyName,
    /// What to call yourself.
    PlayerName,
    /// Where the game is: an address and a port.
    Address,
    /// Open the lobby, which is where the port is asked for.
    Host,
    /// Go to the joining page.
    GoJoin,
    /// Dial the address.
    Dial,
    /// Which knight this seat wants.
    Knight,
    /// Sitting down.
    Ready,
    /// Begin. The host's row and nobody else's.
    Begin,
    /// Out: back to the title, or out of the lobby.
    Back,
}

impl Row {
    /// Whether fire on this row starts typing rather than doing something.
    pub fn is_text(self) -> bool {
        matches!(self, Row::LobbyName | Row::PlayerName | Row::Address)
    }

    /// The label, which is also what the tests read so the two cannot drift.
    pub fn label(self) -> &'static str {
        match self {
            Row::LobbyName => "Game",
            Row::PlayerName => "You",
            Row::Address => "Address",
            Row::Host => "Open a game",
            Row::GoJoin => "Join a game",
            Row::Dial => "Join",
            Row::Knight => "Knight",
            Row::Ready => "Ready",
            Row::Begin => "Begin",
            Row::Back => "Back",
        }
    }
}

/// What the screen is asking the outside world to do. Everything that touches a
/// socket is the caller's, so this stays testable without one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ask {
    /// Open a lobby under this name, on this port.
    Open { game: String, you: String },
    /// Dial this address as this person.
    Dial { address: String, you: String },
    /// Tell the host what this seat wants.
    Seat { knight: Option<u8>, ready: bool },
    /// The host is starting.
    Begin,
    /// Out of the lobby, or off the screen entirely.
    Leave,
}

/// The lobby screen.
#[derive(Clone, Debug)]
pub struct Online {
    pub page: Page,
    /// Which row the arrow is against.
    pub row: usize,
    /// Typing into the highlighted row, which only a text row allows.
    pub typing: bool,
    pub game: String,
    pub you: String,
    pub address: String,
    /// This seat's knight, once chosen. Nothing means the host picks one.
    pub knight: Option<u8>,
    pub ready: bool,
    /// The roster, as the host last said it. Empty until there is one.
    pub roster: Lobby,
    /// The seat the host gave, once it has.
    pub seat: Option<u8>,
    /// Whether we are the host, which decides whether [`Row::Begin`] is there.
    pub hosting: bool,
    /// One line under the roster: what the router said, who just joined, why the
    /// last attempt failed.
    pub note: String,
    /// The address to read out to a friend, once the router has been asked.
    pub reachable: String,
}

impl Default for Online {
    fn default() -> Online {
        Online {
            page: Page::Menu,
            row: 0,
            typing: false,
            // A name nobody has to type before they can play. The knights' own
            // names are the obvious default and the first is as good as any.
            game: String::from("MOONSTONE"),
            you: String::from("KNIGHT"),
            address: String::new(),
            knight: None,
            ready: false,
            roster: Lobby::default(),
            seat: None,
            hosting: false,
            note: String::new(),
            reachable: String::new(),
        }
    }
}

impl Online {
    pub fn new() -> Online {
        Online::default()
    }

    /// The rows of the page that is up.
    pub fn rows(&self) -> Vec<Row> {
        match self.page {
            Page::Menu => vec![Row::Host, Row::GoJoin, Row::Back],
            Page::Create => vec![Row::LobbyName, Row::PlayerName, Row::Host, Row::Back],
            Page::Join => vec![Row::Address, Row::PlayerName, Row::Dial, Row::Back],
            Page::Waiting => {
                let mut rows = vec![Row::Knight, Row::Ready];
                if self.hosting {
                    rows.push(Row::Begin);
                }
                rows.push(Row::Back);
                rows
            }
        }
    }

    pub fn selected(&self) -> Row {
        let rows = self.rows();
        rows[self.row.min(rows.len() - 1)]
    }

    /// What is in the highlighted text row, with the caret on it the way
    /// `ChooseRefresh` draws a name being typed.
    pub fn shown(&self, row: Row) -> String {
        let text = match row {
            Row::LobbyName => &self.game,
            Row::PlayerName => &self.you,
            Row::Address => &self.address,
            _ => return String::new(),
        };
        if self.typing && self.selected() == row {
            format!("{text}{}", henge_core::shell::CARET)
        } else {
            text.clone()
        }
    }

    /// Up and down. Clamps at both ends, like the title's own list.
    pub fn move_by(&mut self, delta: i32) {
        if self.typing {
            return;
        }
        let last = self.rows().len() as i32 - 1;
        self.row = (self.row as i32 + delta).clamp(0, last) as usize;
    }

    /// Left and right. Only the knight row has anything to adjust.
    ///
    /// A knight somebody else has taken is stepped over, the way
    /// `ChooseLoop` steps over one already spoken for, and the far left is
    /// "whichever is free", which is what a player who does not care picks.
    pub fn adjust(&mut self, delta: i32) -> Option<Ask> {
        if self.typing || self.selected() != Row::Knight || delta == 0 {
            return None;
        }
        let dir = delta.signum();
        let mut at = match self.knight {
            None => {
                if dir < 0 {
                    return None;
                }
                -1
            }
            Some(k) => k as i32,
        };
        loop {
            at += dir;
            if at < 0 {
                self.knight = None;
                return Some(self.seat_ask());
            }
            if at >= SEATS as i32 {
                return None;
            }
            if self.free(at as u8) {
                self.knight = Some(at as u8);
                return Some(self.seat_ask());
            }
        }
    }

    /// Whether a knight is ours or unclaimed.
    pub fn free(&self, knight: u8) -> bool {
        !self
            .roster
            .players
            .iter()
            .any(|p| Some(p.seat) != self.seat && p.knight == Some(knight))
    }

    /// Fire, or Enter. The one key that does things.
    pub fn take(&mut self) -> Option<Ask> {
        let row = self.selected();
        // A text row: fire starts the typing and fire ends it, which is
        // `ChooseFIRE` calling `TypeName` and `TypeName` returning.
        if row.is_text() {
            self.typing = !self.typing;
            // A name emptied and left that way is nobody, so the default comes
            // back rather than a blank sitting in the roster.
            if !self.typing {
                if row == Row::PlayerName && self.you.trim().is_empty() {
                    self.you = Online::default().you;
                }
                if row == Row::LobbyName && self.game.trim().is_empty() {
                    self.game = Online::default().game;
                }
            }
            return None;
        }
        match row {
            Row::Host if self.page == Page::Menu => {
                self.page = Page::Create;
                self.row = 0;
                None
            }
            Row::GoJoin => {
                self.page = Page::Join;
                self.row = 0;
                None
            }
            Row::Host => Some(Ask::Open {
                game: self.game.clone(),
                you: self.you.clone(),
            }),
            Row::Dial => {
                if self.address.trim().is_empty() {
                    self.note = "an address to join first".into();
                    return None;
                }
                Some(Ask::Dial {
                    address: self.address.trim().to_string(),
                    you: self.you.clone(),
                })
            }
            Row::Knight => None,
            Row::Ready => {
                self.ready = !self.ready;
                Some(self.seat_ask())
            }
            Row::Begin => Some(Ask::Begin),
            Row::Back => match self.page {
                Page::Menu => Some(Ask::Leave),
                Page::Waiting => Some(Ask::Leave),
                _ => {
                    self.page = Page::Menu;
                    self.row = 0;
                    None
                }
            },
            Row::LobbyName | Row::PlayerName | Row::Address => None,
        }
    }

    /// One character, for the row being typed into.
    pub fn type_char(&mut self, c: char) {
        if !self.typing {
            return;
        }
        let row = self.selected();
        let max = match row {
            Row::LobbyName => LOBBY_NAME_MAX,
            Row::PlayerName => NAME_MAX,
            // Long enough for a dotted quad and a port, and for a name.
            Row::Address => 40,
            _ => return,
        };
        let field = self.field_mut(row);
        if field.chars().count() >= max {
            return;
        }
        field.push(c);
    }

    /// Backspace, which stops at nothing rather than wrapping.
    pub fn backspace(&mut self) {
        if !self.typing {
            return;
        }
        let row = self.selected();
        if !row.is_text() {
            return;
        }
        self.field_mut(row).pop();
    }

    fn field_mut(&mut self, row: Row) -> &mut String {
        match row {
            Row::LobbyName => &mut self.game,
            Row::Address => &mut self.address,
            _ => &mut self.you,
        }
    }

    /// We are in a lobby now: the waiting page, with a seat.
    pub fn sat_down(&mut self, seat: u8, hosting: bool) {
        self.page = Page::Waiting;
        self.row = 0;
        self.typing = false;
        self.seat = Some(seat);
        self.hosting = hosting;
        self.ready = false;
    }

    /// Back to the menu, with something to show for it.
    pub fn back_to_menu(&mut self, note: &str) {
        self.page = Page::Menu;
        self.row = 0;
        self.typing = false;
        self.seat = None;
        self.hosting = false;
        self.ready = false;
        self.roster = Lobby::default();
        self.reachable = String::new();
        self.note = note.to_string();
    }

    fn seat_ask(&self) -> Ask {
        Ask::Seat {
            knight: self.knight,
            ready: self.ready,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use henge_net::proto::Player;

    #[test]
    fn the_menu_goes_two_ways_and_back() {
        let mut o = Online::new();
        assert_eq!(o.selected(), Row::Host);
        assert_eq!(o.take(), None);
        assert_eq!(o.page, Page::Create);
        // Out of the create page, back to the menu, and out of the menu to the
        // title.
        o.move_by(3);
        assert_eq!(o.selected(), Row::Back);
        assert_eq!(o.take(), None);
        assert_eq!(o.page, Page::Menu);
        o.move_by(1);
        assert_eq!(o.selected(), Row::GoJoin);
        o.take();
        assert_eq!(o.page, Page::Join);
        o.move_by(9);
        assert_eq!(o.selected(), Row::Back, "and it clamps at the bottom");
        o.take();
        o.move_by(9);
        assert_eq!(o.take(), Some(Ask::Leave));
    }

    /// A text row behaves like `TypeName`: fire starts it, fire ends it, and
    /// nothing else moves while it is running.
    #[test]
    fn a_name_is_typed_the_way_the_select_screen_types_one() {
        let mut o = Online::new();
        o.take(); // into the create page
        assert_eq!(o.selected(), Row::LobbyName);
        o.take();
        assert!(o.typing);
        for _ in 0..20 {
            o.backspace();
        }
        assert_eq!(o.game, "");
        assert_eq!(o.shown(Row::LobbyName), "\\");
        for c in "CARLS GAME".chars() {
            o.type_char(c);
        }
        assert_eq!(o.shown(Row::LobbyName), "CARLS GAME\\");
        // The arrow cannot move while a name is being typed.
        o.move_by(1);
        assert_eq!(o.selected(), Row::LobbyName);
        o.take();
        assert!(!o.typing);
        assert_eq!(o.shown(Row::LobbyName), "CARLS GAME");
    }

    #[test]
    fn a_name_stops_at_the_length_of_its_field() {
        let mut o = Online::new();
        o.take();
        o.take();
        for _ in 0..40 {
            o.backspace();
        }
        for _ in 0..60 {
            o.type_char('X');
        }
        assert_eq!(o.game.chars().count(), LOBBY_NAME_MAX);
        // And the player's name is the game's own thirteen.
        o.take();
        o.move_by(1);
        o.take();
        for _ in 0..40 {
            o.backspace();
        }
        for _ in 0..60 {
            o.type_char('Y');
        }
        assert_eq!(o.you.chars().count(), NAME_MAX);
    }

    /// A name emptied and left empty comes back rather than going into a roster
    /// as a blank.
    #[test]
    fn an_emptied_name_goes_back_to_its_default() {
        let mut o = Online::new();
        o.take();
        o.move_by(1);
        o.take();
        for _ in 0..20 {
            o.backspace();
        }
        assert_eq!(o.you, "");
        o.take();
        assert_eq!(o.you, Online::default().you);
    }

    #[test]
    fn opening_a_game_asks_for_it_by_name() {
        let mut o = Online::new();
        o.take();
        o.move_by(2);
        assert_eq!(o.selected(), Row::Host);
        assert_eq!(
            o.take(),
            Some(Ask::Open {
                game: "MOONSTONE".into(),
                you: "KNIGHT".into()
            })
        );
    }

    #[test]
    fn joining_needs_an_address_and_says_so() {
        let mut o = Online::new();
        o.move_by(1);
        o.take();
        o.move_by(2);
        assert_eq!(o.selected(), Row::Dial);
        assert_eq!(o.take(), None, "nothing to dial");
        assert!(o.note.contains("address"));
        o.move_by(-2);
        o.take();
        for c in " 10.0.0.4:19910 ".chars() {
            o.type_char(c);
        }
        o.take();
        o.move_by(2);
        assert_eq!(
            o.take(),
            Some(Ask::Dial {
                address: "10.0.0.4:19910".into(),
                you: "KNIGHT".into()
            })
        );
    }

    /// The knight row steps over a knight somebody else has, and the far left is
    /// "whichever is free".
    #[test]
    fn the_knight_row_steps_over_one_already_taken() {
        let mut o = Online::new();
        o.sat_down(1, false);
        o.roster.players = vec![
            Player {
                seat: 0,
                name: "host".into(),
                knight: Some(1),
                ready: true,
            },
            Player {
                seat: 1,
                name: "me".into(),
                knight: None,
                ready: false,
            },
        ];
        assert_eq!(o.selected(), Row::Knight);
        assert_eq!(o.knight, None);
        o.adjust(1);
        assert_eq!(o.knight, Some(0));
        o.adjust(1);
        assert_eq!(o.knight, Some(2), "one is the host's");
        o.adjust(1);
        assert_eq!(o.knight, Some(3));
        assert_eq!(o.adjust(1), None, "and there is no fifth knight");
        assert_eq!(o.knight, Some(3));
        // Back down past the first, and nobody in particular is wanted again.
        o.adjust(-1);
        assert_eq!(o.knight, Some(2));
        o.adjust(-1);
        assert_eq!(o.knight, Some(0));
        assert_eq!(
            o.adjust(-1),
            Some(Ask::Seat {
                knight: None,
                ready: false
            })
        );
        assert_eq!(o.knight, None);
    }

    #[test]
    fn ready_is_a_switch_and_the_host_alone_can_begin() {
        let mut o = Online::new();
        o.sat_down(1, false);
        assert_eq!(o.rows(), vec![Row::Knight, Row::Ready, Row::Back]);
        o.move_by(1);
        assert_eq!(
            o.take(),
            Some(Ask::Seat {
                knight: None,
                ready: true
            })
        );
        assert!(o.ready);
        o.take();
        assert!(!o.ready, "and it switches back");
        // The host has one more row than a guest.
        let mut h = Online::new();
        h.sat_down(0, true);
        assert_eq!(
            h.rows(),
            vec![Row::Knight, Row::Ready, Row::Begin, Row::Back]
        );
        h.move_by(2);
        assert_eq!(h.take(), Some(Ask::Begin));
    }

    #[test]
    fn leaving_a_lobby_goes_back_to_the_menu_with_a_reason() {
        let mut o = Online::new();
        o.sat_down(2, false);
        o.ready = true;
        o.knight = Some(3);
        o.move_by(9);
        assert_eq!(o.selected(), Row::Back);
        assert_eq!(o.take(), Some(Ask::Leave));
        o.back_to_menu("the host closed the game");
        assert_eq!(o.page, Page::Menu);
        assert_eq!(o.seat, None);
        assert!(!o.ready);
        assert!(o.note.contains("closed"));
    }

    #[test]
    fn every_row_has_a_label_and_only_the_text_ones_type() {
        for row in [
            Row::LobbyName,
            Row::PlayerName,
            Row::Address,
            Row::Host,
            Row::GoJoin,
            Row::Dial,
            Row::Knight,
            Row::Ready,
            Row::Begin,
            Row::Back,
        ] {
            assert!(!row.label().is_empty(), "{row:?}");
            assert_eq!(
                row.is_text(),
                matches!(row, Row::LobbyName | Row::PlayerName | Row::Address)
            );
        }
    }
}
