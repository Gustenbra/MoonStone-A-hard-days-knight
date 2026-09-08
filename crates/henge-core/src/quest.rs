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
//! moon it was and the high nibble which of the four knights, and something
//! outside the program is meant to read it. `INTR.EXE` has never been examined
//! ([`docs/BUILD_ORDER.md`] item 55), so what it does with that byte is not
//! known and the ending henge shows is ours. [`Tally::code`] computes the byte
//! anyway, because it is recovered and because it is the one thing the original
//! records about a win.
//!
//! **The words are the original's.** Every message in the chain lives in MOON's
//! text pool at image 0xd6e0, which is a separate blob from the message records
//! themselves: the records are in the 2,906 bytes of DGROUP the load image used
//! to carry as a stale duplicate, so `NoKeysMessage`, `ValleyEnter`, `VICTORY`
//! and `GameOverMes` could not be read at their own addresses. That span is
//! readable now (`docs/REVERSING.md`) and this has not been re-read out of it.
//! The lines can be read either way, and
//! there is exactly one candidate for each:
//!
//! ```text
//! 0xd724  To be granted a / longer life you must / offer an item of /
//!         magical nature to Danu            HengeInstruct
//! 0xd772  You have completed / the quest    VICTORY
//! 0xd78f  bg8.piv
//! 0xd83b  You have proven your skill / and agility against the /
//!         Guardian.  You have been / granted a Moonstone.   ValleyEnter
//! 0xd89c  You may only enter your / own home village.
//! 0xd8c6  You must have all four keys / to enter the /
//!         Valley of the Gods                NoKeysMessage
//! 0xd908  Player       / GAME OVER          GameOverMes
//! ```
//!
//! The pairing is by content, not by address: nothing in the load image
//! connects a record to its lines, because the records are the stale part. The
//! line counts corroborate it, though. The records are ten bytes to a line, and
//! `VICTORY` to `ValleyEnter` is 0xc7 bytes with other messages in between,
//! but `HengeInstruct` to `VICTORY` is 0x28, exactly four lines, and
//! `GameOverMes` to `NoKeysMessage` is 0x14, exactly two, which are the counts
//! of the two blocks above them.
//!
//! `bg8.piv` sitting between the victory lines and the next message is why the
//! ending is drawn over that plate. That is an inference from where the string
//! sits and not a reference anybody has traced, and it is the only full-screen
//! picture MOON names.
//!
//! **The Guardian's own fight is recovered too.** `InitKnightvsDemon` writes
//! 250 into the demon's health, one monster, and `ColourBackDrop` with 4, which
//! is the swamp: the Valley of the Gods is fought on marsh ground.
//!
//! **What is ours**: where the Valley stands on the map, because
//! `MOON:MapIconsTABLE` is in the unreadable part of DGROUP like every other
//! place but the two towns; the ending screen and the tally on it, because the
//! original has neither and exits to DOS instead; and the words of the tally.

use crate::moon::{Key, Moonstone, Phase};
use crate::item::Items;
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

/// `VICTORY`, verbatim.
pub const VICTORY: [&str; 2] = ["You have completed", "the quest"];

/// `GameOverMes`, verbatim. The original prints the player's number into the
/// blank of the first line; a run here belongs to one knight, so his name goes
/// there instead.
pub const GAME_OVER: [&str; 2] = ["Player      ", "GAME OVER"];

/// The plate the ending is drawn over: the only picture MOON names by file,
/// and it sits in the text pool immediately after the victory lines.
pub const VICTORY_PLATE: &str = "scene.bg8";

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

