//! The hand over the dice table: `DD_ShakeDice`, `DD_ThrowDice` and the loop
//! that runs them.
//!
//! `_TAVERN` puts the table up at `TavernOpenScene` (image 0xb0f5) and then
//! sits in `TavernLoop` (0xb137) until somebody leaves or the dice come down:
//!
//! ```text
//! 0xb10b  call load_TILE2                  ; `dice.piv` is already up
//! 0xb11c  fade in
//! 0xb12e  mov word ptr [DiceTHROW], 0
//! 0xb134  call ShakeDice                   ; below
//! TavernLoop 0xb137
//!         call MovePointer                 ; and CHECKGADGET with it
//!         update the tasks; draw them
//!         write the purse on the plank
//!         cmp word ptr [DiceTHROW], 2; je DiceRND
//!         cmp word ptr [XFL], 0; je TavernLoop
//! ```
//!
//! and the two routines that start the animations are three registers apart:
//!
//! ```text
//! ShakeDice 0xb18a
//!   si = DD_ShakeDice; bp = DiceHANDLE; ax = 0xa0; bx = 0; cx = 0x64;
//!   dh = 1; dl = 0x22; ADDTASK
//! ```
//!
//! `dl` is 0x22, so slot 0x22 of the end-of-animation table is what the engine
//! calls when either script reaches its `ff ff`, and that handler is at 0xb1a1:
//!
//! ```text
//! 0xb1a1  mov [JOYSTICK1+6], si
//! 0xb1a5  cmp word ptr [DiceTHROW], 0
//! 0xb1aa  jne DiceDone                     ; the throw has landed
//! 0xb1ac  call the joystick
//! 0xb1af  test bx, 0x10; jne ThrowDice     ; fire over a gadget
//! 0xb1b5  mov word ptr [DiceTHROW], 0
//! 0xb1bb  mov word ptr [0x783a], DD_ShakeDice   ; round again
//! ThrowDice 0xb1c4
//! 0xb1cb  the gadget under the pointer, or back to the shake
//! 0xb1d2  cmp word ptr es:[si+0xe], 1; je SetBET
//! 0xb1d9  cmp word ptr es:[si+0xe], 2; jne back      ; 2 is `XFL`, leaving
//! SetBET 0xb1e8
//! 0xb1e8  mov [BET], es:[si+0x10]          ; the gadget's own stake
//! 0xb1f4  cmp ax, [si+0x32]; jg back       ; more than the purse: refused
//! 0xb1fc  sub [si+0x32], ax                ; the stake is paid now
//! 0xb1ff  mov word ptr [DiceTHROW], 1
//! 0xb205  mov word ptr [0x783a], DD_ThrowDice
//! DiceDone 0xb20e
//! 0xb20e  mov word ptr [DiceTHROW], 2
//! 0xb214  mov word ptr [0x783a], 0         ; nothing follows the throw
//! ```
//!
//! and `DiceRND` at 0xb21d is `RollDice` and the payout, which is
//! [`crate::service`] and already built. So the **whole** of what was missing
//! is the state machine above: the hand shakes on a loop while a stake is
//! being chosen, plays the throw once when one is taken, and the three faces
//! are rolled and drawn on the frame that animation ends.
//!
//! **Nothing here is ours** but [`TICKS_PER_FRAME`], which is the same note
//! `crate::stones` carries: `TavernLoop` has no retrace count of its own, so
//! the circle's three is used rather than inventing a fourth number.

use crate::taskvm::{Frame, ScriptSet, Task, TaskActor};
use serde::{Deserialize, Serialize};

/// `Tav2` and `Tav3` at `DS:0xce17` and `DS:0xce20`, which `load_DiceBACK`
/// (0xaffd) loads: `dice.piv` for the picture and `dice.cel` into
/// `DiceHANDLE`.
pub const BANKS: &str = "dice";

/// The two scripts, `DS:0xce59` and `DS:0xce6d`.
pub const SHAKE: &str = "DD_ShakeDice";
pub const THROW: &str = "DD_ThrowDice";

/// Where the task stands: `ax = 0xa0`, `bx = 0`, `cx = 0x64`, `dh = 1`, which
/// is the same placement `ShakeDice` and the stone circle both use.
pub const TASK_X: i32 = 0xa0;
pub const TASK_Y: i32 = 0;
pub const TASK_Z: i32 = 0x64;
pub const TASK_FACING: u8 = 1;

