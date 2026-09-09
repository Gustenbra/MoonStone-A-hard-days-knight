//! What the five gadgets on a town's parchment open onto.
//!
//! `MOON:HWLOOP` (image 0xe35) and `WDLOOP` (0xd7a) are one ladder each on
//! `es:[si+0xe]`, the id of the gadget fire was over, and every rung is three
//! or four instructions:
//!
//! ```text
//! MERC  0e9c  AddClickSound; fade out; mov ax, 5; call 0bdd3; jmp HWINIT
//! TAV   0e90  AddClickSound; call 0b007;  fade out;           jmp HWINIT
//! HEAL  0eba  AddClickSound; fade out; call 0ba66; fade out;  jmp HWINIT
//! HTEM  0eab  AddClickSound; fade out; mov ax, 6; call 0bdd3; jmp HWINIT
//! MYST  0dd5  AddClickSound; call 0b935;  fade out;           jmp WDINIT
//! CEXIT 0e0b  AddClickSound; call 0a4be;             jmp EncounterAllDone
//! ```
//!
//! `0xbdd3` is the status panel's entry (it writes `ax` into `StatTYPE`, puts
//! the pointer at (0xa0, 0x64), runs `SetUpStatus`, `ReDisplay` and `StatLOOP`),
//! so **the merchant and the high temple are two pages of the status panel**,
//! types 5 and 6, and have no picture of their own. The tavern is `_TAVERN`'s
//! routine at 0xb007, the healer `_WIZARD`'s at 0xba66 and the mystic its at
//! 0xb935, and those three are loops of their own over `TAV.PIV`, `HEA.PIV`
//! and `MYS.PIV`. Every rung comes back to `HWINIT`, which reloads the town
//! and puts the pointer back at (0x122, 0x64) (`WDINIT`: (0x1e, 0x64)).
//!
//! None of the five has a menu. The merchant sells through `BuyGoods`, one
//! gadget per thing on the stall; the temple through `TTemple`, one gadget per
//! thing on either side of the screen; the tavern through six gadgets on the
//! painted parchment; and the healer and the mystic through the donation bowl
//! after a greeting and fire. This module is those routines, transcribed, and
//! nothing that stood in for them: the lists of lines with a highlight that
//! used to be each of these screens are gone.

use crate::dice::Table;
use crate::item::Items;
use crate::run::Run;
use crate::service::{Healing, Reading, Throw, MAGIC_PRICES, ODDS, PURSE_CEILING};
use crate::status::{DonateOp, Donation, Hoard};
use crate::taskvm::{Frame, ScriptSet};
use serde::{Deserialize, Serialize};

/// The five rungs of `HWLOOP`'s ladder, by the gadget id each tests.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum Door {
    /// `MERC`, `WMERC`: `mov ax, 5` and the panel.
    Merchant = 1,
    /// `TAV`, `WTAV`: `_TAVERN` at 0xb007.
    Tavern = 2,
    /// `HEAL`, `WHEA`: `_WIZARD` at 0xba66.
    Healer = 3,
    /// `HTEM`: `mov ax, 6` and the panel. Highwood's fourth gadget.
    Temple = 4,
    /// `MYST`: `_WIZARD` at 0xb935. Waterdeep's fourth gadget.
    Mystic = 5,
}

/// One word written on a screen: the registers `TextP` (0x7a70) is called
/// with, or one record of a chain (0x7a86). `TextPTop` reads bit 0 of the
/// flags as centre between the borders and bit 2 as right against the right
/// border, and ignores `x` for both.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Word {
    pub text: String,
    pub x: i32,
    pub y: i32,
    pub centred: bool,
}

impl Word {
    pub fn at(text: impl Into<String>, x: i32, y: i32) -> Word {
        Word {
            text: text.into(),
            x,
            y,
            centred: false,
        }
    }

    pub fn centred(text: impl Into<String>, y: i32) -> Word {
        Word {
            text: text.into(),
            x: 0,
            y,
            centred: true,
        }
    }
}

// ------------------------------------------------------------- the merchant

/// The cels `+0x40` holds, 0x16 to 0x19, by the pack id of each sword, and
/// the cels `+0x42` holds, 0x1b to 0x1e, by the pack id of each suit.
pub const SWORDS: [&str; 4] = [
    "long_sword",
    "broad_sword",
    "claymore",
    "sword_of_sharpness",
];
pub const ARMOURS: [&str; 4] = [
    "padded_armour",
    "chain_mail",
    "plate_armour",
    "battle_armour",
];

/// `+0x40` as the record holds it: the cel, 0x16 for a long sword. A weapon
/// the four do not name is read as the long sword, which is what `BuyWeapon`'s
/// `cmp word ptr [si+0x40], 0x17; jge` would let past.
pub fn weapon_cel(id: &str) -> u16 {
    0x16 + SWORDS.iter().position(|s| *s == id).unwrap_or(0) as u16
}

/// `+0x42` as the record holds it: the cel, 0x1b for padded armour.
pub fn armour_cel(id: &str) -> u16 {
    0x1b + ARMOURS.iter().position(|s| *s == id).unwrap_or(0) as u16
}

impl Run {
    /// `_STATUS:BuyGoods` at 0xcd33, which `HotGadget` reaches for a gadget
    /// whose `STRP` low nibble is 0xa: the six things `DisplayMerchant` puts
    /// on the stall. `field` is `STPL` and `mask` the high nibble.
    ///
    /// ```text
    /// 0cd37  cmp bx, 0x42; je BuyArmour
    /// 0cd3c  cmp bx, 0x40; je BuyWeapon
    /// 0cd41  cmp bx, 0x34; jne ret;  jmp BuyDagger
    ///
    /// BuyArmour 0cd4a
    ///   test dx, 1:  cmp [si+0x32], 0x1e; jl ret; sub 0x1e; [si+0x42] = 0x1c;
    ///                add [si+0x38], 0xa;  call 0x28d; click; ReDisplay
    ///   test dx, 2:  0x32 for 0x1d and 0x14 health
    ///   test dx, 4:  0x4b for 0x1e and 0x1e health
    /// BuyWeapon 0cdb3
    ///   test dx, 1:  cmp [si+0x32], 0xa;  jl ret; cmp [si+0x40], 0x17; jge ret;
    ///                sub 0xa;  [si+0x40] = 0x17; click; ReDisplay
    ///   test dx, 2:  cmp [si+0x32], 0x19; jl ret; cmp [si+0x40], 0x18; jge ret;
    ///                sub 0x19; [si+0x40] = 0x18; click; ReDisplay
    /// BuyDagger 0cdf7
    ///   cmp [si+0x32], 2; jl ret; cmp byte [si+0x34], 0xa; jge ret;
    ///   sub 2; inc byte [si+0x34]; click; ReDisplay
    /// ```
    ///
    /// So a suit is bought whatever is on the back already, a sword only while
    /// the one in the hand is a lesser one, and a dagger only up to ten. Nothing
    /// bought here goes into a pack: the record's own field is written. Returns
    /// whether `AddClickSound` was reached, which is whether anything was
    /// bought.
    pub fn buy_goods(&mut self, field: u16, mask: u8, items: &Items) -> bool {
        match field {
            0x42 => {
                // `BuyArmour`: the three arms, in the order the routine tests.
                let (price, suit, health) = if mask & 1 != 0 {
                    (0x1e, 1, 0xa)
                } else if mask & 2 != 0 {
                    (0x32, 2, 0x14)
                } else if mask & 4 != 0 {
                    (0x4b, 3, 0x1e)
                } else {
                    return false;
                };
                if self.gold < price {
                    return false;
                }
                self.gold -= price;
                self.knight.armour = ARMOURS[suit].to_string();
                // `add word ptr [si+0x38], 0xa` and then the routine at 0x28d,
                // which writes the ceiling and pulls the health down to it.
                self.health += health;
                self.refresh(items);
                true
            }
            0x40 => {
                // `BuyWeapon`: `cmp word ptr [si+0x40], 0x17 / 0x18; jge ret`.
                let (price, sword) = if mask & 1 != 0 {
                    (0xa, 1)
                } else if mask & 2 != 0 {
                    (0x19, 2)
                } else {
                    return false;
                };
                if self.gold < price || weapon_cel(&self.knight.weapon) >= 0x16 + sword as u16 {
                    return false;
                }
                self.gold -= price;
                self.knight.weapon = SWORDS[sword].to_string();
                true
            }
            0x34 => {
                // `BuyDagger`.
                if self.gold < 2 || self.knight.daggers >= 0xa {
                    return false;
                }
                self.gold -= 2;
                self.knight.daggers += 1;
                true
            }
            _ => false,
        }
    }
}

