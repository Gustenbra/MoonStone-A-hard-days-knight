//! The dragon over the map: where it flies, whom it is after, and when it
//! comes down.
//!
//! The dragon is not a creature that waits by the road. `_MAP:MapEffects`
//! (0xa508) puts it in the air at the start of every knight's turn from the
//! second day on, `DragonWander` (0xa66b) flies it back and forth across the
//! map at the row of a knight it picked at random, `CheckEncounterDone+128`
//! (0x816) notes every frame which knights its shadow is over, and
//! `DragonEncounter` (0xa3e2) starts the fight when the one it is after is
//! one of them. The routine at 0xcf6, which the `PUBLIC` list calls
//! `BATTLEDRAGON`, is the fight and its aftermath: `_dragon_won` (0xd23) for
//! a knight who fell and the write of `0xffff` into the dragon's `+0x31`
//! (0xd38) for one who did not, after which nothing puts it back in the air.
//!
//! Every word here is one of the original's, at DS:`0xcd50` to `0xcd5b`
//! (`DR_XADD` to `DR_WALK`), `0xccae` (`TrackCNT`), `0xccb0` and `0xccb2`
//! (aloft, and dead), the dragon record's `+0x46` (whom it is after) and the
//! four words at DS:`0x490` (whom it is over). The map's frame is the tick,
//! so a step of two is two pixels a tick.
//!
//! What this engine has that the original does not: one traveller. The
//! other three knights are in the roll all the same, as `ContinueDragon`
//! rolls them, and they stand at their home corners, since nothing here
//! moves them; a dragon after one of them flies at that corner's row and
//! never comes down on anybody.

use crate::monster::rnd;
use serde::{Deserialize, Serialize};

/// `MI.C` frame 0x14, the nine by five marker `CheckEncounterDone+133`
/// (0x821) measures the dragon's shadow by: `mov ax, 0x25; sub ax, 0x11`.
pub const SHADOW_FRAME: usize = 0x14;

/// That frame's size, which `GetWIDTH` (0x626) reads out of `MI.C`: nine
/// by five, the same marker `DisplayLairs` puts on every lair.
pub const SHADOW_SIZE: (i32, i32) = (9, 5);

/// The `DR_*` words and the flags round them.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Flight {
    /// `DR_XADD`, DS:`0xcd50`: two, and negated at either edge.
    pub xadd: i32,
    /// `DR_YADD`, DS:`0xcd52`: one in the load image, and nothing writes it.
    pub yadd: i32,
    /// `DR_X`, DS:`0xcd54`.
    pub x: i32,
    /// `DR_Y`, DS:`0xcd56`: the height, which is nought on the map.
    pub y: i32,
    /// `DR_Z`, DS:`0xcd58`: the map row it flies at.
    pub z: i32,
    /// `DR_DIR`, DS:`0xcd5a`: 3 flying right, 1 flying left, as `InitDragon`
    /// writes it and `DragonDONE` flips it with `xor byte [si+8], 2`.
    pub dir: u8,
    /// `DR_WALK`, DS:`0xcd5b`: the index into `DrAnim`, sixteen words that
    /// are each of the eight flight scripts twice.
    pub walk: u8,
    /// `TrackCNT`, DS:`0xccae`.
    pub track_cnt: i32,
    /// DS:`0xccb0`: the dragon is in the air.
    pub aloft: bool,
    /// DS:`0xccb2`: `0xffff` once a knight has killed it.
    pub dead: bool,
    /// The dragon record's `+0x46`: the knight it is after, by seat.
    pub target: Option<usize>,
    /// The four words at DS:`0x490`: the knights its shadow was over on the
    /// last frame, by seat. Rebuilt every frame.
    pub under: Vec<usize>,
}

impl Default for Flight {
    fn default() -> Flight {
        // The load image's own values, and `InitGameStart+2` (0x1c0d): both
        // flags nought.
        Flight {
            xadd: 2,
            yadd: 1,
            x: 100,
            y: 0,
            z: 100,
            dir: 3,
            walk: 0,
            track_cnt: 0,
            aloft: false,
            dead: false,
            target: None,
            under: Vec::new(),
        }
    }
}

