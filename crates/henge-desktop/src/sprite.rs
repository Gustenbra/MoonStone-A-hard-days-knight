//! Cutting one frame out of a packed sheet.
//!
//! Every scene needs this and each one used to write it again. It is four lines
//! of row copying, and getting the bounds check wrong in one of five places is
//! the sort of bug that shows up as a smear in exactly one screen.

use crate::framebuffer::Framebuffer;
use henge_assets::Registry;

/// A frame lifted out of a sheet, with the anchor the pack gives it.
pub struct Cut {
    pub pixels: Vec<u8>,
    pub w: usize,
    pub h: usize,
}

/// Lift frame `index` out of `sheet`. `None` when the pack has neither.
pub fn cut(reg: &mut Registry, sheet: &str, index: usize) -> Option<Cut> {
    let rect = reg.sheet(sheet)?.value.frames.get(index).copied()?;
    let img = reg.image(sheet).ok()?;
    let (w, h) = (rect.w as usize, rect.h as usize);
    let mut pixels = vec![0u8; w * h];
    for row in 0..h {
        let src = (rect.y as usize + row) * img.width + rect.x as usize;
        if src + w <= img.pixels.len() {
            pixels[row * w..(row + 1) * w].copy_from_slice(&img.pixels[src..src + w]);
        }
    }
    Some(Cut { pixels, w, h })
}

/// Draw a frame in its own colours, at a top-left corner.
///
/// For artwork that was authored against the palette already loaded: a die on
/// the dice table, the moon over the night sky, a lair icon on the map. Those
/// are the cases where the original hands the blitter a plain `x`, `y` pair and
/// nothing has to be translated.
pub fn draw(
    reg: &mut Registry,
    fb: &mut Framebuffer,
    sheet: &str,
    index: usize,
    x: i32,
    y: i32,
    mirror: bool,
) {
    if let Some(c) = cut(reg, sheet, index) {
        fb.blit(&c.pixels, c.w, c.h, x, y, mirror);
    }
}

// `draw_mask`, which drew a frame as a flat silhouette in a chosen colour, used
// to sit here, and **nothing calls it any more**.
//
// Nothing the original draws is drawn that way, so every use of it was this
// project's own and each one had to earn its place; the audit that began with
// four uses has finished with none.
//
// - The in-fight name plates went first: the original's fight loop (`Combat`,
//   image 0x351) draws no readout of any kind, so there was nothing there to be
//   a silhouette of.
// - The map's own markers went next: `_MAP:DisplayLairs` and
//   `MOON:CheckLairEncounter` blit `MI.C` 0x14 and 0x1f in their own colours
//   against `MAP.CMP`'s own palette, and the frames henge flattened are
//   outlines the original blits nowhere.
// - The select screen's highlight frame went when `SelectPAL` was read: every
//   pixel of `SEL.CEL` cel 1 is index 15 and that palette says what 15 is, so
//   the frame goes through the ordinary cel blit like everything else.
// - The pointer was the last. `SHOWPOINTER` at image 0xcf31 is one
//   `call 0x5d7f` with cel 0 in `ax` and the coordinates in `bx` and `cx`: the
//   same blit, in `PO.CEL`'s own pixels, with no ink and no halo. See
//   `shell::draw_pointer`.
//
// `Framebuffer::blit_mask` stays, because `text::Font::draw` still uses it for
// the screens whose palette is an arena's rather than one of the three plates
// that reserve the bold face's five entries.