// ---------------------------------------------------------------- the temple

/// The pointer's x `TTemple` divides the screen at: `cmp word ptr [PointerX],
/// 0xa0; jl SellToTemple` (0xce1f). Left of it is the knight's own arch and a
/// sale, right of it the temple's and a purchase.
pub const TEMPLE_MIDDLE: i32 = 0xa0;

/// The knight's magic record, as `TTemple` reads it through `StatMAGIC1`.
///
/// This engine keeps a knight's magic as items in a pack rather than as the
/// 24-byte record, so a field is a pack id (`+0x00` to `+0x12`) or, for the
/// keys and the moonstones, the item whose bit `dl` names.
fn item_of(field: u16, mask: u8) -> Option<&'static str> {
    match field {
        crate::lair::KEYS_FIELD => crate::moon::Key::ALL
            .iter()
            .find(|k| k.bit() == mask)
            .map(|k| k.item()),
        crate::lair::STONES_FIELD => crate::moon::Moonstone::ALL
            .iter()
            .find(|m| m.bit() == mask)
            .map(|m| m.item()),
        f => crate::service::magic_item(f as u8),
    }
}

/// `MagicPrices[bx]`, the word at `DS:0xedae + STPL`.
fn magic_price(field: u16) -> Option<u32> {
    MAGIC_PRICES
        .iter()
        .find(|(f, _)| u16::from(*f) == field)
        .map(|(_, p)| *p)
}

impl Run {
    /// `_STATUS:TTemple` at 0xce11, which is where `HotGadget` sends **every**
    /// magic gadget on a type 6 panel: `HGCastMagic` at 0xca83 and
    /// `HGTakeMagic` at 0xcbaa both `cmp word ptr [StatTYPE], 6; je TTemple`
    /// before they look at a permission bit.
    ///
    /// ```text
    /// 0ce11  si = StatMAGIC1              ; the knight's record
    /// 0ce15  di = 0xed96                  ; the temple's own, SaveTYPE+2
    /// 0ce18  bp = StatHAND1; cx = MagicPrices
    /// 0ce1f  cmp word ptr [PointerX], 0xa0; jl SellToTemple
    /// 0ce27  ax = MagicPrices[bx]
    /// 0ce2d  cmp ax, [bp+0x32]; jg ret    ; TBYR: too dear, and no click
    /// 0ce33  sub [bp+0x32], ax; AddClickSound
    /// 0ce3a  cmp bx, 0x16; je BuyMoonstone
    /// 0ce3f  cmp bx, 0x14; je BuyMoonstone
    /// 0ce44  dec byte [bx+di]; inc byte [bx+si]
    /// 0ce48  cmp bx, 6; jne TBY           ; a ring: twenty health at once
    /// 0ce4f  add word ptr [bp+0x38], 0x14; call 0x28d
    /// TBY    call ReDisplay; ret
    /// BuyMoonstone 0ce5a  or [bx+si], dl; xor [bx+di], dl; jmp TBY
    /// SellMoonstone 0ce60 or [bx+di], dl; xor [bx+si], dl; jmp GoldSell
    /// SellToTemple 0ce66
    /// 0ce66  cmp bx, 0x16; je SellMoonstone
    /// 0ce6b  cmp bx, 0x14; je SellMoonstone
    /// 0ce70  AddClickSound; dec byte [bx+si]; inc byte [bx+di]
    /// GoldSell 0ce77
    /// 0ce77  ax = MagicPrices[bx]; shr ax, 1; add [bp+0x32], ax
    /// 0ce83  cmp [bp+0x32], 0x96; jle; mov [bp+0x32], 0x96
    /// 0ce91  cmp bx, 4; jne; mov word ptr [bp+0x40], 0x16
    /// 0ce9c  call 0x28d; call ReDisplay; ret
    /// ```
    ///
    /// So the temple has a stock of its own, and it is what knights have sold
    /// it: `DS:0xed96` is twenty four zero bytes in the image and nothing but
    /// this routine and `ReDisplay`'s type 6 arm touches it. Returns whether
    /// anything changed hands.
    pub fn trade_at_temple(&mut self, field: u16, mask: u8, pointer_x: i32, items: &Items) -> bool {
        let Some(price) = magic_price(field) else {
            return false;
        };
        let Some(id) = item_of(field, mask) else {
            return false;
        };
        if pointer_x >= TEMPLE_MIDDLE {
            // The temple's arch: a purchase. The gadget is only there while
            // `DisplayMagic` had something to draw, which is a count above zero
            // or the bit set, so the stock is checked here as the drawing did.
            if !self.temple.has(field, mask) {
                return false;
            }
            // `cmp ax, [bp+0x32]; jg TBYR`.
            if price > self.gold {
                return false;
            }
            // **Ours:** a pack that can be full. The record's fields cannot
            // refuse; this engine's pack can, and it refuses before the coin
            // goes rather than after.
            if field != 4 && self.kit.room() == 0 {
                return false;
            }
            self.gold -= price;
            self.temple.give(field, mask);
            if field == 4 {
                // `[bx+si]` with bx 4 is the knight's own magic sword, which
                // the routine at 0x28d reads back into `+0x40` as 0x19.
                self.knight.weapon = SWORDS[3].to_string();
            } else {
                self.kit.take(id, 1);
            }
            if field == 6 {
                // `add word ptr [bp+0x38], 0x14; call 0x28d`.
                self.health += 0x14;
            }
            self.refresh(items);
            return true;
        }
        // The knight's arch: a sale. `SellToTemple`, then `GoldSell`.
        if field == 4 {
            if self.knight.weapon != SWORDS[3] {
                return false;
            }
            self.knight.weapon = SWORDS[0].to_string();
        } else if self.kit.lose(id, 1) == 0 {
            return false;
        }
        self.temple.receive(field, mask);
        // `shr ax, 1`, into a purse that stops at a hundred and fifty.
        self.gold = (self.gold + price / 2).min(PURSE_CEILING);
        self.refresh(items);
        true
    }
}

impl Hoard {
    /// Whether the record has the thing a gadget names: the count at the
    /// field, or the bit in one of the two bit fields.
    pub fn has(&self, field: u16, mask: u8) -> bool {
        match field {
            crate::lair::KEYS_FIELD => self.keys & mask != 0,
            crate::lair::STONES_FIELD => self.moonstones & mask != 0,
            4 => self.magic_sword,
            f => crate::lair::slot_of_field(f)
                .and_then(|s| self.count(s))
                .is_some_and(|n| n > 0),
        }
    }

    /// `dec byte [bx+di]`, or `xor [bx+di], dl` for a bit field.
    pub fn give(&mut self, field: u16, mask: u8) {
        match field {
            crate::lair::KEYS_FIELD => self.keys ^= mask,
            crate::lair::STONES_FIELD => self.moonstones ^= mask,
            4 => self.magic_sword = false,
            f => {
                if let Some(s) = crate::lair::slot_of_field(f) {
                    let n = self.count(s).unwrap_or(0);
                    self.set(s, n.saturating_sub(1));
                }
            }
        }
    }

