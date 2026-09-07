//! A bout: any number of fighters in one arena.
//!
//! This is where combat is actually resolved, and it lives in the simulation
//! rather than in the renderer for three reasons. It can be tested headlessly,
//! it can run on a server that has no graphics at all, and it can be replayed
//! from a seed to prove it is deterministic.
//!
//! The bout takes one `Intent` per fighter and does not care where they came
//! from. A keyboard, an AI and a network packet are interchangeable, which is
//! the seam networked play plugs into.

use crate::arena::Bounds;
use crate::combat::{line_hits_body, Fighter, Intent, State};
use crate::content::ActorDef;
use serde::{Deserialize, Serialize};

/// Reported so a caller can play a sound, shake the screen, or log a replay,
/// without the simulation knowing any of those things exist.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct HitEvent {
    pub attacker: usize,
    pub target: usize,
    pub damage: i32,
    pub fatal: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Bout {
    pub fighters: Vec<Fighter>,
    pub bounds: Bounds,
    pub damage: i32,
    /// Ticks since only one fighter (or none) was left standing.
    pub settled_for: u32,
}

impl Bout {
    pub fn new(bounds: Bounds, fighters: Vec<Fighter>) -> Bout {
        Bout { fighters, bounds, damage: 25, settled_for: 0 }
    }

