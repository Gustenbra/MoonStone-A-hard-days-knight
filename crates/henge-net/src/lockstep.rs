//! The scheduler: which tick may run, and with whose input.
//!
//! **The whole of the rule is one sentence.** A tick does not run until every
//! seat's input for it has arrived, and then it runs with exactly that input on
//! every machine.
//!
//! Nothing is predicted and nothing is rolled back. The alternative, guessing a
//! missing input and correcting later, would mean re-running ticks of
//! `henge_core`, and a rolled-back tick that had already been drawn, heard and
//! shaken is not a tick this game can take back. A peer that falls behind
//! therefore stalls everybody, visibly, which is also the honest thing to show
//! the person: the game is waiting for Anna, and it says so.
//!
//! ### Input delay
//!
//! Local input is not used on the tick it was read. It is scheduled
//! [`Lockstep::delay`] ticks ahead, which is the window a peer's input has to
//! cross the wire in before it is late. The first `delay` ticks of a game are
//! primed with nothing held, which is the only input nobody could have sent in
//! time.
//!
//! What a tick is worth in milliseconds is the *game's* business and not this
//! module's, and the two rates it runs at are far apart:
//!
//! ```text
//! a fight    6 ticks of the 54.6204 Hz timer     109.849 ms a pass
//! elsewhere  1 tick of the 70.0863 Hz retrace     14.268 ms a tick
//! ```
//!
//! So a delay of eight ticks is about 114 ms on the map and about one and a third
//! passes in a fight. [`delay_for_rtt`] turns a measured round trip into a number
//! of ticks on that basis, and the host sends its choice to everybody so that all
//! four machines schedule to the same tick.
//!
//! ### The check
//!
//! Determinism is not assumed, it is checked. Every [`Lockstep::check`] ticks
//! each machine fingerprints its own state and sends it; the host compares, and
//! the first disagreement ends the game as a fair one rather than letting four
//! people play four different games. The fingerprints are the ones the simulation
//! already has: `Bout::state_hash` in a fight, `Run::state_hash` mixed with
//! `Overworld::state_hash` outside one.

use crate::proto::{SeatInput, SEATS};
use std::collections::BTreeMap;

/// A tick's worth of input, in seat order.
pub type Turn = [SeatInput; SEATS];

/// How many ticks of input delay a round trip wants, at a given tick length.
///
/// One round trip is the floor: input read here has to reach the furthest peer
/// and that peer has to be running the tick it belongs to. Half a tick is added
/// so that a round trip of exactly one tick does not land on the boundary, and
/// the result is clamped to [`MIN_DELAY`] and [`MAX_DELAY`].
pub fn delay_for_rtt(rtt: std::time::Duration, tick: std::time::Duration) -> u32 {
    let tick_us = tick.as_micros().max(1);
    let rtt_us = rtt.as_micros();
    let ticks = (rtt_us * 2 + tick_us) / (tick_us * 2);
    (ticks as u32).clamp(MIN_DELAY, MAX_DELAY)
}

/// Two, because one would mean a peer's input had to arrive in the same tick it
/// was read, and nothing arrives in no time.
pub const MIN_DELAY: u32 = 2;

/// Above this the game is unplayable anyway and saying so beats hiding it in a
/// growing delay: forty ticks is over half a second on the retrace.
pub const MAX_DELAY: u32 = 40;

/// How often state is fingerprinted, unless the host says otherwise. Sixty-four
/// ticks is about a second on the map and about ten passes of a fight, which is
/// often enough to catch a divergence while it is still one tick old.
pub const CHECK_EVERY: u32 = 64;

/// What a divergence looks like.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Desync {
    pub tick: u32,
    /// Every seat's fingerprint, in seat order, and nothing for a seat that has
    /// not reported that tick yet.
    pub hashes: Vec<Option<u64>>,
}

impl std::fmt::Display for Desync {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "tick {}:", self.tick)?;
        for (seat, h) in self.hashes.iter().enumerate() {
            match h {
                Some(h) => write!(f, " seat {seat} {h:016x}")?,
                None => write!(f, " seat {seat} -")?,
            }
        }
        Ok(())
    }
}