    /// `inc byte [bx+di]`, or `or [bx+di], dl` for a bit field.
    pub fn receive(&mut self, field: u16, mask: u8) {
        match field {
            crate::lair::KEYS_FIELD => self.keys |= mask,
            crate::lair::STONES_FIELD => self.moonstones |= mask,
            4 => self.magic_sword = true,
            f => {
                if let Some(s) = crate::lair::slot_of_field(f) {
                    let n = self.count(s).unwrap_or(0);
                    self.set(s, n.saturating_add(1));
                }
            }
        }
    }
}

// ---------------------------------------------------------------- the tavern

/// The six gadgets the routine at 0xb007 adds after `CLEARGADGETS` at 0xb053,
/// as `(x, y, w, h, id, strp)`. Five stakes with `+0xe` 1 and `+0x10` the
/// stake, over the `1 gold` to `5 gold` `TAV.PIV` paints down its parchment,
/// and the exit with `+0xe` 2. All six have `+8` zero, so none says anything.
///
/// ```text
/// 0b059  [si+0xa] = 0x10c; [si+0xc] = 0x26; [si+4] = 0x2b; [si+6] = 0x18
/// 0b06d  [si+8] = 0; [si+0xe] = 1; [si+0x10] = 1; [si+0x12] = 0x32; ADDGADGET
/// 0b087  [si+0xc] = 0x42; [si+0x10] = 2; ADDGADGET
/// 0b097  [si+0xc] = 0x5e; [si+0x10] = 3; ADDGADGET
/// 0b0a7  [si+0xc] = 0x79; [si+0x10] = 4; ADDGADGET
/// 0b0b7  [si+0xc] = 0x93; [si+0x10] = 5; ADDGADGET
/// 0b0c7  [si+0xa] = 0x10c; [si+0xc] = 0xb5; [si+4] = 0x2d; [si+6] = 0x10;
///        [si+0xe] = 2; ADDGADGET
/// ```
///
/// They are added once, before `TavernOpenScene`, and never cleared while the
/// tavern is up: the throw's picture and the way back both keep them.
#[rustfmt::skip]
pub const TAVERN_GADGETS: [(i32, i32, i32, i32, usize, u16); 6] = [
    (0x10c, 0x26, 0x2b, 0x18, 1, 1),
    (0x10c, 0x42, 0x2b, 0x18, 1, 2),
    (0x10c, 0x5e, 0x2b, 0x18, 1, 3),
    (0x10c, 0x79, 0x2b, 0x18, 1, 4),
    (0x10c, 0x93, 0x2b, 0x18, 1, 5),
    (0x10c, 0xb5, 0x2d, 0x10, 2, 0),
];

/// The gadget id of a stake, and of the exit: `cmp word ptr es:[si+0xe], 1;
/// je SetBET` and `cmp ..., 2` at 0xb1d2 and 0xb1d9.
pub const STAKE_ID: usize = 1;
pub const TAVERN_EXIT_ID: usize = 2;

/// Where `TavernOpenScene` puts the pointer: `mov word ptr [PointerX], 0x118`
/// at 0xb0f5, and nothing written to its y.
pub const TAVERN_POINTER_X: i32 = 0x118;

/// Where `TavernLoop` writes the purse every frame: `GOLDASCII` at
/// (0x11a, 0xe) with `cx` 2, which `TextPTop` tests no bit of, so left
/// aligned (0xb158).
pub const TAVERN_GOLD_AT: (i32, i32) = (0x11a, 0xe);

/// Where `RollDice` blits the three faces of `DICE.CEL` over `DICE.PIV`:
/// `bx, cx` of (0x73, 0xf), (0x31, 0x26) and (0x4b, 0x58) at 0xb25c, 0xb26e
/// and 0xb280.
pub const DICE_AT: [(i32, i32); 3] = [(0x73, 0xf), (0x31, 0x26), (0x4b, 0x58)];

/// `WIN` (DS:0xcd7a) and `LOST` (DS:0xcd70): three records each, all at x
/// 0xae with flags 0, on rows 0x7e, 0x8c and 0x98. The second and third are
/// shared: `PLAYERPOT` with `GOLDTOTAL` written into its gap, and `CONT`.
pub const RESULT_X: i32 = 0xae;
pub const RESULT_ROWS: [i32; 3] = [0x7e, 0x8c, 0x98];

/// `DiceWait` at 0xb31c: flip, fade in, `mov ax, 0x14; call 0xafeb`, which
/// is twenty retraces, and then `WaitFIRE` at 0x8251.
pub const RESULT_WAIT: u32 = 0x14;

/// Which picture the tavern is on.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum TavernScreen {
    /// `TAV.PIV` (`Tav1`, loaded by 0xaff3 from `TavernOpenScene+22`), with
    /// the hand over the table and the six gadgets live.
    Table,
    /// `DICE.PIV` (`Tav2`, loaded by 0xaffd from `RollDice+22`), with the
    /// three faces and the three lines, waiting on fire.
    Dice,
}

/// The tavern, running: the routine at 0xb007 through `LeaveTavern`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Tavern {
    pub screen: TavernScreen,
    /// The hand, `DiceTHROW` and the one task, which is [`crate::dice`].
    pub table: Table,
    /// `BET`, DS:0xd123.
    pub bet: u32,
    /// The last throw, drawn on the dice picture: `DDICE` and what
    /// `DiceWinner` or the loss wrote.
    pub result: Option<Throw>,
    /// `DiceWait`'s twenty retraces, counted down before fire is read.
    pub wait: u32,
    /// `LeaveTavern` has run.
    pub left: bool,
}

impl Tavern {
    /// The routine at 0xb007: `cmp word ptr [si+0x32], 0; jg` and otherwise
    /// `ret`, before anything is loaded or drawn. An empty purse never sees
    /// the inside of the tavern.
    pub fn open(run: &Run) -> Option<Tavern> {
        if run.gold == 0 {
            return None;
        }
        Some(Tavern {
            screen: TavernScreen::Table,
            table: Table::new(),
            bet: 0,
            result: None,
            wait: 0,
            left: false,
        })
    }

    /// Fire over one of the six, which is `ThrowDice` at 0xb1c4 as the
    /// end-of-shake handler reaches it. `id` and `strp` are the gadget's
    /// `+0xe` and `+0x10`.
    ///
    /// ```text
    /// 0b1d2  cmp word ptr es:[si+0xe], 1; je SetBET
    /// 0b1d9  cmp word ptr es:[si+0xe], 2; jne BBB
    /// 0b1e0  mov word ptr [XFL], 1; jmp BBB
    /// SetBET 0b1e8
    /// 0b1e8  mov [BET], es:[si+0x10]
    /// 0b1f4  cmp ax, [si+0x32]; jg BBB       ; more than the purse: refused
    /// 0b1fc  sub [si+0x32], ax               ; paid now
    /// 0b1ff  mov word ptr [DiceTHROW], 1; [0x783a] = DD_ThrowDice
    /// ```
    pub fn press(&mut self, id: usize, strp: u16, run: &mut Run) {
        if self.screen != TavernScreen::Table || self.table.staked() {
            return;
        }
        match id {
            STAKE_ID => {
                let bet = u32::from(strp);
                if bet > run.gold {
                    return;
                }
                run.gold -= bet;
                self.bet = bet;
                self.table.stake_taken();
            }
            TAVERN_EXIT_ID => self.table.exit_taken(),
            _ => {}
        }
    }

    /// One pass of `TavernLoop` (0xb137) on the table, or of `DiceWait` on
    /// the dice. Returns the hand's frame on a tick it stepped.
    ///
    /// ```text
    /// 0b166  cmp word ptr [DiceTHROW], 2; jne; jmp DiceRND
    /// 0b176  cmp word ptr [XFL], 0; je TavernLoop
    /// LeaveTavern 0b17d
    /// DiceRND 0b21d  call RollDice; ... ; jmp TavernOpenScene
    /// ```
    pub fn tick(&mut self, set: &ScriptSet, run: &mut Run) -> Option<Frame> {
        match self.screen {
            TavernScreen::Table => {
                let frame = self.table.tick(set);
                if self.table.landed() {
                    // `DiceRND`: `RollDice`, the picture, the faces, the payout.
                    self.result = Some(run.roll_dice(self.bet));
                    self.screen = TavernScreen::Dice;
                    self.wait = RESULT_WAIT;
                } else if self.table.xfl {
                    self.left = true;
                }
                frame
            }
            TavernScreen::Dice => {
                self.wait = self.wait.saturating_sub(1);
                None
            }
        }
    }

