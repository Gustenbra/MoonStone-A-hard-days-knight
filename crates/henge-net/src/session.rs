//! A game in progress: the wire and the scheduler joined up.
//!
//! [`crate::lobby`] gets four people into seats and [`crate::lockstep`] decides
//! when a tick may run. This is the small amount of glue between them, and it is
//! here rather than in the renderer so that the relay rule lives in one place:
//!
//! - **A guest sends its own seat's input to the host and runs nothing until the
//!   host sends the tick back.** It never hears another guest directly.
//! - **The host collects, and the moment a tick is complete it broadcasts the
//!   whole tick and runs it.** So every machine runs tick *T* with the same four
//!   words in it, and the host is the only machine that needs a reachable port.
//!
//! That is one hop of latency for a guest and none for the host, which is the
//! price of not asking three people to forward a port. The fingerprint check
//! rides along: each machine hashes its own state before the same tick and the
//! host compares, so a divergence is caught within [`Lockstep::check`] ticks of
//! happening rather than twenty minutes later when somebody notices the gold is
//! wrong.

use crate::lobby::{Event, Guest, Host};
use crate::lockstep::{Lockstep, Turn};
use crate::proto::SeatInput;

/// Which end of the wire this machine is.
///
/// A host carries a listener, a roster and possibly a list-server connection and
/// a guest carries one socket, so the two halves are nothing like the same size.
/// Boxing it would put a pointer chase on the hot path of every tick to save a
/// few hundred bytes that exist once per game.
#[allow(clippy::large_enum_variant)]
pub enum Side {
    Host(Host),
    Guest(Guest),
}

/// A running game.
pub struct Session {
    pub side: Side,
    pub step: Lockstep,
    /// Lines for the person: somebody left, the router said something, the
    /// machines diverged.
    notes: Vec<String>,
    /// Why the game stopped being a game, once it has.
    over: Option<String>,
}

impl Session {
    /// Start hosting a game that is already in seats. The host is seat zero.
    pub fn host(host: Host, seats: usize, delay: u32, check: u32) -> Session {
        Session {
            side: Side::Host(host),
            step: Lockstep::new(seats, 0, delay, check),
            notes: Vec::new(),
            over: None,
        }
    }

    /// Join a game that has started, in the seat the host gave.
    pub fn guest(guest: Guest, seats: usize, mine: usize, delay: u32, check: u32) -> Session {
        Session {
            side: Side::Guest(guest),
            step: Lockstep::new(seats, mine, delay, check),
            notes: Vec::new(),
            over: None,
        }
    }

    /// Which seat this machine's player is in.
    pub fn seat(&self) -> usize {
        self.step.mine
    }

    pub fn hosting(&self) -> bool {
        matches!(self.side, Side::Host(_))
    }

    /// Read the wire. Call once per turn of the loop, before [`Session::advance`].
    pub fn poll(&mut self) {
        let events = match &mut self.side {
            Side::Host(h) => h.poll(),
            Side::Guest(g) => g.poll(),
        };
        for e in events {
            match e {
                // A guest's input, which is the host's whole job here.
                Event::Input { seat, tick, input } => {
                    self.step.put(tick, seat as usize, input);
                }
                // A complete tick from the host, which is a guest's permission
                // to run it.
                Event::Turn { tick, seats } => {
                    for (seat, input) in seats.into_iter().enumerate() {
                        self.step.put(tick, seat, input);
                    }
                }
                Event::Check { seat, tick, hash } => {
                    self.step.heard(tick, seat as usize, hash);
                }
                Event::Desync { tick, hashes } => {
                    self.step.told_desync(tick, hashes);
                }
                Event::Left { seat, name } => {
                    // A seat emptying mid-game stops the game: its input is
                    // never going to arrive, and running without it would mean
                    // inventing what they pressed.
                    let who = if name.is_empty() {
                        format!("seat {seat}")
                    } else {
                        name
                    };
                    self.end(format!("{who} left the game"));
                }
                Event::Lost { seat, why } => {
                    if self.hosting() {
                        self.notes.push(format!("seat {seat}: {why}"));
                    } else {
                        self.end(why);
                    }
                }
                // Worth showing, and nothing to do about: the list server said
                // something while a game was running.
                Event::Note { text } => self.notes.push(text),
                // Nothing in the lobby's half matters once a game is running,
                // and a host that sends one says so in the log.
                Event::Joined { .. }
                | Event::Roster
                | Event::Seated { .. }
                | Event::Refused { .. }
                | Event::Start { .. } => {}
            }
        }
        if let Some(d) = self.step.desync().cloned() {
            if self.over.is_none() {
                // The host is the one that compares, so it is the one that tells
                // everybody. A guest has already been told, or worked it out.
                if let Side::Host(h) = &mut self.side {
                    let hashes = d.hashes.iter().map(|h| h.unwrap_or(0)).collect();
                    h.send_desync(d.tick, hashes);
                }
                self.end(format!("the machines stopped agreeing at {d}"));
            }
        }
    }

