//! The status panel's own tables: what the screen can be, what each of its
//! twenty seven slots is, and what a gadget on one of them says and does.
//!
//! **Recovered.** `_STATUS:SetUpStatus` at image `0xcf45` is the whole of it and
//! it reads as plainly as a table can: seven parallel arrays of twenty seven
//! string pointers, filled by a run of `mov word ptr [si + 2k], imm`, one
//! immediate per slot, preceded by a `loop` that writes `NULL_STR` over all 189
//! words of the seven (`mov si, Inc; mov cx, 0xbd`, and `Inc`..`Identify` is
//! exactly 378 bytes). So the arrays are:
//!
//! ```text
//! Inc       DS:0xedc6    Use      DS:0xedfc    Take     DS:0xee32
//! Purchase  DS:0xee68    Offer    DS:0xee9e    Sell     DS:0xeed4
//! Identify  DS:0xef0a
//! ```
//!
//! and the **slot order is pinned twice over**. `SetUpStatus` writes `ab4`,
//! `ab6`, `ab5` into `Take[0]`, `[1]`, `[2]`, which is Strength, Constitution,
//! Endurance; `DisplayKnight` (`0xc0c2`) prints knight bytes `+0x2e`, `+0x2f`,
//! `+0x30` down rows 0x23, 0x2a, 0x31 and gives the three label cels gadget ids
//! 0, 2 and 4 on those same rows; and `ABorders` puts `STR :`, `CON :`, `END :`
//! there. Every other slot is pinned the same way, by the `STID` the routine
//! that draws its icon hands `AddIconGadget`: `AddIconGadget` (`0xc9ab`) sets
//! the gadget's text record to `RESP + (STID >> 1) * 10`, so **`STID` is twice
//! the slot number** and the drawing routines name every slot.
//!
//! **A gadget carries an operation, not a menu index.** `AddIconGadget` stores
//! `STRP` at `+0x10` and `STPL` at `+0x12`, and `HotGadget` (`0xca0a`) decodes
//! the first: `and cx, 0xf` then 5 cast, 1 take magic, 3 raise an ability, 0xa
//! buy, and anything else falls through to `test ax, 0x20` and `TakeGold`. The
//! high nibble (`and dx, 0xf0; shr dx, 4` four times) is the bit within a field
//! that carries several things: `StatCheckKeys` gives the four keys `STRP`
//! 0x11, 0x21, 0x41 and 0x81, one operation and four masks.
//!
//! **Which array a side of the screen reads is the screen's type.**
//! `SetUpID` (`0xd2b9`) picks the left one and `Paper2` (`0xd3ca`) the right,
//! then each copies its twenty seven pointers into `Response1` (`DS:0xef40`) or
//! `Response2` (`DS:0xf15c`) as ten-byte text records with `x` 0, `y` 3 and
//! flags 3, which is centre plus bold: so **the line a gadget says is drawn
//! centred across the top of the screen in the bold face**, and there is no
//! menu anywhere on the panel. The flags also carry the permission: `or ax,
//! 0x10` on the left and `or ax, 0x20` on the right, both skipped when the
//! array is `Identify`, and `xor word ptr [di + 6], 0x40` on an ability row the
//! experience can pay for. `Identify` is the screen you may look at and not
//! touch.

use serde::{Deserialize, Serialize};

/// How many slots a side of the panel has. `mov cx, 0x1b` in `SetUpID`,
/// `Paper2` and the `Offer` clear.
pub const SLOTS: usize = 27;

/// What `ReDisplay` shifts the right hand party's icons by. `mov word ptr
/// [StatsOffset], 0x96` at `0xbebc`.
pub const RIGHT_OFFSET: i32 = 0x96;

/// What the two single-arch screens shift the knight's own icons by.
/// `DisplayPillars` writes it when `StatTYPE` is in `OffsetValues`.
pub const SINGLE_OFFSET: i32 = 0x4a;

/// `_STATUS:StatTYPE`, which is what every branch in `ReDisplay`, `SetUpID` and
/// `Paper2` tests. The numbers are the image's.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Screen {
    /// Two knights, one in each arch, trading. `ReDisplay` 0xbed4.
    Trade = 1,
    /// A lair's floor in the right arch. `DisplayLair`.
    Lair = 2,
    /// The stone circle's offering: `MOON:Henge+74` (0x109a) is `mov ax, 3`
    /// and the panel, and `SetUpID` 0xd324 picks `Offer` for it. This was
    /// called the temple here once; nothing in a town passes 3.
    Henge = 3,
    /// The merchant's stall: `MOON:MERC+9` (0xea2) and `WMERC+9` (0xdf3) are
    /// `mov ax, 5` and the panel. `DisplayMerchant` fills the right arch.
    Merchant = 5,
    /// The high temple, which buys and sells magic: `MOON:HTEM+9` (0xeb1) is
    /// `mov ax, 6` and the panel. `ReDisplay` 0xbf66 draws its own stock at
    /// `SaveTYPE+2` through `DisplayMagic` and `DisplayMSword`, and every
    /// magic gadget on the page goes to `TTemple`. The Waterdeep mystic is
    /// not a panel at all: it is `_WIZARD`'s routine at 0xb935.
    Temple = 6,
    /// One thing picked up, with the `NEXT` gadget beside it. `DisplayAquire`.
    Acquire = 8,
    /// The plain character sheet. `SetUpID` 0xd365 picks `Use`.
    Sheet = 9,
    /// The same, for the second knight. `ReDisplay` 0xbf13.
    AcquirePair = 0xb,
    /// The dragon's hoard. `DisplayDragon`.
    Dragon = 0xa,
}

impl Screen {
    /// `DisplayPillars`: `OffsetValues` is `{9, 3}` and only those two take
    /// `SingleData` alone, with the knight's numbers shifted right by `0x4a`.
    /// Every other screen walks `TradingData` first, which has no terminator of
    /// its own, so it draws both tables: three pillars and two arches.
    pub fn single_arch(self) -> bool {
        matches!(self, Screen::Sheet | Screen::Henge)
    }

