//! Reading `INTR.EXE`, the intro's own program.
//!
//! `INTR.EXE` is the same engine as `MAIN.EXE` with a different program on
//! top, and `tools/symbolmap.py` reads it unchanged. What it writes with
//! `--image`, though, is **not** a finished load image: the tail of it is still
//! Microsoft EXEPACK's run-length stream, one command every few hundred bytes,
//! and every zero-filled span of the program has been left as a four-byte
//! `fill` command instead of the bytes it stands for.
//!
//! That single fact explains both of the corrections `docs/REVERSING.md`
//! recorded against this executable and could not account for. The code
//! addresses appeared to need "a seven-step monotone correction" because the
//! image is short by exactly the fills that precede each step, and the data
//! addresses appeared to be "14,911 past where the bytes actually are" because
//! by `DGROUP` the fills have accumulated to that. Expand the stream and
//! **every symbol, code and data, lands on its own byte with no correction at
//! all**: `TextPTop` is at 12,589, `PerformCOMMAND` at 16,332, `TextASCII` at
//! 44,634 and `MesFILE` at 46,007, which are the numbers the symbol table
//! carries.
//!
//! ```text
//! read backwards from the last command byte:
//!   b0 / b1   fill:  [u8 value][u16 count][cmd]
//!   b2 / b3   copy:  [count bytes][u16 count][cmd]
//!   bit 0 set on the command is the last one
//! ```
//!
//! With the image expanded, three things this project had written off come out
//! of it: the tile map behind the opening pan, the intro's own animation
//! scripts, and the coordinates its captions are drawn at.

use henge_core::content::{IntroCast as Cast, IntroFrame as Frame};
use henge_core::taskvm::Part;
use std::collections::BTreeMap;

/// `DGROUP` in `INTR.EXE`: the segment every data symbol is relative to.
pub const DS_BASE: usize = 1746 * 16;

/// Expand the EXEPACK run-length stream `symbolmap.py` leaves in the tail of
/// the image it writes for `INTR.EXE`.
///
/// The end of the stream is found rather than assumed: the debug information
/// is appended after it, so the walk is tried from each plausible command byte
/// near the end and the one that decodes cleanly all the way down to a
/// terminator is the right one.
pub fn expand(image: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut best: Option<Vec<u8>> = None;
    let lo = image.len() / 2;
    for end in (lo..image.len()).rev() {
        if image[end] & 0xfe != 0xb0 && image[end] & 0xfe != 0xb2 {
            continue;
        }
        if let Some((head, out)) = walk(image, end) {
            // A real stream covers most of the file and ends on a terminator.
            if end - head > image.len() / 3 {
                best = Some([&image[..head], &out[..]].concat());
                break;
            }
        }
    }
    let out = best.ok_or_else(|| {
        anyhow::anyhow!("no EXEPACK run-length stream in this image; is it INTR.EXE's?")
    })?;
    anyhow::ensure!(
        out.len() > DS_BASE + 0x5000,
        "expanded image is only {} bytes, too short to hold DGROUP",
        out.len()
    );
    Ok(out)
}

/// One attempt at the walk. Returns where the stream started and what it
/// expands to, or nothing if it does not decode.
fn walk(image: &[u8], end: usize) -> Option<(usize, Vec<u8>)> {
    let mut p = end;
    let mut out: Vec<u8> = Vec::new();
    loop {
        let cmd = *image.get(p)?;
        if cmd & 0xfe != 0xb0 && cmd & 0xfe != 0xb2 {
            return None;
        }
        if p < 3 {
            return None;
        }
        let count = u16::from_le_bytes([image[p - 2], image[p - 1]]) as usize;
        // Where this command's own bytes begin.
        let start = if cmd & 0xfe == 0xb0 {
            p - 3
        } else {
            (p + 1).checked_sub(3 + count)?
        };
        if cmd & 0xfe == 0xb0 {
            splice(&mut out, &vec![image[start]; count]);
        } else {
            splice(&mut out, image.get(start..start + count)?);
        }
        if cmd & 1 != 0 {
            return Some((start, out));
        }
        p = start.checked_sub(1)?;
    }
}

fn splice(out: &mut Vec<u8>, head: &[u8]) {
    let mut v = Vec::with_capacity(head.len() + out.len());
    v.extend_from_slice(head);
    v.append(out);
    *out = v;
}

// ------------------------------------------------------------------ the cast

pub use henge_core::content::{IntroCast, IntroFrame};

