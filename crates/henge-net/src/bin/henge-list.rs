//! `henge-list`: the list server, and the relay behind it.
//!
//! **Ours**, and deliberately the least clever program in this repository. It
//! runs on any machine with one port open to the internet, a Raspberry Pi
//! included, and it does two things:
//!
//! - **The list.** Hosts announce their lobby and refresh it; browsers ask what
//!   games there are. It holds a name, an address, a head count and a flag
//!   saying whether a password is wanted. It never holds the password, so it
//!   cannot be made to let anybody into anything.
//! - **The relay.** A host whose router will not open a port keeps one outbound
//!   connection here. When a guest arrives, this asks the host for a second
//!   one, glues the two sockets together and copies bytes between them until one
//!   of them goes. **It never parses what it carries.**
//!
//! It keeps no game state, it cannot join a game, and losing it costs the browse
//! and the relay and nothing else: a game found by typing an address plays
//! exactly the same.
//!
//! ### The reachability probe, which is the part worth having
//!
//! An announcement arrives on a TCP connection, so this program already knows
//! the host's real public address: it is the source address of that connection,
//! and no router has to be believed about it. It then tries to connect *back* to
//! the announced port. That is the only honest answer to "can my friends reach
//! me", and a host that comes back unreachable is offered the relay.
//!
//! It measures a connection from *this* machine, which is the same question a
//! friend is asking only if this machine is on the far side of the host's
//! router. So a list server belongs somewhere out on the internet, not on the
//! same home network as the people using it.
//!
//! ### Running it
//!
//! ```text
//! henge-list                     # port 19911, relay on
//! henge-list --port 25000        # somewhere else
//! henge-list --no-relay          # list only, if the bandwidth is not yours to spend
//! henge-list --quiet             # no line per event
//! ```
//!
//! Forward that one TCP port to the machine and nothing else. There is no
//! database, no configuration file and nothing written to disk: everything it
//! knows is in memory, and the hosts rebuild it themselves within one refresh of
//! a restart.
//!
//! ### What it will not do
//!
//! Every limit below is a refusal with a reason rather than a silent drop, and
//! every one of them exists because the address of this program will eventually
//! be known to people who were not invited:
//!
//! - [`MAX_GAMES`] games at once, and [`MAX_PER_HOST`] from any one address, so
//!   nobody fills the list from one machine.
//! - [`MAX_PIPES`] carried games at once, and a relay is only given to a game
//!   that is on the list **and was measured as unreachable**. A host that can be
//!   reached and asks to be carried anyway is asking for somebody else's
//!   bandwidth for no reason, and is told so.
//! - [`BYTES_A_SECOND`] through any one carried game, which is three times what a
//!   four-seat game at the retrace rate actually uses, and [`TOTAL_A_SECOND`]
//!   through all of them together.
//! - A connection that says nothing for [`SILENCE`] is closed.

use henge_net::list::{Announced, ListMsg, Listing, LIST_PROTOCOL, PATIENCE, STALE};
use henge_net::wire::{Link, Listener, MAX_FRAME};
use std::collections::BTreeMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

/// How many games may be on the list at once.
const MAX_GAMES: usize = 500;

/// How many of those may come from one address. A house with four people in it
/// hosting four games is plausible; forty is not.
const MAX_PER_HOST: usize = 8;

/// How many carried games may run at once.
const MAX_PIPES: usize = 64;

/// How many sockets may be waiting to say what they are.
const MAX_PENDING: usize = 256;

/// What one carried game may pass, in each direction, a second.
///
/// A four-seat game at the 70.0863 Hz retrace sends about seventy small frames a
/// second each way, which is under seven kilobytes. Twenty is three times that,
/// and sixty-four of them still fit down a domestic line.
const BYTES_A_SECOND: usize = 20 * 1024;

/// What every carried game together may pass, in each direction, a second.
const TOTAL_A_SECOND: usize = 1024 * 1024;

/// A connection that has said nothing at all for this long is closed. A host's
/// control connection refreshes every fifteen seconds, so this is generous.
const SILENCE: Duration = Duration::from_secs(90);

