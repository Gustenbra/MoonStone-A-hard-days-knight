//! `COLLIDE.HIT`: the hit lines that drive combat. Unusually for this game it is
//! plain text, one block per sprite bank.
//!
//! ```text
//! Troll1.cel          bank name
//! 00                  this frame has no hit line
//! 00
//! 05                  point count
//! 00                  shape type (always 0 in the shipped file)
//! 000005005005...     that many 3-digit x,y pairs
//! 99                  end of block
//! ```
//!
//! In `kn4.ob`, the weapons bank, the polylines run straight along the blade, so
//! these are the paths a strike sweeps through rather than body outlines. That is
//! what makes Moonstone's combat positional: a swing connects when its line
//! crosses the target, not when two boxes overlap.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    pub x: u16,
    pub y: u16,
}

/// One frame's hit line. Empty means the frame cannot hit anything.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct HitLine {
    pub shape: u8,
    pub points: Vec<Point>,
}

impl HitLine {
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
}

/// Bank name (as spelled in the file) to its per-frame hit lines.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Collide(pub BTreeMap<String, Vec<HitLine>>);

impl Collide {
    pub fn parse(bytes: &[u8]) -> anyhow::Result<Collide> {
        let text = String::from_utf8_lossy(bytes);
        let mut out: BTreeMap<String, Vec<HitLine>> = BTreeMap::new();
        let mut current: Option<String> = None;

        let mut lines = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .peekable();

        while let Some(line) = lines.next() {
            if is_bank_name(line) {
                current = Some(line.to_string());
                out.entry(line.to_string()).or_default();
                continue;
            }
            // A stray 99 closes a block that is already closed. The shipped file
            // ends with one, so treat it as a no-op rather than an error.
            if line == "99" {
                current = None;
                continue;
            }
            let Some(bank) = current.as_ref() else {
                anyhow::bail!("hit data before any bank name: {line}");
            };
            if line == "00" {
                out.get_mut(bank).unwrap().push(HitLine::default());
                continue;
            }

            let count: usize = line
                .parse()
                .map_err(|_| anyhow::anyhow!("{bank}: expected a point count, got {line:?}"))?;
            let shape: u8 = lines
                .next()
                .ok_or_else(|| anyhow::anyhow!("{bank}: truncated after count"))?
                .parse()
                .unwrap_or(0);
            let data = lines
                .next()
                .ok_or_else(|| anyhow::anyhow!("{bank}: truncated before point data"))?;
            anyhow::ensure!(
                data.len() == count * 6,
                "{bank}: {count} points needs {} digits, found {}",
                count * 6,
                data.len()
            );

            let points = (0..count)
                .map(|i| {
                    let s = &data[i * 6..i * 6 + 6];
                    Ok(Point {
                        x: s[0..3].parse()?,
                        y: s[3..6].parse()?,
                    })
                })
                .collect::<Result<Vec<_>, std::num::ParseIntError>>()?;
            out.get_mut(bank).unwrap().push(HitLine { shape, points });
        }
        Ok(Collide(out))
    }
}

fn is_bank_name(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    lower.ends_with(".cel") || lower.ends_with(".ob") || lower.ends_with(".c")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_empty_frames_counts_and_polylines() {
        let src = "Troll1.cel\n00\n00\n03\n00\n000005005005010005\n99\n";
        let c = Collide::parse(src.as_bytes()).unwrap();
        let frames = &c.0["Troll1.cel"];
        assert_eq!(frames.len(), 3);
        assert!(frames[0].is_empty());
        assert!(frames[1].is_empty());
        assert_eq!(
            frames[2].points,
            vec![
                Point { x: 0, y: 5 },
                Point { x: 5, y: 5 },
                Point { x: 10, y: 5 }
            ]
        );
    }

    #[test]
    fn tolerates_a_trailing_terminator() {
        let src = "a.cel\n00\n99\n99\n";
        let c = Collide::parse(src.as_bytes()).unwrap();
        assert_eq!(c.0["a.cel"].len(), 1);
    }

    #[test]
    fn rejects_a_point_count_that_does_not_match_its_data() {
        let src = "a.cel\n02\n00\n000005\n99\n";
        assert!(Collide::parse(src.as_bytes()).is_err());
    }
}