    pub fn alive(&self) -> impl Iterator<Item = usize> + '_ {
        self.fighters
            .iter()
            .enumerate()
            .filter(|(_, f)| f.alive())
            .map(|(i, _)| i)
    }

    pub fn alive_count(&self) -> usize {
        self.fighters.iter().filter(|f| f.alive()).count()
    }

    /// Some(index) once exactly one fighter is left, None while a fight is on or
    /// if everybody died.
    pub fn winner(&self) -> Option<usize> {
        let mut it = self.alive();
        match (it.next(), it.next()) {
            (Some(i), None) => Some(i),
            _ => None,
        }
    }

    pub fn settled(&self) -> bool {
        self.alive_count() <= 1
    }

    /// The nearest living fighter that is not `me`, for an opponent to aim at.
    pub fn nearest_foe(&self, me: usize) -> Option<usize> {
        let m = &self.fighters[me];
        self.alive()
            .filter(|i| *i != me)
            .min_by_key(|i| {
                let f = &self.fighters[*i];
                (f.x - m.x).abs() + (f.y - m.y).abs() * 2
            })
    }

    /// One tick. `intents` is indexed to match `fighters`; a short slice is
    /// treated as idle for the rest, which keeps a caller honest without
    /// panicking mid-fight.
    pub fn step(&mut self, def: &ActorDef, intents: &[Intent]) -> Vec<HitEvent> {
        let mut swings: Vec<(usize, Vec<(i32, i32)>)> = Vec::new();
        for i in 0..self.fighters.len() {
            let intent = intents.get(i).copied().unwrap_or_default();
            let line = self.fighters[i].step(def, intent, self.bounds);
            if !line.is_empty() {
                swings.push((i, line));
            }
        }

        // Resolve every swing against every other fighter. A swing connects at
        // most once, so a single strike cannot damage two people, which matters
        // the moment there are more than two in the arena.
        let mut events = Vec::new();
        for (attacker, line) in swings {
            for target in 0..self.fighters.len() {
                if target == attacker || !self.fighters[target].alive() {
                    continue;
                }
                if self.fighters[attacker].struck {
                    break;
                }
                let depth_ok = (self.fighters[attacker].y - self.fighters[target].y).abs()
                    <= def.depth_tolerance;
                if !depth_ok || !line_hits_body(&line, self.fighters[target].body(def)) {
                    continue;
                }
                self.fighters[attacker].struck = true;
                self.fighters[target].take_hit(self.damage);
                events.push(HitEvent {
                    attacker,
                    target,
                    damage: self.damage,
                    fatal: self.fighters[target].state == State::Dead,
                });
            }
        }

        self.separate(def);
        if self.settled() {
            self.settled_for += 1;
        }
        events
    }

    /// Living fighters at the same depth cannot occupy the same ground. Pushing
    /// them apart rather than blocking movement keeps a scrappy close-quarters
    /// fight readable instead of jamming people into a stalemate.
    fn separate(&mut self, def: &ActorDef) {
        let min_gap = (def.body[2] - def.body[0]) as i32;
        let living: Vec<usize> = self.alive().collect();
        for a in 0..living.len() {
            for b in a + 1..living.len() {
                let (i, j) = (living[a], living[b]);
                if (self.fighters[i].y - self.fighters[j].y).abs() > def.depth_tolerance {
                    continue;
                }
                let gap = self.fighters[j].x - self.fighters[i].x;
                if gap.abs() >= min_gap {
                    continue;
                }
                let push = (min_gap - gap.abs() + 1) / 2;
                let dir = if gap >= 0 { 1 } else { -1 };
                let bounds = self.bounds;
                let (ix, _) = bounds.clamp(self.fighters[i].x - push * dir, self.fighters[i].y);
                let (jx, _) = bounds.clamp(self.fighters[j].x + push * dir, self.fighters[j].y);
                self.fighters[i].x = ix;
                self.fighters[j].x = jx;
            }
        }
    }

    /// A cheap fingerprint of the whole simulation.
    ///
    /// Two bouts fed the same inputs must agree on this at every tick. That is
    /// the property networked play is built on, and the only way to keep it is
    /// to check it continuously rather than to hope.
    pub fn state_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |v: i64| {
            h ^= v as u64;
            h = h.wrapping_mul(0x1000_0000_01b3);
        };
        for f in &self.fighters {
            mix(f.x as i64);
            mix(f.y as i64);
            mix(f.facing as i64);
            mix(f.health as i64);
            mix(f.state as i64);
            mix(f.struck as i64);
            mix(f.player.frame as i64);
            mix(f.player.ticks_in_frame as i64);
            mix(f.player.finished as i64);
        }
        mix(self.settled_for as i64);
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::{EndBehaviour, Frame, Sequence};
    use std::collections::BTreeMap;

    fn def() -> ActorDef {
        let seq = |name: &str, sprites: &[u16], end, hits: bool| {
            let frames = sprites
                .iter()
                .map(|s| Frame {
                    sprite: *s,
                    ticks: 1,
                    hit: if hits { vec![[10, 30], [40, 30]] } else { vec![] },
                    ..Frame::default()
                })
                .collect();
            (name.to_string(), Sequence { name: name.into(), frames, end })
        };
        let mut sequences = BTreeMap::new();
        for (k, v) in [
            seq("idle", &[0], EndBehaviour::Loop, false),
            seq("walk", &[1, 2], EndBehaviour::Loop, false),
            seq("attack", &[3, 4], EndBehaviour::HoldLast, true),
            seq("hurt", &[5], EndBehaviour::HoldLast, false),
            seq("death", &[6], EndBehaviour::HoldLast, false),
        ] {
            sequences.insert(k, v);
        }
        ActorDef {
            sheet: "test".into(), health: 100, speed_x: 2, speed_y: 1,
            reach: 40, depth_tolerance: 6, attack_cooldown: 30,
            bounty: 0,
            body: [-9, 0, 9, 52], sequences,
        }
    }

    fn bounds() -> Bounds {
        Bounds { left: 0, right: 319, top: 10, bottom: 114 }
    }

    fn four() -> Bout {
        let d = def();
        Bout::new(
            bounds(),
            (0..4)
                .map(|i| Fighter::new("k", &d, 40 + i * 70, 100, 1))
                .collect(),
        )
    }

    #[test]
    fn a_bout_holds_as_many_fighters_as_it_is_given() {
        let b = four();
        assert_eq!(b.alive_count(), 4);
        assert_eq!(b.winner(), None, "nobody has won while four are standing");
    }

    #[test]
    fn one_swing_cannot_cut_down_two_people() {
        let d = def();
        // Two targets stacked on the same spot, both inside one swing's line.
        let mut b = Bout::new(
            bounds(),
            vec![
                Fighter::new("k", &d, 100, 100, 1),
                Fighter::new("k", &d, 125, 100, -1),
                Fighter::new("k", &d, 128, 100, -1),
            ],
        );
        let mut hits = Vec::new();
        for _ in 0..8 {
            let intents = [Intent { dx: 0, dy: 0, attack: true }, Intent::default(), Intent::default()];
            hits.extend(b.step(&d, &intents));
            if b.fighters[0].player.finished {
                break;
            }
        }
        assert_eq!(hits.len(), 1, "a single strike must land on one target");
    }

    #[test]
    fn the_last_one_standing_wins() {
        let d = def();
        let mut b = Bout::new(
            bounds(),
            vec![
                Fighter::new("k", &d, 100, 100, 1),
                Fighter::new("k", &d, 160, 100, -1),
            ],
        );
        b.fighters[1].take_hit(1000);
        assert_eq!(b.winner(), Some(0));
        assert!(b.settled());
    }

    #[test]
    fn a_fighter_aims_at_the_nearest_living_opponent() {
        let mut b = four();
        assert_eq!(b.nearest_foe(0), Some(1));
        b.fighters[1].take_hit(1000);
        assert_eq!(b.nearest_foe(0), Some(2), "the dead are not targets");
    }

    /// The property everything networked rests on. Two independent bouts fed the
    /// same inputs must agree tick for tick, or lockstep and rollback are both
    /// impossible.
    #[test]
    fn identical_inputs_produce_identical_simulations() {
        let d = def();
        let script = |t: usize, i: usize| Intent {
            dx: if (t / 7 + i) % 3 == 0 { 1 } else { -1 },
            dy: if (t / 11 + i) % 4 == 0 { 1 } else { 0 },
            attack: (t / 5 + i) % 4 == 0,
        };
        let run = || {
            let mut b = four();
            let mut hashes = Vec::new();
            for t in 0..600 {
                let intents: Vec<Intent> = (0..4).map(|i| script(t, i)).collect();
                b.step(&d, &intents);
                hashes.push(b.state_hash());
            }
            hashes
        };
        assert_eq!(run(), run());
    }

    /// Snapshots have to restore exactly, or a player joining a bout in progress
    /// desynchronises immediately.
    #[test]
    fn a_bout_survives_a_round_trip_through_serialization() {
        let d = def();
        let mut b = four();
        for t in 0..90 {
            let intents: Vec<Intent> = (0..4)
                .map(|i| Intent { dx: 1, dy: 0, attack: (t + i) % 6 == 0 })
                .collect();
            b.step(&d, &intents);
        }

        let json = serde_json::to_string(&b).unwrap();
        let mut restored: Bout = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, b);
        assert_eq!(restored.state_hash(), b.state_hash());

        // And it must keep agreeing once both are simulated onward.
        for t in 0..120 {
            let intents: Vec<Intent> = (0..4)
                .map(|i| Intent { dx: -1, dy: 0, attack: (t + i) % 5 == 0 })
                .collect();
            b.step(&d, &intents);
            restored.step(&d, &intents);
        }
        assert_eq!(restored.state_hash(), b.state_hash());
    }
}