    /// Fire on the dice picture, once `DiceWait`'s twenty retraces are up:
    /// `WaitFIRE` returns and `DiceRND+3` runs on into `TavernOpenScene`,
    /// which turns an empty purse out at the door (`cmp word ptr [si+0x32],
    /// 0; jle LeaveTavern`) and otherwise reloads the table, writes
    /// `DiceTHROW` 0 and starts the hand again.
    pub fn fire(&mut self, run: &Run) {
        if self.screen != TavernScreen::Dice || self.wait > 0 {
            return;
        }
        self.result = None;
        if run.gold == 0 {
            self.left = true;
            return;
        }
        self.screen = TavernScreen::Table;
        self.table = Table::new();
    }

    /// What `RollDice` and `DiceWinner` write on the plank: the chain `WIN`
    /// or `LOST`, whose first two strings have the number written into them.
    ///
    /// `BETWINNER` (DS:0xcdb7) is `You won ` and `BETWINPOT` is the byte
    /// after it; `BETLOSER` (0xcd98) is `You lost ` and `BETLOSTPOT` the byte
    /// after that; `PLAYERPOT` (0xcdd5) is `You now have ` and `GOLDTOTAL`
    /// follows it. The number writer at 0x7c24 puts the decimal at that byte
    /// and `GPTEXT` (0xb2cc) writes ` gp.` and a terminator after it, so the
    /// ` gold pieces.` each string carries beyond the gap is cut off and is
    /// never on the screen.
    pub fn result_words(&self, run: &Run) -> Vec<Word> {
        let Some(t) = self.result.as_ref() else {
            return Vec::new();
        };
        let first = if t.winner() {
            format!("You won {} gp.", t.won)
        } else {
            format!("You lost {} gp.", t.stake)
        };
        vec![
            Word::at(first, RESULT_X, RESULT_ROWS[0]),
            Word::at(
                format!("You now have {} gp.", run.gold),
                RESULT_X,
                RESULT_ROWS[1],
            ),
            Word::at("Press fire to continue", RESULT_X, RESULT_ROWS[2]),
        ]
    }

    /// `GOLDASCII` at (0x11a, 0xe), which `TavernLoop` writes every pass.
    pub fn gold_word(run: &Run) -> Word {
        Word::at(run.gold.to_string(), TAVERN_GOLD_AT.0, TAVERN_GOLD_AT.1)
    }
}

impl Run {
    /// `_TAVERN:RollDice` (0xb234) and `DiceWinner` (0xb2e0), for a stake
    /// `SetBET` has already taken out of the purse.
    ///
    /// ```text
    /// 0b237  mov cx, 3
    /// 0b23a  call RND; and ax, 7; cmp ax, 5; jg 0b23a   ; six faces
    /// 0b245  mov [si], al; inc si; loop
    /// 0b289  call DiceSort                              ; ascending
    /// 0b292  eleven records of DiceODDS, two words at a time
    /// DiceWinner 0b2e0
    /// 0b2e0  ax = BET; bl = [di+4]; mul bx; add [si+0x32], ax
    /// 0b2f1  cmp [si+0x32], 0x96; jle; mov [si+0x32], 0x96
    /// ```
    pub fn roll_dice(&mut self, stake: u32) -> Throw {
        let mut dice = [0u8; crate::service::DICE];
        for d in dice.iter_mut() {
            // `and ax, 7; cmp ax, 5; jg` rolls again: a bounded re-roll here,
            // and a modulus past it, so a run can never hang on a die.
            let mut face = crate::service::FACES;
            for _ in 0..8 {
                let n = self.next_roll() & 7;
                if n < crate::service::FACES {
                    face = n;
                    break;
                }
            }
            if face == crate::service::FACES {
                face = self.roll(crate::service::FACES);
            }
            *d = face as u8;
        }
        dice.sort_unstable();
        let won = ODDS
            .iter()
            .find(|(pattern, _)| *pattern == dice)
            .map_or(0, |(_, odds)| stake.saturating_mul(*odds));
        if won > 0 {
            self.gold = (self.gold + won).min(PURSE_CEILING);
        }
        Throw { dice, stake, won }
    }
}

// ------------------------------------------------- the healer and the mystic

/// Which of `_WIZARD`'s two counters: the routine at 0xba66 over `HEA.PIV`
/// and the one at 0xb935 over `MYS.PIV`. They are one shape: the picture, the
/// greeting, fire, the bowl, the verdict, fire.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Counter {
    Healer,
    Mystic,
}

impl Counter {
    /// `mov ax, 1; call InitDonation` at 0xbaba and `mov ax, 0` at 0xb989:
    /// `BAG`, the purse cel `DonationRefresh` blits at both ends of the panel.
    pub fn bag_cel(self) -> usize {
        match self {
            Counter::Healer => 1,
            Counter::Mystic => 0,
        }
    }

    /// The greeting chain: `Heal1a` (DS:0xdb69) and `My1a` (DS:0xdd32), every
    /// record centred, at the rows the records carry.
    pub fn greeting(self) -> Vec<Word> {
        match self {
            Counter::Healer => vec![
                Word::centred(
                    "Good Day Sir Knight, would you care for a healing.  I have",
                    0xa3,
                ),
                Word::centred(
                    "the best roots, herbs and leeches on this side of the land.",
                    0xac,
                ),
                Word::centred("I am at your service for a small donation", 0xb5),
            ],
            Counter::Mystic => vec![
                Word::centred(
                    "Welcome my child.  I am here to help you in your quest",
                    0xa5,
                ),
                Word::centred("I have the powers to reach into the cosmos and give", 0xad),
                Word::centred("your body new skills and agility.", 0xb5),
                Word::centred("I am at your service for a small donation", 0xbd),
            ],
        }
    }
}

/// Where both counters put the pointer before the bowl: `mov word ptr
/// [PointerX], 0xa0; mov word ptr [PointerY], 0xaa` at 0xb97d and 0xbaae.
pub const BOWL_POINTER: (i32, i32) = (0xa0, 0xaa);

/// `MysticFini` at 0xba20: the verdict drawn, then `mov ax, 0x32; call
/// 0xafeb`, fifty retraces, and only then `WaitFIRE`.
pub const VERDICT_WAIT: u32 = 0x32;

/// Where a counter's visit has got to.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Stage {
    /// The greeting is up and `WaitFIRE` (0x8251) is waiting.
    Greeting,
    /// `DonateLoop`.
    Bowl(Donation),
    /// The verdict is up; `wait` retraces and then `WaitFIRE` again.
    Verdict { words: Vec<Word>, wait: u32 },
    /// Back out to the town.
    Done,
}

/// One visit to the healer or the mystic.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Visit {
    pub counter: Counter,
    pub stage: Stage,
}

/// `Heal2a`, what both say to a knight who leaves the bowl without pressing
/// `OkDonation` (`LeaveMystic`, 0xba18).
pub const LEAVE_LINE: &str = "May Danu aid you in you quest";
/// `Heal3a`, what both say to an empty bowl (`NoDonation`, 0xba13).
pub const NO_DONATION_LINE: &str = "Sorry I cannot be of help.  Come back anytime";

impl Visit {
    pub fn open(counter: Counter) -> Visit {
        Visit {
            counter,
            stage: Stage::Greeting,
        }
    }

