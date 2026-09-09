//! The quest: four keys, the Valley of the Gods, a moonstone, and the end.
//!
//! This is the point of the game, and almost all of it turned out to be in the
//! executable after all. The plan had it down as design.
//!
//! **The chain, recovered end to end.**
//!
//! ```text
//! MOON:Valley          si = knight; si = [si+0x44]      the item record
//!                      cmp byte ptr [si+0x14], 0xf      all four key bits
//!                      jne -> NoKeysMessage, and done
//!                      InitKnightvsDemon; InitCombat    the Guardian
//!                      test KnightDeath, 1
//!                        died: sub byte ptr [si+0x31], 2    two life points
//!                        won:  add word ptr [si+0x36], 3    three experience
//!                              mov byte ptr [si+0x14], 0    the keys are spent
//!                              ValleyEnter                  what it says
//!                              rnd & 3; al = 1 << that
//!                              or byte ptr [di+0x16], al    one moonstone
//!
//! MOON:Henge           ax = tonight's moon; bl = [bx+0x16]
//!                      bl & 4 && ax == 0x2e -> KnightWonGame
//!                      bl & 8 && ax == 0x2e -> KnightWonGame
//!                      bl & 2 && ax == 0x2d -> KnightWonGame
//!                      bl & 1 && ax == 0x31 -> KnightWonGame
//!                      else HengeInstruct, and an offering
//!
//! MOON:KnightWonGame   bp = 1; 0x2e -> 2; 0x2d -> 4; 0x31 -> 3
//!                      [di+0x20] = 3 -> bp |= 0x10, 0 -> 0x20, 1 -> 0x30, 2 -> 0x40
//!                      VICTORY; delay 0x14
//!                      [0x8186] = bp; jmp 0x8e, which is int 21h/4Ch with it in al
//! ```
//!
//! So **winning quits MAIN.EXE with an exit code**: the low nibble says which
//! moon it was and the high nibble which of the four knights, and `INTR.EXE`
//! reads it. Its entry does `mov ax, es:[0x82]; sub ax, 0x3131` on a two digit
//! command tail and jumps to the ending sequence when there is one, and the
//! routine at its 0x3b9d branches on the second digit to patch four twelve bit
//! words into a plate's palette: red for 1, blue for 2, gold for 3, green for 4.
//! `KnightWonGame`'s high nibble is 3 -> 1, 0 -> 2, 1 -> 3, 2 -> 4, and seats 3,
//! 0, 1 and 2 are the red, blue, gold and emerald knights, so the two agree four
//! for four. [`Tally::code`] is that byte. `docs/COMPLETE.md` 8.6 has the rest;
//! the sequence itself is not built.
//!
//! **The two endings are two messages and nothing else.** Both were read out of
//! the image rather than designed:
//!
//! ```text
//! MOON:KnightWonGame 0x10cf  push bp                      ; the exit byte
//!                            mov si, VICTORY
//!                            call 0x8eeb                  ; OCCURMESSAGE
//!                            mov ax, 0x14; call 0x5a24    ; one vertical blank
//!                            call 0x8251                  ; WaitFIRE
//!                            mov ax, 1; call 0x8f80
//!                            pop ax; mov [0x8186], al
//!                            jmp 0x8e   -> mov ah, 4ch; mov al, [0x8186]; int 21h
//!
//! MOON:0x617                 mov si, GameOverMes
//!                            call 0x8f17                  ; INSTRUCTMESSAGE
//!                            call 0x8251                  ; WaitFIRE
//!                            call 0xa554                  ; the map's effects off
//!                            jmp StartAgain               ; the title
//! ```
//!
//! `0x617` is reached from `_MAP:ScrollINPUT`, where scancode 0x10 quits, and
//! from `_MAP:NextWHICH`, where the last knight with no life points left falls
//! through to it.
//!
//! **So there is no ending screen and no tally.** `OCCURMESSAGE` (image 0x8eeb)
//! and `INSTRUCTMESSAGE` (0x8f17) both `call 0x8e90` first, which is the
//! `rep movsb` that puts `MESSAGE.PIV` back, so a win and a loss are the same
//! stone circle every other message is drawn over; the loss gets the red ramp
//! because it goes through the instruction door. A page of seven counted lines
//! over `bg8.piv` used to stand here. `GAMEOVER`, `GAMETABLE`, `TOTALS`,
//! `PPOINT`, `PINDEX`, `FMEM_POINTS` and `FMEM_COLAREA` are PUBLIC names with
//! no addresses behind them, so there was never a tally to port, and `bg8.piv`
//! belongs to `INTR.EXE`'s ending sequence rather than to this.
//!
//! **The words are the original's, and the records have now been read.** Every
//! message in the chain lives in MOON's text pool at image 0xd6e0, which is a
//! separate blob from the message records themselves: the records are in the
//! 2,906 bytes of DGROUP the load image used to carry as a stale duplicate, so
//! `NoKeysMessage`, `ValleyEnter`, `VICTORY` and `GameOverMes` could once only
//! be paired with their lines by content. That span is readable now
//! (`docs/REVERSING.md`), each record is `{text, x, y, flags, next}`, and the
//! chains walk out as:
//!
//! ```text
//! HengeInstruct  To be granted a / longer life you must / offer an item of /
//!                magical nature to Danu / Press fire to continue
//! VICTORY        You have completed  y 75 / the quest  y 95
//! ValleyEnter    You have proven your skill / and agility against the /
//!                Guardian.  You have been / granted a Moonstone. /
//!                Press fire to continue
//! NoKeysMessage  You must have all four keys / to enter the /
//!                Valley of the Gods / Press fire to continue
//! GameOverMes    GAME OVER  y 95 / Press fire to continue  y 180
//! ```
//!
//! Every record in both of those two carries flags 1, which is
//! `TextPTop`'s centre bit.
//!
//! The old pairing was right about every line but one. **`GameOverMes` is not
//! two lines with a blank for the player number.** It is `GOmes1`, `GAME OVER`,
//! centred at y 95, and then `Press fire to continue` at y 180. The string
//! `Player      ` sits nine bytes before `GOmes1` in the data and was taken for
//! the first of them; nothing in the image refers to its address at all, and
//! `GameOverMes`'s first record points at `GOmes1`. It is dead data.
//!
//! `bg8.piv` sits in the text pool between the victory lines and the next
//! message, which is why this project once drew the ending over that plate.
//! Nothing in MOON refers to the string's address, `OCCURMESSAGE` puts
//! `MESSAGE.PIV` up unconditionally, and `bg8` is one of the three plates
//! `INTR.EXE`'s ending half uses. So the name is in MOON's pool and the picture
//! is not MOON's to show.
//!
//! **The Guardian's own fight is recovered too.** `InitKnightvsDemon` writes
//! 250 into the demon's health, one monster, and `ColourBackDrop` with 4, which
//! is the swamp: the Valley of the Gods is fought on marsh ground.
//!
//! **What is ours**: where the Valley stands on the map, because
//! `MOON:MapIconsTABLE` is in the unreadable part of DGROUP like every other
//! place but the two towns. Nothing else.

