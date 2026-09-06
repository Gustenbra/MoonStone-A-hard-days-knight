//! Assets are addressed by logical id, never by filename.
//!
//! `actor.knight.walk`, `sfx.sword.clash`, `music.town`. The game asks the
//! registry for an id and does not know or care which pack answered.
//!
//! Packs are stacked, last one wins. In development the stack is:
//!
//! ```text
//!   packs/original/    our own artwork, grows over time      <- searched first
//!   packs/reference/   baked from the 1991 game, never shipped
//! ```
//!
//! So replacing a character is dropping new PNGs and one manifest entry into the
//! original pack. No code changes, no rebuild, and the reference pack keeps the
//! game playable in the meantime.
//!
//! [`Registry::shippable`] reports every id still answered by derived material.
//! A release build calls it and refuses to start if the list is not empty, which
//! turns "have we replaced everything yet" from a memory test into a check.

pub mod manifest;
pub mod registry;

pub use manifest::{FrameRect, Manifest, Provenance, Sheet};
pub use registry::{Coverage, Registry, Resolved};

/// A decoded, palette-indexed image. Index 0 is transparent, as it is throughout
/// this game.
#[derive(Clone)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}
