//! Telling four identical knights apart, without adding a single colour.
//!
//! Arena palettes are fixed: 32 entries, taken from whichever backdrop is
//! loaded. There is no room to add a blue knight, so a recolour has to be a
//! *substitution* within the colours that are already there.
//!
//! The trick is to preserve luminance and move hue. Shading is carried almost
//! entirely by luminance, so a substitution that keeps brightness and changes
//! hue reads as the same armour in a different colour rather than as a smear.
//! Where a palette has no suitable hue, the entry is left alone, which is the
//! honest failure: a knight in a monochrome arena stays close to the original
//! instead of turning into noise.

/// A per-index substitution table. Index 0 is always transparent and never moved.
pub type Lut = [u8; 32];

pub const IDENTITY: Lut = {
    let mut l = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        l[i] = i as u8;
        i += 1;
    }
    l
};

fn luma(c: u32) -> f32 {
    let (r, g, b) = ((c >> 16 & 0xff) as f32, (c >> 8 & 0xff) as f32, (c & 0xff) as f32);
    0.299 * r + 0.587 * g + 0.114 * b
}

/// Hue in degrees, and saturation as 0..1. Greys have no meaningful hue.
fn hue_sat(c: u32) -> (f32, f32) {
    let (r, g, b) = ((c >> 16 & 0xff) as f32, (c >> 8 & 0xff) as f32, (c & 0xff) as f32);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    if d < 1.0 || max < 1.0 {
        return (0.0, 0.0);
    }
    let h = if max == r {
        60.0 * (((g - b) / d) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    ((h + 360.0) % 360.0, d / max)
}

fn hue_gap(a: f32, b: f32) -> f32 {
    let d = (a - b).abs() % 360.0;
    if d > 180.0 { 360.0 - d } else { d }
}

/// Hue buckets, 30 degrees wide, ranked by how many palette entries they hold.
///
/// The best populated hues make the best substitutes, because a recolour needs
/// a matching brightness at every level of shading. A hue with one entry in it
/// would flatten the armour into a single flat colour.
pub fn hue_buckets(palette: &[u32]) -> Vec<Vec<usize>> {
    let n = palette.len().min(32);
    let mut buckets: [Vec<usize>; 12] = Default::default();
    for i in 1..n {
        let (h, s) = hue_sat(palette[i]);
        if s < MIN_SATURATION {
            continue; // grey: carries metal and shadow, must not move
        }
        buckets[((h / 30.0) as usize).min(11)].push(i);
    }
    let mut ranked: Vec<Vec<usize>> = buckets.into_iter().filter(|b| !b.is_empty()).collect();
    ranked.sort_by_key(|b| std::cmp::Reverse(b.len()));
    ranked
}

/// Colours below this are grey enough that moving them would recolour the
/// armour and the ground shadow along with the tunic.
const MIN_SATURATION: f32 = 0.25;

/// Substitute every saturated entry with the member of `bucket` closest in
/// brightness. Shading survives because brightness is what the eye reads as
/// form; only the colour changes.
pub fn map_into_bucket(palette: &[u32], bucket: &[usize]) -> Lut {
    let mut lut = IDENTITY;
    if bucket.is_empty() {
        return lut;
    }
    let n = palette.len().min(32);
    for i in 1..n {
        let (_, s) = hue_sat(palette[i]);
        if s < MIN_SATURATION {
            continue;
        }
        let want = luma(palette[i]);
        let best = bucket
            .iter()
            .copied()
            .min_by(|a, b| {
                (luma(palette[*a]) - want)
                    .abs()
                    .total_cmp(&(luma(palette[*b]) - want).abs())
            })
            .unwrap_or(i);
        lut[i] = best as u8;
    }
    lut
}

/// One substitution per player, drawn from the hues this palette actually has.
///
/// Player one is always the identity, so the artwork looks the way it was drawn.
/// The others take the next best populated hues in turn. A palette with few hues
/// yields few distinct knights; that is a property of the artwork, not a bug, and
/// [`distinctness`] is how a caller finds out.
pub fn player_luts(palette: &[u32]) -> [Lut; 4] {
    let ranked = hue_buckets(palette);
    let mut out = [IDENTITY; 4];
    if ranked.len() < 2 {
        return out; // nothing to swap to
    }
    for (k, lut) in out.iter_mut().enumerate().skip(1) {
        // Skip rank 0: that is the dominant hue, which player one already wears.
        let bucket = &ranked[1 + (k - 1) % (ranked.len() - 1)];
        *lut = map_into_bucket(palette, bucket);
    }
    out
}

/// The share of saturated entries a substitution actually moved. A palette with
/// one hue in it cannot produce four different knights, and a caller is better
/// off knowing that than guessing.
pub fn distinctness(palette: &[u32], lut: &Lut) -> f32 {
    let n = palette.len().min(32);
    let mut movable = 0;
    let mut moved = 0;
    for i in 1..n {
        if hue_sat(palette[i]).1 < MIN_SATURATION {
            continue;
        }
        movable += 1;
        if lut[i] as usize != i {
            moved += 1;
        }
    }
    if movable == 0 { 0.0 } else { moved as f32 / movable as f32 }
}

/// Is this palette entry colourful enough to be a tunic rather than a shadow?
pub fn is_saturated(palette: &[u32], index: usize) -> bool {
    palette.get(index).map_or(false, |c| hue_sat(*c).1 >= MIN_SATURATION)
}

/// The brightest entry sharing a colour's hue.
///
/// A tunic is a mid shade, which is right on a knight and useless on a status
/// bar against a dark backdrop. This keeps the identity (same hue, so the bar
/// still names its knight) while being legible.
pub fn brightest_of_same_hue(palette: &[u32], index: usize) -> u8 {
    let n = palette.len().min(32);
    if index == 0 || index >= n {
        return index as u8;
    }
    let (h, s) = hue_sat(palette[index]);
    if s < MIN_SATURATION {
        // Grey: just take the brightest grey rather than inventing a colour.
        return (1..n)
            .filter(|j| hue_sat(palette[*j]).1 < MIN_SATURATION)
            .max_by(|a, b| luma(palette[*a]).total_cmp(&luma(palette[*b])))
            .unwrap_or(index) as u8;
    }
    (1..n)
        .filter(|j| {
            let (jh, js) = hue_sat(palette[*j]);
            js >= MIN_SATURATION && hue_gap(jh, h) < 30.0
        })
        .max_by(|a, b| luma(palette[*a]).total_cmp(&luma(palette[*b])))
        .unwrap_or(index) as u8
}

/// A representative colour per seat: the hue that seat's knights are wearing.
///
/// Taken straight from the same ranked hues [`player_luts`] assigns, so a status
/// bar cannot drift away from the figure on screen. Deriving it from the artwork
/// instead is fragile: the most common colour in a sprite sheet is often a
/// highlight or an outline rather than the tunic.
///
/// Picks the brightest entry in the bucket, since these are drawn small and
/// against arbitrary backdrops.
pub fn player_colours(palette: &[u32]) -> [u8; 4] {
    let ranked = hue_buckets(palette);
    let mut out = [1u8; 4];
    if ranked.is_empty() {
        return out;
    }
    for (k, slot) in out.iter_mut().enumerate() {
        let bucket = if k == 0 || ranked.len() < 2 {
            &ranked[0]
        } else {
            &ranked[1 + (k - 1) % (ranked.len() - 1)]
        };
        *slot = bucket
            .iter()
            .copied()
            .max_by(|a, b| luma(palette[*a]).total_cmp(&luma(palette[*b])))
            .unwrap_or(1) as u8;
    }
    out
}

/// How many visibly different knights this palette can actually support.
pub fn max_distinct_players(palette: &[u32]) -> usize {
    hue_buckets(palette).len().min(4).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Several hues, each present at matched brightnesses, plus a grey ramp.
    /// This is the shape real artwork palettes have.
    fn palette() -> Vec<u32> {
        let mut p = vec![0x000000];
        for base in [
            (255.0, 40.0, 40.0),   // red
            (255.0, 150.0, 40.0),  // orange
            (40.0, 255.0, 40.0),   // green
            (40.0, 200.0, 255.0),  // cyan
        ] {
            let base_luma = 0.299 * base.0 + 0.587 * base.1 + 0.114 * base.2;
            for target in [40.0f32, 80.0, 120.0, 160.0] {
                let k = target / base_luma;
                let c = |v: f32| ((v * k).round().clamp(0.0, 255.0) as u32) & 0xff;
                p.push((c(base.0) << 16) | (c(base.1) << 8) | c(base.2));
            }
        }
        for k in 0..6u32 {
            let v = k * 40;
            p.push((v << 16) | (v << 8) | v);
        }
        p
    }

    #[test]
    fn player_one_always_looks_like_the_artwork() {
        let p = palette();
        assert_eq!(player_luts(&p)[0], IDENTITY);
        assert_eq!(distinctness(&p, &IDENTITY), 0.0);
    }

    #[test]
    fn a_substitution_changes_colour_but_keeps_brightness() {
        let p = palette();
        for lut in player_luts(&p).iter().skip(1) {
            for i in 1..p.len() {
                if hue_sat(p[i]).1 < MIN_SATURATION {
                    continue;
                }
                let to = lut[i] as usize;
                assert!(
                    (luma(p[to]) - luma(p[i])).abs() < 30.0,
                    "entry {i} moved too far in brightness, which would wreck the shading"
                );
            }
        }
    }

    #[test]
    fn greys_and_transparency_never_move() {
        let p = palette();
        for lut in player_luts(&p) {
            assert_eq!(lut[0], 0, "transparent must stay transparent");
            for i in 1..p.len() {
                if hue_sat(p[i]).1 < MIN_SATURATION {
                    assert_eq!(lut[i] as usize, i, "grey entry {i} must stay put");
                }
            }
        }
    }

    #[test]
    fn four_hues_give_four_different_knights() {
        let p = palette();
        assert_eq!(max_distinct_players(&p), 4);
        let luts = player_luts(&p);
        for a in 0..4 {
            for b in a + 1..4 {
                assert_ne!(luts[a], luts[b], "players {a} and {b} look identical");
            }
        }
    }

    /// A palette with one hue cannot produce four knights. It has to say so and
    /// degrade quietly rather than produce nonsense.
    #[test]
    fn a_status_colour_keeps_the_hue_but_gains_brightness() {
        let p = palette();
        for i in 1..p.len() {
            let b = brightest_of_same_hue(&p, i) as usize;
            assert!(luma(p[b]) >= luma(p[i]), "entry {i} must not get darker");
            let (hi, si) = hue_sat(p[i]);
            let (hb, sb) = hue_sat(p[b]);
            if si >= MIN_SATURATION {
                assert!(sb >= MIN_SATURATION && hue_gap(hb, hi) < 30.0,
                    "entry {i} changed hue, so the bar would stop naming its knight");
            }
        }
        assert_eq!(brightest_of_same_hue(&p, 0), 0, "transparent stays transparent");
    }

    #[test]
    fn seat_colours_follow_the_same_hues_as_the_knights() {
        let p = palette();
        let colours = player_colours(&p);
        let ranked = hue_buckets(&p);
        // Seat one wears the dominant hue, which is the one the artwork uses.
        assert!(ranked[0].contains(&(colours[0] as usize)));
        // The guarantee is bucket separation, not a particular angle: each seat
        // draws from a different one of the palette's hue groups.
        let bucket_of = |i: u8| {
            ranked.iter().position(|b| b.contains(&(i as usize))).expect("colour came from a bucket")
        };
        let mut seen = std::collections::BTreeSet::new();
        for (seat, c) in colours.iter().enumerate() {
            assert!(seen.insert(bucket_of(*c)), "seat {seat} reuses another seat's hue");
        }
    }

    #[test]
    fn a_single_hue_palette_degrades_instead_of_breaking() {
        let mono: Vec<u32> = (1..12).map(|k: u32| (k * 20) << 16).collect();
        assert_eq!(max_distinct_players(&mono), 1);
        for lut in player_luts(&mono) {
            assert_eq!(lut, IDENTITY, "nothing to swap to, so change nothing");
        }
    }

    /// With three hues the fourth player has to reuse one. The code must not
    /// pretend otherwise, and must never hand a player the dominant hue that
    /// player one already wears.
    #[test]
    fn fewer_hues_than_players_reuses_rather_than_inventing() {
        let mut p = palette();
        p.truncate(1 + 12); // drop the cyan block, leaving three hues
        assert_eq!(max_distinct_players(&p), 3);
        let luts = player_luts(&p);
        assert_eq!(luts[0], IDENTITY);
        assert_ne!(luts[1], luts[2]);
        assert_eq!(luts[3], luts[1], "the fourth wraps back round");
    }
}
