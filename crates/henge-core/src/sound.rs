//! What asks for a sound, and with which number.
//!
//! Moonstone infers nothing about sound. Every sample in the game comes from one
//! of exactly 26 call sites, all of which reach the same routine, `PLAY_SFX` at
//! image `0x5964`, with a sound id in `AL`:
//!
//! * one of them is the task VM's `TASKSOUND` handler (image `0x9b38`), which is
//!   how 131 of the shipped scripts' commands name a sample at an exact frame.
//!   [`crate::taskvm`] runs those and emits [`crate::taskvm::Effect::Sound`];
//! * 23 are the small sound routines the scripts call through `TASKGOSUB`
//!   (`KnightGruntSound`, `AddTrollSND`, `BalokRoarSound` and the rest). They are
//!   this module: each one is three to seven instructions, and every one of them
//!   is transcribed below with its address;
//! * the last two are outside a fight altogether: `ShakeDiceSnd` (`0xb32f`) and
//!   `AddClickSound` (`0xd508`), which are [`DICE_SHAKE`] and [`CLICK`].
//!
//! Nothing else in `MAIN.EXE` calls `PLAY_SFX`. There is no footstep routine, no
//! swing routine, no distance check and no channel limit: the handler loads the
//! id and calls, and `PLAY_SFX` translates the id for whichever device is set
//! and hands it over. What the id means is the other half of the chain and lives
//! in `henge_audio::sfx`, because a sound id is data the simulation never needs
//! to resolve.
//!
//! Two of the 23 routines are dead, and both are transcribed as dead rather than
//! as what they were plainly meant to do. See [`gosub`].

use serde::{Deserialize, Serialize};

/// A sound something asked for on this tick: the id it handed `PLAY_SFX`, and
/// which fighter's script asked. Output, like the hits and the parries, and
/// never read back by the simulation.
///
/// The original passes no position, no priority and no channel, so neither does
/// this: `PLAY_SFX` takes one byte.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundCall {
    /// The fighter whose script asked, or the owner of the missile whose did.
    pub who: usize,
    /// The id handed to `PLAY_SFX`, which is what `TASKSOUND` carries.
    pub id: u8,
}

/// `AddClickSound` (`0xd508`): `mov al, 0x0f` and straight into `PLAY_SFX`, with
/// nothing gating it. The status screen's own click, seventeen callers.
pub const CLICK: u8 = 0x0f;

/// `ShakeDiceSnd` (`0xb32f`): `cmp word ptr [0x8643], 2 / je ret`, then
/// `mov al, 0x10`. DS:`0x8643` is `MUSICTYPE` (0 speaker, 1 AdLib, 2 Roland;
/// the blaster sets 1 there and 3 in `SFXTYPE` at DS:`0x8645`), so on a Roland
/// the dice are silent and on anything else they are this.
pub const DICE_SHAKE: u8 = 0x10;

/// `_WIZARD:RND` (`0xbd89`) as the sound routines use it: the shift register is
/// stepped and the new word is what `AX` holds.
///
/// The register is the one word the whole game rolls against, DS:`0xe22f`, which
/// here belongs to the bout so a fight replays the same way. A sound routine
/// rolling it therefore moves the same register a controller rolls, exactly as
/// it does in the original.
fn roll(rng: &mut u16) -> u16 {
    *rng = crate::monster::rnd(*rng);
    *rng
}

/// `and ax, 3 / je +3 / sub ax, 1`, which five of the routines open with: four
/// outcomes folded onto three, with the first twice as likely.
fn fold3(r: u16) -> u8 {
    match r & 3 {
        0 | 1 => 0,
        n => (n - 1) as u8,
    }
}

/// `AddMudVoice`'s table at DS:`0x7b84`, four bytes, read with `RND & 3`.
#[rustfmt::skip]
const MUD_VOICE: [u8; 4] = [83, 84, 85, 85];

/// `AddTrollSND`'s table at DS:`0x7b88`, four bytes, read with `RND & 3`.
#[rustfmt::skip]
const TROLL_SND: [u8; 4] = [34, 35, 36, 37];

/// `TroggRoarSound`'s table at DS:`0x77a6`: six ids and a `-1` terminator.
#[rustfmt::skip]
const TROGG_ROAR: [u8; 6] = [56, 57, 58, 59, 60, 61];

