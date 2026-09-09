//! Input: the original's own reading of a stick, and a binding table of ours.
//!
//! # What is recovered
//!
//! The whole game is driven by one five-bit word. `_KBD:JOY0` and `_KBD:JOY1`
//! build it from the gameport, the routine beside them builds it from the
//! keyboard, and the two are OR'd together, so a stick and the keys are the
//! same input and always have been. `Rjoystick` and `Ljoystick` then pick which
//! of the eight attacks a direction held with fire means.
//!
//! ```text
//! 0x01 right   0x02 left   0x04 down   0x08 up   0x10 fire
//! ```
//!
//! * **Reading a stick.** `out 0x201, 0xff` fires the two one-shots and the
//!   loop counts until each falls, capped at `0x400`. A count that reaches the
//!   cap is a stick that is not there, and contributes nothing. Otherwise the
//!   count is compared against four thresholds: at or below `JOY_XMIN` is left,
//!   at or above `JOY_XMAX` is right, and the same pair for Y.
//! * **The button.** `in al, 0x201` again, `dl = al & (al >> 1)`, and the pair
//!   of bits for that stick is tested. Gameport buttons are active low, so
//!   *either* button of the pair sets fire.
//! * **Calibration.** `Fix_JoyStick` asks twice, in the original's own words:
//!   *Move joystick to / the top left / and press the / fire button.* and then
//!   *the bottom right*. `GetJoyTL` and `GetJoyBR` store the raw counts, and
//!   `AdjustJoy` then pulls each threshold **one eighth of the measured range**
//!   inwards. So three quarters of the travel is dead and a direction only
//!   registers in the outer eighth at each end. Before anybody calibrates,
//!   the four thresholds are the constants the image ships in `DGROUP` at
//!   `DS:0x817e`: 16, 16, 80, 80. See [`Calibration`].
//! * **Where the menu reaches it.** `OptionKeys` at `0x1282` tests scancode 0x24,
//!   `J`, before anything else and jumps straight to `Fix_JoyStick`, which ends
//!   with a `jmp` back to the options screen at `0x59d7`.
//! * **Debounce.** `BOUNCEBUTTON` waits for fire to go down and then waits for
//!   it to come back up. A press is worth exactly one thing, however long it is
//!   held. See [`Debounce`].
//! * **Opposite directions.** `GetInputDevice` throws away left and right held
//!   together, and up and down together, before anything sees the word. See
//!   [`settle`].
//! * **The original's own keys.** Player one is Enter, Up, Down, Left, Right.
//!   Player two is Tab, W, X, A, D. Both are in the reader at image `0x81ec` as
//!   five `KEYPRESSED` calls each, shifted into the word a bit at a time.
//!
//! # What is ours
//!
//! **Rebindable controls.** The original has no such thing: its keys are ten
//! `mov ax, <scancode>` instructions. So [`Bindings`] is designed, not
//! translated, and it is deliberately *data*: a table of actions to sources
//! that serialises to JSON and can be saved, shipped in a pack, or edited by
//! hand. Nothing in the game asks which key was pressed; it asks which action
//! is held.
//!
//! The table's *contents* are not ours, though. It ships with the original's own
//! ten keys, read out of the reader at image `0x81ec`, because where the original
//! has something this project translates it, and the layout is something it has.

use serde::{Deserialize, Serialize};

/// The original's input word. `Rjoystick` and `Ljoystick` index on these bits.
pub const RIGHT: u8 = 0x01;
pub const LEFT: u8 = 0x02;
pub const DOWN: u8 = 0x04;
pub const UP: u8 = 0x08;
pub const FIRE: u8 = 0x10;

/// What a player can ask for. Five per seat, which is the whole of the
/// original's input word; everything else the game does is a menu on top of it.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Up,
    Down,
    Left,
    Right,
    Fire,
}

impl Action {
    pub const ALL: [Action; 5] = [
        Action::Up,
        Action::Down,
        Action::Left,
        Action::Right,
        Action::Fire,
    ];

    /// The bit this action sets in the original's word.
    pub fn bit(self) -> u8 {
        match self {
            Action::Up => UP,
            Action::Down => DOWN,
            Action::Left => LEFT,
            Action::Right => RIGHT,
            Action::Fire => FIRE,
        }
    }
}

/// Where an action can come from. Names rather than numbers, so a bindings file
/// is readable and survives a change of key enumeration.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    /// A keyboard key, by winit's own `KeyCode` name: `ArrowUp`, `KeyW`, `Space`.
    Key { code: String },
    /// A gamepad button, by gilrs' own `Button` name: `South`, `DPadUp`.
    Button { name: String },
    /// One end of a gamepad axis: `LeftStickX` negative is left.
    Axis { name: String, positive: bool },
}

impl Source {
    pub fn key(code: &str) -> Source {
        Source::Key { code: code.into() }
    }
    pub fn button(name: &str) -> Source {
        Source::Button { name: name.into() }
    }
    pub fn axis(name: &str, positive: bool) -> Source {
        Source::Axis {
            name: name.into(),
            positive,
        }
    }
}

