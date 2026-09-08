//! The original's tunes, and a small synthesiser to play them on.
//!
//! # Where the notes come from
//!
//! `xTUNEn.BIN` is not a music format. It is a **relocatable x86 driver with
//! the song welded into it**: `MAIN.EXE`'s `LOADMUSIC` reads one to a fixed
//! segment, `Install_Timer` points `int 60h` at it, and the timer handler calls
//! it with `ah = 1` every tick. Eighteen of them ship, which is `MusicTable`'s
//! six tunes by three sound cards, and the letter says which card: `a` is an
//! AdLib, `b` is the PC speaker, `r` is a Roland on an MPU-401.
//!
//! The Roland one turned out to be **plain MIDI**. So the tunes were recovered
//! by running the game's own driver under emulation and writing down what it
//! sent down the wire: `tools/tunes.py`, which is the same 8086 harness the
//! executable was unpacked with. That gives note, channel, velocity, start and
//! length on the driver's own tick, which is the game's timer at
//! 1193182 / 0x5555 Hz. Nothing here is transcribed by ear or invented.
//!
//! # What is ours
//!
//! **The sound.** The recovered stream is MIDI, so it names a Roland's
//! instrument numbers and nothing else; how those actually sounded belonged to
//! a synthesiser this project does not have and will not pretend to. So the
//! notes are the original's and the voices are ours: a handful of wavetables
//! and envelopes chosen by instrument family, and a noise burst for the
//! percussion channel. It is a plain synthesiser playing Moonstone's own music,
//! and it is labelled that way rather than passed off as a recording.

use serde::{Deserialize, Serialize};

/// Samples a second the tunes are rendered at. The original's own effects were
/// sampled at 16 kHz, and a tune wants a little more room than that.
pub const RATE: u32 = 22_050;

/// One note: start tick, length in ticks, channel, note number, velocity.
///
/// A tuple because that is what the recovered file holds, one short array per
/// note; a tune is thousands of them and the field names would be most of it.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Note(pub u32, pub u32, pub u8, pub u8, pub u8);

impl Note {
    pub fn start(self) -> u32 {
        self.0
    }
    pub fn ticks(self) -> u32 {
        self.1
    }
    pub fn channel(self) -> u8 {
        self.2
    }
    pub fn note(self) -> u8 {
        self.3
    }
    pub fn velocity(self) -> u8 {
        self.4
    }
    /// Equal temperament on A440, which is what a MIDI note number means.
    pub fn hz(self) -> f32 {
        440.0 * (2.0f32).powf((self.3 as f32 - 69.0) / 12.0)
    }
}

/// A program change: tick, channel, program.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Program(pub u32, pub u8, pub u8);

/// One tune, on the driver's own tick. Self contained, because one of these is
/// one asset in a pack and an asset that needs another file to be read is a
/// trap for whoever replaces it.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Score {
    /// Which file it came out of, for the record.
    #[serde(default)]
    pub file: String,
    /// `Install_Timer` programs the 8253 with mode 3 and a divisor of 0x5555,
    /// so a tick is 1193182 / 21845 of a second: 54.62 Hz.
    #[serde(default = "default_num")]
    pub tick_hz_num: u32,
    #[serde(default = "default_den")]
    pub tick_hz_den: u32,
    /// The tick the tune comes round on, or its whole length if it never does.
    pub loop_ticks: u32,
    /// Whether that tick is a real loop point or just where the capture stopped.
    #[serde(default)]
    pub looping: bool,
    #[serde(default)]
    pub programs: Vec<Program>,
    #[serde(default)]
    pub notes: Vec<Note>,
}

fn default_num() -> u32 {
    1_193_182
}

fn default_den() -> u32 {
    0x5555
}

impl Default for Score {
    fn default() -> Score {
        Score {
            file: String::new(),
            tick_hz_num: default_num(),
            tick_hz_den: default_den(),
            loop_ticks: 0,
            looping: false,
            programs: Vec::new(),
            notes: Vec::new(),
        }
    }
}

impl Score {
    /// Seconds in one tick.
    pub fn tick_seconds(&self) -> f64 {
        if self.tick_hz_num == 0 {
            return 0.0;
        }
        self.tick_hz_den as f64 / self.tick_hz_num as f64
    }

    /// How long one turn of the tune lasts.
    pub fn seconds(&self) -> f64 {
        self.loop_ticks as f64 * self.tick_seconds()
    }

    pub fn parse(text: &str) -> Option<Score> {
        serde_json::from_str(text).ok()
    }
}

