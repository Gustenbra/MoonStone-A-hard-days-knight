//! The ballistic arc a creature is thrown along: `CalcJUMP`, `ADDJUMP` and
//! `ControlJump`.
//!
//! This is the original's own jump engine, not the task VM's `TASKJUMP` (which
//! is a different thing, lives in [`crate::taskvm`], and only `Beast_BackToss`
//! uses). It is a six slot table at DS:`0x76b2`, stride `0x14`, filled from a
//! twenty byte template at DS:`0x77cc`, and stepped once per frame by whoever
//! set it going. Two creatures use it: the ratman, for its leap into a tree
//! and at the knight (`RatmanLeap` 0x31a9, `RatmanInitLeap` 0x3215,
//! `RatmanLeaping` 0x3270, `RatmanGouged` 0x3395), and Balok, for its hop
//! (`BalokJump` 0x366f, `BalokJumping` 0x36d1).
//!
//! The template, as the two callers write it and `ADDJUMP` reads it:
//!
//! | offset | what |
//! |---|---|
//! | `+0` | the actor the jump belongs to |
//! | `+4` | `x0`, the column it starts at (`+2` of the record) |
//! | `+6` | `z0`, the depth (`+6`) |
//! | `+8` | `y0`, the height (`+4`), negative upward |
//! | `+0xa` | `x1`, the column it lands at |
//! | `+0xc` | `z1` |
//! | `+0xe` | `y1` |
//! | `+0x10` | how many frames the arc lasts |
//! | `+0x12` | how high a flat hop rises, in pixels (`NORM` only) |
//!
//! and the live slot, which is what [`Jump`] is:
//!
//! | offset | what |
//! |---|---|
//! | `+4` | frames left |
//! | `+6` | the vertical speed, 8.8 fixed point |
//! | `+8` | what comes off it each frame, the gravity |
//! | `+0xa`, `+0xc` | the horizontal and depth speeds, 10.6 fixed point |
//! | `+0xe`, `+0x10` | the horizontal and depth positions, 10.6 |
//! | `+0x12` | the vertical position, 8.8 |
//!
//! Every store in the original is a 16 bit word and every divide is a 16 bit
//! `idiv`, so every store here is truncated to `i16` rather than left wide. A
//! `xchg bh, bl; xor bl, bl` is `(v & 0xff) << 8`, which is `v * 256` for
//! anything inside a byte and wraps outside it; that is written out as it
//! stands rather than tidied into a multiply.

use serde::{Deserialize, Serialize};

/// A 16 bit word, the way every one of these stores lands.
fn w(v: i32) -> i32 {
    v as i16 as i32
}

/// `xchg bh, bl; xor bl, bl`: the low byte moved into the high one.
fn hi(v: i32) -> i32 {
    w((v & 0xff) << 8)
}

/// `shl bx, 1` six times.
fn shl6(v: i32) -> i32 {
    w(v << 6)
}

/// `div bx` after `xor dx, dx` (or, at 0x2c60, after a `cdq` whose sign
/// extension the divide then ignores): unsigned, into a word.
fn udiv(num: i32, den: i32) -> i32 {
    if den <= 0 {
        return 0;
    }
    w(((num as u16 as u32) / (den as u16 as u32)) as i32)
}

/// `idiv bx` after `cdq`: signed, truncating toward zero, into a word.
fn idiv(num: i32, den: i32) -> i32 {
    if den == 0 {
        // The original would fault. Nothing that reaches here can be zero:
        // every `steps` is at least four (`CalcJUMP`'s floor) and `NORM`
        // halves a number it has just added one to. Kept total anyway,
        // because a divide by zero in a pure crate is a panic in a fight.
        return 0;
    }
    w(num / den)
}

/// The template at DS:`0x77cc`, filled by `CalcJUMP` or by a controller
/// writing the fields itself.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub x0: i32,
    pub z0: i32,
    pub y0: i32,
    pub x1: i32,
    pub z1: i32,
    pub y1: i32,
    /// `+0x10`.
    pub steps: i32,
    /// `+0x12`.
    pub rise: i32,
}