/// How long a guest waits to be introduced before it is told the host did not
/// answer.
const INTRODUCTION: Duration = Duration::from_secs(10);

/// One turn of the loop. Far below the rate anything here changes at, and it
/// keeps the program off the processor when nothing is happening.
const REST: Duration = Duration::from_millis(2);

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("henge-list: the Moonstone game list, and the relay behind it");
        println!();
        println!(
            "  --port <n>    listen here (default {})",
            henge_net::DEFAULT_LIST_PORT
        );
        println!("  --no-relay    hold the list only, and carry nobody's game");
        println!("  --quiet       do not print a line per event");
        println!(
            "  --stale <s>   drop a game whose host has not refreshed for this long (default {})",
            STALE.as_secs()
        );
        println!();
        println!("Forward that one TCP port and nothing else. No files, no database.");
        return;
    }
    let port = after(&args, "--port")
        .and_then(|v| v.parse().ok())
        .unwrap_or(henge_net::DEFAULT_LIST_PORT);
    let relaying = !args.iter().any(|a| a == "--no-relay");
    let loud = !args.iter().any(|a| a == "--quiet");
    // How long a listing outlives its host's last refresh. A knob rather than a
    // constant because a test cannot wait three quarters of a minute to prove
    // that a refreshed game is not dropped, and because a server on a bad line
    // may want to be more forgiving than the default.
    let stale = after(&args, "--stale")
        .and_then(|v| v.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(STALE);

    let door = match Listener::open(port) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("cannot listen on port {port}: {e}");
            std::process::exit(1);
        }
    };
    println!(
        "henge-list on port {}, protocol {LIST_PROTOCOL}, relay {}",
        door.port(),
        if relaying { "on" } else { "off" }
    );

    let mut server = Server {
        games: BTreeMap::new(),
        pending: Vec::new(),
        hosts: BTreeMap::new(),
        waiting: Vec::new(),
        pipes: Vec::new(),
        next: 1,
        relaying,
        loud,
        stale,
        budget: Budget::new(),
    };

    loop {
        for link in door.accept::<ListMsg>() {
            if server.pending.len() >= MAX_PENDING {
                continue;
            }
            server.pending.push(Pending {
                link,
                since: Instant::now(),
            });
        }
        server.read_pending();
        server.read_hosts();
        server.pump_pipes();
        server.expire();
        std::thread::sleep(REST);
    }
}

fn after(args: &[String], flag: &str) -> Option<String> {
    let at = args.iter().position(|a| a == flag)?;
    args.get(at + 1).cloned()
}

/// A socket that has connected and not yet said what it is for.
struct Pending {
    link: Link<ListMsg>,
    since: Instant,
}

/// A game on the list.
struct Game {
    listing: Listing,
    /// The address the announcement came from, which is the one address here
    /// that is not somebody's guess.
    from: IpAddr,
    seen: Instant,
    /// Whether the probe got back in.
    reachable: bool,
    /// The relay code, once one has been handed out.
    code: String,
}

/// A host's control connection, kept open so a waiting guest can be announced on
/// it, and so that its closing says the game is over.
struct HostLink {
    link: Link<ListMsg>,
    seen: Instant,
}

/// A guest that has asked for a code and is waiting for its host to answer.
struct Waiting {
    link: Link<ListMsg>,
    ticket: String,
    since: Instant,
}

/// Two sockets glued together. From here on this program is a pipe, and knows
/// nothing whatever about what is in it.
struct Pipe {
    a: TcpStream,
    b: TcpStream,
    /// Whatever the far end would not take yet, kept in order.
    to_b: Vec<u8>,
    to_a: Vec<u8>,
    spent: usize,
    since: Instant,
}

/// The rate limit, as a bucket that refills once a second.
struct Budget {
    left: usize,
    filled: Instant,
}

impl Budget {
    fn new() -> Budget {
        Budget {
            left: TOTAL_A_SECOND,
            filled: Instant::now(),
        }
    }

    fn tick(&mut self) {
        if self.filled.elapsed() >= Duration::from_secs(1) {
            self.filled = Instant::now();
            self.left = TOTAL_A_SECOND;
        }
    }