/// The script `DrAnim[walk]` names, as `Dragon_Flight1` to `Dragon_Flight8`:
/// `ContinueDragon+108` (0xa61f) starts the task on `DrAnim[0]`, and
/// `DragonDONE+62` (0xa6ef) steps `+0xa`, masks it to sixteen and reads the
/// word there. The table holds each script twice, so the wings beat once
/// every two frames.
pub fn flight_script(walk: u8) -> String {
    format!("Dragon_Flight{}", ((walk & 0xf) >> 1) + 1)
}

/// `DragonWander+22` (0xa682): the script the glide holds.
pub const GLIDE: &str = "Dragon_Flight4";

impl Flight {
    /// The script the map draws this frame: the glide while `TrackCNT` is
    /// ten or under, the beat otherwise.
    pub fn script(&self) -> String {
        if self.track_cnt <= 0xa {
            GLIDE.to_string()
        } else {
            flight_script(self.walk)
        }
    }

    /// `MapEffects+63` (0xa545) at the start of a knight's turn: `InitDragon`
    /// when it is not in the air, `ContinueDragon` when it is.
    ///
    /// ```text
    /// 0a545  cmp word [0xccb0], 0
    /// 0a54a  jne 0a550
    /// 0a54c  call InitDragon
    /// 0a54f  ret
    /// 0a550  call ContinueDragon
    /// ```
    ///
    /// `day` is DS:`0x5b1`, `alive` is `+0x31 > 0` for each of the four
    /// knight records at DS:`0x6c9e`, and `seed` is `_WIZARD:RND`'s
    /// register. The dragon's own `+0x31`, which `InitDragon+21` (0xa588)
    /// tests, is one from `InitGameStart+209` (0x1ce0) until the `0xffff`
    /// at 0xd38, which is `dead` here.
    pub fn turn_begins(&mut self, day: i32, alive: [bool; 4], seed: &mut u16) {
        if !self.aloft {
            self.init(day, alive, seed);
        } else {
            self.continue_flight(alive, seed);
        }
    }

    /// `InitDragon`, image 0xa571.
    ///
    /// ```text
    /// 0a571  cmp word [0x5b1], 1; jge 0a57b       ; not before the second day
    /// 0a578  jmp 0a658                            ; (a bare ret)
    /// 0a57b  cmp word [0xccb2], 0; je 0a585       ; not once it is dead
    /// 0a585  mov si, 0x6e26
    /// 0a588  cmp byte [si+0x31], 0; jg 0a591      ; nor with no life in it
    /// 0a591  mov word [DR_XADD], 2
    /// 0a597  mov word [DR_X], 0xfff6              ; ten off the left edge
    /// 0a59d  mov word [DR_Y], 0
    /// 0a5a3  mov word [DR_Z], 0x64                ; row a hundred
    /// 0a5a9  mov byte [DR_DIR], 3
    /// 0a5ae  mov byte [DR_WALK], 0
    /// ContinueDragon:                             ; and straight on into it
    /// ```
    fn init(&mut self, day: i32, alive: [bool; 4], seed: &mut u16) {
        // 0a571  cmp word ptr [0x5b1], 1; jge
        if day < 1 {
            return;
        }
        // 0a57b  cmp word ptr [0xccb2], 0; je
        if self.dead {
            return;
        }
        // 0a591..0a5ae
        self.xadd = 2;
        self.x = -10;
        self.y = 0;
        self.z = 0x64;
        self.dir = 3;
        self.walk = 0;
        self.continue_flight(alive, seed);
    }