/// One of the six live jumps in the table at DS:`0x76b2`.
///
/// Held on the creature that is jumping rather than in a table of six, for
/// the same reason [`crate::monster::Brain`] is: it is that creature's state,
/// it serializes with the rest of the fight, and it goes into the fingerprint.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Jump {
    /// `+4`.
    pub steps: i32,
    /// `+6`.
    pub yvel: i32,
    /// `+8`.
    pub grav: i32,
    /// `+0xa`, `+0xc`.
    pub xvel: i32,
    pub zvel: i32,
    /// `+0xe`, `+0x10`, `+0x12`.
    pub xpos: i32,
    pub zpos: i32,
    pub ypos: i32,
}

/// What `CalcJUMP` (0x2a8e) leaves behind besides the template: the reach it
/// measured and the two numbers `BalokJump` and `RatmanInitLeap` read back out
/// of DS:`0x7796` and DS:`0x7798`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Aim {
    pub plan: Plan,
    /// DS:`0x76b0`: the larger of the two gaps, which `RatmanInitLeap`
    /// compares with forty.
    pub reach: i32,
    /// DS:`0x7796`, the frame count, floored at four.
    pub steps: i32,
    /// DS:`0x7798`, the rise, floored at three (or set to ten when the frame
    /// count had to be floored).
    pub rise: i32,
}

/// `CalcJUMP`, image 0x2a8e: aim a jump at the opponent and size it.
///
/// ```text
/// 02a8e  mov di, [0x77e8]          ; me
/// 02a97  mov ax, [di+2]            ; me.x
/// 02a9a  mov bx, [si+2]            ; foe.x
/// 02a9e  sub bx, ax
/// 02aa0  js  02aa4                 ; foe is to the left: keep bp
/// 02aa2  neg bp                    ; foe is to the right: land short of him
/// 02aa8  mov [si], di              ; template+0    the actor
/// 02aad  mov ax, [di+2]; mov [si+4], ax    ; x0
/// 02ab5  mov ax, [di+6]; mov [si+6], ax    ; z0
/// 02abd  mov ax, [di+4]; mov [si+8], ax    ; y0
/// 02ac6  mov bx, [0x77ea]                  ; the opponent
/// 02aca  mov ax, [bx+2]; mov [si], ax; add [si], bp   ; x1 = foe.x +- bp
/// 02ad4  mov ax, [bx+2]; mov [di+0x5c], ax; add [di+0x5c], bp
/// 02add  mov ax, [bx+6]; mov [si], ax      ; z1 = foe.z
/// 02ae5  mov ax, [bx+6]; mov [si+0x5e], ax
/// 02aee  mov ax, [bx+4]; mov [0x77da], ax  ; y1 = foe.y
/// 02afe  mov si, 0x77cc
/// 02b01  mov ax, [si+4]; mov bx, [si+0xa]; sub bx, ax; jns; neg bx
/// 02b0d  mov [0x76b0], bx                  ; |dx|
/// 02b11  mov ax, [si+6]; mov bx, [si+0xc]; sub bx, ax; jns; neg bx
/// 02b1d  cmp bx, [0x76b0]; jl; mov [0x76b0], bx      ; the larger of the two
/// 02b27  mov ax, [0x76b0]; mov bx, ax
/// 02b2c  shr ax, 1;    mov [0x7798], ax    ; the rise is half the gap
/// 02b31  shr bx, 1 x3; mov [0x7796], bx    ; the frame count is an eighth
/// 02b3b  cmp [0x7798], 2; jg; mov [0x7798], 3
/// 02b48  cmp [0x7796], 4; jg; mov [0x7796], 4; mov [0x7798], 0xa
/// 02b5b  mov ax, [0x7796]; mov [si+0x10], ax
/// 02b61  mov ax, [0x7798]; mov [si+0x12], ax
/// 02b67  mov ax, [0x7796]; mov [di+0x4a], ax
/// ```
///
/// `bp` is the caller's stand-off: Balok passes 0x50, the ratman its own
/// approach range `+0x52`. `+0x4a` takes the frame count on the way out,
/// which is the cooldown the two callers then read.
///
/// The negate at 0x2aa2 is `js` on `foe.x - me.x`, so the sign flips when the
/// opponent is to the **right**, which is what puts the landing spot on this
/// side of him either way.
pub fn calc(me: (i32, i32, i32), foe: (i32, i32, i32), stand_off: i32) -> Aim {
    let (mx, mz, my) = me;
    let (fx, fz, fy) = foe;
    // 02a9e  sub bx, ax / 02aa0 js / 02aa2 neg bp
    let bp = if fx - mx < 0 { stand_off } else { -stand_off };
    let mut plan = Plan {
        x0: mx,
        z0: mz,
        y0: my,
        x1: w(fx + bp),
        z1: fz,
        y1: fy,
        steps: 0,
        rise: 0,
    };
    // 02b01: |x1 - x0|, then |z1 - z0|, and the larger of the two.
    let mut reach = (plan.x1 - plan.x0).abs();
    let dz = (plan.z1 - plan.z0).abs();
    if dz >= reach {
        reach = dz;
    }
    // 02b2c: the rise is half of it, the frame count an eighth, both floored.
    let mut rise = reach >> 1;
    let mut steps = reach >> 3;
    if rise <= 2 {
        rise = 3;
    }
    if steps <= 4 {
        steps = 4;
        rise = 10;
    }
    plan.steps = steps;
    plan.rise = rise;
    Aim {
        plan,
        reach,
        steps,
        rise,
    }
}

