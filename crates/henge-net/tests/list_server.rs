//! The list server, driven for real: the program is started, spoken to over a
//! socket, and stopped.
//!
//! These are here rather than beside the code because the thing under test is a
//! whole program. Cargo builds it and hands us its path in `CARGO_BIN_EXE_*`, so
//! nothing has to be arranged by hand.

use henge_net::list::{self, Directory, ListMsg, LIST_PROTOCOL};
use henge_net::wire::{Link, Listener};
use henge_net::{Guest, Host};
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The server, and the port it settled on. Killed when it goes out of scope, so
/// a failing test does not leave one running.
struct Serving {
    child: Child,
    port: u16,
}

impl Drop for Serving {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Serving {
    /// Start it on a port the system picks, and read back which one that was.
    /// Asking for a fixed port would make two of these fight over it.
    fn start(extra: &[&str]) -> Serving {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_henge-list"));
        cmd.arg("--port").arg("0");
        for a in extra {
            cmd.arg(a);
        }
        let mut child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("henge-list to start");
        let out = child.stdout.take().expect("its output");
        let mut lines = BufReader::new(out).lines();
        let first = lines
            .next()
            .and_then(|l| l.ok())
            .expect("it to say where it is");
        // "henge-list on port 41234, protocol 1, relay on"
        let port: u16 = first
            .split_whitespace()
            .nth(3)
            .and_then(|w| w.trim_end_matches(',').parse().ok())
            .unwrap_or_else(|| panic!("could not read the port out of {first:?}"));
        // Its own output is drained on a thread of its own, or the pipe fills
        // and the server stops in the middle of a test.
        std::thread::spawn(move || {
            for line in lines {
                let Ok(line) = line else { break };
                let _ = line;
            }
        });
        Serving { child, port }
    }