    /// `ContinueDragon`, image 0xa5b3: the task put up again on `DrAnim[0]`
    /// at the saved position, `TrackCNT` to a hundred, the flag up, and a
    /// knight to fly at chosen by up to four rolls.
    ///
    /// ```text
    /// 0a5b3  cmp word [0xccb2], 0; je 0a5bd; jmp 0a658   ; dead: nothing
    /// 0a5bd  call 0x95c5                          ; the map's tasks cleared
    /// 0a5c0  mov si, 0x6964; mov word [si+0x14], DragonWander  ; kind 0x14's controller
    /// 0a5c9  les di, [0x8975]                     ; MI.C into all five of
    /// 0a5cd  mov si, DrBuffer; ... ; mov [si+0x14], es          ; DrBuffer
    /// 0a5ef  mov si, 0x6e26
    /// 0a5f2  mov ax, [DR_X]; mov [si+2], ax       ; the record from the words
    /// 0a5f8  mov ax, [DR_Y]; mov [si+4], ax
    /// 0a5fe  mov ax, [DR_Z]; mov [si+6], ax
    /// 0a604  mov al, [DR_DIR]; mov [si+8], al
    /// 0a60a  mov word [si+0x18], DrBuffer
    /// 0a60f  mov byte [si+0x35], 0x14
    /// 0a613  mov al, [DR_WALK]; mov [si+0xa], al
    /// 0a619  mov word [si+0x1c], DrAnim
    /// 0a61e  mov di, 0x6e26; mov si, [DrAnim]; call 0x9646   ; the task added
    /// 0a628  mov word [TrackCNT], 0x64
    /// 0a62e  mov word [0xccb0], 1
    /// 0a634  sub cx, cx
    /// 0a636  cmp cx, 4; je 0a659                  ; four misses: down again
    /// 0a63b  call RND; and ax, 3
    /// 0a641  mov dx, 0x62; mul dx                 ; a knight record
    /// 0a646  inc cx
    /// 0a647  mov si, 0x6c9e; add si, ax
    /// 0a64c  cmp byte [si+0x31], 0; jle 0a636     ; dead: roll again
    /// 0a652  mov di, 0x6e26; mov [di+0x46], si    ; after him
    /// 0a658  ret
    /// 0a659  mov word [0xccb0], 0                 ; nobody left to fly at
    /// 0a65f  mov di, 0x6e26; mov word [di+0x46], 0
    /// 0a667  call 0x95c5
    /// ```
    ///
    /// The roll is made every turn, so the knight it is after can change
    /// from one turn to the next whatever `KnightWyrm` wrote a moment before.
    pub fn continue_flight(&mut self, alive: [bool; 4], seed: &mut u16) {
        // 0a5b3  cmp word ptr [0xccb2], 0
        if self.dead {
            return;
        }
        // 0a628  mov word ptr [TrackCNT], 0x64; 0a62e mov word ptr [0xccb0], 1
        self.track_cnt = 0x64;
        self.aloft = true;
        // 0a634..0a655
        for _ in 0..4 {
            // 0a63b  call RND; and ax, 3
            *seed = rnd(*seed);
            let seat = (*seed & 3) as usize;
            // 0a64c  cmp byte ptr [si + 0x31], 0; jle
            if alive[seat] {
                // 0a655  mov word ptr [di + 0x46], si
                self.target = Some(seat);
                return;
            }
        }
        // 0a659  mov word ptr [0xccb0], 0; 0a662 mov word ptr [di + 0x46], 0
        self.aloft = false;
        self.target = None;
    }

