//! How a run ends, drawn.
//!
//! The original has two endings and neither of them is a screen you can read
//! at leisure. Winning shows `VICTORY`, waits twenty ticks and **quits to
//! DOS** with a byte in `al` saying which knight won under which moon; losing
//! shows `GameOverMes` and jumps to `StartAgain`, which is the title. Nothing
//! anywhere counts what the quest cost.
//!
//! So the heading is the original's own words and the page under it is ours.
//! `henge_core::quest` says which of the two is which and works the exit byte
//! out anyway, because it is recovered even though nothing here can use it.
//!
//! The victory plate is `bg8.piv`, the only full-screen picture MOON names by
//! file, and it sits in the text pool between the victory lines and the next
//! message. That was an inference when it was written and the palette has since
//! settled it: of the thirty seven pictures the bake produces, exactly three
//! reserve the bold face's five entries rather than using them for their own
//! artwork, and they are `MESSAGE.PIV`, `CH.PIV` and `bg8.piv`. The first two
//! are the plates the game writes on. So `bg8` is the third, and its text is
//! drawn in the glyphs' own indices like theirs.
//!
//! **A loss goes over `MESSAGE.PIV` in the instruction colour**, because that
//! is what shows it. `MOON:0x617` is `mov si, GameOverMes; call 0x8f17`, and
//! `0x8f17` is `INSTRUCTMESSAGE`: it restores the message picture, sets the
//! bold face, walks the chain and repaints palette entries 1 to 6 as a red
//! ramp before it fades. The words themselves are in the stale span of
//! `DGROUP` and cannot be read, so those are ours; the screen they go on is
//! not.

use crate::framebuffer::Framebuffer;
use crate::text::Font;
use henge_assets::Registry;
use henge_core::quest::{Ending, Tally, VICTORY_PLATE};
use henge_core::{SCREEN_H, SCREEN_W};

/// Where the heading sits, and how far apart the lines are. The heading is set
/// in the bold face, which is twenty pixels tall, and the tally in the small
/// one; the original's own victory message is two lines and it is kept as two,
/// because in one it is wider than the screen.
const HEAD_Y: i32 = 34;
const HEAD_STEP: i32 = 22;
const LINE_STEP: i32 = 12;

/// The end of a run: the original's heading, then what it cost.
///
/// `bold` sets the heading and `small` the tally. Both in the bold face
/// overran the box before the tally had a screen of its own, because that face
/// is twenty pixels tall and `Lairs cleared 11 of 24` is wider than any box it
/// would fit in.
pub fn draw(
    reg: &mut Registry, fb: &mut Framebuffer, bold: Option<&Font>, small: Option<&Font>,
    tally: &Tally,
) {
    if matches!(tally.ending, Ending::Won { .. }) {
        show(reg, fb, VICTORY_PLATE);
    } else {
        show(reg, fb, crate::shell::MESSAGE_PLATE);
        for (i, rgb) in crate::shell::INSTRUCT_RAMP.iter().enumerate() {
            fb.palette[i + 1] = *rgb;
        }
    }
    let body = small.or(bold);
    let Some(body) = body else { return };
    let head = bold.unwrap_or(body);

    // Every line in the glyphs' own indices: both plates reserve the five
    // entries a glyph is drawn in, so there is nothing to flatten and no panel
    // to put under it.
    let lines = tally.lines();
    let heading = tally.heading_lines();
    let mut y = HEAD_Y;
    for line in &heading {
        head.draw_own_centred(reg, fb, line, y);
        y += HEAD_STEP;
    }
    let mut y = HEAD_Y + HEAD_STEP * heading.len() as i32 + 4;
    for line in &lines {
        body.draw_own_centred(reg, fb, line, y);
        y += LINE_STEP;
    }
    // `Press fire to continue`, which is the original's own line and is in
    // MOON's text pool three messages above the victory one.
    body.draw_own_centred(reg, fb, "Press fire to continue", y + 2);
}

/// A full-screen picture and its own palette. `shell::show` does the same
/// thing for the screens in front of the game; this is the one behind it.
fn show(reg: &mut Registry, fb: &mut Framebuffer, scene: &str) {
    if let Some(p) = reg.palette(&format!("palette.{scene}")).map(|r| r.value.clone()) {
        fb.set_palette(&p);
    }
    match reg.image(scene) {
        Ok(img) if img.width == SCREEN_W && img.height == SCREEN_H => {
            fb.pixels.copy_from_slice(&img.pixels)
        }
        _ => fb.clear(0),
    }
}
