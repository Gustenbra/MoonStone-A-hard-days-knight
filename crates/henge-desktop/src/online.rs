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
//! **It settles who is here and nothing else.** Which knight a seat plays is
//! `ChooseKnight`'s, after the game begins: pressing Begin opens the same select
//! screen a game at one keyboard gets, and the four take their turns on it in
//! lockstep. A knight row lived here for a while, which meant two screens for
//! one choice and a screen of ours doing a job the image already has a screen
//! for.
//!
//! It sits here rather than in `henge_core` because it is shell, and beside
//! `shell.rs` rather than inside it because the recovered screens in there
//! should stay recovered.

use henge_core::shell::NAME_MAX;
use henge_net::list::Listing;
use henge_net::proto::{Lobby, LOBBY_NAME_MAX};

/// Which page of the lobby is up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    /// Host a game, or join one.
    Menu,
    /// Naming a game before opening it.
    Create,
    /// The list of games somebody else has opened.
    Browse,
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
    /// The word this game wants, if it wants one.
    Password,
    /// Look at the list of open games.
    GoBrowse,
    /// Ask the list server again.
    Again,
    /// One game on the list.
    Game(usize),
    /// Open the lobby, which is where the port is asked for.
    Host,
    /// Go to the joining page.
    GoJoin,
    /// Dial the address.
    Dial,
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
        matches!(
            self,
            Row::LobbyName | Row::PlayerName | Row::Address | Row::Password
        )
    }

    /// The label, which is also what the tests read so the two cannot drift.
    pub fn label(self) -> &'static str {
        match self {
            Row::LobbyName => "Game",
            Row::PlayerName => "You",
            Row::Address => "Address",
            Row::Password => "Password",
            Row::GoBrowse => "Open games",
            Row::Again => "Look again",
            Row::Game(_) => "",
            Row::Host => "Open a game",
            Row::GoJoin => "Join a game",
            Row::Dial => "Join",
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
    /// Open a lobby under this name, and lock it if a word was given.
    Open {
        game: String,
        you: String,
        password: String,
    },
    /// Dial this address as this person.
    Dial {
        address: String,
        you: String,
        password: String,
    },
    /// Fetch the list of open games from the list server.
    Look,
    /// Join one of the games on the list, however it has to be reached.
    Take {
        game: Listing,
        you: String,
        password: String,
    },
    /// Tell the host whether this seat is ready.
    Seat { ready: bool },
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
    /// The word this game wants, or the one being offered to join with. Empty
    /// means none, and a game with none lets anybody in.
    ///
    /// **It never reaches the list server**: the list carries only the fact that
    /// a game wants one, and the host does the checking.
    pub password: String,
    /// The open games, as the list server last said them.
    pub games: Vec<Listing>,
    /// Whether the list has been asked for since this page was opened, so an
    /// empty list can be told apart from one nobody has fetched.
    pub looked: bool,
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
    /// What each seat's round trip measured, in milliseconds, once the host has
    /// measured it. Drawn beside the name, because a lobby is where somebody
    /// finds out the line is bad rather than a minute into a fight.
    pub trips: std::collections::BTreeMap<u8, u32>,
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
            password: String::new(),
            games: Vec::new(),
            looked: false,
            ready: false,
            roster: Lobby::default(),
            seat: None,
            hosting: false,
            note: String::new(),
            reachable: String::new(),
            trips: std::collections::BTreeMap::new(),
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
            Page::Menu => vec![Row::GoBrowse, Row::Host, Row::GoJoin, Row::Back],
            Page::Create => vec![
                Row::LobbyName,
                Row::PlayerName,
                Row::Password,
                Row::Host,
                Row::Back,
            ],
            Page::Browse => {
                let mut rows: Vec<Row> = (0..self.games.len()).map(Row::Game).collect();
                rows.push(Row::Again);
                rows.push(Row::Back);
                rows
            }
            Page::Join => vec![
                Row::Address,
                Row::PlayerName,
                Row::Password,
                Row::Dial,
                Row::Back,
            ],
            Page::Waiting => {
                let mut rows = vec![Row::Ready];
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
            // A password is never drawn: the screen is on somebody's monitor and
            // there may well be somebody else in the room.
            //
            // Masked with `TypeName`'s own caret rather than a star, because the
            // game's font has no star: `GFX:TextASCII` maps anything it does not
            // know to the blank, and a mask of blanks is no mask at all. It maps
            // 0x5c and 0x2f both to glyph 71, the stroke, so a row of those is
            // something a person can actually see themselves typing.
            //
            // Once the typing is done it says only whether there is one. The
            // length of a password is not a thing to put on a screen either.
            Row::Password => {
                let n = self.password.chars().count();
                return if self.typing && self.selected() == row {
                    std::iter::repeat_n(henge_core::shell::CARET, n + 1).collect()
                } else if n == 0 {
                    "None".to_string()
                } else {
                    "Set".to_string()
                };
            }
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

    /// Left and right. Nothing on this screen adjusts: which knight a seat plays
    /// is settled on `ChooseKnight`'s own screen after the game begins, not
    /// here, and everything else is a row you take.
    pub fn adjust(&mut self, _delta: i32) -> Option<Ask> {
        None
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
            Row::GoBrowse => {
                self.page = Page::Browse;
                self.row = 0;
                self.looked = false;
                Some(Ask::Look)
            }
            Row::Again => {
                self.row = 0;
                Some(Ask::Look)
            }
            Row::Game(n) => {
                let game = self.games.get(n)?.clone();
                // A game that wants a word and has not been given one asks for
                // it rather than being refused by the host a second later.
                if game.locked && self.password.trim().is_empty() {
                    self.note = "that game wants a password: set one below".into();
                    self.page = Page::Join;
                    self.row = 2;
                    self.address = game.at.clone();
                    return None;
                }
                Some(Ask::Take {
                    game,
                    you: self.you.clone(),
                    password: self.password.trim().to_string(),
                })
            }
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
                password: self.password.trim().to_string(),
            }),
            Row::Dial => {
                if self.address.trim().is_empty() {
                    self.note = "an address to join first".into();
                    return None;
                }
                Some(Ask::Dial {
                    address: self.address.trim().to_string(),
                    you: self.you.clone(),
                    password: self.password.trim().to_string(),
                })
            }
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
            Row::LobbyName | Row::PlayerName | Row::Address | Row::Password => None,
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
            Row::Password => 32,
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
            Row::Password => &mut self.password,
            _ => &mut self.you,
        }
    }

    /// The list server has answered.
    pub fn listed(&mut self, games: Vec<Listing>, note: &str) {
        self.games = games;
        self.looked = true;
        self.row = 0;
        self.note = note.to_string();
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
        self.games.clear();
        self.looked = false;
        self.trips.clear();
    }

    fn seat_ask(&self) -> Ask {
        Ask::Seat { ready: self.ready }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use henge_net::proto::Player;

    /// Walk on to the create page, whichever row it starts on.
    fn creating() -> Online {
        let mut o = Online::new();
        while o.selected() != Row::Host {
            o.move_by(1);
        }
        o.take();
        assert_eq!(o.page, Page::Create);
        o
    }

    #[test]
    fn the_menu_goes_three_ways_and_back() {
        let mut o = Online::new();
        // Looking at what is open is the first thing offered, because it is what
        // somebody who was told "we are playing tonight" wants.
        assert_eq!(o.selected(), Row::GoBrowse);
        assert_eq!(o.take(), Some(Ask::Look));
        assert_eq!(o.page, Page::Browse);
        o.move_by(9);
        assert_eq!(o.selected(), Row::Back);
        o.take();
        assert_eq!(o.page, Page::Menu);

        o.move_by(1);
        assert_eq!(o.selected(), Row::Host);
        o.take();
        assert_eq!(o.page, Page::Create);
        o.move_by(9);
        assert_eq!(o.selected(), Row::Back);
        assert_eq!(o.take(), None);
        assert_eq!(o.page, Page::Menu);

        o.move_by(2);
        assert_eq!(o.selected(), Row::GoJoin);
        o.take();
        assert_eq!(o.page, Page::Join);
        o.move_by(9);
        assert_eq!(o.selected(), Row::Back, "and it clamps at the bottom");
        o.take();
        o.move_by(9);
        assert_eq!(o.take(), Some(Ask::Leave));
    }

    /// The list, and taking a game off it.
    #[test]
    fn a_game_is_taken_straight_off_the_list() {
        let mut o = Online::new();
        assert_eq!(o.take(), Some(Ask::Look));
        assert!(!o.looked, "nothing has answered yet");
        // Nothing open: the two rows that are always there, and a line saying so.
        o.listed(Vec::new(), "no games open");
        assert!(o.looked);
        assert_eq!(o.rows(), vec![Row::Again, Row::Back]);
        assert_eq!(o.take(), Some(Ask::Look), "and it can be asked again");

        let open = Listing {
            id: "g1".into(),
            name: "CARLS GAME".into(),
            at: "203.0.113.7:19910".into(),
            code: String::new(),
            players: 1,
            seats: 4,
            locked: false,
            version: "v".into(),
        };
        o.listed(vec![open.clone()], "1 open");
        assert_eq!(o.rows(), vec![Row::Game(0), Row::Again, Row::Back]);
        assert_eq!(
            o.take(),
            Some(Ask::Take {
                game: open,
                you: "KNIGHT".into(),
                password: String::new()
            })
        );
    }

    /// A game that wants a word asks for it instead of being refused by its host
    /// a second later.
    #[test]
    fn a_locked_game_asks_for_the_word_before_dialling() {
        let mut o = Online::new();
        let locked = Listing {
            id: "g1".into(),
            name: "LOCKED".into(),
            at: "203.0.113.7:19910".into(),
            code: String::new(),
            players: 1,
            seats: 4,
            locked: true,
            version: "v".into(),
        };
        o.take();
        o.listed(vec![locked.clone()], "1 open");
        assert_eq!(o.take(), None, "no word yet");
        assert_eq!(o.page, Page::Join);
        assert_eq!(o.selected(), Row::Password);
        assert_eq!(o.address, "203.0.113.7:19910", "and it kept the address");
        assert!(o.note.contains("password"));
        // Given the word, the same game goes through.
        o.take();
        for c in "portcullis".chars() {
            o.type_char(c);
        }
        o.take();
        o.page = Page::Browse;
        o.row = 0;
        assert_eq!(
            o.take(),
            Some(Ask::Take {
                game: locked,
                you: "KNIGHT".into(),
                password: "portcullis".into()
            })
        );
    }

    /// A password is never drawn. Somebody else may be in the room.
    ///
    /// The mask is `TypeName`'s caret and not a star because the font has no
    /// star: `GFX:TextASCII` draws what it does not know as the blank, so a row
    /// of stars would be a row of nothing. Once the typing is over the row says
    /// only whether there is a word, because its length is not a thing to show
    /// either.
    #[test]
    fn a_password_is_masked_and_never_shown_as_itself() {
        let mut o = creating();
        o.move_by(2);
        assert_eq!(o.selected(), Row::Password);
        assert_eq!(o.shown(Row::Password), "None");
        o.take();
        for c in "portcullis".chars() {
            o.type_char(c);
        }
        let caret = henge_core::shell::CARET;
        let masked = o.shown(Row::Password);
        assert_eq!(masked.chars().count(), "portcullis".chars().count() + 1);
        assert!(masked.chars().all(|c| c == caret), "{masked:?}");
        o.take();
        assert_eq!(o.shown(Row::Password), "Set");
        assert_eq!(
            o.password, "portcullis",
            "and it is still the word underneath"
        );
    }

    /// A text row behaves like `TypeName`: fire starts it, fire ends it, and
    /// nothing else moves while it is running.
    #[test]
    fn a_name_is_typed_the_way_the_select_screen_types_one() {
        let mut o = creating();
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
        let mut o = creating();
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
        let mut o = creating();
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
        let mut o = creating();
        o.move_by(3);
        assert_eq!(o.selected(), Row::Host);
        assert_eq!(
            o.take(),
            Some(Ask::Open {
                game: "MOONSTONE".into(),
                you: "KNIGHT".into(),
                password: String::new()
            })
        );
    }

    #[test]
    fn joining_needs_an_address_and_says_so() {
        let mut o = Online::new();
        o.move_by(2);
        assert_eq!(o.selected(), Row::GoJoin);
        o.take();
        o.move_by(3);
        assert_eq!(o.selected(), Row::Dial);
        assert_eq!(o.take(), None, "nothing to dial");
        assert!(o.note.contains("address"));
        o.move_by(-3);
        o.take();
        for c in " 10.0.0.4:19910 ".chars() {
            o.type_char(c);
        }
        o.take();
        o.move_by(3);
        assert_eq!(
            o.take(),
            Some(Ask::Dial {
                address: "10.0.0.4:19910".into(),
                you: "KNIGHT".into(),
                password: String::new()
            })
        );
    }

    /// The lobby says who is here and nothing about which knight they play: that
    /// is `ChooseKnight`'s, after the game begins.
    #[test]
    fn the_lobby_has_no_knight_row() {
        let mut o = Online::new();
        o.sat_down(1, false);
        o.roster.players = vec![
            Player {
                seat: 0,
                name: "host".into(),
                ready: true,
            },
            Player {
                seat: 1,
                name: "me".into(),
                ready: false,
            },
        ];
        assert_eq!(o.selected(), Row::Ready);
        assert_eq!(o.adjust(1), None, "and there is nothing here to adjust");
        assert_eq!(o.adjust(-1), None);
    }

    #[test]
    fn ready_is_a_switch_and_the_host_alone_can_begin() {
        let mut o = Online::new();
        o.sat_down(1, false);
        assert_eq!(o.rows(), vec![Row::Ready, Row::Back]);
        assert_eq!(o.take(), Some(Ask::Seat { ready: true }));
        assert!(o.ready);
        o.take();
        assert!(!o.ready, "and it switches back");
        // The host has one more row than a guest.
        let mut h = Online::new();
        h.sat_down(0, true);
        assert_eq!(h.rows(), vec![Row::Ready, Row::Begin, Row::Back]);
        h.move_by(1);
        assert_eq!(h.take(), Some(Ask::Begin));
    }

    #[test]
    fn leaving_a_lobby_goes_back_to_the_menu_with_a_reason() {
        let mut o = Online::new();
        o.sat_down(2, false);
        o.ready = true;
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
            Row::Password,
            Row::Host,
            Row::GoJoin,
            Row::Dial,
            Row::Ready,
            Row::Begin,
            Row::Back,
        ] {
            assert!(!row.label().is_empty(), "{row:?}");
            assert_eq!(
                row.is_text(),
                matches!(
                    row,
                    Row::LobbyName | Row::PlayerName | Row::Address | Row::Password
                )
            );
        }
    }
}