use crate::item::Items;
use crate::message::{Kind, Line, Message, FLAG_CENTRE};
use crate::moon::{Key, Moonstone, Phase};
use crate::run::Run;
use serde::{Deserialize, Serialize};

/// `NoKeysMessage`, verbatim, in the original's own three lines.
pub const NO_KEYS: [&str; 3] =
    ["You must have all four keys", "to enter the", "Valley of the Gods"];

/// `ValleyEnter`, verbatim. Two spaces after the full stop are the original's.
pub const VALLEY_ENTER: [&str; 4] = [
    "You have proven your skill",
    "and agility against the",
    "Guardian.  You have been",
    "granted a Moonstone.",
];

/// `VICTORY`, verbatim: `SHMES7` at y 75 and `SHMES8` at y 95, both centred.
pub const VICTORY: [(&str, i32); 2] = [("You have completed", 75), ("the quest", 95)];

/// `GameOverMes`, verbatim: `GOmes1` at y 95 and `promes4` at y 180, both
/// centred.
///
/// The string `Player      ` sits nine bytes before `GOmes1` in the data and was
/// once taken for a first line of this chain. Nothing in the image refers to its
/// address; `GameOverMes`'s first record points at `GOmes1`.
pub const GAME_OVER: [(&str, i32); 2] =
    [("GAME OVER", 95), ("Press fire to continue", 180)];