    fn take(&mut self, want: usize) -> usize {
        let got = want.min(self.left);
        self.left -= got;
        got
    }
}

struct Server {
    games: BTreeMap<String, Game>,
    pending: Vec<Pending>,
    hosts: BTreeMap<String, HostLink>,
    waiting: Vec<Waiting>,
    pipes: Vec<Pipe>,
    next: u64,
    relaying: bool,
    loud: bool,
    /// How long a listing outlives its host's last refresh: `--stale`.
    stale: Duration,
    budget: Budget,
}

impl Server {
    fn say(&self, line: impl AsRef<str>) {
        if self.loud {
            println!("{}", line.as_ref());
        }
    }

    /// A name nobody has to read out and nobody can guess the next of. The
    /// counter is never reused, so two are never the same.
    fn name(&mut self, prefix: &str) -> String {
        self.next += 1;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        format!("{prefix}{:x}{:x}", self.next, now)
    }

    /// Whatever the sockets that have not said what they are have to say.
    ///
    /// A stranger's **first** message decides what the socket is for, and the
    /// socket then stops being pending, so this reads one message and no more.
    /// The link is taken out of the list and handed on by value: removing by
    /// index inside a loop over the same list is how a program like this closes
    /// the wrong person's connection.
    fn read_pending(&mut self) {
        let mut said: Vec<(usize, Option<ListMsg>)> = Vec::new();
        for (i, p) in self.pending.iter_mut().enumerate() {
            let (mut msgs, err) = p.link.poll();
            if !msgs.is_empty() {
                said.push((i, Some(msgs.remove(0))));
            } else if err.is_some() || p.since.elapsed() > SILENCE {
                said.push((i, None));
            }
        }
        // Highest index first, so the ones below keep their places.
        for (i, m) in said.into_iter().rev() {
            let link = self.pending.remove(i).link;
            let Some(m) = m else { continue };
            self.dispatch(link, m);
        }
    }

    /// What one socket's opening message means.
    fn dispatch(&mut self, mut link: Link<ListMsg>, m: ListMsg) {
        match m {
            ListMsg::Announce {
                protocol,
                id,
                name,
                port,
                players,
                seats,
                locked,
                version,
            } => {
                if protocol != LIST_PROTOCOL {
                    refuse(&mut link, wrong_protocol(protocol));
                    return;
                }
                self.announce(link, id, name, port, players, seats, locked, version);
            }
            ListMsg::Browse { protocol, version } => {
                if protocol != LIST_PROTOCOL {
                    refuse(&mut link, wrong_protocol(protocol));
                    return;
                }
                // A build is only offered games its own copy of the game can
                // join. An empty version asks for all of them, which is what a
                // tool does and not what a player does.
                let games: Vec<Listing> = self
                    .games
                    .values()
                    .filter(|g| version.is_empty() || g.listing.version == version)
                    .map(|g| g.listing.clone())
                    .collect();
                let _ = link.send(&ListMsg::Games { games });
                let _ = link.flush();
            }
            // One frame out, one frame back, and the socket is done with. It is
            // how a player measures its own leg to this machine, which is the
            // leg that matters when this machine is carrying their game.
            ListMsg::Sound => {
                let _ = link.send(&ListMsg::Sounded);
                let _ = link.flush();
            }
            ListMsg::Reach { code } => self.reach(link, &code),
            ListMsg::Attach { ticket } => self.attach(link, &ticket),
            ListMsg::Withdraw { id } => {
                // Only from the address that put it there.
                let from = link.peer().ip();
                if self.games.get(&id).map(|g| g.from) == Some(from) {
                    self.games.remove(&id);
                    self.hosts.remove(&id);
                    self.say(format!("withdrawn {id}"));
                }
            }
            // Nothing else is a thing a stranger says first.
            _ => refuse(&mut link, "that is not how a conversation starts".into()),
        }
    }

