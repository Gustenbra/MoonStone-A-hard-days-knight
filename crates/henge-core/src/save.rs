//! Saving a run, and loading one back.
//!
//! **This one is ours.** The original has no save at all: `MOON.CFG` is a
//! sound-card profile, there is no slot, no file and no routine, and a game of
//! Moonstone is finished or abandoned in one sitting. So there is nothing to
//! recover and nothing to reproduce, and the whole of what follows is designed
//! rather than ported. It is marked as such here, in `docs/BUILD_ORDER.md` and
//! in `docs/COMPLETE.md`, so that nobody later mistakes it for the original's.
//!
//! What made it easy is that the simulation was already built to allow it.
//! Everything the run consists of is `serde`-serializable, `henge-core` is
//! deterministic by construction (integers only, no wall clock, no unseeded
//! randomness, `BTreeMap` rather than `HashMap` so orderings are defined), and
//! the two generators that matter carry their seeds inside the state. A save is
//! therefore a serialization of the simulation and nothing else.
//!
//! ### The shape
//!
//! ```text
//! magic        "henge-save", so a file that is not one says so before anything
//!              else is attempted
//! format       an integer, bumped whenever the meaning of the rest changes
//! run          the whole Run: purse, pack, knight, lairs, moon, seeds
//! travel       where on the map, what day, and the traveller's own seed
//! players      the title's settings, so continuing resumes the same game
//! gore         and not a differently configured one
//! wait_count   which of the fourteen the Gods say next
//! fingerprint  Run::state_hash mixed with Overworld::state_hash
//! ```
//!
//! ### What happens when a save is stale
//!
//! [`Save::check`] refuses, with a named reason, and the caller is expected to
//! say so and carry on rather than to crash or to load half of it. The three
//! failures are distinguishable on purpose:
//!
//! - [`SaveError::NotASave`]: the magic is wrong. Someone pointed the loader at
//!   the wrong file.
//! - [`SaveError::Version`]: the magic is right and the format is not this one.
//!   The save is real and this build cannot read it, which is a different
//!   sentence to say to a player than "that is not a save".
//! - [`SaveError::Corrupt`]: the magic and the format are right and the
//!   fingerprint does not match the contents, so the file has been edited or
//!   damaged.
//!
//! A save is refused, never repaired. Silently loading a save whose meaning has
//! changed is how a run ends up in a state the simulation cannot produce, and
//! the whole value of a deterministic core is that such a state does not exist.
//!
//! ### Where the bytes go
//!
//! Not here. This module builds the value and checks it; turning it into text
//! and putting it on a disk is `henge-desktop`'s, because `henge-core` does no
//! I/O and has one dependency. **No path of any kind is stored in a save**, so
//! a save written on one machine loads on another, and a pack moved to another
//! directory does not invalidate one.

use crate::overworld::Overworld;
use crate::run::Run;
use serde::{Deserialize, Serialize};

/// What a save file has to begin with to be one.
pub const MAGIC: &str = "henge-save";

/// The format number. **Bump this whenever the meaning of a saved field
/// changes**, and older saves will be refused with [`SaveError::Version`]
/// instead of being misread.
pub const FORMAT: u32 = 1;

/// Why a save could not be loaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveError {
    /// Not a henge save at all.
    NotASave,
    /// A henge save this build is too old or too new to read.
    Version { found: u32, expected: u32 },
    /// The right shape, but the contents do not match the fingerprint.
    Corrupt,
}

impl SaveError {
    /// A line to put in front of a player.
    pub fn message(&self) -> String {
        match self {
            SaveError::NotASave => "that is not a saved game".to_string(),
            SaveError::Version { found, expected } => format!(
                "that save is version {found} and this build reads version {expected}"
            ),
            SaveError::Corrupt => "that save is damaged".to_string(),
        }
    }
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for SaveError {}

/// A saved game.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Save {
    pub magic: String,
    pub format: u32,
    pub run: Run,
    pub travel: Overworld,
    /// The title's own settings, so a loaded game is the same game.
    pub players: usize,
    pub gore: bool,
    /// `WaitCOUNT`: which of the fourteen the Gods say next.
    pub wait_count: usize,
    pub fingerprint: u64,
}

impl Save {
    /// Take a save of a run standing on the map.
    ///
    /// A save is only ever taken between one step and the next. A bout is not
    /// in it: a fight is a few seconds of a game and saving inside one would
    /// mean carrying the whole task VM, every fighter's script pointer and the
    /// missiles in the air, for something nobody wants to resume mid-swing.
    /// The caller enforces that; this only records what it is given.
    pub fn of(run: &Run, travel: &Overworld, players: usize, gore: bool, wait_count: usize) -> Save {
        let mut save = Save {
            magic: MAGIC.to_string(),
            format: FORMAT,
            run: run.clone(),
            travel: travel.clone(),
            players,
            gore,
            wait_count,
            fingerprint: 0,
        };
        save.fingerprint = save.contents_hash();
        save
    }

    /// The fingerprint of what is in this save, whatever it claims.
    pub fn contents_hash(&self) -> u64 {
        let mut h = self.run.state_hash();
        for v in [
            self.travel.state_hash(),
            self.players as u64,
            self.gore as u64,
            self.wait_count as u64,
        ] {
            h ^= v;
            h = h.wrapping_mul(0x1000_0000_01b3);
        }
        h
    }

    /// Is this loadable by this build, and is it intact?
    ///
    /// Checked in that order on purpose: a file that is not a save should not
    /// be reported as the wrong version, and the wrong version should not be
    /// reported as damage.
    pub fn check(&self) -> Result<(), SaveError> {
        if self.magic != MAGIC {
            return Err(SaveError::NotASave);
        }
        if self.format != FORMAT {
            return Err(SaveError::Version { found: self.format, expected: FORMAT });
        }
        if self.fingerprint != self.contents_hash() {
            return Err(SaveError::Corrupt);
        }
        Ok(())
    }