/// Every tune, and the tick rate they share.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Tunes {
    #[serde(default)]
    pub source: String,
    /// `Install_Timer` programs the 8253 with mode 3 and a divisor of 0x5555,
    /// so a tick is 1193182 / 21845 of a second: 54.62 Hz.
    pub tick_hz_num: u32,
    pub tick_hz_den: u32,
    pub tunes: std::collections::BTreeMap<String, Score>,
}

impl Default for Tunes {
    fn default() -> Tunes {
        Tunes {
            source: String::new(),
            tick_hz_num: 1_193_182,
            tick_hz_den: 0x5555,
            tunes: Default::default(),
        }
    }
}

impl Tunes {
    /// Seconds in one tick.
    pub fn tick_seconds(&self) -> f64 {
        if self.tick_hz_num == 0 {
            return 0.0;
        }
        self.tick_hz_den as f64 / self.tick_hz_num as f64
    }

    /// The tunes as standalone scores, each carrying the tick rate, which is
    /// how one of them becomes one asset in a pack.
    pub fn split(&self) -> Vec<(String, Score)> {
        self.tunes
            .iter()
            .map(|(name, s)| {
                let mut s = s.clone();
                s.tick_hz_num = self.tick_hz_num;
                s.tick_hz_den = self.tick_hz_den;
                (name.clone(), s)
            })
            .collect()
    }
}

/// The voice a program number gets. **Ours**: the recovered stream names a
/// Roland's instruments, and these are what this synthesiser has instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Voice {
    /// Struck and plucked: a bright wave that decays away on its own.
    Plucked,
    /// Bass. Nearly a sine, so it carries without muddying anything.
    Bass,
    /// Bowed and blown: comes in slowly and holds.
    Sustained,
    /// Reeds, brass and leads: a hollow wave that holds.
    Reed,
    /// Channel ten, whatever the program says. Noise, pitched by note number.
    Percussion,
}

impl Voice {
    /// General MIDI's families, which is the only thing a program number can be
    /// read as without the synthesiser it was written for.
    pub fn of(channel: u8, program: u8) -> Voice {
        if channel == 9 {
            return Voice::Percussion;
        }
        match program {
            0..=31 => Voice::Plucked,      // piano, chromatic percussion, organ, guitar
            32..=39 => Voice::Bass,
            40..=55 => Voice::Sustained,   // strings and ensembles
            56..=87 => Voice::Reed,        // brass, reed, pipe, lead
            88..=103 => Voice::Sustained,  // pads and effects
            _ => Voice::Plucked,           // ethnic, percussive, sound effects
        }
    }

    /// Attack, decay and release, in seconds, and the level held in between.
    fn envelope(self) -> (f32, f32, f32, f32) {
        match self {
            Voice::Plucked => (0.004, 0.35, 0.0, 0.06),
            Voice::Bass => (0.006, 0.20, 0.55, 0.05),
            Voice::Sustained => (0.09, 0.10, 0.85, 0.12),
            Voice::Reed => (0.02, 0.06, 0.80, 0.06),
            Voice::Percussion => (0.001, 0.09, 0.0, 0.02),
        }
    }

    /// How loud this family sits in the mix, before velocity.
    fn gain(self) -> f32 {
        match self {
            Voice::Plucked => 0.9,
            Voice::Bass => 1.0,
            Voice::Sustained => 0.65,
            Voice::Reed => 0.7,
            Voice::Percussion => 0.8,
        }
    }
}

/// One cycle of a wave, built from harmonics so that it has no step in it.
///
/// Band limited by construction: a fixed harmonic count rather than an ideal
/// saw or square, which is what keeps a high note from turning into a hiss.
const TABLE: usize = 1024;

fn wavetable(voice: Voice) -> Vec<f32> {
    // Harmonic amplitudes. A saw is 1/k, a square is 1/k over odd k only, and
    // a bass is most of the way to a sine.
    let harmonics: Vec<f32> = match voice {
        Voice::Bass => (1..=4).map(|k| if k == 1 { 1.0 } else { 0.12 / k as f32 }).collect(),
        Voice::Plucked => (1..=12).map(|k| 1.0 / k as f32).collect(),
        Voice::Sustained => (1..=10).map(|k| 1.0 / k as f32).collect(),
        Voice::Reed => (1..=11)
            .map(|k| if k % 2 == 1 { 1.0 / k as f32 } else { 0.0 })
            .collect(),
        Voice::Percussion => vec![1.0],
    };
    let norm: f32 = harmonics.iter().sum::<f32>().max(1e-6);
    (0..TABLE)
        .map(|i| {
            let p = i as f32 / TABLE as f32 * std::f32::consts::TAU;
            harmonics
                .iter()
                .enumerate()
                .map(|(k, a)| a * (p * (k + 1) as f32).sin())
                .sum::<f32>()
                / norm
        })
        .collect()
}

