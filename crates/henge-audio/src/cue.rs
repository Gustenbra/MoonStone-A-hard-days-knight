//! Working out what to play, by watching the fight.
//!
//! Sound is presentation, so none of this lives in the simulation. Cues are
//! derived by comparing each fighter against how it looked last tick, which
//! means the simulation stays free of anything it does not need to be correct,
//! and audio can never change the outcome of a bout.

use henge_core::bout::HitEvent;
use henge_core::combat::{Fighter, State};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cue {
    /// A swing starting, whether or not it lands.
    Swing,
    /// A blade landing on somebody.
    Hit,
    /// The blow that finishes them.
    Death,
    /// A footfall.
    Step,
}

impl Cue {
    /// The asset id this cue plays. Ids, not paths, so replacing the sound is a
    /// pack change rather than a code change.
    pub fn sound(self) -> &'static str {
        match self {
            Cue::Swing => "sfx.swish",
            Cue::Hit => "sfx.hit3",
            Cue::Death => "sfx.grnt3",
            Cue::Step => "sfx.kstep",
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Snapshot {
    state: Option<State>,
    frame: usize,
}

/// Watches a bout and reports what should be heard.
#[derive(Default)]
pub struct Voices {
    prev: Vec<Snapshot>,
    /// Which frames of a hand authored walk cycle put a foot down. Two per
    /// eight frame cycle is a walk; every frame would be a stampede.
    pub step_frames: Vec<usize>,
    /// The same, for a fighter animated by the task VM, whose walk is a cycle
    /// of four one-frame scripts rather than an eight frame list. Two of those
    /// four are foot plants, which is the same walk counted differently.
    pub script_step_frames: Vec<usize>,
}

impl Voices {
    pub fn new() -> Voices {
        Voices {
            prev: Vec::new(),
            step_frames: vec![0, 4],
            script_step_frames: vec![0, 2],
        }
    }

    pub fn observe(&mut self, fighters: &[Fighter], hits: &[HitEvent]) -> Vec<(usize, Cue)> {
        self.prev.resize(fighters.len(), Snapshot::default());
        let mut out = Vec::new();

        for (i, f) in fighters.iter().enumerate() {
            // Where the walk is up to. A scripted fighter's cycle is which of
            // its state's scripts is playing; a hand authored one's is the
            // frame the player is on.
            let scripted = f.task.is_some();
            let now = Snapshot {
                state: Some(f.state),
                frame: if scripted { f.cycle } else { f.player.frame },
            };
            let was = self.prev[i];

            // A swing is announced when it begins, not when it connects, so the
            // sound leads the blow the way it does in life.
            if now.state == Some(State::Attack) && was.state != Some(State::Attack) {
                out.push((i, Cue::Swing));
            }
            let steps = if scripted {
                &self.script_step_frames
            } else {
                &self.step_frames
            };
            if f.state == State::Walk && now.frame != was.frame && steps.contains(&now.frame) {
                out.push((i, Cue::Step));
            }
            self.prev[i] = now;
        }

        // Hits come from the simulation itself rather than being inferred, so a
        // blow never goes unheard and a miss is never announced.
        for h in hits {
            out.push((h.target, if h.fatal { Cue::Death } else { Cue::Hit }));
        }
        out
    }

    /// Forget everything, for when a bout restarts and last tick means nothing.
    pub fn reset(&mut self) {
        self.prev.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use henge_core::content::ActorDef;
    use std::collections::BTreeMap;

    fn def() -> ActorDef {
        ActorDef {
            sheet: "t".into(),
            health: 100,
            speed_x: 2,
            speed_y: 1,
            reach: 40,
            depth_tolerance: 6,
            attack_cooldown: 30,
            bounty: 0,
            girth: 0,
            body: [-9, 0, 9, 52],
            sequences: BTreeMap::new(),
            ..ActorDef::default()
        }
    }

    fn fighter(state: State, frame: usize) -> Fighter {
        let mut f = Fighter::new("k", &def(), 0, 0, 1);
        f.state = state;
        f.player.frame = frame;
        f
    }

    #[test]
    fn a_swing_is_heard_once_when_it_starts() {
        let mut v = Voices::new();
        v.observe(&[fighter(State::Idle, 0)], &[]);
        let a = v.observe(&[fighter(State::Attack, 0)], &[]);
        assert_eq!(a, vec![(0, Cue::Swing)]);
        // Still swinging is not a new swing.
        let b = v.observe(&[fighter(State::Attack, 1)], &[]);
        assert!(b.is_empty());
    }

    #[test]
    fn footfalls_land_on_their_frames_and_not_between_them() {
        let mut v = Voices::new();
        v.observe(&[fighter(State::Walk, 7)], &[]);
        assert_eq!(
            v.observe(&[fighter(State::Walk, 0)], &[]),
            vec![(0, Cue::Step)]
        );
        assert!(v.observe(&[fighter(State::Walk, 1)], &[]).is_empty());
        assert!(v.observe(&[fighter(State::Walk, 3)], &[]).is_empty());
        assert_eq!(
            v.observe(&[fighter(State::Walk, 4)], &[]),
            vec![(0, Cue::Step)]
        );
    }

    #[test]
    fn standing_still_makes_no_footsteps() {
        let mut v = Voices::new();
        v.observe(&[fighter(State::Idle, 7)], &[]);
        assert!(v.observe(&[fighter(State::Idle, 0)], &[]).is_empty());
    }

    #[test]
    fn a_landed_blow_is_heard_and_a_fatal_one_sounds_different() {
        let mut v = Voices::new();
        let hits = vec![
            HitEvent {
                attacker: 0,
                target: 1,
                damage: 25,
                fatal: false,
            },
            HitEvent {
                attacker: 0,
                target: 2,
                damage: 25,
                fatal: true,
            },
        ];
        let crowd: Vec<Fighter> = (0..3).map(|_| fighter(State::Attack, 0)).collect();
        let out = v.observe(&crowd, &hits);
        assert!(out.contains(&(1, Cue::Hit)));
        assert!(out.contains(&(2, Cue::Death)));
        assert_ne!(Cue::Hit.sound(), Cue::Death.sound());
    }

    #[test]
    fn a_restart_does_not_replay_the_last_bout() {
        let mut v = Voices::new();
        v.observe(&[fighter(State::Attack, 0)], &[]);
        v.reset();
        // After a reset the first sighting is a fresh start, so a fighter who is
        // mid-swing when the bout resets does not swing twice.
        let out = v.observe(&[fighter(State::Attack, 0)], &[]);
        assert_eq!(out, vec![(0, Cue::Swing)]);
    }
}