    /// A line for a slot list, or for a trace.
    pub fn summary(&self) -> String {
        let who = if self.run.knight.named() { self.run.knight.name.as_str() } else { "nobody" };
        format!(
            "{who}  day {}  hp {}  gold {}  won {} of {}",
            self.run.day, self.run.health.max(0), self.run.gold,
            self.run.victories, self.run.fights
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::Items;

    fn posed() -> (Run, Overworld) {
        let mut run = Run::new(100);
        run.gold = 137;
        run.day = 9;
        run.victories = 4;
        run.fights = 6;
        run.experience = 11;
        run.kit.take("potion", 2);
        run.knight.name = "SIR JEFFREY".into();
        run.knight.seat = 2;
        run.knight.strength = 3;
        run.next_arena("forest", 8);
        run.next_arena("forest", 8);
        run.stock_lairs(&["forest".into(), "glade".into()], &Items::default());
        let mut travel = Overworld::new(146, 115);
        travel.set_seed(0xfeed_1234);
        for _ in 0..40 {
            travel.travel(1, 0, &Default::default());
        }
        (run, travel)
    }

    /// The whole point: a save reloads into a run that is the same run, field
    /// for field and seed for seed, not one that merely looks like it on a
    /// status bar.
    #[test]
    fn a_save_round_trips_to_an_identical_simulation_state() {
        let (run, travel) = posed();
        let save = Save::of(&run, &travel, 2, false, 5);
        let json = serde_json::to_string(&save).unwrap();
        let back: Save = serde_json::from_str(&json).unwrap();

        back.check().expect("a fresh save loads");
        assert_eq!(back, save);
        assert_eq!(back.run, run, "every field of the run");
        assert_eq!(back.travel, travel, "and of the traveller");
        assert_eq!(back.run.state_hash(), run.state_hash(), "fingerprint and all");
        assert_eq!(back.travel.state_hash(), travel.state_hash());
        assert_eq!((back.players, back.gore, back.wait_count), (2, false, 5));
    }

    /// And keeps going the same way. A restored run that mended and was robbed
    /// on different steps would pass an equality check on the numbers and still
    /// be a different game.
    #[test]
    fn a_restored_run_goes_on_exactly_as_the_original_would_have() {
        let (run, travel) = posed();
        let save = Save::of(&run, &travel, 1, true, 0);
        let json = serde_json::to_string(&save).unwrap();
        let mut restored: Save = serde_json::from_str(&json).unwrap();

        let mut kept = run.clone();
        let mut kept_travel = travel.clone();
        for _ in 0..500 {
            kept.travelled();
            kept.waylaid();
            kept_travel.travel(1, 1, &Default::default());
            restored.run.travelled();
            restored.run.waylaid();
            restored.travel.travel(1, 1, &Default::default());
        }
        assert_eq!(restored.run.state_hash(), kept.state_hash(), "same road, same losses");
        assert_eq!(restored.travel.state_hash(), kept_travel.state_hash());
    }

    #[test]
    fn a_save_from_an_older_format_is_refused_and_says_so() {
        let (run, travel) = posed();
        let mut save = Save::of(&run, &travel, 1, true, 0);
        save.format = FORMAT - 1;
        assert_eq!(
            save.check(),
            Err(SaveError::Version { found: FORMAT - 1, expected: FORMAT }),
            "refused, not read half way"
        );
        assert!(save.check().unwrap_err().message().contains("version"));
    }

    #[test]
    fn a_file_that_is_not_a_save_is_told_apart_from_one_of_the_wrong_version() {
        let (run, travel) = posed();
        let mut save = Save::of(&run, &travel, 1, true, 0);
        save.magic = "something else".into();
        save.format = FORMAT - 1;
        assert_eq!(save.check(), Err(SaveError::NotASave), "the magic is asked first");
    }

    #[test]
    fn an_edited_save_is_refused() {
        let (run, travel) = posed();
        let mut save = Save::of(&run, &travel, 1, true, 0);
        save.run.gold += 1000;
        assert_eq!(save.check(), Err(SaveError::Corrupt));
    }

    /// The failure has to survive the round trip through text, because that is
    /// how it will actually arrive: as a file on a disk from an older build.
    #[test]
    fn an_old_save_read_from_text_is_refused_cleanly() {
        let (run, travel) = posed();
        let save = Save::of(&run, &travel, 1, true, 0);
        let mut value: serde_json::Value = serde_json::to_value(&save).unwrap();
        value["format"] = serde_json::json!(0);
        let text = serde_json::to_string(&value).unwrap();
        let old: Save = serde_json::from_str(&text).expect("it still parses as a Save");
        assert!(matches!(old.check(), Err(SaveError::Version { found: 0, .. })));
    }

    #[test]
    fn no_path_is_ever_written_into_a_save() {
        let (run, travel) = posed();
        let save = Save::of(&run, &travel, 1, true, 0);
        let text = serde_json::to_string(&save).unwrap();
        for suspect in ["/", "\\\\", "packs", ".json", ".png"] {
            assert!(
                !text.contains(suspect),
                "a save must carry no paths, and this one has {suspect:?}"
            );
        }
    }

    #[test]
    fn a_summary_says_who_and_when() {
        let (run, travel) = posed();
        let save = Save::of(&run, &travel, 1, true, 0);
        let s = save.summary();
        assert!(s.contains("SIR JEFFREY"), "{s}");
        assert!(s.contains("day 9"), "{s}");
    }
}
