//! What a sound id means: `PLAY_SFX`, and the two tables behind it.
//!
//! A sound id is not a sample number. `TASKSOUND` and the sound routines hand
//! `PLAY_SFX` (image `0x5964`) one byte, and that byte is translated for whatever
//! device the player chose before it reaches a driver:
//!
//! ```text
//! 5964  push ...                        ; it saves everything
//! 596d  cmp  word ptr [0x8645], 3       ; SFXTYPE: 0 speaker, 1 AdLib,
//! 5972  je   0x5989                     ;          2 Roland, 3 Sound Blaster
//! 5974  mov  bx, 0x7d02                 ; the FM effect table
//! 5977  xlatb                           ; al = [bx + al]
//! 5978  mov  bl, 0x0f
//! 597a  mov  ah, 0x40
//! 597c  and  al, 0xff                   ; sets flags, and nothing reads them
//! 597e  cmp  word ptr [0x8645], 3
//! 5983  je   0x5989
//! 5985  int  0x61                       ; the music driver's effect call
//! 5987  jmp  0x5994
//! 5989  mov  bx, 0x7e6e                 ; the sample table
//! 598c  xlatb
//! 598d  mov  ah, 0
//! 598f  and  al, 0xff
//! 5991  call PLAYSAMPLE                 ; image 0x932a
//! 5994  pop ...                         ; and returns
//! ```
//!
//! There is no gate in it. No distance, no volume beyond the constant `bl`, no
//! priority, no channel count, and no check for the same sound already playing:
//! the `and al, 0xff` pair set flags that nothing tests, and every call reaches a
//! driver. The only conditions anywhere near the path are the device word at
//! DS:`0x8645` selecting which table to use, and `PLAYSAMPLE`'s own
//! `cmp word ptr [0x81b4], 0 / jne ret`, the samples-not-loaded flag.
//!
//! We render samples, which is the `SFXTYPE == 3` configuration, so the table
//! that matters here is the one at DS:`0x7e6e` (image `0x1a21e`): [`SAMPLE_OF`].
//! The FM table at DS:`0x7d02` (image `0x1a0b2`) is the same shape and is not
//! translated, because nothing here drives an OPL2.
//!
//! What `PLAYSAMPLE` takes is an index into `SBSampleTab` (DS:`0x865d`), whose 49
//! records are the samples the loader pulled out of `SAMPLES/`, named by
//! `SBFileTable` (DS:`0x8651`, image `0x1aad1`): [`SAMPLES`]. The baker writes
//! those same 49 files as `sfx.<stem>`, so an id resolves to an asset id without
//! anything having to be chosen here.

/// `SBFileTable` (DS:`0x8651`, image `0x1aad1`): 49 near pointers to the sample
/// names, in the order `SBSampleTab` is indexed by. The baker writes each as
/// `sfx.<name>`.
///
/// Entries 8 and 9 both say `cheap2b`. That is what the image says: the string
/// `cheap1b` does not occur in it at all, though `SAMPLES/CHEAP1B` is on the
/// disk and the baker bakes it. So the shipped game loads `cheap2b` twice, id 86
/// (`Mudmen_ArmAttack`) plays it as sample 8, and nothing can ever reach sample 9
/// or the `cheap1b` file.
#[rustfmt::skip]
pub const SAMPLES: [&str; 49] = [
    "baland",  "balshake", "beast1",  "bloodsp",  "bonfire2", "camel3b",  "camel4",
    "camel7",  "cheap2b",  "cheap2b", "chomps",   "chomps2",  "dagmuf3",  "dragnr",
    "dragru",  "femscr",   "grnt1",   "grnt3",    "grnt3b",   "headchop", "hedland",
    "hit3",    "kstep",    "lion1c1", "lion2c1",  "mud2",     "mudfal",   "mudhit",
    "mudvox",  "newkrush", "nitland", "pophed12", "ratgoug",  "ratleap",  "ratmpal",
    "ratscr",  "ratsku",   "rjgrunt", "rjgrunt1", "rjgrunt2", "rjgrunt3", "rjgrunt4",
    "scrape",  "slice3",   "spear1",  "stretch",  "swish",    "swordcl",  "wipcrak",
];

