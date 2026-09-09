//! Lairs: twenty four dangerous places, a guardian in each, and the four keys.
//!
//! A lair is the only place on the map worth going to for what is in it rather
//! than for what is sold there, and it is where the quest begins: one key is
//! hidden in one lair of each terrain family, and four keys open the Valley.
//!
//! **What is recovered**, from `MOON` and `_MAP`. The table is 24 records of
//! 18 bytes at DS:0002, built by the initialiser at image 0x1e00:
//!
//! ```text
//! +0x00  the lair's own item record, 24 bytes of counts at fmem_LairMagic
//! +0x02  which entry of CombatTable sets up the guardian fight
//! +0x04  how many of them: TotalMonsters going in, what is left coming out
//! +0x06  gold
//! +0x08  set to 1 the first time the guardian is beaten (LairWon)
//! +0x0a  x on the map, 0xffff once the lair is stripped bare
//! +0x0c  y
//! +0x0e  the landscape code, which picks the arena family
//! +0x10  the arena layout: fol1..6, wal1..6, swl1..6, gll1..6
//! ```
//!
//! **The keys.** The initialiser calls `NUM16` four times, which is `rnd & 7`
//! rolled again while it exceeds five, and writes 8, 4, 2 and 1 into byte
//! `+0x14` of the chosen lair's item record, stepping 0x6c (six records) on
//! between each. So each family of six hides exactly one key, in a lair chosen
//! uniformly at the start of the quest.
//!
//! **The contents.** `LairFill` rolls 0..=100 against `MOON:LairRND`, four
//! records of a threshold and a kind: 50 gold, 70 magic, 90 both, 100 both
//! again, because the dispatcher's last comparison is dead code and falls
//! through to the same call. Gold is the wizard's own gift routine called with
//! `dx` set, which puts ten to thirty one in the lair; magic is the wizard's
//! own bestowal called twice, so a lair with magic holds two items.
//!
//! **Arriving.** `MOON:CheckLairEncounter` overlaps the traveller's 8x10 token
//! with icon frame 0x1f of `MI.C`, which is 9x5, exactly the way a town is
//! decided, and puts `Enter Lair` on the map's own list. `_MAP:DisplayLairs`
//! draws frame 0x14 at every lair whose x is not negative, so lairs are on the
//! map from the start. A stripped one leaves it: `MOON:CheckLairClear` writes
//! 0xffff over the coordinates when the gold is zero *and* all 24 item counts
//! are zero, so a lair you have beaten but not emptied is still there to go
//! back to.
//!
//! **Leaving.** `MOON:LairWon` sets the cleared flag and adds one to the
//! knight's experience the first time only, then opens the status panel's lair
//! page for the taking.
//!
//! **Where they are, and what is in them, is recovered.** All four of the
//! tables the initialiser copies from are readable, and the baker reads them:
//! `ForestLairs` is 24 pairs of words at DS:0a6a, a `CombatTable` byte offset
//! and a head count; `LairLocation` is 24 pairs at DS:0aca, the corner the map
//! draws the lair at; `LairType` is 24 words at DS:0b2a, the landscape the
//! fight happens on; and `LairFile` is the 24 arena layouts. The copy loop at
//! image 0x1ea0 is what says which is which, field by field. What the guardian
//! can be is recovered too, from `InitGameStart` filling `CombatTable` with
//! the thirteen `InitKnightvs*` routines.
//!
//! Twenty three of the twenty four coordinates land on a cell of `MapType`
//! whose code is that lair's own `LairType`, which is two tables agreeing that
//! were read out of different places; the odd one, lair 15, stands a cell into
//! the treeline and is still fought in the marsh, because `InitLair` hands
//! `ColourBackdrop` the record's landscape and never asks the map.
//!
//! `ForestLairs`'s head count is `TotalMonsters`, everything that comes at you
//! before a lair is clear, and it runs from three to fourteen. **The waves are
//! built**, and they are [`crate::wave`]: `AdjustLevel` (image `0x2824`) writes
//! this number over `TotalMonsters` at `0x287e` and then takes the creature's
//! own row of `lev_adjust` off it, `SetMonsterCombat` (`0x27e4`) stands
//! `MaxMonsters` of them up, which is one for everything but the ratmen, and
//! `CountTheDead` (`0x213`) sends the next one in on the frame the last one's
//! death script reaches its `TASKGOSUB`. So a lair of fourteen is fourteen
//! fights one after another, not a crowd, and the pack carries the number
//! unrounded because the arena no longer has to hold it all at once.
//!
//! **The gem's own flight.**
//! `INITGEM` at image 0xa937 and `RESTOREGEM` at 0xa950, which are the two
//! halves of a gem flight:
//!
//! ```text
//! INITGEM     a937  mov word ptr [EffectFLAG+4], 1
//!             a93d  mov si, [JOYSTICK1+6]
//!             a941  push [si+0x5c]; pop [GemXY]      ; where you were
//!             a948  push [si+0x5e]; pop [GemXY+2]
//! RESTOREGEM  a950  mov si, [JOYSTICK1+6]
//!             a954  push [GemXY];   pop [si+0x5c]    ; and where you are again
//!             a95b  push [GemXY+2]; pop [si+0x5e]
//!             a962  mov word ptr [EffectFLAG+4], 0
//!             a968  mov word ptr [EffectFLAG+2], 0
//!             a96e  mov word ptr [EffectFLAG+6], 0
//! ```
//!
//! **`RESTOREGEM` has exactly one caller**, `LairGEM+19`, so a gem flight ends
//! nowhere but at a lair. Fire on the map enters at `0xa962` instead
//! (`_MAP:ScrollINPUT+71`, and only when `EffectFLAG+2` is up), which clears
//! the three flags without restoring the position: that is the hawk landing
//! where it is, and it is why the gem does not.

