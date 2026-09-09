//! Drawing what a town's five gadgets open onto: the tavern, the healer and
//! the mystic, which are loops of their own over their own pictures.
//!
//! The other two doors, the merchant and the high temple, are pages of the
//! status panel (`mov ax, 5` and `mov ax, 6` before `call 0xbdd3` in `MERC`
//! and `HTEM`) and are drawn by [`crate::status`] like every other page of it.
//!
//! All of the rules live in `henge_core::town`. This file knows where the
//! pictures are and puts pixels down where the routines put them:
//!
//! ```text
//! TavernLoop 0xb137   the hand (0x9702, 0x975b), then GOLDASCII at
//!                     (0x11a, 0xe) with cx 2, then SHOWPOINTER
//! RollDice   0xb234   dice.piv, the three faces at (0x73, 0xf), (0x31, 0x26)
//!                     and (0x4b, 0x58), and the WIN or LOST chain at x 0xae
//! 0xba66, 0xb935      hea.piv or mys.piv, the greeting chain through 0x7a86,
//!                     then DonationRefresh's panel, then the verdict chain
//! ```

use crate::framebuffer::Framebuffer;
use crate::sprite;
use crate::text::Font;
use henge_assets::Registry;
use henge_core::pointer::{Gadget, Gadgets};
use henge_core::run::Run;
use henge_core::status::{Donation, Payload, Screen, DONATE_CELS, DONATE_GADGETS};
use henge_core::town::{Counter, Door, Stage, Tavern, TavernScreen, Visit, Word};
use henge_core::{SCREEN_H, SCREEN_W};

/// `Tav1`, `Tav2` at DS:0xce0f and 0xce17: `tav.piv` and `dice.piv`.
const TAVERN_SCENE: &str = "scene.tav";
const DICE_SCENE: &str = "scene.dice";
/// `HEA` and `MYS` at DS:0xd17e and 0xd186: `hea.piv` and `mys.piv`.
const HEALER_SCENE: &str = "scene.hea";
const MYSTIC_SCENE: &str = "scene.mys";

/// `Tav3`, `dice.cel`, whose first six cels are the six faces.
const DICE_SHEET: &str = "bank.dice";

/// `mys.cel`, which `_WIZARD:LoadGoldCels` at image `0xbd2a` loads into the
/// bank the donation panel is blitted from: `mov dx, MysGol; call the loader`.
const GOLD_SHEET: &str = "bank.mys";

/// One full-screen picture and its palette, as the loader at 0x875e leaves
/// them.
pub struct Picture {
    palette: Vec<u32>,
    pixels: Vec<u8>,
}

impl Picture {
    fn load(reg: &mut Registry, scene: &str) -> anyhow::Result<Picture> {
        let palette = reg
            .palette(&format!("palette.{scene}"))
            .map(|r| r.value.clone())
            .ok_or_else(|| anyhow::anyhow!("no palette for {scene}"))?;
        let img = reg.image(scene)?;
        anyhow::ensure!(
            img.width == SCREEN_W && img.height == SCREEN_H,
            "{scene} is {}x{}, expected a full screen",
            img.width,
            img.height
        );
        Ok(Picture {
            palette,
            pixels: img.pixels.clone(),
        })
    }

    fn show(&self, fb: &mut Framebuffer) {
        fb.set_palette(&self.palette);
        fb.pixels.copy_from_slice(&self.pixels);
    }
}

/// A door standing open: which routine is running and what it has loaded.
pub enum Open {
    /// `_TAVERN` at 0xb007: `tav.piv` under the hand, `dice.piv` for the
    /// throw, and the state machine.
    Tavern {
        table: Picture,
        dice: Picture,
        state: Box<Tavern>,
    },
    /// `_WIZARD` at 0xba66 or 0xb935: one picture and the visit.
    Counter { picture: Picture, state: Visit },
    /// `0xbdd3` with `StatTYPE` 5 or 6: a page of the panel, which
    /// [`crate::status`] draws.
    Panel(Screen),
}

