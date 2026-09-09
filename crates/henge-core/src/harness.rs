//! **A test harness feature, not a game feature.** Serialising a run so a test
//! can pose one, and reading it back.
//!
//! The original has no save, and neither does this game. `MOON.CFG` is a sound
//! card profile; no symbol among the 2,223 is a save (the only `*Save*` hit is
//! `SaveTYPE`, which is a task VM opcode); there is no slot, no file, no routine
//! and no menu entry, and a game of Moonstone is finished or abandoned in one
//! sitting. So nothing a player can reach writes or reads one of these. There is
//! no save key, no load key and no menu item, and there must not be.
//!
//! What is left is the harness. A headless run is driven from the command line,
//! and `--save <path>` with `--load` is how a test poses a run at day nine with a
//! particular purse and a particular traveller's position instead of walking the
//! whole way there every time. That is worth keeping and it is worth keeping
//! honestly labelled, which is why this module is called what it is.
//!
//! It also earns its place as a determinism check. `henge-core` is deterministic
//! by construction (integers only, no wall clock, no unseeded randomness,
//! `BTreeMap` rather than `HashMap` so orderings are defined) and the two
//! generators that matter carry their seeds inside the state, so a round trip
//! through text that comes back bit for bit and then *goes on* identically for
//! five hundred steps is a real test of that. See the tests below.
//!
//! ### The shape
//!
//! ```text
//! magic        "henge-harness", so a file that is not one says so before
//!              anything else is attempted
//! format       an integer, bumped whenever the meaning of the rest changes
//! run          the whole Run: purse, pack, knight, lairs, moon, seeds
//! travel       where on the map, what day, and how far today has gone
//! players      the title's settings, so continuing resumes the same game
//! gore         and not a differently configured one
//! wait_count   which of the fourteen the Gods say next
//! fingerprint  Run::state_hash mixed with Overworld::state_hash
//! ```
//!
//! ### What happens when a snapshot is stale
//!
//! [`Snapshot::check`] refuses, with a named reason, and the caller is expected
//! to say so and stop rather than to load half of it. A harness that silently
//! loaded a snapshot whose meaning had changed would pose a state the simulation
//! cannot produce and then test it, which is worse than no test.
//!
//! ### Where the bytes go
//!
//! Not here. This module builds the value and checks it; turning it into text and
//! putting it on a disk is `henge-desktop`'s, because `henge-core` does no I/O
//! and has one dependency. **No path of any kind is stored**, so a snapshot
//! written on one machine loads on another.

use crate::overworld::Overworld;
use crate::run::Run;
use serde::{Deserialize, Serialize};

/// What a harness snapshot has to begin with to be one.
pub const MAGIC: &str = "henge-harness";

/// The format number. **Bump this whenever the meaning of a stored field
/// changes**, and older snapshots will be refused with
/// [`SnapshotError::Version`] instead of being misread.
///
/// Two: the run carries the dragon over the map (`Run::dragon`, `wyrm_seed`)
/// and both go into its fingerprint, so a snapshot from before them would
/// read as corrupt rather than as old.
pub const FORMAT: u32 = 2;

/// Why a snapshot could not be loaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SnapshotError {
    /// Not a henge snapshot at all.
    NotASnapshot,
    /// A henge snapshot this build is too old or too new to read.
    Version { found: u32, expected: u32 },
    /// The right shape, but the contents do not match the fingerprint.
    Corrupt,
}

impl SnapshotError {
    /// A line to put in front of a player.
    pub fn message(&self) -> String {
        match self {
            SnapshotError::NotASnapshot => "that is not a harness snapshot".to_string(),
            SnapshotError::Version { found, expected } => {
                format!("that snapshot is version {found} and this build reads version {expected}")
            }
            SnapshotError::Corrupt => "that snapshot is damaged".to_string(),
        }
    }
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for SnapshotError {}

/// A posed run, as the harness stores it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
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

impl Snapshot {
    /// Take a snapshot of a run standing on the map.
    ///
    /// A snapshot is only ever taken between one step and the next. A bout is not
    /// in it: a fight is a few seconds of a game and saving inside one would
    /// mean carrying the whole task VM, every fighter's script pointer and the
    /// missiles in the air, for something nobody wants to resume mid-swing.
    /// The caller enforces that; this only records what it is given.
    pub fn of(
        run: &Run,
        travel: &Overworld,
        players: usize,
        gore: bool,
        wait_count: usize,
    ) -> Snapshot {
        let mut snap = Snapshot {
            magic: MAGIC.to_string(),
            format: FORMAT,
            run: run.clone(),
            travel: travel.clone(),
            players,
            gore,
            wait_count,
            fingerprint: 0,
        };
        snap.fingerprint = snap.contents_hash();
        snap
    }

    /// The fingerprint of what is in this snapshot, whatever it claims.
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
    /// Checked in that order on purpose: a file that is not a snapshot should not
    /// be reported as the wrong version, and the wrong version should not be
    /// reported as damage.
    pub fn check(&self) -> Result<(), SnapshotError> {
        if self.magic != MAGIC {
            return Err(SnapshotError::NotASnapshot);
        }
        if self.format != FORMAT {
            return Err(SnapshotError::Version {
                found: self.format,
                expected: FORMAT,
            });
        }
        if self.fingerprint != self.contents_hash() {
            return Err(SnapshotError::Corrupt);
        }
        Ok(())
    }