use crate::item::Items;
use crate::moon::Key;
use crate::run::Run;
use crate::service::{gold_from, magic_item};
use crate::status::Hoard;
use serde::{Deserialize, Serialize};

/// How many lairs there are. `mov cx, 0x18` in the initialiser,
/// `CheckLairEncounter` and `DisplayLairs`.
pub const LAIRS: usize = 24;

/// How many each family holds: the initialiser steps 0x6c bytes, six records,
/// between one key and the next.
pub const PER_FAMILY: usize = 6;

/// How many magic items a lair with magic in it holds: `MagicLair` calls the
/// bestowal twice.
pub const MAGIC_PER_LAIR: usize = 2;

/// What a lair holds, by the roll `LairFill` takes: `MOON:LairRND`, four
/// records of a threshold and a kind, walked for the first threshold the roll
/// does not exceed. 1 is gold, 2 magic, 3 and 4 both.
pub const CONTENTS: [(u32, u8); 4] = [(50, 1), (70, 2), (90, 3), (100, 4)];

/// What is actually in one lair on this run.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Lair {
    /// `+0x06`. Ten to thirty one, or none.
    pub gold: u32,
    /// The item ids on the floor. Two, or none, less whatever was carried out.
    pub magic: Vec<String>,
    /// The key hidden here, if this is the family's one.
    pub key: Option<Key>,
    /// `+0x08`. Whether the guardian has been beaten.
    pub cleared: bool,
}

impl Lair {
    /// Whether anything is left to carry out. `CheckLairClear` asks exactly
    /// this: no gold, and every one of the 24 item counts zero.
    pub fn empty(&self) -> bool {
        self.gold == 0 && self.magic.is_empty() && self.key.is_none()
    }
}

/// What came of walking in. `MOON` image 0x0581: the gem's flag or the
/// guardian, and nothing else, because a lair has no third answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Raid {
    /// The guardian is waiting. The caller starts the bout; the floor is only
    /// yours once it is won.
    Guardian,
    /// Beaten already, so `0x0574` runs straight on to `LairGEM` and the page
    /// goes up over the floor. See [`Page`].
    Floor,
}

impl Run {
    /// Stock every lair, once, at the start of a quest.
    ///
    /// `families` is the terrain family of each lair in table order, which is
    /// what decides where the keys go: the initialiser plants them six records
    /// apart, so lair `i` of family `f` is the one at index `f * 6 + i`. The
    /// rolls are taken in the initialiser's order, the four keys first and then
    /// the 24 fills, so two machines with the same seed lay the same board.
    ///
    /// Magic that the pack does not declare is left out rather than put on a
    /// floor nobody can pick it up from: a lair is stocked from what exists.
    pub fn stock_lairs(&mut self, families: &[String], items: &Items) {
        self.lairs = vec![Lair::default(); families.len()];
        // `call NUM16; mul 0x12; ... mov byte ptr [di+0x14], 8`, then
        // `add si, 0x6c` and again, for 8, 4, 2 and 1.
        for key in Key::ALL {
            let mine: Vec<usize> = (0..families.len())
                .filter(|i| families[*i] == key.family())
                .collect();
            if mine.is_empty() {
                continue;
            }
            let pick = self.num16() as usize % mine.len();
            self.lairs[mine[pick]].key = Some(key);
        }
        for i in 0..self.lairs.len() {
            self.fill_lair(i, items);
        }
    }

