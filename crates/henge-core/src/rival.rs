//! The three computer knights: the seats nobody at the keyboard took.
//!
//! The original always plays four. `InitGameStart` (0x1c0d) fills all four
//! knight records at DS:`0x6c9e` with `Enemy1Name` to `Enemy4Name`, kind 8
//! (`ControlBlackKnight`), colour index 4 and the four corners; `ChooseKnight`
//! (0x1565) then walks `choose_player` from record 0 over the first
//! `NUM_PLAYERS` of them and `InitKnights` (0x157) moves those to their
//! villages. With one player that leaves records 1 to 3 as they were: `SIR
//! DWAIN` at (300, 100), `SIR BALAIN` at (160, 20) and `SIR GUNTHER` at
//! (160, 180), each riding the map on his own turn.
//!
//! What a computer knight does with his turn is `MapLOOP+19` (0xa319), which
//! runs once a frame in place of `PlayerKnight`:
//!
//! ```text
//! 0a313  cmp word [si+0x20], 4; jne PlayerKnight
//! 0a319  call 0xa75c            ; the closest lairs, once a turn
//! 0a31c  call KnightXP          ; experience spent
//! 0a31f  call KnightHeal        ; a potion opened
//! 0a322  call FindKnight        ; a knight to go after, once a turn
//! 0a325  call KnightAquire      ; a scroll of aquisition read at him
//! 0a328  call KnightWyrm        ; a scroll of the wyrm sent at him
//! 0a32b  call KnightHaste       ; a scroll of haste when he is far
//! 0a32e  call KnightSupplies    ; what the purse buys
//! 0a331  call KnightGoesToTown  ; and which town sells it
//! 0a334  call BKCollision       ; arrived: the fight, the shop, the lair
//! 0a337  call CheckSLOW
//! 0a33a  sub bx, bx
//! 0a33c  cmp word [0xcc9e], 0; jne 0a34e
//! 0a343  inc word [0xcc98]      ; the step counted
//! 0a347  cmp word [SlowFLAG], 0; jne MapMovement   ; and refused
//! 0a34e  call TrackLair         ; the direction, one pixel a frame
//! 0a351  mov bx, ax
//! 0a353  jmp MapMovement
//! ```
//!
//! and the rest of the frame is the player's own: `MapMovement`, `ScrollINPUT`
//! (which skips every key for a kind 4 record at 0xa393), `DragonEncounter`,
//! `GoTheDistance` and `FOLLOW`. So a computer knight's day is walked on the
//! map at a pixel a frame, in front of the player, with `SHOW` drawing his
//! token in the glowing purple (`[di+0x20] + 5`, 0xa1f7) and
//! `DisplayOtherKnights` drawing everybody else's.
//!
//! Two things a computer knight never does, and both are the code's. He never
//! fights a lair: `BKCollision+104` (0xab19) finds `CloseLair` under him and
//! spends the rest of the day (`0xab26`), and the only road into a lair's
//! guardian is `StackDecision` off a fire press he cannot make. And he never
//! fights another computer knight: `Combat+159` (0x3f0) sends two kind 4
//! records straight to `HeadBattleDone`.

use crate::item::Items;
use crate::knight::{Ability, Knight};
use crate::monster::rnd;
use crate::overworld::{Landscape, MAX_X, MAX_Y, TOKEN_H, TOKEN_W};
use crate::place::{Effect, Places};
use crate::run::Run;
use crate::status::Hoard;
use serde::{Deserialize, Serialize};

/// `Enemy1Name` to `Enemy4Name`, DS:`0x6922` (image 0x18cd2): the names
/// `InitGameStart` hands the four records before anybody chooses. Record 0
/// is the player's with one player, so the first is overwritten by
/// `ChooseFIRE`; the other three ride.
pub const ENEMY_NAMES: [&str; 4] = ["SIR BANNER", "SIR DWAIN", "SIR BALAIN", "SIR GUNTHER"];

/// `InitGameStart+105` to `+203` (0x1c76 to 0x1cd8): `[di+0x5c]` and
/// `[di+0x5e]` of the four records.
pub const ENEMY_CORNERS: [(i32, i32); 4] = [(15, 100), (300, 100), (160, 20), (160, 180)];

/// `[+0x20]` on every record `InitGameStart` fills: the fifth token colour,
/// the dark purple, and the value `MapLOOP+13` (0xa313) tests to tell a
/// computer knight from a person.
pub const COMPUTER_COLOUR: usize = 4;

/// How many knight records there are: `mov cx, 4` in `DisplayOtherKnights`,
/// `CheckEncounterDone`, `FindKnight` and `ContinueDragon`.
pub const RECORDS: usize = 4;

/// The weapon codes `[si+0x40]` holds: `SupplyWeopon` compares against 0x17
/// and 0x18, `TakeSword` writes 0x19 and `TakeALL` writes 0x16 and 0x19.
pub const LONG_SWORD: u16 = 0x16;
pub const BROAD_SWORD: u16 = 0x17;
pub const CLAYMORE: u16 = 0x18;
pub const SWORD_OF_SHARPNESS: u16 = 0x19;

/// The armour codes `[si+0x42]` holds: `KnightSupplies` compares against
/// 0x1c, 0x1d and 0x1e, and `TakeArmour` writes 0x1b over the loser.
pub const PADDED_ARMOUR: u16 = 0x1b;
pub const CHAIN_MAIL: u16 = 0x1c;
pub const PLATE_ARMOUR: u16 = 0x1d;
pub const BATTLE_ARMOUR: u16 = 0x1e;

/// The pack id of a weapon code, off `SLOT_TABLE`: slots 10 to 13 are the
/// four blades in code order, which is the cel order `DisplayKnight` draws
/// them in (`cel - 0x16`).
pub fn weapon_id(code: u16) -> &'static str {
    let slot = 10 + code.saturating_sub(LONG_SWORD).min(3) as usize;
    crate::status::SLOT_TABLE[slot].item.unwrap_or("long_sword")
}

/// The pack id of an armour code: slots 6 to 9.
pub fn armour_id(code: u16) -> &'static str {
    let slot = 6 + code.saturating_sub(PADDED_ARMOUR).min(3) as usize;
    crate::status::SLOT_TABLE[slot]
        .item
        .unwrap_or("padded_armour")
}

/// The code of a weapon id, and a long sword for anything else.
pub fn weapon_code(id: &str) -> u16 {
    match crate::status::slot_for_item(id) {
        Some(slot @ 10..=13) => LONG_SWORD + (slot - 10) as u16,
        _ => LONG_SWORD,
    }
}

/// The code of an armour id, and padded armour for anything else.
pub fn armour_code(id: &str) -> u16 {
    match crate::status::slot_for_item(id) {
        Some(slot @ 6..=9) => PADDED_ARMOUR + (slot - 6) as u16,
        _ => PADDED_ARMOUR,
    }
}

/// `SelectCNT+2`, DS:`0xf452`: what each suit of armour is worth in health,
/// indexed by `code - 0x1b`. `TakeArmour+16` (0xb9e) reads it to move the
/// health between the two records. The bytes are `00 0a 14 1e`.
#[rustfmt::skip]
pub const ARMOUR_HEALTH: [i32; 4] = [0, 0xa, 0x14, 0x1e];

/// `TakeMagicTABLE`, DS:`0x3f2` (image 0x127a2): the fields of the magic
/// record in the order a winner takes them, ending on `0xffff`. Moonstones,
/// keys, the sword, rings, aquisition, protection, talismans, the hawk, the
/// wyrm, potions, gems, haste.
#[rustfmt::skip]
pub const TAKE_MAGIC_TABLE: [u16; 12] = [
    0x16, 0x14, 0x04, 0x06, 0x0e, 0x12, 0x08, 0x0c, 0x10, 0x00, 0x02, 0x0a,
];

impl Hoard {
    /// The byte at a field offset of the record, as `[bx]` reads it.
    pub fn field(&self, off: u16) -> u8 {
        match off {
            0x00 => self.potions,
            0x02 => self.gems,
            0x04 => u8::from(self.magic_sword),
            0x06 => self.rings,
            0x08 => self.talismans,
            0x0a..=0x12 => self.scrolls[((off - 0x0a) / 2) as usize],
            0x14 => self.keys,
            0x16 => self.moonstones,
            _ => 0,
        }
    }

    /// The byte written back.
    pub fn set_field(&mut self, off: u16, v: u8) {
        match off {
            0x00 => self.potions = v,
            0x02 => self.gems = v,
            0x04 => self.magic_sword = v > 0,
            0x06 => self.rings = v,
            0x08 => self.talismans = v,
            0x0a..=0x12 => self.scrolls[((off - 0x0a) / 2) as usize] = v,
            0x14 => self.keys = v,
            0x16 => self.moonstones = v,
            _ => {}
        }
    }
}

// -------------------------------------------------------------- the record

/// A knight record as the loot routines read it: `+0x35` the kind, `+0x31`
/// the life points, `+0x32` the purse, `+0x38` and `+0x3c` the health,
/// `+0x40` and `+0x42` the blade and the suit, and `[+0x44]` the magic
/// record. The player's is built off the run and written back; a computer
/// knight's is his own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    /// `+0x35`: 6 for a person, 8 for a computer knight, 0xa for the dragon
    /// in the arena and 0x14 for the dragon over the map.
    pub kind: u8,
    pub lives: i32,
    pub gold: u32,
    pub health: i32,
    pub max_health: i32,
    pub weapon: u16,
    pub armour: u16,
    pub hoard: Hoard,
}

/// What `WhoLived+57` took, for whoever wants to say so.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Loot {
    /// `TakeGold`: half the purse, and only when the winner is the dragon.
    Gold(u32),
    /// One field of the magic record, at this offset.
    Field(u16),
    /// `TakeALL`: every field, off a dead loser.
    Everything,
    /// `TakeArmour`: the loser's suit, when it was the better one.
    Armour(u16),
    /// Nothing worth taking.
    Nothing,
}

/// `WhoLived+57`, image 0xaf7: what a winner takes off the one who lost.
/// `si` is the winner and `di` the loser.
///
/// ```text
/// 00af7  mov dx, 5
/// 00afa  cmp byte [si+0x35], 0xa; jne 00b03   ; the dragon takes gold too
/// 00b00  call TakeGold
/// 00b03  mov bx, [di+0x44]; mov [a2], bx      ; the loser's magic record
/// 00b0a  mov bx, [si+0x44]; mov [a3], bx      ; the winner's
/// 00b11  cmp byte [di+0x31], 0; jne 00b1a
/// 00b17  jmp TakeALL                          ; a dead man keeps nothing
/// 00b1a  mov bx, TakeMagicTABLE; mov [a4], bx
/// 00b21  mov bx, [a4]; mov ax, [bx]; add bx, 2; add [a4], 2
/// 00b2f  cmp ax, 0xffff; je 00b82             ; the table's end
/// 00b34  mov bx, [a2]; add bx, ax
/// 00b3a  cmp byte [bx], 0; je 00b21           ; he has none of these
/// 00b3f  mov dx, 1
/// 00b42  cmp ax, 0x16; jne; jmp TakingMoon    ; the moonstones, whole
/// 00b4a  cmp ax, 0x14; jne; jmp TakingMoon    ; the keys, whole
/// 00b52  mov bx, [a2]; add bx, ax; sub byte [bx], 1    ; one off him
/// 00b5b  mov bx, [a3]; add bx, ax; add byte [bx], 1    ; and onto the winner
/// 00b64  mov bx, [a2]; add bx, ax; cmp byte [bx], 0; jne 00b82
/// 00b6f  cmp ax, 4; jne 00b82                 ; his last sword of sharpness:
/// 00b74  mov word [di+0x40], 0x16             ; a long sword in his hand
/// 00b79  mov bx, [a2]; add bx, ax; mov byte [bx], 0
/// 00b82  cmp dx, 0; jne REFF
/// 00b87  call TakeArmour                      ; nothing magic: the suit
/// REFF:
/// 00b8a  call 0x279; ret                      ; the health put back together
/// TakeArmour:
/// 00b8e  mov ax, [si+0x42]; cmp ax, [di+0x42]; jge TakeGold  ; his is no better
/// 00b96  push ax; mov ax, [di+0x42]; mov [si+0x42], ax; pop ax
/// 00b9e  sub ax, 0x1b; mov bx, 0xf452; add bx, ax; mov al, [bx]; cwde
/// 00bad  sub [di+0x38], ax; add [si+0x38], ax  ; the winner's OLD suit's worth
/// 00bb3  mov word [di+0x42], 0x1b             ; padded for the loser
/// TakeGold:
/// 00bb9  mov ax, [di+0x32]; shr ax, 1; add [si+0x32], ax; mov [di+0x32], ax
/// TakingMoon:
/// 00c52  ..[a3+ax] |= [a2+ax]; [a2+ax] = 0; jmp REFF
/// TakeALL:
/// 00bc5  every table entry: 0x16 and 0x14 OR'd across, 4 with 0x19 into
///        [di+0x40] when he had one, the rest added; all zeroed on the loser
/// ```
///
/// Two things in it are the code's and not what a reader would expect.
/// `TakeArmour` falls through into `TakeGold` when the loser's suit is no
/// better, so a winner with nothing magic to take and no better suit to take
/// halves the loser's purse anyway. And the health it moves is the worth of
/// the *winner's* old suit, read at `0xb9e` off the `ax` pushed at `0xb96`,
/// not the suit that changed hands.
pub fn take_from_the_fallen(winner: &mut Record, loser: &mut Record) -> Loot {
    // 00af7  mov dx, 5
    let mut dx = 5;
    let mut loot = Loot::Nothing;
    // 00afa  cmp byte ptr [si + 0x35], 0xa; jne; call TakeGold
    if winner.kind == 0xa {
        loot = Loot::Gold(take_gold(winner, loser));
    }
    // 00b11  cmp byte ptr [di + 0x31], 0; jne; jmp TakeALL
    if loser.lives == 0 {
        take_all(winner, loser);
        return Loot::Everything;
    }
    // 00b21..00b7f
    for ax in TAKE_MAGIC_TABLE {
        // 00b3a  cmp byte ptr [bx], 0; je
        if loser.hoard.field(ax) == 0 {
            continue;
        }
        // 00b3f  mov dx, 1
        dx = 1;
        loot = Loot::Field(ax);
        // 00b42 / 00b4a: TakingMoon, the field OR'd across whole.
        if ax == 0x16 || ax == 0x14 {
            let bits = winner.hoard.field(ax) | loser.hoard.field(ax);
            winner.hoard.set_field(ax, bits);
            loser.hoard.set_field(ax, 0);
            return loot;
        }
        // 00b58  sub byte ptr [bx], 1; 00b61 add byte ptr [bx], 1
        loser.hoard.set_field(ax, loser.hoard.field(ax) - 1);
        winner
            .hoard
            .set_field(ax, winner.hoard.field(ax).saturating_add(1));
        // 00b6a  cmp byte ptr [bx], 0; jne; 00b6f cmp ax, 4; jne
        if loser.hoard.field(ax) == 0 && ax == 4 {
            // 00b74  mov word ptr [di + 0x40], 0x16
            loser.weapon = LONG_SWORD;
            loser.hoard.set_field(ax, 0);
        }
        break;
    }
    // 00b82  cmp dx, 0; jne REFF: dx is five or one, so the `jne` is the
    // table having found nothing, and TakeArmour (0xb87) follows.
    if dx == 5 {
        loot = take_armour(winner, loser, loot);
    }
    loot
}