impl Plan {
    /// `ADDJUMP`, image 0x2b8d, from `FFREE` (0x2bac) on: turn the template
    /// into a live slot.
    ///
    /// The search for a slot above it is not reproduced; a jump is held on
    /// the creature that is jumping, so there is always exactly one free.
    ///
    /// ```text
    /// 02bc3  mov ax, [di+8]; sub ax, [di+0xe]; jge JT; neg ax
    /// 02bcf  cmp ax, 5; jle NORM              ; a rise of five or less is flat
    ///
    /// 02bd4  mov ax, [di+0]; mov [si], ax     ; slot+0  the actor
    /// 02bdb  mov ax, [di+0x10]; mov [si], ax  ; slot+4  the frame count
    /// 02be3  mov ax, [di+0x10]                ; steps
    /// 02be6  mov bx, [di+8]; sub bx, [di+0xe] ; y0 - y1
    /// 02bec  jl  JUMPDOWN
    /// 02bee  xchg bh, bl; xor bl, bl          ; (dy & 0xff) << 8
    /// 02bf2  xchg bx, ax; cdq; idiv bx        ; / steps
    /// 02bf6  xchg bx, ax; add bx, bx          ; doubled
    /// 02bf9  mov [si], bx                     ; slot+6  the speed
    /// 02bfe  sub ax, 1                        ; steps - 1
    /// 02c01  xchg bx, ax; cdq; idiv bx
    /// 02c05  xchg bx, ax; mov [si], bx        ; slot+8  the gravity
    /// 02c0b  jmp REST
    /// JUMPDOWN:
    /// 02c0d  mov word [si], 0                 ; slot+6  no speed at all
    /// 02c14  xchg bh, bl; xor bl, bl
    /// 02c18  xchg bx, ax; cdq; idiv bx
    /// 02c1c  xchg bx, ax; add bx, bx
    /// 02c1f  sub ax, 1
    /// 02c22  xchg bx, ax; cdq; idiv bx
    /// 02c26  xchg bx, ax; neg bx; mov [si], bx  ; slot+8, the sign turned
    /// NORM:
    /// 02c30  mov ax, [di+0]; mov [si], ax     ; slot+0
    /// 02c37  mov ax, [di+0x10]; mov [si], ax  ; slot+4
    /// 02c3f  mov ax, [di+0x10]; add ax, 1; shr ax, 1   ; half the frames
    /// 02c47  mov bx, [di+0x12]; xchg bh, bl; xor bl, bl ; the rise, << 8
    /// 02c4e  xchg bx, ax; xor dx, dx; div bx  ; unsigned, unlike the others
    /// 02c53  xchg bx, ax; add bx, bx; mov [si], bx     ; slot+6
    /// 02c5b  sub ax, 1; xchg bx, ax; cdq; idiv bx
    /// 02c62  xchg bx, ax; mov [si], bx                 ; slot+8
    /// REST:
    /// 02c68  mov ax, [di+0x10]                 ; steps
    /// 02c6b  mov bx, [di+0xa]; sub bx, [di+4]; shl bx, 1 x6
    /// 02c7d  xchg bx, ax; cdq; idiv bx; xchg bx, ax
    /// 02c82  mov [si], bx                      ; slot+0xa  the x speed
    /// 02c87  mov bx, [di+0xc]; sub bx, [di+6]; shl bx, 1 x6
    /// 02c99  xchg bx, ax; cdq; idiv bx; xchg bx, ax; mov [si], bx  ; slot+0xc
    /// 02ca3  mov ax, [di+4];  shl ax, 1 x6; mov [si], ax  ; slot+0xe
    /// 02cb7  mov ax, [di+6];  shl ax, 1 x6; mov [si], ax  ; slot+0x10
    /// 02ccb  mov ax, [di+8];  xchg ah, al; xor al, al; mov [si], ax ; slot+0x12
    /// ```
    ///
    /// The two branches differ in more than a sign. Jumping **up** starts with
    /// a speed and loses it; jumping **down** starts at rest and gains, and
    /// its gravity is `2 * dy / steps / (steps - 1)` negated, which is not the
    /// same number as the upward form's. `NORM`, the flat hop, is the only one
    /// that divides unsigned and the only one that reads the template's
    /// `+0x12`.
    pub fn start(&self) -> Jump {
        let steps = self.steps;
        let dy = self.y0 - self.y1;
        let (yvel, grav) = if dy.abs() <= 5 {
            // NORM, 0x2c30. Both divides here are `div`, not `idiv`: the
            // second one is preceded by a `cdq` (0x2c5f) whose sign extension
            // an unsigned divide then ignores, which is the same number for
            // every rise the two callers pass.
            let half = (steps + 1) >> 1;
            let yvel = w(udiv(hi(self.rise), half) * 2);
            (yvel, udiv(yvel, half - 1))
        } else if dy >= 0 {
            // Up, 0x2bee.
            let yvel = w(idiv(hi(dy), steps) * 2);
            (yvel, idiv(yvel, steps - 1))
        } else {
            // JUMPDOWN, 0x2c0d.
            let q = w(idiv(hi(dy), steps) * 2);
            (0, w(-idiv(q, steps - 1)))
        };
        Jump {
            steps,
            yvel,
            grav,
            xvel: idiv(shl6(self.x1 - self.x0), steps),
            zvel: idiv(shl6(self.z1 - self.z0), steps),
            xpos: shl6(self.x0),
            zpos: shl6(self.z0),
            ypos: hi(self.y0),
        }
    }
}