/// A deterministic noise source. Seeded, and the same on every machine, because
/// a rendered tune that differed between two runs would be a poor thing to hash
/// or to compare.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        // xorshift32.
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 >> 8) as f32 / 8_388_608.0 - 1.0
    }
}

/// Renders one tune to mono 16-bit samples at [`RATE`].
///
/// A tune that loops is rendered to exactly one turn of the loop, and a note
/// whose tail runs past the end wraps round to the beginning, so the seam is
/// silent rather than a click.
pub fn render(score: &Score) -> Vec<i16> {
    render_at(score, RATE)
}

pub fn render_at(score: &Score, rate: u32) -> Vec<i16> {
    let secs_per_tick = score.tick_seconds();
    let total = ((score.loop_ticks as f64 * secs_per_tick * rate as f64).round() as usize).max(1);
    if score.notes.is_empty() {
        return vec![0; total];
    }
    let mut buf = vec![0f32; total];
    let tables: Vec<Vec<f32>> = [
        Voice::Plucked, Voice::Bass, Voice::Sustained, Voice::Reed, Voice::Percussion,
    ]
    .iter()
    .map(|v| wavetable(*v))
    .collect();
    let table_of = |v: Voice| match v {
        Voice::Plucked => 0,
        Voice::Bass => 1,
        Voice::Sustained => 2,
        Voice::Reed => 3,
        Voice::Percussion => 4,
    };
    let mut noise = Noise(0x1379_2531);

    for n in &score.notes {
        let program = program_at(score, n.channel(), n.start());
        let voice = Voice::of(n.channel(), program);
        let (attack, decay, sustain, release) = voice.envelope();
        let held = (n.ticks() as f64 * secs_per_tick * rate as f64) as usize;
        let tail = (release * rate as f32) as usize;
        let start = (n.start() as f64 * secs_per_tick * rate as f64) as usize;
        let amp = voice.gain() * (n.velocity() as f32 / 127.0).powf(1.4) * 0.22;
        let step = n.hz() / rate as f32 * TABLE as f32;
        let table = &tables[table_of(voice)];
        let mut phase = 0f32;
        let (a, d) = ((attack * rate as f32) as usize, (decay * rate as f32) as usize);

        for i in 0..held + tail {
            let env = if i < a {
                i as f32 / a.max(1) as f32
            } else if i < a + d {
                let t = (i - a) as f32 / d.max(1) as f32;
                1.0 - t * (1.0 - sustain)
            } else if i < held {
                sustain
            } else {
                let t = (i - held) as f32 / tail.max(1) as f32;
                let from = if held > a + d { sustain } else { 1.0 };
                from * (1.0 - t)
            };
            // A plucked voice has no sustain, so once its decay is spent there
            // is nothing left to write and the rest of the note is silence.
            if env <= 0.0 && i >= a + d {
                break;
            }
            let s = if voice == Voice::Percussion {
                // Pitched noise: low notes get a body, high ones a tick.
                let body = (1.0 - (n.note() as f32 / 100.0)).clamp(0.15, 1.0);
                noise.next() * body + table[(phase as usize) & (TABLE - 1)] * (1.0 - body) * 0.4
            } else {
                let idx = phase as usize & (TABLE - 1);
                let frac = phase - phase.floor();
                let a0 = table[idx];
                let a1 = table[(idx + 1) & (TABLE - 1)];
                a0 + (a1 - a0) * frac
            };
            phase += step;
            if phase >= TABLE as f32 {
                phase -= TABLE as f32;
            }
            // Wrapped, so a tail that runs off the end of a loop lands back at
            // the beginning instead of being cut off.
            let at = start + i;
            let at = if score.looping { at % total } else { at };
            if at >= total {
                break;
            }
            buf[at] += s * env * amp;
        }
    }

    // Levelled so that six tunes written for six different rooms are the same
    // loudness as each other, then bent rather than clipped at the top: a soft
    // knee, so a busy bar squashes instead of tearing.
    let peak = buf.iter().fold(0f32, |m, v| m.max(v.abs())).max(1e-6);
    // Capped, so that a tune which is nearly silence is not amplified into
    // hiss. Sixteen is loose enough to level every one of the six and still
    // bounded, so a near-empty score stays quiet.
    let scale = (0.85 / peak).min(16.0);
    buf.iter()
        .map(|v| {
            let x = (v * scale).clamp(-1.0, 1.0);
            let y = x - x * x * x / 3.0;
            (y * 0.94 * 32767.0) as i16
        })
        .collect()
}