    /// `DragonWander`, image 0xa66b, which is `CONTROLTABLE` slot 0x14 while
    /// the dragon is on the map: one frame of the flight.
    ///
    /// ```text
    /// 0a66b  mov [0x77e8], si
    /// 0a66f  dec word [TrackCNT]
    /// 0a673  jns 0a67b
    /// 0a675  mov word [TrackCNT], 0x64            ; round again
    /// 0a67b  cmp word [TrackCNT], 0xa
    /// 0a680  jg  DragonTRACK
    /// 0a682  mov word [0x783a], Dragon_Flight4    ; the glide
    /// 0a688  mov dx, [DR_XADD]; add [si+2], dx    ; along, and no tracking
    /// 0a68f  jmp DragonControlDone
    /// DragonTRACK:
    /// 0a691  mov dx, [DR_XADD]; add [si+2], dx    ; along
    /// 0a698  mov dx, [DR_YADD]
    /// 0a69c  mov di, 0x6e26; mov di, [di+0x46]    ; the knight it is after
    /// 0a6a2  mov ax, [di+0x5e]                    ; his row
    /// 0a6a5  cmp ax, [si+6]
    /// 0a6a8  je  DragonDONE                       ; on it
    /// 0a6aa  jg  0a6ae
    /// 0a6ac  neg dx                               ; above it: up
    /// 0a6ae  add [si+6], dx                       ; a row towards him
    /// DragonDONE:
    /// 0a6b1  cmp word [si+2], 0x15e; jle 0a6c5
    /// 0a6b8  mov word [si+2], 0x159               ; past 350: back to 345,
    /// 0a6bd  xor byte [si+8], 2                   ; turned,
    /// 0a6c1  neg word [DR_XADD]                   ; and the other way
    /// 0a6c5  cmp word [si+2], -0x14; jg 0a6d8
    /// 0a6cb  mov word [si+2], 0xfff6              ; past -20: back to -10
    /// 0a6d0  xor byte [si+8], 2
    /// 0a6d4  neg word [DR_XADD]
    /// 0a6d8  cmp word [si+6], 0xc8; jle 0a6e4
    /// 0a6df  mov word [si+6], 0                   ; past 200: row nought
    /// 0a6e4  cmp word [si+6], 0; jns 0a6ef
    /// 0a6ea  mov word [si+6], 0xc8                ; under nought: 200
    /// 0a6ef  inc byte [si+0xa]
    /// 0a6f2  and byte [si+0xa], 0xf               ; DrAnim's sixteen
    /// 0a6f6  mov al, [si+0xa]; cwde; mov di, DrAnim; shl ax, 1; add di, ax
    /// 0a701  push word [di]; pop word [0x783a]    ; the frame
    /// DragonControlDone:
    /// 0a707  ...the record back into DR_X, DR_Y, DR_Z, DR_DIR, DR_WALK
    /// 0a725  jmp NOTEND+3
    /// ```
    ///
    /// `target_row` is `[target+0x5e]`, the row of the knight it is after.
    /// Returns the script the frame is drawn on.
    pub fn wander(&mut self, target_row: i32) -> String {
        // 0a66f  dec word ptr [TrackCNT]; jns
        self.track_cnt -= 1;
        if self.track_cnt < 0 {
            // 0a675  mov word ptr [TrackCNT], 0x64
            self.track_cnt = 0x64;
        }
        // 0a67b  cmp word ptr [TrackCNT], 0xa; jg DragonTRACK
        if self.track_cnt <= 0xa {
            // 0a688  mov dx, [DR_XADD]; add word ptr [si + 2], dx
            self.x += self.xadd;
            // 0a682  mov word ptr [0x783a], Dragon_Flight4
            return GLIDE.to_string();
        }
        // DragonTRACK:
        // 0a691  mov dx, [DR_XADD]; add word ptr [si + 2], dx
        self.x += self.xadd;
        // 0a698  mov dx, [DR_YADD]
        let mut dx = self.yadd;
        // 0a6a2  mov ax, [di+0x5e]; cmp ax, [si+6]; je DragonDONE; jg; neg dx
        if target_row != self.z {
            if target_row < self.z {
                dx = -dx;
            }
            // 0a6ae  add word ptr [si + 6], dx
            self.z += dx;
        }
        // DragonDONE:
        // 0a6b1  cmp word ptr [si + 2], 0x15e; jle
        if self.x > 0x15e {
            self.x = 0x159;
            self.dir ^= 2;
            self.xadd = -self.xadd;
        }
        // 0a6c5  cmp word ptr [si + 2], -0x14; jg
        if self.x <= -0x14 {
            self.x = -10;
            self.dir ^= 2;
            self.xadd = -self.xadd;
        }
        // 0a6d8  cmp word ptr [si + 6], 0xc8; jle
        if self.z > 0xc8 {
            self.z = 0;
        }
        // 0a6e4  cmp word ptr [si + 6], 0; jns
        if self.z < 0 {
            self.z = 0xc8;
        }
        // 0a6ef  inc byte ptr [si + 0xa]; and byte ptr [si + 0xa], 0xf
        self.walk = (self.walk + 1) & 0xf;
        // 0a6f6..0a703: DrAnim[walk]
        flight_script(self.walk)
    }