    /// `SetUpID` 0xd317 to 0xd367, in the order the routine tests.
    pub fn left_table(self) -> Table {
        match self {
            Screen::Temple => Table::Sell,
            Screen::Henge => Table::Offer,
            Screen::Sheet => Table::Use,
            _ => Table::Identify,
        }
    }

    /// `Paper2` 0xd3ca to 0xd412. `scouted` is `[0xcca0]`, the gem's own flag:
    /// a lair seen from the air is `Identify` rather than `Take`.
    pub fn right_table(self, scouted: bool) -> Table {
        match self {
            Screen::Dragon | Screen::Trade | Screen::Acquire => Table::Take,
            Screen::Lair if !scouted => Table::Take,
            Screen::Merchant | Screen::Temple => Table::Purchase,
            _ => Table::Identify,
        }
    }
}

/// One of `SetUpStatus`' seven arrays.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Table {
    Inc,
    Use,
    Take,
    Purchase,
    Offer,
    Sell,
    Identify,
}

impl Table {
    /// The array itself.
    pub fn lines(self) -> &'static [&'static str; SLOTS] {
        match self {
            Table::Inc => &INC,
            Table::Use => &USE,
            Table::Take => &TAKE,
            Table::Purchase => &PURCHASE,
            Table::Offer => &OFFER,
            Table::Sell => &SELL,
            Table::Identify => &IDENTIFY,
        }
    }

    /// What slot `n` says on this screen, or nothing where the array still
    /// holds `NULL_STR`.
    pub fn line(self, slot: usize) -> Option<&'static str> {
        match self.lines().get(slot) {
            Some(&"") => None,
            Some(s) => Some(s),
            None => None,
        }
    }

    /// Whether a gadget off this array can be acted on. `SetUpID` and `Paper2`
    /// both OR their permission bit in for every array but this one.
    pub fn acts(self) -> bool {
        self != Table::Identify
    }
}

/// `HotGadget`'s decode of `STRP`'s low nibble.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Op {
    /// 1: `HGTakeMagic`, which moves one magic item between two records.
    TakeMagic = 1,
    /// 2: not a case of its own. `HotGadget` falls past the four tests and
    /// `test ax, 0x20` sends it to `TakeGold`, whose arms are `TKAR` the
    /// armour, `TKWP` the weapon, `TakeSword` and `TKGP` the gold. So this is
    /// the operation on everything a knight wears or carries loose, and only
    /// the right hand side of the screen can do it.
    Take = 2,
    /// 3: `HGAbility`, which spends experience. Lit by the `0x40` bit.
    Raise = 3,
    /// 5: `HGCastMagic`, which is `MagicCast` and the nine `Cast*` arms.
    Cast = 5,
    /// 0xa: `BuyGoods`, which is `BuyArmour`, `BuyWeapon` and `BuyDagger`.
    Buy = 0xa,
}

impl Op {
    /// `and cx, 0xf`, then the four `cmp`s in order.
    pub fn from_strp(strp: u16) -> Option<Op> {
        Some(match strp & 0xf {
            1 => Op::TakeMagic,
            2 => Op::Take,
            3 => Op::Raise,
            5 => Op::Cast,
            0xa => Op::Buy,
            _ => return None,
        })
    }

    /// `and dx, 0xf0` and four `shr dx, 1`: which bit of a field that holds
    /// several things this gadget is. Zero where the field holds one thing.
    pub fn mask_of(strp: u16) -> u8 {
        ((strp & 0xf0) >> 4) as u8
    }
}

/// `AddIconGadget`'s payload, as it sits in the twenty-byte record.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Payload {
    /// `+0x10`, `STRP`.
    pub strp: u16,
    /// `+0x12`, `STPL`: the byte offset into the record the operation works on.
    pub field: u16,
}

impl Payload {
    pub fn new(strp: u16, field: u16) -> Payload {
        Payload { strp, field }
    }

    pub fn op(self) -> Option<Op> {
        Op::from_strp(self.strp)
    }

    pub fn mask(self) -> u8 {
        Op::mask_of(self.strp)
    }
}

/// The gadget id `CreateExit` gives every pillar, and the one `HotGadget` tests
/// first: `cmp word ptr es:[si + 0xe], 7`.
pub const EXIT_ID: usize = 7;

/// `STID` is twice the slot. `AddIconGadget`: `mov ax, [STID]; shr ax, 1` to
/// index `RESP`.
pub fn slot_of(id: usize) -> Option<usize> {
    let slot = id / 2;
    (id.is_multiple_of(2) && slot < SLOTS).then_some(slot)
}

/// What a slot holds, where the pack has an item for it.
pub struct Slot {
    /// Its plain name, which is what `Take[0..3]` and `Identify` call it.
    pub name: &'static str,
    /// The pack's id, where a slot is a thing the kit, the hand or the back can
    /// hold. `None` for the three abilities, the life points, the purse and the
    /// key slot, which stands for all four keys at once.
    pub item: Option<&'static str>,
}