/// `INTR.EXE`'s own opcode table, which is **not** `MAIN.EXE`'s.
///
/// Both are filled by an `INITTASK` that writes immediates into
/// `TaskCommandTable`, and the intro's has one more entry: `TaskStop` takes the
/// slot at `+0x14` and everything above it moves up one, so `TASKGOSUB` is
/// `0x9a` in the intro where it is `0x98` in the game. Each handler's operand
/// width is its own `add word ptr [di+2], n`, read the same way.
const WIDTH: [(u8, usize); 21] = [
    (0x80, 2), // TASK_FLIP
    (0x82, 4), // TASKGOTO
    (0x84, 2), // TASKHOLD
    (0x86, 8), // TASKJUMP
    (0x8a, 2), // TASKLOOP
    (0x8c, 4), // TASKSKIP
    (0x8e, 4), // TASKTIME
    (0x90, 8), // TASKMOVE
    (0x92, 2), // TASKSOUND
    (0x94, 4), // TASKSTOP
    (0x96, 6), // TASKSAVE
    (0x98, 4), // TASKSHADOW
    (0x9a, 4), // TASKGOSUB
    (0x9c, 4), // TASKDEAD
    (0x9e, 2), // TASKADDTASK
    (0xa0, 2), // TASKKILLTASK
    (0xa2, 2), // TASKCELBUF
    (0xa4, 4), // TASKFACE
    (0xa6, 6), // TASKTESTEQ
    (0xa8, 6), // TASKTESTNE
    (0xaa, 2), // TASKANIMCLR
];

fn width(op: u8) -> Option<usize> {
    WIDTH.iter().find(|(o, _)| *o == op).map(|(_, w)| *w)
}

/// The five sprite banks the intro's own cast is drawn from, in slot order.
///
/// The loader fills the table at `DS:0x445d` four bytes a slot, and which slot
/// a file lands in is the address the loader writes it to, not the order it
/// loads them in. The intro's loader at `0x38a1` writes `au1` to `0x445d`,
/// `li1` to `0x4461`, `da1` to `0x446d`, `ha1` to `0x4465` and `dw1` to
/// `0x4469`, so in slot order it is `au1`, `li1`, `ha1`, `dw1`, `da1`.
const CAST_BANKS: [&str; 5] = ["bank.au1", "bank.li1", "bank.ha1", "bank.dw1", "bank.da1"];

/// The six the **ending** is drawn from, which is a different set in a
/// different order.
///
/// The ending's loader at `0x3a00` writes `dg1` to `0x445d`, `li1` to
/// `0x4461`, `ha1` to `0x4469`, `co1` to `0x4465`, `da1` to `0x446d` and
/// `klift1` to `0x4471`, so in slot order it is `dg1`, `li1`, `co1`, `ha1`,
/// `da1`, `klift1`. Note that `ha1` and `co1` are loaded in the opposite order
/// from the slots they go into, and that the five druid scripts the two halves
/// share sit in slot 4 in both, which is `da1` either way.
const ENDING_BANKS: [&str; 6] = [
    "bank.dg1",
    "bank.li1",
    "bank.co1",
    "bank.ha1",
    "bank.da1",
    "bank.klift1",
];