/// Where a jump has got to, and whether it has landed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Step {
    pub x: i32,
    pub z: i32,
    pub y: i32,
    pub done: bool,
}

impl Jump {
    /// `ControlJump`/`CONJUMP`, image 0x2cde, from `FNDIT` (0x2cf5) on.
    ///
    /// ```text
    /// 02cf6  mov bx, [si+6]        ; the speed, before this frame's gravity
    /// 02cf9  mov ax, [si+8]
    /// 02cfc  sub [si+6], ax        ; speed -= gravity
    /// 02cff  sub [si+0x12], bx     ; height -= the *old* speed
    /// 02d02  mov ax, [si+0xa]; add [si+0xe], ax
    /// 02d08  mov ax, [si+0xc]; add [si+0x10], ax
    /// 02d0e  mov bx, [si+0xe]; sar bx, 1 x6    ; x
    /// 02d1d  mov cx, [si+0x10]; sar cx, 1 x6   ; z
    /// 02d2c  mov dx, [si+0x12]; sar dx, 1 x8   ; y
    /// 02d3f  sub word [si+4], 1
    /// 02d43  jne NOTEND
    /// 02d45  mov word [si], 0      ; the slot is free again
    /// 02d49  mov ax, 1             ; and the caller is told it landed
    /// ```
    ///
    /// The height is stepped by the speed as it was **before** the gravity
    /// came off it, which is one frame of lead the arc keeps all the way
    /// through. The frame count is decremented after the move, so the last
    /// frame is taken and then the jump ends.
    pub fn step(&mut self) -> Step {
        let before = self.yvel;
        self.yvel = w(self.yvel - self.grav);
        self.ypos = w(self.ypos - before);
        self.xpos = w(self.xpos + self.xvel);
        self.zpos = w(self.zpos + self.zvel);
        let x = self.xpos >> 6;
        let z = self.zpos >> 6;
        let y = self.ypos >> 8;
        self.steps = w(self.steps - 1);
        Step {
            x,
            z,
            y,
            done: self.steps == 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `CalcJUMP`'s two floors, and which way the stand-off is applied.
    #[test]
    fn an_aim_lands_short_of_the_opponent_on_whichever_side_he_is() {
        // The opponent to the right: `js` is taken on a positive difference
        // being tested for sign... it is not, so `bp` is negated and the
        // landing spot is eighty short of him.
        let a = calc((0, 100, 0), (200, 100, 0), 80);
        assert_eq!(
            a.plan.x1, 120,
            "0x2aa2: neg bp when the foe is to the right"
        );
        // And to the left, the landing spot is eighty to his right.
        let b = calc((200, 100, 0), (0, 100, 0), 80);
        assert_eq!(b.plan.x1, 80);
        // The reach is the larger of the two gaps, and the two figures come
        // off it by a shift each.
        assert_eq!(a.reach, 120);
        assert_eq!(a.steps, 120 >> 3);
        assert_eq!(a.rise, 120 >> 1);
        // Close in, both floors bite, and the frame count floor takes the
        // rise with it: `mov [0x7798], 0xa`.
        let c = calc((0, 100, 0), (20, 100, 0), 8);
        assert_eq!(c.reach, 12);
        assert_eq!((c.steps, c.rise), (4, 10), "0x2b48 floors both together");
        // Depth counts as much as distance: the larger of the two wins.
        let d = calc((0, 0, 0), (10, 90, 0), 0);
        assert_eq!(d.reach, 90);
    }

    /// The arc rises and comes back down, and lands where it was aimed.
    #[test]
    fn a_jump_up_rises_and_returns_to_the_ground() {
        let plan = Plan {
            x0: 0,
            z0: 100,
            y0: 0,
            x1: 96,
            z1: 100,
            y1: 0,
            steps: 12,
            rise: 3,
        };
        let mut j = plan.start();
        let mut highest = 0;
        let mut last;
        let mut frames = 0;
        loop {
            last = j.step();
            frames += 1;
            highest = highest.min(last.y);
            if last.done || frames > 64 {
                break;
            }
        }
        assert_eq!(frames, 12, "the frame count is the template's");
        assert!(highest < 0, "it left the ground: {highest}");
        assert!(
            (last.x - 96).abs() <= 1,
            "and came down where it was aimed: {}",
            last.x
        );
    }

    /// `JUMPDOWN`: a fall starts at rest.
    #[test]
    fn a_jump_down_starts_at_rest() {
        let plan = Plan {
            x0: 0,
            z0: 100,
            y0: -60,
            x1: 60,
            z1: 100,
            y1: 0,
            steps: 10,
            rise: 3,
        };
        let mut j = plan.start();
        assert_eq!(j.yvel, 0, "0x2c0d: mov word [si], 0");
        assert!(j.grav > 0, "and gains speed downward: {}", j.grav);
        let first = j.step();
        assert_eq!(first.y, -60, "the first frame has not moved yet");
    }

    /// The flat hop reads the template's own rise and nothing else.
    #[test]
    fn a_flat_hop_rises_by_the_template_and_lands_level() {
        let plan = Plan {
            x0: 0,
            z0: 100,
            y0: 0,
            x1: 40,
            z1: 100,
            y1: 0,
            steps: 14,
            rise: 20,
        };
        let mut j = plan.start();
        assert!(j.yvel > 0, "NORM gives it a speed off `+0x12`");
        let mut highest = 0;
        for _ in 0..14 {
            highest = highest.min(j.step().y);
        }
        assert!(highest <= -18, "it cleared eighteen rows: {highest}");
    }
}
