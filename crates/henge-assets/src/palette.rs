//! Palette fades, colour cycling and colour glows, as the original does them.
//!
//! All four routines are recovered rather than designed. The original keeps a
//! live palette of **32 twelve-bit colours** (`0x0RGB`, one nibble a channel,
//! the Amiga word the artwork was authored in) and hands it to the VGA DAC as
//! 96 bytes of `nibble << 2`. Three things animate it:
//!
//! * **`COLCON`**, queued on the frame list by `ADDCOL`, walks two tables once
//!   a frame: six [`Cycle`] slots and six [`Glow`] slots. If either changed
//!   anything it reconverts the palette and reloads the DAC.
//! * **`COLOURCYCLE`** installs a cycle: a first and last index, a direction
//!   and a period. Every `period` frames the entries in that span rotate by
//!   one, which is how water moves.
//! * **`COLOURGLOW`** installs a glow: one index walks one step a channel
//!   towards a target colour every `period` frames, and when it arrives the
//!   target and the colour it started from swap, so it breathes. `DYNAMIC` is
//!   the step, and it steps red, green and blue independently.
//!
//! Fades are a separate pair, `FADEPALETTEIN` and `FADEPALETTEOUT`, and both
//! are **sixteen steps, one a frame, linear**. Fade in accumulates
//! `target * 16` into a 16.8 fixed-point channel and shows the high byte, so
//! step *k* of sixteen shows *k*/16 of the picture; fade out reads the DAC back
//! and subtracts its way to black. Every screen in the game is a fade out, a
//! load, and a fade in.
//!
//! None of this touches the framebuffer. It is applied to the palette on its
//! way to the screen, which is the whole reason the framebuffer is indexed.

use serde::{Deserialize, Serialize};

/// Colours in a Moonstone palette. Five bitplanes, so 32.
pub const ENTRIES: usize = 32;
/// Cycle and glow slots. Six each, which is the size of `CYCLES` and `GLOWS`.
pub const SLOTS: usize = 6;
/// Steps in a fade, one a frame. `mov cx, 0x10` in both fade routines.
pub const FADE_STEPS: u16 = 16;

/// 0xRRGGBB to the original's `0x0RGB`.
///
/// Every palette this engine loads came out of a PIV or CMP whose entries are
/// four bits a channel widened by seventeen, so this is exact for anything the
/// game itself drew and the nearest nibble for anything else.
pub fn to12(c: u32) -> u16 {
    let ch = |v: u32| (((v & 0xff) * 15 + 127) / 255) as u16;
    (ch(c >> 16) << 8) | (ch(c >> 8) << 4) | ch(c)
}

/// The original's `0x0RGB` back to 0xRRGGBB, by the same widening the decoders use.
pub fn from12(w: u16) -> u32 {
    let ch = |v: u16| ((v & 0xf) as u32) * 17;
    (ch(w >> 8) << 16) | (ch(w >> 4) << 8) | ch(w)
}

/// One step of `DYNAMIC`: `cur` moves one towards `target` in each channel.
///
/// The original does this on the packed word, adding or subtracting 1, 0x10 and
/// 0x100, which cannot carry between channels because it stops on equality.
pub fn dynamic(cur: u16, target: u16) -> u16 {
    let mut out = cur;
    for shift in [0u16, 4, 8] {
        let c = (cur >> shift) & 0xf;
        let t = (target >> shift) & 0xf;
        if c < t {
            out = out.wrapping_add(1 << shift);
        } else if c > t {
            out = out.wrapping_sub(1 << shift);
        }
    }
    out
}

/// A `CYCLES` record: `first`, `last`, direction and period, in frames.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cycle {
    /// First palette index in the span.
    pub first: u8,
    /// Last palette index in the span, inclusive.
    pub last: u8,
    /// The original's third byte. False rotates towards the first index
    /// (`LOOPY`), true towards the last (`POS`).
    #[serde(default)]
    pub up: bool,
    /// Frames between rotations.
    pub period: u8,
}

