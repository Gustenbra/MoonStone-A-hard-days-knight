//! What crosses the wire.
//!
//! Every message is JSON in a length-prefixed frame ([`crate::wire`]). JSON
//! because the whole of this project's data already is, because a protocol you
//! can read in a log is a protocol you can debug from a bug report, and because
//! the volume is nothing: the busiest message is [`Msg::Turn`], and even at the
//! 70.0863 Hz retrace that is under five kilobytes a second.

use serde::{Deserialize, Serialize};

/// Bumped whenever the meaning of anything below changes. A peer that does not
/// match is turned away by name at [`Msg::Hello`] rather than left to desync
/// twenty minutes into a quest.
pub const PROTOCOL: u32 = 1;

/// Four seats, because there are four knights. The same number as
/// `henge_core::shell::SEATS`, and asserted equal to it in the tests.
pub const SEATS: usize = 4;

/// The port a host opens unless told otherwise. Not a registered number and it
/// does not need to be: it is the year on the box.
pub const DEFAULT_PORT: u16 = 19910;

/// As much of a lobby name as is kept. The roster is drawn with the game's own
/// font on a 320-pixel screen, so a longer one would not fit on it anyway.
pub const LOBBY_NAME_MAX: usize = 24;

/// As much of a player name as is kept: `TypeName`'s own thirteen, so a name
/// typed in the lobby is a name the select screen could have typed.
pub const PLAYER_NAME_MAX: usize = 13;

/// One seat's input for one tick.
///
/// `pad` is the original's own word and nothing else: `GetInputDevice` ORs the
/// stick into the keyboard and hands the combat loop five bits, and those five
/// bits are what the simulation reads. The other three fields are the keyboard
/// beyond the stick, which the original reaches through `ScanKEYS` and
/// `DisplayStack` rather than through the game port, and which this game needs
/// on exactly two screens: typing a name, and the paper's number keys.
///
/// **There is no "pressed" field, on purpose.** An edge is the rising edge of
/// `pad` against the tick before it, computed on every machine from the same
/// stream, so two peers cannot disagree about whether a press happened. That is
/// also how the original gets its edges: `BOUNCEBUTTON` remembers the last word
/// and compares.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SeatInput {
    /// `RIGHT`, `LEFT`, `DOWN`, `UP`, `FIRE`: `henge_desktop::input`'s five bits.
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub pad: u8,
    /// The keys that are not on the stick: see [`key`].
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub keys: u8,
    /// What `ASCIIKEY` gave for whatever was pressed, for `TypeName`. Already an
    /// edge: a held key types one character.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typed: Option<char>,
    /// One to nine, for `_MAP:DisplayStack`'s reader. An edge against the tick
    /// before it, like `pad`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<u8>,
}

fn is_zero_u8(v: &u8) -> bool {
    *v == 0
}

/// [`SeatInput::keys`]' bits.
pub mod key {
    /// Enter or Space: what `ScanKEYS` takes as the end of a name, and what
    /// every menu in this build takes as well as fire.
    pub const TAKE: u8 = 0x01;
    /// Backspace, scancode 0x0e, which `ScanKEYS` tests before `ASCIIKEY`.
    pub const BACK: u8 = 0x02;
}

impl SeatInput {
    pub fn take(&self) -> bool {
        self.keys & key::TAKE != 0
    }

    pub fn back(&self) -> bool {
        self.keys & key::BACK != 0
    }

    /// Nothing held and nothing typed: what a seat sends while its player is
    /// doing nothing, and what the first few ticks of a game are primed with.
    pub fn idle(&self) -> bool {
        *self == SeatInput::default()
    }
}

/// One player in a lobby.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Player {
    /// Which of the four seats. The host is always seat zero.
    pub seat: u8,
    /// What they called themselves, cut to [`PLAYER_NAME_MAX`].
    pub name: String,
    /// Sitting down and happy to start.
    #[serde(default)]
    pub ready: bool,
}

/// A lobby, as the host keeps it and as every guest is told it.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Lobby {
    /// What the host called the game, cut to [`LOBBY_NAME_MAX`].
    pub name: String,
    /// Everyone in it, in seat order, seat zero first.
    pub players: Vec<Player>,
    /// `GOREOPT`. The host's, because it is one world.
    #[serde(default)]
    pub gore: bool,
    /// Whether the host has started: a lobby that has is not joinable.
    #[serde(default)]
    pub started: bool,
}