/// `TakeArmour`, image 0xb8e, which falls into `TakeGold`.
fn take_armour(winner: &mut Record, loser: &mut Record, loot: Loot) -> Loot {
    // 00b8e  mov ax, [si+0x42]; cmp ax, [di+0x42]; jge TakeGold
    let ax = winner.armour;
    if ax >= loser.armour {
        let gold = take_gold(winner, loser);
        return if gold > 0 { Loot::Gold(gold) } else { loot };
    }
    // 00b97  mov ax, [di+0x42]; mov [si+0x42], ax
    let taken = loser.armour;
    winner.armour = taken;
    // 00b9e  sub ax, 0x1b; mov bx, 0xf452; add bx, ax; mov al, [bx]; cwde
    let worth = ARMOUR_HEALTH[(ax.saturating_sub(PADDED_ARMOUR) as usize).min(3)];
    // 00bad  sub word ptr [di + 0x38], ax; 00bb0 add word ptr [si + 0x38], ax
    loser.health -= worth;
    winner.health += worth;
    // 00bb3  mov word ptr [di + 0x42], 0x1b
    loser.armour = PADDED_ARMOUR;
    Loot::Armour(taken)
}

/// `TakeGold`, image 0xbb9: half the loser's purse, rounded down, and the
/// loser keeps the other half.
fn take_gold(winner: &mut Record, loser: &mut Record) -> u32 {
    // 00bb9  mov ax, [di+0x32]; shr ax, 1
    let ax = loser.gold >> 1;
    // 00bbe  add [si+0x32], ax; mov [di+0x32], ax
    winner.gold = winner.gold.saturating_add(ax);
    loser.gold = ax;
    ax
}

/// `TakeALL`, image 0xbc5: everything in the magic record.
fn take_all(winner: &mut Record, loser: &mut Record) {
    for ax in TAKE_MAGIC_TABLE {
        match ax {
            // 00c12: the bit fields OR'd across.
            0x16 | 0x14 => {
                let bits = winner.hoard.field(ax) | loser.hoard.field(ax);
                winner.hoard.set_field(ax, bits);
                loser.hoard.set_field(ax, 0);
            }
            // 00c3e  cmp byte ptr [si], 0; je next
            // 00c4b  mov word ptr [di + 0x40], 0x19; jmp 00bee
            4 => {
                if loser.hoard.field(4) == 0 {
                    continue;
                }
                loser.weapon = SWORD_OF_SHARPNESS;
                let sum = winner.hoard.field(4).saturating_add(loser.hoard.field(4));
                winner.hoard.set_field(4, sum);
                loser.hoard.set_field(4, 0);
            }
            // 00bee..00c10: added across and zeroed.
            _ => {
                let sum = winner.hoard.field(ax).saturating_add(loser.hoard.field(ax));
                winner.hoard.set_field(ax, sum);
                loser.hoard.set_field(ax, 0);
            }
        }
    }
}

// -------------------------------------------------------- a computer knight

/// One of the three seats the machine plays, which is a knight record at
/// DS:`0x6c9e + 0x62 * n` for `n` in one to three.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Rival {
    /// `+0x4c` the name, `+0x2e` to `+0x30` the abilities, `+0x34` the
    /// daggers, `+0x40` and `+0x42` the blade and the suit, and `+0x20` the
    /// colour, which is four.
    pub knight: Knight,
    /// `+0x5c`, `+0x5e`: the token's top left corner.
    pub x: i32,
    pub y: i32,
    /// `+0x2a`, `+0x2c`: the grid cell `CalcKnGrid` last wrote.
    pub grid: (i32, i32),
    /// `+0x31`.
    pub lives: i32,
    /// `+0x38`, `+0x3c`.
    pub health: i32,
    pub max_health: i32,
    /// `+0x32`.
    pub gold: u32,
    /// `+0x36`.
    pub experience: u32,
    /// `[+0x44]`: `MagicP2` to `MagicP4`.
    pub hoard: Hoard,
    /// `+0x3a`: days left as a toad. Nothing on his day sends him to the
    /// wizard, so it stays nought; `AdjustTIME` and `NextWHICH` read it all
    /// the same.
    pub toad: u32,
    /// `+0x46`: the knight record he is after, by record index, or nought.
    pub target: Option<usize>,
}

impl Rival {
    /// `InitGameStart` and `SetKnightEquipment` (0x1fa2) for record `n`: the
    /// enemy name, colour 4, the corner, one of each ability, five life
    /// points, ten daggers, ten gold, a long sword and padded armour, and the
    /// health the routine at 0x28d makes of that.
    pub fn new(record: usize, items: &Items) -> Rival {
        let n = record.min(RECORDS - 1);
        let knight = Knight {
            name: ENEMY_NAMES[n].to_string(),
            seat: COMPUTER_COLOUR,
            strength: 1,
            constitution: 1,
            endurance: 1,
            daggers: 10,
            weapon: "long_sword".into(),
            armour: "padded_armour".into(),
        };
        let max_health = knight.max_health(items);
        Rival {
            knight,
            x: ENEMY_CORNERS[n].0,
            y: ENEMY_CORNERS[n].1,
            grid: (0, 0),
            lives: 5,
            health: max_health,
            max_health,
            gold: 10,
            experience: 0,
            hoard: Hoard::default(),
            toad: 0,
            target: None,
        }
    }

    /// `[si+0x31] > 0`: on the map as a rider rather than a grave.
    pub fn alive(&self) -> bool {
        self.lives > 0
    }

    /// The routine at 0x28d: `10 * constitution + armour + 10`, the rings at
    /// twenty each, and health held under the ceiling.
    pub fn refresh(&mut self, items: &Items) {
        self.max_health = self.knight.max_health(items) + i32::from(self.hoard.rings) * 20;
        if self.health > self.max_health {
            self.health = self.max_health;
        }
    }

    /// `DistanceDONE+12` (0xa4be): `[di+0x3e] << 4`, and once more for haste.
    pub fn day_steps(&self, items: &Items, hasted: bool) -> u32 {
        let base = self.knight.steps_per_day(items);
        if hasted {
            base * 2
        } else {
            base
        }
    }

    /// The record the loot routines read.
    pub fn record(&self) -> Record {
        Record {
            kind: 8,
            lives: self.lives,
            gold: self.gold,
            health: self.health,
            max_health: self.max_health,
            weapon: weapon_code(&self.knight.weapon),
            armour: armour_code(&self.knight.armour),
            hoard: self.hoard,
        }
    }

    /// The record written back, and the health redone as `REFF` (0xb8a)
    /// does through 0x279.
    pub fn write_record(&mut self, rec: &Record, items: &Items) {
        self.lives = rec.lives;
        self.gold = rec.gold;
        self.health = rec.health;
        self.knight.weapon = weapon_id(rec.weapon).to_string();
        self.knight.armour = armour_id(rec.armour).to_string();
        self.hoard = rec.hoard;
        self.refresh(items);
    }

    /// `WhoLived` (0xabe) for this record: put down, whole again, and a life
    /// point the poorer. Returns whether he was down.
    pub fn who_lived(&mut self) -> bool {
        // 00acc  cmp word ptr [si + 0x38], 0; jg
        if self.health > 0 {
            return false;
        }
        // 00ad7  mov ax, [si+0x3c]; mov [si+0x38], ax; sub byte [si+0x31], 1
        self.health = self.max_health;
        self.lives -= 1;
        true
    }

    /// `AdjustTIME` (0x119f) for this record: a toad a day less of a toad,
    /// and a quarter of the missing health back, never less than a point.
    pub fn adjust_time(&mut self) {
        // 011b5  cmp byte ptr [si + 0x3a], 0; je; sub byte ptr [si + 0x3a], 1
        self.toad = self.toad.saturating_sub(1);
        // 011bf  mov bx, [si+0x3c]; sub bx, [si+0x38]; je; shr bx, 2; or bx, 1
        let mut bx = self.max_health - self.health;
        if bx != 0 {
            bx = (bx >> 2) | 1;
        }
        // 011ce  add [si+0x38], bx; and held under the ceiling
        self.health += bx;
        if self.health > self.max_health {
            self.health = self.max_health;
        }
    }

    /// `CalcKnGrid` (0xab83): `(x + 4) >> 3` and `(y + 10) >> 3` into
    /// `+0x2a` and `+0x2c`.
    pub fn calc_grid(&mut self) {
        self.grid = ((self.x + TOKEN_W / 2) >> 3, (self.y + TOKEN_H) >> 3);
    }
}

// ------------------------------------------------------------- the day's scratch

/// `TrackRoute`'s six words at DS:`0xcd36` to `0xcd40`: the line a computer
/// knight walks this turn, written once and stepped every frame.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Track {
    /// `X_dir`: 1 right or 2 left.
    pub x_dir: u32,
    /// `Y_dir`: 4 down or 8 up.
    pub y_dir: u32,
    /// `step`: the error term.
    pub step: i32,
    /// `x_abs`, `y_abs`: how far to go each way.
    pub x_abs: i32,
    pub y_abs: i32,
    /// `what_constant`: nought when x is the long axis, minus one when y is.
    pub y_major: bool,
}

/// `SUPPLYPrice`, `SUPPLYIndex` and `SUPPLY` at DS:`0xc424`: what a computer
/// knight would buy if he reached a town.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Supply {
    pub price: u32,
    /// The record field the goods go in: 0x42 armour, 0x40 weapon, 0x34
    /// daggers, 0x31 the healer. `None` is the 0xffff `KnightGoesToTown`
    /// returns on.
    pub index: Option<u16>,
    pub goods: u16,
}

/// The words in `_MAP` a computer knight's turn is kept in, all of which
/// `NextWHICH+51` (0xa46f to 0xa48b) clears between turns.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Turn {
    /// `[0xcc98]`: the frames this turn has spent.
    pub steps: u32,
    /// `LairFLAG`: the closest lairs have been found.
    pub lair_flag: bool,
    /// `LookFLAG`: the other knights have been looked at.
    pub look_flag: bool,
    /// `BlackKnightFLAG`: the route has been laid.
    pub black_knight_flag: bool,
    /// `CloseLair`: the lair he is bound for, by table index.
    pub close_lair: Option<usize>,
    /// `NoLairsFLAG`: the lair the roll picked is off the map.
    pub no_lairs: bool,
    /// `TownX`, `TownY`: nought and nought when he is not going to town.
    pub town: Option<(i32, i32)>,
    pub supply: Supply,
    pub track: Track,
}

/// What a frame of a computer knight's turn came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame {
    /// A pixel walked, or a frame stood.
    Walked,
    /// `GoTheDistance` (0xa422): the day is spent, and `NextWHICH` follows.
    TurnOver,
    /// `BKCollision+85` (0xab06): he is standing on the knight he is after,
    /// and `Combat+102` (0x3b7) is next, with him as the challenger. The
    /// rest of the day is spent whatever comes of it.
    Challenge { target: usize },
    /// `BKCollision+40` (0xaad9): he is in a town, `KnightInTown` has been
    /// through the purse, and the day is spent.
    Shopped,
    /// `BKCollision+117` (0xab26): he is on the lair he was bound for, and
    /// the day is spent. Nothing else happens there.
    AtLair,
    /// `DragonEncounter+54` (0xa41b) into 0xcf3: the dragon came down on
    /// him, took a life point and one thing, and the day is spent.
    Dragon(Loot),
}

/// Everything a computer knight's frame looks at that is not the run's.
pub struct Board<'a> {
    pub land: &'a Landscape,
    pub places: &'a Places,
    pub items: &'a Items,
    /// The player's token: record 0's `+0x5c` and `+0x5e`.
    pub player_at: (i32, i32),
}

/// `SearchEncounter` (0xab2f) over what the walker at 0x6b5 pushed: which of
/// the things under this token the caller asked about is there. This is
/// the stack rebuilt from the same overlap the walker makes.
struct Stack {
    /// Kinds 0x19 and 0x1a: a town's box under the token.
    town: bool,
    /// `CheckEncounterDone` (0x798): the knight records whose 8 by 10 token
    /// overlaps this one, by record index.
    knights: Vec<usize>,
    /// `CheckLairEncounter` (0x85a): the lairs on the map under the token.
    lairs: Vec<usize>,
}

fn spans(a: i32, aw: i32, b: i32, bw: i32) -> bool {
    a < b + bw && b < a + aw
}

/// The sort at 0xa7ad to 0xa7db over `RangeTABLE`, pairs of a distance word
/// and a lair record pointer, as the code has it:
///
/// ```text
/// 0a7ad  mov ax, 0x17
/// 0a7b0  mov si, RangeTABLE; mov bx, ax
/// 0a7b5  mov cx, [si]; or cx, cx; js 0a7c0      ; 0xffff: to the back
/// 0a7bb  cmp cx, [si+4]; jb 0a7d4               ; unsigned, and equal swaps
/// 0a7c0  push word [si+4]; pop word [si]
/// 0a7c5  mov [si+4], cx
/// 0a7c8  mov cx, [si+2]
/// 0a7cb  push word [si+6]; pop word [si+2]
/// 0a7d1  mov [si+4], cx                         ; sic
/// 0a7d4  add si, 4; dec bx; jne 0a7b5
/// 0a7da  dec ax; jne 0a7b0
/// ```
///
/// The second half of the swap moves the record pointer up and then writes
/// the pointer that came down over the distance word at `[si+4]`, where
/// `[si+6]` was meant. So the lower entry keeps its old neighbour's record
/// and carries a record pointer for a distance, and nothing puts it right.
pub fn range_sort(table: &mut [(u16, u16)]) {
    let mut ax = 0x17;
    while ax != 0 {
        let mut si = 0;
        let mut bx = ax;
        while bx != 0 {
            let cx = table[si].0;
            // 0a7b7  or cx, cx; js swap; 0a7bb cmp cx, [si+4]; jb skip
            if (cx as i16) < 0 || cx >= table[si + 1].0 {
                // 0a7c0  push [si+4]; pop [si]; mov [si+4], cx
                table[si].0 = table[si + 1].0;
                table[si + 1].0 = cx;
                // 0a7c8  mov cx, [si+2]; push [si+6]; pop [si+2]
                let cx = table[si].1;
                table[si].1 = table[si + 1].1;
                // 0a7d1  mov [si+4], cx
                table[si + 1].0 = cx;
            }
            si += 1;
            bx -= 1;
        }
        ax -= 1;
    }
}

impl Run {
    /// `InitGameStart` and `SetKnightEquipment` for the seats the player did
    /// not take: records one to three.
    pub fn seat_the_rivals(&mut self, items: &Items) {
        self.rivals = (1..RECORDS).map(|n| Rival::new(n, items)).collect();
    }

    /// The four records' `+0x5c` and `+0x5e`, given the player's own.
    pub fn positions(&self, player_at: (i32, i32)) -> [(i32, i32); RECORDS] {
        let mut at = [player_at; RECORDS];
        for (n, r) in self.rivals.iter().enumerate() {
            if let Some(slot) = at.get_mut(n + 1) {
                *slot = (r.x, r.y);
            }
        }
        at
    }

    /// `+0x31 > 0` for each of the four knight records at DS:`0x6c9e`, as
    /// `ContinueDragon` rolls over them and `NextWHICH` passes over them.
    pub fn knights_alive(&self) -> [bool; RECORDS] {
        let mut alive = [false; RECORDS];
        alive[0] = !self.over;
        for (n, r) in self.rivals.iter().enumerate() {
            if let Some(slot) = alive.get_mut(n + 1) {
                *slot = r.alive();
            }
        }
        alive
    }

    /// The player's record as the loot routines read it, with kind 6.
    pub fn player_record(&self) -> Record {
        Record {
            kind: 6,
            lives: self.lives,
            gold: self.gold,
            health: self.health,
            max_health: self.max_health,
            weapon: weapon_code(&self.knight.weapon),
            armour: armour_code(&self.knight.armour),
            hoard: Hoard::of(&self.kit, self.knight.weapon == crate::lair::MAGIC_SWORD),
        }
    }