/// One seat's controls, and which gamepad answers for it.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Seat {
    /// Which pad, in the order the system reports them. Seat zero takes the
    /// first pad, seat one the second, and a seat with no pad is keys only.
    #[serde(default)]
    pub pad: usize,
    /// Every source that can raise each action. A list, because a key and a
    /// pad both being able to move the same knight is the original's own
    /// behaviour: it ORs the keyboard word into the stick word.
    pub actions: std::collections::BTreeMap<Action, Vec<Source>>,
}

impl Seat {
    fn of(pad: usize, rows: [(Action, Vec<Source>); 5]) -> Seat {
        Seat {
            pad,
            actions: rows.into_iter().collect(),
        }
    }
}

/// The whole table. Two seats, which is as many as share one keyboard.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Bindings {
    pub seats: Vec<Seat>,
    /// Where the sticks sit, per seat. Saved with the bindings because a
    /// calibration is as much a setting as a key is.
    #[serde(default)]
    pub calibration: Vec<Calibration>,
}

impl Default for Bindings {
    /// **The original's own ten keys**, plus a pad for each seat.
    ///
    /// The reader at image `0x81ec` is two runs of five `KEYPRESSED` calls, each
    /// followed by an `rcl` into the seat's word, so the order the scancodes are
    /// asked for is the bit order and the last one shifted in is bit 0. Player
    /// one, into `bx`: `0x1c` Enter, `0x48` up, `0x50` down, `0x4b` left, `0x4d`
    /// right. Player two, into `dx` from `0x821d`: `0x0f` Tab, `0x11` W, `0x2d`
    /// X, `0x1e` A, `0x20` D. Against the word's own bits (`0x10` fire, `0x08`
    /// up, `0x04` down, `0x02` left, `0x01` right) that is Enter and the arrows
    /// for one, and Tab, W, X, A, D for two.
    ///
    /// The pad sources beside them are ours: the original reads a gameport, and a
    /// d-pad and a south button are the same five bits arriving on hardware it
    /// never saw.
    fn default() -> Bindings {
        let pad_dirs = |pad: usize, keys: [&str; 5]| {
            Seat::of(
                pad,
                [
                    (
                        Action::Up,
                        vec![
                            Source::key(keys[0]),
                            Source::button("DPadUp"),
                            Source::axis("LeftStickY", true),
                        ],
                    ),
                    (
                        Action::Down,
                        vec![
                            Source::key(keys[1]),
                            Source::button("DPadDown"),
                            Source::axis("LeftStickY", false),
                        ],
                    ),
                    (
                        Action::Left,
                        vec![
                            Source::key(keys[2]),
                            Source::button("DPadLeft"),
                            Source::axis("LeftStickX", false),
                        ],
                    ),
                    (
                        Action::Right,
                        vec![
                            Source::key(keys[3]),
                            Source::button("DPadRight"),
                            Source::axis("LeftStickX", true),
                        ],
                    ),
                    (
                        Action::Fire,
                        vec![
                            Source::key(keys[4]),
                            Source::button("South"),
                            Source::button("East"),
                        ],
                    ),
                ],
            )
        };
        Bindings {
            seats: vec![
                // 0x48, 0x50, 0x4b, 0x4d, 0x1c.
                pad_dirs(
                    0,
                    ["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Enter"],
                ),
                // 0x11, 0x2d, 0x1e, 0x20, 0x0f.
                pad_dirs(1, ["KeyW", "KeyX", "KeyA", "KeyD", "Tab"]),
            ],
            calibration: vec![Calibration::default(), Calibration::default()],
        }
    }
}

impl Bindings {
    /// Every action a source raises, across every seat.
    pub fn raised_by(&self, src: &Source) -> Vec<(usize, Action)> {
        let mut out = Vec::new();
        for (i, seat) in self.seats.iter().enumerate() {
            for (a, list) in &seat.actions {
                if list.contains(src) {
                    out.push((i, *a));
                }
            }
        }
        out
    }

    /// Points one action at one source, taking that source off whatever else
    /// on the same seat had it. Rebinding is only useful if it cannot leave two
    /// actions fighting over one key.
    pub fn bind(&mut self, seat: usize, action: Action, src: Source) {
        let Some(s) = self.seats.get_mut(seat) else {
            return;
        };
        for (_, list) in s.actions.iter_mut() {
            list.retain(|x| x != &src);
        }
        s.actions.entry(action).or_default().push(src);
    }

    pub fn calibration(&self, seat: usize) -> Calibration {
        self.calibration.get(seat).copied().unwrap_or_default()
    }

    pub fn set_calibration(&mut self, seat: usize, c: Calibration) {
        while self.calibration.len() <= seat {
            self.calibration.push(Calibration::default());
        }
        self.calibration[seat] = c;
    }