/// Flatten one script into frames, and say where it rejoins itself.
///
/// The intro's scripts use five of the twenty-one commands and nothing else:
/// `TASKHOLD`, `TASKGOTO`, `TASKLOOP`, `TASKGOSUB` and the sprite-part record.
/// A `TASKGOTO` whose mode is not 3 arms a jump taken at the *next* end of
/// frame, which is what makes a script that finishes on `ff ff` carry on
/// anyway: that is how the intro's scenery holds still while the one script
/// with no pending jump decides how long the scene lasts.
///
/// The second return is the frame a script that outlives its own length goes
/// back to, and `None` for one that simply stops. `0x3fbe` builds a frame by
/// walking the script from the task's pointer, and a task sitting on `ff ff`
/// walks straight into `0x418b`, which clears the running flag and returns
/// **without emitting a single part**. So a script that has run out is not
/// drawn at all, rather than holding its last frame: the forest's druids walk
/// off the left of the screen and are gone.
///
/// Where a script does loop, the walk comes back to a script position it has
/// already started a frame at, and that position's frame is where it rejoins.
pub fn flatten(img: &[u8], off: u16, cap: usize) -> anyhow::Result<(Vec<Frame>, Option<usize>)> {
    let mut p = DS_BASE + off as usize;
    let mut frames: Vec<Frame> = Vec::new();
    let mut cur = Frame {
        hold: 1,
        parts: Vec::new(),
    };
    let mut pending: Option<u16> = None;
    // Where each frame started, so a script that comes back to one is known to
    // loop and known to loop from there.
    let mut starts: Vec<usize> = vec![p];
    while frames.len() < cap {
        let b = *img
            .get(p)
            .ok_or_else(|| anyhow::anyhow!("script {off:04x} runs off the image"))?;
        if b == 0xff {
            let end = img[p + 1];
            frames.push(std::mem::replace(
                &mut cur,
                Frame {
                    hold: 1,
                    parts: Vec::new(),
                },
            ));
            p += 2;
            if let Some(t) = pending.take() {
                p = DS_BASE + t as usize;
            } else if end == 0xff {
                return Ok((frames, None));
            }
            if let Some(at) = starts.iter().position(|q| *q == p) {
                return Ok((frames, Some(at)));
            }
            starts.push(p);
            continue;
        }
        if b & 0x80 != 0 {
            let w = width(b)
                .ok_or_else(|| anyhow::anyhow!("script {off:04x}: unknown opcode {b:#04x}"))?;
            match b {
                0x84 => cur.hold = img[p + 1].max(1),
                0x82 => {
                    let target = u16::from_le_bytes([img[p + 2], img[p + 3]]);
                    if img[p + 1] == 3 {
                        p = DS_BASE + target as usize;
                        continue;
                    }
                    pending = Some(target);
                }
                _ => {}
            }
            p += w;
            continue;
        }
        anyhow::ensure!(
            b % 4 == 0 && b < 0x20,
            "script {off:04x}: bad bank selector {b:#04x}"
        );
        cur.parts.push(Part {
            table: 1,
            bank: b / 4,
            cel: img[p + 1],
            y: img[p + 2] as i8 as i16,
            flags: img[p + 3],
            x: i16::from_le_bytes([img[p + 4], img[p + 5]]),
        });
        p += 6;
    }
    // Long enough to have been cut rather than ended: it is going round
    // something, so send it back to the top.
    Ok((frames, Some(0)))
}

/// Every script the intro's own scene routines start, flattened.
///
/// The list is the `mov si, imm16` operands of the calls to the two task
/// starters at `0x1ef` and `0x207`, read out of the intro's main module.
pub const SCRIPTS: [u16; 18] = [
    0x1583, 0x1739, 0x1e29, 0x2669, 0x26e3, 0x2927, 0x2ce5, 0x2dc5, 0x2e51, 0x2e75, 0x2e99, 0x2ec3,
    0x309f, 0x30c3, 0x3221, 0x338f, 0x348f, 0x349b,
];

/// Every script the **ending**'s four scene routines start.
///
/// Read the same way, out of `0x547`, `0x628`, `0x6db` and `0x778` and the two
/// tables they step: `0x35cf` is the five of the word table at `DS:0x000c`,
/// and the last five are the standing druids `0x4d0` puts down, which the
/// intro's scene `0x465` puts down too.
pub const ENDING_SCRIPTS: [u16; 15] = [
    0x2351, 0x2e51, 0x2e75, 0x2e99, 0x309f, 0x3221, 0x3531, 0x35cf, 0x393d, 0x3a49, 0x3b07, 0x3b1f,
    0x3c75, 0x3ceb, 0x3ddb,
];

pub fn cast(img: &[u8]) -> anyhow::Result<Cast> {
    flatten_all(img, &SCRIPTS, &CAST_BANKS)
}

/// The ending's cast, out of the same image with the ending's own bank table.
pub fn ending_cast(img: &[u8]) -> anyhow::Result<Cast> {
    flatten_all(img, &ENDING_SCRIPTS, &ENDING_BANKS)
}

fn flatten_all(img: &[u8], list: &[u16], banks: &[&str]) -> anyhow::Result<Cast> {
    let mut scripts = BTreeMap::new();
    let mut loops = BTreeMap::new();
    for off in list.iter().copied() {
        // A looping script is cut where it comes back on itself, and the frame
        // it comes back to is kept so it can go round for as long as the scene
        // needs. One that ends on `ff ff` gets no entry, and stops being drawn.
        let (frames, from) = flatten(img, off, 64)?;
        anyhow::ensure!(!frames.is_empty(), "script {off:04x} has no frames");
        let name = format!("{off:04x}");
        if let Some(from) = from {
            loops.insert(name.clone(), from as u32);
        }
        scripts.insert(name, frames);
    }
    Ok(Cast {
        banks: banks.iter().map(|s| s.to_string()).collect(),
        scripts,
        loops,
    })
}

// ------------------------------------------------------------------ the check