    /// The player's record written back into the run: the purse, the blade,
    /// the suit, the life points, and every magic count into the pack.
    pub fn write_player_record(&mut self, rec: &Record, items: &Items) {
        self.lives = rec.lives;
        self.gold = rec.gold;
        self.health = rec.health;
        self.knight.weapon = weapon_id(rec.weapon).to_string();
        self.knight.armour = armour_id(rec.armour).to_string();
        // `TakeSword` and `TakeALL` keep the sword of sharpness in the hand
        // through `+0x40`, and the routine at 0x28d through `+4`.
        if rec.hoard.magic_sword && rec.weapon == SWORD_OF_SHARPNESS {
            self.knight.weapon = crate::lair::MAGIC_SWORD.to_string();
        }
        self.set_kit_to(&rec.hoard);
        if self.lives <= 0 {
            self.lives = 0;
            self.over = true;
        }
        self.refresh(items);
    }

    /// The pack made to hold what a magic record says, count for count.
    fn set_kit_to(&mut self, hoard: &Hoard) {
        use crate::status::{SCROLL_SLOTS, SLOT_TABLE};
        let mut set = |id: &str, n: u32| {
            let have = self.kit.count(id);
            if have > n {
                self.kit.lose(id, have - n);
            } else if n > have {
                self.kit.take(id, n - have);
            }
        };
        for (slot, field) in [(18usize, 0u16), (19, 2), (20, 6), (21, 8)] {
            if let Some(id) = SLOT_TABLE[slot].item {
                set(id, u32::from(hoard.field(field)));
            }
        }
        for (i, slot) in SCROLL_SLOTS.into_iter().enumerate() {
            if let Some(id) = SLOT_TABLE[slot].item {
                set(id, u32::from(hoard.scrolls[i]));
            }
        }
        for key in crate::moon::Key::ALL {
            set(key.item(), u32::from(hoard.keys & key.bit() != 0));
        }
        for stone in crate::moon::Moonstone::ALL {
            set(stone.item(), u32::from(hoard.moonstones & stone.bit() != 0));
        }
    }

    /// A record by index: the player's, or a computer knight's.
    pub fn record(&self, index: usize) -> Option<Record> {
        if index == 0 {
            return Some(self.player_record());
        }
        self.rivals.get(index - 1).map(Rival::record)
    }

    /// A record written back by index.
    pub fn write_record(&mut self, index: usize, rec: &Record, items: &Items) {
        if index == 0 {
            self.write_player_record(rec, items);
        } else if let Some(r) = self.rivals.get_mut(index - 1) {
            r.write_record(rec, items);
        }
    }

    /// `WhoLived+57` (0xaf7) between two records: `winner` takes off `loser`.
    pub fn take_between(&mut self, winner: usize, loser: usize, items: &Items) -> Loot {
        if winner == loser {
            return Loot::Nothing;
        }
        let (Some(mut w), Some(mut l)) = (self.record(winner), self.record(loser)) else {
            return Loot::Nothing;
        };
        let loot = take_from_the_fallen(&mut w, &mut l);
        self.write_record(winner, &w, items);
        self.write_record(loser, &l, items);
        loot
    }

    /// The name on a record: `[si+0x4c]`.
    pub fn record_name(&self, index: usize) -> String {
        if index == 0 {
            return self.knight.name.clone();
        }
        self.rivals
            .get(index - 1)
            .map_or_else(String::new, |r| r.knight.name.clone())
    }

    /// The routine at 0x1148 for the three computer knights: `AdjustTIME`
    /// walks five records, and these are three of them.
    pub fn rivals_new_day(&mut self) {
        for r in &mut self.rivals {
            r.adjust_time();
        }
    }

    /// `_WIZARD:RND` (0xbd89), which every roll in `_MAP` goes through.
    fn map_rnd(&mut self) -> u16 {
        self.rnd_seed = rnd(self.rnd_seed);
        self.rnd_seed
    }

    // ---------------------------------------------------- the turn's routines

    /// The routine at 0xa75c, which the `PUBLIC` list calls `FINDCLOSELAIR`:
    /// once a turn, the distance to every lair, a sort, and a roll among
    /// the closest.
    ///
    /// ```text
    /// 0a75c  cmp word [LairFLAG], 0; je; ret
    /// 0a764  mov word [LairFLAG], 1
    /// 0a76a  mov si, [0x920]; mov di, [0x77e8]; mov bp, RangeTABLE
    /// 0a775  mov ax, [di+0x5c]; mov bx, [di+0x5e]; mov cx, 0x18
    /// 0a77e  push cx; mov cx, [si+0xa]; or cx, cx; jns 0a78b
    /// 0a786  mov dx, 0xffff; jmp 0a79c           ; off the map: no distance
    /// 0a78b  mov dx, [si+0xc]; sub cx, ax; jns; neg cx; sub dx, bx; jns; neg dx
    /// 0a79a  add dx, cx                          ; |dx| + |dy|
    /// 0a79c  pop cx; mov [bp], dx; mov [bp+2], si ; distance, record
    /// 0a7a5  add bp, 4; add si, 0x12; loop 0a77e
    /// 0a7ad  mov ax, 0x17
    /// 0a7b0  mov si, RangeTABLE; mov bx, ax
    /// 0a7b5  mov cx, [si]; or cx, cx; js 0a7c0    ; off the map: to the back
    /// 0a7bb  cmp cx, [si+4]; jb 0a7d4            ; in order already
    /// 0a7c0  push word [si+4]; pop word [si]     ; the pair swapped
    /// 0a7c5  mov [si+4], cx
    /// 0a7c8  mov cx, [si+2]
    /// 0a7cb  push word [si+6]; pop word [si+2]
    /// 0a7d1  mov [si+4], cx                      ; sic: the record over the distance
    /// 0a7d4  add si, 4; dec bx; jne 0a7b5
    /// 0a7da  dec ax; jne 0a7b0
    /// 0a7dd  mov si, RangeTABLE; call RND; and ax, 3; jne; mov ax, 1
    /// 0a7eb  shl ax, 1; shl ax, 1; add si, ax
    /// 0a7f1  push word [si+2]; pop word [CloseLair]  ; the second to fourth
    /// 0a7f8  mov word [NoLairsFLAG], 0
    /// 0a7fe  mov si, [CloseLair]; cmp word [si+0xa], 0; jns; mov word [NoLairsFLAG], 1
    /// ```
    ///
    /// The instruction at 0xa7d1 is the sort's own slip: the second `mov` of
    /// the swap writes the moved record pointer over the distance word at
    /// `[si+4]` instead of into `[si+6]`, so after every swap the entry that
    /// moved down carries its old neighbour's record and a distance that is
    /// a record pointer. The lair records sit at DS:`0x0002` every 0x12
    /// bytes, so those pointers are 2 to 416, which is the same range as a
    /// real distance across the map, and the roll at 0xa7e0 lands on
    /// whatever that leaves in the second to fourth places. Kept as it is,
    /// pointers and all, because it is what decides where he goes.
    fn find_close_lair(&mut self, at: (i32, i32), lairs: &[Option<(i32, i32)>]) {
        // 0a75c  cmp word ptr [LairFLAG], 0; je; ret
        if self.turn.lair_flag {
            return;
        }
        self.turn.lair_flag = true;
        // 0a76a..0a7ab: RangeTABLE, a distance and a record pointer a lair.
        let mut table: Vec<(u16, u16)> = (0..24)
            .map(|n| {
                let ptr = 2 + 0x12 * n as u16;
                match lairs.get(n).copied().flatten() {
                    Some((lx, ly)) if lx >= 0 => {
                        let d = (lx - at.0).abs() + (ly - at.1).abs();
                        (d as u16, ptr)
                    }
                    _ => (0xffff, ptr),
                }
            })
            .collect();
        // 0a7ad..0a7db: the sort, slip and all.
        range_sort(&mut table);
        // 0a7e0  call RND; and ax, 3; jne; mov ax, 1
        let mut roll = (self.map_rnd() & 3) as usize;
        if roll == 0 {
            roll = 1;
        }
        // 0a7f1  the record pointer of that entry, back to a table index.
        let ptr = table[roll].1;
        let index = ((ptr - 2) / 0x12) as usize;
        self.turn.close_lair = Some(index);
        // 0a7f8..0a80e
        self.turn.no_lairs = !matches!(lairs.get(index).copied().flatten(), Some((x, _)) if x >= 0);
    }

    /// `FindKnight` (0xa80f) through `KnightMagicLoop` (0xa8aa): once a turn,
    /// the other knights by distance, and one of them to go after.
    ///
    /// ```text
    /// 0a80f  cmp word [LookFLAG], 0; je; ret
    /// 0a817  mov word [LookFLAG], 1
    /// 0a81d  KDistance's four words zeroed
    /// 0a82c  mov si, 0x6c9e; mov di, [0x77e8]; bp = KDistance; bx = KPlayer; cx = 4
    /// FindKnightLoop:
    /// 0a83d  cmp di, si; je FindNextKnight       ; not himself
    /// 0a842  ..call FindDistance                 ; |dx| + |dy| to the record
    /// 0a852  mov [bx], si; add bx, 2; mov [bp], dx; add bp, 2
    /// FindNextKnight:
    /// 0a85f  add si, 0x62; loop FindKnightLoop
    /// BubbleSort:                                ; the first three, by distance
    /// 0a899  mov si, KPlayer; mov di, [0x77e8]
    /// 0a8a0  cmp word [di+0x46], 0; je; ret      ; already after somebody
    /// 0a8a7  mov cx, 4
    /// KnightMagicLoop:
    /// 0a8aa  mov bp, [si]; add si, 2
    /// 0a8af  cmp di, bp; je NextKnightMagic
    /// 0a8b3  cmp word [bp+0x20], 4; je NextKnightMagic   ; never a computer knight
    /// 0a8ba  cmp word [NoLairsFLAG], 0; jne 0a8d8        ; no lair to go to: him
    /// 0a8c1  mov bx, [bp+0x44]; call BKDecideCombat
    /// 0a8c8  cmp ax, 2; je 0a8d8                          ; the roll said him
    /// 0a8cd  or ax, ax; jne NextKnightMagic               ; nothing worth having
    /// 0a8d1  call CheckBother; or ax, ax; jne NextKnightMagic
    /// 0a8d8  mov [di+0x46], bp; ret
    /// NextKnightMagic:
    /// 0a8dc  loop KnightMagicLoop
    /// BKDecideCombat:
    /// 0a8e0  mov ax, 1
    /// 0a8e3  mov bx, [bp+0x44]
    /// 0a8e7  cmp byte [bx+0x16], 0; je; sub ax, ax        ; he has a moonstone
    /// 0a8ef  cmp byte [bx+0x14], 0; je; sub ax, ax        ; or a key
    /// 0a8f7  call RND; and ax, 0x7f; cmp ax, 0x19; jg; mov ax, 2   ; 26 in 128
    /// CheckBother:
    /// 0a908  call RND; and ax, 0x7f; mov bx, ax; mov ax, 1
    /// 0a913  cmp bx, 0x19; jg; sub ax, ax                 ; 26 in 128
    /// ```
    ///
    /// `KPlayer` gets three records written and the loop reads four: the
    /// fourth word is BSS the sort never touches, so it is a null pointer,
    /// and what it reads through it lands in the lair table at DS:`0x20` and
    /// DS:`0x44`. Neither read can come to a target: `[0x20]` is a lair's
    /// row and never four, so the entry goes on to `NoLairsFLAG` or a roll,
    /// and either way what it writes into `+0x46` is the null pointer itself,
    /// which is no target. The three real entries are walked here and the
    /// fourth is not.
    fn find_knight(&mut self, which: usize, at: [(i32, i32); RECORDS]) {
        // 0a80f  cmp word ptr [LookFLAG], 0; je; ret
        if self.turn.look_flag {
            return;
        }
        self.turn.look_flag = true;
        // 0a82c..0a862: the three other records and their distances.
        let mut table: Vec<(i32, usize)> = (0..RECORDS)
            .filter(|n| *n != which)
            .map(|n| {
                let d = (at[n].0 - at[which].0).abs() + (at[n].1 - at[which].1).abs();
                (d, n)
            })
            .collect();
        // BubbleSort (0xa864): `mov cx, 3; dec cx`, so two neighbours a pass
        // over the first three, until a pass makes no swap.
        loop {
            let mut swapped = false;
            for i in 0..2 {
                // 0a870  mov dx, [si+2]; cmp dx, [si]; jae
                if table[i + 1].0 < table[i].0 {
                    table.swap(i, i + 1);
                    swapped = true;
                }
            }
            if !swapped {
                break;
            }
        }
        // 0a8a0  cmp word ptr [di + 0x46], 0; je; ret
        let Some(me) = self.rivals.get(which.wrapping_sub(1)) else {
            return;
        };
        if me.target.is_some() {
            return;
        }
        for (_, n) in table {
            // 0a8b3  cmp word ptr ds:[bp + 0x20], 4; je NextKnightMagic: only
            // record 0 is a person here.
            if n != 0 {
                continue;
            }
            // 0a8ba  cmp word ptr [NoLairsFLAG], 0; jne target
            if self.turn.no_lairs {
                self.set_target(which, Some(n));
                return;
            }
            // BKDecideCombat, 0xa8df.
            let his = self.player_record().hoard;
            let mut ax = 1;
            if his.field(0x16) != 0 {
                ax = 0;
            }
            if his.field(0x14) != 0 {
                ax = 0;
            }
            // 0a8f7  call RND; and ax, 0x7f; cmp ax, 0x19; jg; mov ax, 2
            if (self.map_rnd() & 0x7f) <= 0x19 {
                ax = 2;
            }
            // 0a8c8  cmp ax, 2; je target
            if ax == 2 {
                self.set_target(which, Some(n));
                return;
            }
            // 0a8cd  or ax, ax; jne NextKnightMagic
            if ax != 0 {
                continue;
            }
            // CheckBother, 0xa907: 0a913 cmp bx, 0x19; jg; sub ax, ax
            if (self.map_rnd() & 0x7f) <= 0x19 {
                self.set_target(which, Some(n));
                return;
            }
        }
    }

    fn set_target(&mut self, which: usize, target: Option<usize>) {
        if let Some(r) = self.rivals.get_mut(which.wrapping_sub(1)) {
            r.target = target;
        }
    }

    /// `KnightXP` (0xac61): a point of ability bought whenever the
    /// experience covers `[0x718]`, picked by the wizard's own roll.
    ///
    /// ```text
    /// 0ac65  mov ax, [0x718]; cmp ax, [si+0x36]; jg ret
    /// 0ac6d  call 0xb7e9                     ; the WIZABL picker
    /// 0ac70  or bx, bx; je ret               ; all three at five
    /// 0ac78  inc byte [bx+si]
    /// 0ac7a  mov ax, [0x718]; sub [si+0x36], ax
    /// ```
    fn knight_xp(&mut self, which: usize) {
        let cost = self.xp_per_level;
        let roll = self.map_rnd();
        let Some(r) = self.rivals.get_mut(which.wrapping_sub(1)) else {
            return;
        };
        // 0ac6b  jg ret
        if cost > r.experience {
            return;
        }
        // 0ac6d  call 0xb7e9; or bx, bx; je ret
        let Some(which) = r.knight.rolled_ability(u32::from(roll)) else {
            return;
        };
        // 0ac78  inc byte ptr [bx + si]
        r.knight.raise_unchecked(which);
        // 0ac7d  sub word ptr [si + 0x36], ax. Nothing here calls 0x28d, so
        // a point of constitution moves the ceiling the next time something
        // does: a suit bought, a fight won, a thing taken.
        r.experience -= cost;
    }