    /// Fire, on the two stages that wait for it.
    ///
    /// On the greeting, `WaitFIRE` returns and `InitDonation` runs: the purse
    /// into `GOLDP`, the bowl empty. On the verdict, only once the fifty
    /// retraces are up, the routine fades out and returns to `HWINIT`.
    pub fn fire(&mut self, run: &Run) {
        match &self.stage {
            Stage::Greeting => self.stage = Stage::Bowl(Donation::open(run.gold)),
            Stage::Verdict { wait: 0, .. } => self.stage = Stage::Done,
            _ => {}
        }
    }

    /// One retrace of the verdict's wait.
    pub fn tick(&mut self) {
        if let Stage::Verdict { wait, .. } = &mut self.stage {
            *wait = wait.saturating_sub(1);
        }
    }

    /// One of the bowl's four gadgets pressed. The two that close it hand
    /// `ax` back to the counter's routine, and what follows is:
    ///
    /// ```text
    /// the healer, 0xbac0             the mystic, 0xb98f
    ///   cmp ax, 3; jne LeaveMystic     cmp word ptr [DONATION], 0; je NoDonation
    ///   bx = DONATION                  cmp ax, 3; jne LeaveMystic
    ///   or bx, bx; je NoDonation       call 0xb7e9 (the ability), MysticUpDown
    ///   HealDon ... ExitHealer         MysticAbility / MysticJudge / ExitMystic
    /// ```
    ///
    /// The two test their conditions in opposite orders, which shows once: a
    /// knight who fills the bowl and then presses the exit is told `May Danu
    /// aid you` by the healer and `Sorry I cannot be of help` by the mystic,
    /// because `ExitDonation` has emptied `DONATION` by the time the mystic
    /// looks at it. Both are kept as they stand.
    pub fn press(&mut self, op: DonateOp, run: &mut Run, items: &Items) {
        let Stage::Bowl(bowl) = &mut self.stage else {
            return;
        };
        if !bowl.act(op) {
            return;
        }
        // `OkDonation` writes `GOLDP` into `[si+0x32]`; `ExitDonation` zeroes
        // both words and writes nothing back. `GOLDP + DONATION` is the purse
        // as it was, so the purse is untouched here either way and the two
        // routines below take the donation out of it themselves, which is
        // what `sub [si+0x32]` in each of them amounts to.
        let given = if op == DonateOp::Ok { bowl.given } else { 0 };
        debug_assert_eq!(bowl.purse + bowl.given, run.gold);
        let words = match self.counter {
            Counter::Healer => {
                if op != DonateOp::Ok {
                    vec![Word::centred(LEAVE_LINE, 0xb4)]
                } else if given == 0 {
                    vec![Word::centred(NO_DONATION_LINE, 0xb4)]
                } else {
                    // `HealDon` spends the bowl down.
                    match run.donate_to_healer(given) {
                        Some(got) => healer_verdict(&got),
                        None => vec![Word::centred(LEAVE_LINE, 0xb4)],
                    }
                }
            }
            Counter::Mystic => {
                if given == 0 {
                    vec![Word::centred(NO_DONATION_LINE, 0xb4)]
                } else if op != DonateOp::Ok {
                    vec![Word::centred(LEAVE_LINE, 0xb4)]
                } else {
                    mystic_verdict(run.consult_the_mystic(given, items))
                }
            }
        };
        self.stage = Stage::Verdict {
            words,
            wait: VERDICT_WAIT,
        };
    }
}

/// `ExitHealer` at 0xbb1b, choosing between `Heal4a`, `Heal5a` and `Heal6a`
/// on the donation as it was before `HealDon` spent it and on the bit
/// `HealDon` set when it cleared the ratman's bite.
///
/// ```text
/// Heal4a  DS:0xdb9b  two records, rows 0xaf and 0xb9
/// Heal5a  DS:0xdbaf  one record, row 0xb4
/// Heal6a  DS:0xdbb9  one record, row 0xb4
/// ```
pub fn healer_verdict(got: &Healing) -> Vec<Word> {
    if got.gave <= 9 {
        vec![
            Word::centred("For that amount of coin the best I can", 0xaf),
            Word::centred("offer you is some roots and herbal tea", 0xb9),
        ]
    } else if got.unbitten {
        vec![Word::centred("You have been healed.", 0xb4)]
    } else {
        vec![Word::centred("You are healed of your wounds", 0xb4)]
    }
}