    /// Put a game on the list, or refresh one already there, and say what the
    /// world can see of it.
    ///
    /// The socket becomes the host's control connection.
    #[allow(clippy::too_many_arguments)]
    fn announce(
        &mut self,
        mut link: Link<ListMsg>,
        id: String,
        name: String,
        port: u16,
        players: u8,
        seats: u8,
        locked: bool,
        version: String,
    ) {
        let from = link.peer().ip();
        // A refresh keeps its entry, its code and its probe result: the probe is
        // the expensive part, and its answer does not change while a lobby is
        // open.
        if !id.is_empty() {
            if let Some(g) = self.games.get_mut(&id) {
                if g.from != from {
                    refuse(&mut link, "that game belongs to somebody else".into());
                    return;
                }
                g.listing.players = players;
                g.listing.name = name;
                g.listing.locked = locked;
                g.seen = Instant::now();
                let told = Announced {
                    id: id.clone(),
                    seen_as: g.listing.at.clone(),
                    reachable: g.reachable,
                };
                let code = g.code.clone();
                let _ = link.send(&ListMsg::Announced(told));
                if !code.is_empty() {
                    let _ = link.send(&ListMsg::Relayed { code });
                }
                let _ = link.flush();
                // The newest connection replaces the old control one, because it
                // is the one we know is alive.
                self.hosts.insert(
                    id,
                    HostLink {
                        link,
                        seen: Instant::now(),
                    },
                );
                return;
            }
        }
        if self.games.len() >= MAX_GAMES {
            refuse(&mut link, "this list is full".into());
            return;
        }
        if self.games.values().filter(|g| g.from == from).count() >= MAX_PER_HOST {
            refuse(
                &mut link,
                "that machine already has enough games on this list".into(),
            );
            return;
        }
        let at = SocketAddr::new(from, port).to_string();
        // **The probe.** See the note at the top: this is the whole reason a
        // host does not have to trust its router.
        let reachable = reachable(from, port);
        let id = self.name("g");
        self.games.insert(
            id.clone(),
            Game {
                listing: Listing {
                    id: id.clone(),
                    name: name.clone(),
                    at: at.clone(),
                    code: String::new(),
                    players,
                    seats,
                    locked,
                    version,
                },
                from,
                seen: Instant::now(),
                reachable,
                code: String::new(),
            },
        );
        let _ = link.send(&ListMsg::Announced(Announced {
            id: id.clone(),
            seen_as: at.clone(),
            reachable,
        }));
        let _ = link.flush();
        self.hosts.insert(
            id.clone(),
            HostLink {
                link,
                seen: Instant::now(),
            },
        );
        self.say(format!(
            "listed {id} \"{name}\" at {at}, {}",
            if reachable {
                "reachable"
            } else {
                "not reachable from here"
            }
        ));
    }

    /// The hosts' control connections: a refresh, a request to be carried, or a
    /// goodbye.
    ///
    /// **The refresh is the important one.** A host keeps its listing alive by
    /// sending [`ListMsg::Announce`] every fifteen seconds down the connection it
    /// already has, and [`Serving::expire`] drops any game whose entry has not
    /// been touched for [`STALE`]. This arm is what touches it. Without it every
    /// game fell off the list forty five seconds after it was opened, and the
    /// host saw its connection closed a moment later, which is what
    /// `Directory::poll` reports as having lost the list server.
    ///
    /// The reply matters too, and not only for tidiness: it is the only thing
    /// this server ever sends down an idle host connection, and a host behind a
    /// carrier's NAT needs traffic coming back or the mapping is dropped from
    /// under it.
    fn read_hosts(&mut self) {
        let ids: Vec<String> = self.hosts.keys().cloned().collect();
        for id in ids {
            let Some(h) = self.hosts.get_mut(&id) else {
                continue;
            };
            let (msgs, err) = h.link.poll();
            let mut want_relay = false;
            let mut withdraw = false;
            let mut refresh = None;
            for m in msgs {
                h.seen = Instant::now();
                match m {
                    ListMsg::WantRelay => want_relay = true,
                    ListMsg::Withdraw { .. } => withdraw = true,
                    ListMsg::Announce {
                        name,
                        players,
                        locked,
                        ..
                    } => refresh = Some((name, players, locked)),
                    _ => {}
                }
            }
            if withdraw || err.is_some() || h.seen.elapsed() > SILENCE {
                self.hosts.remove(&id);
                self.games.remove(&id);
                self.say(format!("gone {id}"));
                continue;
            }
            if let Some((name, players, locked)) = refresh {
                let told = self.games.get_mut(&id).map(|g| {
                    g.listing.name = name;
                    g.listing.players = players;
                    g.listing.locked = locked;
                    g.seen = Instant::now();
                    (
                        Announced {
                            id: id.clone(),
                            seen_as: g.listing.at.clone(),
                            reachable: g.reachable,
                        },
                        g.code.clone(),
                    )
                });
                if let (Some((told, code)), Some(h)) = (told, self.hosts.get_mut(&id)) {
                    let _ = h.link.send(&ListMsg::Announced(told));
                    if !code.is_empty() {
                        let _ = h.link.send(&ListMsg::Relayed { code });
                    }
                    let _ = h.link.flush();
                }
            }
            if want_relay {
                self.give_relay(&id);
            }
        }
    }