    /// `KnightHeal` (0xabc6): a potion opened when a life point is missing or
    /// the health is not whole.
    ///
    /// ```text
    /// 0abca  cmp byte [si+0x31], 5; jl HealLife
    /// 0abd0  mov ax, [si+0x3c]; mov bx, [si+0x38]; cmp bx, ax; je BKHealDone
    /// HealLife:
    /// 0abda  mov di, [si+0x44]; cmp byte [di], 0; je BKHealDone
    /// 0abe2  dec byte [di]; call 0xcad0
    /// ```
    ///
    /// and 0xcad0 is the potion: whole already, a life point up to five;
    /// otherwise whole.
    fn knight_heal(&mut self, which: usize) {
        let Some(r) = self.rivals.get_mut(which.wrapping_sub(1)) else {
            return;
        };
        // 0abca / 0abd6
        if r.lives >= 5 && r.health == r.max_health {
            return;
        }
        // 0abdd  cmp byte ptr [di], 0; je
        if r.hoard.potions == 0 {
            return;
        }
        // 0abe2  dec byte ptr [di]
        r.hoard.potions -= 1;
        // 0cad5  mov ax, [si+0x3c]; cmp ax, [si+0x38]; jne 0caea
        if r.health == r.max_health {
            // 0cadd  inc byte [si+0x31]; cmp byte [si+0x31], 6; jb; mov byte [si+0x31], 5
            r.lives += 1;
            if r.lives >= 6 {
                r.lives = 5;
            }
        }
        // 0caea  push [si+0x3c]; pop [si+0x38]
        r.health = r.max_health;
    }

    /// `KnightAquire` (0xac19): a scroll of aquisition read at the knight
    /// he is after, wherever he is.
    ///
    /// ```text
    /// 0ac1d  cmp word [si+0x46], 0; je ret
    /// 0ac23  mov di, [si+0x44]; cmp byte [di+0xe], 0; je ret
    /// 0ac2c  dec byte [di+0xe]
    /// 0ac2f  mov di, [si+0x46]; call 0xaf7      ; WhoLived+57: si takes off di
    /// ```
    fn knight_aquire(&mut self, which: usize, items: &Items) -> Option<Loot> {
        let r = self.rivals.get_mut(which.wrapping_sub(1))?;
        let target = r.target?;
        if r.hoard.field(0xe) == 0 {
            return None;
        }
        r.hoard.set_field(0xe, r.hoard.field(0xe) - 1);
        Some(self.take_between(which, target, items))
    }

    /// `KnightHaste` (0xac36): a scroll of haste when the knight he is after
    /// is further than the day would reach.
    ///
    /// ```text
    /// 0ac3a  mov bp, [si+0x44]; cmp byte [bp+0xa], 0; je ret
    /// 0ac44  cmp word [si+0x46], 0; je ret
    /// 0ac4a  mov di, [si+0x46]; call GetDistance
    /// 0ac50  cmp dx, [0xccac]; jl ret
    /// 0ac56  dec byte [bp+0xa]; mov word [0xcca2], 0xffff
    /// ```
    fn knight_haste(&mut self, which: usize, at: [(i32, i32); RECORDS], budget: u32) {
        let Some(r) = self.rivals.get_mut(which.wrapping_sub(1)) else {
            return;
        };
        if r.hoard.field(0xa) == 0 {
            return;
        }
        let Some(target) = r.target else {
            return;
        };
        let d = (at[target].0 - at[which].0).abs() + (at[target].1 - at[which].1).abs();
        // 0ac50  cmp dx, [0xccac]; jl ret
        if d < budget as i32 {
            return;
        }
        r.hoard.set_field(0xa, r.hoard.field(0xa) - 1);
        self.hasted = true;
    }

    /// `KnightSupplies` (0xac81): what the purse would buy, into `SUPPLY*`.
    ///
    /// ```text
    /// 0ac81  TownX, TownY, SUPPLYPrice, SUPPLY = 0; SUPPLYIndex = 0xffff
    /// 0aca3  cmp word [si+0x32], 0xa; jg; ret          ; ten gold or under: nothing
    /// 0acaa  cmp word [si+0x32], 0xf; jle 0acc3
    /// 0acb0  cmp byte [si+0x31], 2; jg 0acc3
    /// 0acb6  price 0xf, index 0x31; ret                 ; the healer, two lives or fewer
    /// 0acc3  cmp word [si+0x32], 0x4b; jl 0ace2
    /// 0acc9  cmp word [si+0x42], 0x1e; je SupplyWeopon
    /// 0accf  price 0x4b, index 0x42, goods 0x1e; ret    ; battle armour
    /// 0ace2  cmp word [si+0x32], 0x32; jl 0ad01
    /// 0ace8  cmp word [si+0x42], 0x1d; jge SupplyWeopon
    /// 0acee  price 0x32, index 0x42, goods 0x1d; ret    ; plate
    /// 0ad01  cmp word [si+0x32], 0x1e; jl SupplyWeopon
    /// 0ad07  cmp word [si+0x42], 0x1c; jge SupplyWeopon
    /// 0ad0d  price 0x1e, index 0x42, goods 0x1c; ret    ; chain mail
    /// SupplyWeopon:
    /// 0ad20  cmp word [si+0x32], 0x19; jl w1$
    /// 0ad26  cmp word [si+0x40], 0x18; je w2$
    /// 0ad2c  price 0x19, index 0x40, goods 0x18; ret    ; a claymore
    /// w1$:
    /// 0ad3f  cmp word [si+0x32], 0xa; jl w2$
    /// 0ad45  cmp word [si+0x40], 0x17; jge w2$
    /// 0ad4b  price 0xa, index 0x40, goods 0x17; ret     ; a broad sword
    /// w2$:
    /// 0ad5e  cmp byte [si+0x34], 5; jg w3$
    /// 0ad64  price 2, index 0x34, goods 0; ret          ; daggers
    /// ```
    ///
    /// The claymore test is `je` on 0x18 and nothing else, so a knight with
    /// the sword of sharpness in his hand and twenty five gold in his purse
    /// buys a claymore over it. Kept.
    fn knight_supplies(&mut self, which: usize) {
        self.turn.town = None;
        self.turn.supply = Supply::default();
        let Some(r) = self.rivals.get(which.wrapping_sub(1)) else {
            return;
        };
        let (gold, lives) = (r.gold, r.lives);
        let (weapon, armour) = (weapon_code(&r.knight.weapon), armour_code(&r.knight.armour));
        let daggers = r.knight.daggers;
        let mut want = |price: u32, index: u16, goods: u16| {
            self.turn.supply = Supply {
                price,
                index: Some(index),
                goods,
            };
        };
        // 0aca3  cmp word ptr [si + 0x32], 0xa; jg; ret
        if gold <= 0xa {
            return;
        }
        // 0acaa / 0acb0
        if gold > 0xf && lives <= 2 {
            want(0xf, 0x31, 0);
            return;
        }
        // 0acc3..0ad1f: the suits, dearest first.
        let weapon_next = if gold >= 0x4b {
            if armour == BATTLE_ARMOUR {
                true
            } else {
                want(0x4b, 0x42, BATTLE_ARMOUR);
                return;
            }
        } else if gold >= 0x32 {
            if armour >= PLATE_ARMOUR {
                true
            } else {
                want(0x32, 0x42, PLATE_ARMOUR);
                return;
            }
        } else if gold >= 0x1e {
            if armour >= CHAIN_MAIL {
                true
            } else {
                want(0x1e, 0x42, CHAIN_MAIL);
                return;
            }
        } else {
            true
        };
        debug_assert!(weapon_next);
        // SupplyWeopon, 0xad20.
        if gold >= 0x19 {
            if weapon != CLAYMORE {
                want(0x19, 0x40, CLAYMORE);
                return;
            }
        } else if gold >= 0xa && weapon < BROAD_SWORD {
            // w1$
            want(0xa, 0x40, BROAD_SWORD);
            return;
        }
        // w2$
        if daggers <= 5 {
            want(2, 0x34, 0);
        }
    }

    /// `KnightGoesToTown` (0xad78): the nearer of the two cities, by grid
    /// cell, when there is something to buy.
    ///
    /// ```text
    /// 0ad78  cmp word [SUPPLYIndex], -1; jne; ret
    /// 0ad84  ax = [si+0x2a], bx = [si+0x2c], cx = 0xc, dx = 7; call FindDistance
    /// 0ad93  mov [HighwoodDistance], dx
    /// 0ad9d  cx = 0x25, dx = 0x14; call FindDistance; mov [WaterdeepDistance], dx
    /// 0adaa  cmp dx, [HighwoodDistance]; jl 0adbd
    /// 0adb0  TownX = 0x5e, TownY = 0x2f; ret         ; Highwood
    /// 0adbd  TownX = 0x129, TownY = 0x9d; ret        ; Waterdeep
    /// ```
    fn knight_goes_to_town(&mut self, which: usize) {
        if self.turn.supply.index.is_none() {
            return;
        }
        let Some(r) = self.rivals.get(which.wrapping_sub(1)) else {
            return;
        };
        let (ax, bx) = r.grid;
        let highwood = (0xc - ax).abs() + (7 - bx).abs();
        let waterdeep = (0x25 - ax).abs() + (0x14 - bx).abs();
        self.turn.town = Some(if waterdeep < highwood {
            (0x129, 0x9d)
        } else {
            (0x5e, 0x2f)
        });
    }

    /// `KnightInTown` (0xadca): the purse paid and the goods put where they
    /// go.
    ///
    /// ```text
    /// 0adce  mov ax, [SUPPLYPrice]; sub [si+0x32], ax
    /// 0add4  cmp [SUPPLYIndex], 0x42; je MerchantArmour
    /// 0addb  cmp [SUPPLYIndex], 0x40; je MerchantWeopon
    /// 0ade2  cmp [SUPPLYIndex], 0x34; je MerchantDagger
    /// 0ade9  cmp [SUPPLYIndex], 0x31; je CityHealer
    /// MerchantArmour:  [si+0x42] = SUPPLY; call 0x28d; call 0x2e7
    /// MerchantWeopon:  [si+0x40] = SUPPLY
    /// MerchantDagger:  inc byte [si+0x34]; cmp word [si+0x32], 2; jl ret
    ///                  cmp byte [si+0x34], 0xa; je ret; sub word [si+0x32], 2; jmp MerchantDagger
    /// CityHealer:      push [si+0x3c]; pop [si+0x38]; inc byte [si+0x31]
    /// ```
    fn knight_in_town(&mut self, which: usize, items: &Items) {
        let supply = self.turn.supply;
        let Some(r) = self.rivals.get_mut(which.wrapping_sub(1)) else {
            return;
        };
        // 0add1  sub word ptr [si + 0x32], ax
        r.gold = r.gold.saturating_sub(supply.price);
        match supply.index {
            Some(0x42) => {
                r.knight.armour = armour_id(supply.goods).to_string();
                r.refresh(items);
            }
            Some(0x40) => {
                r.knight.weapon = weapon_id(supply.goods).to_string();
            }
            Some(0x34) => loop {
                // 0ae07  inc byte ptr [si + 0x34]
                r.knight.daggers += 1;
                // 0ae0a  cmp word ptr [si + 0x32], 2; jl ret
                if r.gold < 2 {
                    break;
                }
                // 0ae10  cmp byte ptr [si + 0x34], 0xa; je ret
                if r.knight.daggers == 0xa {
                    break;
                }
                // 0ae16  sub word ptr [si + 0x32], 2
                r.gold -= 2;
            },
            Some(0x31) => {
                r.health = r.max_health;
                r.lives += 1;
            }
            _ => {}
        }
    }

    /// The walker at 0x6b5 as it runs for this token: `CheckGROOC` over
    /// `MapIconsTABLE` for the towns, `CheckEncounterDone` over the other
    /// three records, `CheckLairEncounter` over the lairs on the map.
    fn stack_under(
        &self,
        which: usize,
        at: [(i32, i32); RECORDS],
        board: &Board,
        lairs: &[Option<(i32, i32)>],
    ) -> Stack {
        let (x, y) = at[which];
        let mut town = false;
        for def in board.places.values() {
            let is_town = def
                .options
                .iter()
                .any(|c| matches!(c.effect, Effect::Door { .. }));
            if is_town && def.covers_for(x, y, COMPUTER_COLOUR) {
                town = true;
            }
        }
        // 007a9..007bf: the other records' tokens, 8 by 10 against 8 by 10.
        let knights = (0..RECORDS)
            .filter(|n| *n != which)
            .filter(|n| {
                spans(x, TOKEN_W, at[*n].0, TOKEN_W) && spans(y, TOKEN_H, at[*n].1, TOKEN_H)
            })
            .collect();
        // 00865..00886: the lairs, frame 0x1f's nine by five.
        let lairs = lairs
            .iter()
            .enumerate()
            .filter_map(|(n, spot)| spot.map(|s| (n, s)))
            .filter(|(_, (lx, ly))| {
                *lx >= 0 && spans(x, TOKEN_W, *lx, 9) && spans(y, TOKEN_H, *ly, 5)
            })
            .map(|(n, _)| n)
            .collect();
        Stack {
            town,
            knights,
            lairs,
        }
    }

    /// `TrackLair` (0xa9e1): the route laid once a turn, and one frame of
    /// it.
    ///
    /// ```text
    /// 0a9e1  mov word [JOYS], 0
    /// 0a9eb  cmp word [BlackKnightFLAG], 0; jne TrackStart
    /// 0a9f2  mov word [BlackKnightFLAG], 1
    /// 0a9f8  mov ax, [si+0x5c]; mov bx, [si+0x5e]
    /// 0a9fe  cmp word [si+0x46], 0; je 0aa0f
    /// 0aa04  mov di, [si+0x46]; cx = [di+0x5c]; dx = [di+0x5e]; jmp TrackRoute   ; the knight
    /// 0aa0f  cmp word [TownX], 0; jne; cmp word [TownY], 0; je 0aa27
    /// 0aa1d  cx = [TownX]; dx = [TownY]; jmp TrackRoute                          ; the town
    /// 0aa27  mov si, [CloseLair]; cx = [si+0xa]; dx = [si+0xc]                   ; the lair
    /// TrackRoute:
    /// 0aa31  sub cx, ax; pushf; mov ax, 1; popf; jns; neg cx; shl ax, 1   ; X_dir 1 or 2
    /// 0aa3e  sub dx, bx; pushf; mov bx, 4; popf; jns; neg dx; shl bx, 1   ; Y_dir 4 or 8
    /// 0aa4b  mov di, 0; mov bp, cx; cmp cx, dx; jge; mov di, -1; mov bp, dx
    /// 0aa59  x_abs = cx; y_abs = dx; X_dir = ax; Y_dir = bx; step = bp; what_constant = di
    /// TrackStart:
    /// 0aa70  cmp word [what_constant], 0; js Y_loop
    /// 0aa77  mov ax, [X_dir]; mov bx, [step]; sub bx, [y_abs]; jns 0aa8c
    /// 0aa84  add bx, [x_abs]; or ax, [Y_dir]
    /// 0aa8c  mov [step], bx; mov [JOYS], ax; ret
    /// Y_loop: the same with the axes swapped
    /// ```
    ///
    /// So the long axis moves every frame and the short one when the error
    /// runs out, which is a line; the six words are written once, so the
    /// line is aimed at where the target stood when the turn began and is
    /// walked past the far end if the day is longer than it.
    fn track_lair(
        &mut self,
        which: usize,
        at: [(i32, i32); RECORDS],
        lairs: &[Option<(i32, i32)>],
    ) -> u32 {
        // 0a9eb  cmp word ptr [BlackKnightFLAG], 0; jne TrackStart
        if !self.turn.black_knight_flag {
            self.turn.black_knight_flag = true;
            let (ax, bx) = at[which];
            let target = self
                .rivals
                .get(which.wrapping_sub(1))
                .and_then(|r| r.target);
            let (cx, dx) = if let Some(t) = target {
                at[t]
            } else if let Some(town) = self.turn.town {
                town
            } else {
                let lair = self
                    .turn
                    .close_lair
                    .and_then(|n| lairs.get(n).copied().flatten());
                // A record whose x is 0xffff, which is what a cleared lair
                // holds, is walked towards all the same.
                lair.unwrap_or((-1, -1))
            };
            // TrackRoute, 0xaa31.
            let mut cx = cx - ax;
            let mut x_dir = 1;
            if cx < 0 {
                cx = -cx;
                x_dir = 2;
            }
            let mut dx = dx - bx;
            let mut y_dir = 4;
            if dx < 0 {
                dx = -dx;
                y_dir = 8;
            }
            let (y_major, step) = if cx >= dx { (false, cx) } else { (true, dx) };
            self.turn.track = Track {
                x_dir,
                y_dir,
                step,
                x_abs: cx,
                y_abs: dx,
                y_major,
            };
        }
        // TrackStart, 0xaa70.
        let t = &mut self.turn.track;
        if !t.y_major {
            let mut ax = t.x_dir;
            let mut bx = t.step - t.y_abs;
            if bx < 0 {
                bx += t.x_abs;
                ax |= t.y_dir;
            }
            t.step = bx;
            ax
        } else {
            let mut ax = t.y_dir;
            let mut bx = t.step - t.x_abs;
            if bx < 0 {
                bx += t.y_abs;
                ax |= t.x_dir;
            }
            t.step = bx;
            ax
        }
    }