/// The sampled half of `PLAY_SFX`'s translation: the table at DS:`0x7e6e`,
/// `xlatb`'d with the sound id, giving an index into [`SAMPLES`].
///
/// The table in the image is 364 bytes, which is the distance to the next object
/// in the data segment; the first 256 of them are here, because an id is a byte
/// and cannot reach past them. Only 0x00 to 0x6c are meant: from 0x6d to the end
/// the table is filled with `0x15`, and five ids the scripts actually use
/// (0x6d, 0x87, 0x8f, 0x92, 0x93, among them the eight in the beast's scripts)
/// land in that filler and play `hit3`. Quoted as it is rather than trimmed,
/// since the filler is what those ids do.
#[rustfmt::skip]
pub const SAMPLE_OF: [u8; 256] = [
    0x00, 0x16, 0x16, 0x16, 0x10, 0x11, 0x12, 0x10, 0x11, 0x13, 0x15, 0x2e, 0x2e, 0x2e, 0x15, 0x15,  // 0x00
    0x14, 0x2f, 0x2f, 0x29, 0x0e, 0x0e, 0x0e, 0x0e, 0x0a, 0x0b, 0x04, 0x0d, 0x28, 0x04, 0x25, 0x26,  // 0x10
    0x27, 0x15, 0x17, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x1f, 0x2e, 0x00, 0x00, 0x00,  // 0x20
    0x01, 0x0a, 0x0a, 0x0a, 0x2b, 0x03, 0x2c, 0x2e, 0x05, 0x15, 0x06, 0x06, 0x07, 0x07, 0x30, 0x0c,  // 0x30
    0x0c, 0x0c, 0x0c, 0x0c, 0x0c, 0x02, 0x02, 0x02, 0x02, 0x0f, 0x0f, 0x0f, 0x0f, 0x09, 0x30, 0x19,  // 0x40
    0x1b, 0x1a, 0x1c, 0x1c, 0x1c, 0x2d, 0x08, 0x08, 0x08, 0x1d, 0x23, 0x23, 0x24, 0x24, 0x20, 0x20,  // 0x50
    0x21, 0x21, 0x21, 0x21, 0x1d, 0x1d, 0x22, 0x22, 0x22, 0x15, 0x15, 0x2a, 0x2e, 0x15, 0x15, 0x15,  // 0x60
    0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15,  // 0x70
    0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15,  // 0x80
    0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15,  // 0x90
    0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15,  // 0xa0
    0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15,  // 0xb0
    0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15,  // 0xc0
    0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15,  // 0xd0
    0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15,  // 0xe0
    0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15, 0x15,  // 0xf0
];

/// `xlatb` through [`SAMPLE_OF`]: which of the 49 samples a sound id plays.
pub fn sample(id: u8) -> u8 {
    SAMPLE_OF[id as usize]
}

/// What that sample is called in `SBFileTable`, which is also the stem of the
/// file the baker wrote.
pub fn name(id: u8) -> &'static str {
    SAMPLES[sample(id) as usize]
}

/// The asset id for a sound id, for a [`crate::Sink`].
pub fn asset(id: u8) -> String {
    format!("sfx.{}", name(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// Spot checks across the table, each one a sound whose script says plainly
    /// what it should be: the knight's swing, a blow landing, the balok coming
    /// down, the dragon's breath, a thrown spear, a whip.
    #[test]
    fn an_id_resolves_to_the_sample_its_script_means() {
        assert_eq!(name(0x0b), "swish"); // Knight_SwSwing, and SpeedKnife
        assert_eq!(name(0x12), "swordcl"); // thirteen uses, every one a blow
        assert_eq!(name(0x09), "headchop"); // Knight_SwDeCap
        assert_eq!(name(0x10), "hedland"); // and the head coming down
        assert_eq!(name(0x2d), "baland"); // Balok_Dead, and BigLandAudio
        assert_eq!(name(0x1d), "bonfire2"); // Dragon_HighBreath, DrBreathSnd
        assert_eq!(name(0x36), "spear1"); // TroggSpear_Toss
        assert_eq!(name(0x4e), "wipcrak"); // Demon_Slap
        assert_eq!(name(0x50), "mudhit"); // AddMudSound, off Mudmen_Stance
        assert_eq!(name(0x51), "mudfal"); // and Mudmen_Hit is the fall
        assert_eq!(name(0x31), "chomps"); // Balok_BiteKnight
    }

    /// The ids past the end of the meant part of the table, which five of the
    /// scripts' commands carry, play the filler rather than nothing.
    #[test]
    fn an_id_in_the_filler_plays_hit3() {
        for id in [0x6d, 0x87, 0x8f, 0x92, 0x93, 0xff] {
            assert_eq!(name(id), "hit3", "id {id:#04x}");
        }
        assert_eq!(sample(0x6c), 0x2e, "0x6c is the last one meant");
    }

    /// 49 names, 48 of them distinct, and the duplicate is where the image puts
    /// it. This is the check that the table was not quietly tidied.
    #[test]
    fn the_sample_table_is_the_shipped_one() {
        assert_eq!(SAMPLES.len(), 49);
        assert_eq!(SAMPLES[8], "cheap2b");
        assert_eq!(SAMPLES[9], "cheap2b");
        let distinct: BTreeSet<&str> = SAMPLES.iter().copied().collect();
        assert_eq!(distinct.len(), 48);
        assert!(!SAMPLES.contains(&"cheap1b"));
        // Sorted, because the loader walked the directory in order.
        let mut sorted = SAMPLES;
        sorted.sort_unstable();
        assert_eq!(sorted, SAMPLES);
    }

    /// Every sample a shipped script can ask for is one the baker writes, and
    /// an asset id is what the sink is handed.
    #[test]
    fn an_id_becomes_an_asset_id() {
        assert_eq!(asset(0x0b), "sfx.swish");
        assert_eq!(asset(0x2d), "sfx.baland");
    }
}