/// A `GLOWS` record. `repeat` of zero is the original's "forever".
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glow {
    /// The palette index that breathes.
    pub index: u8,
    /// The colour it walks towards, as `0x0RGB`.
    pub target: u16,
    /// Frames between steps.
    pub period: u16,
    /// How many arrivals before the slot frees itself. Zero never stops.
    #[serde(default)]
    pub repeat: u16,
}

/// What one screen installs. Named by scene so it can live in a pack.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SceneEffects {
    #[serde(default)]
    pub cycles: Vec<Cycle>,
    #[serde(default)]
    pub glows: Vec<Glow>,
}

/// The whole table, keyed by the scene id the desktop uses.
pub type EffectTable = std::collections::BTreeMap<String, SceneEffects>;

#[derive(Clone, Copy, Debug)]
struct CycleState {
    def: Cycle,
    count: u8,
    /// How far the span has rotated. Kept rather than mutating the palette,
    /// because the scene reloads its base palette every frame.
    step: u32,
}

#[derive(Clone, Copy, Debug)]
struct GlowState {
    def: Glow,
    /// Where the colour is now, as `0x0RGB`. `COLOURGLOW` seeds it from the
    /// live palette entry, and so does this.
    cur: u16,
    target: u16,
    other: u16,
    count: u16,
    left: u16,
    done: bool,
    /// The entry this slot was seeded from. The original installs a glow after
    /// the screen's palette is loaded and never moves it afterwards; here the
    /// screen reloads its palette as it draws, so a slot whose entry no longer
    /// matches what it was seeded with is seeded again. See [`Effects::reseed`].
    origin: u16,
}

/// Which way a fade is going, and how far through it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fade {
    /// Fully lit. What a screen with no transition on it is.
    None,
    /// Coming up from black. `step` counts 0 to [`FADE_STEPS`].
    In(u16),
    /// Going down to black.
    Out(u16),
    /// Held black, between a fade out and whatever comes next.
    Black,
}

impl Fade {
    /// The fraction of the palette showing, as sixteenths.
    pub fn level(self) -> u16 {
        match self {
            Fade::None => FADE_STEPS,
            Fade::Black => 0,
            Fade::In(s) => s.min(FADE_STEPS),
            Fade::Out(s) => FADE_STEPS.saturating_sub(s),
        }
    }

    /// True once the fade has run its sixteen frames.
    pub fn finished(self) -> bool {
        match self {
            Fade::None | Fade::Black => true,
            Fade::In(s) | Fade::Out(s) => s >= FADE_STEPS,
        }
    }

    fn advance(self) -> Fade {
        match self {
            Fade::In(s) if s < FADE_STEPS => Fade::In(s + 1),
            Fade::Out(s) if s < FADE_STEPS => Fade::Out(s + 1),
            other => other,
        }
    }
}

/// `COLCON`'s two tables and the fade, ticked once a frame.
#[derive(Clone, Debug)]
pub struct Effects {
    cycles: Vec<CycleState>,
    glows: Vec<GlowState>,
    fade: Fade,
}

impl Default for Effects {
    fn default() -> Effects {
        Effects {
            cycles: Vec::new(),
            glows: Vec::new(),
            fade: Fade::None,
        }
    }
}

impl Effects {
    pub fn new() -> Effects {
        Effects::default()
    }

    /// Empties both tables, which is what leaving a screen does: `MapEffects`
    /// frees its glow and its cycle by writing zero into the slot.
    pub fn clear(&mut self) {
        self.cycles.clear();
        self.glows.clear();
    }

    /// `COLOURCYCLE`. Silently ignored past six, as the original's own search
    /// for a free slot is.
    pub fn install_cycle(&mut self, c: Cycle) {
        if self.cycles.len() >= SLOTS || c.period == 0 || c.last <= c.first {
            return;
        }
        self.cycles.push(CycleState {
            def: c,
            count: c.period,
            step: 0,
        });
    }

    /// `COLOURGLOW`. `base` is the live palette, which the original reads
    /// through `PALLOC` to seed the slot's starting colour.
    pub fn install_glow(&mut self, g: Glow, base: &[u32; ENTRIES]) {
        if self.glows.len() >= SLOTS || g.period == 0 || g.index as usize >= ENTRIES {
            return;
        }
        let cur = to12(base[g.index as usize]);
        self.glows.push(GlowState {
            def: g,
            cur,
            target: g.target,
            other: cur,
            count: g.period,
            left: g.repeat,
            done: false,
            origin: cur,
        });
    }