    /// Hand a host a relay code, if it has earned one.
    fn give_relay(&mut self, id: &str) {
        let why = if !self.relaying {
            Some("this list server does not carry games".to_string())
        } else if self.games.get(id).is_some_and(|g| g.reachable) {
            Some("your port answered from out here, so there is nothing to carry".to_string())
        } else if self.pipes.len() >= MAX_PIPES {
            Some("this list server is carrying as many games as it can".to_string())
        } else {
            None
        };
        if let Some(why) = why {
            if let Some(h) = self.hosts.get_mut(id) {
                refuse(&mut h.link, why);
            }
            return;
        }
        if self.games.get(id).is_none_or(|g| !g.code.is_empty()) {
            return;
        }
        let code = self.name("");
        if let Some(g) = self.games.get_mut(id) {
            g.code = code.clone();
            g.listing.code = code.clone();
        }
        if let Some(h) = self.hosts.get_mut(id) {
            let _ = h.link.send(&ListMsg::Relayed { code: code.clone() });
            let _ = h.link.flush();
        }
        self.say(format!("carrying {id} as {code}"));
    }

    /// A guest asking to be introduced to a code.
    fn reach(&mut self, mut link: Link<ListMsg>, code: &str) {
        let Some(id) = self
            .games
            .iter()
            .find(|(_, g)| !code.is_empty() && g.code == code)
            .map(|(id, _)| id.clone())
        else {
            refuse(
                &mut link,
                "no game here is being carried under that code".into(),
            );
            return;
        };
        if self.pipes.len() >= MAX_PIPES {
            refuse(
                &mut link,
                "this list server is carrying as many games as it can".into(),
            );
            return;
        }
        let ticket = self.name("t");
        let Some(h) = self.hosts.get_mut(&id) else {
            refuse(&mut link, "that game's host is not connected".into());
            return;
        };
        let _ = h.link.send(&ListMsg::Waiting {
            ticket: ticket.clone(),
        });
        let _ = h.link.flush();
        self.waiting.push(Waiting {
            link,
            ticket,
            since: Instant::now(),
        });
    }

    /// The host's second connection, arriving to be glued to a waiting guest.
    fn attach(&mut self, mut link: Link<ListMsg>, ticket: &str) {
        let Some(at) = self.waiting.iter().position(|w| w.ticket == ticket) else {
            refuse(&mut link, "nobody is waiting under that ticket".into());
            return;
        };
        let mut guest = self.waiting.remove(at);
        // Both ends are told, in the language they have been speaking, that this
        // is the last thing this program will ever say to them.
        let _ = guest.link.send(&ListMsg::Open);
        let _ = guest.link.flush();
        let _ = link.send(&ListMsg::Open);
        let _ = link.flush();
        let (a, to_b) = link.into_raw();
        let (b, to_a) = guest.link.into_raw();
        self.say(format!("introduced {ticket}"));
        self.pipes.push(Pipe {
            a,
            b,
            to_b,
            to_a,
            spent: 0,
            since: Instant::now(),
        });
    }