    /// `CheckSLOW` (0xa728) for this token: `SlowDELAY` stepped on hard
    /// ground, and `SlowFLAG` when the mask catches it.
    fn check_slow(&self, which: usize, land: &Landscape, going: &mut u32) -> bool {
        let Some(r) = self.rivals.get(which.wrapping_sub(1)) else {
            return false;
        };
        let mask = land.going_at(r.x, r.y);
        if mask == 0 {
            return false;
        }
        *going = going.wrapping_add(1);
        *going & mask != 0
    }

    /// The lairs as `CheckLairEncounter` and the routine at 0xa75c read
    /// them: `[si+0xa]` and `[si+0xc]` by table index, negative once
    /// `CheckLairClear` has written 0xffff over a beaten, emptied one.
    pub fn lair_spots(&self, places: &Places) -> Vec<Option<(i32, i32)>> {
        let mut spots: Vec<Option<(i32, i32)>> = Vec::new();
        for def in places.values() {
            for c in &def.options {
                if let Effect::Raid { lair, .. } = &c.effect {
                    if spots.len() <= *lair {
                        spots.resize(*lair + 1, None);
                    }
                    spots[*lair] = Some(if self.lair_on_the_map(*lair) {
                        (def.x, def.y)
                    } else {
                        (-1, -1)
                    });
                }
            }
        }
        spots
    }

    /// One frame of a computer knight's turn: `MapLOOP+19` (0xa319) round
    /// to `DistanceDONE` (0xa4b2), for the record `WHICH` names.
    ///
    /// `going` is `SlowDELAY`, which is one word for everybody.
    pub fn rival_frame(&mut self, board: &Board, going: &mut u32) -> Frame {
        let which = self.which;
        if which == 0 || which > self.rivals.len() {
            return Frame::TurnOver;
        }
        let items = board.items;
        let lairs = self.lair_spots(board.places);
        let at = self.positions(board.player_at);
        // 0a2f1  call DistanceDONE+12: the budget, this frame's.
        let budget = self.rivals[which - 1].day_steps(items, self.hasted);
        // 0a319  call 0xa75c
        self.find_close_lair(at[which], &lairs);
        // 0a31c  call KnightXP
        self.knight_xp(which);
        // 0a31f  call KnightHeal
        self.knight_heal(which);
        // 0a322  call FindKnight
        self.find_knight(which, at);
        // 0a325  call KnightAquire
        self.knight_aquire(which, items);
        // 0a328  call KnightWyrm
        {
            let day = self.moon_moves();
            let alive = self.knights_alive();
            let (chosen, scrolls) = self
                .rivals
                .get(which - 1)
                .map_or((None, 0), |r| (r.target, u32::from(r.hoard.field(0x10))));
            let mut seed = self.rnd_seed;
            if self
                .dragon
                .knight_wyrm(day, chosen, scrolls, alive, &mut seed)
            {
                if let Some(r) = self.rivals.get_mut(which - 1) {
                    r.hoard.set_field(0x10, r.hoard.field(0x10) - 1);
                }
            }
            self.rnd_seed = seed;
        }
        // 0a32b  call KnightHaste
        self.knight_haste(which, at, budget);
        // 0a32e  call KnightSupplies; 0a331 call KnightGoesToTown
        self.knight_supplies(which);
        self.knight_goes_to_town(which);
        // An encounter in the last frame's BKCollision or DragonEncounter
        // spent the day (`[0xcc98] = [0xccac]`), and that frame ran on into
        // GoTheDistance and NextWHICH before another BKCollision could run.
        // The caller took the encounter between the two, so this is where
        // the frame picks up.
        if self.turn.steps >= budget {
            return Frame::TurnOver;
        }
        // 0a334  call BKCollision
        let stack = self.stack_under(which, at, board, &lairs);
        let collided = self.bk_collision(which, &stack, items, budget);
        if let Some(frame) = collided {
            return frame;
        }
        // 0a337  call CheckSLOW
        let slow = self.check_slow(which, board.land, going);
        // 0a33a..0a353
        let mut bx = 0;
        // 0a33c  cmp word ptr [0xcc9e], 0: the hawk's flag, never his.
        self.turn.steps += 1;
        if !slow {
            bx = self.track_lair(which, at, &lairs);
        }
        // ScrollINPUT, 0xa3bb onwards: no keys for a kind 4 record, and no
        // fire in `JOYS`. DragonEncounter, 0xa3e2.
        if self.dragon.comes_down_on(which, false) {
            return self.dragon_on_rival(which, budget, items);
        }
        // GoTheDistance, 0xa422.
        if self.turn.steps >= budget {
            return Frame::TurnOver;
        }
        // DistanceDONE: FOLLOW, which is HawkBorders and the move.
        self.follow(which, bx);
        // The walker's shadow table, and the dragon's own task after SHOW.
        let at = self.positions(board.player_at);
        self.dragon_frame_over(at);
        Frame::Walked
    }

    /// `FOLLOW` (0xa29f) for this record: `HawkBorders` clears the bits that
    /// would leave the rectangle, and the rest move the token a pixel.
    fn follow(&mut self, which: usize, joys: u32) {
        let Some(r) = self.rivals.get_mut(which.wrapping_sub(1)) else {
            return;
        };
        let mut joys = joys;
        if joys != 0 {
            // HawkBorders, 0xa9a9.
            if r.x <= 0 {
                joys &= !2;
            }
            if r.x >= 0x136 {
                joys &= !1;
            }
            if r.y <= 0 {
                joys &= !8;
            }
            if r.y >= 0xbe {
                joys &= !4;
            }
            // 0a2b0..0a2cd
            if joys & 1 != 0 {
                r.x += 1;
            }
            if joys & 2 != 0 {
                r.x -= 1;
            }
            if joys & 8 != 0 {
                r.y -= 1;
            }
            if joys & 4 != 0 {
                r.y += 1;
            }
            r.x = r.x.clamp(0, MAX_X);
            r.y = r.y.clamp(0, MAX_Y);
        }
        // 0a2d0  call CalcKnGrid
        r.calc_grid();
    }

    /// `BKCollision` (0xaab1): what the stack under him means.
    ///
    /// ```text
    /// 0aab1  cmp word [TownX], 0; jne; cmp word [TownY], 0; je 0aaf2
    /// 0aabf  bx = 0x19, cx = 4; call SearchEncounter; or ax, ax; jne 0aad9
    /// 0aacc  bx = 0x1a, cx = 4; call SearchEncounter; or ax, ax; je BKCollideDone
    /// 0aad9  push [0xccac]; pop [0xcc98]            ; the day spent
    /// 0aae1  call KnightInTown; TownX = TownY = 0; jmp BKCollideDone
    /// 0aaf2  mov si, [0x77e8]; mov bx, [si+0x46]; or bx, bx; je 0ab19
    /// 0aafd  sub cx, cx; call SearchEncounter; or ax, ax; je BKCollideDone
    /// 0ab06  mov di, [si+0x46]; call Combat+102      ; the fight
    /// 0ab0c  call 0xa962                            ; the effect flags down
    /// 0ab0f  push [0xccac]; pop [0xcc98]; jmp BKCollideDone
    /// 0ab19  sub cx, cx; mov bx, [CloseLair]; call SearchEncounter
    /// 0ab22  or ax, ax; je BKCollideDone
    /// 0ab26  push [0xccac]; pop [0xcc98]            ; the day spent, and nothing else
    /// ```
    fn bk_collision(
        &mut self,
        which: usize,
        stack: &Stack,
        items: &Items,
        budget: u32,
    ) -> Option<Frame> {
        if self.turn.town.is_some() {
            if !stack.town {
                return None;
            }
            self.turn.steps = budget;
            self.knight_in_town(which, items);
            self.turn.town = None;
            return Some(Frame::Shopped);
        }
        let target = self.rivals.get(which - 1).and_then(|r| r.target);
        if let Some(t) = target {
            if !stack.knights.contains(&t) {
                return None;
            }
            // 0ab0c  call 0xa962; 0ab0f the day spent
            self.hasted = false;
            self.turn.steps = budget;
            return Some(Frame::Challenge { target: t });
        }
        let close = self.turn.close_lair?;
        if !stack.lairs.contains(&close) {
            return None;
        }
        self.turn.steps = budget;
        Some(Frame::AtLair)
    }

    /// The routine at 0xcf3 for a kind 4 record, which `DragonEncounter+54`
    /// (0xa41b) calls when the dragon's shadow is over the knight it is
    /// after and it is his turn.
    ///
    /// ```text
    /// 00cf3  mov ax, [0x77e8]; mov [KnightTable], ax; mov si, [KnightTable]
    /// 00cfd  cmp word [si+0x20], 4; jne _notblackknight
    /// 00d03  sub byte [si+0x31], 1                 ; a life point, no fight
    /// 00d07  jmp _dragon_won
    /// _dragon_won:
    /// 00d23  mov si, 0x6e26; mov di, [KnightTable]; call 0xaf7   ; the dragon takes
    /// 00d2d  or word [KnightDeath], 1
    /// 00d32  jmp EncounterAllDone                  ; the day spent
    /// ```
    ///
    /// The dragon's record over the map has kind 0x14 (`ContinueDragon+92`,
    /// 0xa60f) and `InitKnightvsDragon` is not called on this path, so the
    /// `cmp byte [si+0x35], 0xa` at 0xafa does not take his gold. What it
    /// takes goes into the dragon's own magic record, `[0x6e26+0x44]`.
    fn dragon_on_rival(&mut self, which: usize, budget: u32, items: &Items) -> Frame {
        let mut dragon = Record {
            kind: 0x14,
            lives: 1,
            gold: 0,
            health: 0,
            max_health: 0,
            weapon: 0,
            armour: 0,
            hoard: self.dragon_hoard,
        };
        let Some(r) = self.rivals.get_mut(which - 1) else {
            return Frame::TurnOver;
        };
        // 00d03  sub byte ptr [si + 0x31], 1
        r.lives -= 1;
        let mut loser = r.record();
        let loot = take_from_the_fallen(&mut dragon, &mut loser);
        r.write_record(&loser, items);
        self.dragon_hoard = dragon.hoard;
        // EncounterAllDone: the day spent.
        self.turn.steps = budget;
        Frame::Dragon(loot)
    }

    /// `DragonWander` and the shadow table for the four positions.
    fn dragon_frame_over(&mut self, at: [(i32, i32); RECORDS]) {
        if !self.dragon.aloft {
            return;
        }
        let row = self
            .dragon
            .target
            .and_then(|t| at.get(t))
            .map_or(self.dragon.z, |p| p.1);
        self.dragon.wander(row);
        self.dragon.frame_ends(at, crate::dragon::SHADOW_SIZE);
    }

    // ------------------------------------------------------------ the turns

    /// `NextWHICH` (0xa434): the next record whose turn it is, and the day
    /// turned when the four have had theirs.
    ///
    /// ```text
    /// NextWHICH:
    /// 0a434  call 0xa962                ; the three effect flags off
    /// 0a437  inc word [WHICH]
    /// 0a43b  mov word [NextFLAG], 0
    /// 0a441  mov word [0xcc98], 0
    /// 0a447  and word [WHICH], 3; jne 0a463
    /// 0a44e  call 0x1148                ; the day turns
    /// 0a451  call 0xa554; call 0x8e5b; call 0x824c; call 0x59e1   ; the moon screen
    /// 0a45d  mov word [0xccaa], 0
    /// 0a463  call DistanceDONE+12       ; the record, its kind, its budget
    /// 0a466  mov si, [0x77e8]; mov word [si+0x46], 0
    /// 0a46f  TownX, TownY, BlackKnightFLAG, LairFLAG, LookFLAG = 0
    /// 0a48d  cmp byte [si+0x3a], 0; jg NextWHICH          ; a toad: passed over
    /// 0a493  cmp byte [si+0x31], 0; jle 0a49c
    /// 0a499  jmp 0xa2d7                                    ; his turn
    /// 0a49c  cmp word [si+0x20], 4; je NextWHICH           ; a dead computer knight: passed over
    /// 0a4a2  inc word [0xccaa]; mov ax, [0xccaa]
    /// 0a4a9  cmp ax, [NUM_PLAYERS]; jne NextWHICH          ; a dead person: counted
    /// 0a4af  jmp 0x617                                     ; all of them dead: game over
    /// ```
    ///
    /// Returns whose turn begins and whether the day turned first, or
    /// nothing when the run is over. The routine at 0x1148 is
    /// [`Run::new_day`] and [`Run::rivals_new_day`].
    pub fn next_which(&mut self) -> Option<TurnBegins> {
        let mut day_turned = false;
        // Four seats, and the player's own answers within four passes.
        for _ in 0..RECORDS + 1 {
            // 0a434  call 0xa962
            self.hasted = false;
            // 0a437  inc word ptr [WHICH]; 0a447 and word ptr [WHICH], 3
            self.which = (self.which + 1) & 3;
            self.turn = Turn::default();
            if self.which == 0 {
                // 0a44e  call 0x1148
                self.new_day();
                self.rivals_new_day();
                day_turned = true;
            }
            // 0a466  mov word ptr [si + 0x46], 0
            let which = self.which;
            if which > 0 {
                self.set_target(which, None);
            }
            let (toad, alive) = if self.which == 0 {
                (self.is_toad(), !self.over)
            } else {
                match self.rivals.get(self.which - 1) {
                    Some(r) => (r.toad > 0, r.alive()),
                    None => (false, false),
                }
            };
            // 0a48d  cmp byte ptr [si + 0x3a], 0; jg NextWHICH
            if toad {
                continue;
            }
            // 0a493  cmp byte ptr [si + 0x31], 0; jle
            if alive {
                return Some(TurnBegins {
                    which: self.which,
                    day_turned,
                });
            }
            // 0a49c  cmp word ptr [si + 0x20], 4; je NextWHICH
            if self.which > 0 {
                continue;
            }
            // 0a4a2..0a4af: one player, and he is dead.
            return None;
        }
        None
    }
}

/// What `NextWHICH` settled on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TurnBegins {
    /// `WHICH`: 0 for the player, 1 to 3 for a computer knight.
    pub which: usize,
    /// The routine at 0x1148 ran on the way: the moon screen is next.
    pub day_turned: bool,
}

// ---------------------------------------------------------- knight vs knight

/// What `Combat+102` (0x3b7) decided before any blow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Challenged {
    /// `0x3cc` and `0x3d5`: the challenged is a toad or a grave, so the
    /// challenger wins without a fight. `Knight1Won` follows.
    Walkover,
    /// `KnightProtection` returned one: the scroll turned him away.
    Averted,
    /// `InitKnightBattle`: the fight is on. `cursed` is `KnightCursed` after
    /// a scroll that backfired.
    Fight { cursed: bool },
    /// `0x3f0`: two computer knights, and nothing comes of it.
    Nothing,
}

