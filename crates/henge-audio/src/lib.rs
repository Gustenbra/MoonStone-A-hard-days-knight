//! Sound for a bout.
//!
//! Two halves, deliberately separate. [`cue`] decides *what* should be heard by
//! watching the fight, and is pure logic with no platform behind it. [`sink`]
//! decides *where it comes out*, and is the only part that knows a sound card
//! exists.
//!
//! Splitting them that way means the interesting half is testable without a
//! sound card, and a browser or server build replaces only the boring half.

pub mod cue;
pub mod sink;

pub use cue::{Cue, Voices};
pub use sink::{Clips, Recording, Silent, Sink};

#[cfg(feature = "native")]
pub use sink::Native;