    /// Offer this machine's input and take a tick if one is ready.
    ///
    /// `hash` is asked for the fingerprint of the state **before** the tick that
    /// is about to run, and only on the ticks a check falls on, so a caller can
    /// make it as expensive as it likes.
    ///
    /// Returns the tick's input for every seat, which the caller applies and then
    /// steps the game once. Nothing means the tick is not all in yet: draw, and
    /// ask again.
    pub fn advance(
        &mut self,
        local: SeatInput,
        hash: &mut dyn FnMut() -> u64,
    ) -> Option<(u32, Turn)> {
        if self.over.is_some() {
            return None;
        }
        // Our own input, for the tick it belongs to, and on the wire if there is
        // a host to send it to.
        if let Some(at) = self.step.schedule(local) {
            if let Side::Guest(g) = &mut self.side {
                g.send_input(at, local);
            }
        }
        let tick = self.step.tick();
        // The fingerprint, before the tick rather than after it, so that every
        // machine hashes the same moment.
        if self.step.check_due() {
            let h = hash();
            self.step.note(tick, h);
            match &mut self.side {
                Side::Host(host) => host.send_check(tick, h),
                Side::Guest(g) => g.send_check(tick, h),
            }
        }
        let turn = self.step.peek()?;
        // The host is the relay: the tick goes out complete, then it runs.
        if let Side::Host(host) = &mut self.side {
            host.send_turn(tick, turn[..self.step.seats].to_vec());
        }
        self.step.take().map(|t| (tick, t))
    }

    /// Which seats the game is waiting for, for the line that says so.
    pub fn waiting_on(&self) -> Vec<usize> {
        self.step.waiting_on()
    }

    /// Why the game stopped, if it has.
    pub fn over(&self) -> Option<&str> {
        self.over.as_deref()
    }