/// **Ours, and the same one `crate::stones` marks.** `TavernLoop` waits on
/// whatever `0x9702` and `0x975b` cost it and names no retrace count, so the
/// circle's three retraces a frame stands here rather than a number invented
/// for this screen alone.
pub const TICKS_PER_FRAME: u32 = 3;

/// `DiceTHROW` at `DS:0xd119`, which is the whole state machine.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Throw {
    /// 0: the hand is shaking and a stake has not been taken.
    #[default]
    Shaking,
    /// 1: a stake was taken, the purse has already paid it, and
    /// `DD_ThrowDice` is running.
    Throwing,
    /// 2: `DiceDone` at 0xb20e. The faces are rolled and drawn on this frame.
    Landed,
}

/// The dice table, running.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Table {
    /// `DiceTHROW`.
    pub throw: Throw,
    /// The one task `ShakeDice` adds, which `0x783a` re-points at whichever of
    /// the two scripts comes next.
    pub task: Task,
    actor: TaskActor,
    /// Ticks since the last frame, against [`TICKS_PER_FRAME`].
    held: u32,
    /// A stake taken and paid for, waiting for the handler.
    ///
    /// The original has no such flag because it does not need one:
    /// `ThrowDice` is reachable **only** from the end-of-animation handler at
    /// 0xb1a1, so `DiceTHROW` goes from 0 to 1 and `0x783a` is pointed at
    /// `DD_ThrowDice` in the same breath and never while the shake is still
    /// running. A gadget here is pressed on whatever tick the pointer is over
    /// it, so the press is held until the next `ff ff` and the two writes
    /// happen together there, which is the same order.
    pending: bool,
}

impl Default for Table {
    fn default() -> Table {
        Table::new()
    }
}

impl Table {
    /// `0xb12e`: `DiceTHROW = 0`, then `ShakeDice`.
    pub fn new() -> Table {
        let mut task = Task::new(SHAKE, TASK_X, TASK_Y, TASK_FACING);
        task.z = TASK_Z;
        Table {
            throw: Throw::Shaking,
            task,
            actor: TaskActor::default(),
            held: 0,
            pending: false,
        }
    }

    /// A stake has been taken. `SetBET` at 0xb1e8: the purse has paid, so all
    /// that is left is `DiceTHROW = 1` and the next script.
    ///
    /// The original can only reach this from the end-of-animation handler, so
    /// the throw always starts on a frame boundary; this does the same by
    /// waiting for the shake to come round rather than cutting it off.
    pub fn stake_taken(&mut self) {
        if self.throw == Throw::Shaking {
            self.pending = true;
        }
    }

    /// Whether a stake has been taken at all, which is what stops a second
    /// one: `ThrowDice` is only reached while `DiceTHROW` is zero.
    pub fn staked(&self) -> bool {
        self.pending || self.throw != Throw::Shaking
    }

    /// Whether the faces are down: `cmp word ptr [DiceTHROW], 2` at 0xb166,
    /// which is what sends `TavernLoop` to `DiceRND`.
    pub fn landed(&self) -> bool {
        self.throw == Throw::Landed
    }