/// The final tally.
///
/// **Ours.** The original keeps no such page: `KnightWonGame` shows two lines
/// and exits to DOS, and the game-over routine shows two lines and goes back
/// to the title. Everything counted here is state the run already carries, so
/// the tally invents nothing; it only reads out what a quest cost.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tally {
    pub knight: String,
    /// The seat, which is also the knight number `KnightWonGame` folds into
    /// its exit code.
    pub seat: usize,
    pub ending: Ending,
    pub day: u32,
    pub fights: u32,
    pub victories: u32,
    /// Lairs whose guardian is down, out of however many there are.
    pub lairs_cleared: usize,
    pub lairs: usize,
    pub keys: Vec<Key>,
    pub stones: Vec<Moonstone>,
    pub gold: u32,
    pub experience: u32,
    pub lives: i32,
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

    /// The heading, which is one of the original's two messages.
    pub fn heading(&self) -> String {
        self.heading_lines().join(" ")
    }

    /// The heading as the original sets it: `VICTORY` is two lines and
    /// `GameOverMes` two, of which the first is a blank for the player number
    /// and is replaced here by the knight's name.
    pub fn heading_lines(&self) -> Vec<String> {
        match self.ending {
            Ending::Won { .. } => VICTORY.iter().map(|l| l.to_string()).collect(),
            Ending::Slain => vec![GAME_OVER[1].to_string()],
        }
    }

    /// The lines under it. Ours: the original counts none of this, which is
    /// why they read as plainly as they do. `Life points left` is the one
    /// label of the seven that is the original's, off the status panel.
    pub fn lines(&self) -> Vec<String> {
        vec![
            format!("{}   Day {}", self.knight, self.day),
            format!("Won {} of {} fights", self.victories, self.fights),
            format!("Lairs cleared {} of {}", self.lairs_cleared, self.lairs),
            format!("Gold {}   Experience {}", self.gold, self.experience),
            format!("Life points left {}", self.lives),
            format!("Keys {} of {}", self.keys.len(), Key::ALL.len()),
            if self.stones.is_empty() {
                "No moonstone".to_string()
            } else {
                self.stones.iter().map(|s| s.name()).collect::<Vec<_>>().join(", ")
            },
        ]
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

    /// The final page. `None` while the run is still going.
    pub fn tally(&self) -> Option<Tally> {
        let ending = self.ending()?;
        Some(Tally {
            knight: if self.knight.named() {
                self.knight.name.clone()
            } else {
                "A knight".into()
            },
            seat: self.knight.seat,
            ending,
            day: self.day,
            fights: self.fights,
            victories: self.victories,
            lairs_cleared: self.lairs.iter().filter(|l| l.cleared).count(),
            lairs: self.lairs.len(),
            keys: self.keys_held(),
            stones: self.stones_held(),
            gold: self.gold,
            experience: self.experience,
            lives: self.lives.max(0),
        })
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
            name: "Sir Banner".into(),
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
        assert_eq!(tally.knight, "Sir Banner");
        assert_eq!(tally.stones, vec![stone]);
        assert!(tally.keys.is_empty());
        assert_eq!(tally.heading(), "You have completed the quest");
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

    /// Losing properly: the last life ends the run, and the tally says so.
    #[test]
    fn the_last_life_ends_the_run_and_the_tally_says_what_it_cost() {
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
        assert_eq!(tally.heading(), "GAME OVER");
        assert_eq!(tally.fights, 5);
        assert_eq!(tally.victories, 0);
        assert_eq!(tally.lairs_cleared, 1);
        assert_eq!(tally.lairs, PER_FAMILY);
        assert_eq!(tally.gold, 42);
        let lines = tally.lines();
        assert!(lines.iter().any(|l| l == "No moonstone"), "{lines:?}");
        assert!(lines.iter().any(|l| l == "Lairs cleared 1 of 6"), "{lines:?}");
        assert!(lines.iter().any(|l| l == "Keys 0 of 4"), "{lines:?}");
        assert!(lines.iter().any(|l| l == "Life points left 0"), "{lines:?}");
        // Nothing on the page runs away with itself: the ending screen is 320
        // pixels wide and a line that overflows it is a line nobody can read.
        assert!(lines.iter().all(|l| l.len() <= 40), "{lines:?}");
    }

    #[test]
    fn an_ending_survives_serialization() {
        let e = Ending::Won { stone: Moonstone::Half, phase: Phase::Gibbous };
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(serde_json::from_str::<Ending>(&json).unwrap(), e);
    }
}
