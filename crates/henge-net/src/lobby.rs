//! The two state machines in front of a game: hosting one, and joining one.
//!
//! Both are pixel-free, like the title and the select screens they sit beside in
//! `henge_core::shell`, and for the same reason: a test drives them exactly as a
//! person does. Neither of them knows what a lobby looks like.
//!
//! The host owns the roster. A guest never edits it; it asks with
//! [`crate::Msg::Seated`] and is told the answer with [`crate::Msg::Roster`], so
//! there is one copy of the truth and three mirrors of it. That is not an
//! authority over the *game* (there is none: see [`crate::lockstep`]) but an
//! authority over who is sitting where, which somebody has to have.

use crate::proto::{cut, Lobby, Msg, Player, SeatInput, PLAYER_NAME_MAX, PROTOCOL, SEATS};
use crate::wire::{Link, Listener, WireError};

/// What happened on the wire, once the framing and the protocol have been dealt
/// with. The caller draws these and nothing else.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// Somebody arrived, and this is the seat they are in.
    Joined { seat: u8, name: String },
    /// Somebody left. A guest sees this as a roster change; the host sees it as
    /// this, because it has to free the seat.
    Left { seat: u8, name: String },
    /// The roster changed for any other reason: a knight taken, a ready flag.
    Roster,
    /// A guest only: you are in, and this is your seat.
    Seated { seat: u8 },
    /// A guest only: you are not in, and this is why.
    Refused { why: String },
    /// The game begins, on exactly these terms.
    Start {
        seats: u8,
        gore: bool,
        knights: Vec<u8>,
        names: Vec<String>,
        delay: u8,
        check: u32,
    },
    /// A host only: a seat's input for a tick.
    Input {
        seat: u8,
        tick: u32,
        input: SeatInput,
    },
    /// A guest only: every seat's input for a tick, which is permission to run it.
    Turn { tick: u32, seats: Vec<SeatInput> },
    /// A fingerprint from a peer.
    Check { seat: u8, tick: u32, hash: u64 },
    /// The host says the machines have diverged.
    Desync { tick: u32, hashes: Vec<u64> },
    /// The link is gone. On a guest this is the end of the game; on a host it is
    /// one seat emptying, and [`Event::Left`] comes with it.
    Lost { seat: u8, why: String },
}

/// What [`Host::open`] and [`Guest::join`] can come back with.
#[derive(Debug)]
pub enum JoinError {
    Wire(WireError),
}