/// What `InitKnightBattle+65` (0x440) onwards made of the fight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Settled {
    /// `BothKnightsDied`.
    BothDied,
    /// `Knight1Won` with a person as the winner: the trade page, `StatTYPE`
    /// 1, is his to take from `loser`.
    PlayerWon { loser: usize },
    /// `BKwon`: a computer knight won, took his point and his loot.
    RivalWon { winner: usize, loot: Loot },
}

impl Run {
    /// `Combat+102` (0x3b7) up to `InitKnightBattle`: `attacker` is `si`,
    /// the record whose turn it is, and `defender` is `di`.
    ///
    /// ```text
    /// 003c4  mov [0x8979], si; mov [0x897b], di
    /// 003cc  cmp byte [di+0x3a], 0; je; jmp Knight1Won   ; a toad
    /// 003d5  cmp byte [di+0x31], 0; jg; jmp Knight1Won   ; a grave
    /// 003de  call KnightProtection; or ax, ax; je; jmp HeadBattleDone
    /// 003f0  cmp word [si+0x20], 4; jne InitKnightBattle
    /// 003f6  cmp word [di+0x20], 4; jne InitKnightBattle
    /// 003fc  jmp HeadBattleDone
    /// ```
    ///
    /// `KnightProtection` (0x4e8) is only for a person: `cmp word [di+0x20],
    /// 4; je` returns nought for a computer knight at once. For a person it
    /// is the scroll he has already cast here, [`Run::challenged`].
    pub fn challenge(&mut self, attacker: usize, defender: usize) -> Challenged {
        let (toad, alive) = if defender == 0 {
            (self.is_toad(), !self.over)
        } else {
            match self.rivals.get(defender.wrapping_sub(1)) {
                Some(r) => (r.toad > 0, r.alive()),
                None => return Challenged::Nothing,
            }
        };
        // 003cc / 003d5
        if toad || !alive {
            return Challenged::Walkover;
        }
        // 003de  call KnightProtection
        let cursed = if defender == 0 {
            match self.challenged() {
                crate::run::Challenge::Averted => return Challenged::Averted,
                crate::run::Challenge::Backfired => true,
                crate::run::Challenge::Fight => false,
            }
        } else {
            false
        };
        // 003f0 / 003f6
        if attacker != 0 && defender != 0 {
            return Challenged::Nothing;
        }
        Challenged::Fight { cursed }
    }

    /// `WhoLived` (0xabe) and `InitKnightBattle+65` (0x440) onwards, after a
    /// fight between `attacker` and `defender` that ended with these two
    /// healths.
    ///
    /// ```text
    /// 00436  mov word [si+0x46], 0; mov word [di+0x46], 0
    /// 00440  cmp word [KnightDeath], 3; je BothKnightsDied
    /// 00447  test word [KnightDeath], 1; je Knight1Won   ; si stood
    /// 0044f  ksw = 1; si and di swapped                   ; di stood
    /// Knight1Won:
    /// 00465  cmp word [si+0x20], 4; je BKwon
    /// 0046b  mov ax, 1; call the panel                    ; a person takes
    /// BKwon:
    /// 0049f  add word [si+0x36], 1; mov word [si+0x46], 0
    /// 004a8  call BKAddstuff; call 0xaf7; jmp HeadBattleDone
    /// ```
    ///
    /// For the player's record `WhoLived` is [`Run::finished_fight_worth`],
    /// which is where the life point goes; for a computer knight's it is
    /// [`Rival::who_lived`].
    pub fn knight_fight_over(
        &mut self,
        attacker: usize,
        defender: usize,
        attacker_health: i32,
        defender_health: i32,
        items: &Items,
    ) -> Settled {
        let (player_health, rival_index, rival_health) = if attacker == 0 {
            (attacker_health, defender, defender_health)
        } else {
            (defender_health, attacker, attacker_health)
        };
        // WhoLived, both records.
        let player_died = player_health <= 0;
        self.finished_fight_worth(player_health, !player_died, 0, 0);
        let rival_died = match self.rivals.get_mut(rival_index.wrapping_sub(1)) {
            Some(r) => {
                r.health = rival_health;
                r.target = None;
                r.who_lived()
            }
            None => false,
        };
        // 00440  cmp word ptr [KnightDeath], 3
        if player_died && rival_died {
            return Settled::BothDied;
        }
        // 00447 / 0044f: the one who stood is the winner.
        if !player_died {
            return Settled::PlayerWon { loser: rival_index };
        }
        self.bk_won(rival_index, 0, items)
    }

    /// `BKwon` (0x49f) and `BKAddstuff` (0x4b0) for a computer knight over
    /// `loser`: the only levelling that happens inside a bout rather than on
    /// the status screen, and it is his alone. `Knight1Won+6` (0x46b) sends
    /// a person to the trade page instead and pays him nothing.
    ///
    /// ```text
    /// BKwon:
    /// 0049f  add word [si+0x36], 1        ; a point for putting him down
    /// 004a3  mov word [si+0x46], 0
    /// 004a8  call BKAddstuff
    /// 004ab  call 0xaf7                   ; WhoLived+57: what he takes
    /// BKAddstuff:
    /// 004b1  mov ax, [si+0x36]
    /// 004b4  cmp ax, [0x718]
    /// 004b8  jl  ret                      ; not enough yet
    /// 004ba  call RND
    /// 004bd  and ax, 3
    /// 004c0  cmp ax, 2; jle; mov ax, 2    ; three in four is the last one
    /// 004c8  mov bx, ax
    /// 004ca  inc byte [bx+si+0x2e]        ; and no ceiling is asked about
    /// 004cd  cmp bx, 1; jne 004d6
    /// 004d2  add word [si+0x38], 0xa      ; constitution also adds ten now
    /// 004d6  mov cx, [si+0x36]; sub cx, [0x718]; mov [si+0x36], cx
    /// 004e0  call the two health routines
    /// ```
    ///
    /// Three things it does that the status screen's `HGAbility` does not.
    /// The roll is flat and **not** the weighted one `WIZABL` holds, so the
    /// three abilities are 1, 1 and 2 in four rather than the wizard's
    /// weighting. It **skips no ability that is already at five**: the
    /// increment at 0x4ca is unconditional, which is the one way in the game
    /// past the ceiling. And the ten hit points at 0x4d2 are added before the
    /// maximum is recomputed at 0x4e0, so a point of constitution pays twice
    /// on the frame it lands.
    pub fn bk_won(&mut self, winner: usize, loser: usize, items: &Items) -> Settled {
        let roll = self.map_rnd();
        let cost = self.xp_per_level;
        if let Some(r) = self.rivals.get_mut(winner.wrapping_sub(1)) {
            // 0049f  add word ptr [si + 0x36], 1; mov word ptr [si + 0x46], 0
            r.experience = r.experience.saturating_add(1);
            r.target = None;
            // BKAddstuff, 0x4b0.
            if r.experience >= cost {
                // 004bd  and ax, 3; cmp ax, 2; jle; mov ax, 2
                let bx = (roll & 3).min(2) as usize;
                r.knight.raise_unchecked(Ability::ALL[bx]);
                // 004cd  cmp bx, 1; jne; add word ptr [si + 0x38], 0xa
                if bx == 1 {
                    r.health += 10;
                }
                r.experience -= cost;
                r.refresh(items);
            }
        }
        // 004ab  call 0xaf7
        let loot = self.take_between(winner, loser, items);
        Settled::RivalWon { winner, loot }
    }

    /// `Knight1Won` when the defender never fought: a toad or a grave under
    /// the challenger. `attacker` wins outright.
    pub fn walkover(&mut self, attacker: usize, defender: usize, items: &Items) -> Settled {
        if let Some(r) = self.rivals.get_mut(attacker.wrapping_sub(1)) {
            r.target = None;
        }
        if attacker == 0 {
            Settled::PlayerWon { loser: defender }
        } else {
            self.bk_won(attacker, defender, items)
        }
    }
}

// ------------------------------------------------------------- the trade page