/// The twenty seven slots, in `SetUpStatus`' own order.
///
/// The ids are the reference pack's, and every one of them is the thing the
/// original's own line at that slot names.
#[rustfmt::skip]
pub const SLOT_TABLE: [Slot; SLOTS] = [
    Slot { name: "Strength",             item: None },
    Slot { name: "Constitution",         item: None },
    Slot { name: "Endurance",            item: None },
    Slot { name: "Life points",          item: None },
    Slot { name: "Gold",                 item: None },
    Slot { name: "Dagger",               item: Some("dagger") },
    Slot { name: "Padded armour",        item: Some("padded_armour") },
    Slot { name: "Chain mail",           item: Some("chain_mail") },
    Slot { name: "Plate armour",         item: Some("plate_armour") },
    Slot { name: "Battle armour",        item: Some("battle_armour") },
    Slot { name: "Long sword",           item: Some("long_sword") },
    Slot { name: "Broad sword",          item: Some("broad_sword") },
    Slot { name: "Claymore sword",       item: Some("claymore") },
    Slot { name: "Sword of Sharpness",   item: Some("sword_of_sharpness") },
    Slot { name: "New moon Moonstone",   item: Some("moonstone.new") },
    Slot { name: "Full Moonstone",       item: Some("moonstone.full") },
    Slot { name: "Half Moonstone",       item: Some("moonstone.half") },
    Slot { name: "Key to the Valley",    item: None },
    Slot { name: "Potion of Healing",    item: Some("potion") },
    Slot { name: "Gem of Seeing",        item: Some("gem_of_seeing") },
    Slot { name: "Ring of Protection",   item: Some("ring_of_protection") },
    Slot { name: "Talisman of the Wyrm", item: Some("talisman_of_the_wyrm") },
    Slot { name: "Scroll of Haste",      item: Some("scroll_of_haste") },
    Slot { name: "Scroll of the Hawk",   item: Some("scroll_of_the_hawk") },
    Slot { name: "Scroll of Aquisition", item: Some("scroll_of_acquisition") },
    Slot { name: "Scroll of the Wyrm",   item: Some("scroll_of_the_wyrm") },
    Slot { name: "Scroll of Protection", item: Some("scroll_of_protection") },
];

/// Which slot a pack id is, where it is one of the twenty seven.
pub fn slot_for_item(id: &str) -> Option<usize> {
    SLOT_TABLE.iter().position(|s| s.item == Some(id))
}

/// `Inc`, `SetUpStatus` 0xcfdd: the three `Increase` lines and then what every
/// other slot is called while it is being raised, which is its plain name for
/// the gear and its cast line for the magic.
#[rustfmt::skip]
pub const INC: [&str; SLOTS] = [
    "Increase Strength",          // ab1
    "Increase Constitution",      // ab3
    "Increase Endurance",         // ab2
    "Life points",                // ab7
    "Gold",                       // ab8
    "Dagger",                     // ab9
    "Padded armour",              // ab10
    "Chain mail",                 // ab12
    "Plate armour",               // ab13
    "Battle armour",              // ab14
    "Long sword",                 // ab11
    "Broad sword",                // ab15
    "Claymore sword",             // ab16
    "Sword of Sharpness",         // ab17
    "New moon Moonstone",         // st10
    "Full Moonstone",             // st11
    "Half Moonstone",             // st12
    "Key to the Valley",          // st14
    "Drink Healing potion",       // st1
    "Use Gem of Seeing",          // st2
    "Ring of Protection",         // st3
    "Talisman of the Wyrm",       // st4
    "Cast scroll of Haste",       // st5
    "Cast scroll of the Hawk",    // st7
    "Cast scroll of Aquisition",  // st6
    "Cast scroll of the Wyrm",    // st8
    "Cast scroll of Protection",  // st9
];

/// `Use`, `SetUpStatus` 0xd066. `Inc` with the three abilities named rather
/// than offered: `ab4`, `ab6`, `ab5` in place of `ab1`, `ab3`, `ab2`. This is
/// the plain character sheet's own array.
#[rustfmt::skip]
pub const USE: [&str; SLOTS] = [
    "Strength",                   // ab4
    "Constitution",               // ab6
    "Endurance",                  // ab5
    "Life points",                // ab7
    "Gold",                       // ab8
    "Dagger",                     // ab9
    "Padded armour",              // ab10
    "Chain mail",                 // ab12
    "Plate armour",               // ab13
    "Battle armour",              // ab14
    "Long sword",                 // ab11
    "Broad sword",                // ab15
    "Claymore sword",             // ab16
    "Sword of Sharpness",         // ab17
    "New moon Moonstone",         // st10
    "Full Moonstone",             // st11
    "Half Moonstone",             // st12
    "Key to the Valley",          // st14
    "Drink Healing potion",       // st1
    "Use Gem of Seeing",          // st2
    "Ring of Protection",         // st3
    "Talisman of the Wyrm",       // st4
    "Cast scroll of Haste",       // st5
    "Cast scroll of the Hawk",    // st7
    "Cast scroll of Aquisition",  // st6
    "Cast scroll of the Wyrm",    // st8
    "Cast scroll of Protection",  // st9
];

/// `Take`, `SetUpStatus` 0xcf54: what the other arch offers up.
///
/// `ta11` is two strings in one blob, `Take Daggers` and `Take Padwed armour`,
/// and `SetUpStatus` writes `ab10` over slot 6 rather than the second of them,
/// so the misspelling is never shown.
#[rustfmt::skip]
pub const TAKE: [&str; SLOTS] = [
    "Strength",                   // ab4
    "Constitution",               // ab6
    "Endurance",                  // ab5
    "Life points left",           // ta21
    "Take Gold",                  // ta10
    "Take Daggers",               // ta11
    "Padded armour",              // ab10
    "Take Chainmail",             // ta18
    "Take Plate armour",          // ta17
    "Take Battle armour",         // ta16
    "Long sword",                 // ab11
    "Take Broad sword",           // ta15
    "Take Claymore sword",        // ta13
    "Take Sword of Sharpness",    // ta14
    "Take Moonstone",             // ta20
    "Take Moonstone",             // ta20
    "Take Moonstone",             // ta20
    "Take Key to the Valley",     // ta19
    "Take Potion of Healing",     // ta1
    "Take Gem of Seeing",         // ta2
    "Take Ring of Protection",    // ta3
    "Take Talisman of the Wyrm",  // ta4
    "Take scroll of Haste",       // ta5
    "Take scroll of the Hawk",    // ta7
    "Take scroll of Aquisition",  // ta6
    "Take scroll of the Wyrm",    // ta8
    "Take scroll of Protection",  // ta9
];

