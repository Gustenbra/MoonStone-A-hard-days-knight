//! One-on-one combat: state machine, movement, and positional hit resolution.
//!
//! No rendering, no assets, no platform. Everything here is driven by content
//! data, so retuning the feel of the game is editing JSON rather than editing
//! Rust, and swapping in our own artwork later changes nothing in this file.

use crate::anim::{Player, Sequence};
use crate::arena::Bounds;
use crate::content::ActorDef;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Idle,
    Walk,
    Attack,
    Hurt,
    Dead,
}

impl State {
    pub fn sequence_name(self) -> &'static str {
        match self {
            State::Idle => "idle",
            State::Walk => "walk",
            State::Attack => "attack",
            State::Hurt => "hurt",
            State::Dead => "death",
        }
    }

    /// While these run, the fighter is committed: no turning, no new attack.
    /// Commitment is what gives a swing weight and makes spacing matter.
    pub fn is_committed(self) -> bool {
        matches!(self, State::Attack | State::Hurt | State::Dead)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Intent {
    pub dx: i32,
    pub dy: i32,
    pub attack: bool,
}

#[derive(Clone, Debug)]
pub struct Fighter {
    pub actor: String,
    pub x: i32,
    pub y: i32,
    /// Facing right is +1, left is -1.
    pub facing: i32,
    pub state: State,
    pub player: Player,
    pub health: i32,
    pub max_health: i32,
    /// One connect per swing, however many frames carry a hit line.
    pub struck: bool,
}

impl Fighter {
    /// A zeroed definition, only for building a Fighter before the real
    /// definitions are in hand. Never simulated.
    pub fn placeholder_def() -> ActorDef {
        ActorDef {
            sheet: String::new(), health: 1, speed_x: 0, speed_y: 0,
            reach: 0, depth_tolerance: 0, attack_cooldown: 0,
            body: [0; 4], sequences: Default::default(),
        }
    }

    pub fn new(actor: impl Into<String>, def: &ActorDef, x: i32, y: i32, facing: i32) -> Fighter {
        Fighter {
            actor: actor.into(),
            x, y, facing,
            state: State::Idle,
            player: Player::default(),
            health: def.health,
            max_health: def.health,
            struck: false,
        }
    }

    pub fn alive(&self) -> bool {
        self.state != State::Dead
    }

    fn enter(&mut self, state: State) {
        if self.state == state {
            return;
        }
        self.state = state;
        self.player.restart();
        self.struck = false;
    }

    pub fn sequence<'a>(&self, def: &'a ActorDef) -> Option<&'a Sequence> {
        def.sequence(self.state.sequence_name())
    }

    /// One tick. Returns the hit line this fighter is sweeping, in world space,
    /// if the current frame carries one and it has not already connected.
    pub fn step(&mut self, def: &ActorDef, intent: Intent, bounds: Bounds) -> Vec<(i32, i32)> {
        if self.state == State::Dead {
            if let Some(seq) = self.sequence(def) {
                self.player.advance(seq);
            }
            return Vec::new();
        }

        // A committed action runs to completion before input is looked at again.
        if self.state.is_committed() {
            if self.player.finished {
                self.enter(State::Idle);
            }
        } else if intent.attack {
            self.enter(State::Attack);
        } else if intent.dx != 0 || intent.dy != 0 {
            if intent.dx != 0 {
                self.facing = intent.dx.signum();
            }
            self.enter(State::Walk);
        } else {
            self.enter(State::Idle);
        }

        // Free movement only while not committed. Committed frames may still
        // carry the actor via their own dx/dy.
        if !self.state.is_committed() && self.state == State::Walk {
            let (x, y) = bounds.clamp(
                self.x + intent.dx * def.speed_x,
                self.y + intent.dy * def.speed_y,
            );
            self.x = x;
            self.y = y;
        }

        let Some(seq) = def.sequence(self.state.sequence_name()) else {
            return Vec::new();
        };
        let frame = self.player.current(seq).cloned();
        self.player.advance(seq);

        let Some(frame) = frame else { return Vec::new() };
        if frame.dx != 0 || frame.dy != 0 {
            let (x, y) = bounds.clamp(
                self.x + frame.dx as i32 * self.facing,
                self.y + frame.dy as i32,
            );
            self.x = x;
            self.y = y;
        }

        if self.state != State::Attack || self.struck || frame.hit.is_empty() {
            return Vec::new();
        }
        frame
            .hit
            .iter()
            .map(|[hx, hy]| (self.x + *hx as i32 * self.facing, self.y - *hy as i32))
            .collect()
    }

    /// Body rectangle in world space: (left, top, right, bottom), screen axes.
    pub fn body(&self, def: &ActorDef) -> (i32, i32, i32, i32) {
        let [x0, y0, x1, y1] = def.body;
        let (a, b) = (x0 as i32 * self.facing, x1 as i32 * self.facing);
        (
            self.x + a.min(b),
            self.y - y1 as i32,
            self.x + a.max(b),
            self.y - y0 as i32,
        )
    }

    pub fn take_hit(&mut self, damage: i32) {
        if self.state == State::Dead {
            return;
        }
        self.health -= damage;
        if self.health <= 0 {
            self.health = 0;
            self.enter(State::Dead);
        } else {
            self.enter(State::Hurt);
        }
    }

    /// Depth key. Everything in an arena sorts by where its feet are.
    pub fn depth(&self) -> i32 {
        self.y
    }
}