/// All four key bits. `cmp byte ptr [si+0x14], 0xf`.
pub const ALL_KEYS: u8 = 0xf;

/// What the Valley pays for the Guardian. `add word ptr [si+0x36], 3`.
pub const VALLEY_EXPERIENCE: u32 = 3;

/// What losing there costs. `sub byte ptr [si+0x31], 2`.
pub const VALLEY_LIVES: i32 = 2;

/// What walking up to the Valley of the Gods did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Gate {
    /// Fewer than four keys. The gate says so and nothing else happens.
    Barred,
    /// Four keys, and the Guardian is waiting. The caller fights it and comes
    /// back through [`Run::valley_won`] or [`Run::valley_lost`].
    Guardian,
}

impl Gate {
    /// What the gate says. `NoKeysMessage` when it is shut.
    pub fn describe(&self) -> String {
        match self {
            Gate::Barred => NO_KEYS.join(" "),
            Gate::Guardian => "The Guardian rises.".into(),
        }
    }
}

/// How a run ended, for the screen that says so.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Ending {
    /// The stone circle took the stone of the night. `MOON:KnightWonGame`.
    Won { stone: Moonstone, phase: Phase },
    /// The last life point is gone. The routine at image 0x617:
    /// `GameOverMes`, then `jmp StartAgain`, which is the title screen.
    Slain,
}

/// How a finished run ends: which of the two messages goes up, and the byte a
/// win leaves behind.
///
/// There is nothing else in it. `KnightWonGame` shows `VICTORY` and quits with
/// the exit byte; the routine at 0x617 shows `GameOverMes` and jumps to
/// `StartAgain`. Neither counts anything, so neither does this.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tally {
    /// The seat, which is the knight number `KnightWonGame` folds into its
    /// exit code: `[di+0x20]`.
    pub seat: usize,
    pub ending: Ending,
}

impl Tally {
    /// The byte `KnightWonGame` leaves in `al` for DOS.
    ///
    /// ```text
    /// bp = 1; if moon == 0x2e { bp = 2 }; if moon == 0x2d { bp = 4 };
    /// if moon == 0x31 { bp = 3 }
    /// knight 3 -> bp |= 0x10   knight 0 -> 0x20
    /// knight 1 -> bp |= 0x30   knight 2 -> 0x40
    /// ```
    ///
    /// A loss leaves nothing in it, because the loss path never reaches this
    /// routine: it jumps to `StartAgain` and the program keeps running.
    pub fn code(&self) -> Option<u8> {
        let Ending::Won { phase, .. } = self.ending else { return None };
        let low = match phase {
            Phase::Gibbous => 2,
            Phase::Full => 4,
            Phase::New => 3,
            _ => 1,
        };
        let high = match self.seat {
            3 => 0x10,
            0 => 0x20,
            1 => 0x30,
            2 => 0x40,
            _ => 0,
        };
        Some(low | high)
    }

    /// The chain that goes up, and which of the three doors shows it.
    ///
    /// `KnightWonGame` hands `VICTORY` to `OCCURMESSAGE` and the routine at
    /// 0x617 hands `GameOverMes` to `INSTRUCTMESSAGE`, so a win comes up in the
    /// ordinary message colours and a loss in the red ramp that door installs.
    pub fn message(&self) -> Message {
        let (kind, chain): (Kind, &[(&str, i32)]) = match self.ending {
            Ending::Won { .. } => (Kind::Occurrence, &VICTORY),
            Ending::Slain => (Kind::Instruction, &GAME_OVER),
        };
        Message {
            kind,
            lines: chain.iter().map(|(t, y)| Line::new(t, 0, *y, FLAG_CENTRE)).collect(),
        }
    }
}

impl Run {
    /// The four key bits, the way the original keeps them: byte `+0x14` of the
    /// item record, which `_STATUS:StatCheckKeys` draws and `MOON:Valley`
    /// tests against 0xf.
    pub fn key_bits(&self) -> u8 {
        Key::ALL.iter().filter(|k| self.kit.count(k.item()) > 0).map(|k| k.bit()).sum()
    }

    /// The moonstone bits, byte `+0x16`.
    pub fn stone_bits(&self) -> u8 {
        Moonstone::ALL.iter().filter(|m| self.kit.count(m.item()) > 0).map(|m| m.bit()).sum()
    }