    /// `--bind 0:fire=Space`, `--bind 1:up=pad:DPadUp`, `--bind 0:left=axis:LeftStickX-`.
    ///
    /// A rebinding screen is a screen, and this shell has no settings page yet,
    /// so the table is edited from the command line or in the file itself. The
    /// grammar is deliberately the same names the file uses.
    pub fn parse_bind(spec: &str) -> Option<(usize, Action, Source)> {
        let (who, src) = spec.split_once('=')?;
        let (seat, action) = who.split_once(':')?;
        let seat: usize = seat.trim().parse().ok()?;
        let action = match action.trim() {
            "up" => Action::Up,
            "down" => Action::Down,
            "left" => Action::Left,
            "right" => Action::Right,
            "fire" => Action::Fire,
            _ => return None,
        };
        let src = src.trim();
        let source = if let Some(name) = src.strip_prefix("pad:") {
            Source::button(name)
        } else if let Some(name) = src.strip_prefix("axis:") {
            let positive = name.ends_with('+');
            let name = name.trim_end_matches(['+', '-']);
            if name.is_empty() {
                return None;
            }
            Source::axis(name, positive)
        } else if src.is_empty() {
            return None;
        } else {
            Source::key(src.trim_start_matches("key:"))
        };
        Some((seat, action, source))
    }

    pub fn load(path: &str) -> Option<Bindings> {
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn save(&self, path: &str) -> std::io::Result<()> {
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, text)
    }
}

/// `JOY_XMIN`, `JOY_XMAX`, `JOY_YMIN`, `JOY_YMAX`.
///
/// Counts, in the same units `JOY0` and `JOY1` produce: how many times round
/// the read loop before the one-shot for that axis fell. A modern pad reports a
/// fraction rather than a resistance, so [`Calibration::from_axis`] puts one on
/// the same scale and the original's own comparisons are then used unchanged.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Calibration {
    pub xmin: i32,
    pub xmax: i32,
    pub ymin: i32,
    pub ymax: i32,
}

impl Default for Calibration {
    /// **The original's own shipped thresholds**, and they are four constants,
    /// not a derivation.
    ///
    /// `JOY0` compares against four words in `DGROUP` at `DS:0x817e`, `0x8180`,
    /// `0x8182` and `0x8184`. `Fix_JoyStick` overwrites them, but the image has
    /// them initialised, at image `0x1a52e` onwards, and they read **16, 16, 80,
    /// 80**: `xmin`, `ymin`, `xmax`, `ymax`. That is what the game answers with
    /// before anybody calibrates anything, so it is what this answers with.
    ///
    /// The earlier default here invented two corners at six tenths of deflection
    /// and put them through `AdjustJoy`. There was never any need: the numbers
    /// were in the image. They are also self consistent with `AdjustJoy`, which
    /// is a check on having read the right four words: corners of 6 and 90 have
    /// a range of 84, an eighth of 84 is 10, and 6 + 10 with 90 - 10 is exactly
    /// 16 and 80.
    fn default() -> Calibration {
        Calibration {
            xmin: Calibration::SHIPPED_MIN,
            ymin: Calibration::SHIPPED_MIN,
            xmax: Calibration::SHIPPED_MAX,
            ymax: Calibration::SHIPPED_MAX,
        }
    }
}

impl Calibration {
    /// `cmp bx, 0x400`. A count that gets this far is a stick that is not
    /// plugged in, and the original ignores that axis entirely. It is the
    /// runaway cap of the read loop and not a deflection, so nothing a present
    /// stick reports may ever reach it.
    pub const TIMEOUT: i32 = 0x400;

    /// The thresholds the image ships with, at `DS:0x817e` and `DS:0x8182`
    /// (image `0x1a52e` and `0x1a532`): 16 and 80.
    pub const SHIPPED_MIN: i32 = 16;
    pub const SHIPPED_MAX: i32 = 80;

    /// The corner pair [`Calibration::SHIPPED_MIN`] and
    /// [`Calibration::SHIPPED_MAX`] are `AdjustJoy` of, which is 6 and 90, and
    /// therefore the count scale the original's own numbers live on.
    ///
    /// **This pair is the inherent part.** A gameport count is how many times
    /// round a busy loop, so the same stick on a faster machine counts higher and
    /// there is no fixed number of counts that means "hard left": that is exactly
    /// why `Fix_JoyStick` exists. A pad that reports a fraction has to be put on
    /// *some* count scale before the original's comparisons can be used on it,
    /// and the only scale the image names is this one. Mapping -1 onto 6 and +1
    /// onto 90 also makes the two agree: calibrating a pad that reports clean
    /// extremes records corners of 6 and 90 and `AdjustJoy` gives back the
    /// shipped 16 and 80, so the default and a fresh calibration are the same
    /// answer.
    ///
    /// Unused, along with [`Calibration::from_axis`], in a build with the gamepad
    /// feature off: that is a machine with no stick to put on any scale.
    #[cfg_attr(not(feature = "gamepad"), allow(dead_code))]
    pub const AXIS_LOW: i32 = 6;
    #[cfg_attr(not(feature = "gamepad"), allow(dead_code))]
    pub const AXIS_HIGH: i32 = 90;