impl Open {
    /// Open the door: load what its routine loads. `None` for a routine that
    /// returns before it draws anything, which is the tavern's `cmp word ptr
    /// [si+0x32], 0; jg` at 0xb00b, or for a pack that lacks the picture.
    pub fn through(reg: &mut Registry, door: Door, run: &Run) -> Option<Open> {
        let load = |reg: &mut Registry, scene: &str| match Picture::load(reg, scene) {
            Ok(p) => Some(p),
            Err(e) => {
                eprintln!("cannot open {door:?}: {e:#}");
                None
            }
        };
        Some(match door {
            Door::Merchant => Open::Panel(Screen::Merchant),
            Door::Temple => Open::Panel(Screen::Temple),
            Door::Tavern => Open::Tavern {
                state: Box::new(Tavern::open(run)?),
                table: load(reg, TAVERN_SCENE)?,
                dice: load(reg, DICE_SCENE)?,
            },
            Door::Healer => Open::Counter {
                picture: load(reg, HEALER_SCENE)?,
                state: Visit::open(Counter::Healer),
            },
            Door::Mystic => Open::Counter {
                picture: load(reg, MYSTIC_SCENE)?,
                state: Visit::open(Counter::Mystic),
            },
        })
    }

    /// The panel page this door is, if it is one.
    pub fn panel(&self) -> Option<Screen> {
        match self {
            Open::Panel(s) => Some(*s),
            _ => None,
        }
    }

    /// The music key the pack's table is read with: the three rooms that
    /// load a tune (`mov ax, n; call 0x900d; mov ah, 0; int 60h` at 0xb012,
    /// 0xba66 and 0xb935).
    pub fn music_key(&self) -> Option<&'static str> {
        match self {
            Open::Tavern { .. } => Some("door.tavern"),
            Open::Counter { state, .. } => Some(match state.counter {
                Counter::Healer => "door.healer",
                Counter::Mystic => "door.mystic",
            }),
            Open::Panel(_) => None,
        }
    }

    /// Whether the routine has returned, which is `HWINIT`'s cue.
    pub fn closed(&self) -> bool {
        match self {
            Open::Tavern { state, .. } => state.left,
            Open::Counter { state, .. } => state.stage == Stage::Done,
            Open::Panel(_) => false,
        }
    }

    /// `CLEARGADGETS` and the `ADDGADGET`s of whichever loop is running:
    /// the tavern's six (0xb053) while the table is up, and the bowl's four
    /// (`InitDonation`, 0xbb48) while it is. The dice picture and the two
    /// chains have none: they wait on `WaitFIRE`, which reads the stick and
    /// not the table. The pointer is drawn on the screens that have gadgets,
    /// which is the same three of the six that blit `PO.CEL`:
    /// `TavernLoop+57`, `DonateLoop+52` and `StatLOOP+19`.
    pub fn gadgets(&self, gadgets: &mut Gadgets) {
        match self {
            Open::Tavern { state, .. } if state.screen == TavernScreen::Table => {
                for (x, y, w, h, id, strp) in henge_core::town::TAVERN_GADGETS {
                    gadgets.add(Gadget {
                        id,
                        x,
                        y,
                        w,
                        h,
                        label: String::new(),
                        payload: Payload::new(strp, 0x32),
                        lit: true,
                    });
                }
            }
            Open::Counter {
                state:
                    Visit {
                        stage: Stage::Bowl(_),
                        ..
                    },
                ..
            } => {
                for (n, (x, y, w, h, op)) in DONATE_GADGETS.into_iter().enumerate() {
                    gadgets.add(Gadget {
                        id: n,
                        x,
                        y,
                        w,
                        h,
                        label: String::new(),
                        payload: Payload::new(op as u16, 0x32),
                        lit: true,
                    });
                }
            }
            _ => {}
        }
    }

    /// Paint the screen, all but the pointer. The panel is not drawn here.
    pub fn render(
        &self,
        reg: &mut Registry,
        fb: &mut Framebuffer,
        font: Option<&Font>,
        run: &Run,
        hand: impl FnOnce(&mut Registry, &mut Framebuffer),
    ) {
        match self {
            Open::Tavern { table, dice, state } => match state.screen {
                TavernScreen::Table => {
                    table.show(fb);
                    // `0x9702` and `0x975b`: the tasks, which is the hand.
                    hand(reg, fb);
                    if let Some(font) = font {
                        draw_word(reg, fb, font, &Tavern::gold_word(run));
                    }
                }
                TavernScreen::Dice => {
                    dice.show(fb);
                    if let Some(t) = state.result.as_ref() {
                        for (n, (x, y)) in henge_core::town::DICE_AT.into_iter().enumerate() {
                            sprite::draw(reg, fb, DICE_SHEET, t.dice[n] as usize, x, y, false);
                        }
                    }
                    if let Some(font) = font {
                        for w in state.result_words(run) {
                            draw_word(reg, fb, font, &w);
                        }
                    }
                }
            },
            Open::Counter { picture, state } => {
                picture.show(fb);
                let Some(font) = font else { return };
                match &state.stage {
                    Stage::Greeting => {
                        for w in state.counter.greeting() {
                            draw_word(reg, fb, font, &w);
                        }
                    }
                    Stage::Bowl(bowl) => draw_bowl(reg, fb, font, bowl, state.counter),
                    Stage::Verdict { words, .. } => {
                        for w in words {
                            draw_word(reg, fb, font, w);
                        }
                    }
                    Stage::Done => {}
                }
            }
            Open::Panel(_) => {}
        }
    }
}