    /// `CheckEncounterDone+128` (0x816), which the overlap walk `FOLLOW`
    /// runs every frame falls into: which knights the dragon's shadow is
    /// over.
    ///
    /// ```text
    /// 007fc  mov si, 0x490; four words zeroed          ; the table cleared
    /// 00816  cmp word [0xccb0], 0; je CheckLairEncounter
    /// 0081d  push cx
    /// 0081e  mov di, 0x6e26
    /// 00821  mov ax, 0x25; sub ax, 0x11                 ; icon 0x14, nine by five
    /// 00824  mov bx, [di+2]; sub bx, 0xa                ; ten left of the dragon
    /// 00827  mov cx, [di+6]                             ; on its row
    /// 00830  mov dx, [si+0x5c]; mov bp, [si+0x5e]       ; the knight's token
    /// 00838  call CheckGROOC
    /// 0083d  cmp bp, 2; jne 00852                       ; both axes overlap
    /// 00842  xchg [0x948], bx; mov [bx], si; add bx, 2  ; into the table
    /// 0084f  call DisplayGlowKnight
    /// 00852  pop cx; add si, 0x62; loop 0081d           ; all four
    /// ```
    ///
    /// `CheckGROOC` (0x653) overlaps the icon's box with the eight by ten
    /// token one axis at a time, which is [`crate::place::PlaceDef::covers`]
    /// with the marker's own size; `shadow` is that size, `MI.C` frame 0x14's.
    pub fn frame_ends(&mut self, positions: [(i32, i32); 4], shadow: (i32, i32)) {
        use crate::overworld::{TOKEN_H, TOKEN_W};
        // 007fc..0080d: the table cleared.
        self.under.clear();
        // 00816  cmp word ptr [0xccb0], 0
        if !self.aloft {
            return;
        }
        let spans = |a: i32, aw: i32, b: i32, bw: i32| a < b + bw && b < a + aw;
        let (bx, cx) = (self.x - 0xa, self.z);
        for (seat, (kx, ky)) in positions.iter().enumerate() {
            if spans(*kx, TOKEN_W, bx, shadow.0) && spans(*ky, TOKEN_H, cx, shadow.1) {
                self.under.push(seat);
            }
        }
    }

    /// `DragonEncounter`, image 0xa3e2, which `ScrollINPUT` reaches every
    /// frame fire is not down: whether the dragon comes down on this knight.
    ///
    /// ```text
    /// 0a3e2  mov di, 0x6e26
    /// 0a3e5  cmp word [0xccb2], -1; js GoTheDistance    ; never taken: see below
    /// 0a3ec  cmp word [0xccb0], 0; je GoTheDistance     ; not in the air
    /// 0a3f3  mov ax, [di+0x46]; cmp ax, [0x77e8]; jne   ; not after this knight
    /// 0a3fc  cmp word [0xcc9e], 0; jne                  ; the hawk is up
    /// 0a403  cmp word [0xcca0], 0; jne                  ; the gem is up
    /// 0a40a  mov si, 0x490; mov cx, 4
    /// 0a410  add si, 2; cmp ax, [si-2]; jne 0a420       ; in the table?
    /// 0a418  call 0xa554; call 0xcf6                    ; the fight
    /// 0a420  loop 0a410
    /// ```
    ///
    /// The first test is `js` on `cmp` against minus one: the flags are
    /// those of `word + 1`, which is nought for a dead dragon and one for a
    /// live one, and neither is negative. So it never turns the encounter
    /// away, and a dead dragon is kept out by the second test, since
    /// `_dragon_won+41` (0xd4a) takes the flag down with the same write.
    /// `aloft_on_magic` is the two effect flags.
    pub fn comes_down_on(&self, seat: usize, aloft_on_magic: bool) -> bool {
        // 0a3e5  cmp word ptr [0xccb2], -1; js: never taken.
        // 0a3ec  cmp word ptr [0xccb0], 0; je
        if !self.aloft {
            return false;
        }
        // 0a3f3  mov ax, [di+0x46]; cmp ax, [0x77e8]; jne
        if self.target != Some(seat) {
            return false;
        }
        // 0a3fc / 0a403
        if aloft_on_magic {
            return false;
        }
        // 0a40a..0a420
        self.under.contains(&seat)
    }