    /// `GetJoyTL`, `GetJoyBR` and `AdjustJoy`, in that order.
    ///
    /// The two corners are the raw counts with the stick held top left and then
    /// bottom right. Each threshold is then moved an eighth of the measured
    /// range towards the middle, so the dead zone is the middle three quarters.
    pub fn from_corners(tl: (i32, i32), br: (i32, i32)) -> Calibration {
        let (mut xmin, mut ymin) = tl;
        let (mut xmax, mut ymax) = br;
        // `sub ax, ...; shr ax, 1` three times: an eighth of the range, and the
        // original's shift is unsigned, so a range read backwards gives nothing
        // rather than a negative nudge.
        let eighth = |lo: i32, hi: i32| ((hi - lo).max(0) as u32 >> 3) as i32;
        let ex = eighth(xmin, xmax);
        let ey = eighth(ymin, ymax);
        xmin += ex;
        xmax -= ex;
        ymin += ey;
        ymax -= ey;
        Calibration {
            xmin,
            xmax,
            ymin,
            ymax,
        }
    }

    /// One axis of a modern pad, as -1.0 to 1.0, on the counts' own scale.
    ///
    /// Deliberately not a shortcut past the original's thresholds: a stick that
    /// reports a fraction is turned into the count the same deflection would
    /// have produced on a gameport, and then read exactly as `JOY0` reads one.
    /// The scale is [`Calibration::AXIS_LOW`] to [`Calibration::AXIS_HIGH`],
    /// which is the scale the shipped thresholds are measured on.
    #[cfg_attr(not(feature = "gamepad"), allow(dead_code))]
    pub fn from_axis(v: f32) -> i32 {
        let n = ((v + 1.0) * 0.5).clamp(0.0, 1.0);
        let span = (Calibration::AXIS_HIGH - Calibration::AXIS_LOW) as f32;
        let c = Calibration::AXIS_LOW + (n * span + 0.5) as i32;
        c.clamp(Calibration::AXIS_LOW, Calibration::AXIS_HIGH)
    }

    /// `JOY0`'s four comparisons, and its treatment of a timed out axis.
    pub fn mask(&self, x: i32, y: i32) -> u8 {
        let mut m = 0;
        if y != Calibration::TIMEOUT {
            if y <= self.ymin {
                m |= UP;
            }
            if y >= self.ymax {
                m |= DOWN;
            }
        }
        if x != Calibration::TIMEOUT {
            if x <= self.xmin {
                m |= LEFT;
            }
            if x >= self.xmax {
                m |= RIGHT;
            }
        }
        m
    }
}

/// `GetInputDevice`: left with right, and up with down, are thrown away.
///
/// A stick cannot do it but two keys can, and a knight told to walk both ways
/// at once is worse than one told to stand still.
pub fn settle(mask: u8) -> u8 {
    let mut m = mask;
    if m & (LEFT | RIGHT) == (LEFT | RIGHT) {
        m &= !(LEFT | RIGHT);
    }
    if m & (UP | DOWN) == (UP | DOWN) {
        m &= !(UP | DOWN);
    }
    m
}

/// `BOUNCEBUTTON`: one press is worth one thing.
///
/// The original blocks in a loop, waiting for fire to go down and then waiting
/// for it to come back up, because it is used where the game has nothing else
/// to do. Here the same rule is kept as an edge: [`Debounce::edge`] answers true
/// on the frame fire goes down and never again until it has been released.
#[derive(Default, Clone, Copy, Debug)]
pub struct Debounce {
    held: bool,
}

impl Debounce {
    /// True on the frame fire goes down, once per press.
    pub fn edge(&mut self, mask: u8) -> bool {
        let down = mask & FIRE != 0;
        let edge = down && !self.held;
        self.held = down;
        edge
    }

    /// Forgets the press, so the next one is an edge again. What the original
    /// gets for free by returning only once fire has been let go.
    pub fn clear(&mut self) {
        self.held = false;
    }
}

/// `Fix_JoyStick`, as a state machine rather than as a blocking loop.
///
/// The original asks twice and blocks on `BOUNCEBUTTON` each time. The prompts
/// are its own, recovered from `joy_mess_to1` to `joy_mess_bo4`; the only thing
/// changed is that this returns to the caller between the two answers, because
/// the game here has a frame to draw and the original did not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Calibrating {
    /// Waiting for the stick held to the top left.
    TopLeft,
    /// Top left recorded, waiting for the bottom right.
    BottomRight { tl: (i32, i32) },
}