/// `MysticJudge` at 0xb9fc, which walks `MysticTable` (DS:0xde04) or
/// `MyLowTab` (DS:0xde1c) for the ability's offset and takes the chain beside
/// it, and the three chains the arms above it name outright.
///
/// ```text
/// MysticTable  2e -> MySa  2f -> MyCa  30 -> MyEa   (granted)
/// MyLowTab     2e -> My5a  2f -> My3a  30 -> My4a   (taken)
/// My6a  0xb9ad   every ability at five        rows 0xaa, 0xb4, 0xbe
/// My7a  ExitMystic (0xba1d)                   rows 0xb4, 0xbe
/// ```
///
/// One quirk of the original is not reproduced: `MysticJudge` walks the
/// table with `cx` of two, so endurance, the third entry, is never found and
/// `ExitMystic`'s `My7a` is said after the point is quietly given or taken.
/// The endurance chains are in the image and are used.
pub fn mystic_verdict(reading: Reading) -> Vec<Word> {
    use crate::knight::Ability;
    match reading {
        Reading::Granted(Ability::Strength) => {
            vec![Word::centred(
                "The cosmos has granted you more strength.",
                0xb4,
            )]
        }
        Reading::Granted(Ability::Constitution) => vec![
            Word::centred("I see many harsh physical hardships in your future.", 0xb4),
            Word::centred("The cosmos has granted you more constitution", 0xbe),
        ],
        Reading::Granted(Ability::Endurance) => vec![
            Word::centred("I see many long paths that you will need to follow.", 0xb4),
            Word::centred("I will grant you more endurance.", 0xbe),
        ],
        Reading::Taken(Ability::Strength) => vec![
            Word::centred("The cosmos has been hard on your soul and", 0xb4),
            Word::centred("has stolen some of your strength.", 0xbe),
        ],
        Reading::Taken(Ability::Constitution) => vec![
            Word::centred(
                "Your soul has taken much punishment within the cosmos.",
                0xb4,
            ),
            Word::centred("You have lost some of your constitution.", 0xbe),
        ],
        Reading::Taken(Ability::Endurance) => vec![
            Word::centred("The cosmos has imprisoned part of your soul", 0xb4),
            Word::centred("thus stealing away part of your endurance.", 0xbe),
        ],
        Reading::Maxed => vec![
            Word::centred(
                "You have already reached skills and agilities that even I",
                0xaa,
            ),
            Word::centred(
                "can no longer raise or improve upon.  Now go and complete",
                0xb4,
            ),
            Word::centred(
                "your quest before the Black Knights fulfill their treachery.",
                0xbe,
            ),
        ],
        Reading::Weak => vec![
            Word::centred(
                "My powers are weak right now and I was unable to reach the",
                0xb4,
            ),
            Word::centred("cosmos.  Perhaps later in the week I may be of help.", 0xbe),
        ],
        Reading::NoDonation => vec![Word::centred(NO_DONATION_LINE, 0xb4)],
        Reading::TooPoor => vec![Word::centred(LEAVE_LINE, 0xb4)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{ItemDef, Virtue};
    use crate::knight::KnightDef;
    use crate::taskvm::{End, Instr, Part, Script};
    use std::collections::BTreeMap;

    #[rustfmt::skip]
    fn goods() -> Items {
        let mut items = Items::new();
        let mut add = |id: &str, price: u32, virtue: Virtue, consumed: bool| {
            items.insert(id.into(), ItemDef { name: id.into(), price, virtue, consumed });
        };
        add("potion", 20, Virtue::Restore, true);
        add("gem_of_seeing", 32, Virtue::Inert, false);
        add("sword_of_sharpness", 100, Virtue::Weapon { damage: 5 }, false);
        add("ring_of_protection", 50, Virtue::Ward { health: 20 }, false);
        add("talisman_of_the_wyrm", 52, Virtue::Inert, false);
        add("scroll_of_haste", 36, Virtue::Haste, true);
        add("long_sword", 0, Virtue::Weapon { damage: 0 }, false);
        add("broad_sword", 10, Virtue::Weapon { damage: 2 }, false);
        add("claymore", 25, Virtue::Weapon { damage: 3 }, false);
        add("padded_armour", 0, Virtue::Armour { health: 0, stride: 0 }, false);
        add("chain_mail", 30, Virtue::Armour { health: 10, stride: 1 }, false);
        add("plate_armour", 50, Virtue::Armour { health: 20, stride: 0 }, false);
        add("battle_armour", 75, Virtue::Armour { health: 30, stride: 2 }, false);
        for k in crate::moon::Key::ALL {
            add(k.item(), 12, Virtue::Inert, false);
        }
        for m in crate::moon::Moonstone::ALL {
            add(m.item(), 20, Virtue::Inert, false);
        }
        items
    }

    fn run() -> (Run, Items) {
        let items = goods();
        let def = KnightDef {
            name: "SIR GODBER".into(),
            shades: vec![0x2244cc],
            home: [16, 16],
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
        r.kit.capacity = 40;
        (r, items)
    }

    // The merchant: `BuyGoods`, `BuyArmour`, `BuyWeapon`, `BuyDagger`.

    /// `BuyArmour` writes the suit and adds its health outright, whatever was
    /// on the back before: a knight in battle armour who buys chain mail is
    /// in chain mail.
    #[test]
    fn the_merchant_sells_a_suit_at_its_own_price_and_puts_it_on() {
        let (mut r, items) = run();
        r.gold = 29;
        assert!(
            !r.buy_goods(0x42, 1, &items),
            "twenty nine will not buy chain mail"
        );
        assert_eq!(r.knight.armour, "padded_armour");
        r.gold = 30;
        assert!(r.buy_goods(0x42, 1, &items));
        assert_eq!(r.gold, 0);
        assert_eq!(r.knight.armour, "chain_mail");
        assert_eq!(r.max_health, 30, "10 * con + 10 + 10 for the mail");
        assert_eq!(r.health, 30, "and `add [si+0x38], 0xa` filled it");
        r.gold = 75;
        assert!(r.buy_goods(0x42, 4, &items));
        assert_eq!(r.knight.armour, "battle_armour");
        assert_eq!(r.max_health, 50);
        r.gold = 50;
        assert!(
            r.buy_goods(0x42, 2, &items),
            "and back down to plate: no check"
        );
        assert_eq!(r.knight.armour, "plate_armour");
        assert_eq!(r.health, 40, "the ceiling pulled it down");
    }

    /// `cmp word ptr [si+0x40], 0x17; jge ret`: a sword is only sold to a
    /// hand holding a lesser one.
    #[test]
    fn the_merchant_sells_a_sword_only_to_a_lesser_hand() {
        let (mut r, items) = run();
        r.gold = 100;
        assert!(r.buy_goods(0x40, 1, &items));
        assert_eq!(r.knight.weapon, "broad_sword");
        assert_eq!(r.gold, 90);
        assert!(!r.buy_goods(0x40, 1, &items), "not a second broad sword");
        assert!(r.buy_goods(0x40, 2, &items));
        assert_eq!(r.knight.weapon, "claymore");
        assert_eq!(r.gold, 65);
        assert!(!r.buy_goods(0x40, 1, &items), "and never back down");
        r.knight.weapon = "sword_of_sharpness".into();
        assert!(!r.buy_goods(0x40, 2, &items));
        r.knight.weapon = "long_sword".into();
        r.gold = 9;
        assert!(!r.buy_goods(0x40, 1, &items), "ten is the price");
    }

    /// `BuyDagger`: two gold, and `cmp byte ptr [si+0x34], 0xa; jge` stops at
    /// ten.
    #[test]
    fn daggers_are_two_gold_each_and_stop_at_ten() {
        let (mut r, items) = run();
        r.gold = 20;
        r.knight.daggers = 8;
        assert!(r.buy_goods(0x34, 0, &items));
        assert!(r.buy_goods(0x34, 0, &items));
        assert!(!r.buy_goods(0x34, 0, &items), "ten is the belt");
        assert_eq!((r.knight.daggers, r.gold), (10, 16));
        r.knight.daggers = 0;
        r.gold = 1;
        assert!(!r.buy_goods(0x34, 0, &items));
        assert!(
            !r.buy_goods(0x32, 0, &items),
            "nothing else is on the stall"
        );
    }

    // The temple: `TTemple`, `SellToTemple`, `GoldSell`.

    /// A sale is half the price into a capped purse, and what was sold is on
    /// the temple's own counter afterwards, where it can be bought back at
    /// the full price.
    #[test]
    fn the_temple_buys_at_half_and_sells_it_back_at_full() {
        let (mut r, items) = run();
        r.kit.take("gem_of_seeing", 1);
        assert!(
            r.trade_at_temple(2, 0, 0x40, &items),
            "the pointer on the knight's arch"
        );
        assert_eq!(r.gold, 26, "ten and sixteen");
        assert_eq!(r.kit.count("gem_of_seeing"), 0);
        assert_eq!(r.temple.gems, 1, "and the temple has it");
        assert!(
            !r.trade_at_temple(2, 0, 0x40, &items),
            "there is no second gem to sell"
        );
        assert!(
            !r.trade_at_temple(2, 0, 0xa0, &items),
            "and twenty six will not buy it back"
        );
        r.gold = 32;
        assert!(r.trade_at_temple(2, 0, 0xa0, &items));
        assert_eq!(
            (r.gold, r.kit.count("gem_of_seeing"), r.temple.gems),
            (0, 1, 0)
        );
        assert!(
            !r.trade_at_temple(2, 0, 0xa0, &items),
            "the counter is empty again"
        );
    }

    /// `cmp bx, 4; jne; mov word ptr [bp+0x40], 0x16`: the sword goes out
    /// of the hand and a long sword is left in it; bought back, the routine
    /// at 0x28d puts it in the hand again.
    #[test]
    fn selling_the_magic_sword_leaves_a_long_sword_in_the_hand() {
        let (mut r, items) = run();
        r.knight.weapon = "sword_of_sharpness".into();
        assert!(r.trade_at_temple(4, 0, 0, &items));
        assert_eq!(r.knight.weapon, "long_sword");
        assert_eq!(r.gold, 60, "fifty for it");
        assert!(r.temple.magic_sword);
        assert!(!r.trade_at_temple(4, 0, 0, &items), "nothing left to sell");
        r.gold = 100;
        assert!(r.trade_at_temple(4, 0, TEMPLE_MIDDLE, &items));
        assert_eq!(r.knight.weapon, "sword_of_sharpness");
        assert_eq!(r.gold, 0);
        assert!(!r.temple.magic_sword);
        assert_eq!(
            r.kit.count("sword_of_sharpness"),
            0,
            "it is in the hand, not the pack"
        );
    }

    /// The keys and the moonstones move as bits: `or [bx+di], dl; xor
    /// [bx+si], dl`, and the price is `MagicPrices[0x14]` or `[0x16]`.
    #[test]
    fn a_key_and_a_moonstone_move_as_bits_at_their_own_prices() {
        let (mut r, items) = run();
        let key = crate::moon::Key::ALL[1];
        let stone = crate::moon::Moonstone::ALL[2];
        r.kit.take(key.item(), 1);
        r.kit.take(stone.item(), 1);
        assert!(r.trade_at_temple(0x14, key.bit(), 0, &items));
        assert_eq!(r.gold, 16, "six for a key");
        assert_eq!(r.temple.keys, key.bit());
        assert!(
            !r.trade_at_temple(0x14, crate::moon::Key::ALL[0].bit(), 0, &items),
            "not a key you have not got"
        );
        assert!(r.trade_at_temple(0x16, stone.bit(), 0, &items));
        assert_eq!(r.gold, 26, "ten for a moonstone");
        assert_eq!(r.temple.moonstones, stone.bit());
        r.gold = 12;
        assert!(r.trade_at_temple(0x14, key.bit(), 0xa0, &items));
        assert_eq!(r.kit.count(key.item()), 1);
        assert_eq!((r.temple.keys, r.gold), (0, 0));
    }

    /// `cmp [bp+0x32], 0x96; jle; mov [bp+0x32], 0x96` in `GoldSell`, and the
    /// ring's `add word ptr [bp+0x38], 0x14` on the way in.
    #[test]
    fn the_purse_stops_at_a_hundred_and_fifty_and_a_ring_bought_is_worn() {
        let (mut r, items) = run();
        r.gold = 140;
        r.kit.take("talisman_of_the_wyrm", 1);
        assert!(r.trade_at_temple(8, 0, 0, &items));
        assert_eq!(
            r.gold, 150,
            "twenty six would have made it a hundred and sixty six"
        );
        r.temple.rings = 1;
        r.health = 5;
        assert!(r.trade_at_temple(6, 0, 0xa0, &items));
        assert_eq!(r.gold, 100);
        assert_eq!(r.kit.count("ring_of_protection"), 1);
        assert_eq!(r.max_health, 40, "twenty on the ceiling for the ring");
        assert_eq!(r.health, 25, "and twenty on the health outright");
    }

    // The tavern.

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

    fn set() -> ScriptSet {
        let mut s: ScriptSet = BTreeMap::new();
        s.insert(
            crate::dice::SHAKE.into(),
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
        s.insert(crate::dice::THROW.into(), Script::new(throw));
        s
    }

    fn frames(t: &mut Tavern, set: &ScriptSet, run: &mut Run, n: usize) {
        for _ in 0..n * crate::dice::TICKS_PER_FRAME as usize {
            t.tick(set, run);
        }
    }

    /// `cmp word ptr [si+0x32], 0; jg` at 0xb00b: an empty purse is turned
    /// away before the tavern is drawn at all.
    #[test]
    fn an_empty_purse_never_gets_through_the_door() {
        let (mut r, _) = run();
        r.gold = 0;
        assert!(Tavern::open(&r).is_none());
        r.gold = 1;
        assert!(Tavern::open(&r).is_some());
    }

    /// The six gadgets are the ones at 0xb053: five stakes with `+0xe` 1 and
    /// the stake in `+0x10`, and the exit with `+0xe` 2, all on the parchment
    /// at x 0x10c.
    #[test]
    fn the_six_gadgets_are_five_stakes_and_an_exit_on_the_parchment() {
        for (n, (x, y, w, h, id, strp)) in TAVERN_GADGETS.iter().enumerate() {
            assert_eq!(*x, 0x10c);
            if n < 5 {
                assert_eq!((*w, *h, *id, *strp), (0x2b, 0x18, STAKE_ID, n as u16 + 1));
            } else {
                assert_eq!((*y, *w, *h, *id), (0xb5, 0x2d, 0x10, TAVERN_EXIT_ID));
            }
        }
        assert_eq!(TAVERN_GADGETS[1].1, 0x42);
        assert_eq!(TAVERN_GADGETS[4].1, 0x93);
    }

    /// `SetBET`: the stake is paid the moment the handler takes it, the hand
    /// throws, `DiceRND` rolls on the frame it lands, and the dice picture
    /// waits twenty retraces and then fire before the table comes back with
    /// the hand shaking again.
    #[test]
    fn a_stake_pays_throws_shows_the_dice_and_comes_back_to_the_table() {
        let (mut r, _) = run();
        r.gold = 20;
        let set = set();
        let mut t = Tavern::open(&r).unwrap();
        frames(&mut t, &set, &mut r, 3);
        t.press(STAKE_ID, 5, &mut r);
        assert_eq!(r.gold, 15, "`sub [si+0x32], ax` at 0xb1fc");
        assert_eq!(t.bet, 5);
        t.press(STAKE_ID, 3, &mut r);
        assert_eq!((r.gold, t.bet), (15, 5), "a second stake is not taken");
        let mut played = 0;
        while t.screen == TavernScreen::Table && played < 200 {
            frames(&mut t, &set, &mut r, 1);
            played += 1;
        }
        assert_eq!(t.screen, TavernScreen::Dice, "`DiceRND` after the throw");
        assert!(played > 10, "the throw played its fourteen frames first");
        let throw = t.result.expect("RollDice has rolled");
        assert_eq!(throw.stake, 5);
        assert!(throw.dice.windows(2).all(|w| w[0] <= w[1]), "DiceSort");
        let expect = if throw.winner() { 15 + throw.won } else { 15 };
        assert_eq!(r.gold, expect.min(PURSE_CEILING));
        let words = t.result_words(&r);
        assert_eq!(words.len(), 3);
        assert_eq!((words[0].x, words[0].y), (RESULT_X, RESULT_ROWS[0]));
        assert_eq!(words[2].text, "Press fire to continue");
        assert_eq!(words[1].text, format!("You now have {} gp.", r.gold));
        // `DiceWait`: fire is not read for twenty retraces.
        t.fire(&r);
        assert_eq!(
            t.screen,
            TavernScreen::Dice,
            "fire before the wait does nothing"
        );
        for _ in 0..RESULT_WAIT {
            t.tick(&set, &mut r);
        }
        t.fire(&r);
        assert_eq!(t.screen, TavernScreen::Table, "`jmp TavernOpenScene`");
        assert!(t.result.is_none());
        assert!(!t.left);
        assert!(!t.table.staked(), "`DiceTHROW` is zero and the hand shakes");
    }

    /// `cmp ax, [si+0x32]; jg BBB` at 0xb1f7: a stake over the purse is
    /// refused and the hand goes on shaking.
    #[test]
    fn a_stake_the_purse_cannot_cover_is_refused_and_the_hand_shakes_on() {
        let (mut r, _) = run();
        r.gold = 3;
        let set = set();
        let mut t = Tavern::open(&r).unwrap();
        t.press(STAKE_ID, 5, &mut r);
        assert_eq!(r.gold, 3);
        assert!(!t.table.staked());
        frames(&mut t, &set, &mut r, 40);
        assert_eq!(t.screen, TavernScreen::Table);
        assert!(!t.left);
    }

    /// `XFL` is set by the handler and read by `TavernLoop` after the frame,
    /// and `TavernOpenScene` turns an emptied purse out at the door on the
    /// way back from the dice.
    #[test]
    fn the_exit_leaves_and_so_does_an_emptied_purse() {
        let (mut r, _) = run();
        r.gold = 1;
        let set = set();
        let mut t = Tavern::open(&r).unwrap();
        t.press(TAVERN_EXIT_ID, 0, &mut r);
        assert!(!t.left, "not until the shake reaches its `ff ff`");
        frames(&mut t, &set, &mut r, 3);
        assert!(t.left);

        let mut t = Tavern::open(&r).unwrap();
        t.press(STAKE_ID, 1, &mut r);
        assert_eq!(r.gold, 0);
        let mut n = 0;
        while t.screen == TavernScreen::Table && n < 200 {
            frames(&mut t, &set, &mut r, 1);
            n += 1;
        }
        for _ in 0..RESULT_WAIT {
            t.tick(&set, &mut r);
        }
        if r.gold == 0 {
            t.fire(&r);
            assert!(t.left, "`cmp word ptr [si+0x32], 0; jle LeaveTavern`");
        }
    }

    /// The same seed throws the same dice, on this side of the door as on
    /// the other.
    #[test]
    fn a_seeded_dice_game_replays_identically() {
        let (mut a, _) = run();
        a.gold = 100;
        let mut b = a.clone();
        let left: Vec<Throw> = (0..40).map(|_| a.roll_dice(5)).collect();
        let right: Vec<Throw> = (0..40).map(|_| b.roll_dice(5)).collect();
        assert_eq!(left, right);
        let json = serde_json::to_string(&a).unwrap();
        let mut restored: Run = serde_json::from_str(&json).unwrap();
        assert_eq!(a.roll_dice(5), restored.roll_dice(5));
    }

    /// Two hundred throws see every face and pay at least once, and never
    /// past the ceiling.
    #[test]
    fn the_dice_use_all_six_faces_and_sometimes_pay_and_never_past_the_ceiling() {
        let (mut r, _) = run();
        let mut seen = [false; crate::service::FACES as usize];
        let mut wins = 0;
        for _ in 0..200 {
            r.gold = 149;
            let t = r.roll_dice(5);
            for d in t.dice {
                seen[d as usize] = true;
            }
            if t.winner() {
                wins += 1;
                assert_eq!(t.won, 5 * t.odds().unwrap());
            }
            assert!(r.gold <= PURSE_CEILING);
        }
        assert!(seen.iter().all(|s| *s), "every face comes up: {seen:?}");
        assert!(wins > 0);
    }

    // The healer and the mystic.

    /// The greeting, fire, the bowl, `OkDonation`, `HealDon`, the verdict.
    #[test]
    fn the_healer_greets_takes_the_bowl_and_says_what_it_bought() {
        let (mut r, items) = run();
        r.gold = 40;
        r.health = 5;
        let mut v = Visit::open(Counter::Healer);
        assert_eq!(v.counter.greeting().len(), 3);
        assert_eq!(v.counter.greeting()[0].y, 0xa3);
        assert_eq!(v.counter.bag_cel(), 1, "`mov ax, 1` at 0xbaba");
        v.press(DonateOp::More, &mut r, &items);
        assert_eq!(v.stage, Stage::Greeting, "nothing takes a coin before fire");
        v.fire(&r);
        let Stage::Bowl(b) = v.stage.clone() else {
            panic!("the bowl");
        };
        assert_eq!((b.purse, b.given), (40, 0));
        for _ in 0..10 {
            v.press(DonateOp::More, &mut r, &items);
        }
        assert_eq!(r.gold, 40, "the purse is `GOLDP` until the bowl closes");
        v.press(DonateOp::Ok, &mut r, &items);
        let Stage::Verdict { words, wait } = &v.stage else {
            panic!("the verdict");
        };
        assert_eq!(wait, &VERDICT_WAIT);
        assert_eq!(words[0].text, "You are healed of your wounds");
        assert_eq!(words[0].y, 0xb4);
        assert_eq!(r.gold, 30);
        assert_eq!(r.health, r.max_health);
        v.fire(&r);
        assert!(
            matches!(v.stage, Stage::Verdict { .. }),
            "fifty retraces first"
        );
        for _ in 0..VERDICT_WAIT {
            v.tick();
        }
        v.fire(&r);
        assert_eq!(v.stage, Stage::Done);
    }

    /// `ExitDonation` against `OkDonation`, and the two routines' opposite
    /// orders of test: the healer says `Heal2a` to a filled bowl abandoned,
    /// the mystic `Heal3a`, and nine coins buy the healer's tea.
    #[test]
    fn leaving_the_bowl_and_an_empty_bowl_get_their_own_lines() {
        let (mut r, items) = run();
        r.gold = 40;
        let mut v = Visit::open(Counter::Healer);
        v.fire(&r);
        for _ in 0..5 {
            v.press(DonateOp::More, &mut r, &items);
        }
        v.press(DonateOp::Exit, &mut r, &items);
        assert_eq!(r.gold, 40, "`ExitDonation` writes nothing back");
        let Stage::Verdict { words, .. } = &v.stage else {
            panic!()
        };
        assert_eq!(words[0].text, LEAVE_LINE);

        let mut v = Visit::open(Counter::Mystic);
        assert_eq!(v.counter.bag_cel(), 0, "`mov ax, 0` at 0xb989");
        v.fire(&r);
        for _ in 0..5 {
            v.press(DonateOp::More, &mut r, &items);
        }
        v.press(DonateOp::Exit, &mut r, &items);
        let Stage::Verdict { words, .. } = &v.stage else {
            panic!()
        };
        assert_eq!(
            words[0].text, NO_DONATION_LINE,
            "the mystic reads DONATION first"
        );

        let mut v = Visit::open(Counter::Healer);
        v.fire(&r);
        v.press(DonateOp::Ok, &mut r, &items);
        let Stage::Verdict { words, .. } = &v.stage else {
            panic!()
        };
        assert_eq!(words[0].text, NO_DONATION_LINE);

        let mut v = Visit::open(Counter::Healer);
        v.fire(&r);
        for _ in 0..9 {
            v.press(DonateOp::More, &mut r, &items);
        }
        v.press(DonateOp::Ok, &mut r, &items);
        let Stage::Verdict { words, .. } = &v.stage else {
            panic!()
        };
        assert_eq!(words.len(), 2, "`Heal4a` is two records");
        assert_eq!((words[0].y, words[1].y), (0xaf, 0xb9));
        assert_eq!(r.gold, 31, "and the nine are the healer's");
    }

    /// The mystic's verdict is the chain `MysticJudge` finds beside the
    /// ability, or the one the arm names.
    #[test]
    fn the_mystic_speaks_in_its_own_chains() {
        use crate::knight::Ability;
        let (mut r, items) = run();
        r.gold = 60;
        let mut v = Visit::open(Counter::Mystic);
        v.fire(&r);
        for _ in 0..50 {
            v.press(DonateOp::More, &mut r, &items);
        }
        v.press(DonateOp::Ok, &mut r, &items);
        assert_eq!(r.gold, 10);
        let Stage::Verdict { words, .. } = &v.stage else {
            panic!()
        };
        assert!(!words.is_empty());
        assert!(words.iter().all(|w| w.centred));
        for a in Ability::ALL {
            let up = mystic_verdict(Reading::Granted(a));
            let down = mystic_verdict(Reading::Taken(a));
            assert_eq!(up.join_text(), Reading::Granted(a).describe());
            assert_eq!(down.join_text(), Reading::Taken(a).describe());
        }
        assert_eq!(mystic_verdict(Reading::Maxed).len(), 3);
        assert_eq!(mystic_verdict(Reading::Maxed)[0].y, 0xaa);
    }

    trait JoinText {
        fn join_text(&self) -> String;
    }

    impl JoinText for Vec<Word> {
        fn join_text(&self) -> String {
            self.iter()
                .map(|w| w.text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        }
    }

    /// The healer's three verdicts against the words `Healing::describe`
    /// already carries, so the chains and the lines cannot drift apart.
    #[test]
    fn the_healers_chains_say_what_the_lines_say() {
        let tea = Healing {
            gave: 9,
            ..Healing::default()
        };
        assert_eq!(healer_verdict(&tea).join_text(), tea.describe());
        let mended = Healing {
            gave: 10,
            healed: true,
            ..Healing::default()
        };
        assert_eq!(healer_verdict(&mended).join_text(), mended.describe());
        let unbitten = Healing {
            gave: 10,
            unbitten: true,
            ..Healing::default()
        };
        assert_eq!(healer_verdict(&unbitten).join_text(), unbitten.describe());
    }

    /// `weapon_cel` and `armour_cel` are the record's own numbers.
    #[test]
    fn the_cels_are_the_records_own_numbers() {
        assert_eq!(weapon_cel("long_sword"), 0x16);
        assert_eq!(weapon_cel("sword_of_sharpness"), 0x19);
        assert_eq!(
            weapon_cel("stick"),
            0x16,
            "an unknown blade is read as the long sword"
        );
        assert_eq!(armour_cel("padded_armour"), 0x1b);
        assert_eq!(armour_cel("battle_armour"), 0x1e);
    }
}