    /// The end of the fight the routine at 0xcf6 runs.
    ///
    /// ```text
    /// 00d1b  test word [KnightDeath], 1; je 00d35
    /// _dragon_won:
    /// 00d23  mov si, 0x6e26; mov di, [KnightTable]
    /// 00d2a  call 0xaf7                           ; WhoLived+57: what it takes off him
    /// 00d2d  or  word [KnightDeath], 1
    /// 00d32  jmp EncounterAllDone
    /// 00d35  mov si, 0x6e26
    /// 00d38  mov byte [si+0x31], 0xff             ; the dragon is dead
    /// 00d3c  mov si, [KnightTable]
    /// 00d40  add word [si+0x36], 2                ; two points of experience
    /// 00d44  mov ax, 0xa; call 0xbdd3             ; the panel on StatTYPE 0xa
    /// 00d4a  mov word [0xccb0], 0                 ; out of the air
    /// 00d50  mov word [0xccb2], 0xffff            ; and never back
    /// 00d56  jmp EncounterAllDone
    /// ```
    ///
    /// A knight who lost stays a target; the dragon flies on and
    /// `ContinueDragon` rolls again next turn. The experience and the panel
    /// are the run's and the desktop's.
    pub fn fight_over(&mut self, knight_won: bool) {
        if knight_won {
            // 00d38, 00d4a, 00d50
            self.dead = true;
            self.aloft = false;
            self.target = None;
        }
    }

    /// `KnightWyrm`, image 0xabe8, which the computer knights' turn runs and
    /// the Scroll of the Wyrm is cast through: the dragon sent after the
    /// knight chosen, one scroll spent, and the flight begun again.
    ///
    /// ```text
    /// 0abe8  cmp word [0x5b1], 1; jl 0ac18          ; not before the second day
    /// 0abef  mov si, [0x77e8]
    /// 0abf3  cmp word [si+0x46], 0; je 0ac18        ; no knight chosen
    /// 0abf9  mov di, [si+0x44]
    /// 0abfc  cmp byte [di+0x10], 0; je 0ac18        ; no scroll of the Wyrm
    /// 0ac02  mov bp, 0x6e26
    /// 0ac05  mov ax, [si+0x46]
    /// 0ac08  cmp ax, [bp+0x46]; je 0ac18            ; already after him
    /// 0ac0e  mov [bp+0x46], ax                      ; after him now
    /// 0ac12  dec byte [di+0x10]                     ; one scroll fewer
    /// 0ac15  call ContinueDragon
    /// 0ac18  ret
    /// ```
    ///
    /// `ContinueDragon` then rolls a target of its own over the one just
    /// written (0xa655), so what the scroll buys is the dragon in the air
    /// this turn, after a knight the roll names. That is the code.
    /// Returns whether a scroll was spent.
    pub fn knight_wyrm(
        &mut self,
        day: i32,
        chosen: Option<usize>,
        scrolls: u32,
        alive: [bool; 4],
        seed: &mut u16,
    ) -> bool {
        // 0abe8  cmp word ptr [0x5b1], 1; jl
        if day < 1 {
            return false;
        }
        // 0abf3  cmp word ptr [si + 0x46], 0; je
        let Some(chosen) = chosen else {
            return false;
        };
        // 0abfc  cmp byte ptr [di + 0x10], 0; je
        if scrolls == 0 {
            return false;
        }
        // 0ac08  cmp ax, [bp+0x46]; je
        if self.target == Some(chosen) {
            return false;
        }
        // 0ac0e  mov [bp+0x46], ax
        self.target = Some(chosen);
        // 0ac12  dec byte ptr [di + 0x10]; 0ac15 call ContinueDragon
        self.continue_flight(alive, seed);
        true
    }