/// What the expanded image has to say, so that a differently unpacked one is
/// rejected rather than baked into a wrong intro.
///
/// `TextASCII` is 95 bytes at `DS:0x413a` with `A` at index 33, and the pan's
/// own speed table is eleven ascending thresholds at `DS:0x124` with eleven
/// speeds beside them at `DS:0x13a`.
pub fn check(img: &[u8]) -> anyhow::Result<()> {
    let w =
        |off: usize| -> u16 { u16::from_le_bytes([img[DS_BASE + off], img[DS_BASE + off + 1]]) };
    let ascii = &img[DS_BASE + 0x413a..DS_BASE + 0x413a + 95];
    anyhow::ensure!(
        ascii[33..59] == (0u8..26).collect::<Vec<u8>>()[..],
        "TextASCII is not where the symbol table puts it; the image is not expanded"
    );
    let thresholds: Vec<u16> = (0..11).map(|i| w(0x124 + i * 2)).collect();
    let speeds: Vec<u16> = (0..11).map(|i| w(0x13a + i * 2)).collect();
    anyhow::ensure!(
        thresholds == henge_core::intro::PAN_STOPS.to_vec()
            && speeds
                == henge_core::intro::PAN_SPEEDS
                    .iter()
                    .map(|s| *s as u16)
                    .collect::<Vec<_>>(),
        "the pan tables in the image are {thresholds:?} / {speeds:?}, not the recovered ones"
    );
    Ok(())
}

// -------------------------------------------------------------- the panorama

/// The tile map behind the opening pan.
///
/// `.STI` is a tile map, and the tile engine that reads it is in `GFX` beside
/// the text engine. `PlaceTile` takes a tile number, `FindTile` cuts it out of
/// a 320x200 sheet as `((n % 10) * 32, (n / 10) * 25)`, which is the same 32x25
/// grid ten across that `CMP` scenery sheets use, and the routine that walks
/// the map reads **big-endian** words ten to a row and divides each by 80 to
/// pick which of three loaded sheets it comes from.
///
/// So `INTRO.STI`'s 960 bytes are 48 rows of ten tiles: a 320 by 1200 panorama,
/// and the pan is a window 200 tall moving down it.
pub const MAP_W: usize = 10;
pub const TILE_W: usize = 32;
pub const TILE_H: usize = 25;

