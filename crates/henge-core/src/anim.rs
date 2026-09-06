//! Frame-sequence playback.
//!
//! An animation is a list of frames, each held for a number of ticks. Sequences
//! either loop or hold their final frame, which is what a death or a completed
//! swing needs.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndBehaviour {
    Loop,
    HoldLast,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Frame {
    /// Index into whichever sprite bank this sequence belongs to.
    pub sprite: u16,
    /// How many ticks to hold it.
    pub ticks: u8,
    /// Where the sprite sits relative to the actor's feet.
    #[serde(default)]
    pub offset_x: i16,
    #[serde(default)]
    pub offset_y: i16,
    /// How far the actor is carried by this frame, so movement comes from the
    /// animation rather than being bolted on beside it. A lunge that travels is
    /// a property of the swing, not of the input.
    #[serde(default)]
    pub dx: i16,
    #[serde(default)]
    pub dy: i16,
    /// The path a strike sweeps through while this frame is on screen, in the
    /// actor's own space: x forward, y up from the feet. Empty means this frame
    /// cannot hit anything.
    ///
    /// This is the shape `COLLIDE.HIT` stores for the original's creatures, and
    /// it is what makes the combat positional: a swing connects when its line
    /// crosses the target, not when two boxes overlap.
    #[serde(default)]
    pub hit: Vec<[i16; 2]>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Sequence {
    pub name: String,
    pub frames: Vec<Frame>,
    pub end: EndBehaviour,
}

impl Sequence {
    pub fn total_ticks(&self) -> u32 {
        self.frames.iter().map(|f| f.ticks as u32).sum()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Player {
    pub frame: usize,
    pub ticks_in_frame: u8,
    pub finished: bool,
}

impl Player {
    pub fn restart(&mut self) {
        *self = Player::default();
    }

    pub fn advance(&mut self, seq: &Sequence) {
        if seq.frames.is_empty() || (self.finished && seq.end == EndBehaviour::HoldLast) {
            return;
        }
        self.ticks_in_frame += 1;
        if self.ticks_in_frame >= seq.frames[self.frame].ticks.max(1) {
            self.ticks_in_frame = 0;
            self.frame += 1;
            if self.frame >= seq.frames.len() {
                match seq.end {
                    EndBehaviour::Loop => self.frame = 0,
                    EndBehaviour::HoldLast => {
                        self.frame = seq.frames.len() - 1;
                        self.finished = true;
                    }
                }
            }
        }
    }

    pub fn current<'a>(&self, seq: &'a Sequence) -> Option<&'a Frame> {
        seq.frames.get(self.frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seq(end: EndBehaviour) -> Sequence {
        Sequence {
            name: "test".into(),
            end,
            frames: (0..3)
                .map(|i| Frame { sprite: i, ticks: 2, ..Frame::default() })
                .collect(),
        }
    }

    #[test]
    fn holds_each_frame_for_its_tick_count() {
        let s = seq(EndBehaviour::Loop);
        let mut p = Player::default();
        p.advance(&s);
        assert_eq!(p.frame, 0, "still on frame 0 after one tick of a two-tick frame");
        p.advance(&s);
        assert_eq!(p.frame, 1);
    }

    #[test]
    fn loops_back_to_the_start() {
        let s = seq(EndBehaviour::Loop);
        let mut p = Player::default();
        for _ in 0..s.total_ticks() {
            p.advance(&s);
        }
        assert_eq!(p.frame, 0);
        assert!(!p.finished);
    }

    #[test]
    fn hold_last_stops_on_the_final_frame() {
        let s = seq(EndBehaviour::HoldLast);
        let mut p = Player::default();
        for _ in 0..s.total_ticks() * 3 {
            p.advance(&s);
        }
        assert_eq!(p.frame, 2);
        assert!(p.finished);
    }
}
