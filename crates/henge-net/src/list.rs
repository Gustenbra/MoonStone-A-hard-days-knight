//! The list server: finding a game without being told an address.
//!
//! **Ours**, like everything else in this crate. It is two small services in one
//! program, and neither of them is the game:
//!
//! - **The list.** A host announces its lobby and refreshes every
//!   [`REFRESH`]; anybody can ask for the list. What is announced is a name, a
//!   port, how many are in it and whether it wants a password. **The password
//!   itself never leaves the host**: the list carries [`Listing::locked`], the
//!   host checks the word. So the directory cannot be made to hand out entry to
//!   a game, because it does not know how.
//! - **The relay**, for a host whose router will not open a port. The host keeps
//!   one outbound connection to the server; when a guest arrives the server asks
//!   for a second one, glues the two sockets together and copies bytes between
//!   them for the rest of the game. It never parses what it carries.
//!
//! ### The reachability probe, which is the part worth having
//!
//! An announcement arrives on a TCP connection, so the server already knows the
//! host's public address: it is the source address of that connection, and no
//! router has to be believed about it. The server then tries to connect *back*
//! to the announced port. That is the real test, and it is the only honest
//! answer to "can my friends reach me": not what UPnP claimed, but whether
//! something outside the house actually got in. [`Announced::reachable`] is that
//! answer, and a host that comes back unreachable asks for a relay.
//!
//! ### What the server is not
//!
//! It is not authoritative over anything. It holds no game state, it cannot
//! join a game, it cannot see a password, and a game that finds its peers
//! without it plays exactly the same. Losing it costs the *browse* and the
//! relay, and nothing else.

use crate::proto::{cut, LOBBY_NAME_MAX};
use crate::wire::{Link, WireError};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Bumped when the meaning of anything below changes. Kept apart from
/// [`crate::proto::PROTOCOL`] because the two evolve for different reasons: a
/// change to what a seat's input word carries is nothing to do with the
/// directory.
pub const LIST_PROTOCOL: u32 = 1;

/// The port the list server listens on unless told otherwise. One above the
/// game's own, so a machine can run both without being asked about it.
pub const DEFAULT_LIST_PORT: u16 = 19911;

/// How often a host re-announces. Three of these is [`STALE`], so a host has to
/// miss three in a row before its game drops off the list.
pub const REFRESH: Duration = Duration::from_secs(15);

/// A game not heard from for this long is taken off the list.
pub const STALE: Duration = Duration::from_secs(45);

/// How long the server will wait when it tries to connect back to a host, and
/// how long a client waits for the list. Both are short on purpose: a menu that
/// has stopped for a second looks broken.
pub const PATIENCE: Duration = Duration::from_millis(700);

/// One game on the list.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    /// The server's own name for this entry, which is what a refresh and a
    /// withdrawal quote back.
    pub id: String,
    /// What the host called the game.
    pub name: String,
    /// Where to join: an address and a port as the server saw them, or, for a
    /// relayed game, the server's own address. A browser does not have to tell
    /// the two apart, which is the point of [`Listing::code`].
    pub at: String,
    /// The relay code, when this game is reached through the server rather than
    /// directly. Empty otherwise.
    #[serde(default)]
    pub code: String,
    /// How many are in the lobby, and how many it holds.
    pub players: u8,
    pub seats: u8,
    /// Whether a password is wanted. **Not the password.**
    #[serde(default)]
    pub locked: bool,
    /// The build that opened it, so a browser is not offered a game its own copy
    /// of the game cannot join.
    #[serde(default)]
    pub version: String,
}

impl Listing {
    /// Whether it is reached through the relay.
    pub fn relayed(&self) -> bool {
        !self.code.is_empty()
    }

    /// Whether there is room.
    pub fn open(&self) -> bool {
        self.players < self.seats
    }

    /// One line for the browser's list.
    pub fn line(&self) -> String {
        format!(
            "{} {}/{}{}",
            self.name,
            self.players,
            self.seats,
            if self.locked { " *" } else { "" }
        )
    }
}