    /// `MOON:NUM16`: `rnd & 7`, rolled again while it is over five. Bounded
    /// here, because a simulation two machines keep in step must not be able
    /// to hang; eight refusals in a row is one chance in sixty five thousand.
    fn num16(&mut self) -> u32 {
        for _ in 0..8 {
            let n = self.next_roll() & 7;
            if n <= 5 {
                return n;
            }
        }
        self.next_roll() % 6
    }

    /// `LairFill`, for one lair.
    fn fill_lair(&mut self, index: usize, items: &Items) {
        let roll = self.roll(101);
        let kind = CONTENTS
            .iter()
            .find(|(threshold, _)| roll <= *threshold)
            .map_or(4, |(_, kind)| *kind);
        if kind != 2 {
            let gold = gold_from(self.next_roll());
            if let Some(lair) = self.lairs.get_mut(index) {
                lair.gold += gold;
            }
        }
        if kind != 1 {
            for _ in 0..MAGIC_PER_LAIR {
                let Some(slot) = self.known_magic_slot(items) else {
                    continue;
                };
                let Some(id) = magic_item(slot) else { continue };
                if let Some(lair) = self.lairs.get_mut(index) {
                    lair.magic.push(id.to_string());
                }
            }
        }
    }

    /// Whether a lair is still on the map: beaten *and* stripped takes it off,
    /// which is `CheckLairClear` writing 0xffff over its coordinates. A lair
    /// the run has not stocked is on the map, so a pack works before a quest
    /// has begun.
    pub fn lair_on_the_map(&self, index: usize) -> bool {
        self.lairs
            .get(index)
            .is_none_or(|l| !(l.cleared && l.empty()))
    }

    /// Walk in. `MOON` image 0x0581 tests the lair's `+8` through `LairWon`
    /// and `0x0586` the gem's flag: either the guardian is up, or the page
    /// goes over the floor. Nothing is carried out here, because in the
    /// original nothing is: the floor is handed over a gadget at a time on the
    /// page itself.
    pub fn raid(&mut self, index: usize) -> Raid {
        match self.lairs.get(index) {
            Some(lair) if !lair.cleared => Raid::Guardian,
            _ => Raid::Floor,
        }
    }

    /// Which keys the run carries, as `Valley` reads them: all four bits set
    /// is `0xf`, and the Valley wants all four.
    pub fn keys_held(&self) -> Vec<Key> {
        Key::ALL
            .into_iter()
            .filter(|k| self.kit.count(k.item()) > 0)
            .collect()
    }

    /// The guardian is down, and nothing has been carried out yet.
    ///
    /// `MOON:LairWon` at image 0x05ac, which is the whole of it:
    ///
    /// ```text
    /// 0x05ac  mov di, [0x6962]          ; the lair record the entry stored
    /// 0x05b0  cmp word ptr [di+8], 0    ; beaten before?
    /// 0x05b4  jne LairGEM               ; then it is worth nothing more
    /// 0x05b6  mov word ptr [di+8], 1
    /// 0x05bb  mov di, [JOYSTICK1+6]
    /// 0x05bf  add word ptr [di+0x36], 1 ; and one point of experience
    /// ```
    ///
    /// and then it falls into `LairGEM`, which opens the page. Nothing is
    /// swept off the floor here: the floor is handed over a gadget at a time
    /// by [`Run::take_from_lair`] and [`Run::take_lair_gold`].
    pub fn lair_beaten(&mut self, index: usize) -> Page {
        let first = match self.lairs.get_mut(index) {
            Some(lair) if !lair.cleared => {
                lair.cleared = true;
                true
            }
            _ => false,
        };
        if first {
            // `add word ptr [di+0x36], 1`, the same field a won bout pays into.
            self.earned_experience(1);
        }
        Page {
            lair: index,
            scouted: false,
        }
    }

    /// What the right arch draws: the floor as a record of counts, and the
    /// pile of coin beside it.
    ///
    /// `ReDisplay` hands `DisplayLair` the lair's own 24-byte item record at
    /// `fmem_LairMagic` and `DisplayGold` the word at `+6`, so this is that
    /// record built out of what the run keeps on the floor instead.
    pub fn lair_floor(&self, index: usize) -> (Hoard, u32) {
        let Some(lair) = self.lairs.get(index) else {
            return (Hoard::default(), 0);
        };
        let mut floor = crate::item::Inventory::new(u32::MAX);
        for id in &lair.magic {
            floor.take(id, 1);
        }
        if let Some(key) = lair.key {
            floor.take(key.item(), 1);
        }
        let sword = floor.count(MAGIC_SWORD) > 0;
        (Hoard::of(&floor, sword), lair.gold)
    }