/// `Purchase`, `SetUpStatus` 0xd1cb. The seven slots it leaves as `NULL_STR`
/// are the three abilities, the life points, the purse, the padded armour and
/// the long sword: what a knight begins with and no stall sells.
#[rustfmt::skip]
pub const PURCHASE: [&str; SLOTS] = [
    "",                                    //
    "",                                    //
    "",                                    //
    "",                                    //
    "",                                    //
    "Buy a dagger for 2 GP",               // pu6
    "",                                    //
    "Buy Chainmail for 30 GP",             // pu3
    "Buy Plate armour for 50 GP",          // pu4
    "Buy Battle armour for 75 GP",         // pu5
    "",                                    //
    "Buy Broad Sword for 10 GP",           // pu1
    "Buy Claymore sword for 25 GP",        // pu2
    "Buy Sword of Sharpness for 100 GP",   // pu7
    "Buy Moonstone for 20 GP",             // pu18
    "Buy Moonstone for 20 GP",             // pu18
    "Buy Moonstone for 20 GP",             // pu18
    "Buy Key for 12 GP",                   // pu8
    "Buy Potion of healing for 20 GP",     // pu9
    "Buy gem of seeing for 32 GP",         // pu10
    "Buy ring of protection for 50 GP",    // pu11
    "Buy Talisman for 52 GP",              // pu12
    "Buy scroll of Haste for 36 GP",       // pu13
    "Buy scroll of the Hawk for 52 GP",    // pu15
    "Buy scroll of Aquisition for 52 GP",  // pu14
    "Buy scroll of the Wyrm for 40 GP",    // pu16
    "Buy scroll of Protection for 24 GP",  // pu17
];

/// `Offer`, `SetUpStatus` 0xd178: the temple's array. The fourteen slots below
/// the moonstones are cleared to `Blank`, a single space, rather than to
/// `NULL_STR`, which is why they say nothing and still say it.
#[rustfmt::skip]
pub const OFFER: [&str; SLOTS] = [
    " ",                             // Blank
    " ",                             // Blank
    " ",                             // Blank
    " ",                             // Blank
    " ",                             // Blank
    " ",                             // Blank
    " ",                             // Blank
    " ",                             // Blank
    " ",                             // Blank
    " ",                             // Blank
    " ",                             // Blank
    " ",                             // Blank
    " ",                             // Blank
    " ",                             // Blank
    "New moon Moonstone",            // st10
    "Full Moonstone",                // st11
    "Half Moonstone",                // st12
    "Key to the Valley",             // st14
    "Offer potion of Healing",       // sa1
    "Offer Gem of Seeing",           // sa2
    "Offer Ring of Protection",      // sa3
    "Offer Talisman of the Wyrm",    // sa4
    "Offer scroll of Haste",         // sa5
    "Offer scroll of the Hawk",      // sa7
    "Offer scroll of Aquisition",    // sa6
    "Offer scroll of the Wyrm",      // sa8
    "Offer scroll of Protection",    // sa9
];

/// `Sell`, `SetUpStatus` 0xd232. Thirteen of the fourteen prices are exactly half
/// the one `Purchase` asks, which is `GoldSell`'s own `shr ax, 1` written out.
///
/// **The thirteenth is wrong in the original.** `pu13` is `Buy scroll of Haste
/// for 36 GP` and `se13` is `Sell scroll of Haste for 16 GP`, but `GoldSell`
/// computes the payout as `MagicPrices[5] >> 1`, which is 18. The line is a
/// typo in the game's own data and the code pays the other number. Both are
/// kept as they stand: the string is the string, and [`price_of`] with the
/// shift is what a counter should actually hand over.
#[rustfmt::skip]
pub const SELL: [&str; SLOTS] = [
    "",                                     //
    "",                                     //
    "",                                     //
    "",                                     //
    "",                                     //
    "",                                     //
    "",                                     //
    "",                                     //
    "",                                     //
    "",                                     //
    "",                                     //
    "",                                     //
    "",                                     //
    "Sell Sword of Sharpness for 50 GP",    // se7
    "Sell Moonstone for 10 GP",             // se18
    "Sell Moonstone for 10 GP",             // se18
    "Sell Moonstone for 10 GP",             // se18
    "Sell Key for 6 GP",                    // se8
    "Sell Potion of healing for 10 GP",     // se9
    "Sell gem of seeing for 16 GP",         // se10
    "Sell ring for 25 GP",                  // se11
    "Sell Talisman for 26 GP",              // se12
    "Sell scroll of Haste for 16 GP",       // se13
    "Sell scroll of the Hawk for 26 GP",    // se15
    "Sell scroll of Aquisition for 26 GP",  // se14
    "Sell scroll of the Wyrm for 20 GP",    // se16
    "Sell scroll of Protection for 12 GP",  // se17
];

/// `Identify`, `SetUpStatus` 0xd0ef: the array for a screen you may read and
/// not touch. The nine magic items take `sti1`..`sti9`, which are their names
/// without a verb, and the ring and the talisman keep `st3` and `st4` because
/// those two have no `sti` of their own.
#[rustfmt::skip]
pub const IDENTIFY: [&str; SLOTS] = [
    "Strength",              // ab4
    "Constitution",          // ab6
    "Endurance",             // ab5
    "Life points",           // ab7
    "Gold",                  // ab8
    "Dagger",                // ab9
    "Padded armour",         // ab10
    "Chain mail",            // ab12
    "Plate armour",          // ab13
    "Battle armour",         // ab14
    "Long sword",            // ab11
    "Broad sword",           // ab15
    "Claymore sword",        // ab16
    "Sword of Sharpness",    // ab17
    "New moon Moonstone",    // st10
    "Full Moonstone",        // st11
    "Half Moonstone",        // st12
    "Key to the Valley",     // st14
    "Potion of Healing",     // sti1
    "Gem of Seeing",         // sti2
    "Ring of Protection",    // st3
    "Talisman of the Wyrm",  // st4
    "Scroll of Haste",       // sti5
    "Scroll of the Hawk",    // sti7
    "Scroll of Aquisition",  // sti6
    "Scroll of the Wyrm",    // sti8
    "Scroll of Protection",  // sti9
];