/// What the server was told, and what it says back.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum ListMsg {
    /// Host to server, on opening a lobby and every [`REFRESH`] after.
    /// `id` is empty the first time and the server's own name for the entry
    /// after that.
    Announce {
        protocol: u32,
        id: String,
        name: String,
        port: u16,
        players: u8,
        seats: u8,
        locked: bool,
        version: String,
    },
    /// Server to host: it is on the list, this is its name, this is how the
    /// server saw you, and this is whether the server could get back in.
    Announced(Announced),
    /// Host to server: my port cannot be reached, so send guests through you.
    WantRelay,
    /// Server to host: guests who ask for this code will be introduced.
    Relayed { code: String },
    /// Server to host, on the control connection: somebody is waiting under your
    /// code. Open a second connection and [`ListMsg::Attach`] this ticket to it.
    Waiting { ticket: String },
    /// Host to server, on a fresh connection: this socket is my end of that
    /// introduction.
    Attach { ticket: String },
    /// Guest to server, on a fresh connection: introduce me to that game.
    Reach { code: String },
    /// Server to both ends: you are introduced. **Everything after this frame on
    /// this socket is the game's own, and the server does not read it.**
    Open,
    /// Host to server.
    Withdraw { id: String },
    /// Browser to server.
    Browse { protocol: u32, version: String },
    /// Server to browser.
    Games { games: Vec<Listing> },
    /// Server to anybody, with a sentence a person can act on.
    Refused { why: String },
}

/// What the server said about an announcement.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Announced {
    pub id: String,
    /// The address the server saw the announcement come from, with the announced
    /// port on it. This is what a friend types, and it is measured rather than
    /// guessed.
    pub seen_as: String,
    /// Whether the server, from outside the house, could open a connection to
    /// that address. The only honest answer to "can my friends reach me".
    pub reachable: bool,
}

/// The host's side of the list: announce, refresh, withdraw.
///
/// Non-blocking after the first connection, like everything else in this crate,
/// so a lobby screen keeps drawing while the server thinks.
pub struct Directory {
    link: Link<ListMsg>,
    /// The server's name for our entry, once it has given one.
    pub id: String,
    /// The last thing the server said about us.
    pub told: Option<Announced>,
    /// The relay code, once we have asked for and been given one.
    pub code: String,
    /// The address the list server is at, which is also the address a relayed
    /// game is joined through.
    pub server: String,
    /// What to re-announce, kept so a refresh needs no arguments.
    name: String,
    port: u16,
    seats: u8,
    locked: bool,
    version: String,
    last: std::time::Instant,
    /// Anything worth showing the person.
    notes: Vec<String>,
}

impl Directory {
    /// Announce a lobby. Blocks only for the connection.
    #[allow(clippy::too_many_arguments)]
    pub fn announce(
        server: &str,
        name: &str,
        port: u16,
        seats: u8,
        locked: bool,
        version: &str,
    ) -> Result<Directory, WireError> {
        let mut link: Link<ListMsg> = Link::connect_within(with_port(server), PATIENCE * 4)?;
        let name = cut(name, LOBBY_NAME_MAX);
        link.send(&ListMsg::Announce {
            protocol: LIST_PROTOCOL,
            id: String::new(),
            name: name.clone(),
            port,
            players: 1,
            seats,
            locked,
            version: version.to_string(),
        })?;
        link.flush()?;
        Ok(Directory {
            link,
            id: String::new(),
            told: None,
            code: String::new(),
            server: with_port(server),
            name,
            port,
            seats,
            locked,
            version: version.to_string(),
            last: std::time::Instant::now(),
            notes: Vec::new(),
        })
    }