/// The scheduler.
#[derive(Clone, Debug)]
pub struct Lockstep {
    /// How many seats are playing. Seats beyond this are the computer knights,
    /// which need nothing from the wire.
    pub seats: usize,
    /// Which seat this machine's player is in.
    pub mine: usize,
    /// Ticks between reading input and using it.
    pub delay: u32,
    /// How often a fingerprint is taken.
    pub check: u32,
    /// The next tick to run.
    tick: u32,
    /// Input that has arrived and has not been used: tick and seat to word.
    queue: BTreeMap<(u32, usize), SeatInput>,
    /// The last tick this machine scheduled its own input for, so it cannot
    /// schedule the same one twice or skip one.
    scheduled: Option<u32>,
    /// Our own fingerprints, and everyone's as they report them.
    mine_hashes: BTreeMap<u32, u64>,
    their_hashes: BTreeMap<(u32, usize), u64>,
    /// The first divergence, once there is one. A game does not recover from it.
    found: Option<Desync>,
}

impl Lockstep {
    /// A game about to run its first tick.
    ///
    /// The first `delay` ticks are primed with nothing held for every seat: they
    /// are the ticks whose input would have had to be sent before the game
    /// started.
    pub fn new(seats: usize, mine: usize, delay: u32, check: u32) -> Lockstep {
        let seats = seats.clamp(1, SEATS);
        let delay = delay.clamp(MIN_DELAY, MAX_DELAY);
        let mut l = Lockstep {
            seats,
            mine: mine.min(seats - 1),
            delay,
            check: check.max(1),
            tick: 0,
            queue: BTreeMap::new(),
            scheduled: None,
            mine_hashes: BTreeMap::new(),
            their_hashes: BTreeMap::new(),
            found: None,
        };
        for t in 0..delay {
            for s in 0..l.seats {
                l.queue.insert((t, s), SeatInput::default());
            }
        }
        l
    }

    /// The tick that will run next.
    pub fn tick(&self) -> u32 {
        self.tick
    }

    /// Whether everything needed to run [`Lockstep::tick`] is in.
    pub fn ready(&self) -> bool {
        self.found.is_none() && (0..self.seats).all(|s| self.queue.contains_key(&(self.tick, s)))
    }

    /// Which seats are holding the game up, if any. What the "waiting for Anna"
    /// line is drawn from.
    pub fn waiting_on(&self) -> Vec<usize> {
        (0..self.seats)
            .filter(|s| !self.queue.contains_key(&(self.tick, *s)))
            .collect()
    }

    /// Put away an input for a tick. A tick already run is dropped: it is a
    /// duplicate, and accepting it would leave the queue growing for ever.
    ///
    /// Returns whether it was kept.
    pub fn put(&mut self, tick: u32, seat: usize, input: SeatInput) -> bool {
        if tick < self.tick || seat >= self.seats {
            return false;
        }
        self.queue.insert((tick, seat), input);
        true
    }

    /// Read this machine's own input, for the tick it belongs to.
    ///
    /// Returns the tick it was scheduled for, so the caller knows what to put on
    /// the wire, or nothing when this tick's input has already been scheduled
    /// (the loop may poll more often than it steps).
    pub fn schedule(&mut self, input: SeatInput) -> Option<u32> {
        let at = self.tick + self.delay;
        if self.scheduled == Some(at) {
            return None;
        }
        self.scheduled = Some(at);
        self.queue.insert((at, self.mine), input);
        Some(at)
    }

    /// The turn that is ready, without taking it. What the host broadcasts
    /// before it runs the tick itself.
    pub fn peek(&self) -> Option<Turn> {
        if !self.ready() {
            return None;
        }
        let mut turn = [SeatInput::default(); SEATS];
        for (s, slot) in turn.iter_mut().enumerate().take(self.seats) {
            if let Some(i) = self.queue.get(&(self.tick, s)) {
                *slot = *i;
            }
        }
        Some(turn)
    }

    /// Take the turn and step on, or nothing if it is not all in yet.
    ///
    /// Seats past [`Lockstep::seats`] come back as nothing held, which is what a
    /// computer knight's seat reads as: the original's `GetInputDevice` is not
    /// called for them either.
    pub fn take(&mut self) -> Option<Turn> {
        if !self.ready() {
            return None;
        }
        let mut turn = [SeatInput::default(); SEATS];
        for (s, slot) in turn.iter_mut().enumerate().take(self.seats) {
            if let Some(i) = self.queue.remove(&(self.tick, s)) {
                *slot = i;
            }
        }
        self.tick += 1;
        Some(turn)
    }

    /// Whether a fingerprint is due for the tick about to run.
    pub fn check_due(&self) -> bool {
        self.tick.is_multiple_of(self.check)
    }

    /// Our own fingerprint for a tick.
    pub fn note(&mut self, tick: u32, hash: u64) {
        self.mine_hashes.insert(tick, hash);
        self.compare(tick);
    }

