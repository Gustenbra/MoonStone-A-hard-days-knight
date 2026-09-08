//! Readers for the original Moonstone (Rob Anderson / Mindscape, 1991) data files.
//!
//! This crate exists to *study* the original: to recover how its arenas are built,
//! how its animations are framed, and how its combat is tuned. It is a research
//! tool and is deliberately not a dependency of the shipping game, which uses
//! original artwork only.
//!
//! Every format here was recovered by inspecting the data directly. No code from
//! any other reimplementation was copied, since the only such project is AGPL and
//! that licence would be incompatible with a commercial release.

pub mod cel;
pub mod collide;
pub mod depack;
pub mod introexe;
pub mod library;
pub mod piv;
pub mod taskvm;
pub mod terrain;
pub mod voc;

pub use cel::Cel;
pub use collide::Collide;
pub use library::Library;
pub use piv::{Piv, Sprite};
pub use terrain::Terrain;
pub use voc::Sample;