    /// Which moonstones the run carries.
    pub fn stones_held(&self) -> Vec<Moonstone> {
        Moonstone::ALL.into_iter().filter(|m| self.kit.count(m.item()) > 0).collect()
    }

    /// Walk up to the Valley of the Gods. `MOON:Valley`, its first four
    /// instructions: all four keys or nothing at all.
    pub fn valley(&self) -> Gate {
        if self.key_bits() == ALL_KEYS {
            Gate::Guardian
        } else {
            Gate::Barred
        }
    }

    /// The Guardian is down.
    ///
    /// Three points of experience, the four keys spent, and one of the four
    /// moonstones, chosen by `rnd & 3` and set as `1 << that`. The keys go
    /// first, so there is always room in the pack for the stone they bought.
    pub fn valley_won(&mut self, items: &Items) -> Moonstone {
        self.earned_experience(VALLEY_EXPERIENCE);
        // `mov byte ptr [si+0x14], 0`. Four keys are what the Valley costs,
        // and a second stone means going back for four more.
        for key in Key::ALL {
            self.kit.lose(key.item(), self.kit.count(key.item()));
        }
        let bit = 1u8 << (self.next_roll() & 3);
        let stone = Moonstone::from_bit(bit).unwrap_or(Moonstone::New);
        self.kit.take(stone.item(), 1);
        self.refresh(items);
        stone
    }

    /// The Guardian won. Two life points, which is the only place in the game
    /// that takes more than one.
    ///
    /// **And they are on top of the one the fight itself took.** `MOON:Combat`
    /// ends its loop with `call WhoLived`, whatever it was a fight with, so a
    /// knight who goes down in the Valley has already paid a life for the
    /// death before `Valley` reaches `sub byte ptr [si+0x31], 2`. Losing there
    /// costs three of the five.
    ///
    /// The original also decrements a byte at `si + bx` with `bx` holding
    /// whatever the combat left in it, which is not reproducible and is not
    /// reproduced: nothing in `Valley` sets `bx` before it is used.
    pub fn valley_lost(&mut self) {
        for _ in 0..VALLEY_LIVES {
            if self.lives <= 0 {
                break;
            }
            self.spend_life();
        }
    }

    /// How a run finished, or nothing while it is still going.
    pub fn ending(&self) -> Option<Ending> {
        if self.won {
            let phase = self.moon.phase();
            let stone = self
                .stones_held()
                .into_iter()
                .find(|s| s.phase() == phase)
                .unwrap_or(Moonstone::Full);
            return Some(Ending::Won { stone, phase });
        }
        (!self.alive()).then_some(Ending::Slain)
    }