impl Lobby {
    pub fn new(name: &str, gore: bool) -> Lobby {
        Lobby {
            name: cut(name, LOBBY_NAME_MAX),
            players: Vec::new(),
            gore,
            started: false,
        }
    }

    pub fn seated(&self, seat: u8) -> Option<&Player> {
        self.players.iter().find(|p| p.seat == seat)
    }

    /// The lowest seat nobody is in, or nothing when the lobby is full.
    pub fn free_seat(&self) -> Option<u8> {
        (0..SEATS as u8).find(|s| self.seated(*s).is_none())
    }

    /// How many are in it, which becomes `NUM_PLAYERS`.
    pub fn players(&self) -> usize {
        self.players.len()
    }

    /// Whether the host may start: somebody is in it and everybody in it is
    /// ready.
    ///
    /// **Which knight each of them plays is not settled here.** That is
    /// `ChooseKnight`'s screen, which the original already has and which this
    /// game already draws; a lobby that chose knights would be a second, worse
    /// copy of it. What the lobby answers is who is at the table.
    pub fn can_start(&self) -> bool {
        !self.players.is_empty() && self.players.iter().all(|p| p.ready)
    }
}

/// Cut a name to a length, by characters rather than by bytes so a multi-byte
/// one cannot be cut in half.
pub fn cut(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Everything a message can be.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Msg {
    /// Guest to host, first thing. The protocol is checked before anything else
    /// is believed.
    ///
    /// `password` is empty unless the host asked for one, and it goes **to the
    /// host and to nobody else**: the list server carries only
    /// [`Listing::locked`](crate::list::Listing::locked), the fact that there is
    /// a word, so a directory cannot be made to hand out entry to a game.
    Hello {
        protocol: u32,
        name: String,
        #[serde(default)]
        password: String,
    },
    /// Host to guest: you are in, and this is your seat.
    Welcome {
        seat: u8,
        lobby: Lobby,
    },
    /// Host to guest: no. `why` is shown to the person, so it is a sentence.
    Refused {
        why: String,
    },
    /// Host to everyone, whenever the lobby changes.
    Roster {
        lobby: Lobby,
    },
    /// Guest to host: I am ready, or I am not. The host is the only thing that
    /// edits the roster, so a guest asks rather than tells.
    Seated {
        ready: bool,
    },
    /// Host to everyone: the game begins. After this, nothing but input,
    /// checks and goodbyes.
    ///
    /// `seats` is `NUM_PLAYERS`, which is what every machine opens
    /// `ChooseKnight` with. `delay` is the input delay in ticks, the host's
    /// choice for everyone; `check` is how often fingerprints are compared.
    ///
    /// No knights and no names: the select screen settles those, in lockstep,
    /// with every seat driving its own turn on it.
    Start {
        seats: u8,
        gore: bool,
        delay: u8,
        check: u32,
    },
    /// Guest to host: this seat's input for that tick.
    Input {
        tick: u32,
        input: SeatInput,
    },
    /// Host to everyone: every seat's input for that tick, in seat order. A tick
    /// is not sent until all of it is in, so receiving this is permission to run.
    Turn {
        tick: u32,
        seats: Vec<SeatInput>,
    },
    /// Either way: my state's fingerprint at that tick.
    Check {
        tick: u32,
        hash: u64,
    },
    /// Host to everyone: they differed, and the game is over as a fair one.
    /// `hashes` is seat order, so the log says which machine went its own way.
    Desync {
        tick: u32,
        hashes: Vec<u64>,
    },
    /// Host to guest, and the guest echoes it back unchanged.
    ///
    /// `at` is a number the host chose and only the host reads: no clock is
    /// shared and none has to be. What comes back tells the host how long the
    /// round trip took, which is what the input delay is chosen from. See
    /// [`crate::lockstep::delay_for_rtt`].
    Ping {
        at: u64,
    },
    Pong {
        at: u64,
    },
    /// Leaving, politely. `why` is empty for an ordinary quit.
    Bye {
        why: String,
    },
}

impl Msg {
    /// Whether this message belongs to the lobby rather than to a running game.
    /// A guest that gets one of these after [`Msg::Start`] has a host that is
    /// confused, and says so rather than acting on it.
    pub fn is_lobby(&self) -> bool {
        matches!(
            self,
            Msg::Hello { .. }
                | Msg::Welcome { .. }
                | Msg::Refused { .. }
                | Msg::Roster { .. }
                | Msg::Seated { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_seat_count_is_the_games_own() {
        assert_eq!(SEATS, henge_core::shell::SEATS);
        assert_eq!(PLAYER_NAME_MAX, henge_core::shell::NAME_MAX);
    }

    /// An idle seat is the common case and costs almost nothing on the wire,
    /// which is what makes a per-tick protocol affordable at 70 Hz.
    #[test]
    fn an_idle_seat_is_two_bytes_of_json() {
        let json = serde_json::to_string(&SeatInput::default()).unwrap();
        assert_eq!(json, "{}");
        assert!(SeatInput::default().idle());
    }

    #[test]
    fn a_seat_input_round_trips() {
        let i = SeatInput {
            pad: 0x11,
            keys: key::TAKE | key::BACK,
            typed: Some('Q'),
            number: Some(3),
        };
        let json = serde_json::to_string(&i).unwrap();
        assert_eq!(serde_json::from_str::<SeatInput>(&json).unwrap(), i);
        assert!(i.take() && i.back());
        assert!(!i.idle());
    }

    #[test]
    fn a_lobby_fills_seat_by_seat_and_then_is_full() {
        let mut l = Lobby::new("Carl's game", true);
        for expect in 0..SEATS as u8 {
            let seat = l.free_seat().expect("a seat");
            assert_eq!(seat, expect);
            l.players.push(Player {
                seat,
                name: format!("p{seat}"),
                ready: true,
            });
        }
        assert_eq!(l.free_seat(), None);
        assert_eq!(l.players(), SEATS);
        assert!(l.can_start());
    }

    /// The lobby does not choose knights, so two people wanting the same one is
    /// not a thing it can even represent. `ChooseKnight`'s own `choose_knight`
    /// bitmask settles that on the select screen, as it always did.
    #[test]
    fn the_lobby_settles_who_is_here_and_nothing_about_knights() {
        let json = serde_json::to_string(&Player {
            seat: 0,
            name: "carl".into(),
            ready: true,
        })
        .unwrap();
        assert!(!json.to_lowercase().contains("knight"));
    }

    #[test]
    fn nobody_starts_until_everybody_is_ready() {
        let mut l = Lobby::new("x", false);
        assert!(!l.can_start(), "an empty lobby starts nothing");
        l.players.push(Player {
            seat: 0,
            name: "a".into(),
            ready: false,
        });
        assert!(!l.can_start());
        l.players[0].ready = true;
        assert!(l.can_start());
    }

    #[test]
    fn a_name_is_cut_by_characters() {
        assert_eq!(cut("SIR CHRISTOPHER", PLAYER_NAME_MAX), "SIR CHRISTOPH");
        // Thirteen characters, not thirteen bytes.
        let s = cut("ååååååååååååååå", PLAYER_NAME_MAX);
        assert_eq!(s.chars().count(), PLAYER_NAME_MAX);
    }

    /// The password rides on the hello and on nothing else, so it reaches the
    /// host and no other part of this crate ever sees it.
    #[test]
    fn a_password_travels_on_the_hello_and_nowhere_else() {
        let hello = Msg::Hello {
            protocol: PROTOCOL,
            name: "anna".into(),
            password: "portcullis".into(),
        };
        let json = serde_json::to_string(&hello).unwrap();
        assert_eq!(serde_json::from_str::<Msg>(&json).unwrap(), hello);
        // A lobby is what everybody in it is told, and it does not carry one.
        let mut lobby = Lobby::new("x", false);
        lobby.players.push(Player {
            seat: 0,
            name: "carl".into(),
            ready: true,
        });
        let roster = serde_json::to_string(&lobby).unwrap();
        assert!(!roster.to_lowercase().contains("portcullis"));
        assert!(!roster.to_lowercase().contains("password"));
        // And an older peer that sends no password at all is understood as
        // sending an empty one rather than refused for the wrong reason.
        let bare: Msg = serde_json::from_str(r#"{"Hello":{"protocol":1,"name":"anna"}}"#).unwrap();
        assert_eq!(
            bare,
            Msg::Hello {
                protocol: 1,
                name: "anna".into(),
                password: String::new()
            }
        );
    }

    #[test]
    fn a_message_round_trips() {
        let m = Msg::Start {
            seats: 2,
            gore: true,
            delay: 6,
            check: 64,
        };
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<Msg>(&json).unwrap(), m);
        assert!(!m.is_lobby());
        assert!(Msg::Roster {
            lobby: Lobby::new("x", false)
        }
        .is_lobby());
    }
}