/// `BalokRoarSound`'s table at DS:`0x77ba`: eight ids and a `-1` terminator.
/// Every one of the eight translates to the same sample, which is what makes
/// `AddRoar`'s lost index harmless for the balok and not for the trogg.
#[rustfmt::skip]
const BALOK_ROAR: [u8; 8] = [35, 36, 37, 38, 39, 40, 41, 42];

/// One of the original's sound routines, called by a script's `TASKGOSUB`.
///
/// Pushes the ids it hands `PLAY_SFX`, in order, and returns whether the name is
/// one of the 23. `x` is the task's own x, which is what the `TASKGOSUB` handler
/// (`0x9bda`) leaves in `AX`: only `AddCrushSnd` reads it, and it reads it by
/// accident.
///
/// Three of the routines keep a counter in BSS. None of the three counters is
/// ever read for anything that reaches a speaker, so none of them is kept here:
///
/// * `0x77a2`, the knight's grunt counter, is read only to build the return
///   value of a routine that returns to a handler which throws it away;
/// * `0x77a4`, the roar index, decides only when the roar table wraps, and the
///   entry it selects is discarded before the sound is played;
/// * `0x7b9e`, `AddCrushSnd`'s counter, has exactly one reference in the whole
///   image, the `add` that increments it.
///
/// Each is named at its site below, so what was dropped is on the record.
pub fn gosub(routine: &str, x: i32, rng: &mut u16, out: &mut Vec<u8>) -> bool {
    match routine {
        // `KnightGruntSound` / `KnightStruckSound` (0x3d5a), the two names the
        // symbol table gives the same entry point:
        //
        //   3d5a  add word ptr [0x77a2], 1     ; the grunt counter
        //   3d5f  cmp word ptr [0x77a2], 5
        //   3d64  jl   0x3d6c
        //   3d66  mov  word ptr [0x77a2], 0
        //   3d6c  mov  ax, word ptr [0x77a2]
        //   3d6f  add  ax, 4                   ; ids 4..8, the knight's grunts
        //   3d72  ret
        //   3d73  jmp  PLAY_SFX                ; never reached
        //
        // The `ret` is unconditional, the `jmp` after it is the only thing that
        // would have played the id, and nothing in the image branches to it: no
        // code calls 0x3d73, and all ten of the scripts' gosubs carry 0x3d66,
        // the link-time offset of 0x3d5a. So the ten calls in `Knight_SwChop`,
        // `Knight_SwRThrust` and the rest compute a grunt and play nothing, and
        // `grnt3b` (id 6) is one of the five samples the shipped game can never
        // reach. Whether that one byte was a patch to silence the grunts or a
        // mistake cannot be read off the image, so this plays nothing either.
        "KnightGruntSound" | "KnightStruckSound" => {}

        // `TroggRoarSound` (0x3d76) and `BalokRoarSound` (0x3e01) are a table
        // pointer and a jump into `AddRoar` (0x3d79):
        //
        //   3d79  add  word ptr [0x77a4], 2    ; the roar index, in bytes
        //   3d7e  mov  ax, word ptr [0x77a4]
        //   3d81  push si
        //   3d82  add  si, ax
        //   3d84  cmp  word ptr [si], -1       ; the indexed entry, for the wrap
        //   3d87  pop  si                      ; and the index is gone with it
        //   3d88  jne  0x3d93
        //   3d8a  mov  word ptr [0x77a4], 0
        //   3d90  mov  ax, 0
        //   3d93  mov  ax, word ptr [si]       ; si is the table base again
        //   3d95  jmp  PLAY_SFX
        //
        // The `pop` restores the base before the load, so what plays is always
        // entry zero however far the index has walked. For the balok that
        // changes nothing: all eight of its ids translate to `lion2c1`. For the
        // trogg it means all thirteen of its roars are `camel3b`, and `camel4`
        // is another of the five unreachable samples.
        "TroggRoarSound" => out.push(TROGG_ROAR[0]),
        "BalokRoarSound" => out.push(BALOK_ROAR[0]),

        // `DragonKnightSound` / `KnightCrySound` (0x3d98): the knight in the
        // dragon's jaws. `call RND / and ax,3 / je / sub ax,1 / add ax,0x1e`.
        "DragonKnightSound" | "KnightCrySound" => out.push(0x1e + fold3(roll(rng))),
        // `RatLeapSound` (0x3da9): `and ax,3 / add ax,0x61`.
        "RatLeapSound" => out.push(0x61 + (roll(rng) & 3) as u8),
        // `SkullRatSound` (0x3db5): `and ax,3 / add ax,0x65`.
        "SkullRatSound" => out.push(0x65 + (roll(rng) & 3) as u8),
        // `RatImpaleSound` (0x3dc1): folded, `add ax,0x67`.
        "RatImpaleSound" => out.push(0x67 + fold3(roll(rng))),
        // `RatScreamSound` (0x3dd2): `and ax,1 / add ax,0x5b`.
        "RatScreamSound" => out.push(0x5b + (roll(rng) & 1) as u8),
        // `DragonChewSound` (0x3dde) plays two, and rolls twice: a chew
        // (`add ax,0x18`, folded) and then a growl (`and ax,3 / add ax,0x14`).
        "DragonChewSound" => {
            out.push(0x18 + fold3(roll(rng)));
            out.push(0x14 + (roll(rng) & 3) as u8);
        }
        // `DrFireSnd` (0x3dfb): `mov ax, 0x1b`.
        "DrFireSnd" => out.push(0x1b),
        // `RatHangSound` (0x3e07): `and ax,1 / add ax,0x6a`.
        "RatHangSound" => out.push(0x6a + (roll(rng) & 1) as u8),
        // `BalokThrashSound` (0x3e13) plays two outright: `mov ax,0x30` then
        // `mov ax,0x12`, the shake and the clash.
        "BalokThrashSound" => {
            out.push(0x30);
            out.push(0x12);
        }
        // `DrBreathSnd` (0x3e20): `mov ax, 0x1d`.
        "DrBreathSnd" => out.push(0x1d),

        // `PlayScareMusic` (0x57cc) plays no music: `call RND / and ax,1 /
        // add ax,8 / call PLAY_SFX`. The mudmen's two scripts that call it get
        // a grunt or a head, whatever the name says.
        "PlayScareMusic" => out.push(8 + (roll(rng) & 1) as u8),
        // `AddMudVoice` (0x57d9): `and ax,3`, then the byte out of the table.
        "AddMudVoice" => out.push(MUD_VOICE[(roll(rng) & 3) as usize]),
        // `AddMudSound` (0x57ee): `mov ax, 0x50`.
        "AddMudSound" => out.push(0x50),
        // `AddTrollSND` (0x57f5): as `AddMudVoice`, from its own table.
        "AddTrollSND" => out.push(TROLL_SND[(roll(rng) & 3) as usize]),
        // `GuardianYellSnd` (0x580a): `mov ax, 0x4a`.
        "GuardianYellSnd" => out.push(0x4a),
        // `GuardianDiesSnd` (0x5810): `mov ax, 0x40`.
        "GuardianDiesSnd" => out.push(0x40),

        // `AddCrushSnd` (0x5816):
        //
        //   5816  add  word ptr [0x7b9e], 1    ; counted, never read
        //   581b  and  ax, 3
        //   581e  jne  0x5826
        //   5820  mov  ax, 0x5a
        //   5823  call PLAY_SFX
        //
        // Nothing loads `AX` before the `and`, so what is tested is what the
        // `TASKGOSUB` handler left there: the task's own x. The mudman's crush
        // is heard on three quarters of the screen's columns and not on the
        // fourth, which is certainly not what the counter above it was for.
        "AddCrushSnd" => {
            if x & 3 == 0 {
                out.push(0x5a);
            }
        }

        // `BigLandAudio` (0x3751) and `LittleLandAudio` (0x3758): `mov ax,0x2d`
        // and `mov ax,0x2f`, both of which translate to `baland`. Only the big
        // one is a script's gosub, in `Troll_Chop`; `BalokJumping` (0x3741)
        // picks between the two on the fall height at DS:`0x7798`.
        "BigLandAudio" => out.push(0x2d),
        "LittleLandAudio" => out.push(0x2f),

        // `KAudio0` / `KAudio1` (0x5827) and `HengeThunderSnd` (0xb419) are each
        // a bare `ret`. The dragon's three fire scripts call the first and
        // `Knight_LiftMagic` calls the second, and in the shipped game all four
        // calls are silent.
        "KAudio0" | "KAudio1" | "HengeThunderSnd" => {}

        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taskvm::{gosub_kind, GosubKind, GOSUB_TARGETS};

    fn ids(routine: &str, rng: &mut u16) -> Vec<u8> {
        let mut out = Vec::new();
        assert!(gosub(routine, 0, rng, &mut out), "{routine} is recovered");
        out
    }

    /// Every `TASKGOSUB` target the shipped scripts call that plays a sample is
    /// accounted for here. A name this module does not know would be a sound
    /// that silently never happens.
    #[test]
    fn every_sound_gosub_the_scripts_call_is_recovered() {
        let mut seen = 0;
        for (name, kind) in GOSUB_TARGETS {
            if *kind != GosubKind::Sound {
                continue;
            }
            let mut out = Vec::new();
            let mut rng = 0x2f1d;
            assert!(
                gosub(name, 0, &mut rng, &mut out),
                "{name} is not recovered"
            );
            seen += 1;
        }
        assert_eq!(seen, 23, "23 of the 41 gosub targets play a sample");
        assert!(!gosub("KnifeThrow", 0, &mut 1, &mut Vec::new()));
        assert_eq!(gosub_kind("AddTrollSND"), GosubKind::Sound);
    }

    /// The three routines that are silent in the shipped image stay silent.
    #[test]
    fn the_dead_routines_play_nothing() {
        let mut rng = 0x2f1d;
        assert!(ids("KnightGruntSound", &mut rng).is_empty());
        assert!(ids("KAudio0", &mut rng).is_empty());
        assert!(ids("HengeThunderSnd", &mut rng).is_empty());
        // And a silent routine does not move the shared register either.
        assert_eq!(rng, 0x2f1d);
    }

    /// `AddRoar` loses its index before it loads, so the roar is entry zero
    /// every time, however many times it is called.
    #[test]
    fn a_roar_is_always_the_first_entry_of_its_table() {
        let mut rng = 0x2f1d;
        for _ in 0..5 {
            assert_eq!(ids("TroggRoarSound", &mut rng), vec![56]);
            assert_eq!(ids("BalokRoarSound", &mut rng), vec![35]);
        }
    }

    /// The rolled ones stay inside the ranges their `add` sets, and the folded
    /// form never reaches the fourth id.
    #[test]
    fn a_rolled_id_stays_in_its_range() {
        let mut rng = 0x2f1d;
        let mut leaps = std::collections::BTreeSet::new();
        let mut cries = std::collections::BTreeSet::new();
        for _ in 0..200 {
            let leap = ids("RatLeapSound", &mut rng);
            assert_eq!(leap.len(), 1);
            assert!((0x61..=0x64).contains(&leap[0]));
            leaps.insert(leap[0]);
            let cry = ids("DragonKnightSound", &mut rng);
            assert!((0x1e..=0x20).contains(&cry[0]), "folded to three, not four");
            cries.insert(cry[0]);
        }
        assert_eq!(leaps.len(), 4, "all four leap ids come up");
        assert_eq!(cries.len(), 3, "and only three cries");
    }

    /// The two that play twice, and in that order.
    #[test]
    fn two_of_them_play_two_samples() {
        let mut rng = 0x2f1d;
        assert_eq!(ids("BalokThrashSound", &mut rng), vec![0x30, 0x12]);
        let chew = ids("DragonChewSound", &mut rng);
        assert_eq!(chew.len(), 2);
        assert!((0x18..=0x1a).contains(&chew[0]));
        assert!((0x14..=0x17).contains(&chew[1]));
    }

    /// `AddCrushSnd` tests the x the handler left in `AX`.
    #[test]
    fn the_crush_is_heard_on_three_columns_in_four() {
        let mut rng = 0x2f1d;
        let mut out = Vec::new();
        gosub("AddCrushSnd", 160, &mut rng, &mut out);
        assert_eq!(out, vec![0x5a], "160 is a multiple of four");
        out.clear();
        gosub("AddCrushSnd", 161, &mut rng, &mut out);
        assert!(out.is_empty(), "161 is not");
    }

    /// A routine that rolls moves the shared register, which is the one the
    /// controllers roll: the original has a single word at DS:0xe22f.
    #[test]
    fn rolling_for_a_sound_moves_the_one_register() {
        let mut rng = 0x2f1d;
        let _ = ids("AddTrollSND", &mut rng);
        assert_ne!(rng, 0x2f1d);
        assert_eq!(rng, crate::monster::rnd(0x2f1d));
    }

    /// The seam, end to end: a swing puts the id its own script names onto the
    /// bout, once, and standing and walking put nothing there.
    ///
    /// This replaces what used to be inferred. The old cue layer announced a
    /// swing the moment a fighter's state became `Attack`, and a footfall on
    /// frames 0 and 4 of a walk, neither of which the original does: the swing
    /// is `TASKSOUND 0x0b` on the second frame of `Knight_SwSwing` and the walk
    /// scripts carry no sound command at all.
    #[test]
    fn a_swing_is_heard_because_its_script_says_so_and_a_walk_is_not() {
        use crate::arena::{Border, Field};
        use crate::bout::Bout;
        use crate::combat::tests::scripted_def;
        use crate::combat::{Fighter, Intent};

        let d = scripted_def();
        let ground = Field::new(vec![Border {
            left: 0,
            right: 319,
            bottom: 60,
            top: 10,
        }]);
        let mut b = Bout::new(ground, vec![Fighter::new("k", &d, 100, 100, 1)]);
        let still = Intent {
            dx: 0,
            dy: 0,
            attack: false,
        };
        let walk = Intent {
            dx: 1,
            dy: 0,
            attack: false,
        };
        let swing = Intent {
            dx: 0,
            dy: 0,
            attack: true,
        };

        for _ in 0..10 {
            b.step(&d, &[still]);
            assert!(b.sounds.is_empty(), "a stance has no sound command");
        }
        for _ in 0..20 {
            b.step(&d, &[walk]);
            assert!(b.sounds.is_empty(), "and neither has a walk cycle");
        }

        // One press, and then the swing plays itself out: the swish comes once,
        // on the frame the script names, and not again on the frames after it.
        let mut heard: Vec<SoundCall> = Vec::new();
        b.step(&d, &[swing]);
        heard.extend(b.sounds.iter().copied());
        for _ in 0..7 {
            b.step(&d, &[still]);
            heard.extend(b.sounds.iter().copied());
        }
        assert_eq!(
            heard,
            vec![SoundCall { who: 0, id: 0x0b }],
            "one swing, one swish, and the grunt routine beside it is silent"
        );
    }

    /// A thrown dagger has a script of its own, and its `TASKSOUND` is on its
    /// first frame, which is the frame `KnifeThrow` shows the moment the blade
    /// leaves the hand. It was the one sound in the game that a bout could drop,
    /// because that frame is stepped where the task is built.
    #[test]
    fn a_thrown_dagger_is_heard_as_it_leaves_the_hand() {
        use crate::arena::{Border, Field};
        use crate::bout::Bout;
        use crate::combat::tests::depth_def;
        use crate::combat::{Fighter, Intent};
        use crate::taskvm::field;

        let d = depth_def();
        let ground = Field::new(vec![Border {
            left: 0,
            right: 319,
            bottom: 60,
            top: 10,
        }]);
        let mut b = Bout::new(
            ground,
            vec![
                Fighter::new("k", &d, 100, 100, 1),
                Fighter::new("k", &d, 220, 100, -1),
            ],
        );
        b.fighters[0].record.set(field::DAGGERS, 1);
        let throw = Intent {
            dx: -1,
            dy: -1,
            attack: true,
        };
        let mut heard: Vec<u8> = Vec::new();
        for t in 0..20 {
            let me = if t < 2 { throw } else { Intent::default() };
            b.step(&d, &[me, Intent::default()]);
            heard.extend(b.sounds.iter().filter(|c| c.who == 0).map(|c| c.id));
        }
        assert_eq!(b.fighters[0].record.get(field::DAGGERS), 0, "one thrown");
        // The dagger's own `TASKSOUND 0x0b`, once, from the frame the throw
        // shows as it spawns. (The baked `Knight_SwKnife` carries a grunt of
        // its own beside it; this miniature throwing script does not.)
        assert_eq!(heard, vec![0x0b], "the swish of the blade, once");
    }
}