impl Run {
    /// One gadget on the trade page, `StatTYPE` 1, pressed: the winner,
    /// `StatHAND1`, takes from the loser, `StatHAND2`, which is record
    /// `loser`. `field` is the gadget's `STPL`.
    ///
    /// `HotGadget` (0xca0a) sends `STRP` 1 to `HGTakeMagic` and everything
    /// with the right arch's `0x20` bit to `TakeGold`:
    ///
    /// ```text
    /// HGTakeMagic:
    /// 0cbbc  cmp bx, 0x16; je HGTakeMoonstone
    /// 0cbc1  cmp bx, 0x14; je HGTakeMoonstone
    /// 0cbc6  cmp bx, 4; jne; jmp TakeSword
    /// 0cbce  dec byte [bx+di]; inc byte [bx+si]          ; one across
    /// 0cbd2  cmp bx, 6; jne HGTakeDone
    /// 0cbd7  mov si, [StatHAND1]; add word [si+0x38], 0x14; call 0x28d   ; a ring's twenty
    /// HGTakeMoonstone:
    /// 0cbe4  mov al, [bx+di]; mov byte [bx+di], 0; or [bx+si], al       ; the field whole
    /// HGTakeDone:
    /// 0cbeb  inc word [TakeCNT]; 0x28d and 0x2e7 for StatHAND1; ReDisplay
    /// TakeGold:
    /// 0cc6d  cmp bx, 0x32; jne; jmp TKGP
    /// 0cc75  cmp bx, 0x42; je TKAR
    /// 0cc7a  cmp bx, 0x40; je TKWP
    /// TKAR:
    /// 0cc82  mov bp, [di+0x42]; cmp bp, 0x1b; jne; jmp RT    ; padded: nothing
    /// 0cc8d  dx = bp - 0x1b; al = [0xf452 + dx]; cwde
    /// 0cc9c  sub [di+0x38], ax                               ; the loser's health
    /// 0cc9f  cmp bp, [si+0x42]; jl 0ccad                      ; no better than mine
    /// 0cca4  push [di+0x42]; pop [si+0x42]; add [si+0x38], ax ; mine now
    /// 0ccb0  mov word [di+0x42], 0x1b; jmp HGTakeDone          ; padded either way
    /// TKWP:
    /// 0ccb8  mov bp, [di+0x40]; cmp bp, 0x16; je ret         ; a long sword: nothing
    /// 0ccc0  cmp bp, [si+0x40]; jl 0ccc8; mov [si+0x40], bp   ; mine if no worse
    /// 0ccc8  mov word [di+0x40], 0x16; jmp HGTakeDone
    /// TakeSword:
    /// 0ccd4  mov word [si+4], 1; mov word [di+4], 0
    /// 0ccde  mov si, [StatHAND1]; mov word [si+0x40], 0x19
    /// 0cce7  cmp word [StatTYPE], 2; je HGTakeDone
    /// 0ccee  mov si, [StatHAND2]; mov word [si+0x40], 0x16
    /// TKGP:
    /// 0cd12  lea di, [di+0x32]; cmp word [di], 0; je ReDisplay
    /// 0cd1a  cmp word [si+0x32], 0x96; je 0cd2b              ; a hundred and fifty
    /// 0cd21  dec word [di]; inc word [si+0x32]; cmp word [di], 0; jne 0cd1a
    /// 0cd2b  inc word [TakeCNT]
    /// ```
    ///
    /// `TKAR` takes the loser's suit off him and his health with it whether
    /// or not the winner wants it: a worse suit goes to nobody. Returns
    /// whether `TakeCNT` was stepped, which `ReDisplay` (0xbee6) reads to
    /// turn the page into the winner's own sheet when the loser is alive.
    pub fn trade_take(&mut self, loser: usize, field: u16, items: &Items) -> bool {
        let (Some(mut w), Some(mut l)) = (self.record(0), self.record(loser)) else {
            return false;
        };
        let taken = match field {
            // TKGP
            0x32 => {
                if l.gold == 0 {
                    return false;
                }
                while l.gold > 0 && w.gold != crate::service::PURSE_CEILING {
                    l.gold -= 1;
                    w.gold += 1;
                }
                true
            }
            // TKAR
            0x42 => {
                let bp = l.armour;
                if bp == PADDED_ARMOUR {
                    return false;
                }
                let ax = ARMOUR_HEALTH[(bp.saturating_sub(PADDED_ARMOUR) as usize).min(3)];
                l.health -= ax;
                if bp >= w.armour {
                    w.armour = bp;
                    w.health += ax;
                }
                l.armour = PADDED_ARMOUR;
                true
            }
            // TKWP
            0x40 => {
                let bp = l.weapon;
                if bp == LONG_SWORD {
                    return false;
                }
                if bp >= w.weapon {
                    w.weapon = bp;
                }
                l.weapon = LONG_SWORD;
                true
            }
            // HGTakeMoonstone
            0x14 | 0x16 => {
                let al = l.hoard.field(field);
                l.hoard.set_field(field, 0);
                w.hoard.set_field(field, w.hoard.field(field) | al);
                true
            }
            // TakeSword
            4 => {
                w.hoard.magic_sword = true;
                l.hoard.magic_sword = false;
                w.weapon = SWORD_OF_SHARPNESS;
                l.weapon = LONG_SWORD;
                true
            }
            // 0cbce: one across, and a ring pays at once.
            0x00 | 0x02 | 0x06 | 0x08 | 0x0a | 0x0c | 0x0e | 0x10 | 0x12 => {
                let have = l.hoard.field(field);
                if have == 0 {
                    return false;
                }
                l.hoard.set_field(field, have - 1);
                w.hoard
                    .set_field(field, w.hoard.field(field).saturating_add(1));
                if field == 6 {
                    w.health += 0x14;
                }
                true
            }
            _ => return false,
        };
        self.write_record(0, &w, items);
        self.write_record(loser, &l, items);
        taken
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{ItemDef, Virtue};
    use crate::knight::KnightDef;

    #[rustfmt::skip]
    fn goods() -> Items {
        let mut items = Items::new();
        let mut put = |id: &str, name: &str, price: u32, consumed: bool, virtue: Virtue| {
            items.insert(id.into(), ItemDef { name: name.into(), price, virtue, consumed });
        };
        put("long_sword", "Long sword", 0, false, Virtue::Weapon { damage: 0 });
        put("broad_sword", "Broad sword", 10, false, Virtue::Weapon { damage: 2 });
        put("claymore", "Claymore sword", 25, false, Virtue::Weapon { damage: 3 });
        put("sword_of_sharpness", "Sword of Sharpness", 0, false, Virtue::Weapon { damage: 5 });
        put("padded_armour", "Padded armour", 0, false, Virtue::Armour { health: 0, stride: 0 });
        put("chain_mail", "Chain mail", 30, false, Virtue::Armour { health: 10, stride: 2 });
        put("plate_armour", "Plate armour", 50, false, Virtue::Armour { health: 20, stride: 0 });
        put("battle_armour", "Battle armour", 75, false, Virtue::Armour { health: 30, stride: 2 });
        put("potion", "Potion of Healing", 20, true, Virtue::Restore);
        put("gem_of_seeing", "Gem of Seeing", 32, true, Virtue::Sight { astray: 0, returns: true });
        put("ring_of_protection", "Ring of Protection", 50, false, Virtue::Ward { health: 20 });
        put("talisman_of_the_wyrm", "Talisman", 52, false, Virtue::Inert);
        put("scroll_of_haste", "Scroll of Haste", 36, true, Virtue::Haste);
        put("scroll_of_the_hawk", "Scroll of the Hawk", 52, true, Virtue::Sight { astray: 16, returns: false });
        put("scroll_of_acquisition", "Scroll of Aquisition", 52, true, Virtue::Seize);
        put("scroll_of_the_wyrm", "Scroll of the Wyrm", 52, false, Virtue::Wyrm);
        put("scroll_of_protection", "Scroll of Protection", 24, true, Virtue::Protection { backfire: 11 });
        items
    }

    fn run() -> Run {
        let items = goods();
        let def = KnightDef {
            name: "SIR GODBER".into(),
            shades: vec![0],
            home: [10, 10],
            strength: 1,
            constitution: 1,
            endurance: 1,
            life: 5,
            daggers: 10,
            gold: 10,
            weapon: "long_sword".into(),
            armour: "padded_armour".into(),
        };
        let mut r = Run::for_knight(&def, 0, &items);
        r.kit.capacity = 1000;
        r
    }

    fn record(gold: u32, weapon: u16, armour: u16) -> Record {
        Record {
            kind: 6,
            lives: 3,
            gold,
            health: 20,
            max_health: 20,
            weapon,
            armour,
            hoard: Hoard::default(),
        }
    }

    /// `InitGameStart` (0x1c0d): three records the player did not take,
    /// named off `Enemy2Name` to `Enemy4Name`, colour 4, at the corners, and
    /// `SetKnightEquipment`'s sheet.
    #[test]
    fn the_three_seats_nobody_took_are_the_enemy_knights() {
        let mut r = run();
        assert_eq!(r.rivals.len(), 3, "Run::for_knight seats them");
        let names: Vec<&str> = r.rivals.iter().map(|k| k.knight.name.as_str()).collect();
        assert_eq!(names, ["SIR DWAIN", "SIR BALAIN", "SIR GUNTHER"]);
        let at: Vec<(i32, i32)> = r.rivals.iter().map(|k| (k.x, k.y)).collect();
        assert_eq!(at, [(300, 100), (160, 20), (160, 180)], "0x1c95..0x1cd8");
        for k in &r.rivals {
            assert_eq!(k.knight.seat, COMPUTER_COLOUR, "0x1c71: [di+0x20] = 4");
            assert_eq!((k.lives, k.gold, k.knight.daggers), (5, 10, 10));
            assert_eq!(k.max_health, 20, "the routine at 0x28d");
        }
        assert_eq!(r.knights_alive(), [true; 4]);
        r.rivals[1].lives = 0;
        assert_eq!(r.knights_alive(), [true, true, false, true], "+0x31 > 0");
    }

    /// `WhoLived+57` (0xaf7): the first field of `TakeMagicTABLE` the loser
    /// has, one of it, and the bit fields whole.
    #[test]
    fn a_winner_takes_one_thing_in_the_tables_order() {
        let mut w = record(0, LONG_SWORD, PADDED_ARMOUR);
        let mut l = record(40, LONG_SWORD, PADDED_ARMOUR);
        l.hoard.potions = 2;
        l.hoard.rings = 1;
        assert_eq!(
            take_from_the_fallen(&mut w, &mut l),
            Loot::Field(6),
            "rings before potions"
        );
        assert_eq!((w.hoard.rings, l.hoard.rings), (1, 0));
        assert_eq!(l.hoard.potions, 2, "one thing, and only one");
        assert_eq!(
            l.gold, 40,
            "a knight takes no gold: 0xafa is for the dragon"
        );
        // The keys go whole.
        l.hoard.keys = 0b0101;
        w.hoard.keys = 0b1000;
        assert_eq!(take_from_the_fallen(&mut w, &mut l), Loot::Field(0x14));
        assert_eq!((w.hoard.keys, l.hoard.keys), (0b1101, 0), "TakingMoon");
        // The last sword of sharpness puts a long sword in his hand.
        l.hoard.potions = 0;
        l.hoard.magic_sword = true;
        l.weapon = SWORD_OF_SHARPNESS;
        assert_eq!(take_from_the_fallen(&mut w, &mut l), Loot::Field(4));
        assert!(w.hoard.magic_sword && !l.hoard.magic_sword);
        assert_eq!(l.weapon, LONG_SWORD, "00b74");
    }

    /// `TakeArmour` (0xb8e) when the loser has nothing magic: his suit if it
    /// is the better one, and the health moved is the winner's old suit's.
    /// When it is not, `TakeGold` is fallen into.
    #[test]
    fn with_nothing_magic_the_suit_changes_hands_or_the_purse_is_halved() {
        let mut w = record(0, LONG_SWORD, CHAIN_MAIL);
        let mut l = record(41, LONG_SWORD, BATTLE_ARMOUR);
        assert_eq!(
            take_from_the_fallen(&mut w, &mut l),
            Loot::Armour(BATTLE_ARMOUR)
        );
        assert_eq!(
            (w.armour, l.armour),
            (BATTLE_ARMOUR, PADDED_ARMOUR),
            "00bb3"
        );
        assert_eq!(
            (w.health, l.health),
            (30, 10),
            "00b9e: chain mail's ten, not battle's thirty"
        );
        assert_eq!(l.gold, 41, "and the purse is left");
        let mut w = record(0, LONG_SWORD, PLATE_ARMOUR);
        let mut l = record(41, LONG_SWORD, CHAIN_MAIL);
        assert_eq!(
            take_from_the_fallen(&mut w, &mut l),
            Loot::Gold(20),
            "00b94: jge TakeGold"
        );
        assert_eq!((w.gold, l.gold), (20, 20), "00bb9: halved, rounded down");
    }

    /// `TakeALL` (0xbc5) off a dead man, and `0xafa`: the dragon takes gold
    /// as well.
    #[test]
    fn a_dead_loser_keeps_nothing_and_the_dragon_takes_gold_too() {
        let mut w = record(0, LONG_SWORD, PADDED_ARMOUR);
        let mut l = record(30, LONG_SWORD, PADDED_ARMOUR);
        l.lives = 0;
        l.hoard.potions = 2;
        l.hoard.scrolls = [1, 1, 1, 1, 1];
        l.hoard.moonstones = 2;
        l.hoard.magic_sword = true;
        assert_eq!(take_from_the_fallen(&mut w, &mut l), Loot::Everything);
        assert_eq!(w.hoard.potions, 2);
        assert_eq!(w.hoard.scrolls, [1, 1, 1, 1, 1]);
        assert_eq!(w.hoard.moonstones, 2);
        assert!(w.hoard.magic_sword);
        assert_eq!(l.hoard, Hoard::default());
        assert_eq!(l.weapon, SWORD_OF_SHARPNESS, "00c4b: sic, the grave's hand");
        assert_eq!(l.gold, 30, "a knight takes no gold");
        let mut d = record(0, 0, 0);
        d.kind = 0xa;
        let mut l = record(31, LONG_SWORD, PADDED_ARMOUR);
        l.hoard.gems = 1;
        assert_eq!(take_from_the_fallen(&mut d, &mut l), Loot::Field(2));
        assert_eq!(
            (d.gold, l.gold),
            (15, 15),
            "00afa: the arena dragon halves the purse first"
        );
    }

    /// `KnightSupplies` (0xac81), rung by rung.
    #[test]
    fn what_a_computer_knight_shops_for_follows_the_purse() {
        let items = goods();
        let mut r = run();
        r.which = 1;
        let want =
            |r: &mut Run, gold: u32, lives: i32, armour: &str, weapon: &str, daggers: u32| {
                let k = &mut r.rivals[0];
                k.gold = gold;
                k.lives = lives;
                k.knight.armour = armour.into();
                k.knight.weapon = weapon.into();
                k.knight.daggers = daggers;
                r.knight_supplies(1);
                r.turn.supply
            };
        let none = Supply::default();
        assert_eq!(
            want(&mut r, 10, 5, "padded_armour", "long_sword", 10),
            none,
            "0aca3"
        );
        assert_eq!(
            want(&mut r, 16, 2, "padded_armour", "long_sword", 10),
            Supply {
                price: 0xf,
                index: Some(0x31),
                goods: 0
            },
            "0acb6: the healer"
        );
        assert_eq!(
            want(&mut r, 75, 5, "plate_armour", "long_sword", 10),
            Supply {
                price: 0x4b,
                index: Some(0x42),
                goods: BATTLE_ARMOUR
            }
        );
        assert_eq!(
            want(&mut r, 50, 5, "chain_mail", "long_sword", 10),
            Supply {
                price: 0x32,
                index: Some(0x42),
                goods: PLATE_ARMOUR
            }
        );
        assert_eq!(
            want(&mut r, 30, 5, "padded_armour", "long_sword", 10),
            Supply {
                price: 0x1e,
                index: Some(0x42),
                goods: CHAIN_MAIL
            }
        );
        assert_eq!(
            want(&mut r, 30, 5, "chain_mail", "long_sword", 10),
            Supply {
                price: 0x19,
                index: Some(0x40),
                goods: CLAYMORE
            },
            "0ad2c: mail already, so a claymore"
        );
        assert_eq!(
            want(&mut r, 30, 5, "chain_mail", "sword_of_sharpness", 10),
            Supply {
                price: 0x19,
                index: Some(0x40),
                goods: CLAYMORE
            },
            "0ad26: je on 0x18 alone, so over the sword of sharpness too"
        );
        assert_eq!(
            want(&mut r, 12, 5, "padded_armour", "long_sword", 10),
            Supply {
                price: 0xa,
                index: Some(0x40),
                goods: BROAD_SWORD
            },
            "w1$"
        );
        assert_eq!(
            want(&mut r, 12, 5, "padded_armour", "broad_sword", 4),
            Supply {
                price: 2,
                index: Some(0x34),
                goods: 0
            },
            "w2$: daggers at five or under"
        );
        assert_eq!(
            want(&mut r, 12, 5, "padded_armour", "broad_sword", 6),
            none,
            "w3$"
        );
        // KnightInTown: the daggers loop, two gold each up to ten.
        r.rivals[0].gold = 12;
        r.rivals[0].knight.daggers = 4;
        r.knight_supplies(1);
        r.knight_in_town(1, &items);
        assert_eq!(
            (r.rivals[0].knight.daggers, r.rivals[0].gold),
            (10, 0),
            "0ae07..0ae1a"
        );
        // And the armour comes with its health.
        r.rivals[0].gold = 80;
        r.knight_supplies(1);
        r.knight_in_town(1, &items);
        assert_eq!(r.rivals[0].knight.armour, "battle_armour");
        assert_eq!(
            r.rivals[0].max_health, 50,
            "MerchantArmour: 0x28d run again"
        );
        assert_eq!(r.rivals[0].gold, 5);
    }

    /// `KnightGoesToTown` (0xad78): the nearer city by grid cell.
    #[test]
    fn a_shopping_knight_heads_for_the_nearer_city() {
        let mut r = run();
        r.which = 1;
        r.rivals[0].gold = 40;
        r.rivals[0].x = 20;
        r.rivals[0].y = 20;
        r.rivals[0].calc_grid();
        r.knight_supplies(1);
        r.knight_goes_to_town(1);
        assert_eq!(r.turn.town, Some((0x5e, 0x2f)), "Highwood");
        r.rivals[0].x = 280;
        r.rivals[0].y = 160;
        r.rivals[0].calc_grid();
        r.knight_goes_to_town(1);
        assert_eq!(r.turn.town, Some((0x129, 0x9d)), "Waterdeep");
        r.rivals[0].gold = 5;
        r.knight_supplies(1);
        r.knight_goes_to_town(1);
        assert_eq!(r.turn.town, None, "0ad78: nothing to buy, nowhere to go");
    }

    /// `TrackRoute` and `TrackStart` (0xaa31, 0xaa70): a line to the target,
    /// the long axis every frame and the short one on the error term.
    #[test]
    fn the_route_is_a_line_walked_a_pixel_a_frame() {
        let mut r = run();
        r.which = 1;
        r.rivals[0].x = 100;
        r.rivals[0].y = 100;
        r.rivals[0].target = Some(0);
        let at = r.positions((110, 105));
        let lairs = vec![None; 24];
        let mut steps = Vec::new();
        for _ in 0..10 {
            let joys = r.track_lair(1, at, &lairs);
            steps.push(joys);
            r.follow(1, joys);
        }
        assert_eq!(r.turn.track.x_abs, 10);
        assert_eq!(r.turn.track.y_abs, 5);
        assert!(!r.turn.track.y_major, "0aa50: x is the long axis");
        assert!(steps.iter().all(|s| s & 1 != 0), "right every frame");
        // The error term starts at x_abs, so the first step down waits two
        // frames: `step = 10; 10 - 5 = 5; 5 - 5 = 0; 0 - 5 < 0`. Four of the
        // ten frames step down, not five.
        assert_eq!(
            steps.iter().filter(|s| *s & 4 != 0).count(),
            4,
            "0aa7e..0aa88"
        );
        assert_eq!(
            (r.rivals[0].x, r.rivals[0].y),
            (110, 104),
            "ten along, four down"
        );
        // Past the far end it keeps going: the six words are written once.
        let joys = r.track_lair(1, at, &lairs);
        r.follow(1, joys);
        assert_eq!(r.rivals[0].x, 111);
    }

    /// The routine at 0xa75c: the distance table, the sort with its slip at
    /// 0xa7d1, and the roll among places two to four.
    #[test]
    fn the_closest_lair_is_picked_off_a_sort_with_a_slip_in_it() {
        let mut r = run();
        r.which = 1;
        // Lairs in a row along y = 100, one every twenty pixels from x 0.
        let lairs: Vec<Option<(i32, i32)>> = (0..24).map(|n| Some((n * 20, 100))).collect();
        r.find_close_lair((0, 100), &lairs);
        assert!(r.turn.lair_flag);
        let picked = r.turn.close_lair.expect("a lair");
        // Whatever the slip does to the middle of the table, the roll lands
        // on a real record and every record pointer is a real lair.
        assert!(picked < 24);
        assert!(!r.turn.no_lairs);
        // Done once a turn: a second call changes nothing.
        let again = r.turn.close_lair;
        r.find_close_lair((300, 100), &lairs);
        assert_eq!(r.turn.close_lair, again, "0a75c: LairFLAG");
        // Every lair off the map: `NoLairsFLAG`.
        r.turn = Turn::default();
        let gone: Vec<Option<(i32, i32)>> = vec![Some((-1, -1)); 24];
        r.find_close_lair((0, 100), &gone);
        assert!(r.turn.no_lairs, "0a808");
    }

    /// The sort's slip, on its own. Three lairs on the map, two hundred,
    /// ten and fifty away, and twenty one off it. A sort would put ten,
    /// fifty and two hundred at the front; the code puts lair one's record
    /// in the first two places, lair zero in none, and lair four, which is
    /// off the map, in the fourth with a distance of fifty six, which is
    /// lair three's record pointer.
    #[test]
    fn the_range_table_sort_writes_a_record_over_a_distance() {
        let mut t: Vec<(u16, u16)> = (0..24).map(|n| (0xffff, 2 + 0x12 * n)).collect();
        t[0].0 = 200;
        t[1].0 = 10;
        t[2].0 = 50;
        range_sort(&mut t);
        assert_eq!(&t[..4], &[(2, 20), (20, 20), (50, 38), (56, 74)], "0a7d1");
        // So the roll at 0xa7e0, which takes the second to fourth, can name
        // lair one, lair two or lair four, and lair four is off the map.
        let picks: Vec<usize> = (1..4).map(|i| ((t[i].1 - 2) / 0x12) as usize).collect();
        assert_eq!(picks, [1, 2, 4]);
    }

    /// `FindKnight` (0xa80f) to `KnightMagicLoop`: a person is the only
    /// target, and with no lair left to go to he is taken without a roll.
    #[test]
    fn a_computer_knight_only_ever_goes_after_a_person() {
        let mut r = run();
        r.which = 1;
        r.turn.no_lairs = true;
        let at = r.positions((50, 50));
        r.find_knight(1, at);
        assert_eq!(r.rivals[0].target, Some(0), "0a8ba: NoLairsFLAG");
        // Looked once a turn.
        r.rivals[0].target = None;
        r.find_knight(1, at);
        assert_eq!(r.rivals[0].target, None, "0a80f: LookFLAG");
        // With lairs to go to it is a roll, 26 in 128, twice over when he
        // carries a key.
        let mut hits = 0;
        for _ in 0..512 {
            r.turn = Turn::default();
            r.rivals[0].target = None;
            r.find_knight(1, at);
            if r.rivals[0].target.is_some() {
                hits += 1;
            }
        }
        assert!((60..150).contains(&hits), "26 of 128, got {hits} of 512");
        r.kit.take("key.forest", 1);
        let mut with_key = 0;
        for _ in 0..512 {
            r.turn = Turn::default();
            r.rivals[0].target = None;
            r.find_knight(1, at);
            if r.rivals[0].target.is_some() {
                with_key += 1;
            }
        }
        assert!(
            with_key > hits,
            "BKDecideCombat: a key is worth a second roll"
        );
    }

    /// `NextWHICH` (0xa434): the four seats in order, the day turned when
    /// they wrap, toads and dead computer knights passed over, and the run
    /// over when the person is dead.
    #[test]
    fn the_turns_go_round_the_four_seats_and_the_day_turns_on_the_wrap() {
        let mut r = run();
        assert_eq!(r.which, 0);
        let day = r.day;
        let t = r.next_which().unwrap();
        assert_eq!((t.which, t.day_turned), (1, false));
        assert_eq!(r.next_which().unwrap().which, 2);
        assert_eq!(r.next_which().unwrap().which, 3);
        let t = r.next_which().unwrap();
        assert_eq!((t.which, t.day_turned), (0, true), "0a44e");
        assert_eq!(r.day, day + 1);
        // A dead computer knight is passed over, and so is a toad.
        r.rivals[0].lives = 0;
        r.rivals[1].toad = 1;
        assert_eq!(r.next_which().unwrap().which, 3, "0a48d, 0a49c");
        // The target and the turn's words are cleared on the way.
        r.rivals[2].target = Some(0);
        r.turn.lair_flag = true;
        r.which = 2;
        r.next_which();
        assert_eq!(r.rivals[2].target, None, "0a46a");
        assert!(!r.turn.lair_flag, "0a481");
        // A dead person ends it.
        r.which = 3;
        r.lives = 0;
        r.over = true;
        assert_eq!(r.next_which(), None, "0a4af: jmp 0x617");
    }

    /// `KnightHeal` (0xabc6) and the potion at 0xcad0.
    #[test]
    fn a_hurt_computer_knight_opens_a_potion() {
        let mut r = run();
        r.which = 1;
        r.rivals[0].hoard.potions = 2;
        r.rivals[0].health = 5;
        r.knight_heal(1);
        assert_eq!((r.rivals[0].health, r.rivals[0].hoard.potions), (20, 1));
        r.knight_heal(1);
        assert_eq!(
            r.rivals[0].hoard.potions, 1,
            "0abd6: whole and five lives: nothing"
        );
        r.rivals[0].lives = 4;
        r.knight_heal(1);
        assert_eq!(
            (r.rivals[0].lives, r.rivals[0].hoard.potions),
            (5, 0),
            "0cadd: a life point"
        );
    }

    /// `KnightXP` (0xac61): a point bought the frame the experience covers it.
    #[test]
    fn a_computer_knight_spends_experience_as_soon_as_he_has_it() {
        let mut r = run();
        r.which = 1;
        r.rivals[0].experience = 3;
        r.knight_xp(1);
        let k = &r.rivals[0].knight;
        assert_eq!(k.strength + k.constitution + k.endurance, 4, "0ac78");
        assert_eq!(r.rivals[0].experience, 0, "0ac7d");
    }

    /// `Combat+102` (0x3b7): a toad or a grave is a walkover, two computer
    /// knights are nothing, and otherwise the fight.
    #[test]
    fn a_challenge_is_decided_before_a_blow() {
        let items = goods();
        let mut r = run();
        assert_eq!(r.challenge(1, 0), Challenged::Fight { cursed: false });
        assert_eq!(r.challenge(1, 2), Challenged::Nothing, "003f0");
        r.rivals[1].lives = 0;
        assert_eq!(r.challenge(0, 2), Challenged::Walkover, "003d5: a grave");
        r.turned_to_toad();
        assert_eq!(r.challenge(1, 0), Challenged::Walkover, "003cc: a toad");
        // The walkover: the challenger takes.
        r.rivals[1].hoard.gems = 1;
        r.rivals[1].gold = 20;
        assert_eq!(r.walkover(0, 2, &items), Settled::PlayerWon { loser: 2 });
        r.rivals[0].hoard.potions = 0;
        r.kit.take("potion", 1);
        r.toad = 0;
        match r.walkover(1, 0, &items) {
            Settled::RivalWon { winner: 1, loot } => assert_eq!(loot, Loot::Field(0)),
            other => panic!("{other:?}"),
        }
        assert_eq!(r.rivals[0].hoard.potions, 1, "0xaf7: the potion is his now");
        assert_eq!(r.kit.count("potion"), 0);
    }

    /// `WhoLived` and `Knight1Won`: whoever stood wins, a computer knight
    /// takes through `BKwon`, and both down is both a life point poorer.
    #[test]
    fn the_fight_settles_on_who_stood() {
        let items = goods();
        let mut r = run();
        r.kit.take("gem_of_seeing", 1);
        let lives = (r.lives, r.rivals[0].lives);
        match r.knight_fight_over(1, 0, 12, 0, &items) {
            Settled::RivalWon { winner: 1, loot } => assert_eq!(loot, Loot::Field(2)),
            other => panic!("{other:?}"),
        }
        assert_eq!(r.lives, lives.0 - 1, "WhoLived: a life point");
        assert_eq!(r.health, r.max_health, "and whole again");
        assert_eq!(r.rivals[0].experience, 1, "BKwon: 0049f");
        assert_eq!(r.rivals[0].hoard.gems, 1);
        assert_eq!(r.rivals[0].health, 12);
        // The player wins: the trade page, and nothing taken yet.
        assert_eq!(
            r.knight_fight_over(0, 1, 5, 0, &items),
            Settled::PlayerWon { loser: 1 }
        );
        assert_eq!(r.rivals[0].lives, lives.1 - 1);
        assert_eq!(
            r.rivals[0].hoard.gems, 1,
            "0046b: the panel takes, not this"
        );
        assert_eq!(r.knight_fight_over(0, 1, 0, 0, &items), Settled::BothDied);
    }

    /// The exact chain `henge-desktop`'s own `knight_fight` leans on:
    /// `Run::challenge` into `Run::knight_fight_over`, rather than either
    /// tested alone. Covers both directions and the scroll of protection's
    /// backfire, which is the one case `knight_fight_over` has to see the
    /// curse `challenge` already set, through `Run::challenged`.
    #[test]
    fn a_challenge_that_becomes_a_fight_settles_through_knight_fight_over() {
        let items = goods();
        let mut r = run();
        // A rival challenges the player and loses: `attacker` a computer
        // knight, `defender` 0.
        assert_eq!(r.challenge(1, 0), Challenged::Fight { cursed: false });
        match r.knight_fight_over(1, 0, 0, 12, &items) {
            Settled::PlayerWon { loser: 1 } => {}
            other => panic!("{other:?}"),
        }
        // The player challenges a rival and loses: `attacker` 0.
        assert_eq!(r.challenge(0, 2), Challenged::Fight { cursed: false });
        match r.knight_fight_over(0, 2, 0, 5, &items) {
            Settled::RivalWon { winner: 2, .. } => {}
            other => panic!("{other:?}"),
        }
        // A scroll of protection sometimes backfires rather than turning
        // the challenger away: the fight goes ahead and the caster is
        // cursed for it, which `knight_fight_over`'s own call into
        // `Run::finished_fight_worth` clears once the bout settles.
        assert_eq!(
            r.kit.take("scroll_of_protection", 512),
            512,
            "room to keep casting"
        );
        let mut backfired = false;
        for _ in 0..512 {
            assert_eq!(
                r.cast("scroll_of_protection", &items),
                crate::run::Cast::Warded
            );
            match r.challenge(1, 0) {
                Challenged::Fight { cursed: true } => {
                    backfired = true;
                    assert!(r.is_cursed(), "challenge already set it");
                    r.knight_fight_over(1, 0, 5, 5, &items);
                    assert!(!r.is_cursed(), "a curse is one bout");
                    break;
                }
                Challenged::Averted => assert!(!r.is_cursed()),
                other => panic!("{other:?}"),
            }
        }
        assert!(
            backfired,
            "11 of 128 over 512 casts should have backfired at least once"
        );
    }

    /// The dragon on a computer knight, 0xcf3: a life point and one thing
    /// into the dragon's hoard, no fight, and the day spent.
    #[test]
    fn the_dragon_takes_a_life_point_and_a_thing_off_a_computer_knight() {
        let mut r = run();
        r.which = 1;
        r.rivals[0].hoard.talismans = 1;
        r.rivals[0].gold = 30;
        let f = r.dragon_on_rival(1, 96, &goods());
        assert_eq!(f, Frame::Dragon(Loot::Field(8)));
        assert_eq!(r.rivals[0].lives, 4, "00d03");
        assert_eq!(r.dragon_hoard.talismans, 1, "into [0x6e26+0x44]");
        assert_eq!(r.rivals[0].gold, 30, "kind 0x14: no gold");
        assert_eq!(r.turn.steps, 96, "EncounterAllDone");
    }

    /// A whole turn on open ground: the day's frames walked toward the
    /// target, and the turn over when they are spent.
    #[test]
    fn a_turn_is_the_days_frames_walked_at_a_pixel_each() {
        let items = goods();
        let mut r = run();
        let land = Landscape::open();
        let places = Places::new();
        let mut going = 0;
        r.which = 1;
        r.turn.no_lairs = true;
        r.turn.lair_flag = true;
        let start = (r.rivals[0].x, r.rivals[0].y);
        let board = Board {
            land: &land,
            places: &places,
            items: &items,
            player_at: (10, 10),
        };
        let mut frames = 0;
        loop {
            frames += 1;
            match r.rival_frame(&board, &mut going) {
                Frame::Walked => {}
                Frame::TurnOver => break,
                other => panic!("{other:?} on frame {frames}"),
            }
            assert!(frames < 1000);
        }
        assert_eq!(frames, 96, "0a4f6: six times sixteen, the last not walked");
        assert_eq!(r.rivals[0].target, Some(0), "after the person");
        let end = (r.rivals[0].x, r.rivals[0].y);
        assert_eq!(end.0, start.0 - 95, "x is the long axis: a pixel a frame");
        assert!(
            end.1 < start.1 && end.1 >= start.1 - 95,
            "and y on the error term"
        );
    }

    /// `BKCollision+85` (0xab06): standing on the knight he is after is the
    /// challenge, and the day is spent whatever comes of it.
    #[test]
    fn standing_on_the_knight_he_is_after_is_the_challenge() {
        let items = goods();
        let mut r = run();
        let land = Landscape::open();
        let places = Places::new();
        let mut going = 0;
        r.which = 1;
        r.turn.no_lairs = true;
        r.turn.lair_flag = true;
        r.rivals[0].x = 100;
        r.rivals[0].y = 100;
        let board = Board {
            land: &land,
            places: &places,
            items: &items,
            player_at: (104, 96),
        };
        // FindKnight names him and BKCollision finds him in the same frame:
        // the stack it reads was built by the walker at the turn's start.
        assert_eq!(
            r.rival_frame(&board, &mut going),
            Frame::Challenge { target: 0 }
        );
        assert_eq!(r.rival_frame(&board, &mut going), Frame::TurnOver, "0ab0f");
    }

    #[test]
    fn a_run_with_rivals_survives_serialization() {
        let mut r = run();
        r.next_which();
        r.rivals[0].target = Some(0);
        r.turn.close_lair = Some(3);
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<Run>(&json).unwrap(), r);
    }