    /// Read the server, and re-announce when it is time. Call once a tick.
    ///
    /// Returns the tickets the server has handed us, which are guests waiting to
    /// be let in through the relay.
    pub fn poll(&mut self, players: u8) -> Vec<String> {
        let mut tickets = Vec::new();
        let (msgs, err) = self.link.poll();
        for m in msgs {
            match m {
                ListMsg::Announced(a) => {
                    self.id = a.id.clone();
                    if !a.reachable && self.code.is_empty() {
                        // The server could not get back in, so ask it to carry
                        // the game instead. This is the whole reason the probe
                        // exists.
                        let _ = self.link.send(&ListMsg::WantRelay);
                        let _ = self.link.flush();
                    }
                    self.told = Some(a);
                }
                ListMsg::Relayed { code } => {
                    self.code = code;
                    self.notes
                        .push("your router would not open the port, so the game is being carried by the list server".into());
                }
                ListMsg::Waiting { ticket } => tickets.push(ticket),
                ListMsg::Refused { why } => self.notes.push(why),
                // The server says nothing else to a host.
                _ => {}
            }
        }
        if let Some(e) = err {
            self.notes.push(format!("the list server went away: {e}"));
        }
        if self.last.elapsed() >= REFRESH {
            self.last = std::time::Instant::now();
            let _ = self.link.send(&ListMsg::Announce {
                protocol: LIST_PROTOCOL,
                id: self.id.clone(),
                name: self.name.clone(),
                port: self.port,
                players,
                seats: self.seats,
                locked: self.locked,
                version: self.version.clone(),
            });
            let _ = self.link.flush();
        }
        tickets
    }

    /// Anything worth telling the person, taken as it is read.
    pub fn notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notes)
    }

    /// Whether the server got back in. Nothing until it has said.
    pub fn reachable(&self) -> Option<bool> {
        self.told.as_ref().map(|t| t.reachable)
    }

    /// The address to read out to a friend.
    pub fn address(&self) -> Option<String> {
        if !self.code.is_empty() {
            return Some(format!("{} code {}", self.server, self.code));
        }
        self.told.as_ref().map(|t| t.seen_as.clone())
    }

    /// Take the game off the list. A lobby that closes politely does not leave a
    /// dead entry for somebody to try to join.
    pub fn withdraw(&mut self) {
        if self.id.is_empty() {
            return;
        }
        let _ = self.link.send(&ListMsg::Withdraw {
            id: self.id.clone(),
        });
        let _ = self.link.flush();
    }

    /// Open the host's end of one introduction: a fresh connection to the server
    /// carrying a ticket, which comes back as an ordinary game link.
    pub fn attach(&self, ticket: &str) -> Result<Link<crate::proto::Msg>, WireError> {
        let mut link: Link<ListMsg> = Link::connect_within(&self.server, PATIENCE * 4)?;
        link.send(&ListMsg::Attach {
            ticket: ticket.to_string(),
        })?;
        link.flush()?;
        wait_for_open(link)
    }
}