    /// Frees every glow on one entry, which is what writing zero into a
    /// glow's handle does: `KnightGlowOff` and `MudmenGlowOff` both end that
    /// way, and nothing else in the game takes a glow out early.
    pub fn remove_glow(&mut self, index: u8) {
        self.glows.retain(|g| g.def.index != index);
    }

    /// Installs everything one scene asks for, after clearing what the last one had.
    pub fn install(&mut self, fx: &SceneEffects, base: &[u32; ENTRIES]) {
        self.clear();
        for c in &fx.cycles {
            self.install_cycle(*c);
        }
        for g in &fx.glows {
            self.install_glow(*g, base);
        }
    }

    /// Seeds any glow whose palette entry is not the one it started from.
    ///
    /// Call it once the screen has loaded its own palette. Without it a glow
    /// installed a frame before the picture it belongs to would breathe
    /// between the wrong two colours for the rest of the scene.
    pub fn reseed(&mut self, base: &[u32; ENTRIES]) {
        for g in &mut self.glows {
            let want = to12(base[g.def.index as usize]);
            if want != g.origin {
                g.origin = want;
                g.cur = want;
                g.other = want;
                g.target = g.def.target;
                g.count = g.def.period;
                g.left = g.def.repeat;
                g.done = false;
            }
        }
    }

    pub fn fade(&self) -> Fade {
        self.fade
    }

    pub fn set_fade(&mut self, f: Fade) {
        self.fade = f;
    }

    pub fn cycles(&self) -> usize {
        self.cycles.len()
    }

    pub fn glows(&self) -> usize {
        self.glows.iter().filter(|g| !g.done).count()
    }

    /// One frame of `COLCON`, and one step of whichever fade is running.
    pub fn tick(&mut self) {
        for c in &mut self.cycles {
            c.count -= 1;
            if c.count == 0 {
                c.count = c.def.period;
                c.step = c.step.wrapping_add(1);
            }
        }
        for g in &mut self.glows {
            if g.done {
                continue;
            }
            g.count -= 1;
            if g.count != 0 {
                continue;
            }
            g.count = g.def.period;
            g.cur = dynamic(g.cur, g.target);
            if g.cur == g.target {
                std::mem::swap(&mut g.target, &mut g.other);
                if g.left != 0 {
                    g.left -= 1;
                    if g.left == 0 {
                        g.done = true;
                    }
                }
            }
        }
        self.fade = self.fade.advance();
    }