    /// Which of the two endings is up, and the seat the exit byte is made from.
    /// `None` while the run is still going.
    pub fn tally(&self) -> Option<Tally> {
        Some(Tally { seat: self.knight.seat, ending: self.ending()? })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{ItemDef, Virtue};
    use crate::knight::KnightDef;
    use crate::lair::PER_FAMILY;
    use crate::moon::Phase;

    fn goods() -> Items {
        let mut items = Items::new();
        let mut add = |id: &str, virtue: Virtue| {
            items.insert(id.into(), ItemDef { name: id.into(), price: 0, virtue, consumed: false });
        };
        for id in ["potion", "gem_of_seeing"] {
            add(id, Virtue::Inert);
        }
        add("long_sword", Virtue::Weapon { damage: 0 });
        add("padded_armour", Virtue::Armour { health: 0, stride: 0 });
        for k in Key::ALL {
            add(k.item(), Virtue::Inert);
        }
        for m in Moonstone::ALL {
            add(m.item(), Virtue::Inert);
        }
        items
    }

    fn knight() -> KnightDef {
        KnightDef {
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
        }
    }

    fn run(seed: u32) -> (Run, Items) {
        let items = goods();
        let mut r = Run::for_knight(&knight(), 0, &items);
        r.reseed(seed);
        r.kit.capacity = 40;
        (r, items)
    }

    fn all_four(r: &mut Run) {
        for k in Key::ALL {
            r.kit.take(k.item(), 1);
        }
    }

    /// `cmp byte ptr [si+0x14], 0xf`: three keys is not four.
    #[test]
    fn the_valley_is_shut_until_all_four_keys_are_in_the_pack() {
        let (mut r, _) = run(1);
        assert_eq!(r.valley(), Gate::Barred);
        for k in [Key::Forest, Key::Waste, Key::Swamp] {
            r.kit.take(k.item(), 1);
            assert_eq!(r.valley(), Gate::Barred, "{k:?} and still short");
        }
        assert_eq!(r.key_bits(), 8 | 4 | 2);
        r.kit.take(Key::Glade.item(), 1);
        assert_eq!(r.key_bits(), ALL_KEYS);
        assert_eq!(r.valley(), Gate::Guardian);
        assert_eq!(Gate::Barred.describe(), "You must have all four keys to enter the Valley of the Gods");
    }

    /// The whole point of the keys: they are spent, and what they buy is a
    /// moonstone.
    #[test]
    fn the_guardian_beaten_spends_the_keys_and_pays_a_moonstone() {
        let (mut r, items) = run(7);
        all_four(&mut r);
        assert_eq!(r.experience, 0);
        let stone = r.valley_won(&items);
        assert_eq!(r.experience, VALLEY_EXPERIENCE);
        assert_eq!(r.key_bits(), 0, "mov byte ptr [si+0x14], 0");
        assert_eq!(r.keys_held(), Vec::new());
        assert_eq!(r.stones_held(), vec![stone]);
        assert_eq!(r.stone_bits(), stone.bit());
        assert_eq!(r.valley(), Gate::Barred, "and the gate is shut behind you");
    }

    /// `rnd & 3`, so all four come up and the quest is not the same walk
    /// every time.
    #[test]
    fn which_moonstone_is_a_roll_of_four() {
        let mut seen = std::collections::BTreeSet::new();
        for seed in 1..80u32 {
            let (mut r, items) = run(seed.wrapping_mul(0x9e37_79b9) | 1);
            all_four(&mut r);
            seen.insert(r.valley_won(&items));
        }
        assert_eq!(seen.len(), 4, "all four stones: {seen:?}");
    }

    /// A pack with no room still gets the stone, because four keys leave it
    /// first.
    #[test]
    fn a_full_pack_still_has_room_for_what_the_keys_bought() {
        let (mut r, items) = run(3);
        all_four(&mut r);
        r.kit.capacity = r.kit.carried();
        assert_eq!(r.kit.room(), 0);
        let stone = r.valley_won(&items);
        assert_eq!(r.kit.count(stone.item()), 1);
    }

    /// `sub byte ptr [si+0x31], 2`, and the keys stay: the win branch is the
    /// only one that clears them, so a beating is worth trying again.
    #[test]
    fn losing_to_the_guardian_costs_two_life_points_and_keeps_the_keys() {
        let (mut r, _) = run(5);
        all_four(&mut r);
        assert_eq!(r.lives, 5);
        r.valley_lost();
        assert_eq!(r.lives, 3);
        // And in a real defeat the bout's own `WhoLived` has taken one first,
        // so three of the five are gone.
        let mut whole = run(5).0;
        whole.finished_fight(0, false, 0);
        whole.valley_lost();
        assert_eq!(whole.lives, 2);
        assert_eq!(r.key_bits(), ALL_KEYS, "the gate is still open");
        assert!(r.alive());
        r.valley_lost();
        r.valley_lost();
        assert_eq!(r.lives, 0);
        assert!(!r.alive(), "and the last two end it");
    }

    /// The whole chain: keys out of lairs, the Valley, the stone, the circle.
    #[test]
    fn four_keys_a_guardian_and_a_moonstone_win_the_game() {
        let items = goods();
        let mut r = Run::for_knight(&knight(), 0, &items);
        r.reseed(0x1234_5679);
        r.kit.capacity = 40;
        assert!(r.tally().is_none(), "nothing is over yet");
        all_four(&mut r);
        assert_eq!(r.valley(), Gate::Guardian);
        let stone = r.valley_won(&items);
        // Walk the moon round to the night that stone answers to.
        while r.moon.phase() != stone.phase() {
            r.moon.new_day();
        }
        assert_eq!(r.rite_at_the_stones(None, &items), crate::service::Rite::Won(stone));
        let tally = r.tally().expect("the quest is done");
        assert_eq!(tally.ending, Ending::Won { stone, phase: stone.phase() });
        assert_eq!(r.knight.name, "SIR GODBER");
        assert_eq!(r.stones_held(), vec![stone]);
        assert!(r.keys_held().is_empty());
        // `KnightWonGame` hands `VICTORY` to `OCCURMESSAGE`, so the win is two
        // centred lines over `MESSAGE.PIV` in the ordinary message colours.
        let m = tally.message();
        assert_eq!(m.kind, Kind::Occurrence);
        assert_eq!(
            m.lines.iter().map(|l| (l.text.as_str(), l.y)).collect::<Vec<_>>(),
            vec![("You have completed", 75), ("the quest", 95)],
        );
        assert!(m.lines.iter().all(|l| l.align == crate::message::Align::Centre));
    }

    /// The exit byte `KnightWonGame` leaves for DOS, for each of the four
    /// knights and each of the three moons it names.
    #[test]
    fn a_win_has_the_original_exit_code_in_it() {
        let items = goods();
        let mut r = Run::for_knight(&knight(), 0, &items);
        r.won = true;
        r.kit.take(Moonstone::Full.item(), 1);
        assert_eq!(r.moon.phase(), Phase::Full);
        let mut t = r.tally().unwrap();
        assert_eq!(t.code(), Some(0x24), "knight 0 on the full moon");
        for (seat, want) in [(0, 0x20), (1, 0x30), (2, 0x40), (3, 0x10)] {
            t.seat = seat;
            assert_eq!(t.code(), Some(want | 4));
        }
        t.ending = Ending::Won { stone: Moonstone::New, phase: Phase::New };
        t.seat = 0;
        assert_eq!(t.code(), Some(0x23));
        t.ending = Ending::Won { stone: Moonstone::Gibbous, phase: Phase::Gibbous };
        assert_eq!(t.code(), Some(0x22));
        t.ending = Ending::Won { stone: Moonstone::Half, phase: Phase::Half };
        assert_eq!(t.code(), Some(0x21), "a moon the routine does not name leaves the 1");
        t.ending = Ending::Slain;
        assert_eq!(t.code(), None, "the loss path never reaches that routine");
    }

    /// Losing properly: the last life ends the run, and what goes up is
    /// `GameOverMes` through the instruction door.
    #[test]
    fn the_last_life_ends_the_run_and_game_over_goes_up() {
        let items = goods();
        let mut r = Run::for_knight(&knight(), 0, &items);
        r.lairs = vec![crate::lair::Lair::default(); PER_FAMILY];
        r.lairs[0].cleared = true;
        r.gold = 42;
        for n in 1..=5 {
            assert!(r.tally().is_none(), "still up after {} deaths", n - 1);
            r.finished_fight(0, false, 0);
        }
        let tally = r.tally().expect("out of lives");
        assert_eq!(tally.ending, Ending::Slain);
        assert_eq!(tally.code(), None, "a loss leaves no exit byte");
        // The run's own counters are untouched by a run ending: nothing reads
        // them out onto a screen, because the original has no such screen, but
        // the loss itself has to have been counted.
        assert_eq!(r.fights, 5);
        assert_eq!(r.victories, 0);
        assert_eq!(r.lairs.iter().filter(|l| l.cleared).count(), 1);
        assert_eq!(r.lairs.len(), PER_FAMILY);
        assert_eq!(r.gold, 42);
        assert_eq!(r.lives, 0);
        // `mov si, GameOverMes; call 0x8f17`, which is `INSTRUCTMESSAGE`: the
        // red ramp, `GOmes1` at y 95 and `promes4` at y 180, both centred.
        let m = tally.message();
        assert_eq!(m.kind, Kind::Instruction);
        assert_eq!(
            m.lines.iter().map(|l| (l.text.as_str(), l.y)).collect::<Vec<_>>(),
            vec![("GAME OVER", 95), ("Press fire to continue", 180)],
        );
        assert!(m.lines.iter().all(|l| l.align == crate::message::Align::Centre));
    }

    #[test]
    fn an_ending_survives_serialization() {
        let e = Ending::Won { stone: Moonstone::Half, phase: Phase::Gibbous };
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(serde_json::from_str::<Ending>(&json).unwrap(), e);
    }
}