    /// A line for a trace.
    pub fn summary(&self) -> String {
        let who = if self.run.knight.named() {
            self.run.knight.name.as_str()
        } else {
            "nobody"
        };
        format!(
            "{who}  day {}  hp {}  gold {}  won {} of {}",
            self.run.day,
            self.run.health.max(0),
            self.run.gold,
            self.run.victories,
            self.run.fights
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
        travel.steps_per_day = 96;
        for _ in 0..40 {
            travel.travel(1, 0, &Default::default());
        }
        (run, travel)
    }

    /// The whole point: a snapshot reloads into a run that is the same run, field
    /// for field and seed for seed, not one that merely looks like it on a
    /// status bar.
    #[test]
    fn a_snapshot_round_trips_to_an_identical_simulation_state() {
        let (run, travel) = posed();
        let snap = Snapshot::of(&run, &travel, 2, false, 5);
        let json = serde_json::to_string(&snap).unwrap();
        let back: Snapshot = serde_json::from_str(&json).unwrap();

        back.check().expect("a fresh snapshot loads");
        assert_eq!(back, snap);
        assert_eq!(back.run, run, "every field of the run");
        assert_eq!(back.travel, travel, "and of the traveller");
        assert_eq!(
            back.run.state_hash(),
            run.state_hash(),
            "fingerprint and all"
        );
        assert_eq!(back.travel.state_hash(), travel.state_hash());
        assert_eq!((back.players, back.gore, back.wait_count), (2, false, 5));
    }

    /// And keeps going the same way. A restored run whose days mended it
    /// differently, or whose casts rolled differently, would pass an equality
    /// check on the numbers and still be a different game.
    #[test]
    fn a_restored_run_goes_on_exactly_as_the_original_would_have() {
        let (run, travel) = posed();
        let snap = Snapshot::of(&run, &travel, 1, true, 0);
        let json = serde_json::to_string(&snap).unwrap();
        let mut restored: Snapshot = serde_json::from_str(&json).unwrap();

        let mut kept = run.clone();
        let mut kept_travel = travel.clone();
        for i in 0..500 {
            if kept_travel.travel(1, 1, &Default::default()).turn_over {
                kept.new_day();
            }
            if restored.travel.travel(1, 1, &Default::default()).turn_over {
                restored.run.new_day();
            }
            if i % 97 == 0 {
                kept.roll(128);
                restored.run.roll(128);
            }
        }
        assert_eq!(
            restored.run.state_hash(),
            kept.state_hash(),
            "same road, same nights, same rolls"
        );
        assert_eq!(restored.travel.state_hash(), kept_travel.state_hash());
    }

    #[test]
    fn a_snapshot_from_an_older_format_is_refused_and_says_so() {
        let (run, travel) = posed();
        let mut snap = Snapshot::of(&run, &travel, 1, true, 0);
        snap.format = FORMAT - 1;
        assert_eq!(
            snap.check(),
            Err(SnapshotError::Version {
                found: FORMAT - 1,
                expected: FORMAT
            }),
            "refused, not read half way"
        );
        assert!(snap.check().unwrap_err().message().contains("version"));
    }

    #[test]
    fn a_file_that_is_not_a_snapshot_is_told_apart_from_one_of_the_wrong_version() {
        let (run, travel) = posed();
        let mut snap = Snapshot::of(&run, &travel, 1, true, 0);
        snap.magic = "something else".into();
        snap.format = FORMAT - 1;
        assert_eq!(
            snap.check(),
            Err(SnapshotError::NotASnapshot),
            "the magic is asked first"
        );
    }

    #[test]
    fn an_edited_snapshot_is_refused() {
        let (run, travel) = posed();
        let mut snap = Snapshot::of(&run, &travel, 1, true, 0);
        snap.run.gold += 1000;
        assert_eq!(snap.check(), Err(SnapshotError::Corrupt));
    }

    /// The failure has to survive the round trip through text, because that is
    /// how it will actually arrive: as a file on a disk from an older build.
    #[test]
    fn an_old_snapshot_read_from_text_is_refused_cleanly() {
        let (run, travel) = posed();
        let snap = Snapshot::of(&run, &travel, 1, true, 0);
        let mut value: serde_json::Value = serde_json::to_value(&snap).unwrap();
        value["format"] = serde_json::json!(0);
        let text = serde_json::to_string(&value).unwrap();
        let old: Snapshot = serde_json::from_str(&text).expect("it still parses as a Snapshot");
        assert!(matches!(
            old.check(),
            Err(SnapshotError::Version { found: 0, .. })
        ));
    }

    #[test]
    fn no_path_is_ever_written_into_a_snapshot() {
        let (run, travel) = posed();
        let snap = Snapshot::of(&run, &travel, 1, true, 0);
        let text = serde_json::to_string(&snap).unwrap();
        for suspect in ["/", "\\\\", "packs", ".json", ".png"] {
            assert!(
                !text.contains(suspect),
                "a snapshot must carry no paths, and this one has {suspect:?}"
            );
        }
    }

    #[test]
    fn a_summary_says_who_and_when() {
        let (run, travel) = posed();
        let snap = Snapshot::of(&run, &travel, 1, true, 0);
        let s = snap.summary();
        assert!(s.contains("SIR JEFFREY"), "{s}");
        assert!(s.contains("day 9"), "{s}");
    }
}