impl Calibrating {
    /// `joy_mess_to*` and `joy_mess_bo*`, verbatim, one line each.
    pub fn prompt(self) -> [&'static str; 4] {
        match self {
            Calibrating::TopLeft => [
                "Move joystick to",
                "the top left",
                "and press the",
                "fire button.",
            ],
            Calibrating::BottomRight { .. } => [
                "Move joystick to",
                "the bottom right",
                "and press the",
                "fire button.",
            ],
        }
    }

    /// One frame. `raw` is where the stick is now, `fire` the debounced press.
    /// Answers the next step, or the finished calibration.
    pub fn step(self, raw: (i32, i32), fire: bool) -> Result<Calibrating, Calibration> {
        if !fire {
            return Ok(self);
        }
        match self {
            Calibrating::TopLeft => Ok(Calibrating::BottomRight { tl: raw }),
            Calibrating::BottomRight { tl } => Err(Calibration::from_corners(tl, raw)),
        }
    }
}

/// The gamepads, when the build has them.
///
/// `gilrs` reports no pads rather than failing when there is no input
/// subsystem, and this wraps it so that a build without the feature, a machine
/// without a pad and a container without udev are all the same thing: an empty
/// list of sticks and a game that plays on the keys.
pub struct Pads {
    #[cfg(feature = "gamepad")]
    inner: Option<gilrs::Gilrs>,
    /// Why there are no pads, if there are none. Said once, like the sound.
    pub note: Option<String>,
}

impl Pads {
    #[cfg(feature = "gamepad")]
    pub fn open() -> Pads {
        match gilrs::Gilrs::new() {
            Ok(g) => Pads {
                inner: Some(g),
                note: None,
            },
            Err(e) => Pads {
                inner: None,
                note: Some(format!("no gamepads: {e}")),
            },
        }
    }

    #[cfg(not(feature = "gamepad"))]
    pub fn open() -> Pads {
        Pads {
            note: Some("built without gamepad support".into()),
        }
    }

    /// Drains the event queue, which is how the library learns a pad was
    /// plugged in or taken away.
    #[cfg(feature = "gamepad")]
    pub fn poll(&mut self) {
        if let Some(g) = self.inner.as_mut() {
            while g.next_event().is_some() {}
        }
    }

    #[cfg(not(feature = "gamepad"))]
    pub fn poll(&mut self) {}

    #[cfg(feature = "gamepad")]
    pub fn count(&self) -> usize {
        self.inner.as_ref().map_or(0, |g| g.gamepads().count())
    }

    #[cfg(not(feature = "gamepad"))]
    pub fn count(&self) -> usize {
        0
    }

    /// The original's five-bit word for one seat: the stick through the seat's
    /// own calibration, the buttons and axis ends the bindings name, and then
    /// `GetInputDevice`'s cancellation over the lot.
    #[cfg(feature = "gamepad")]
    pub fn word(&self, seat: &Seat, cal: &Calibration) -> u8 {
        use gilrs::{Axis, Button};
        let Some(g) = self.inner.as_ref() else {
            return 0;
        };
        let Some((_, pad)) = g.gamepads().nth(seat.pad) else {
            return 0;
        };
        // The stick, read the way `JOY0` reads one. gilrs points Y up and the
        // gameport counts down the screen, so the axis is negated to put the
        // two on the same footing.
        let x = Calibration::from_axis(pad.value(Axis::LeftStickX));
        let y = Calibration::from_axis(-pad.value(Axis::LeftStickY));
        let mut m = cal.mask(x, y);
        for (action, sources) in &seat.actions {
            for s in sources {
                let on = match s {
                    Source::Button { name } => {
                        button_named(name).is_some_and(|b: Button| pad.is_pressed(b))
                    }
                    Source::Axis { name, positive } => axis_named(name).is_some_and(|a: Axis| {
                        let v = pad.value(a);
                        let c = Calibration::from_axis(if *positive { -v } else { v });
                        // One end of an axis is one direction, and the same
                        // thresholds decide it.
                        c <= cal.xmin
                    }),
                    Source::Key { .. } => false,
                };
                if on {
                    m |= action.bit();
                }
            }
        }
        settle(m)
    }

    #[cfg(not(feature = "gamepad"))]
    pub fn word(&self, _seat: &Seat, cal: &Calibration) -> u8 {
        // No stick at all, which `JOY0` already has an answer for: both axes
        // read as the timed out count, and a timed out axis contributes
        // nothing. So a build with no gamepad support is a gameport with
        // nothing plugged into it.
        settle(cal.mask(Calibration::TIMEOUT, Calibration::TIMEOUT))
    }

    /// The raw counts for one pad, for a calibration screen to record.
    #[cfg(feature = "gamepad")]
    pub fn raw(&self, pad_index: usize) -> Option<(i32, i32)> {
        use gilrs::Axis;
        let g = self.inner.as_ref()?;
        let (_, pad) = g.gamepads().nth(pad_index)?;
        Some((
            Calibration::from_axis(pad.value(Axis::LeftStickX)),
            Calibration::from_axis(-pad.value(Axis::LeftStickY)),
        ))
    }