/// `TextP` at 0x7a70 with the registers a caller loads, or one record of a
/// chain: centred between `TextLeftBorder` 0 and `TextRightBorder` 0x140 when
/// bit 0 is set, else at its own x.
fn draw_word(reg: &mut Registry, fb: &mut Framebuffer, font: &Font, w: &Word) {
    if w.centred {
        font.draw_own_centred(reg, fb, &w.text, w.y);
    } else {
        font.draw_own(reg, fb, &w.text, w.x, w.y);
    }
}

/// `_WIZARD:DonationRefresh` at image `0xbc9e`, blit for blit.
///
/// ```text
/// cel BAG   at (2, 0xa2) and (0x10e, 0xa2)   the two purses
/// cel 2     at (0xad, 0xb9)                  leave
/// cel 3     at (0x83, 0xb9)                  take it
/// cel 4     at (0x90, 0xa9)                  one coin off
/// cel 5     at (0xa2, 0xa9)                  one coin on
/// Don       x 0x106, y 0xbe, cx 4            `Donation`, right aligned, which
///                                            TextPTop puts at the right border
///                                            less its length and not at 0x106
/// YGOL      x 2, y 0xbe, cx 0                `Your Gold`
/// ```
///
/// and `DonateLoop` itself writes `GOLDP` at (0x14, 0xaf) and `DONATION` at
/// (0x120, 0xaf) every time round. `BAG` is the word `InitDonation` is handed
/// in `ax`: 1 from the healer (0xbaba), 0 from the mystic (0xb989).
fn draw_bowl(
    reg: &mut Registry,
    fb: &mut Framebuffer,
    font: &Font,
    bowl: &Donation,
    counter: Counter,
) {
    for x in [2, 0x10e] {
        sprite::draw(reg, fb, GOLD_SHEET, counter.bag_cel(), x, 0xa2, false);
    }
    for (cel, (x, y, _, _, _)) in DONATE_CELS.into_iter().zip(DONATE_GADGETS) {
        sprite::draw(reg, fb, GOLD_SHEET, cel, x, y, false);
    }
    let don = "Donation";
    let w = font.width(reg, don);
    font.draw_own(reg, fb, don, SCREEN_W as i32 - w, 0xbe);
    font.draw_own(reg, fb, "Your Gold", 2, 0xbe);
    font.draw_own(reg, fb, &bowl.purse.to_string(), 0x14, 0xaf);
    font.draw_own(reg, fb, &bowl.given.to_string(), 0x120, 0xaf);
}

/// A one-line summary for the headless trace.
pub fn describe(open: &Open, run: &Run) -> String {
    match open {
        Open::Tavern { state, .. } => match state.screen {
            TavernScreen::Table => format!(
                "TAVERN {:?} bet {} gold {}",
                state.table.throw, state.bet, run.gold
            ),
            TavernScreen::Dice => {
                let words: Vec<String> = state
                    .result_words(run)
                    .into_iter()
                    .map(|w| w.text)
                    .collect();
                format!("DICE {}", words.join(" / "))
            }
        },
        Open::Counter { state, .. } => {
            let who = match state.counter {
                Counter::Healer => "HEALER",
                Counter::Mystic => "MYSTIC",
            };
            match &state.stage {
                Stage::Greeting => format!("{who} greeting"),
                Stage::Bowl(b) => format!("{who} bowl purse {} donation {}", b.purse, b.given),
                Stage::Verdict { words, .. } => {
                    let t: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
                    format!("{who} {}", t.join(" "))
                }
                Stage::Done => format!("{who} done"),
            }
        }
        Open::Panel(s) => format!("PANEL {s:?}"),
    }
}