/// Ask the list server what games there are. Blocks, briefly: it is one
/// question, and the screen that asks it has just been opened by somebody who
/// pressed a key and is waiting.
pub fn browse(server: &str, version: &str) -> Result<Vec<Listing>, WireError> {
    let mut link: Link<ListMsg> = Link::connect_within(with_port(server), PATIENCE * 4)?;
    link.send(&ListMsg::Browse {
        protocol: LIST_PROTOCOL,
        version: version.to_string(),
    })?;
    link.flush()?;
    let until = std::time::Instant::now() + PATIENCE * 6;
    while std::time::Instant::now() < until {
        let (msgs, err) = link.poll();
        for m in msgs {
            match m {
                ListMsg::Games { games } => return Ok(games),
                ListMsg::Refused { why } => {
                    return Err(WireError::Io(std::io::Error::other(why)));
                }
                _ => {}
            }
        }
        if let Some(e) = err {
            return Err(e);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Err(WireError::Io(std::io::Error::other(
        "the list server did not answer",
    )))
}

/// A guest's side of the relay: connect to the server, ask for a code, and come
/// away with an ordinary game link.
pub fn reach(server: &str, code: &str) -> Result<Link<crate::proto::Msg>, WireError> {
    let mut link: Link<ListMsg> = Link::connect_within(with_port(server), PATIENCE * 8)?;
    link.send(&ListMsg::Reach {
        code: code.to_string(),
    })?;
    link.flush()?;
    wait_for_open(link)
}

/// Wait for the server to say the two ends are introduced, then stop speaking
/// its language.
///
/// The bytes that arrived in the same read as [`ListMsg::Open`] are carried over
/// rather than dropped: see [`Link::into_carrying`].
fn wait_for_open(mut link: Link<ListMsg>) -> Result<Link<crate::proto::Msg>, WireError> {
    let until = std::time::Instant::now() + PATIENCE * 20;
    while std::time::Instant::now() < until {
        let (msgs, err) = link.poll();
        for m in msgs {
            match m {
                ListMsg::Open => return Ok(link.into_carrying()),
                ListMsg::Refused { why } => {
                    return Err(WireError::Io(std::io::Error::other(why)));
                }
                _ => {}
            }
        }
        if let Some(e) = err {
            return Err(e);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Err(WireError::Io(std::io::Error::other(
        "the list server did not introduce us",
    )))
}

/// A bare address means the usual port, because nobody wants to type a number
/// they were never told.
pub fn with_port(server: &str) -> String {
    let s = server.trim();
    // An IPv6 literal in brackets already carries its own colons.
    let has_port = if let Some(end) = s.rfind(']') {
        s[end..].contains(':')
    } else {
        s.contains(':')
    };
    if has_port {
        s.to_string()
    } else {
        format!("{s}:{DEFAULT_LIST_PORT}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_address_gets_the_usual_port() {
        assert_eq!(with_port("203.0.113.7"), "203.0.113.7:19911");
        assert_eq!(with_port(" games.example.com "), "games.example.com:19911");
        assert_eq!(with_port("203.0.113.7:25000"), "203.0.113.7:25000");
        // An IPv6 literal is full of colons and only the one after the bracket
        // is a port.
        assert_eq!(with_port("[2001:db8::1]"), "[2001:db8::1]:19911");
        assert_eq!(with_port("[2001:db8::1]:25000"), "[2001:db8::1]:25000");
    }

    #[test]
    fn a_listing_says_what_it_is_in_one_line() {
        let l = Listing {
            id: "a".into(),
            name: "CARLS GAME".into(),
            at: "203.0.113.7:19910".into(),
            code: String::new(),
            players: 2,
            seats: 4,
            locked: true,
            version: "0.1.0".into(),
        };
        assert_eq!(l.line(), "CARLS GAME 2/4 *");
        assert!(l.open());
        assert!(!l.relayed());
        let full = Listing {
            players: 4,
            locked: false,
            ..l.clone()
        };
        assert_eq!(full.line(), "CARLS GAME 4/4");
        assert!(!full.open());
        let carried = Listing {
            code: "7f3a".into(),
            ..l
        };
        assert!(carried.relayed());
    }

    /// The password is the host's business. The list carries only the fact that
    /// there is one, so the server cannot be made to hand out entry to a game.
    #[test]
    fn the_list_never_carries_a_password() {
        let json = serde_json::to_string(&ListMsg::Announce {
            protocol: LIST_PROTOCOL,
            id: String::new(),
            name: "x".into(),
            port: 1,
            players: 1,
            seats: 4,
            locked: true,
            version: "v".into(),
        })
        .unwrap();
        assert!(json.contains("\"locked\":true"));
        assert!(!json.to_lowercase().contains("password"));
    }

    #[test]
    fn a_message_round_trips() {
        for m in [
            ListMsg::WantRelay,
            ListMsg::Open,
            ListMsg::Relayed {
                code: "abcd".into(),
            },
            ListMsg::Waiting {
                ticket: "t1".into(),
            },
            ListMsg::Announced(Announced {
                id: "g1".into(),
                seen_as: "203.0.113.7:19910".into(),
                reachable: false,
            }),
        ] {
            let json = serde_json::to_string(&m).unwrap();
            assert_eq!(serde_json::from_str::<ListMsg>(&json).unwrap(), m);
        }
    }

    /// Three missed refreshes and no more, so a host on a bad line is not
    /// dropped for one lost packet.
    #[test]
    fn a_game_survives_two_missed_refreshes() {
        assert!(STALE >= REFRESH * 3);
        assert!(STALE < REFRESH * 4);
    }
}