    /// A peer's fingerprint for a tick.
    pub fn heard(&mut self, tick: u32, seat: usize, hash: u64) {
        if seat >= self.seats {
            return;
        }
        self.their_hashes.insert((tick, seat), hash);
        self.compare(tick);
    }

    /// The first divergence, if there has been one.
    pub fn desync(&self) -> Option<&Desync> {
        self.found.as_ref()
    }

    /// Hand a divergence the host found to a guest, which has nothing to compare
    /// against of its own.
    pub fn told_desync(&mut self, tick: u32, hashes: Vec<u64>) {
        if self.found.is_none() {
            self.found = Some(Desync {
                tick,
                hashes: hashes.into_iter().map(Some).collect(),
            });
        }
    }

    /// Compare whatever is in for a tick. Only the first disagreement is kept:
    /// everything after it is a consequence of it.
    fn compare(&mut self, tick: u32) {
        if self.found.is_some() {
            return;
        }
        let Some(mine) = self.mine_hashes.get(&tick).copied() else {
            return;
        };
        let mut differs = false;
        for s in 0..self.seats {
            if s == self.mine {
                continue;
            }
            match self.their_hashes.get(&(tick, s)) {
                Some(h) if *h != mine => differs = true,
                _ => {}
            }
        }
        if !differs {
            return;
        }
        let hashes = (0..self.seats)
            .map(|s| {
                if s == self.mine {
                    Some(mine)
                } else {
                    self.their_hashes.get(&(tick, s)).copied()
                }
            })
            .collect();
        self.found = Some(Desync { tick, hashes });
    }

    /// Drop fingerprints older than a tick, which the host does once every seat
    /// has agreed about them. Without this a long game keeps every one.
    pub fn forget_before(&mut self, tick: u32) {
        self.mine_hashes.retain(|t, _| *t >= tick);
        self.their_hashes.retain(|(t, _), _| *t >= tick);
    }

