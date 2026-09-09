//! The stone circle's set piece: `HengeControl`, `HengeLOOP` and the thunder.
//!
//! `MOON:Henge` (image 0x1053) is two halves. The first is the four tests that
//! end the game, which are in [`crate::service`]. The second is what happens
//! when the night's moon is not yours:
//!
//! ```text
//! 0x1088  mov si, HengeInstruct; call INSTRUCTMESSAGE
//! 0x108e  call WaitFIRE
//! 0x1091  call the fade
//! 0x1094  mov word ptr [0xf378], 0xffff      ; nothing offered yet
//! 0x109a  mov ax, 3; call ColourStatus       ; the offering page
//! 0x10a0  cmp word ptr [0xf378], -1; je      ; nothing offered: leave
//! 0x10a7  call 0xb35e                        ; the set piece, below
//! 0x10aa  cmp byte ptr [si+0x31], 5; je      ; a life point, capped at five
//! 0x10b4  add byte ptr [si+0x31], 1
//! 0x10b8  call 0x28d                         ; max health = 10*con + armour + 10
//! 0x10bb  mov ax, [si+0x3c]; mov [si+0x38], ax   ; and full again
//! 0x10c1  mov byte ptr [si+0x60], 0          ; the ratman's bite lifted
//! ```
//!
//! and `0xb35e`, in `_TAVERN`, is the set piece itself:
//!
//! ```text
//! 0xb372  mov si, HengeWait; call INSTRUCTMESSAGE   ; `The druids prepare / for the ritual`
//! 0xb37d  mov dx, HengeFILE1; call LoadScreen       ; `Hen1.p`
//! 0xb383  mov ah, 0; int 60h                        ; the tune starts
//! 0xb398  mov dx, HengeFILE2; call ObjLoadV         ; `Hen1.c` into DiceHANDLE
//! 0xb39e  mov si, 0x6964; mov [si+0x14], HengeControl
//! 0xb3a6  si = Torches;           bp = DiceHANDLE; ax = 0xa0; bx = 0; cx = 0x64; dh = 1; dl = 0x14; ADDTASK
//! 0xb3bc  si = Knight_LiftMagic;  bp = DiceHANDLE; ax = 0xa0; bx = 0; cx = 0x64; dh = 1; dl = 0x14; ADDTASK
//! 0xb3e1  mov si, 0x80bb; mov ax, 0x10; call ColourEn4Knight
//! 0xb3ea  fade in
//! 0xb3f0  mov word ptr [HengeFLAG], 0
//! HengeLOOP 0xb3f6
//!         update the tasks; if HengeFLAG != 0 -> out
//!         draw; mov ax, 3; call 0xafeb        ; three vertical retraces a frame
//!         jmp HengeLOOP
//! 0xb414  mov ah, 2; int 60h                  ; the tune stops
//! ```
//!
//! **`HengeControl` is an end-of-animation handler, not a controller.**
//! `0xb39e` writes it into slot `0x14` of the table at `DS:0x6964`, and `0x14`
//! is the `dl` both `ADDTASK` calls are given, so it is the routine the engine
//! calls when one of these two animations reaches its `ff ff`. All it does is
//! `mov word ptr [HengeFLAG], 1`, which is what ends `HengeLOOP`. So the set
//! piece lasts exactly as long as `Knight_LiftMagic`, and [`Stones::done`] is
//! that task's `running` flag going false.
//!
//! **The thunder is in the script.** `Knight_LiftMagic` (`DS:0xcf41`) is the
//! knight raising the offering: `Hen1.c` cels 0 to 6 for the arms and the item,
//! then `TASKLOOP 5` around five frames that add cels 25, 26 and 27 at `y -100`,
//! a bolt above his head, five times over, with
//! `TASKGOSUB HengeThunderSnd` on the first of them. `HengeThunderSnd` at image
//! 0xb419 is a bare `ret`: this build makes no sound there, and none is
//! invented here. `Torches` (`DS:0xd051`) is three frames of ten flames, cels 7
//! to 24, that go round for ever on a `TASKGOTO` back to its own head.
//!
//! **Nothing here is ours** but the note above about the frame rate, and the
//! one thing the original gets wrong, which is reproduced rather than fixed:
//! see [`KNIGHT_INK`].

use crate::taskvm::{Frame, ScriptSet, Task, TaskActor};
use serde::{Deserialize, Serialize};

/// `HengeFILE1` and `HengeFILE2`, `Hen1.p` and `Hen1.c`.
pub const PLATE: &str = "scene.hen1";
pub const PALETTE: &str = "palette.scene.hen1";
/// The bank table `ADDTASK` is handed, which is one bank and only one.
pub const BANKS: &str = "henge";

/// The two scripts, in the order `0xb3a6` and `0xb3bc` start them, which is
/// also the order they are drawn in.
pub const TORCHES: &str = "Torches";
pub const LIFT: &str = "Knight_LiftMagic";

/// Where both tasks stand: `ax = 0xa0`, `bx = 0`, `cx = 0x64`, `dh = 1`.
pub const TASK_X: i32 = 0xa0;
pub const TASK_Y: i32 = 0;
pub const TASK_Z: i32 = 0x64;
pub const TASK_FACING: u8 = 1;

/// `HengeLOOP` waits three vertical retraces a frame (`mov ax, 3; call 0xafeb`,
/// and `0xafeb` is `mov cx, ax` round the retrace wait). This engine's tick is
/// a retrace, so three ticks is the frame and no rounding is needed.
pub const TICKS_PER_FRAME: u32 = 3;

/// `ColourEn4Knight` (0xb4b2), which writes the winner's armour into `Hen1.p`'s
/// palette: `add si, ax` with `si` the live palette and `ax` 0x10, so four
/// words at entries 8 to 11.
pub const KNIGHT_FIRST: usize = 8;