    fn at(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }
}

/// Keep asking until it is true, or give up. Everything here crosses a real
/// socket, so nothing is instant and nothing may be a fixed sleep either.
fn until(what: &str, mut f: impl FnMut() -> bool) {
    let stop = Instant::now() + Duration::from_secs(10);
    while Instant::now() < stop {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("timed out waiting for {what}");
}

/// A game announces itself, a browser finds it, and taking it down takes it off.
#[test]
fn a_game_is_announced_found_and_withdrawn() {
    let server = Serving::start(&[]);
    let mut host = Host::open("CARLS GAME", "carl", 0, true).unwrap();
    host.lock("portcullis");
    host.list_on(&server.at(), "test");

    let mut found = None;
    until("the game to appear on the list", || {
        host.poll();
        let games = list::browse(&server.at(), "test").unwrap_or_default();
        found = games.into_iter().find(|g| g.name == "CARLS GAME");
        found.is_some()
    });
    let g = found.unwrap();
    assert_eq!(g.players, 1);
    assert_eq!(g.seats, 4);
    assert!(g.locked, "it wants a password, and that much is public");
    assert!(g.at.ends_with(&format!(":{}", host.port())));
    // The probe got back in: the host really is listening on that port, and the
    // server measured it rather than believing anybody.
    assert!(!g.relayed(), "a reachable host is not carried");
    // The address the server saw comes back on the announcement's own reply,
    // which is a poll of its own: the game is on the list from the moment the
    // announcement lands, and the host learns where it was seen from just after.
    until("the host to learn its own public address", || {
        host.poll();
        host.address().is_some()
    });
    assert_eq!(host.address().as_deref(), Some(g.at.as_str()));

    host.close("done");
    until("the game to leave the list", || {
        list::browse(&server.at(), "test")
            .unwrap_or_default()
            .iter()
            .all(|g| g.name != "CARLS GAME")
    });
}

/// A build is only offered games its own copy of the game can join.
#[test]
fn a_game_from_another_build_is_not_offered() {
    let server = Serving::start(&[]);
    let mut host = Host::open("OLD GAME", "carl", 0, false).unwrap();
    host.list_on(&server.at(), "an-older-build");
    until("the game to be listed at all", || {
        host.poll();
        !list::browse(&server.at(), "")
            .unwrap_or_default()
            .is_empty()
    });
    let mine = list::browse(&server.at(), "test").unwrap();
    assert!(
        mine.iter().all(|g| g.name != "OLD GAME"),
        "a build should not be offered a game it cannot join"
    );
}

/// The password is checked by the host and by nobody else. The list says only
/// that there is one.
#[test]
fn the_host_checks_the_password_and_the_list_never_sees_it() {
    let server = Serving::start(&[]);
    let mut host = Host::open("LOCKED", "carl", 0, false).unwrap();
    host.lock("portcullis");
    host.list_on(&server.at(), "test");
    let port = host.port();

    // Wrong word: turned away with a reason.
    let mut wrong = Guest::join(("127.0.0.1", port), "anna", "drawbridge").unwrap();
    let mut refused = None;
    until("the wrong word to be refused", || {
        host.poll();
        for e in wrong.poll() {
            if let henge_net::Event::Refused { why } = e {
                refused = Some(why);
            }
        }
        refused.is_some()
    });
    assert!(refused.unwrap().contains("not the password"));
    assert_eq!(host.lobby.players(), 1, "and no seat was spent on it");

    // No word at all: told what is missing rather than left guessing.
    let mut silent = Guest::join(("127.0.0.1", port), "anna", "").unwrap();
    let mut told = None;
    until("a wordless guest to be told why", || {
        host.poll();
        for e in silent.poll() {
            if let henge_net::Event::Refused { why } = e {
                told = Some(why);
            }
        }
        told.is_some()
    });
    assert!(told.unwrap().contains("wants a password"));

    // The right word: in.
    let mut right = Guest::join(("127.0.0.1", port), "anna", "portcullis").unwrap();
    until("the right word to get in", || {
        host.poll();
        right.poll();
        right.seat.is_some()
    });
    assert_eq!(right.seat, Some(1));

    // And the whole conversation with the list server never carried the word.
    let mut games = Vec::new();
    until("the locked game to be on the list", || {
        host.poll();
        games = list::browse(&server.at(), "test").unwrap_or_default();
        games.iter().any(|g| g.name == "LOCKED")
    });
    let json = serde_json::to_string(&games).unwrap();
    assert!(!json.contains("portcullis"));
    assert!(
        games.iter().any(|g| g.locked),
        "the list says a word is wanted, and nothing more: {json}"
    );
}

/// A host the server cannot get back in to is carried by it instead, and the
/// game that goes over that pipe is the ordinary one.
#[test]
fn a_host_that_cannot_be_reached_is_carried() {
    let server = Serving::start(&[]);
    // A port with nothing behind it, so the probe cannot get in. This is what a
    // host behind a router that refuses to forward looks like from outside.
    let dead = {
        let door = Listener::open(0).unwrap();
        let port = door.port();
        drop(door);
        port
    };
    let mut d = Directory::announce(&server.at(), "SHUT IN", dead, 4, false, "test").unwrap();
    until("the server to say it could not get in", || {
        d.poll(1);
        d.reachable() == Some(false)
    });
    // Having been told, the host asks to be carried, and is given a code.
    until("a relay code", || {
        d.poll(1);
        !d.code.is_empty()
    });
    let code = d.code.clone();
    assert!(d.address().unwrap().contains(&code));

    // A guest reaches that code; the server asks the host for its end; the two
    // are glued together and speak the game's own language over the pipe.
    let joined = std::thread::spawn({
        let at = server.at();
        let code = code.clone();
        move || list::reach(&at, &code)
    });
    let mut ticket = None;
    until("the host to be told somebody is waiting", || {
        let t = d.poll(1);
        ticket = t.first().cloned();
        ticket.is_some()
    });
    let mut host_side: Link = d.attach(&ticket.unwrap()).expect("to be introduced");
    let mut guest_side: Link = joined.join().unwrap().expect("to be introduced");

    // The relay carries bytes and does not read them: an ordinary game message
    // goes across untouched.
    let hello = henge_net::Msg::Hello {
        protocol: henge_net::PROTOCOL,
        name: "anna".into(),
        password: String::new(),
    };
    guest_side.send(&hello).unwrap();
    guest_side.flush().unwrap();
    let mut got = Vec::new();
    until("the message to come through the relay", || {
        guest_side.poll();
        let (msgs, _) = host_side.poll();
        got.extend(msgs);
        !got.is_empty()
    });
    assert_eq!(got[0], hello);

    // And the other way, because a relay that only works one way is not one.
    let bye = henge_net::Msg::Bye {
        why: "enough".into(),
    };
    host_side.send(&bye).unwrap();
    host_side.flush().unwrap();
    let mut back = Vec::new();
    until("the answer to come back", || {
        host_side.poll();
        let (msgs, _) = guest_side.poll();
        back.extend(msgs);
        !back.is_empty()
    });
    assert_eq!(back[0], bye);
}

/// A host that *can* be reached is told there is nothing to carry, rather than
/// being handed somebody else's bandwidth for no reason.
#[test]
fn a_reachable_host_is_refused_a_relay() {
    let server = Serving::start(&[]);
    let door = Listener::open(0).unwrap();
    let mut d = Directory::announce(&server.at(), "FINE", door.port(), 4, false, "test").unwrap();
    until("the probe to get in", || {
        d.poll(1);
        d.reachable() == Some(true)
    });
    // Ask anyway, the way a host with an old idea of its own reachability would.
    let mut link: Link<ListMsg> = Link::connect(server.at()).unwrap();
    link.send(&ListMsg::Announce {
        protocol: LIST_PROTOCOL,
        id: String::new(),
        name: "FINE TOO".into(),
        port: door.port(),
        players: 1,
        seats: 4,
        locked: false,
        version: "test".into(),
    })
    .unwrap();
    link.flush().unwrap();
    let mut refused = None;
    until("the server to say no", || {
        let (msgs, _) = link.poll();
        for m in msgs {
            if let ListMsg::Announced(a) = &m {
                assert!(a.reachable);
                link.send(&ListMsg::WantRelay).unwrap();
                link.flush().unwrap();
            }
            if let ListMsg::Refused { why } = m {
                refused = Some(why);
            }
        }
        refused.is_some()
    });
    assert!(refused.unwrap().contains("nothing to carry"));
    assert!(d.code.is_empty());
}

/// `--no-relay` means what it says, for somebody whose bandwidth is not theirs
/// to spend.
#[test]
fn a_list_only_server_carries_nobody() {
    let server = Serving::start(&["--no-relay"]);
    let dead = {
        let door = Listener::open(0).unwrap();
        let port = door.port();
        drop(door);
        port
    };
    let mut d = Directory::announce(&server.at(), "SHUT IN", dead, 4, false, "test").unwrap();
    let mut said = Vec::new();
    until("the server to refuse to carry it", || {
        d.poll(1);
        said.extend(d.notes());
        said.iter().any(|n| n.contains("does not carry games"))
    });
    assert!(d.code.is_empty());
    // The game is still on the list: unreachable is not unlistable, and a friend
    // on the same network can still join it.
    assert!(list::browse(&server.at(), "test")
        .unwrap()
        .iter()
        .any(|g| g.name == "SHUT IN"));
}

/// A peer speaking a different protocol is turned away by name, before it can
/// take a place on anything.
#[test]
fn a_mismatched_protocol_is_refused_by_name() {
    let server = Serving::start(&[]);
    let mut link: Link<ListMsg> = Link::connect(server.at()).unwrap();
    link.send(&ListMsg::Browse {
        protocol: LIST_PROTOCOL + 77,
        version: "test".into(),
    })
    .unwrap();
    link.flush().unwrap();
    let mut refused = None;
    until("the server to refuse", || {
        let (msgs, _) = link.poll();
        for m in msgs {
            if let ListMsg::Refused { why } = m {
                refused = Some(why);
            }
        }
        refused.is_some()
    });
    let why = refused.unwrap();
    assert!(why.contains(&format!("{}", LIST_PROTOCOL + 77)), "{why}");
}

/// The announcement is made on a thread of its own, so the frame it was asked on
/// costs nothing even when the server will never answer.
///
/// This is the whole reason `Host::list_on` returns nothing: a list server that
/// is down takes the full patience to say so, and a lobby that stopped dead for
/// that long while it found out reads as a game that has hung.
#[test]
fn announcing_to_a_server_that_never_answers_does_not_stop_the_game() {
    let mut host = Host::open("PATIENT", "carl", 0, false).unwrap();
    // 203.0.113.0/24 is TEST-NET-3, reserved for documentation and routed
    // nowhere, so this connection can only fail: either at once, or when the
    // patience runs out. Which of the two it is, is not this test's business.
    let began = Instant::now();
    host.list_on("203.0.113.1:19911", "test");
    assert!(
        began.elapsed() < Duration::from_millis(100),
        "list_on held the caller for {:?}",
        began.elapsed()
    );
    assert!(host.listing(), "and it says the answer is still coming");
    assert!(host.directory.is_none());

    // And when the answer does come it is a note, said once.
    let mut said = Vec::new();
    until("the host to be told it is not on the list", || {
        for e in host.poll() {
            if let henge_net::Event::Note { text } = e {
                said.push(text);
            }
        }
        !host.listing()
    });
    assert!(
        said.iter().any(|n| n.contains("not on the list")),
        "{said:?}"
    );
    assert!(host.directory.is_none());
}

/// A game whose host keeps refreshing stays on the list, and the host's
/// connection is not closed under it.
///
/// This is the regression test for the bug that took every game off the list
/// three quarters of a minute after it was opened. A host refreshes by sending
/// `Announce` down the control connection it already has, and the server was
/// only ever reading `WantRelay` and `Withdraw` off that connection, so the
/// entry's own clock never moved and `expire` dropped it. What the person saw
/// was their game vanishing from the browser and their lobby saying it had lost
/// the list server.
///
/// The server is started with `--stale 1` so the test is over in seconds rather
/// than in the default forty five.
#[test]
fn a_refreshed_game_is_not_dropped_off_the_list() {
    let server = Serving::start(&["--stale", "1"]);
    let mut host = Host::open("STAYING", "carl", 0, false).unwrap();
    host.list_on(&server.at(), "test");
    until("the game to be listed", || {
        host.poll();
        list::browse(&server.at(), "test")
            .unwrap_or_default()
            .iter()
            .any(|g| g.name == "STAYING")
    });

    // Well past the point where an unrefreshed game would have been dropped.
    // The refresh is sent by hand rather than waited for: `Directory::poll`
    // sends one every fifteen seconds and a test should not take that long.
    let until_then = Instant::now() + Duration::from_secs(3);
    while Instant::now() < until_then {
        host.poll();
        if let Some(d) = host.directory.as_mut() {
            d.refresh(1);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        list::browse(&server.at(), "test")
            .unwrap_or_default()
            .iter()
            .any(|g| g.name == "STAYING"),
        "a game whose host is still there should still be on the list"
    );
    assert!(
        host.directory.is_some(),
        "and its connection to the list server should still be open"
    );
}

/// Two different measurements, and both reach both screens.
///
/// `Player::ms` is one machine's own leg to the list server, which is also the
/// relay: only that machine can measure it, so each one does and tells the host.
/// `Lobby::between` is the whole path between two players, which only the host
/// can measure, and is what the input delay is chosen off.
#[test]
fn the_roster_carries_each_leg_and_the_line_between() {
    let server = Serving::start(&[]);
    let mut host = Host::open("MEASURED", "carl", 0, false).unwrap();
    let port = host.port();
    assert_eq!(
        host.lobby.seated(0).and_then(|p| p.ms),
        None,
        "nothing is claimed before anything is measured"
    );
    assert_eq!(host.lobby.between, None);

    let mut guest = Guest::join(("127.0.0.1", port), "anna", "").unwrap();
    until("the guest to sit down", || {
        host.poll();
        guest.poll();
        guest.seat == Some(1)
    });

    // Each end measures its own distance to the server and says so, which is
    // what the game does for itself once a lobby is up.
    let mine = list::sound(&server.at()).expect("the host's own leg");
    host.set_leg(mine);
    let theirs = list::sound(&server.at()).expect("the guest's own leg");
    guest.send_leg(theirs);

    until(
        "both legs and the line between to reach both screens",
        || {
            host.poll();
            guest.poll();
            let legs = |l: &henge_net::Lobby| {
                (
                    l.seated(0).and_then(|p| p.ms),
                    l.seated(1).and_then(|p| p.ms),
                    l.between,
                )
            };
            let h = legs(&host.lobby);
            // The line between takes a ping and its answer, so it lands about a
            // second after the two legs do.
            h.2.is_some()
                && h == (Some(mine), Some(theirs), host.worst_trip())
                && h == legs(&guest.lobby)
        },
    );
    assert!(
        host.lobby.between.is_some(),
        "the host measured the line between the two of them"
    );
}

/// A machine can measure how far it is from the list server, which is the leg
/// that matters when the server is carrying the game.
#[test]
fn a_machine_can_sound_the_distance_to_the_server() {
    let server = Serving::start(&[]);
    // Twice, because the socket is opened and closed each time and a second one
    // must work as well as the first.
    for _ in 0..2 {
        let ms = list::sound(&server.at()).expect("an answer");
        assert!(ms < 2000, "a loopback round trip should be quick: {ms}ms");
    }
    // And it does not disturb anything else the server is doing.
    let mut host = Host::open("STILL HERE", "carl", 0, false).unwrap();
    host.list_on(&server.at(), "test");
    until("the game to be listed", || {
        host.poll();
        list::browse(&server.at(), "test")
            .unwrap_or_default()
            .iter()
            .any(|g| g.name == "STILL HERE")
    });
    list::sound(&server.at()).expect("an answer with a game on the list");
}