/// `MagicPrices`, the twelve words `SetUpStatus` writes at 0xd27b, paired with
/// the slot each belongs to.
///
/// The order is the image's and so is every number. The slots are read off the
/// `pu` lines that carry the same figure: 20 is `Buy Potion of healing for 20
/// GP`, 100 is `Buy Sword of Sharpness for 100 GP`, and so on down. The only
/// pair the figures cannot separate is the two scrolls at 52, and `SetUpStatus`
/// separates them itself by writing `pu14` into slot 24 and `pu15` into 23.
#[rustfmt::skip]
pub const MAGIC_PRICES: [(usize, u32); 12] = [
    (18, 20),   // Potion of healing
    (19, 32),   // gem of seeing
    (13, 100),  // Sword of Sharpness
    (20, 50),   // ring of protection
    (21, 52),   // Talisman
    (22, 36),   // scroll of Haste
    (24, 52),   // scroll of Aquisition
    (23, 52),   // scroll of the Hawk
    (25, 40),   // scroll of the Wyrm
    (26, 24),   // scroll of Protection
    (17, 12),   // Key
    (14, 20),   // Moonstone
];

/// What the counter asks for one of these. `GoldSell`'s `shr ax, 1` is what it
/// pays for one, so a sale is always half.
pub fn price_of(slot: usize) -> Option<u32> {
    // The three moonstone slots share one price: `SetUpStatus` writes `pu18`
    // into all three and `MagicPrices` carries it once.
    let slot = if (14..=16).contains(&slot) { 14 } else { slot };
    MAGIC_PRICES
        .iter()
        .find(|(s, _)| *s == slot)
        .map(|(_, p)| *p)
}

/// The magic record a knight, a lair, a hoard and the temple's stock all are.
///
/// **Recovered, field for field**, out of what reads it. `DisplayKnight` ends
/// on `push word ptr [si + 0x44]; pop word ptr [Address]` and falls straight
/// into `DisplayMagic` at `0xc38e`, so `knight[0x44]` is a pointer to one of
/// these; `ReDisplay` hands `DisplayMagic` `SaveTYPE+2` for the temple's stock
/// and `DisplayLair` hands it `StatMAGIC2`. The offsets are the ones those two
/// routines and `DisplayMSword` read:
///
/// ```text
/// +0x00  potions      DisplayMagic h1$, cel 4
/// +0x02  gems         DisplayMagic 0xc394, cel 9
/// +0x04  magic sword  DisplayMSword 0xc70c, cel 0x19
/// +0x06  rings        DisplayMagic 0xc3d4, cel 3
/// +0x08  talismans    DisplayMagic 0xc414, cel 0xa
/// +0x0a  haste        the five scrolls, cels 0xb to 0xf, through
/// +0x0c  hawk         StatPlaceScroll; STID starts at 0x2c and steps by two,
/// +0x0e  aquisition   which is slots 22 to 26 in order
/// +0x10  wyrm
/// +0x12  protection
/// +0x14  the four keys as bits     StatCheckKeys, cels 5 to 8
/// +0x16  the moonstones as bits    StatCheckKeys m1$, cels 2, 1, 0
/// ```
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hoard {
    pub potions: u8,
    pub gems: u8,
    pub magic_sword: bool,
    pub rings: u8,
    pub talismans: u8,
    /// Haste, hawk, aquisition, wyrm, protection, in the order the five fields
    /// sit in.
    pub scrolls: [u8; 5],
    pub keys: u8,
    pub moonstones: u8,
}

/// The five scroll slots, in the order the record's five fields are read.
pub const SCROLL_SLOTS: [usize; 5] = [22, 23, 24, 25, 26];

impl Hoard {
    /// How many of the thing at this slot the record holds, for the slots the
    /// record covers. `None` for a slot that is not one of its fields.
    pub fn count(&self, slot: usize) -> Option<u8> {
        Some(match slot {
            13 => u8::from(self.magic_sword),
            18 => self.potions,
            19 => self.gems,
            20 => self.rings,
            21 => self.talismans,
            22..=26 => self.scrolls[slot - 22],
            _ => return None,
        })
    }

    /// The record as a pack of goods fills it.
    ///
    /// Everything on it is a slot of [`SLOT_TABLE`], so this is a transcription
    /// rather than a mapping: the five scroll fields are [`SCROLL_SLOTS`] in
    /// order, the keys and the moonstones are the bits `MOON:Valley` and
    /// `MOON:Henge` read, and the magic sword is the one thing the record keeps
    /// that is also worn.
    pub fn of(kit: &crate::item::Inventory, magic_sword: bool) -> Hoard {
        let n = |slot: usize| {
            SLOT_TABLE[slot]
                .item
                .map_or(0, |id| kit.count(id).min(255) as u8)
        };
        let mut scrolls = [0u8; 5];
        for (i, slot) in SCROLL_SLOTS.into_iter().enumerate() {
            scrolls[i] = n(slot);
        }
        let bits = |on: &dyn Fn(usize) -> bool| -> u8 {
            let mut b = 0u8;
            for i in 0..4 {
                if on(i) {
                    b |= 1 << i;
                }
            }
            b
        };
        Hoard {
            potions: n(18),
            gems: n(19),
            magic_sword,
            rings: n(20),
            talismans: n(21),
            scrolls,
            // `StatCheckKeys` tests bits 1, 2, 4 and 8 of `+0x14` in that order
            // and gives them cels 5, 6, 7 and 8, so the bit is the slot on the
            // row and not the order they were found in.
            keys: bits(&|i| {
                crate::moon::Key::ALL
                    .iter()
                    .any(|k| k.bit() == 1 << i && kit.count(k.item()) > 0)
            }),
            moonstones: bits(&|i| {
                crate::moon::Moonstone::ALL
                    .iter()
                    .any(|m| m.bit() == 1 << i && kit.count(m.item()) > 0)
            }),
        }
    }