/// The four words, by the knight's `[di+0x20]`.
///
/// **The original's third branch is wrong and it is reproduced.** The routine
/// tests `[di+0x20]` for 0, then 1, then **`[si+0x20]`** for 3, then `[di+0x20]`
/// for 2. `si` is the palette pointer, so the third test reads palette entry 24
/// of whatever picture is loaded instead of the knight's index, and `Hen1.p`'s
/// entry 24 is `0x00f`. It is never 3, so **seat 3 falls through every branch
/// and is given no colours at all**. It does not show: `Hen1.p` ships with
/// `0xf00, 0xa00, 0x600, 0x300` already in 8 to 11, which is the red knight to
/// within one shade of the `0xe00, 0x900, 0x600, 0x300` the branch would have
/// written. [`knight_ink`] returns nothing for seat 3, and the plate's own
/// entries stand, exactly as they do there.
// Hand-aligned: one seat per row, in the order the routine tests them.
#[rustfmt::skip]
pub const KNIGHT_INK: [(u8, [u16; 4]); 3] = [
    (0, [0x05d, 0x028, 0x016, 0x003]),  // blue
    (1, [0xfa0, 0xb40, 0x930, 0x710]),  // gold
    (2, [0x0c5, 0x082, 0x061, 0x040]),  // green
];

/// The four words for a seat, or nothing where the routine writes none.
pub fn knight_ink(knight: u8) -> Option<[u16; 4]> {
    KNIGHT_INK
        .iter()
        .find(|(seat, _)| *seat == knight)
        .map(|(_, w)| *w)
}

/// The set piece, running.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Stones {
    /// The winner's `[di+0x20]`, which is his colour index.
    pub knight: u8,
    pub torches: Task,
    pub lift: Task,
    actor: TaskActor,
    /// Ticks since the last frame was stepped, against [`TICKS_PER_FRAME`].
    held: u32,
    /// `HengeFLAG`, which `HengeControl` sets when the lift's animation ends.
    pub flag: bool,
}

impl Stones {
    /// Both tasks placed and started, the way `0xb3a6` and `0xb3bc` place them.
    pub fn new(knight: u8) -> Stones {
        let task = |script: &str| {
            let mut t = Task::new(script, TASK_X, TASK_Y, TASK_FACING);
            t.z = TASK_Z;
            t
        };
        Stones {
            knight,
            torches: task(TORCHES),
            lift: task(LIFT),
            actor: TaskActor::default(),
            held: 0,
            flag: false,
        }
    }

    /// One tick. Returns the two frames when the loop stepped its tasks this
    /// tick, and nothing on the two ticks in three that are only the wait.
    pub fn tick(&mut self, set: &ScriptSet) -> Option<[Frame; 2]> {
        if self.flag {
            return None;
        }
        self.held += 1;
        if self.held < TICKS_PER_FRAME {
            return None;
        }
        self.held = 0;
        // `0x9702` walks the task list in the order the tasks were added, so
        // the torches step before the knight does.
        let torches = self.torches.step(set, &mut self.actor, false);
        let lift = self.lift.step(set, &mut self.actor, false);
        // `HengeControl`: the end of either animation raises the flag, and the
        // torches never end.
        if torches.finished || lift.finished {
            self.flag = true;
        }
        Some([torches, lift])
    }

    /// Whether `HengeLOOP` has left.
    pub fn done(&self) -> bool {
        self.flag
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

    /// A stand-in pair: torches that go round for ever and a lift that ends.
    fn set() -> ScriptSet {
        let mut s: ScriptSet = BTreeMap::new();
        s.insert(
            TORCHES.into(),
            Script::new(vec![
                part(7),
                Instr::EndFrame { end: End::Next },
                part(8),
                Instr::Goto {
                    mode: 0,
                    target: TORCHES.into(),
                },
                Instr::EndFrame { end: End::Stop },
            ]),
        );
        s.insert(
            LIFT.into(),
            Script::new(vec![
                part(1),
                Instr::EndFrame { end: End::Next },
                part(2),
                Instr::EndFrame { end: End::Stop },
            ]),
        );
        s
    }

    /// Three ticks to a frame, because `HengeLOOP` waits three retraces.
    #[test]
    fn the_loop_steps_once_every_three_ticks() {
        let set = set();
        let mut s = Stones::new(0);
        assert!(s.tick(&set).is_none());
        assert!(s.tick(&set).is_none());
        assert!(s.tick(&set).is_some(), "the third tick is the frame");
    }

    /// `HengeControl` ends the loop when the lift's animation does, and the
    /// torches, which never end, do not end it.
    #[test]
    fn the_lift_ending_is_what_ends_the_loop() {
        let set = set();
        let mut s = Stones::new(0);
        let mut frames = 0;
        while !s.done() && frames < 1_000 {
            if s.tick(&set).is_some() {
                frames += 1;
            }
        }
        assert!(s.done(), "the loop leaves");
        assert!(!s.lift.running, "because the lift ran out");
        assert!(s.torches.running, "the torches are still going round");
        assert!(s.tick(&set).is_none(), "and nothing steps after that");
    }

    /// The three seats the routine actually writes, and the fourth it does not.
    #[test]
    fn seat_three_is_given_no_colours_because_the_original_gives_it_none() {
        assert_eq!(knight_ink(0), Some([0x05d, 0x028, 0x016, 0x003]));
        assert_eq!(knight_ink(1), Some([0xfa0, 0xb40, 0x930, 0x710]));
        assert_eq!(knight_ink(2), Some([0x0c5, 0x082, 0x061, 0x040]));
        assert_eq!(knight_ink(3), None, "`cmp [si+0x20], 3` never matches");
    }
}