/// Does a swept hit line cross a body rectangle?
pub fn line_hits_body(line: &[(i32, i32)], body: (i32, i32, i32, i32)) -> bool {
    let (l, t, r, b) = body;
    if line.is_empty() {
        return false;
    }
    // A single point still counts, which matters for a thrust.
    if line.len() == 1 {
        let (x, y) = line[0];
        return x >= l && x <= r && y >= t && y <= b;
    }
    line.windows(2).any(|w| segment_hits_rect(w[0], w[1], l, t, r, b))
}

fn segment_hits_rect(a: (i32, i32), b: (i32, i32), l: i32, t: i32, r: i32, bo: i32) -> bool {
    let inside = |(x, y): (i32, i32)| x >= l && x <= r && y >= t && y <= bo;
    if inside(a) || inside(b) {
        return true;
    }
    // Otherwise the segment must cross one of the four edges.
    let edges = [
        ((l, t), (r, t)),
        ((r, t), (r, bo)),
        ((r, bo), (l, bo)),
        ((l, bo), (l, t)),
    ];
    edges.iter().any(|(p, q)| segments_cross(a, b, *p, *q))
}

fn segments_cross(p1: (i32, i32), p2: (i32, i32), p3: (i32, i32), p4: (i32, i32)) -> bool {
    let d = |a: (i32, i32), b: (i32, i32), c: (i32, i32)| -> i64 {
        (b.0 as i64 - a.0 as i64) * (c.1 as i64 - a.1 as i64)
            - (b.1 as i64 - a.1 as i64) * (c.0 as i64 - a.0 as i64)
    };
    let (d1, d2, d3, d4) = (d(p3, p4, p1), d(p3, p4, p2), d(p1, p2, p3), d(p1, p2, p4));
    ((d1 > 0 && d2 < 0) || (d1 < 0 && d2 > 0)) && ((d3 > 0 && d4 < 0) || (d3 < 0 && d4 > 0))
}