    /// The palette as it should reach the screen: cycled, glowing and faded.
    ///
    /// The base is passed in every frame rather than held, because each scene
    /// reloads its own palette as it draws. Cycling is therefore a rotation
    /// applied to the base rather than a rotation of it, which comes to the
    /// same picture and cannot drift.
    pub fn apply(&self, base: &[u32; ENTRIES]) -> [u32; ENTRIES] {
        let mut out = *base;
        for c in &self.cycles {
            let (first, last) = (c.def.first as usize, c.def.last as usize);
            if last >= ENTRIES || last <= first {
                continue;
            }
            let n = (last - first + 1) as u32;
            let k = c.step % n;
            for j in 0..n {
                let from = if c.def.up {
                    (j + n - k) % n
                } else {
                    (j + k) % n
                };
                out[first + j as usize] = base[first + from as usize];
            }
        }
        for g in &self.glows {
            if g.done {
                continue;
            }
            out[g.def.index as usize] = from12(g.cur);
        }
        let level = self.fade.level() as u32;
        if level < FADE_STEPS as u32 {
            for c in out.iter_mut() {
                let ch = |v: u32| (v & 0xff) * level / FADE_STEPS as u32;
                *c = (ch(*c >> 16) << 16) | (ch(*c >> 8) << 8) | ch(*c);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp() -> [u32; ENTRIES] {
        let mut p = [0u32; ENTRIES];
        for (i, c) in p.iter_mut().enumerate() {
            let v = (i as u32 % 16) * 17;
            *c = (v << 16) | (v << 8) | v;
        }
        p
    }

    #[test]
    fn twelve_bit_round_trips_through_the_widening_the_decoders_use() {
        for w in 0u16..0x1000 {
            assert_eq!(to12(from12(w)), w, "{w:#05x}");
        }
    }

    #[test]
    fn dynamic_steps_each_channel_by_one_and_stops_on_arrival() {
        // Red down, green up, blue already there.
        assert_eq!(dynamic(0x5a3, 0x2c3), 0x4b3);
        assert_eq!(dynamic(0x000, 0xfff), 0x111);
        assert_eq!(dynamic(0x777, 0x777), 0x777);
    }

    #[test]
    fn dynamic_reaches_its_target_and_never_overshoots() {
        let mut c = 0x08f;
        for _ in 0..64 {
            c = dynamic(c, 0xf10);
        }
        assert_eq!(c, 0xf10);
    }

    #[test]
    fn a_cycle_rotates_its_span_and_leaves_the_rest_alone() {
        let base = ramp();
        let mut fx = Effects::new();
        // The map's river: entries 0x15 to 0x17, upwards, every twelfth frame.
        fx.install_cycle(Cycle {
            first: 0x15,
            last: 0x17,
            up: true,
            period: 12,
        });
        assert_eq!(fx.apply(&base), base);
        for _ in 0..12 {
            fx.tick();
        }
        let out = fx.apply(&base);
        assert_eq!(out[0x15], base[0x17]);
        assert_eq!(out[0x16], base[0x15]);
        assert_eq!(out[0x17], base[0x16]);
        for i in 0..ENTRIES {
            if !(0x15..=0x17).contains(&i) {
                assert_eq!(out[i], base[i], "entry {i} moved");
            }
        }
    }

    /// `KnightGlowOn` puts the knight's three entries on three slots, and
    /// `KnightGlowOff` writes zero into each handle. Taking one out must leave
    /// the others, and the mudmen's, breathing.
    #[test]
    fn a_glow_can_be_taken_out_by_its_entry_and_the_rest_stay() {
        let base = ramp();
        let mut fx = Effects::new();
        for (index, target, period) in [
            (6u8, 0xfa0u16, 2u16),
            (7, 0xe70, 1),
            (8, 0xc50, 1),
            (14, 0x100, 2),
        ] {
            fx.install_glow(
                Glow {
                    index,
                    target,
                    period,
                    repeat: 0,
                },
                &base,
            );
        }
        assert_eq!(fx.glows(), 4);
        for _ in 0..4 {
            fx.tick();
        }
        fx.remove_glow(7);
        assert_eq!(fx.glows(), 3);
        let out = fx.apply(&base);
        assert_eq!(
            out[7], base[7],
            "a freed slot leaves its entry as the palette has it"
        );
        assert_ne!(out[6], base[6], "the entries still installed keep walking");
        assert_ne!(out[8], base[8]);
        assert_ne!(out[14], base[14]);
        fx.remove_glow(7);
        assert_eq!(
            fx.glows(),
            3,
            "freeing an entry with no glow on it is nothing"
        );
    }

    #[test]
    fn a_cycle_comes_back_to_where_it_started() {
        let base = ramp();
        let mut fx = Effects::new();
        fx.install_cycle(Cycle {
            first: 4,
            last: 7,
            up: false,
            period: 1,
        });
        for _ in 0..4 {
            fx.tick();
        }
        assert_eq!(fx.apply(&base), base);
    }

    #[test]
    fn the_two_directions_are_opposites() {
        let base = ramp();
        let (mut a, mut b) = (Effects::new(), Effects::new());
        a.install_cycle(Cycle {
            first: 2,
            last: 6,
            up: false,
            period: 1,
        });
        b.install_cycle(Cycle {
            first: 2,
            last: 6,
            up: true,
            period: 1,
        });
        a.tick();
        for _ in 0..4 {
            b.tick();
        }
        assert_eq!(a.apply(&base), b.apply(&base));
    }

    #[test]
    fn a_glow_breathes_between_its_own_colour_and_the_target() {
        let mut base = ramp();
        base[14] = from12(0x000);
        let mut fx = Effects::new();
        // The mudmen's: entry 14 towards 0x100 every other frame, forever.
        fx.install_glow(
            Glow {
                index: 14,
                target: 0x100,
                period: 2,
                repeat: 0,
            },
            &base,
        );
        assert_eq!(to12(fx.apply(&base)[14]), 0x000);
        fx.tick();
        fx.tick();
        assert_eq!(to12(fx.apply(&base)[14]), 0x100);
        // Arrived, so the target and the colour it came from have swapped.
        fx.tick();
        fx.tick();
        assert_eq!(to12(fx.apply(&base)[14]), 0x000);
        assert_eq!(fx.glows(), 1);
    }

    #[test]
    fn a_glow_seeded_before_its_screen_loaded_is_seeded_again() {
        let base = ramp();
        let mut fx = Effects::new();
        fx.install_glow(
            Glow {
                index: 5,
                target: 0xfff,
                period: 1,
                repeat: 0,
            },
            &[0u32; ENTRIES],
        );
        fx.tick();
        // Started from black, so one step towards white is 0x111.
        assert_eq!(to12(fx.apply(&[0u32; ENTRIES])[5]), 0x111);
        fx.reseed(&base);
        assert_eq!(fx.apply(&base)[5], base[5]);
        fx.tick();
        assert_eq!(to12(fx.apply(&base)[5]), dynamic(to12(base[5]), 0xfff));
    }

    #[test]
    fn a_glow_with_a_repeat_count_frees_its_slot() {
        let mut base = ramp();
        base[3] = from12(0x000);
        let mut fx = Effects::new();
        fx.install_glow(
            Glow {
                index: 3,
                target: 0x001,
                period: 1,
                repeat: 2,
            },
            &base,
        );
        for _ in 0..8 {
            fx.tick();
        }
        assert_eq!(fx.glows(), 0);
        assert_eq!(fx.apply(&base)[3], base[3]);
    }

    #[test]
    fn six_slots_each_and_no_more() {
        let base = ramp();
        let mut fx = Effects::new();
        for i in 0..10 {
            fx.install_cycle(Cycle {
                first: 0,
                last: 4,
                up: false,
                period: 1 + i,
            });
            fx.install_glow(
                Glow {
                    index: 1,
                    target: 0xfff,
                    period: 1,
                    repeat: 0,
                },
                &base,
            );
        }
        assert_eq!(fx.cycles(), SLOTS);
        assert_eq!(fx.glows(), SLOTS);
        fx.clear();
        assert_eq!(fx.cycles(), 0);
        assert_eq!(fx.glows(), 0);
    }

    #[test]
    fn a_fade_in_is_sixteen_linear_steps_from_black() {
        let base = ramp();
        let mut fx = Effects::new();
        fx.set_fade(Fade::In(0));
        assert_eq!(fx.apply(&base)[31], 0);
        let mut seen = Vec::new();
        for _ in 0..FADE_STEPS {
            fx.tick();
            seen.push(fx.apply(&base)[31] & 0xff);
        }
        assert!(fx.fade().finished());
        assert_eq!(seen.last(), Some(&(base[31] & 0xff)));
        // Monotone, and never brighter than the palette it is fading towards.
        for w in seen.windows(2) {
            assert!(w[1] >= w[0]);
        }
    }

    #[test]
    fn a_fade_out_ends_on_black() {
        let base = ramp();
        let mut fx = Effects::new();
        fx.set_fade(Fade::Out(0));
        assert_eq!(fx.apply(&base), base);
        for _ in 0..FADE_STEPS {
            fx.tick();
        }
        assert!(fx.fade().finished());
        assert_eq!(fx.apply(&base), [0u32; ENTRIES]);
    }

    #[test]
    fn a_fade_and_a_cycle_compose_without_either_being_lost() {
        let base = ramp();
        let mut fx = Effects::new();
        fx.install_cycle(Cycle {
            first: 0x15,
            last: 0x17,
            up: true,
            period: 1,
        });
        fx.set_fade(Fade::In(8));
        fx.tick();
        let out = fx.apply(&base);
        // Nine sixteenths lit, and rotated by one.
        let want = base[0x17] & 0xff;
        assert_eq!(out[0x15] & 0xff, want * 9 / 16);
    }
}