/// The program in force on a channel at a tick. Zero until something says
/// otherwise, which is what a MIDI channel with no program change is.
fn program_at(score: &Score, channel: u8, tick: u32) -> u8 {
    score
        .programs
        .iter()
        .filter(|p| p.1 == channel && p.0 <= tick)
        .next_back()
        .map_or(0, |p| p.2)
}

/// A 16-bit mono WAV, for writing a tune out and looking at it.
pub fn wav(samples: &[i16], rate: u32) -> Vec<u8> {
    let data = samples.len() * 2;
    let mut out = Vec::with_capacity(44 + data);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&((36 + data) as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data as u32).to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(note: u8, start: u32, ticks: u32) -> Score {
        Score {
            file: "test".into(),
            loop_ticks: start + ticks + 20,
            looping: true,
            programs: vec![Program(0, 1, 48)],
            notes: vec![Note(start, ticks, 1, note, 100)],
            ..Default::default()
        }
    }

    #[test]
    fn a_note_number_is_equal_temperament_on_a440() {
        assert!((Note(0, 1, 0, 69, 64).hz() - 440.0).abs() < 0.01);
        assert!((Note(0, 1, 0, 81, 64).hz() - 880.0).abs() < 0.01);
        assert!((Note(0, 1, 0, 57, 64).hz() - 220.0).abs() < 0.01);
        // The range the recovered tunes actually use.
        assert!((Note(0, 1, 0, 24, 64).hz() - 32.70).abs() < 0.01);
        assert!((Note(0, 1, 0, 104, 64).hz() - 3322.44).abs() < 0.01);
    }

    #[test]
    fn the_tick_is_the_games_own_timer() {
        let hz = 1.0 / Score::default().tick_seconds();
        // `Install_Timer`: mode 3, divisor 0x5555.
        assert!((hz - 54.62).abs() < 0.01, "tick was {hz} Hz");
    }

    #[test]
    fn the_length_rendered_is_the_length_the_tune_says() {
        let s = one(60, 0, 100);
        let pcm = render(&s);
        let want = (s.loop_ticks as f64 * s.tick_seconds() * RATE as f64).round() as usize;
        assert_eq!(pcm.len(), want);
        // 120 ticks is a shade over two seconds.
        assert!((pcm.len() as f64 / RATE as f64 - 2.196).abs() < 0.01);
    }

    #[test]
    fn a_note_puts_sound_where_it_starts_and_silence_before_it() {
        let s = one(60, 40, 40);
        let pcm = render(&s);
        let secs = Score::default().tick_seconds();
        let at = |tick: u32| (tick as f64 * secs * RATE as f64) as usize;
        let rms = |a: usize, b: usize| {
            let n = (b - a) as f64;
            (pcm[a..b].iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / n).sqrt()
        };
        // Nothing before it, something during it.
        assert!(rms(0, at(35)) < 1.0, "sound before the note");
        assert!(rms(at(45), at(75)) > 300.0, "the note is inaudible");
    }

    #[test]
    fn every_family_makes_a_sound_and_none_of_them_clips() {
        for program in [0u8, 34, 48, 60, 90, 117] {
            let mut s = one(64, 0, 60);
            s.programs = vec![Program(0, 1, program)];
            let pcm = render(&s);
            let peak = pcm.iter().map(|v| v.unsigned_abs()).max().unwrap_or(0);
            assert!(peak > 500, "program {program} was silent");
            assert!(peak < 32_700, "program {program} clipped at {peak}");
        }
        // And the drum channel, whatever program it is on.
        let mut s = one(38, 0, 8);
        s.notes[0].2 = 9;
        let pcm = render(&s);
        assert!(pcm.iter().map(|v| v.unsigned_abs()).max().unwrap_or(0) > 500);
    }

    #[test]
    fn rendering_is_deterministic_including_the_percussion() {
        let mut s = one(40, 0, 20);
        s.notes[0].2 = 9;
        s.notes.push(Note(10, 20, 9, 60, 90));
        assert_eq!(render(&s), render(&s));
    }

    #[test]
    fn a_looping_tune_has_no_silent_seam() {
        // A note that ends exactly on the loop point: its release has to wrap.
        let s = Score {
            file: "seam".into(),
            loop_ticks: 60,
            looping: true,
            programs: vec![Program(0, 1, 48)],
            notes: vec![Note(0, 60, 1, 60, 100)],
            ..Default::default()
        };
        let pcm = render(&s);
        let head: i32 = pcm[..200].iter().map(|v| (*v as i32).abs()).max().unwrap_or(0);
        assert!(head > 100, "the wrapped tail did not land at the start");
    }

    #[test]
    fn the_voice_families_are_the_general_midi_ones() {
        assert_eq!(Voice::of(0, 0), Voice::Plucked);
        assert_eq!(Voice::of(0, 34), Voice::Bass);
        assert_eq!(Voice::of(0, 48), Voice::Sustained);
        assert_eq!(Voice::of(0, 63), Voice::Reed);
        assert_eq!(Voice::of(0, 90), Voice::Sustained);
        assert_eq!(Voice::of(0, 117), Voice::Plucked);
        // Channel ten is percussion whatever the program says.
        assert_eq!(Voice::of(9, 48), Voice::Percussion);
    }

    #[test]
    fn a_program_change_takes_effect_from_its_own_tick_and_not_before() {
        let s = Score {
            programs: vec![Program(0, 1, 10), Program(50, 1, 40), Program(0, 2, 99)],
            ..Default::default()
        };
        assert_eq!(program_at(&s, 1, 0), 10);
        assert_eq!(program_at(&s, 1, 49), 10);
        assert_eq!(program_at(&s, 1, 50), 40);
        assert_eq!(program_at(&s, 2, 60), 99);
        // A channel nothing ever set is program zero, as a MIDI channel is.
        assert_eq!(program_at(&s, 3, 60), 0);
    }

    #[test]
    fn a_score_is_data_and_survives_the_round_trip() {
        let s = one(60, 4, 30);
        let text = serde_json::to_string(&s).expect("a score serialises");
        // The compact array form is what the recovered file holds.
        assert!(text.contains("[4,30,1,60,100]"), "{text}");
        let back: Score = serde_json::from_str(&text).expect("a score deserialises");
        assert_eq!(back.notes, s.notes);
        assert_eq!(back.loop_ticks, s.loop_ticks);
    }

    #[test]
    fn tunes_come_out_at_the_same_loudness_as_each_other() {
        // Two tunes written at different velocities should still arrive at the
        // same level, because they play in different rooms and nobody reaches
        // for a volume knob on the way through a door.
        let chord = |vel: u8| Score {
            loop_ticks: 240,
            looping: true,
            programs: vec![Program(0, 1, 48)],
            notes: [59u8, 62, 66, 71]
                .iter()
                .map(|n| Note(0, 200, 1, *n, vel))
                .collect(),
            ..Default::default()
        };
        let peak = |s: &Score| render(s).iter().map(|v| v.unsigned_abs()).max().unwrap_or(0);
        let (q, l) = (peak(&chord(56)), peak(&chord(120)));
        assert!(q > 15_000 && l > 15_000, "quiet {q}, loud {l}");
        assert!(q.max(l) as f32 / (q.min(l) as f32) < 1.1, "quiet {q}, loud {l}");
        // And neither of them anywhere near the top of the scale.
        assert!(q.max(l) < 28_000, "levelled to {}", q.max(l));
    }

    #[test]
    fn something_barely_there_is_not_amplified_into_hiss() {
        // The levelling is capped, so a tune that is almost silence stays
        // almost silence rather than being pulled up to full scale.
        let mut faint = one(60, 0, 200);
        faint.notes[0].4 = 3;
        let peak = render(&faint).iter().map(|v| v.unsigned_abs()).max().unwrap_or(0);
        assert!(peak < 4_000, "a whisper came out at {peak}");
    }

    #[test]
    fn an_empty_tune_renders_to_its_own_length_of_silence() {
        let s = Score { loop_ticks: 55, ..Default::default() };
        let pcm = render(&s);
        assert!(!pcm.is_empty());
        assert!(pcm.iter().all(|v| *v == 0));
    }

    #[test]
    fn a_wav_header_says_what_the_samples_are() {
        let w = wav(&[0, 1, -1, 2], 22_050);
        assert_eq!(&w[..4], b"RIFF");
        assert_eq!(&w[8..12], b"WAVE");
        assert_eq!(u32::from_le_bytes([w[24], w[25], w[26], w[27]]), 22_050);
        assert_eq!(u32::from_le_bytes([w[40], w[41], w[42], w[43]]), 8);
        assert_eq!(w.len(), 44 + 8);
    }
}