    /// Copy bytes both ways, within the budget. This is the whole of the relay.
    fn pump_pipes(&mut self) {
        self.budget.tick();
        let per_turn = ((BYTES_A_SECOND as f64) * REST.as_secs_f64()).ceil() as usize;
        let mut dead: Vec<usize> = Vec::new();
        for (i, p) in self.pipes.iter_mut().enumerate() {
            // The per-game allowance refills on its own second, so one busy game
            // cannot spend the next one's.
            if p.since.elapsed() >= Duration::from_secs(1) {
                p.since = Instant::now();
                p.spent = 0;
            }
            let want = BYTES_A_SECOND.saturating_sub(p.spent).min(per_turn.max(1));
            let room = self.budget.take(want);
            let mut moved = 0;
            let alive = shovel(&mut p.a, &mut p.b, &mut p.to_b, room, &mut moved)
                & shovel(&mut p.b, &mut p.a, &mut p.to_a, room, &mut moved);
            p.spent += moved;
            if !alive {
                dead.push(i);
            }
        }
        for i in dead.into_iter().rev() {
            self.pipes.remove(i);
            self.say("a carried game ended");
        }
    }

    /// Drop what has gone quiet.
    fn expire(&mut self) {
        let stale: Vec<String> = self
            .games
            .iter()
            .filter(|(_, g)| g.seen.elapsed() > self.stale)
            .map(|(id, _)| id.clone())
            .collect();
        for id in stale {
            self.games.remove(&id);
            self.hosts.remove(&id);
            self.say(format!("expired {id}"));
        }
        let mut i = 0;
        while i < self.waiting.len() {
            if self.waiting[i].since.elapsed() > INTRODUCTION {
                let mut w = self.waiting.remove(i);
                refuse(&mut w.link, "that game's host did not answer".into());
            } else {
                i += 1;
            }
        }
    }
}

fn wrong_protocol(theirs: u32) -> String {
    format!("this list speaks protocol {LIST_PROTOCOL} and yours speaks {theirs}")
}

/// Say no, with a sentence a person can act on, and then let the socket go.
fn refuse(link: &mut Link<ListMsg>, why: String) {
    let _ = link.send(&ListMsg::Refused { why });
    let _ = link.flush();
}

/// Try to open a connection back to a host that has just announced itself.
fn reachable(ip: IpAddr, port: u16) -> bool {
    if port == 0 {
        return false;
    }
    TcpStream::connect_timeout(&SocketAddr::new(ip, port), PATIENCE).is_ok()
}

/// Move up to `room` bytes from one socket to another, keeping whatever the far
/// end would not take. Returns whether the pair is still alive.
fn shovel(
    from: &mut TcpStream,
    to: &mut TcpStream,
    held: &mut Vec<u8>,
    room: usize,
    moved: &mut usize,
) -> bool {
    // Whatever is already held goes first, so nothing is ever reordered.
    if !held.is_empty() {
        match to.write(held) {
            Ok(0) => return false,
            Ok(n) => {
                held.drain(..n);
                *moved += n;
            }
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => return true,
            Err(ref e) if e.kind() == ErrorKind::Interrupted => return true,
            Err(_) => return false,
        }
    }
    if held.len() > MAX_FRAME * 4 {
        // The far end is not reading and the buffer is running away. Better to
        // end one game than to hold somebody else's memory.
        return false;
    }
    if room == 0 {
        return true;
    }
    let mut chunk = vec![0u8; room.min(16 * 1024)];
    match from.read(&mut chunk) {
        Ok(0) => false,
        Ok(n) => {
            chunk.truncate(n);
            *moved += n;
            match to.write(&chunk) {
                Ok(0) => false,
                Ok(w) => {
                    if w < n {
                        held.extend_from_slice(&chunk[w..]);
                    }
                    true
                }
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                    held.extend_from_slice(&chunk);
                    true
                }
                Err(ref e) if e.kind() == ErrorKind::Interrupted => {
                    held.extend_from_slice(&chunk);
                    true
                }
                Err(_) => false,
            }
        }
        Err(ref e) if e.kind() == ErrorKind::WouldBlock => true,
        Err(ref e) if e.kind() == ErrorKind::Interrupted => true,
        Err(_) => false,
    }
}