    /// One gadget on the lair page, pressed. `_STATUS:HGTakeMagic` at 0xcba2:
    ///
    /// ```text
    /// 0xcba2  si = StatMAGIC1           ; the knight's record
    /// 0xcba6  di = StatMAGIC2           ; the floor's
    /// 0xcbb4  test ax, 0x20; je ret     ; the right arch's permission bit,
    ///                                   ; which `Identify` does not carry
    /// 0xcbbc  cmp bx, 0x16; je HGTakeMoonstone
    /// 0xcbc1  cmp bx, 0x14; je HGTakeMoonstone   ; the keys, a whole field
    /// 0xcbc6  cmp bx, 4;    je TakeSword
    /// 0xcbce  dec byte ptr [bx+di]      ; otherwise one off the floor
    /// 0xcbd0  inc byte ptr [bx+si]      ; and one onto the knight
    /// 0xcbd2  cmp bx, 6; jne            ; a ring, and only a ring, is
    /// 0xcbd7  add word ptr [si+0x38], 0x14 ; twenty points of health at once
    /// 0xcbdf  call 0x28d                ; and the maximum recomputed
    /// ```
    ///
    /// `field` is the gadget's `STPL`, which is the byte offset into the
    /// record it moves. Returns whether anything moved, which is what
    /// `TakeCNT` counts.
    ///
    /// **Ours:** a pack that can be full. The original's knight record has a
    /// fixed field per kind and cannot refuse, and this one has
    /// [`crate::item::Inventory::capacity`], so a floor with no room to go to
    /// stays a floor.
    pub fn take_from_lair(&mut self, index: usize, field: u16, items: &Items) -> bool {
        // `cmp bx, 0x16; je HGTakeMoonstone` and `cmp bx, 0x14; je` are the
        // first two tests the routine makes, and both go to the arm that moves
        // a whole bit field rather than one count. A lair's floor never holds
        // a moonstone (`LairFill` puts gold and magic on it and nothing else),
        // so of the two only the keys ever have anything to move.
        if field == KEYS_FIELD || field == STONES_FIELD {
            return field == KEYS_FIELD && self.take_lair_key(index, items);
        }
        let Some(slot) = slot_of_field(field) else {
            return false;
        };
        let Some(id) = crate::status::SLOT_TABLE[slot].item else {
            return false;
        };
        let Some(lair) = self.lairs.get(index) else {
            return false;
        };
        let Some(at) = lair.magic.iter().position(|held| held == id) else {
            return false;
        };
        if self.kit.take(id, 1) == 0 {
            return false;
        }
        self.lairs[index].magic.remove(at);
        // `cmp bx, 6`: the ring is the one field that pays as it is picked up.
        self.refresh(items);
        true
    }

    /// The key, which `HGTakeMoonstone` moves as a whole bit field.
    fn take_lair_key(&mut self, index: usize, items: &Items) -> bool {
        let Some(key) = self.lairs.get(index).and_then(|l| l.key) else {
            return false;
        };
        if self.kit.take(key.item(), 1) == 0 {
            return false;
        }
        self.lairs[index].key = None;
        self.refresh(items);
        true
    }

    /// The pile of coin, pressed. `_STATUS:TKGP` at 0xccfa, which is the only
    /// place in the program that knows a lair page has a floor of its own:
    ///
    /// ```text
    /// 0xccfd  cmp word ptr [StatTYPE], 2   ; the lair page
    /// 0xcd02  jne 0xcd12
    /// 0xcd04  mov di, [0x6962]             ; the lair record
    /// 0xcd08  lea di, [di+6]               ; and its gold
    /// 0xcd0b  cmp word ptr [di], 0; je out
    /// 0xcd1a  cmp word ptr [si+0x32], 0x96 ; a purse of a hundred and fifty
    /// 0xcd1f  je  0xcd2b                   ; is as much as anyone carries
    /// 0xcd21  dec word ptr [di]            ; one coin
    /// 0xcd23  inc word ptr [si+0x32]
    /// 0xcd26  cmp word ptr [di], 0; jne 0xcd1a
    /// ```
    ///
    /// So the whole pile goes over a coin at a time and stops dead at the
    /// ceiling, and what is over the ceiling stays on the floor. Returns how
    /// many coins moved.
    pub fn take_lair_gold(&mut self, index: usize) -> u32 {
        let Some(lair) = self.lairs.get(index) else {
            return 0;
        };
        let mut floor = lair.gold;
        let mut moved = 0;
        while floor > 0 && self.gold < crate::service::PURSE_CEILING {
            floor -= 1;
            self.gold += 1;
            moved += 1;
        }
        self.lairs[index].gold = floor;
        moved
    }
}