    /// Set one of those counts.
    pub fn set(&mut self, slot: usize, n: u8) {
        match slot {
            13 => self.magic_sword = n > 0,
            18 => self.potions = n,
            19 => self.gems = n,
            20 => self.rings = n,
            21 => self.talismans = n,
            22..=26 => self.scrolls[slot - 22] = n,
            _ => {}
        }
    }
}

/// The donation panel, which is the healer's and the mystic's whole counter.
///
/// **Recovered.** `_WIZARD:InitDonation` at image `0xbb34` clears the gadget
/// table and adds exactly four, all with `STPL` 0x32 (the purse) and `STID` 1,
/// and `DonateLoop` at `0xbbe6` reads `es:[si + 0x10]` itself rather than going
/// through `HotGadget`: 2 leaves, 3 commits, 4 takes a coin back and 5 puts one
/// in. So the amount is **built a coin at a time** and there is no list of
/// sums anywhere.
///
/// `AddDonation` refuses when the purse is empty and `SubDonation` when the
/// donation is; both are a single `inc`/`dec` pair between `DONATION` and
/// `GOLDP`, and `ExitDonation` throws the whole donation away while
/// `OkDonation` writes `GOLDP` back into `knight[0x32]`.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Donation {
    /// `GOLDP`, what is left in the purse while the panel is up.
    pub purse: u32,
    /// `DONATION`, what is in the bowl.
    pub given: u32,
}

/// `DonateLoop`'s own four operations, which share no numbering with [`Op`].
///
/// What `OkDonation` then buys is `_WIZARD:HealDon` at the healer and
/// `MysticUpDown` at the mystic, and both of those are already transcribed in
/// [`crate::service`]: this module only carries the panel that decides the
/// amount.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DonateOp {
    /// `ExitDonation`: the bowl is emptied and the purse is untouched.
    Exit = 2,
    /// `OkDonation`: `knight[0x32] = GOLDP`.
    Ok = 3,
    /// `SubDonation`.
    Less = 4,
    /// `AddDonation`.
    More = 5,
}

impl DonateOp {
    pub fn from_strp(strp: u16) -> Option<DonateOp> {
        Some(match strp {
            2 => DonateOp::Exit,
            3 => DonateOp::Ok,
            4 => DonateOp::Less,
            5 => DonateOp::More,
            _ => return None,
        })
    }
}

/// The four gadgets `InitDonation` adds, as `(x, y, w, h, op)`.
///
/// The rectangles are the immediates at 0xbb4e, 0xbb7c, 0xbb8c and 0xbbb0, and
/// the cels `DonationRefresh` draws in them are 4, 5, 3 and 2 of the bank
/// `LoadGoldCels` loads.
#[rustfmt::skip]
pub const DONATE_GADGETS: [(i32, i32, i32, i32, DonateOp); 4] = [
    (0x90, 0xa9, 0x0e, 8, DonateOp::Less),
    (0xa2, 0xa9, 0x0e, 8, DonateOp::More),
    (0x83, 0xb9, 0x14, 8, DonateOp::Ok),
    (0xad, 0xb9, 0x20, 8, DonateOp::Exit),
];

/// The cel each of those four is drawn with, in the same order.
pub const DONATE_CELS: [usize; 4] = [4, 5, 3, 2];

impl Donation {
    /// `InitDonation`: the purse is moved into `GOLDP` whole and the bowl
    /// starts empty.
    pub fn open(gold: u32) -> Donation {
        Donation {
            purse: gold,
            given: 0,
        }
    }

    /// One press. `true` when the panel should close, which is what
    /// `ExitDonation` and `OkDonation` both do by returning out of
    /// `DonateLoop`.
    pub fn act(&mut self, op: DonateOp) -> bool {
        match op {
            DonateOp::More => {
                // `cmp word ptr [GOLDP], 0; jne` then `inc DONATION; dec GOLDP`.
                if self.purse > 0 {
                    self.purse -= 1;
                    self.given += 1;
                }
                false
            }
            DonateOp::Less => {
                if self.given > 0 {
                    self.given -= 1;
                    self.purse += 1;
                }
                false
            }
            DonateOp::Ok | DonateOp::Exit => true,
        }
    }