    /// One tick of `TavernLoop`. Returns the frame on the tick the loop
    /// stepped its task, and nothing on the two ticks in three that are wait.
    pub fn tick(&mut self, set: &ScriptSet) -> Option<Frame> {
        if self.landed() {
            return None;
        }
        self.held += 1;
        if self.held < TICKS_PER_FRAME {
            return None;
        }
        self.held = 0;
        let frame = self.task.step(set, &mut self.actor, false);
        if frame.finished {
            // The handler at 0xb1a1, in its own order.
            match self.throw {
                // `cmp word ptr [DiceTHROW], 0; jne DiceDone` at 0xb1a5: the
                // throw has played, so the dice are down.
                Throw::Throwing => self.throw = Throw::Landed,
                // `SetBET` at 0xb1e8, which is where `ThrowDice` ends up when
                // fire was over a stake: `DiceTHROW = 1` and `0x783a` pointed
                // at `DD_ThrowDice`, both here and neither anywhere else.
                Throw::Shaking if self.pending => {
                    self.pending = false;
                    self.throw = Throw::Throwing;
                    self.task.replace(THROW);
                }
                // `mov word ptr [0x783a], DD_ShakeDice` at 0xb1bb.
                Throw::Shaking => self.task.replace(SHAKE),
                Throw::Landed => {}
            }
        }
        Some(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taskvm::{End, Instr, Part, Script};
    use std::collections::BTreeMap;

    fn part(cel: u8) -> Instr {
        Instr::Part(Part {
            table: 1,
            bank: 0,
            cel,
            x: 0,
            y: 0,
            flags: 0,
        })
    }

    /// Two stand-ins with the shipped scripts' own shapes: the shake is two
    /// frames and the throw fourteen, and both end on `ff ff`.
    fn set() -> ScriptSet {
        let mut s: ScriptSet = BTreeMap::new();
        s.insert(
            SHAKE.into(),
            Script::new(vec![
                part(6),
                Instr::EndFrame { end: End::Next },
                part(7),
                Instr::EndFrame { end: End::Stop },
            ]),
        );
        let mut throw = Vec::new();
        for cel in 8..22u8 {
            throw.push(part(cel));
            throw.push(Instr::EndFrame {
                end: if cel == 21 { End::Stop } else { End::Next },
            });
        }
        s.insert(THROW.into(), Script::new(throw));
        s
    }

    fn frames(t: &mut Table, set: &ScriptSet, n: usize) {
        for _ in 0..n * TICKS_PER_FRAME as usize {
            t.tick(set);
        }
    }

    /// Three ticks to a frame, the way `crate::stones` counts them.
    #[test]
    fn the_loop_steps_once_every_three_ticks() {
        let set = set();
        let mut t = Table::new();
        assert!(t.tick(&set).is_none());
        assert!(t.tick(&set).is_none());
        assert!(t.tick(&set).is_some(), "the third tick is the frame");
    }

    /// `0xb1bb`: the shake is re-pointed at itself every time it ends, so it
    /// never stops on its own and nothing about it ever lands.
    #[test]
    fn the_shake_goes_round_for_ever_until_a_stake_is_taken() {
        let set = set();
        let mut t = Table::new();
        frames(&mut t, &set, 200);
        assert_eq!(t.throw, Throw::Shaking);
        assert!(!t.landed(), "a shake never lands the dice");
        assert_eq!(t.task.pc.script, SHAKE);
        assert!(t.task.running);
    }

    /// `SetBET` then the handler: the throw starts on the next frame boundary,
    /// runs its own length once, and `DiceDone` marks it landed.
    #[test]
    fn a_stake_taken_throws_once_and_then_lands() {
        let set = set();
        let mut t = Table::new();
        frames(&mut t, &set, 3);
        t.stake_taken();
        assert!(t.staked(), "the stake is paid and the handler will take it");
        assert_eq!(
            t.throw,
            Throw::Shaking,
            "`DiceTHROW` is still zero until the shake reaches its `ff ff`"
        );
        assert!(!t.landed(), "the throw has not played yet");
        // The shake finishes its frame, the throw is swapped in, and the
        // fourteen frames of it run.
        for _ in 0..60 {
            if t.landed() {
                break;
            }
            frames(&mut t, &set, 1);
        }
        assert!(t.landed(), "`DiceDone` sets `DiceTHROW` to 2");
        assert_eq!(t.throw, Throw::Landed);
        assert!(t.tick(&set).is_none(), "and nothing steps after that");
    }

    /// The throw plays its whole length before it lands, which is what makes
    /// it an animation rather than a flag.
    #[test]
    fn the_throw_is_more_than_one_frame_long() {
        let set = set();
        let mut t = Table::new();
        t.stake_taken();
        let mut played = 0;
        while !t.landed() && played < 200 {
            frames(&mut t, &set, 1);
            played += 1;
        }
        assert!(t.landed());
        assert!(
            played > 10,
            "the throw is fourteen frames in the shipped script, not one"
        );
    }

    /// A stake cannot be taken twice: `ThrowDice` is only reachable from the
    /// handler while `DiceTHROW` is zero.
    #[test]
    fn a_second_stake_is_ignored_while_the_dice_are_in_the_air() {
        let set = set();
        let mut t = Table::new();
        t.stake_taken();
        frames(&mut t, &set, 4);
        assert_eq!(t.throw, Throw::Throwing, "the handler took the first");
        t.stake_taken();
        assert_eq!(t.throw, Throw::Throwing, "and a second is not taken at all");
        assert!(t.staked());
    }
}