/// The lair page, which is where a lair's floor is actually handed over.
///
/// **The whole of it is one routine**, `MOON` image 0x0574 to 0x05dc, which
/// the map reaches through `_MAP:StackDecision+41` when the thing under the
/// token is of type 2:
///
/// ```text
/// 0x0574  mov word ptr [dragonbodge3], 0
/// 0x057a  mov [0x6962], di                  ; the lair record
/// 0x057e  call 0xa554                       ; the map's own glows taken down
/// 0x0581  cmp word ptr [EffectFLAG+4], 0    ; INITGEM's flag
/// 0x0586  jne LairGEM                       ; aloft: look, and do not fight
/// 0x0588  call ClearCombat
/// 0x058b  call InitLair
/// 0x058e  call InitCombat                   ; the guardian
/// 0x0597  test word ptr [KnightDeath], 1
/// 0x059d  je LairWon
/// 0x059f  mov ax, 9; call the panel         ; a death opens the sheet instead
/// 0x05a8  mov ax, 1; ret
/// LairWon 0x05ac                            ; see `Run::lair_beaten`
/// LairGEM 0x05c3
/// 0x05c3  mov ax, 2                         ; StatTYPE 2, and `Screen::Lair`
/// 0x05c6  call the panel                    ; which runs until `ExitFLAG`
/// 0x05c9  call CheckLairClear               ; empty and beaten leaves the map
/// 0x05cf  cmp word ptr [EffectFLAG+4], 0
/// 0x05d4  je 0x05d9
/// 0x05d6  call 0xa950                       ; RESTOREGEM, below
/// 0x05d9  mov ax, 1; ret
/// ```
///
/// So **the page a gem flight ends on and the page a won lair opens are the
/// same page**, reached by the same three instructions, and the only
/// difference between them is `EffectFLAG+4`: `_STATUS:Paper2` at 0xd3ed reads
/// that flag and gives the right arch `Identify` instead of `Take`, so a lair
/// seen from the air can be read and not emptied.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Page {
    /// Which lair. `[0x6962]`, the record `0x057a` stored.
    pub lair: usize,
    /// `EffectFLAG+4`, the gem's own flag.
    pub scouted: bool,
}

/// `StatCheckKeys`' own field, `STPL` 0x16: the moonstones as bits, which
/// `HGTakeMoonstone` moves whole.
pub const STONES_FIELD: u16 = 0x16;

/// `StatCheckKeys`' own field, `STPL` 0x14: the four keys as bits.
pub const KEYS_FIELD: u16 = 0x14;

/// The pack id of the one thing the record keeps that is also worn, which
/// `DisplayMSword` draws and `TakeSword` moves.
pub const MAGIC_SWORD: &str = "sword_of_sharpness";

/// Which slot of the panel a gadget's `STPL` works on.
///
/// The offsets are the ones `DisplayMagic`, `DisplayMSword` and
/// `StatPlaceScroll` write into their gadgets, and they are the fields of the
/// 24-byte record in the order [`Hoard`] carries them.
// Hand-aligned: the record's own byte offset, then the panel slot it draws at.
#[rustfmt::skip]
pub const FIELD_SLOTS: [(u16, usize); 10] = [
    (0x00, 18),  // potions
    (0x02, 19),  // gems
    (0x04, 13),  // the magic sword
    (0x06, 20),  // rings
    (0x08, 21),  // talismans
    (0x0a, 22),  // haste
    (0x0c, 23),  // hawk
    (0x0e, 24),  // aquisition
    (0x10, 25),  // wyrm
    (0x12, 26),  // protection
];

/// The slot a `STPL` names, or nothing for a field that is not one of them.
pub fn slot_of_field(field: u16) -> Option<usize> {
    FIELD_SLOTS
        .iter()
        .find(|(f, _)| *f == field)
        .map(|(_, slot)| *slot)
}