    /// What the purse is worth once the panel closes. `OkDonation` writes
    /// `GOLDP` back; `ExitDonation` writes nothing, so the whole of it returns.
    pub fn closing_gold(&self, op: DonateOp) -> u32 {
        match op {
            DonateOp::Ok => self.purse,
            _ => self.purse + self.given,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every array is twenty seven long, which is the `mov cx, 0x1b` three
    /// different routines walk them with.
    #[test]
    fn the_seven_arrays_are_the_size_setupid_walks() {
        for t in [
            Table::Inc,
            Table::Use,
            Table::Take,
            Table::Purchase,
            Table::Offer,
            Table::Sell,
            Table::Identify,
        ] {
            assert_eq!(t.lines().len(), SLOTS);
        }
        assert_eq!(SLOT_TABLE.len(), SLOTS);
    }

    /// `SetUpStatus` writes `ab1`, `ab3`, `ab2` into `Inc[0]`, `[1]`, `[2]`, so
    /// the second row of the sheet is constitution and the third endurance.
    /// That order is not alphabetical, not the order `ab1`..`ab3` sit in, and
    /// not the one a guess would pick.
    #[test]
    fn the_ability_rows_are_strength_constitution_endurance() {
        assert_eq!(INC[0], "Increase Strength");
        assert_eq!(INC[1], "Increase Constitution");
        assert_eq!(INC[2], "Increase Endurance");
        assert_eq!(SLOT_TABLE[0].name, "Strength");
        assert_eq!(SLOT_TABLE[1].name, "Constitution");
        assert_eq!(SLOT_TABLE[2].name, "Endurance");
    }

    /// `Use` is `Inc` with the three abilities named rather than offered, and
    /// nothing else differs.
    #[test]
    fn use_differs_from_inc_in_exactly_three_slots() {
        let differ: Vec<usize> = (0..SLOTS).filter(|&i| USE[i] != INC[i]).collect();
        assert_eq!(differ, vec![0, 1, 2]);
    }

    /// Slot 23 is the hawk and 24 aquisition in every one of the five arrays
    /// that name both. `SetUpStatus` writes the pair out of order on purpose,
    /// `si + 0x30` before `si + 0x2e`, five separate times.
    #[test]
    fn the_hawk_comes_before_aquisition_in_every_array() {
        for t in [
            Table::Inc,
            Table::Use,
            Table::Take,
            Table::Purchase,
            Table::Offer,
            Table::Sell,
        ] {
            let hawk = t.line(23).unwrap();
            let seize = t.line(24).unwrap();
            assert!(hawk.contains("Hawk"), "{t:?} slot 23 is {hawk}");
            assert!(seize.contains("Aquisition"), "{t:?} slot 24 is {seize}");
        }
        assert_eq!(SLOT_TABLE[23].item, Some("scroll_of_the_hawk"));
        assert_eq!(SLOT_TABLE[24].item, Some("scroll_of_acquisition"));
    }

    /// The seven slots `Purchase` leaves empty are what a knight starts with.
    #[test]
    fn a_stall_sells_nothing_a_knight_already_has() {
        let empty: Vec<usize> = (0..SLOTS).filter(|&i| PURCHASE[i].is_empty()).collect();
        assert_eq!(empty, vec![0, 1, 2, 3, 4, 6, 10]);
    }

    /// `Offer`'s fourteen cleared slots hold `Blank`, a single space, not
    /// `NULL_STR`. The difference is the whole of why the temple's panel says
    /// nothing over a sword instead of saying the sword's name.
    #[test]
    fn the_temple_blanks_rather_than_clears() {
        for (i, line) in OFFER.iter().enumerate().take(14) {
            assert_eq!(*line, " ");
            assert_eq!(Table::Offer.line(i), Some(" "));
        }
        assert_eq!(Table::Sell.line(0), None, "Sell really is NULL_STR there");
    }

    /// Every price `Sell` names is half the one `Purchase` asks, which is
    /// `GoldSell`'s `shr ax, 1` -- except the scroll of Haste, whose line is
    /// wrong in the original by two. Pinned as it stands so nobody "fixes" the
    /// string into agreement with the arithmetic.
    #[test]
    fn selling_pays_half_of_buying_but_for_the_hastes_own_typo() {
        fn figure(line: &str) -> Option<u32> {
            let rest = line.strip_suffix(" GP")?;
            rest.rsplit(' ').next()?.parse().ok()
        }
        let mut checked = 0;
        for i in 0..SLOTS {
            let (Some(buy), Some(sell)) = (figure(PURCHASE[i]), figure(SELL[i])) else {
                continue;
            };
            if i == 22 {
                assert_eq!((buy, sell), (36, 16), "pu13 and se13, verbatim");
                assert_eq!(price_of(22).unwrap() / 2, 18, "what GoldSell would pay");
                checked += 1;
                continue;
            }
            assert_eq!(sell, buy / 2, "slot {i}");
            checked += 1;
        }
        assert_eq!(
            checked, 14,
            "fourteen slots carry both a price and a payout"
        );
    }

    /// `MagicPrices` agrees with the lines, every entry.
    #[test]
    fn magic_prices_agree_with_the_lines_that_quote_them() {
        for (slot, price) in MAGIC_PRICES {
            assert_eq!(price_of(slot), Some(price));
            let line = PURCHASE[slot];
            if line.is_empty() {
                continue;
            }
            assert!(
                line.contains(&format!("for {price} GP")),
                "slot {slot}: {line} against {price}"
            );
        }
        // The three moonstone slots share `pu18`'s figure.
        for slot in 14..=16 {
            assert_eq!(price_of(slot), Some(20));
        }
    }

    /// `HotGadget`'s nibble decode, and the key masks that ride in the other
    /// nibble.
    #[test]
    fn a_payload_decodes_the_way_hotgadget_does() {
        assert_eq!(Payload::new(5, 0).op(), Some(Op::Cast));
        assert_eq!(Payload::new(1, 0).op(), Some(Op::TakeMagic));
        assert_eq!(Payload::new(3, 0).op(), Some(Op::Raise));
        assert_eq!(Payload::new(0xa, 0).op(), Some(Op::Buy));
        assert_eq!(Payload::new(2, 0).op(), Some(Op::Take));
        // `StatCheckKeys` gives the four keys one operation and four masks.
        for (strp, bit) in [(0x11, 1), (0x21, 2), (0x41, 4), (0x81, 8)] {
            let p = Payload::new(strp, 0x14);
            assert_eq!(p.op(), Some(Op::TakeMagic));
            assert_eq!(p.mask(), bit);
        }
        // `DisplayMerchant`'s three armours: one buy, three masks.
        for (strp, bit) in [(0x1a, 1), (0x2a, 2), (0x4a, 4)] {
            let p = Payload::new(strp, 0x42);
            assert_eq!(p.op(), Some(Op::Buy));
            assert_eq!(p.mask(), bit);
        }
    }

    /// `STID` is twice the slot, which is what makes `AddIconGadget`'s
    /// `shr ax, 1` land on the right text record.
    #[test]
    fn a_gadget_id_is_twice_its_slot() {
        assert_eq!(slot_of(0), Some(0));
        assert_eq!(slot_of(0x34), Some(26), "the last scroll");
        assert_eq!(slot_of(0x36), None, "past the end of the arrays");
        assert_eq!(slot_of(7), None, "the exit is not a slot");
    }

    /// `DisplayPillars` picks `SingleData` alone for the two types in
    /// `OffsetValues` and both tables for everything else.
    #[test]
    fn only_the_sheet_and_the_stones_are_one_arch() {
        assert!(Screen::Sheet.single_arch());
        assert!(Screen::Henge.single_arch());
        for s in [
            Screen::Trade,
            Screen::Lair,
            Screen::Merchant,
            Screen::Temple,
            Screen::Acquire,
            Screen::Dragon,
        ] {
            assert!(!s.single_arch(), "{s:?}");
        }
    }

    /// `SetUpID` and `Paper2`, screen by screen.
    #[test]
    fn each_screen_reads_the_arrays_setupid_gives_it() {
        assert_eq!(Screen::Sheet.left_table(), Table::Use);
        assert_eq!(Screen::Henge.left_table(), Table::Offer);
        assert_eq!(Screen::Temple.left_table(), Table::Sell);
        assert_eq!(Screen::Lair.left_table(), Table::Identify);
        assert_eq!(Screen::Merchant.left_table(), Table::Identify);
        assert_eq!(Screen::Merchant.right_table(false), Table::Purchase);
        assert_eq!(Screen::Temple.right_table(false), Table::Purchase);
        assert_eq!(Screen::Trade.right_table(false), Table::Take);
        assert_eq!(Screen::Lair.right_table(false), Table::Take);
        assert_eq!(
            Screen::Lair.right_table(true),
            Table::Identify,
            "a lair seen through the gem is looked at, not emptied"
        );
        assert!(Table::Use.acts());
        assert!(
            !Table::Identify.acts(),
            "Identify carries no permission bit"
        );
    }

    /// The donation is built a coin at a time and refuses at both ends.
    #[test]
    fn the_donation_moves_one_coin_at_a_time() {
        let mut d = Donation::open(3);
        for _ in 0..5 {
            assert!(!d.act(DonateOp::More));
        }
        assert_eq!(
            (d.purse, d.given),
            (0, 3),
            "AddDonation stops at an empty purse"
        );
        assert!(!d.act(DonateOp::Less));
        assert_eq!((d.purse, d.given), (1, 2));
        for _ in 0..5 {
            d.act(DonateOp::Less);
        }
        assert_eq!(
            (d.purse, d.given),
            (3, 0),
            "and SubDonation at an empty bowl"
        );
    }

    /// `OkDonation` keeps what is left; `ExitDonation` gives it all back.
    #[test]
    fn leaving_the_panel_either_way_does_what_the_routine_does() {
        let mut d = Donation::open(50);
        for _ in 0..25 {
            d.act(DonateOp::More);
        }
        assert!(d.act(DonateOp::Ok));
        assert_eq!(d.closing_gold(DonateOp::Ok), 25);
        assert_eq!(d.closing_gold(DonateOp::Exit), 50);
    }

    /// The record's five scroll fields are slots 22 to 26 in order, because
    /// `DisplayMagic` starts `STID` at 0x2c and steps it by two per field.
    #[test]
    fn the_hoard_reads_its_own_fields() {
        let mut h = Hoard {
            potions: 2,
            gems: 1,
            magic_sword: true,
            rings: 1,
            talismans: 3,
            scrolls: [1, 2, 3, 4, 5],
            keys: 0xf,
            moonstones: 2,
        };
        assert_eq!(h.count(18), Some(2));
        assert_eq!(h.count(19), Some(1));
        assert_eq!(h.count(13), Some(1));
        assert_eq!(h.count(20), Some(1));
        assert_eq!(h.count(21), Some(3));
        assert_eq!(SCROLL_SLOTS, [22, 23, 24, 25, 26]);
        for (n, slot) in SCROLL_SLOTS.iter().enumerate() {
            assert_eq!(h.count(*slot), Some(n as u8 + 1));
        }
        assert_eq!(h.count(4), None, "the purse is not one of its fields");
        h.set(22, 9);
        assert_eq!(h.scrolls[0], 9);
    }

    /// The pack ids line up with the slots, and `slot_for_item` is their
    /// inverse.
    #[test]
    fn every_slot_with_an_item_round_trips() {
        for (i, s) in SLOT_TABLE.iter().enumerate() {
            if let Some(id) = s.item {
                assert_eq!(slot_for_item(id), Some(i), "{id}");
            }
        }
        assert_eq!(slot_for_item("nothing_like_it"), None);
    }

    /// A pack of goods fills the record's own fields, bit order included.
    #[test]
    fn a_pack_fills_the_record() {
        let mut kit = crate::item::Inventory::new(40);
        kit.take("potion", 3);
        kit.take("gem_of_seeing", 1);
        kit.take("scroll_of_the_hawk", 2);
        kit.take("key.glade", 1);
        kit.take("moonstone.full", 1);
        let h = Hoard::of(&kit, true);
        assert_eq!(h.potions, 3);
        assert_eq!(h.gems, 1);
        assert!(h.magic_sword);
        assert_eq!(h.scrolls, [0, 2, 0, 0, 0], "the hawk is the second field");
        // `Key::Glade` is bit 1, which `StatCheckKeys` draws as cel 5 at the
        // leftmost of the four slots.
        assert_eq!(h.keys, 1);
        assert_eq!(h.moonstones, 2, "Moonstone::Full is bit 2");
        assert_eq!(
            Hoard::of(&crate::item::Inventory::new(8), false),
            Hoard::default()
        );
    }
}
