//! Online play.
//!
//! **All of this is ours.** The original has no network code of any kind: its
//! multiplayer is up to four joysticks on one machine, `NUM_PLAYERS` says how
//! many of the four `KnightTAB` records a person is driving, and `GetInputDevice`
//! reads them all from the same keyboard and game port. There is nothing in the
//! image to recover here, so nothing in this crate claims to be recovered, and
//! every other crate's rule still holds: **a number that came out of the image
//! is not allowed to change because of anything in here.**
//!
//! What this crate does is narrower than it sounds. It moves **one byte per seat
//! per tick** between machines and makes every machine run the same ticks in the
//! same order with the same bytes in them. That byte is the original's own: the
//! five bits `GetInputDevice` builds for a seat, which
//! [`henge_desktop`'s input module](../henge_desktop/input/index.html) already
//! keeps as `RIGHT`, `LEFT`, `DOWN`, `UP` and `FIRE`. The simulation is never
//! told that a peer exists.
//!
//! ### Why lockstep and not a server
//!
//! `henge-core` is deterministic by construction: integers only, `BTreeMap`
//! rather than `HashMap`, no wall clock, no unseeded randomness, and both
//! generators carry their seeds inside the state. Two machines fed the same
//! inputs therefore produce the same state, bit for bit, which is already proved
//! for a whole bout by `Bout::state_hash` and for a whole run by
//! `Run::state_hash`. So there is no authoritative world to host: every machine
//! *is* the world, and all that has to cross the wire is input.
//!
//! That is also why there is no prediction and no rollback here. A tick does not
//! run until every seat's input for it has arrived, and a peer that falls behind
//! stalls the others rather than guessing. It is the honest version, it cannot
//! desync, and at the rates this game runs at ([`lockstep`]) the delay is small
//! enough to play on.
//!
//! ### The shape of a game
//!
//! One machine hosts. Guests connect to it, and the host is the only machine
//! that needs a reachable port, which is the whole reason [`portmap`] exists.
//!
//! ```text
//! guest                     host
//!   |-- Hello ------------->|   protocol check, a seat, the roster
//!   |<------------- Welcome -|
//!   |<------------- Roster --|   again whenever anybody joins or leaves
//!   |-- Seated ------------>|   the knight and the name this seat wants
//!   |<------------- Start ---|   host only, once everyone is in
//!   |-- Input{t} ---------->|   every tick, this seat's five bits
//!   |<------------- Turn{t} -|   every tick, all seats' five bits
//!   |-- Check{t,hash} ----->|   now and then, and the host compares
//!   |<------------- Desync --|   if they ever differ
//! ```
//!
//! [`wire`] is the framing, [`proto`] the messages, [`lobby`] the two state
//! machines in front of a game, [`lockstep`] the scheduler that decides when a
//! tick may run, [`session`] the two of them joined up, [`portmap`] the router
//! talk, and [`list`] the directory a game is found on when nobody wants to be
//! told an address.
//!
//! ### Finding a game
//!
//! Everything above works with nothing but an address typed in by hand, and that
//! is still the plainest way to play. [`list`] adds the other way: a small
//! server, run by anybody, that hosts announce themselves to and browsers ask.
//! It holds names and addresses and nothing else. It never sees a password, it
//! holds no game state, it cannot join a game, and a game that found its peers
//! without it plays exactly the same. Its one clever trick is honest rather than
//! magic: an announcement arrives on a connection, so the server knows the
//! host's real public address without believing any router, and it tries to
//! connect *back* to say whether anybody outside the house can actually get in.
//! A host that comes back unreachable is carried by the server instead, byte for
//! byte, without the server ever parsing what it carries.

pub mod later;
pub mod list;
pub mod lobby;
pub mod lockstep;
pub mod portmap;
pub mod proto;
pub mod session;
pub mod wire;

pub use later::Later;
pub use list::{browse, Directory, ListMsg, Listing, DEFAULT_LIST_PORT};
pub use lobby::{Event, Guest, Host, JoinError};
pub use lockstep::{Desync, Lockstep, Turn};
pub use portmap::{Mapping, Opener, Reach};
pub use proto::{key, Lobby, Msg, Player, SeatInput, DEFAULT_PORT, PROTOCOL, SEATS};
pub use session::{Session, Side};
pub use wire::{Link, Listener, WireError};
