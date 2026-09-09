//! Sound for a bout.
//!
//! Two halves, deliberately separate. [`sfx`] says what a sound id means, which
//! is the original's own `PLAY_SFX` translation and nothing more: *what* should
//! be heard is decided by the scripts, in the simulation, and arrives here as a
//! list of ids. [`sink`] decides *where it comes out*, and is the only part that
//! knows a sound card exists.
//!
//! Splitting them that way means the interesting half is testable without a
//! sound card, and a browser or server build replaces only the boring half.

pub mod music;
pub mod sfx;
pub mod sink;

pub use music::{Note, Score, Tunes};
pub use sink::{Clips, Recording, Silent, Sink};

#[cfg(feature = "native")]
pub use sink::Native;