impl std::fmt::Display for JoinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JoinError::Wire(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for JoinError {}

impl From<WireError> for JoinError {
    fn from(e: WireError) -> JoinError {
        JoinError::Wire(e)
    }
}

/// Hosting. The host is always seat zero, which is the seat the original's own
/// `WHICH` starts its round on.
pub struct Host {
    door: Listener,
    /// The roster, which this is the only copy of.
    pub lobby: Lobby,
    /// One link per guest, with the seat it is sitting in.
    guests: Vec<(u8, Link)>,
    /// Whether [`Host::start`] has been called.
    pub started: bool,
}

impl Host {
    /// Open a lobby. `port` of zero takes whatever the system gives, which is
    /// what the tests want and what a person never wants.
    pub fn open(name: &str, player: &str, port: u16, gore: bool) -> Result<Host, JoinError> {
        let door = Listener::open(port)?;
        let mut lobby = Lobby::new(name, gore);
        lobby.players.push(Player {
            seat: 0,
            name: cut(player, PLAYER_NAME_MAX),
            knight: None,
            ready: false,
        });
        Ok(Host {
            door,
            lobby,
            guests: Vec::new(),
            started: false,
        })
    }

    /// The port guests are to be told about.
    pub fn port(&self) -> u16 {
        self.door.port()
    }

    /// How many guests are connected, which is one fewer than the lobby holds.
    pub fn guests(&self) -> usize {
        self.guests.len()
    }

    /// Accept, read, and answer. Call once a tick.
    pub fn poll(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        // Anybody new. A lobby that has started or is full turns them away by
        // name rather than dropping the socket, so the person sees why.
        for mut link in self.door.accept() {
            if self.started {
                let _ = link.send(&Msg::Refused {
                    why: "that game has already started".into(),
                });
                let _ = link.flush();
                continue;
            }
            if self.lobby.free_seat().is_none() {
                let _ = link.send(&Msg::Refused {
                    why: "that game is full".into(),
                });
                let _ = link.flush();
                continue;
            }
            // No seat until the hello, because the protocol has not been checked
            // and a seat held by a stranger is a seat a friend cannot have.
            self.guests.push((SEAT_UNSEATED, link));
        }
        // Everything said to us.
        let mut lost: Vec<(usize, String)> = Vec::new();
        let mut roster_changed = false;
        for (i, (seat, link)) in self.guests.iter_mut().enumerate() {
            let (msgs, err) = link.poll();
            for m in msgs {
                match m {
                    Msg::Hello { protocol, name } => {
                        if protocol != PROTOCOL {
                            let _ = link.send(&Msg::Refused {
                                why: format!(
                                    "that game speaks protocol {PROTOCOL} and yours speaks {protocol}"
                                ),
                            });
                            // Pushed out now rather than at the end of the poll:
                            // this link is about to be dropped, and a refusal
                            // still in its buffer is a refusal nobody reads.
                            let _ = link.flush();
                            lost.push((i, "a different protocol".into()));
                            continue;
                        }
                        if *seat != SEAT_UNSEATED {
                            lost.push((i, "said hello twice".into()));
                            continue;
                        }
                        let Some(free) = self.lobby.free_seat() else {
                            let _ = link.send(&Msg::Refused {
                                why: "that game is full".into(),
                            });
                            let _ = link.flush();
                            lost.push((i, "full".into()));
                            continue;
                        };
                        *seat = free;
                        let name = cut(&name, PLAYER_NAME_MAX);
                        self.lobby.players.push(Player {
                            seat: free,
                            name: name.clone(),
                            knight: None,
                            ready: false,
                        });
                        self.lobby.players.sort_by_key(|p| p.seat);
                        let _ = link.send(&Msg::Welcome {
                            seat: free,
                            lobby: self.lobby.clone(),
                        });
                        out.push(Event::Joined { seat: free, name });
                        roster_changed = true;
                    }
                    Msg::Seated { knight, ready } => {
                        if *seat == SEAT_UNSEATED {
                            continue;
                        }
                        // A knight somebody else has is refused here rather than
                        // at the select screen, where it would already be too
                        // late. Settled before the seat's own record is reached,
                        // because answering it means reading the others.
                        let free = match knight {
                            None => true,
                            Some(k) => {
                                (k as usize) < SEATS
                                    && !self
                                        .lobby
                                        .players
                                        .iter()
                                        .any(|o| o.seat != *seat && o.knight == Some(k))
                            }
                        };
                        if let Some(p) = self.lobby.players.iter_mut().find(|p| p.seat == *seat) {
                            if free {
                                p.knight = knight;
                            }
                            p.ready = ready;
                            roster_changed = true;
                            out.push(Event::Roster);
                        }
                    }
                    Msg::Input { tick, input } => {
                        if *seat != SEAT_UNSEATED {
                            out.push(Event::Input {
                                seat: *seat,
                                tick,
                                input,
                            });
                        }
                    }
                    Msg::Check { tick, hash } => {
                        if *seat != SEAT_UNSEATED {
                            out.push(Event::Check {
                                seat: *seat,
                                tick,
                                hash,
                            });
                        }
                    }
                    Msg::Bye { why } => {
                        lost.push((i, if why.is_empty() { "left".into() } else { why }))
                    }
                    // A guest does not get to tell the host any of these.
                    other => {
                        lost.push((i, format!("said something a guest does not say: {other:?}")));
                    }
                }
            }
            if let Some(e) = err {
                lost.push((i, e.to_string()));
            }
        }
        // Drop what was lost, highest index first so the rest keep their places.
        lost.sort_by_key(|l| std::cmp::Reverse(l.0));
        lost.dedup_by_key(|(i, _)| *i);
        for (i, why) in lost {
            if i >= self.guests.len() {
                continue;
            }
            let (seat, _) = self.guests.remove(i);
            if seat == SEAT_UNSEATED {
                continue;
            }
            let name = self
                .lobby
                .seated(seat)
                .map(|p| p.name.clone())
                .unwrap_or_default();
            self.lobby.players.retain(|p| p.seat != seat);
            out.push(Event::Left {
                seat,
                name: name.clone(),
            });
            out.push(Event::Lost { seat, why });
            roster_changed = true;
        }
        if roster_changed {
            self.broadcast(&Msg::Roster {
                lobby: self.lobby.clone(),
            });
        }
        for (_, link) in self.guests.iter_mut() {
            let _ = link.flush();
        }
        out
    }

    /// The host's own knight and ready flag.
    pub fn seat(&mut self, knight: Option<u8>, ready: bool) {
        let taken = knight.is_some_and(|k| {
            self.lobby
                .players
                .iter()
                .any(|p| p.seat != 0 && p.knight == Some(k))
        });
        if let Some(p) = self.lobby.players.iter_mut().find(|p| p.seat == 0) {
            if !taken {
                p.knight = knight;
            }
            p.ready = ready;
        }
        self.broadcast(&Msg::Roster {
            lobby: self.lobby.clone(),
        });
    }

    pub fn set_gore(&mut self, gore: bool) {
        self.lobby.gore = gore;
        self.broadcast(&Msg::Roster {
            lobby: self.lobby.clone(),
        });
    }

    /// Begin. Every seat is given a knight it has not chosen for itself, lowest
    /// free one first, so a lobby nobody bothered to choose in still starts.
    ///
    /// Returns the terms, which are also what went out on the wire, or nothing
    /// when the lobby is not startable.
    pub fn start(&mut self, delay: u32, check: u32) -> Option<Event> {
        if self.started || !self.lobby.can_start() {
            return None;
        }
        let mut knights: Vec<u8> = Vec::new();
        let mut names: Vec<String> = Vec::new();
        let mut used = [false; SEATS];
        for p in &self.lobby.players {
            if let Some(k) = p.knight {
                if (k as usize) < SEATS {
                    used[k as usize] = true;
                }
            }
        }
        for p in &self.lobby.players {
            let k = match p.knight {
                Some(k) if (k as usize) < SEATS => k,
                _ => {
                    let free = (0..SEATS as u8).find(|k| !used[*k as usize])?;
                    used[free as usize] = true;
                    free
                }
            };
            knights.push(k);
            names.push(p.name.clone());
        }
        self.started = true;
        self.lobby.started = true;
        let ev = Event::Start {
            seats: self.lobby.players() as u8,
            gore: self.lobby.gore,
            knights,
            names,
            delay: delay.min(u8::MAX as u32) as u8,
            check,
        };
        if let Event::Start {
            seats,
            gore,
            knights,
            names,
            delay,
            check,
        } = &ev
        {
            self.broadcast(&Msg::Start {
                seats: *seats,
                gore: *gore,
                knights: knights.clone(),
                names: names.clone(),
                delay: *delay,
                check: *check,
            });
        }
        Some(ev)
    }

    /// A tick's input, to everybody.
    pub fn send_turn(&mut self, tick: u32, seats: Vec<SeatInput>) {
        self.broadcast(&Msg::Turn { tick, seats });
    }

    /// The host's own fingerprint, so a guest can compare for itself rather than
    /// waiting to be told.
    pub fn send_check(&mut self, tick: u32, hash: u64) {
        self.broadcast(&Msg::Check { tick, hash });
    }

    /// A divergence, to everybody, and then the game is over.
    pub fn send_desync(&mut self, tick: u32, hashes: Vec<u64>) {
        self.broadcast(&Msg::Desync { tick, hashes });
    }

    pub fn broadcast(&mut self, msg: &Msg) {
        for (_, link) in self.guests.iter_mut() {
            let _ = link.send(msg);
            let _ = link.flush();
        }
    }

    /// Say goodbye to everybody. A lobby that closes politely frees their
    /// screens at once instead of leaving them waiting on a socket.
    pub fn close(&mut self, why: &str) {
        self.broadcast(&Msg::Bye { why: why.into() });
        self.guests.clear();
        self.lobby.players.retain(|p| p.seat == 0);
    }
}

/// The seat of a socket that has connected and not yet said hello.
const SEAT_UNSEATED: u8 = u8::MAX;

/// Joining.
pub struct Guest {
    link: Link,
    /// The seat the host gave us, once it has.
    pub seat: Option<u8>,
    /// The roster as last told.
    pub lobby: Lobby,
    pub started: bool,
}

impl Guest {
    /// Dial a host and say hello. The hello is queued, not awaited: the answer
    /// arrives through [`Guest::poll`] like everything else.
    pub fn join(addr: impl std::net::ToSocketAddrs, player: &str) -> Result<Guest, JoinError> {
        let mut link = Link::connect(addr)?;
        link.send(&Msg::Hello {
            protocol: PROTOCOL,
            name: cut(player, PLAYER_NAME_MAX),
        })?;
        link.flush()?;
        Ok(Guest {
            link,
            seat: None,
            lobby: Lobby::default(),
            started: false,
        })
    }

    pub fn poll(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        let (msgs, err) = self.link.poll();
        for m in msgs {
            match m {
                Msg::Welcome { seat, lobby } => {
                    self.seat = Some(seat);
                    self.lobby = lobby;
                    out.push(Event::Seated { seat });
                }
                Msg::Refused { why } => out.push(Event::Refused { why }),
                Msg::Roster { lobby } => {
                    self.lobby = lobby;
                    out.push(Event::Roster);
                }
                Msg::Start {
                    seats,
                    gore,
                    knights,
                    names,
                    delay,
                    check,
                } => {
                    self.started = true;
                    self.lobby.started = true;
                    out.push(Event::Start {
                        seats,
                        gore,
                        knights,
                        names,
                        delay,
                        check,
                    });
                }
                Msg::Turn { tick, seats } => out.push(Event::Turn { tick, seats }),
                Msg::Check { tick, hash } => out.push(Event::Check {
                    seat: 0,
                    tick,
                    hash,
                }),
                Msg::Desync { tick, hashes } => out.push(Event::Desync { tick, hashes }),
                Msg::Bye { why } => out.push(Event::Lost {
                    seat: self.seat.unwrap_or(SEAT_UNSEATED),
                    why: if why.is_empty() {
                        "the host closed the game".into()
                    } else {
                        why
                    },
                }),
                // A host does not say any of these to a guest.
                other => out.push(Event::Lost {
                    seat: self.seat.unwrap_or(SEAT_UNSEATED),
                    why: format!("the host said something a host does not say: {other:?}"),
                }),
            }
        }
        if let Some(e) = err {
            out.push(Event::Lost {
                seat: self.seat.unwrap_or(SEAT_UNSEATED),
                why: e.to_string(),
            });
        }
        out
    }

    /// Ask for a knight and say whether we are ready.
    pub fn seat_request(&mut self, knight: Option<u8>, ready: bool) {
        let _ = self.link.send(&Msg::Seated { knight, ready });
        let _ = self.link.flush();
    }

    /// This seat's input for a tick.
    pub fn send_input(&mut self, tick: u32, input: SeatInput) {
        let _ = self.link.send(&Msg::Input { tick, input });
        let _ = self.link.flush();
    }

    /// This machine's fingerprint for a tick.
    pub fn send_check(&mut self, tick: u32, hash: u64) {
        let _ = self.link.send(&Msg::Check { tick, hash });
        let _ = self.link.flush();
    }

    pub fn close(&mut self, why: &str) {
        let _ = self.link.send(&Msg::Bye { why: why.into() });
        let _ = self.link.flush();
    }

    pub fn lost(&self) -> bool {
        self.link.closed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Turn the crank on both ends until the guest has something to say about
    /// itself, which is what a menu's own loop does.
    fn settle(host: &mut Host, guests: &mut [&mut Guest]) -> (Vec<Event>, Vec<Vec<Event>>) {
        let mut h = Vec::new();
        let mut g: Vec<Vec<Event>> = guests.iter().map(|_| Vec::new()).collect();
        for _ in 0..150 {
            h.extend(host.poll());
            for (i, guest) in guests.iter_mut().enumerate() {
                g[i].extend(guest.poll());
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        (h, g)
    }

    #[test]
    fn a_guest_joins_and_gets_the_next_seat() {
        let mut host = Host::open("Carl's game", "carl", 0, true).unwrap();
        let port = host.port();
        let mut anna = Guest::join(("127.0.0.1", port), "anna").unwrap();
        let (h, g) = settle(&mut host, &mut [&mut anna]);
        assert!(h.iter().any(|e| matches!(e, Event::Joined { seat: 1, .. })));
        assert!(g[0].contains(&Event::Seated { seat: 1 }));
        assert_eq!(anna.seat, Some(1));
        assert_eq!(anna.lobby.name, "Carl's game");
        assert_eq!(anna.lobby.players(), 2);
        assert_eq!(host.guests(), 1);
    }

    #[test]
    fn a_long_name_is_cut_to_the_field_it_has_to_fit() {
        let mut host = Host::open("a game", "a host whose name is far too long", 0, false).unwrap();
        assert_eq!(host.lobby.players[0].name.chars().count(), PLAYER_NAME_MAX);
        let port = host.port();
        let mut g = Guest::join(("127.0.0.1", port), "anna of the lakelands").unwrap();
        settle(&mut host, &mut [&mut g]);
        assert_eq!(host.lobby.players[1].name, "anna of the l");
    }

    #[test]
    fn a_fifth_player_is_told_why_rather_than_dropped() {
        let mut host = Host::open("full", "carl", 0, false).unwrap();
        let port = host.port();
        let mut three: Vec<Guest> = (0..3)
            .map(|i| Guest::join(("127.0.0.1", port), &format!("g{i}")).unwrap())
            .collect();
        {
            let mut refs: Vec<&mut Guest> = three.iter_mut().collect();
            settle(&mut host, &mut refs);
        }
        assert_eq!(host.lobby.players(), SEATS);
        let mut late = Guest::join(("127.0.0.1", port), "late").unwrap();
        let (_, g) = settle(&mut host, &mut [&mut late]);
        assert!(
            g[0].iter()
                .any(|e| matches!(e, Event::Refused { why } if why.contains("full"))),
            "{:?}",
            g[0]
        );
        assert_eq!(late.seat, None);
    }

    #[test]
    fn a_guest_that_leaves_frees_its_seat() {
        let mut host = Host::open("x", "carl", 0, false).unwrap();
        let port = host.port();
        let mut anna = Guest::join(("127.0.0.1", port), "anna").unwrap();
        settle(&mut host, &mut [&mut anna]);
        assert_eq!(host.lobby.players(), 2);
        anna.close("");
        drop(anna);
        let mut left = false;
        for _ in 0..400 {
            if host
                .poll()
                .iter()
                .any(|e| matches!(e, Event::Left { seat: 1, .. }))
            {
                left = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(left, "the seat was never freed");
        assert_eq!(host.lobby.players(), 1);
        assert_eq!(host.lobby.free_seat(), Some(1));
    }

    #[test]
    fn two_guests_cannot_take_the_same_knight() {
        let mut host = Host::open("x", "carl", 0, false).unwrap();
        let port = host.port();
        let mut a = Guest::join(("127.0.0.1", port), "a").unwrap();
        let mut b = Guest::join(("127.0.0.1", port), "b").unwrap();
        settle(&mut host, &mut [&mut a, &mut b]);
        a.seat_request(Some(2), true);
        settle(&mut host, &mut [&mut a, &mut b]);
        b.seat_request(Some(2), true);
        settle(&mut host, &mut [&mut a, &mut b]);
        assert_eq!(host.lobby.seated(1).unwrap().knight, Some(2));
        assert_eq!(
            host.lobby.seated(2).unwrap().knight,
            None,
            "the second ask for knight two is refused"
        );
        assert!(host.lobby.knights_are_distinct());
    }

    #[test]
    fn nothing_starts_until_everyone_is_ready_and_then_everyone_is_told() {
        let mut host = Host::open("x", "carl", 0, true).unwrap();
        let port = host.port();
        let mut anna = Guest::join(("127.0.0.1", port), "anna").unwrap();
        settle(&mut host, &mut [&mut anna]);
        host.seat(Some(0), true);
        assert_eq!(host.start(6, 64), None, "anna has not sat down");
        anna.seat_request(Some(3), true);
        settle(&mut host, &mut [&mut anna]);
        let started = host.start(6, 64).expect("the lobby is ready");
        assert_eq!(
            started,
            Event::Start {
                seats: 2,
                gore: true,
                knights: vec![0, 3],
                names: vec!["carl".into(), "anna".into()],
                delay: 6,
                check: 64,
            }
        );
        let (_, g) = settle(&mut host, &mut [&mut anna]);
        assert!(g[0].contains(&started), "{:?}", g[0]);
        assert!(anna.started);
    }

    /// A lobby whose players never chose are given knights rather than refused,
    /// because a game of four people all pressing ready is a game that should
    /// start.
    #[test]
    fn a_seat_that_chose_nothing_is_given_the_lowest_free_knight() {
        let mut host = Host::open("x", "carl", 0, false).unwrap();
        let port = host.port();
        let mut anna = Guest::join(("127.0.0.1", port), "anna").unwrap();
        settle(&mut host, &mut [&mut anna]);
        host.seat(Some(2), true);
        anna.seat_request(None, true);
        settle(&mut host, &mut [&mut anna]);
        let Some(Event::Start { knights, .. }) = host.start(4, 64) else {
            panic!("it should start");
        };
        assert_eq!(knights, vec![2, 0]);
    }

    #[test]
    fn a_game_that_has_started_turns_a_latecomer_away() {
        let mut host = Host::open("x", "carl", 0, false).unwrap();
        let port = host.port();
        host.seat(Some(0), true);
        host.start(4, 64).expect("one player is a lobby");
        let mut late = Guest::join(("127.0.0.1", port), "late").unwrap();
        let (_, g) = settle(&mut host, &mut [&mut late]);
        assert!(
            g[0].iter()
                .any(|e| matches!(e, Event::Refused { why } if why.contains("already started"))),
            "{:?}",
            g[0]
        );
    }

    /// A peer speaking a different protocol is told so by name, which is the
    /// whole reason the number is in the first message.
    #[test]
    fn a_mismatched_protocol_is_refused_by_name() {
        let mut host = Host::open("x", "carl", 0, false).unwrap();
        let port = host.port();
        let mut link = Link::connect(("127.0.0.1", port)).unwrap();
        link.send(&Msg::Hello {
            protocol: PROTOCOL + 99,
            name: "wrong".into(),
        })
        .unwrap();
        link.flush().unwrap();
        for _ in 0..400 {
            host.poll();
            let (msgs, _) = link.poll();
            if let Some(Msg::Refused { why }) = msgs.first() {
                assert!(why.contains(&format!("{}", PROTOCOL + 99)), "{why}");
                assert_eq!(host.lobby.players(), 1, "and no seat was spent on it");
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("it was not refused");
    }

    /// Input and fingerprints reach the host tagged with the seat that sent
    /// them, which is what stops a guest from speaking for somebody else.
    #[test]
    fn a_guests_input_arrives_as_its_own_seats() {
        let mut host = Host::open("x", "carl", 0, false).unwrap();
        let port = host.port();
        let mut anna = Guest::join(("127.0.0.1", port), "anna").unwrap();
        settle(&mut host, &mut [&mut anna]);
        anna.send_input(
            9,
            SeatInput {
                pad: 0x10,
                ..SeatInput::default()
            },
        );
        anna.send_check(8, 0xfeed);
        let (h, _) = settle(&mut host, &mut [&mut anna]);
        assert!(h.iter().any(|e| matches!(
            e,
            Event::Input {
                seat: 1,
                tick: 9,
                input
            } if input.pad == 0x10
        )));
        assert!(h.contains(&Event::Check {
            seat: 1,
            tick: 8,
            hash: 0xfeed
        }));
    }

    #[test]
    fn the_host_closing_reaches_the_guests_screen() {
        let mut host = Host::open("x", "carl", 0, false).unwrap();
        let port = host.port();
        let mut anna = Guest::join(("127.0.0.1", port), "anna").unwrap();
        settle(&mut host, &mut [&mut anna]);
        host.close("the host went to bed");
        let (_, g) = settle(&mut host, &mut [&mut anna]);
        assert!(
            g[0].iter()
                .any(|e| matches!(e, Event::Lost { why, .. } if why.contains("went to bed"))),
            "{:?}",
            g[0]
        );
    }
}
