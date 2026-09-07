//! Cutting one frame out of a packed sheet.
//!
//! Every scene needs this and each one used to write it again. It is four lines
//! of row copying, and getting the bounds check wrong in one of five places is
//! the sort of bug that shows up as a smear in exactly one screen.

use crate::framebuffer::Framebuffer;
use henge_assets::{Lut, Registry};

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

/// Draw a frame through a colour substitution table.
pub fn draw_lut(
    reg: &mut Registry, fb: &mut Framebuffer, sheet: &str, index: usize,
    x: i32, y: i32, lut: &Lut,
) {
    if let Some(c) = cut(reg, sheet, index) {
        fb.blit_lut(&c.pixels, c.w, c.h, x, y, false, lut);
    }
}

/// Draw a frame as a flat silhouette.
///
/// Sheet pixels are palette indices and nothing records which palette they were
/// baked against, so a sprite drawn over an unrelated screen comes out as noise.
/// The fonts already solve this by drawing a shape in a chosen colour; the
/// status furniture is small and needs to read over any backdrop, so it does the
/// same.
pub fn draw_mask(
    reg: &mut Registry, fb: &mut Framebuffer, sheet: &str, index: usize,
    x: i32, y: i32, colour: u8,
) -> (i32, i32) {
    match cut(reg, sheet, index) {
        Some(c) => {
            fb.blit_mask(&c.pixels, c.w, c.h, x, y, colour);
            (c.w as i32, c.h as i32)
        }
        None => (0, 0),
    }
}

/// How big a frame is, without drawing it.
pub fn size(reg: &Registry, sheet: &str, index: usize) -> (i32, i32) {
    reg.sheet(sheet)
        .and_then(|r| r.value.frames.get(index).map(|f| (f.w as i32, f.h as i32)))
        .unwrap_or((0, 0))
}