    /// The trade page's gadgets: `TKGP`, `TKAR`, `TKWP`, `TakeSword` and
    /// `HGTakeMagic`, the winner taking off the loser's record.
    #[test]
    fn the_trade_page_takes_one_gadget_at_a_time() {
        let items = goods();
        let mut r = run();
        let k = &mut r.rivals[0];
        k.gold = 200;
        k.knight.armour = "chain_mail".into();
        k.knight.weapon = "claymore".into();
        k.hoard.rings = 1;
        k.hoard.keys = 0b0110;
        k.hoard.magic_sword = true;
        k.refresh(&items);
        k.health = k.max_health;
        assert!(r.trade_take(1, 0x32, &items), "TKGP");
        assert_eq!(
            (r.gold, r.rivals[0].gold),
            (150, 60),
            "0cd1a: the purse stops at 0x96"
        );
        assert!(r.trade_take(1, 0x42, &items), "TKAR");
        assert_eq!(r.knight.armour, "chain_mail");
        assert_eq!(r.rivals[0].knight.armour, "padded_armour", "0ccb0");
        assert_eq!(
            r.rivals[0].health, 40,
            "0cc9c: mail's ten off his fifty, under the ceiling 0x28d makes"
        );
        assert!(!r.trade_take(1, 0x42, &items), "0cc85: padded is nothing");
        assert!(r.trade_take(1, 0x40, &items), "TKWP");
        assert_eq!(r.knight.weapon, "claymore");
        assert_eq!(r.rivals[0].knight.weapon, "long_sword", "0ccc8");
        assert!(
            !r.trade_take(1, 0x40, &items),
            "0ccbb: a long sword is nothing"
        );
        assert!(r.trade_take(1, 0x14, &items), "HGTakeMoonstone");
        assert_eq!(r.rivals[0].hoard.keys, 0, "0cbe6: the field whole");
        assert_eq!(r.kit.count("key.waste") + r.kit.count("key.swamp"), 2);
        let before = r.health;
        assert!(r.trade_take(1, 6, &items), "a ring");
        assert_eq!(r.kit.count("ring_of_protection"), 1);
        assert_eq!(r.health, before + 0x14, "0cbdb: twenty on the spot");
        assert!(!r.trade_take(1, 6, &items), "0cbce: none left");
        assert!(r.trade_take(1, 4, &items), "TakeSword");
        assert_eq!(r.knight.weapon, "sword_of_sharpness", "0cce2");
        assert!(!r.rivals[0].hoard.magic_sword);
    }

    /// `BKwon` (0x49f) and `BKAddstuff` (0x4b0): the point a won duel pays a
    /// computer knight is spent where it is earned, at `XPlevels[0]` three,
    /// and the ceiling is not asked about. This replaces a test that paid
    /// the point to the player, whom `Knight1Won+6` (0x46b) sends to the
    /// trade page with nothing.
    #[test]
    fn a_computer_knight_who_wins_a_duel_is_paid_a_point_and_spends_it_on_the_spot() {
        let items = goods();
        let mut r = run();
        // 004b8: three wins before the first one lands.
        assert_eq!(r.xp_per_level, 3);
        r.bk_won(1, 0, &items);
        r.bk_won(1, 0, &items);
        assert_eq!(r.rivals[0].experience, 2);
        let k = &r.rivals[0].knight;
        let before = [k.strength, k.constitution, k.endurance];
        r.bk_won(1, 0, &items);
        assert_eq!(
            r.rivals[0].experience, 0,
            "004d9: the cost comes straight off"
        );
        let k = &r.rivals[0].knight;
        let after = [k.strength, k.constitution, k.endurance];
        let raised: i32 = after.iter().zip(before).map(|(a, b)| a - b).sum();
        assert_eq!(raised, 1, "004ca: one point, into one of the three");
        // 004ca has no ceiling: a knight who keeps winning goes past five.
        let k = &mut r.rivals[0].knight;
        k.strength = 5;
        k.constitution = 5;
        k.endurance = 5;
        assert!(k.maxed());
        r.rivals[0].experience = 0;
        for _ in 0..30 {
            r.bk_won(1, 0, &items);
        }
        let k = &r.rivals[0].knight;
        assert_eq!(
            k.strength + k.constitution + k.endurance,
            25,
            "thirty wins at three a point"
        );
        assert!(
            [k.strength, k.constitution, k.endurance]
                .iter()
                .any(|a| *a > crate::knight::MAX_ABILITY),
            "0x4ca asks no ceiling"
        );
        assert_eq!(
            r.experience, 0,
            "and the player was paid nothing for losing"
        );
    }
}
