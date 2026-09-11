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

use crate::later::Later;
use crate::list::Directory;
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
    /// The game begins, on exactly these terms. Which knight each seat plays is
    /// settled after this, on `ChooseKnight`'s own screen, in lockstep.
    Start {
        seats: u8,
        gore: bool,
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
    /// Something worth showing the person that is nobody's fault: what the list
    /// server said, what the router said.
    Note { text: String },
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
    /// The list server, while this game is announced on one.
    pub directory: Option<Directory>,
    /// The announcement, while it is still being made. Connecting to a list
    /// server is a socket to the far side of a country and the frame it is asked
    /// on cannot wait for it, so it is asked on a thread and collected in
    /// [`Host::poll`].
    announcing: Option<Later<Result<Directory, WireError>>>,
    /// Where this game is listed, kept so the listing can be put back after the
    /// connection to the list server breaks.
    listed_on: Option<(String, String)>,
    /// When to try again, once one has broken.
    relist_at: Option<std::time::Instant>,
    /// What each seat's round trip measured, in milliseconds, and when it was
    /// last asked. The input delay is chosen from the worst of them.
    trips: std::collections::BTreeMap<u8, u32>,
    /// When this host started, so a ping carries a small number rather than a
    /// wall clock, and no clock has to be shared with anybody.
    began: std::time::Instant,
    asked: std::time::Instant,
    /// The word a guest has to say, when the host asked for one.
    ///
    /// **It never leaves this machine.** The list carries only the fact that
    /// there is one, and the check happens here, on the hello.
    password: String,
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
            ready: false,
            // Nothing to show until there is a line to measure. See
            // `Player::ms`: what goes there is the game's round trip, the same
            // on every seat, and a lobby with one person in it has no game.
            ms: None,
        });
        Ok(Host {
            door,
            lobby,
            guests: Vec::new(),
            started: false,
            directory: None,
            announcing: None,
            listed_on: None,
            relist_at: None,
            trips: std::collections::BTreeMap::new(),
            began: std::time::Instant::now(),
            asked: std::time::Instant::now(),
            password: String::new(),
        })
    }

    /// The worst round trip measured so far, in milliseconds, or nothing while
    /// nobody has answered yet.
    pub fn worst_trip(&self) -> Option<u32> {
        self.trips.values().copied().max()
    }

    /// What one seat's round trip measured.
    pub fn trip(&self, seat: u8) -> Option<u32> {
        self.trips.get(&seat).copied()
    }

    /// How many ticks of input delay this lobby's measurements ask for.
    ///
    /// The worst round trip in the lobby, because a game runs at the speed of
    /// the peer furthest away. Nothing measured yet falls back to a guess, which
    /// is the one number here that is not a measurement and is marked so.
    pub fn suggested_delay(&self, tick: std::time::Duration) -> u32 {
        // Ours, and a guess: what a domestic line to a nearby machine tends to
        // be. Only used in the first moment of a lobby, before anybody has
        // answered a ping.
        const ASSUMED: std::time::Duration = std::time::Duration::from_millis(80);
        let rtt = match self.worst_trip() {
            Some(ms) => std::time::Duration::from_millis(ms as u64),
            None => ASSUMED,
        };
        crate::lockstep::delay_for_rtt(rtt, tick)
    }

    /// Ask for a word before anybody may join. Empty means anybody may.
    pub fn lock(&mut self, password: &str) {
        self.password = password.trim().to_string();
    }

    /// Whether a word is wanted, which is all the list server is ever told.
    pub fn locked(&self) -> bool {
        !self.password.is_empty()
    }

    /// Put this game on a list server, so it can be found without an address
    /// being read out.
    ///
    /// The server answers with the address it saw the announcement come from and
    /// whether it could get back in, which is the only honest test of whether
    /// friends can reach this machine. A host it could not reach is carried by
    /// the server instead.
    ///
    /// **This returns at once and does not mean the game is listed.** The
    /// connection is made on a thread of its own, the way the router is asked,
    /// and the answer arrives through [`Host::poll`] as an [`Event::Note`]: a
    /// list server that is down takes the whole of `PATIENCE` to say so, and a
    /// lobby that froze for that long while it found out would look broken. Ask
    /// [`Host::listing`] whether the answer is still coming.
    pub fn list_on(&mut self, server: &str, version: &str) {
        self.listed_on = Some((server.to_string(), version.to_string()));
        self.relist_at = None;
        self.announce_now();
    }

    /// Start one announcement on a thread, from what [`Host::list_on`] was told.
    fn announce_now(&mut self) {
        let Some((server, version)) = self.listed_on.clone() else {
            return;
        };
        let (name, port, locked) = (self.lobby.name.clone(), self.door.port(), self.locked());
        self.announcing = Some(Later::start(move || {
            Directory::announce(&server, &name, port, SEATS as u8, locked, &version)
        }));
    }

    /// Whether an announcement is still being made, so the screen can say it is
    /// asking rather than say nothing.
    pub fn listing(&self) -> bool {
        self.announcing.as_ref().is_some_and(|a| a.waiting())
    }

    /// The address to read out to a friend, once the list server has said what
    /// it is.
    pub fn address(&self) -> Option<String> {
        self.directory.as_ref().and_then(|d| d.address())
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
        // The list server first, because it can hand us a guest.
        self.poll_directory(&mut out);
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
        // A ping, now and then, so the input delay is chosen from what the line
        // actually does rather than from an assumption. Once a second is plenty:
        // it is measuring a lobby, not a frame.
        let now = self.began.elapsed().as_millis() as u64;
        let asking = self.asked.elapsed() >= std::time::Duration::from_secs(1);
        if asking {
            self.asked = std::time::Instant::now();
        }
        // Everything said to us.
        let mut lost: Vec<(usize, String)> = Vec::new();
        let mut roster_changed = false;
        let mut trips: Vec<(u8, u32)> = Vec::new();
        for (i, (seat, link)) in self.guests.iter_mut().enumerate() {
            if asking && *seat != SEAT_UNSEATED {
                let _ = link.send(&Msg::Ping { at: now });
            }
            let (msgs, err) = link.poll();
            for m in msgs {
                match m {
                    Msg::Hello {
                        protocol,
                        name,
                        password,
                    } => {
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
                        // The word, checked here and nowhere else. Compared
                        // whole rather than by prefix, and a game with no word
                        // ignores whatever was sent.
                        if !self.password.is_empty() && password != self.password {
                            let _ = link.send(&Msg::Refused {
                                why: if password.is_empty() {
                                    "that game wants a password".into()
                                } else {
                                    "that is not the password".into()
                                },
                            });
                            let _ = link.flush();
                            lost.push((i, "the wrong password".into()));
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
                            ready: false,
                            ms: None,
                        });
                        self.lobby.players.sort_by_key(|p| p.seat);
                        let _ = link.send(&Msg::Welcome {
                            seat: free,
                            lobby: self.lobby.clone(),
                        });
                        out.push(Event::Joined { seat: free, name });
                        roster_changed = true;
                    }
                    Msg::Seated { ready } => {
                        if *seat == SEAT_UNSEATED {
                            continue;
                        }
                        if let Some(p) = self.lobby.players.iter_mut().find(|p| p.seat == *seat) {
                            p.ready = ready;
                            roster_changed = true;
                            out.push(Event::Roster);
                        }
                    }
                    // How far that guest is from the relay, which only that
                    // machine can measure. See `Player::ms`.
                    Msg::Leg { ms } => {
                        if *seat == SEAT_UNSEATED {
                            continue;
                        }
                        if let Some(p) = self.lobby.players.iter_mut().find(|p| p.seat == *seat) {
                            if p.ms != Some(ms) {
                                p.ms = Some(ms);
                                roster_changed = true;
                            }
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
                    // The host's own ping, come back. Only the host reads the
                    // number in it, so no clock is shared.
                    Msg::Pong { at } => {
                        if *seat != SEAT_UNSEATED {
                            trips.push((*seat, now.saturating_sub(at) as u32));
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
        for (seat, ms) in trips {
            // Kept as the latest rather than smoothed: a lobby is measured over
            // seconds and the number is only read once, when the host starts.
            self.trips.insert(seat, ms);
        }
        // The worst line in the game onto the roster, so both ends can see the
        // number the input delay is chosen off. See `Lobby::between`.
        let worst = self.worst_trip();
        if self.lobby.between != worst {
            self.lobby.between = worst;
            roster_changed = true;
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
            self.trips.remove(&seat);
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

    /// The list server: the refresh, whatever it has to say, and any guest it is
    /// holding for us.
    ///
    /// A ticket is a guest waiting at the relay. Answering one means opening a
    /// second connection to the server, which blocks for as long as that takes;
    /// it happens once per person joining a lobby, so a frame is dropped when
    /// somebody arrives and never otherwise.
    fn poll_directory(&mut self, out: &mut Vec<Event>) {
        // The announcement, if one is still being made. It is collected before
        // the directory is read, so the first tick a list server answers on is
        // also the first tick its answer is acted on.
        if let Some(answer) = self.announcing.as_mut().and_then(|a| a.take()) {
            match answer {
                Ok(d) => {
                    self.directory = Some(d);
                    self.relist_at = None;
                }
                Err(e) => {
                    out.push(Event::Note {
                        text: format!("not on the list: {e}"),
                    });
                    // Try again later rather than never. A list server that is
                    // being restarted, or a network that blinked, should not
                    // cost the game its listing for the rest of the evening.
                    self.relist_at = Some(std::time::Instant::now() + RELIST);
                }
            }
            self.announcing = None;
        }
        // A listing whose connection broke is thrown away and made again. The
        // game itself is untouched by this: the lobby is still open on its own
        // port and anybody who has the address can still knock.
        if self.directory.as_ref().is_some_and(|d| d.lost()) {
            if let Some(mut d) = self.directory.take() {
                for text in d.notes() {
                    out.push(Event::Note { text });
                }
            }
            self.relist_at = Some(std::time::Instant::now() + RELIST);
        }
        if self.announcing.is_none() && self.directory.is_none() {
            if let Some(at) = self.relist_at {
                if std::time::Instant::now() >= at {
                    self.relist_at = None;
                    self.announce_now();
                }
            }
        }
        let players = self.lobby.players() as u8;
        let Some(d) = self.directory.as_mut() else {
            return;
        };
        let tickets = d.poll(players);
        for text in d.notes() {
            out.push(Event::Note { text });
        }
        for ticket in tickets {
            match self.directory.as_ref().unwrap().attach(&ticket) {
                Ok(link) => {
                    if self.started || self.lobby.free_seat().is_none() {
                        let mut link = link;
                        let _ = link.send(&Msg::Refused {
                            why: if self.started {
                                "that game has already started".into()
                            } else {
                                "that game is full".into()
                            },
                        });
                        let _ = link.flush();
                        continue;
                    }
                    self.guests.push((SEAT_UNSEATED, link));
                }
                Err(e) => out.push(Event::Note {
                    text: format!("could not let somebody in through the list server: {e}"),
                }),
            }
        }
    }

    /// The host's own leg to the list server, measured the same way a guest
    /// measures its own. See [`Player::ms`].
    pub fn set_leg(&mut self, ms: u32) {
        let Some(p) = self.lobby.players.iter_mut().find(|p| p.seat == 0) else {
            return;
        };
        if p.ms == Some(ms) {
            return;
        }
        p.ms = Some(ms);
        self.broadcast(&Msg::Roster {
            lobby: self.lobby.clone(),
        });
        for (_, link) in self.guests.iter_mut() {
            let _ = link.flush();
        }
    }

    /// The host's own ready flag.
    pub fn seat(&mut self, ready: bool) {
        if let Some(p) = self.lobby.players.iter_mut().find(|p| p.seat == 0) {
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

    /// Begin.
    ///
    /// Returns the terms, which are also what went out on the wire, or nothing
    /// when the lobby is not startable. Knights are not among them: every
    /// machine opens `ChooseKnight` with the same seat count and settles that
    /// there, which is the screen the original settles it on.
    pub fn start(&mut self, delay: u32, check: u32) -> Option<Event> {
        if self.started || !self.lobby.can_start() {
            return None;
        }
        self.started = true;
        self.lobby.started = true;
        let seats = self.lobby.players() as u8;
        let gore = self.lobby.gore;
        let delay = delay.min(u8::MAX as u32) as u8;
        self.broadcast(&Msg::Start {
            seats,
            gore,
            delay,
            check,
        });
        Some(Event::Start {
            seats,
            gore,
            delay,
            check,
        })
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
        // Off the list, so nobody tries to join a game that is over.
        if let Some(d) = self.directory.as_mut() {
            d.withdraw();
        }
        self.directory = None;
    }
}

/// The seat of a socket that has connected and not yet said hello.
const SEAT_UNSEATED: u8 = u8::MAX;

/// How long to wait before putting a lost listing back.
///
/// **Ours**, and picked rather than recovered, like everything in this crate.
/// Long enough that a list server being restarted is not hammered while it comes
/// up, short enough that a friend browsing a minute later still finds the game.
const RELIST: std::time::Duration = std::time::Duration::from_secs(10);

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
    pub fn join(
        addr: impl std::net::ToSocketAddrs,
        player: &str,
        password: &str,
    ) -> Result<Guest, JoinError> {
        Guest::over(Link::connect(addr)?, player, password)
    }

    /// Say hello over a link that is already open.
    ///
    /// What a game reached through the list server's relay uses: the socket was
    /// opened to the server and introduced to the host, and from the hello on it
    /// is an ordinary game link that nobody in the middle reads.
    pub fn over(mut link: Link, player: &str, password: &str) -> Result<Guest, JoinError> {
        link.send(&Msg::Hello {
            protocol: PROTOCOL,
            name: cut(player, PLAYER_NAME_MAX),
            password: password.trim().to_string(),
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
                    delay,
                    check,
                } => {
                    self.started = true;
                    self.lobby.started = true;
                    out.push(Event::Start {
                        seats,
                        gore,
                        delay,
                        check,
                    });
                }
                // Echoed at once and without comment: the host is timing the
                // line, and a guest that thought about it would be timing its
                // own thinking too.
                Msg::Ping { at } => {
                    let _ = self.link.send(&Msg::Pong { at });
                    let _ = self.link.flush();
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

    /// Say whether we are ready.
    pub fn seat_request(&mut self, ready: bool) {
        let _ = self.link.send(&Msg::Seated { ready });
        let _ = self.link.flush();
    }

    /// How far this machine is from the list server. See [`Player::ms`].
    pub fn send_leg(&mut self, ms: u32) {
        let _ = self.link.send(&Msg::Leg { ms });
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
        let mut anna = Guest::join(("127.0.0.1", port), "anna", "").unwrap();
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
        let mut g = Guest::join(("127.0.0.1", port), "anna of the lakelands", "").unwrap();
        settle(&mut host, &mut [&mut g]);
        assert_eq!(host.lobby.players[1].name, "anna of the l");
    }

    #[test]
    fn a_fifth_player_is_told_why_rather_than_dropped() {
        let mut host = Host::open("full", "carl", 0, false).unwrap();
        let port = host.port();
        let mut three: Vec<Guest> = (0..3)
            .map(|i| Guest::join(("127.0.0.1", port), &format!("g{i}"), "").unwrap())
            .collect();
        {
            let mut refs: Vec<&mut Guest> = three.iter_mut().collect();
            settle(&mut host, &mut refs);
        }
        assert_eq!(host.lobby.players(), SEATS);
        let mut late = Guest::join(("127.0.0.1", port), "late", "").unwrap();
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
        let mut anna = Guest::join(("127.0.0.1", port), "anna", "").unwrap();
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
    fn nothing_starts_until_everyone_is_ready_and_then_everyone_is_told() {
        let mut host = Host::open("x", "carl", 0, true).unwrap();
        let port = host.port();
        let mut anna = Guest::join(("127.0.0.1", port), "anna", "").unwrap();
        settle(&mut host, &mut [&mut anna]);
        host.seat(true);
        assert_eq!(host.start(6, 64), None, "anna has not sat down");
        anna.seat_request(true);
        settle(&mut host, &mut [&mut anna]);
        let started = host.start(6, 64).expect("the lobby is ready");
        assert_eq!(
            started,
            Event::Start {
                seats: 2,
                gore: true,
                delay: 6,
                check: 64,
            }
        );
        let (_, g) = settle(&mut host, &mut [&mut anna]);
        assert!(g[0].contains(&started), "{:?}", g[0]);
        assert!(anna.started);
    }

    #[test]
    fn a_game_that_has_started_turns_a_latecomer_away() {
        let mut host = Host::open("x", "carl", 0, false).unwrap();
        let port = host.port();
        host.seat(true);
        host.start(4, 64).expect("one player is a lobby");
        let mut late = Guest::join(("127.0.0.1", port), "late", "").unwrap();
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
            password: String::new(),
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
        let mut anna = Guest::join(("127.0.0.1", port), "anna", "").unwrap();
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

    /// The line is measured rather than assumed, because with a relay in the
    /// path an assumption is not even close.
    #[test]
    fn the_round_trip_is_measured_and_the_delay_follows_it() {
        let mut host = Host::open("x", "carl", 0, false).unwrap();
        let port = host.port();
        let mut anna = Guest::join(("127.0.0.1", port), "anna", "").unwrap();
        assert_eq!(host.worst_trip(), None, "nothing measured yet");
        // The guess, while there is nothing better. Marked as a guess in the
        // code and used for no more than the first second of a lobby.
        let retrace = std::time::Duration::from_micros(14_268);
        assert_eq!(host.suggested_delay(retrace), 6);
        // A ping goes out once a second, so this waits for one.
        let stop = std::time::Instant::now() + std::time::Duration::from_secs(6);
        while std::time::Instant::now() < stop && host.trip(1).is_none() {
            host.poll();
            anna.poll();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let ms = host.trip(1).expect("a measurement");
        // Over loopback this is a millisecond or two, so the delay it asks for
        // is the floor.
        assert!(ms < 500, "a loopback round trip of {ms}ms is not one");
        assert_eq!(host.worst_trip(), Some(ms));
        assert_eq!(host.suggested_delay(retrace), crate::lockstep::MIN_DELAY);
        // And a seat that leaves takes its measurement with it, so a slow guest
        // who has gone does not hold the delay up.
        anna.close("");
        drop(anna);
        let stop = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while std::time::Instant::now() < stop && host.trip(1).is_some() {
            host.poll();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(host.trip(1), None);
    }

    #[test]
    fn the_host_closing_reaches_the_guests_screen() {
        let mut host = Host::open("x", "carl", 0, false).unwrap();
        let port = host.port();
        let mut anna = Guest::join(("127.0.0.1", port), "anna", "").unwrap();
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