pub struct Panorama {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

/// Composite a `.STI` tile map over its sheets.
pub fn panorama(sti: &[u8], sheets: &[&crate::Piv]) -> anyhow::Result<Panorama> {
    anyhow::ensure!(
        sti.len().is_multiple_of(MAP_W * 2) && !sti.is_empty(),
        "not a tile map: {} bytes",
        sti.len()
    );
    let rows = sti.len() / (MAP_W * 2);
    let (width, height) = (MAP_W * TILE_W, rows * TILE_H);
    let mut pixels = vec![0u8; width * height];
    let per_sheet = (crate::piv::W / TILE_W) * (crate::piv::H / TILE_H);
    for row in 0..rows {
        for col in 0..MAP_W {
            let i = (row * MAP_W + col) * 2;
            let tile = u16::from_be_bytes([sti[i], sti[i + 1]]) as usize;
            let Some(sheet) = sheets.get(tile / per_sheet) else {
                continue;
            };
            let cell = tile % per_sheet;
            let (sx, sy) = (
                (cell % (crate::piv::W / TILE_W)) * TILE_W,
                (cell / (crate::piv::W / TILE_W)) * TILE_H,
            );
            for y in 0..TILE_H {
                for x in 0..TILE_W {
                    let src = (sy + y) * crate::piv::W + sx + x;
                    let dst = (row * TILE_H + y) * width + col * TILE_W + x;
                    pixels[dst] = sheet.pixels[src];
                }
            }
        }
    }
    Ok(Panorama {
        width,
        height,
        pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a stream the way EXEPACK writes one: commands read backwards, the
    /// last one to be executed carrying bit 0.
    fn stream(head: &[u8], cmds: &[(bool, &[u8])]) -> Vec<u8> {
        let mut out = head.to_vec();
        for (i, (fill, data)) in cmds.iter().enumerate() {
            let last = i == 0;
            if *fill {
                out.push(data[0]);
                out.extend_from_slice(&(data[1] as u16).to_le_bytes());
                out.push(0xb0 | last as u8);
            } else {
                out.extend_from_slice(data);
                out.extend_from_slice(&(data.len() as u16).to_le_bytes());
                out.push(0xb2 | last as u8);
            }
        }
        out
    }

    #[test]
    fn a_copy_block_comes_back_verbatim_and_a_fill_expands() {
        // Commands are given innermost first, which is the order they are
        // written: the terminator is the lowest one in the file.
        let packed = stream(
            &[],
            &[(false, b"hello"), (true, &[0x41, 3]), (false, b"world")],
        );
        let (head, out) = walk(&packed, packed.len() - 1).expect("decodes");
        assert_eq!(head, 0);
        assert_eq!(out, b"helloAAAworld");
    }

    /// A fill of zeros is what makes the packed image shorter than the program,
    /// and it is the whole reason the symbol table's addresses looked wrong.
    #[test]
    fn a_fill_is_four_bytes_standing_for_as_many_as_it_says() {
        let packed = stream(&[], &[(true, &[0x00, 200])]);
        assert_eq!(packed.len(), 4);
        let (_, out) = walk(&packed, packed.len() - 1).expect("decodes");
        assert_eq!(out.len(), 200);
        assert!(out.iter().all(|b| *b == 0));
    }

    #[test]
    fn a_stream_that_does_not_decode_is_refused_rather_than_guessed_at() {
        assert!(walk(&[0x00, 0x01, 0x02, 0x03], 3).is_none());
        assert!(expand(&[0u8; 64]).is_err());
    }

    /// Ten tiles to a row, 32 by 25 each, big-endian, and the sheet a tile
    /// comes from is its number divided by eighty.
    #[test]
    fn a_tile_map_is_ten_wide_and_picks_its_sheet_by_eighties() {
        let mut sheets: Vec<crate::Piv> = (0..2)
            .map(|n| crate::Piv {
                kind: 5,
                planes: 5,
                palette: vec![0; 32],
                pixels: vec![n as u8 + 1; crate::piv::W * crate::piv::H],
            })
            .collect();
        // Give sheet 0's cell 11 a mark, so its geometry can be checked.
        sheets[0].pixels[25 * crate::piv::W + 32] = 99;
        let refs: Vec<&crate::Piv> = sheets.iter().collect();
        // Two rows: the first all cell 11 of sheet 0, the second all sheet 1.
        let mut sti = Vec::new();
        for _ in 0..MAP_W {
            sti.extend_from_slice(&11u16.to_be_bytes());
        }
        for _ in 0..MAP_W {
            sti.extend_from_slice(&80u16.to_be_bytes());
        }
        let pan = panorama(&sti, &refs).expect("a two row map");
        assert_eq!((pan.width, pan.height), (320, 50));
        assert_eq!(pan.pixels[0], 99, "cell 11 starts at (32, 25) of its sheet");
        assert_eq!(pan.pixels[25 * 320], 2, "the second row comes from sheet 1");
    }

    /// A script that finishes on `ff ff` reports no loop, and one that jumps
    /// back reports the frame it jumps to. That is the difference between a
    /// druid who walks out of shot and one who stands there flickering his
    /// torch for the rest of the scene.
    #[test]
    fn a_script_says_whether_it_stops_or_goes_round() {
        let mut img = vec![0u8; DS_BASE + 0x100];
        // Two frames of one part each, then `ff ff`.
        let stops: Vec<u8> = vec![
            0x0c, 1, 0, 0, 0, 0, 0xff, 0x00, 0x0c, 2, 0, 0, 0, 0, 0xff, 0xff,
        ];
        img[DS_BASE + 0x10..DS_BASE + 0x10 + stops.len()].copy_from_slice(&stops);
        let (frames, from) = flatten(&img, 0x10, 64).expect("it decodes");
        assert_eq!(frames.len(), 2);
        assert_eq!(from, None, "it ends rather than repeating");

        // Three frames, the last arming a jump back to the second. The jump is
        // taken at the end of the frame it is read in, not where it stands.
        let goes: Vec<u8> = vec![
            0x0c, 1, 0, 0, 0, 0, 0xff, 0x00, 0x0c, 2, 0, 0, 0, 0, 0xff, 0x00, 0x0c, 3, 0, 0, 0, 0,
            0x82, 0x00, 0x58, 0x00, 0xff, 0x00,
        ];
        img[DS_BASE + 0x50..DS_BASE + 0x50 + goes.len()].copy_from_slice(&goes);
        let (frames, from) = flatten(&img, 0x50, 64).expect("it decodes");
        assert_eq!(frames.len(), 3);
        assert_eq!(from, Some(1), "it rejoins at the frame the jump lands on");
    }

    #[test]
    fn a_map_that_is_not_ten_words_to_a_row_is_refused() {
        assert!(panorama(&[0u8; 7], &[]).is_err());
        assert!(panorama(&[], &[]).is_err());
    }
}