/// A deliberately plain opponent: close the distance, then swing when in reach.
/// Real behaviour comes later; this exists so combat can be felt end to end.
pub fn simple_ai(me: &Fighter, foe: &Fighter, def: &ActorDef, cooldown: &mut i32) -> Intent {
    if !me.alive() || !foe.alive() {
        return Intent::default();
    }
    *cooldown = (*cooldown - 1).max(0);
    let dx = foe.x - me.x;
    let dy = foe.y - me.y;
    let reach = def.reach;

    if dx.abs() <= reach && dy.abs() <= def.depth_tolerance {
        if *cooldown == 0 {
            *cooldown = def.attack_cooldown;
            return Intent { dx: 0, dy: 0, attack: true };
        }
        return Intent::default();
    }
    Intent {
        dx: if dx.abs() > reach - 4 { dx.signum() } else { 0 },
        dy: if dy.abs() > def.depth_tolerance { dy.signum() } else { 0 },
        attack: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::{EndBehaviour, Frame};
    use crate::content::ActorDef;
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
            sheet: "test".into(),
            health: 100,
            speed_x: 2,
            speed_y: 1,
            reach: 40,
            depth_tolerance: 6,
            attack_cooldown: 30,
            body: [-9, 0, 9, 52],
            sequences,
        }
    }

    fn bounds() -> Bounds {
        Bounds { left: 0, right: 319, top: 10, bottom: 114 }
    }

    #[test]
    fn walking_moves_and_turns_the_fighter() {
        let d = def();
        let mut f = Fighter::new("a", &d, 100, 100, 1);
        f.step(&d, Intent { dx: -1, dy: 0, attack: false }, bounds());
        assert_eq!(f.facing, -1);
        assert_eq!(f.x, 98);
        assert_eq!(f.state, State::Walk);
    }

    #[test]
    fn an_attack_commits_and_ignores_input_until_it_finishes() {
        let d = def();
        let mut f = Fighter::new("a", &d, 100, 100, 1);
        f.step(&d, Intent { dx: 0, dy: 0, attack: true }, bounds());
        assert_eq!(f.state, State::Attack);
        // Trying to walk away mid-swing does nothing.
        f.step(&d, Intent { dx: 1, dy: 0, attack: false }, bounds());
        assert_eq!(f.state, State::Attack);
        assert_eq!(f.x, 100);
    }

    /// Both attack frames carry a hit line. Once the arena reports a connect,
    /// the rest of that swing must stay inert, or a single strike would deal
    /// damage twice.
    #[test]
    fn a_swing_connects_once_however_many_frames_carry_the_line() {
        let d = def();
        let mut f = Fighter::new("a", &d, 100, 100, 1);
        let mut lines = 0;
        // One swing only: stop as soon as the sequence completes.
        for _ in 0..8 {
            if !f.step(&d, Intent { dx: 0, dy: 0, attack: true }, bounds()).is_empty() {
                lines += 1;
                f.struck = true; // what the arena does once a hit lands
            }
            if f.player.finished {
                break;
            }
        }
        assert_eq!(lines, 1, "two frames carry a line but only one connect is allowed");
    }

    /// A swing that misses must not disarm the rest of the swing.
    #[test]
    fn a_missed_swing_keeps_offering_its_line() {
        let d = def();
        let mut f = Fighter::new("a", &d, 100, 100, 1);
        let mut lines = 0;
        for _ in 0..8 {
            if !f.step(&d, Intent { dx: 0, dy: 0, attack: true }, bounds()).is_empty() {
                lines += 1; // nothing was hit, so `struck` stays false
            }
            if f.player.finished {
                break;
            }
        }
        assert_eq!(lines, 2);
    }

    /// Releasing and pressing again is a new swing, which may connect again.
    #[test]
    fn a_second_swing_can_connect_again() {
        let d = def();
        let mut f = Fighter::new("a", &d, 100, 100, 1);
        let mut swings = 0;
        for _ in 0..24 {
            if !f.step(&d, Intent { dx: 0, dy: 0, attack: true }, bounds()).is_empty() {
                swings += 1;
                f.struck = true;
            }
        }
        assert!(swings >= 2, "holding attack keeps swinging, got {swings}");
    }

    #[test]
    fn the_hit_line_mirrors_with_facing() {
        let d = def();
        let mut right = Fighter::new("a", &d, 100, 100, 1);
        let mut left = Fighter::new("a", &d, 100, 100, -1);
        let a = loop {
            let l = right.step(&d, Intent { dx: 0, dy: 0, attack: true }, bounds());
            if !l.is_empty() { break l; }
        };
        let b = loop {
            let l = left.step(&d, Intent { dx: 0, dy: 0, attack: true }, bounds());
            if !l.is_empty() { break l; }
        };
        assert_eq!(a[0], (110, 70));
        assert_eq!(b[0], (90, 70));
    }

    #[test]
    fn a_line_crossing_a_body_counts_even_with_both_ends_outside() {
        let body = (90, 50, 110, 100);
        assert!(line_hits_body(&[(60, 70), (140, 70)], body));
        assert!(line_hits_body(&[(100, 70)], body));
        assert!(!line_hits_body(&[(60, 20), (140, 20)], body));
        assert!(!line_hits_body(&[], body));
    }

    #[test]
    fn enough_damage_kills_and_death_is_final() {
        let d = def();
        let mut f = Fighter::new("a", &d, 0, 0, 1);
        f.take_hit(60);
        assert_eq!(f.state, State::Hurt);
        f.take_hit(60);
        assert_eq!(f.state, State::Dead);
        assert_eq!(f.health, 0);
        f.take_hit(60);
        assert_eq!(f.health, 0);
    }

    #[test]
    fn the_opponent_closes_distance_then_swings() {
        let d = def();
        let me = Fighter::new("a", &d, 0, 100, 1);
        let far = Fighter::new("b", &d, 200, 100, -1);
        let mut cd = 0;
        assert_eq!(simple_ai(&me, &far, &d, &mut cd).dx, 1, "walks toward a distant foe");

        let near = Fighter::new("b", &d, 20, 100, -1);
        let mut cd = 0;
        assert!(simple_ai(&me, &near, &d, &mut cd).attack, "swings when in reach");
    }
}