    #[cfg(not(feature = "gamepad"))]
    pub fn raw(&self, _pad_index: usize) -> Option<(i32, i32)> {
        None
    }
}

#[cfg(feature = "gamepad")]
fn button_named(name: &str) -> Option<gilrs::Button> {
    use gilrs::Button::*;
    Some(match name {
        "South" => South,
        "East" => East,
        "North" => North,
        "West" => West,
        "LeftTrigger" => LeftTrigger,
        "LeftTrigger2" => LeftTrigger2,
        "RightTrigger" => RightTrigger,
        "RightTrigger2" => RightTrigger2,
        "Select" => Select,
        "Start" => Start,
        "Mode" => Mode,
        "LeftThumb" => LeftThumb,
        "RightThumb" => RightThumb,
        "DPadUp" => DPadUp,
        "DPadDown" => DPadDown,
        "DPadLeft" => DPadLeft,
        "DPadRight" => DPadRight,
        _ => return None,
    })
}

#[cfg(feature = "gamepad")]
fn axis_named(name: &str) -> Option<gilrs::Axis> {
    use gilrs::Axis::*;
    Some(match name {
        "LeftStickX" => LeftStickX,
        "LeftStickY" => LeftStickY,
        "RightStickX" => RightStickX,
        "RightStickY" => RightStickY,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_word_is_the_originals_five_bits() {
        assert_eq!(
            (RIGHT, LEFT, DOWN, UP, FIRE),
            (0x01, 0x02, 0x04, 0x08, 0x10)
        );
        assert_eq!(Action::Up.bit() | Action::Fire.bit(), 0x18);
    }

    #[test]
    fn adjust_joy_pulls_each_threshold_in_by_an_eighth_of_the_range() {
        // A stick reading 100 at the left and 900 at the right: an eighth of
        // 800 is 100, so the thresholds land on 200 and 800.
        let c = Calibration::from_corners((100, 100), (900, 900));
        assert_eq!((c.xmin, c.xmax), (200, 800));
        assert_eq!((c.ymin, c.ymax), (200, 800));
    }

    #[test]
    fn three_quarters_of_the_travel_is_dead() {
        let c = Calibration::from_corners((0, 0), (800, 800));
        // An eighth of 800 is 100, so the live bands are 0..=100 and 700..=800.
        assert_eq!((c.xmin, c.xmax), (100, 700));
        assert_eq!(c.mask(400, 400), 0);
        assert_eq!(c.mask(101, 400), 0);
        assert_eq!(c.mask(100, 400), LEFT);
        assert_eq!(c.mask(699, 400), 0);
        assert_eq!(c.mask(700, 400), RIGHT);
        assert_eq!(c.mask(400, 100), UP);
        assert_eq!(c.mask(400, 700), DOWN);
        assert_eq!(c.mask(100, 700), LEFT | DOWN);
    }

    #[test]
    fn an_axis_that_times_out_is_a_stick_that_is_not_there() {
        let c = Calibration::from_corners((0, 0), (800, 800));
        // 0x400 is the cap the read loop stops at, and the original skips the
        // comparison entirely rather than reading it as hard right.
        assert_eq!(c.mask(Calibration::TIMEOUT, 400), 0);
        assert_eq!(c.mask(400, Calibration::TIMEOUT), 0);
        assert_eq!(c.mask(Calibration::TIMEOUT, Calibration::TIMEOUT), 0);
    }

    #[test]
    fn a_backwards_calibration_does_not_invert_the_dead_zone() {
        // Someone who answers the two prompts the wrong way round should get a
        // stick that does nothing, not one that is live everywhere.
        let c = Calibration::from_corners((900, 900), (100, 100));
        assert_eq!((c.xmin, c.xmax), (900, 100));
        // Everything reads as both directions at once, and `settle` cancels it.
        assert_eq!(settle(c.mask(500, 500)), 0);
    }

    /// The four words the image ships at `DS:0x817e`, verbatim.
    #[test]
    fn the_default_thresholds_are_the_four_words_the_image_ships() {
        let c = Calibration::default();
        assert_eq!(
            (c.xmin, c.ymin, c.xmax, c.ymax),
            (16, 16, 80, 80),
            "image 0x1a52e onwards"
        );
    }

    /// And they are `AdjustJoy` of the scale's own two ends, which is both the
    /// check that the right four words were read and the reason the scale is
    /// what it is.
    #[test]
    fn the_shipped_thresholds_are_adjust_joy_of_the_scale_ends() {
        let from_extremes = Calibration::from_corners(
            (Calibration::AXIS_LOW, Calibration::AXIS_LOW),
            (Calibration::AXIS_HIGH, Calibration::AXIS_HIGH),
        );
        assert_eq!(from_extremes, Calibration::default());
    }

    #[test]
    fn a_normalised_axis_lands_on_the_counts_scale() {
        assert_eq!(Calibration::from_axis(-1.0), Calibration::AXIS_LOW);
        assert_eq!(Calibration::from_axis(1.0), Calibration::AXIS_HIGH);
        let mid = Calibration::from_axis(0.0);
        assert_eq!(mid, 48, "the midpoint of 6 and 90");
        // Nothing a present stick reports may be mistaken for an absent one.
        assert!(Calibration::from_axis(1.0) < Calibration::TIMEOUT);
        // The shipped thresholds leave a centred stick alone and answer a stick
        // pushed the whole way.
        let c = Calibration::default();
        assert_eq!(c.mask(mid, mid), 0);
        assert_eq!(c.mask(Calibration::from_axis(-1.0), mid), LEFT);
        assert_eq!(c.mask(Calibration::from_axis(1.0), mid), RIGHT);
        assert_eq!(c.mask(mid, Calibration::from_axis(-1.0)), UP);
        assert_eq!(c.mask(mid, Calibration::from_axis(1.0)), DOWN);
        // A light nudge is inside the dead zone, which is the point of it. The
        // live band is the outer quarter of each half, which is what an eighth of
        // the range at each end comes to.
        assert_eq!(c.mask(Calibration::from_axis(-0.4), mid), 0);
        assert_eq!(c.mask(Calibration::from_axis(-0.7), mid), 0);
        assert_eq!(c.mask(Calibration::from_axis(-0.8), mid), LEFT);
        assert_eq!(c.mask(Calibration::from_axis(0.8), mid), RIGHT);
    }

    #[test]
    fn opposites_cancel_and_nothing_else_is_touched() {
        assert_eq!(settle(LEFT | RIGHT), 0);
        assert_eq!(settle(UP | DOWN), 0);
        assert_eq!(settle(LEFT | RIGHT | FIRE), FIRE);
        assert_eq!(settle(UP | DOWN | LEFT | FIRE), LEFT | FIRE);
        assert_eq!(settle(UP | LEFT | FIRE), UP | LEFT | FIRE);
    }

    #[test]
    fn a_held_button_is_one_press() {
        let mut d = Debounce::default();
        assert!(!d.edge(0));
        assert!(d.edge(FIRE));
        assert!(!d.edge(FIRE));
        assert!(!d.edge(FIRE));
        assert!(!d.edge(0));
        assert!(d.edge(FIRE));
        assert!(!d.edge(FIRE));
        d.clear();
        assert!(d.edge(FIRE));
    }

    #[test]
    fn debounce_ignores_the_direction_bits() {
        let mut d = Debounce::default();
        assert!(d.edge(FIRE | LEFT));
        assert!(!d.edge(FIRE | RIGHT));
        assert!(!d.edge(UP));
        assert!(d.edge(FIRE | UP));
    }

    /// The reader at image `0x81ec`, key for key and bit for bit. Ten assertions
    /// where there used to be four, because the layout is now recovered and a
    /// recovered layout is checkable in full.
    #[test]
    fn the_default_bindings_are_the_keys_the_original_reads() {
        let b = Bindings::default();
        assert_eq!(b.seats.len(), 2);
        // Player one, `bx`: 0x1c, 0x48, 0x50, 0x4b, 0x4d.
        for (a, k) in [
            (Action::Fire, "Enter"),
            (Action::Up, "ArrowUp"),
            (Action::Down, "ArrowDown"),
            (Action::Left, "ArrowLeft"),
            (Action::Right, "ArrowRight"),
        ] {
            assert!(
                b.seats[0].actions[&a].contains(&Source::key(k)),
                "seat one {a:?} is {k}"
            );
        }
        // Player two, `dx`: 0x0f, 0x11, 0x2d, 0x1e, 0x20.
        for (a, k) in [
            (Action::Fire, "Tab"),
            (Action::Up, "KeyW"),
            (Action::Down, "KeyX"),
            (Action::Left, "KeyA"),
            (Action::Right, "KeyD"),
        ] {
            assert!(
                b.seats[1].actions[&a].contains(&Source::key(k)),
                "seat two {a:?} is {k}"
            );
        }
        // And nothing of ours is left in: space and F were henge's own fire keys
        // and the original has neither.
        for k in ["Space", "KeyF", "KeyS"] {
            assert!(
                b.raised_by(&Source::key(k)).is_empty(),
                "{k} is not one of the original's ten"
            );
        }
        // Each seat still has a pad of its own, which is ours and stays.
        assert_eq!((b.seats[0].pad, b.seats[1].pad), (0, 1));
        assert!(b.seats[0].actions[&Action::Up].contains(&Source::button("DPadUp")));
    }

    #[test]
    fn binding_a_key_takes_it_off_whatever_had_it() {
        let mut b = Bindings::default();
        b.bind(0, Action::Fire, Source::key("ArrowUp"));
        assert!(!b.seats[0].actions[&Action::Up].contains(&Source::key("ArrowUp")));
        assert!(b.seats[0].actions[&Action::Fire].contains(&Source::key("ArrowUp")));
        assert_eq!(
            b.raised_by(&Source::key("ArrowUp")),
            vec![(0, Action::Fire)]
        );
        // The other seat is a different set of controls and is left alone.
        assert!(b.seats[1].actions[&Action::Up].contains(&Source::key("KeyW")));
    }

    #[test]
    fn one_source_may_serve_two_seats_because_the_original_ors_them_together() {
        let mut b = Bindings::default();
        b.bind(0, Action::Fire, Source::button("Start"));
        b.bind(1, Action::Fire, Source::button("Start"));
        assert_eq!(
            b.raised_by(&Source::button("Start")),
            vec![(0, Action::Fire), (1, Action::Fire)]
        );
    }

    #[test]
    fn bindings_are_data_and_survive_the_round_trip() {
        let mut b = Bindings::default();
        b.bind(1, Action::Fire, Source::button("North"));
        b.set_calibration(1, Calibration::from_corners((40, 60), (900, 880)));
        let text = serde_json::to_string(&b).expect("bindings serialise");
        let back: Bindings = serde_json::from_str(&text).expect("bindings deserialise");
        assert_eq!(back.calibration(1), b.calibration(1));
        assert!(back.seats[1].actions[&Action::Fire].contains(&Source::button("North")));
        assert_eq!(
            back.seats[0].actions[&Action::Left],
            b.seats[0].actions[&Action::Left]
        );
    }

    #[test]
    fn a_binding_spec_reads_the_way_the_file_does() {
        assert_eq!(
            Bindings::parse_bind("0:fire=Space"),
            Some((0, Action::Fire, Source::key("Space")))
        );
        assert_eq!(
            Bindings::parse_bind("1:up=pad:DPadUp"),
            Some((1, Action::Up, Source::button("DPadUp")))
        );
        assert_eq!(
            Bindings::parse_bind("0:left=axis:LeftStickX-"),
            Some((0, Action::Left, Source::axis("LeftStickX", false)))
        );
        assert_eq!(
            Bindings::parse_bind("0:right=axis:LeftStickX+"),
            Some((0, Action::Right, Source::axis("LeftStickX", true)))
        );
        // Refusals rather than guesses, so a typo cannot silently unbind a key.
        assert!(Bindings::parse_bind("0:jump=Space").is_none());
        assert!(Bindings::parse_bind("nonsense").is_none());
        assert!(Bindings::parse_bind("0:fire=").is_none());
    }

    #[test]
    fn a_missing_or_broken_bindings_file_is_not_an_error() {
        assert!(Bindings::load("/nonexistent/henge-controls.json").is_none());
    }

    #[test]
    fn calibrating_asks_twice_and_then_hands_back_a_calibration() {
        let mut c = Calibrating::TopLeft;
        assert_eq!(c.prompt()[1], "the top left");
        // Fire not pressed: nothing moves on, however far the stick is pushed.
        c = c.step((30, 40), false).expect("still asking");
        assert_eq!(c, Calibrating::TopLeft);
        c = c
            .step((30, 40), true)
            .expect("now asking for the other corner");
        assert_eq!(c.prompt()[1], "the bottom right");
        c = c.step((910, 880), false).expect("still asking");
        let done = c
            .step((910, 880), true)
            .expect_err("that was the second answer");
        assert_eq!(done, Calibration::from_corners((30, 40), (910, 880)));
        // And that is the calibration `AdjustJoy` would have produced.
        assert_eq!((done.xmin, done.xmax), (30 + 110, 910 - 110));
    }

    #[test]
    fn a_calibration_run_end_to_end_gives_a_usable_dead_zone() {
        // Drive it the way a player would, through the debounce.
        let mut d = Debounce::default();
        let mut c = Ok(Calibrating::TopLeft);
        let script = [
            (0, 0),
            (0, 0),
            (60, 70),
            (60, 70),
            (500, 500),
            (940, 930),
            (940, 930),
        ];
        let fires = [false, true, true, false, false, false, true];
        for (raw, held) in script.into_iter().zip(fires) {
            let edge = d.edge(if held { FIRE } else { 0 });
            if let Ok(state) = c {
                c = state.step(raw, edge);
            }
        }
        let cal = c.expect_err("two presses finish it");
        // The first press was made before the stick moved, and a held button is
        // one press, so the corners are (0,0) and (940,930).
        assert_eq!(cal, Calibration::from_corners((0, 0), (940, 930)));
        assert_eq!(cal.mask(cal.xmin, cal.ymax), LEFT | DOWN);
        assert_eq!(cal.mask(470, 465), 0);
    }

    #[test]
    fn no_pad_is_a_normal_state() {
        // Whatever the machine has, asking for a word must not panic and a
        // seat pointed at a pad that is not there must read as nothing held.
        let mut pads = Pads::open();
        pads.poll();
        let b = Bindings::default();
        let far = Seat {
            pad: 99,
            ..Default::default()
        };
        assert_eq!(pads.word(&far, &b.calibration(0)), 0);
        assert!(pads.raw(99).is_none());
    }
}