    /// Everything that goes into the fingerprint.
    pub fn hash_into(&self, mix: &mut impl FnMut(i64)) {
        for v in [
            self.xadd,
            self.yadd,
            self.x,
            self.y,
            self.z,
            self.dir as i32,
            self.walk as i32,
            self.track_cnt,
            self.aloft as i32,
            self.dead as i32,
            self.target.map_or(-1, |t| t as i32),
        ] {
            mix(v as i64);
        }
        for u in &self.under {
            mix(*u as i64);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [bool; 4] = [true; 4];
    const HOMES: [(i32, i32); 4] = [(10, 10), (300, 5), (26, 180), (300, 185)];

    /// `InitDragon` (0xa571): nothing on the first day, and from the second
    /// the dragon starts ten off the left edge at row a hundred, in the air,
    /// after a knight the roll names.
    #[test]
    fn the_dragon_takes_the_air_from_the_second_day() {
        let mut f = Flight::default();
        let mut seed = 0x2f1d;
        f.turn_begins(0, ALL, &mut seed);
        assert!(!f.aloft, "0a571: cmp word [0x5b1], 1");
        f.turn_begins(1, ALL, &mut seed);
        assert!(f.aloft, "0a62e");
        assert_eq!(
            (f.x, f.z, f.xadd, f.dir, f.walk),
            (-10, 100, 2, 3, 0),
            "0a591..0a5ae"
        );
        assert_eq!(f.track_cnt, 100, "0a628");
        assert!(f.target.is_some(), "0a655");
        // Dead knights are rolled past, and four misses put it down again.
        let mut f = Flight::default();
        f.turn_begins(1, [false; 4], &mut seed);
        assert!(!f.aloft, "0a659");
        assert_eq!(f.target, None, "0a662");
        let mut f = Flight::default();
        f.turn_begins(1, [false, false, true, false], &mut seed);
        assert!(
            f.target.is_none_or(|t| t == 2),
            "0a64c: only a living knight"
        );
        // And a dead dragon never flies again.
        let mut f = Flight {
            dead: true,
            ..Flight::default()
        };
        f.turn_begins(5, ALL, &mut seed);
        assert!(!f.aloft, "0a57b");
    }

    /// `DragonWander` (0xa66b): two along a frame, a row towards the knight
    /// it is after, the wings beating every second frame, the glide on the
    /// last eleven of every hundred, and the turn at either edge.
    #[test]
    fn the_dragon_sweeps_the_map_at_its_targets_row_and_turns_at_the_edges() {
        let mut f = Flight::default();
        let mut seed = 0x2f1d;
        f.turn_begins(1, ALL, &mut seed);
        let s = f.wander(120);
        assert_eq!((f.x, f.z), (-8, 101), "0a691, 0a6ae: two along, one down");
        assert_eq!(f.track_cnt, 99);
        assert_eq!(s, "Dragon_Flight1", "0a6ef: walk 1 is DrAnim[1]");
        assert_eq!(f.wander(120), "Dragon_Flight2", "and 2 is DrAnim[2]");
        for _ in 0..14 {
            f.wander(120);
        }
        assert_eq!(f.walk, 0, "0a6f2: sixteen and round");
        // Above him: up a row.
        f.z = 130;
        f.wander(120);
        assert_eq!(f.z, 129, "0a6ac: neg dx");
        f.z = 120;
        f.wander(120);
        assert_eq!(f.z, 120, "0a6a8: on his row it stays");
        // Right across: past 350 it turns, and past -20 it turns back.
        f.x = 349;
        f.wander(120);
        assert_eq!((f.x, f.xadd, f.dir), (345, -2, 1), "0a6b8..0a6c1");
        f.x = -19;
        f.wander(120);
        assert_eq!((f.x, f.xadd, f.dir), (-10, 2, 3), "0a6cb..0a6d4");
        // The glide: eleven frames on Flight4 and no tracking, then round.
        f.track_cnt = 11;
        f.z = 50;
        assert_eq!(f.wander(120), "Dragon_Flight4", "0a682");
        assert_eq!(f.z, 50, "0a688: along and nothing else");
        f.track_cnt = 1;
        assert_eq!(f.wander(120), "Dragon_Flight4", "0a67b: ten and under");
        assert_eq!(f.track_cnt, 0);
        // Under nought it is a hundred again, and that frame already tracks.
        assert_ne!(f.wander(120), "Dragon_Flight4", "0a675: round again");
        assert_eq!(f.track_cnt, 100);
        assert_eq!(f.z, 51, "and tracking with it");
    }

    /// `CheckEncounterDone+128` (0x816) and `DragonEncounter` (0xa3e2): the
    /// fight starts when the dragon's nine by five shadow is over the knight
    /// it is after, with neither the gem nor the hawk up.
    #[test]
    fn the_dragon_comes_down_on_the_knight_it_is_after_when_its_shadow_covers_him() {
        let mut f = Flight {
            aloft: true,
            target: Some(0),
            x: 100,
            z: 100,
            ..Flight::default()
        };
        let mut at = HOMES;
        at[0] = (92, 96);
        f.frame_ends(at, (9, 5));
        assert_eq!(f.under, vec![0], "00842: the table");
        assert!(f.comes_down_on(0, false), "0a418");
        assert!(
            !f.comes_down_on(0, true),
            "0a3fc, 0a403: not aloft on magic"
        );
        assert!(
            !f.comes_down_on(1, false),
            "0a3f3: not on a knight it is not after"
        );
        // Out from under it: nothing.
        at[0] = (120, 96);
        f.frame_ends(at, (9, 5));
        assert!(f.under.is_empty());
        assert!(!f.comes_down_on(0, false));
        // A knight it is not after, standing under it, is noted and left.
        f.target = Some(2);
        at[0] = (92, 96);
        f.frame_ends(at, (9, 5));
        assert_eq!(f.under, vec![0]);
        assert!(!f.comes_down_on(0, false));
        // Not in the air: no table at all.
        f.aloft = false;
        f.frame_ends(at, (9, 5));
        assert!(f.under.is_empty(), "00816");
    }

    /// The routine at 0xcf6: a knight who kills it grounds it for good, one
    /// who does not leaves it flying.
    #[test]
    fn a_dead_dragon_stays_dead_and_a_live_one_flies_on() {
        let mut f = Flight {
            aloft: true,
            target: Some(0),
            ..Flight::default()
        };
        f.fight_over(false);
        assert!(f.aloft && !f.dead, "00d23: the dragon won, and flies on");
        f.fight_over(true);
        assert!(!f.aloft && f.dead, "00d4a, 00d50");
        let mut seed = 0x2f1d;
        f.turn_begins(9, ALL, &mut seed);
        assert!(!f.aloft, "0a57b: never again");
    }

    /// `KnightWyrm` (0xabe8): a scroll spent sends the dragon after the
    /// knight chosen and puts it in the air, and the roll `ContinueDragon`
    /// makes then names whom it is after.
    #[test]
    fn the_scroll_of_the_wyrm_puts_the_dragon_in_the_air_after_a_knight() {
        let mut seed = 0x2f1d;
        let mut f = Flight::default();
        assert!(
            !f.knight_wyrm(0, Some(1), 1, ALL, &mut seed),
            "0abe8: not on the first day"
        );
        assert!(
            !f.knight_wyrm(2, None, 1, ALL, &mut seed),
            "0abf3: nobody chosen"
        );
        assert!(
            !f.knight_wyrm(2, Some(1), 0, ALL, &mut seed),
            "0abfc: no scroll"
        );
        assert!(
            f.knight_wyrm(2, Some(1), 1, ALL, &mut seed),
            "0ac12: one spent"
        );
        assert!(f.aloft, "0ac15: ContinueDragon");
        assert!(f.target.is_some(), "0a655: after whoever the roll named");
        let t = f.target;
        assert!(
            !f.knight_wyrm(2, t, 1, ALL, &mut seed),
            "0ac08: already after him"
        );
        // A dead dragon takes the scroll and stays down.
        let mut f = Flight {
            dead: true,
            target: Some(0),
            ..Flight::default()
        };
        assert!(f.knight_wyrm(2, Some(1), 1, ALL, &mut seed));
        assert!(!f.aloft, "0a5b3");
    }
}
