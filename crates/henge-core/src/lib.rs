//! The game simulation: rules, state and timing, with no rendering, no platform
//! and no assets. Keeping this layer free of dependencies is deliberate. It means
//! the renderer can be replaced, the game can be tested headlessly, and content
//! can be added as data rather than as code.

pub mod anim;
pub mod arena;
pub mod battle_palette;
pub mod bout;
pub mod combat;
pub mod content;
pub mod intro;
pub mod item;
pub mod knight;
pub mod lair;
pub mod message;
pub mod monster;
pub mod moon;
pub mod overworld;
pub mod place;
pub mod pointer;
pub mod quest;
pub mod run;
pub mod save;
pub mod service;
pub mod shell;
pub mod taskvm;
pub mod wave;

/// The original ran at 320x200 on a 4:3 display. Keeping that resolution keeps the
/// art direction honest; the window scales it up.
pub const SCREEN_W: usize = 320;
pub const SCREEN_H: usize = 200;