    /// How much input is waiting, for a diagnostic line.
    pub fn queued(&self) -> usize {
        self.queue.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::key;
    use std::time::Duration;

    fn pad(p: u8) -> SeatInput {
        SeatInput {
            pad: p,
            ..SeatInput::default()
        }
    }

    /// The primed ticks let the game start at once, and then it stops dead until
    /// the other seat's input for the tick actually arrives.
    #[test]
    fn it_runs_the_primed_ticks_and_then_waits() {
        let mut l = Lockstep::new(2, 0, 4, CHECK_EVERY);
        for t in 0..4 {
            assert!(l.ready(), "tick {t} was primed");
            assert_eq!(l.take(), Some([SeatInput::default(); SEATS]));
        }
        assert_eq!(l.tick(), 4);
        assert!(!l.ready(), "and now it needs the wire");
        assert_eq!(l.waiting_on(), vec![0, 1]);
        assert_eq!(l.schedule(pad(1)), Some(8), "ours is four ticks ahead");
        assert!(!l.ready(), "which is not this tick");
        l.put(4, 0, pad(2));
        assert_eq!(l.waiting_on(), vec![1]);
        l.put(4, 1, pad(4));
        assert!(l.ready());
        let turn = l.take().expect("a turn");
        assert_eq!(turn[0].pad, 2);
        assert_eq!(turn[1].pad, 4);
        assert_eq!(turn[2], SeatInput::default(), "a computer knight's seat");
    }

    /// Two machines scheduling their own input end up with the same tick for it,
    /// because they are on the same tick and the same delay.
    #[test]
    fn both_machines_schedule_to_the_same_tick() {
        let mut a = Lockstep::new(2, 0, 3, CHECK_EVERY);
        let mut b = Lockstep::new(2, 1, 3, CHECK_EVERY);
        for _ in 0..3 {
            a.take();
            b.take();
        }
        let at_a = a.schedule(pad(1)).expect("a tick");
        let at_b = b.schedule(pad(2)).expect("a tick");
        assert_eq!(at_a, at_b);
        a.put(at_b, 1, pad(2));
        b.put(at_a, 0, pad(1));
        // And they both run it with both words in it.
        for _ in 0..3 {
            a.take();
            b.take();
        }
        assert_eq!(a.take(), b.take());
    }

    #[test]
    fn the_same_tick_is_not_scheduled_twice() {
        let mut l = Lockstep::new(1, 0, 2, CHECK_EVERY);
        assert_eq!(l.schedule(pad(1)), Some(2));
        assert_eq!(l.schedule(pad(8)), None, "the loop polled again");
        l.take();
        assert_eq!(l.schedule(pad(8)), Some(3));
    }

    #[test]
    fn input_for_a_tick_already_run_is_dropped() {
        let mut l = Lockstep::new(1, 0, 2, CHECK_EVERY);
        l.take();
        l.take();
        assert_eq!(l.tick(), 2);
        assert!(!l.put(1, 0, pad(1)), "that tick has been and gone");
        assert!(l.put(2, 0, pad(1)));
    }

    /// A seat that nobody is in cannot hold the game up and cannot be written to.
    #[test]
    fn a_seat_nobody_is_in_is_not_waited_for() {
        let mut l = Lockstep::new(2, 0, 2, CHECK_EVERY);
        assert!(!l.put(5, 3, pad(1)));
        l.take();
        l.take();
        l.put(2, 0, pad(1));
        l.put(2, 1, pad(1));
        assert!(l.ready(), "seats two and three are the computer's");
    }

    /// The whole point of the check: the moment two machines differ, the game
    /// stops being a fair one and says which one went its own way.
    #[test]
    fn a_divergence_is_caught_and_named() {
        let mut l = Lockstep::new(3, 0, 2, 8);
        l.note(16, 0xdead_beef);
        l.heard(16, 1, 0xdead_beef);
        assert!(l.desync().is_none(), "agreement so far");
        l.heard(16, 2, 0x0bad_f00d);
        let d = l.desync().expect("a divergence");
        assert_eq!(d.tick, 16);
        assert_eq!(d.hashes[0], Some(0xdead_beef));
        assert_eq!(d.hashes[2], Some(0x0bad_f00d));
        assert!(d.to_string().contains("seat 2 000000000badf00d"));
        // And it cannot run another tick on it.
        assert!(!l.ready());
    }

    #[test]
    fn only_the_first_divergence_is_kept() {
        let mut l = Lockstep::new(2, 0, 2, 8);
        l.note(8, 1);
        l.heard(8, 1, 2);
        l.note(16, 3);
        l.heard(16, 1, 4);
        assert_eq!(l.desync().map(|d| d.tick), Some(8));
    }

    /// A guest has nothing to compare against, so the host tells it.
    #[test]
    fn a_guest_takes_the_hosts_word_for_a_divergence() {
        let mut l = Lockstep::new(2, 1, 2, 8);
        assert!(l.desync().is_none());
        l.told_desync(24, vec![7, 9]);
        assert_eq!(l.desync().map(|d| d.tick), Some(24));
        assert!(!l.ready());
    }

    #[test]
    fn fingerprints_are_forgotten_once_they_are_agreed() {
        let mut l = Lockstep::new(2, 0, 2, 8);
        for t in [8u32, 16, 24] {
            l.note(t, t as u64);
            l.heard(t, 1, t as u64);
        }
        l.forget_before(24);
        assert_eq!(l.mine_hashes.len(), 1);
        assert_eq!(l.their_hashes.len(), 1);
    }

    /// The delay is a number of ticks and the tick is not the same length on
    /// every screen, so the same round trip is worth fewer ticks in a fight.
    #[test]
    fn a_round_trip_becomes_a_number_of_ticks() {
        let retrace = Duration::from_micros(14_268);
        let pass = Duration::from_micros(109_849);
        assert_eq!(delay_for_rtt(Duration::from_millis(0), retrace), MIN_DELAY);
        assert_eq!(delay_for_rtt(Duration::from_millis(40), retrace), 3);
        assert_eq!(delay_for_rtt(Duration::from_millis(100), retrace), 7);
        assert_eq!(delay_for_rtt(Duration::from_millis(100), pass), MIN_DELAY);
        assert_eq!(delay_for_rtt(Duration::from_secs(9), retrace), MAX_DELAY);
    }

    /// The `keys` byte travels like the pad does, because name entry is input
    /// too and a lobby where one person types a name is a lobby where everybody
    /// has to see the same letters.
    #[test]
    fn the_keyboard_half_travels_with_the_stick() {
        let mut l = Lockstep::new(2, 0, 2, CHECK_EVERY);
        l.take();
        l.take();
        l.put(
            2,
            0,
            SeatInput {
                keys: key::BACK,
                typed: Some('A'),
                ..SeatInput::default()
            },
        );
        l.put(2, 1, SeatInput::default());
        let turn = l.take().expect("a turn");
        assert_eq!(turn[0].typed, Some('A'));
        assert!(turn[0].back());
    }
}