    /// Anything worth telling the person, taken as it is read.
    pub fn notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notes)
    }

    /// Say goodbye. A game left politely frees the other screens at once.
    pub fn close(&mut self, why: &str) {
        match &mut self.side {
            Side::Host(h) => h.close(why),
            Side::Guest(g) => g.close(why),
        }
    }

    fn end(&mut self, why: String) {
        if self.over.is_none() {
            self.notes.push(why.clone());
            self.over = Some(why);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lobby::Event as LobbyEvent;
    use crate::lockstep::CHECK_EVERY;
    use crate::proto::SEATS;
    use henge_core::overworld::{Landscape, Overworld};

    /// Two tokens on an open map, one per seat, stepped by the two seats' own
    /// input words. Assetless, integer-only and deterministic, which is the whole
    /// of what lockstep needs from a simulation.
    struct Toy {
        land: Landscape,
        who: [Overworld; SEATS],
    }

    impl Toy {
        fn new() -> Toy {
            let mut who = [
                Overworld::new(100, 100),
                Overworld::new(140, 100),
                Overworld::new(100, 140),
                Overworld::new(140, 140),
            ];
            // `DistanceDONE+12` writes the day's allowance before the first
            // frame, and an allowance of nothing would end every turn at once
            // and make the two worlds agree for the wrong reason.
            for w in &mut who {
                w.steps_per_day = 100_000;
            }
            Toy {
                land: Landscape::open(),
                who,
            }
        }

        fn run(&mut self, turn: &Turn) {
            for (seat, input) in turn.iter().enumerate() {
                let dx = (input.pad & 0x01 != 0) as i32 - (input.pad & 0x02 != 0) as i32;
                let dy = (input.pad & 0x04 != 0) as i32 - (input.pad & 0x08 != 0) as i32;
                self.who[seat].travel(dx, dy, &self.land);
            }
        }

        fn hash(&self) -> u64 {
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for w in &self.who {
                h ^= w.state_hash();
                h = h.wrapping_mul(0x1000_0000_01b3);
            }
            h
        }
    }

    /// A different input script per seat, so the two machines really are sending
    /// each other something neither could have guessed.
    fn script(seat: usize, tick: u32) -> SeatInput {
        let n = tick
            .wrapping_mul(2_654_435_761)
            .wrapping_add(seat as u32 * 97);
        SeatInput {
            pad: ((n >> 11) & 0x0f) as u8,
            ..SeatInput::default()
        }
    }

    /// Get a host and a guest through a lobby and into a started game.
    fn started(delay: u32, check: u32) -> (Session, Session) {
        let mut host = Host::open("two machines", "carl", 0, true).unwrap();
        let port = host.port();
        let mut guest = Guest::join(("127.0.0.1", port), "anna", "").unwrap();
        let mut seat = None;
        let mut terms = None;
        for turn in 0..600 {
            for e in host.poll() {
                if let LobbyEvent::Joined { .. } = e {
                    host.seat(Some(0), true);
                }
            }
            for e in guest.poll() {
                match e {
                    LobbyEvent::Seated { seat: s } => {
                        seat = Some(s as usize);
                        guest.seat_request(Some(1), true);
                    }
                    LobbyEvent::Start { seats, .. } => terms = Some(seats as usize),
                    _ => {}
                }
            }
            if turn > 60 && seat.is_some() && terms.is_none() {
                host.start(delay, check);
            }
            if terms.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let seats = terms.expect("the game to start");
        assert_eq!(seats, 2);
        (
            Session::host(host, seats, delay, check),
            Session::guest(guest, seats, seat.unwrap(), delay, check),
        )
    }

    /// The proof this whole crate exists for: two machines, two different people
    /// pressing two different things, and one world.
    #[test]
    fn two_machines_stay_bit_for_bit_identical() {
        const TICKS: u32 = 200;
        let (mut h, mut g) = started(4, 16);
        let (mut hw, mut gw) = (Toy::new(), Toy::new());
        // The hash each machine had after each tick. The host runs a little
        // ahead of the guest (that is the hop the guest pays), so the two are
        // compared tick by tick rather than at the same instant.
        let mut theirs: std::collections::BTreeMap<u32, u64> = std::collections::BTreeMap::new();
        let mut ours: std::collections::BTreeMap<u32, u64> = std::collections::BTreeMap::new();
        let mut turns: std::collections::BTreeMap<u32, Turn> = std::collections::BTreeMap::new();
        for _ in 0..40_000 {
            h.poll();
            g.poll();
            assert_eq!(h.over(), None, "{:?}", h.over());
            assert_eq!(g.over(), None, "{:?}", g.over());
            let ht = h.step.tick();
            if let Some((tick, turn)) = h.advance(script(0, ht), &mut || hw.hash()) {
                assert_eq!(tick, ht);
                hw.run(&turn);
                theirs.insert(tick, hw.hash());
                turns.insert(tick, turn);
            }
            let gt = g.step.tick();
            if let Some((tick, turn)) = g.advance(script(1, gt), &mut || gw.hash()) {
                assert_eq!(tick, gt);
                gw.run(&turn);
                ours.insert(tick, gw.hash());
                // The same tick ran with the same words in it on both machines.
                assert_eq!(turns.get(&tick), Some(&turn), "tick {tick}");
                assert_eq!(theirs.get(&tick), ours.get(&tick), "tick {tick}");
                if tick + 1 >= TICKS {
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_micros(200));
        }
        let ran = ours.len() as u32;
        assert!(ran >= TICKS, "only {ran} ticks ran");
        // Not `hw.hash() == gw.hash()`: the host is legitimately a tick or two
        // further on. What has to match is the same tick on both machines, which
        // is what the loop checked for every one of them.
        let last = *ours.keys().next_back().unwrap();
        assert_eq!(theirs.get(&last), ours.get(&last));
        // And they really were driven apart by two different scripts: a world
        // that ignored the wire would not match one that did.
        let mut alone = Toy::new();
        for t in 0..ran {
            let mut turn = [SeatInput::default(); SEATS];
            turn[0] = script(0, t);
            alone.run(&turn);
        }
        assert_ne!(alone.hash(), gw.hash(), "seat one's input did nothing");
    }

    /// A machine that goes its own way is caught by the check and the game stops,
    /// rather than four people playing four different games.
    #[test]
    fn a_machine_that_diverges_is_caught_and_the_game_stops() {
        let (mut h, mut g) = started(4, 8);
        let (mut hw, mut gw) = (Toy::new(), Toy::new());
        // The guest's world is wrong from the start, which is what a real
        // divergence looks like by the time anybody can measure it.
        gw.who[0].x += 1;
        let mut caught = false;
        for _ in 0..20_000 {
            h.poll();
            g.poll();
            if h.over().is_some() && g.over().is_some() {
                caught = true;
                break;
            }
            let ht = h.step.tick();
            if let Some((_, turn)) = h.advance(script(0, ht), &mut || hw.hash()) {
                hw.run(&turn);
            }
            let gt = g.step.tick();
            if let Some((_, turn)) = g.advance(script(1, gt), &mut || gw.hash()) {
                gw.run(&turn);
            }
            std::thread::sleep(std::time::Duration::from_micros(200));
        }
        assert!(caught, "nobody noticed");
        assert!(h.over().unwrap().contains("stopped agreeing"));
        assert!(g.over().unwrap().contains("stopped agreeing"));
        // And neither of them will run another tick on it.
        assert!(h.advance(SeatInput::default(), &mut || 0).is_none());
        assert!(g.advance(SeatInput::default(), &mut || 0).is_none());
    }

    /// A guest that vanishes stops the host rather than leaving it to guess what
    /// that seat was pressing.
    #[test]
    fn a_seat_that_vanishes_stops_the_game_rather_than_being_guessed_at() {
        let (mut h, mut g) = started(4, 64);
        let (mut hw, mut gw) = (Toy::new(), Toy::new());
        for _ in 0..400 {
            h.poll();
            g.poll();
            let ht = h.step.tick();
            if let Some((_, turn)) = h.advance(script(0, ht), &mut || hw.hash()) {
                hw.run(&turn);
            }
            let gt = g.step.tick();
            if let Some((_, turn)) = g.advance(script(1, gt), &mut || gw.hash()) {
                gw.run(&turn);
            }
            std::thread::sleep(std::time::Duration::from_micros(200));
        }
        assert_eq!(h.over(), None, "{:?}", h.over());
        assert!(h.step.tick() > 8, "the game got going first");
        g.close("");
        drop(g);
        let mut stopped = false;
        for _ in 0..4_000 {
            h.poll();
            if h.over().is_some() {
                stopped = true;
                break;
            }
            let ht = h.step.tick();
            if let Some((_, turn)) = h.advance(script(0, ht), &mut || hw.hash()) {
                hw.run(&turn);
            }
            std::thread::sleep(std::time::Duration::from_micros(500));
        }
        assert!(stopped, "the host ran on without them");
        assert!(
            h.over().unwrap().contains("left the game"),
            "{:?}",
            h.over()
        );
    }

    /// With nobody else in it, a host runs on its own: the same code path, one
    /// seat, and no wire to wait for.
    #[test]
    fn one_player_needs_nothing_from_the_wire() {
        let host = Host::open("alone", "carl", 0, false).unwrap();
        let mut s = Session::host(host, 1, 4, CHECK_EVERY);
        let mut w = Toy::new();
        for t in 0..500u32 {
            s.poll();
            let (tick, turn) = s
                .advance(script(0, t), &mut || w.hash())
                .expect("nothing to wait for");
            assert_eq!(tick, t);
            w.run(&turn);
        }
        assert_eq!(s.over(), None);
        assert_eq!(s.waiting_on(), Vec::<usize>::new());
    }
}