/// The same table read the other way: which gadget on the page carries a
/// slot. The keys are slot 17 and their field is [`KEYS_FIELD`], because
/// `StatCheckKeys` gives all four the one field and tells them apart by the
/// bit in `STRP`'s high nibble.
pub fn field_of_slot(slot: usize) -> Option<u16> {
    if slot == 17 {
        return Some(KEYS_FIELD);
    }
    FIELD_SLOTS
        .iter()
        .find(|(_, s)| *s == slot)
        .map(|(f, _)| *f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{ItemDef, Virtue};
    use crate::knight::KnightDef;

    fn goods() -> Items {
        let mut items = Items::new();
        let mut add = |id: &str, virtue: Virtue| {
            items.insert(
                id.into(),
                ItemDef {
                    name: id.into(),
                    price: 0,
                    virtue,
                    consumed: false,
                },
            );
        };
        for id in [
            "potion",
            "gem_of_seeing",
            "ring_of_protection",
            "talisman",
            "scroll_of_haste",
        ] {
            add(id, Virtue::Inert);
        }
        add("sword_of_sharpness", Virtue::Weapon { damage: 5 });
        add("long_sword", Virtue::Weapon { damage: 0 });
        add(
            "padded_armour",
            Virtue::Armour {
                health: 0,
                stride: 0,
            },
        );
        for k in Key::ALL {
            add(k.item(), Virtue::Inert);
        }
        items
    }

    fn families() -> Vec<String> {
        ["forest", "waste", "swamp", "glade"]
            .iter()
            .flat_map(|f| std::iter::repeat_n(f.to_string(), PER_FAMILY))
            .collect()
    }

    fn run(seed: u32) -> (Run, Items) {
        let items = goods();
        let def = KnightDef {
            name: "SIR GODBER".into(),
            shades: vec![0],
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
        r.reseed(seed);
        r.kit.capacity = 200;
        r.stock_lairs(&families(), &items);
        (r, items)
    }

    #[test]
    fn there_are_twenty_four_lairs_six_to_a_family() {
        let (r, _) = run(1);
        assert_eq!(families().len(), LAIRS);
        assert_eq!(r.lairs.len(), LAIRS);
    }

    /// One key per family, in a lair of that family, and never two in one.
    #[test]
    fn each_family_hides_exactly_one_key() {
        let fams = families();
        for seed in 1..60u32 {
            let (r, _) = run(seed.wrapping_mul(2_654_435_761) | 1);
            for key in Key::ALL {
                let here: Vec<usize> = (0..LAIRS)
                    .filter(|n| r.lairs[*n].key == Some(key))
                    .collect();
                assert_eq!(here.len(), 1, "{key:?} at seed {seed}: {here:?}");
                assert_eq!(
                    fams[here[0]],
                    key.family(),
                    "and in its own family's ground"
                );
            }
        }
    }

    /// And which lair it is in moves with the seed, or the quest would be the
    /// same walk every time.
    #[test]
    fn where_a_key_is_depends_on_the_seed() {
        let mut seen = std::collections::BTreeSet::new();
        for seed in 1..80u32 {
            let (r, _) = run(seed.wrapping_mul(0x9e37_79b9) | 1);
            seen.insert(
                r.lairs[..PER_FAMILY]
                    .iter()
                    .position(|l| l.key.is_some())
                    .unwrap(),
            );
        }
        assert!(seen.len() >= 5, "the forest key moves about: {seen:?}");
    }

    /// `LairRND`: half gold, a fifth magic, the rest both. Nothing is empty,
    /// because the fourth kind falls through to the same call as the third.
    #[test]
    fn every_lair_holds_something() {
        let (mut only_gold, mut only_magic, mut both) = (0, 0, 0);
        for seed in 1..40u32 {
            let (r, _) = run(seed.wrapping_mul(48271) | 1);
            for lair in &r.lairs {
                assert!(!lair.empty(), "a lair with nothing in it");
                match (lair.gold > 0, !lair.magic.is_empty()) {
                    (true, false) => only_gold += 1,
                    (false, true) => only_magic += 1,
                    (true, true) => both += 1,
                    (false, false) => panic!("neither, and no key either"),
                }
                if !lair.magic.is_empty() {
                    assert_eq!(lair.magic.len(), MAGIC_PER_LAIR, "two items, or none");
                }
                if lair.gold > 0 {
                    assert!((10..=31).contains(&lair.gold), "{}", lair.gold);
                }
            }
        }
        assert!(
            only_gold > both && both > only_magic,
            "{only_gold} {both} {only_magic}"
        );
    }

    /// A lair is stocked from what the pack declares. A slot the pack has no
    /// item for is left off the floor rather than put there as an id nobody
    /// can pick up.
    #[test]
    fn a_lair_only_holds_what_the_pack_knows() {
        let mut items = goods();
        items.retain(|id, _| {
            id == "potion"
                || id.starts_with("key.")
                || id.ends_with("sword")
                || id.ends_with("armour")
        });
        let mut r = run(9).0;
        r.stock_lairs(&families(), &items);
        for lair in &r.lairs {
            assert!(
                lair.magic.iter().all(|id| id == "potion"),
                "{:?}",
                lair.magic
            );
        }
    }

    /// What a lair's floor hands over when a gadget on the page is pressed.
    /// `Run::take_from_lair` wants the `STPL` the gadget carries, and the
    /// panel builds that off the slot, so a test reaches it the same way.
    fn take(r: &mut Run, at: usize, id: &str, items: &Items) -> bool {
        let slot = crate::status::slot_for_item(id).expect(id);
        let field = field_of_slot(slot).expect(id);
        r.take_from_lair(at, field, items)
    }

    /// Everything on one floor, gadget by gadget, which is the only way
    /// anything comes off it.
    fn empty_it(r: &mut Run, at: usize, items: &Items) {
        r.take_lair_gold(at);
        if r.lairs[at].key.is_some() {
            r.take_from_lair(at, KEYS_FIELD, items);
        }
        for id in r.lairs[at].magic.clone() {
            take(r, at, &id, items);
        }
    }

    /// `MOON` 0x0581 and `LairWon` at 0x05ac: walking in fights, and beating
    /// the guardian marks the lair and pays one point of experience, once.
    /// **Nothing at all comes off the floor**, which is the whole difference
    /// between the page and the sweep that used to stand here.
    #[test]
    fn the_guardian_is_up_until_it_is_beaten_and_the_floor_is_untouched() {
        let (mut r, _items) = run(7);
        let before = r.gold;
        assert_eq!(r.raid(0), Raid::Guardian);
        assert_eq!(r.gold, before, "walking in takes nothing off the floor");
        let floor = r.lairs[0].clone();
        let page = r.lair_beaten(0);
        assert_eq!(page.lair, 0);
        assert!(!page.scouted, "a lair fought for is not a lair flown over");
        assert!(r.lairs[0].cleared);
        assert_eq!(r.experience, 1, "the first kill is worth a point");
        assert_eq!(r.gold, before, "and the gold is still on the floor");
        assert_eq!(
            r.lairs[0],
            Lair {
                cleared: true,
                ..floor
            }
        );
        r.lair_beaten(0);
        assert_eq!(r.experience, 1, "and only the first");
        assert_eq!(r.raid(0), Raid::Floor, "and the guardian stays down");
    }

    /// `_STATUS:TKGP` at 0xccfa: `dec [di]; inc [si+0x32]` round a loop that
    /// stops dead at a purse of 0x96, so what is over the ceiling stays where
    /// it is.
    #[test]
    fn the_gold_goes_over_a_coin_at_a_time_and_stops_at_the_ceiling() {
        let (mut r, _items) = run(7);
        let at = (0..LAIRS).find(|i| r.lairs[*i].gold > 0).unwrap();
        let pile = r.lairs[at].gold;
        r.lair_beaten(at);
        r.gold = crate::service::PURSE_CEILING - 1;
        assert_eq!(r.take_lair_gold(at), 1, "one coin, and the purse is full");
        assert_eq!(r.gold, crate::service::PURSE_CEILING);
        assert_eq!(r.lairs[at].gold, pile - 1, "the rest is still on the floor");
        assert_eq!(r.take_lair_gold(at), 0, "and a full purse takes nothing");
        r.gold = 0;
        assert_eq!(r.take_lair_gold(at), pile - 1);
        assert_eq!(r.lairs[at].gold, 0);
        assert_eq!(r.take_lair_gold(at), 0, "an empty floor pays nothing");
    }

    #[test]
    fn a_lair_leaves_the_map_only_when_it_is_beaten_and_stripped() {
        let (mut r, items) = run(11);
        assert!(r.lair_on_the_map(0));
        r.lair_beaten(0);
        assert!(
            r.lair_on_the_map(0) || r.lairs[0].empty(),
            "a beaten lair with a floor is still on the map"
        );
        empty_it(&mut r, 0, &items);
        assert!(r.lairs[0].empty());
        assert!(!r.lair_on_the_map(0), "beaten and stripped is off the map");
        assert_eq!(r.raid(0), Raid::Floor);
        assert!(
            r.lair_on_the_map(99),
            "a lair the run has not stocked is still on it"
        );
    }

    /// `HGTakeMagic` moves one thing per press, and a pack with no room takes
    /// nothing: the floor keeps it, which is why a beaten lair is worth
    /// coming back to.
    #[test]
    fn a_full_pack_leaves_the_magic_on_the_floor_but_the_key_still_comes_out() {
        let (mut r, items) = run(3);
        let key_at = r.lairs[..PER_FAMILY]
            .iter()
            .position(|l| l.key.is_some())
            .unwrap();
        let floor = r.lairs[key_at].clone();
        r.lair_beaten(key_at);
        // Room for the key and nothing else, and the key pressed first.
        r.kit.capacity = r.kit.carried() + 1;
        assert!(
            r.take_from_lair(key_at, KEYS_FIELD, &items),
            "the key came out"
        );
        assert!(r.lairs[key_at].key.is_none());
        assert_eq!(r.keys_held(), vec![floor.key.unwrap()]);
        assert!(
            !r.take_from_lair(key_at, KEYS_FIELD, &items),
            "and there is no second key to press"
        );
        for id in &floor.magic {
            assert!(!take(&mut r, key_at, id, &items), "{id} should not fit");
        }
        assert_eq!(r.lairs[key_at].magic, floor.magic);
        if !floor.magic.is_empty() {
            assert!(r.lair_on_the_map(key_at));
        }
    }

    /// A beaten lair you could not empty is worth coming back to, and coming
    /// back does not fight the guardian again.
    #[test]
    fn coming_back_to_a_beaten_lair_picks_up_what_was_left() {
        let (mut r, items) = run(5);
        let at = (0..LAIRS).find(|i| !r.lairs[*i].magic.is_empty()).unwrap();
        r.kit.capacity = r.kit.carried();
        r.lair_beaten(at);
        let left = r.lairs[at].magic.len();
        assert!(left > 0, "nothing fitted");
        r.kit.capacity = 200;
        assert_eq!(r.raid(at), Raid::Floor, "the guardian should stay down");
        empty_it(&mut r, at, &items);
        assert!(r.lairs[at].empty());
        assert!(!r.lair_on_the_map(at));
    }

    /// `ReDisplay` hands `DisplayLair` the lair's own record, so what the page
    /// draws is the floor and not the pack.
    #[test]
    fn the_page_draws_the_floor_and_not_the_pack() {
        let (mut r, items) = run(13);
        let at = (0..LAIRS)
            .find(|i| r.lairs[*i].magic.iter().any(|m| m == "potion"))
            .unwrap();
        r.kit.take("potion", 3);
        let (hoard, gold) = r.lair_floor(at);
        let on_the_floor = r.lairs[at].magic.iter().filter(|m| *m == "potion").count();
        assert_eq!(u32::from(hoard.potions), on_the_floor as u32);
        assert_eq!(gold, r.lairs[at].gold);
        r.lair_beaten(at);
        assert!(take(&mut r, at, "potion", &items));
        assert_eq!(
            u32::from(r.lair_floor(at).0.potions),
            on_the_floor as u32 - 1,
            "one press, one potion"
        );
    }

    /// The `STPL` table, which is the only thing between a gadget and the
    /// field it moves.
    #[test]
    fn every_field_the_page_can_carry_names_a_slot() {
        for (field, slot) in FIELD_SLOTS {
            assert_eq!(slot_of_field(field), Some(slot));
            assert_eq!(field_of_slot(slot), Some(field));
        }
        // Slot 17 is the four keys as bits and its field is `StatCheckKeys`'.
        assert_eq!(field_of_slot(17), Some(KEYS_FIELD));
        assert_eq!(
            slot_of_field(0x32),
            None,
            "gold is `TKGP`, not `HGTakeMagic`"
        );
        assert_eq!(slot_of_field(0x99), None);
    }

    #[test]
    fn a_stocked_board_survives_serialization_and_two_seeds_agree() {
        let (a, _) = run(99);
        let (b, _) = run(99);
        assert_eq!(a.lairs, b.lairs, "the same seed stocks the same board");
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(serde_json::from_str::<Run>(&json).unwrap().lairs, a.lairs);
        let (c, _) = run(100);
        assert_ne!(a.lairs, c.lairs, "and a different seed a different one");
    }

    /// A lair looked at from the air. `_STATUS:Paper2` at 0xd3ed reads
    /// `EffectFLAG+4` and gives the right arch `Identify`, which carries no
    /// permission bit, so `HGTakeMagic` returns at its `test ax, 0x20` and
    /// nothing moves. The page still draws the floor.
    #[test]
    fn a_lair_seen_from_the_air_is_read_and_not_emptied() {
        let (r, _) = run(21);
        let at = (0..LAIRS).find(|i| !r.lairs[*i].empty()).unwrap();
        let page = Page {
            lair: at,
            scouted: true,
        };
        assert!(page.scouted);
        assert_eq!(
            crate::status::Screen::Lair.right_table(page.scouted),
            crate::status::Table::Identify
        );
        assert!(
            !crate::status::Screen::Lair.right_table(page.scouted).acts(),
            "no permission bit, so nothing on the floor can be taken"
        );
        assert!(crate::status::Screen::Lair.right_table(false).acts());
        // And the floor is still drawn: the page is a look, not a blank.
        let (hoard, gold) = r.lair_floor(at);
        assert!(gold > 0 || hoard != crate::status::Hoard::default());
    }
}
