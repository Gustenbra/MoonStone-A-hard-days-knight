//! Desktop entry point.
//!
//! Release builds contain no original-game data. `--features research` adds a
//! viewer for studying the 1991 files, which is a development tool only.

mod ending;
mod framebuffer;
mod input;
mod map;
mod place;
mod shell;
mod sprite;
mod status;
mod text;
mod world;
#[cfg(feature = "research")]
mod research;

use framebuffer::Framebuffer;
use henge_assets::Registry;
use henge_audio::{Clips, Sink, Voices};
use map::MapScene;
use text::Font;
use henge_core::combat::{Intent, State};
use henge_core::item::{Items, Loss};
use henge_core::knight::{Ability, Knight, Knights, MAX_ABILITY};
use henge_core::intro::Intro;
use henge_core::message::{Message, Messages};
use henge_core::place::{Answer, Approach, Places};
use henge_core::pointer::{Gadgets, Pointer};
use henge_core::run::{Cast, Challenge, Run};
use henge_core::save::Save;
use henge_core::shell::Start;
use henge_core::{SCREEN_H, SCREEN_W};
use world::{Sheet, World};
use std::num::NonZeroU32;
use std::rc::Rc;
use winit::event::{ElementState, Event, WindowEvent};
use winit::event_loop::EventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::WindowBuilder;

fn args_of() -> Vec<String> { std::env::args().collect() }

/// What the pack holds, for one column of the trace. Ids rather than names,
/// because a trace is read against the data and the data is keyed by id.
fn carrying(run: &Run) -> String {
    if run.kit.is_empty() {
        return "-".into();
    }
    run.kit
        .iter()
        .map(|(id, n)| if n == 1 { id.to_string() } else { format!("{id}x{n}") })
        .collect::<Vec<_>>()
        .join(",")
}

/// A scripted run for the headless modes, so a screen that needs walking to and
/// a menu driving is reachable without a display.
///
///   --at <place-id>   start standing in that place
///   --goto <x>,<y>    walk there first, the way a held key would
///   --input <script>  one key press per tick afterwards:
///                     u/d move the highlight, s takes the option,
///                     h/j/k/l walk, . waits
///   --hurt <hp>       start the run already wounded, so a healer has something
///                     to do without first having to win and lose a fight
///   --gold <n>        start the run with coin, so a stall can be reached
///                     without first winning the fights that pay for it
///   --keys <0..4>     start holding that many of the four lair keys
///   --stone <name>    start carrying one of the four moonstones
///   --lives <n>       how many life points to ride out with
///   --start <screen>  title, select, map or arena. Defaults to the map, so
///                     every recipe written before the shell existed still does
///                     what it did
///   --knight <0..3>   begin the run as one of the four, without going through
///                     the select screen
///   --sheet           hold the character sheet open over whatever is drawn
///   --foe <actor>     fill the opponents' seats with that creature, so each
///                     of the bestiary can be captured on its own
///   --bloodless       the title's gore switch, off: gated parts are dropped
///                     and the decapitation becomes a collapse
///   --scripts         (trace only) name the script each fighter is on, so a
///                     death variant can be told from another
///
/// In `--input`, a digit is the joystick held with fire, laid out like a
/// numpad: 8 up, 2 down, 4 left, 6 right, 7 9 1 3 the diagonals, 5 fire on
/// its own. That is how the direction chosen attacks are reached headlessly.
fn hurt_arg(a: &[String]) -> Option<i32> {
    a.iter().position(|s| s == "--hurt").and_then(|i| a.get(i + 1)).and_then(|v| v.parse().ok())
}

fn gold_arg(a: &[String]) -> Option<u32> {
    a.iter().position(|s| s == "--gold").and_then(|i| a.get(i + 1)).and_then(|v| v.parse().ok())
}

/// `--keys <0..4>`: start holding that many of the four lair keys, so the
/// Valley of the Gods can be reached without first clearing four lairs.
fn keys_arg(a: &[String]) -> Option<usize> {
    a.iter().position(|s| s == "--keys").and_then(|i| a.get(i + 1)).and_then(|v| v.parse().ok())
}

/// `--lives <n>`: how many life points to ride out with, so the game-over
/// screen is one lost fight away instead of five.
fn lives_arg(a: &[String]) -> Option<i32> {
    a.iter().position(|s| s == "--lives").and_then(|i| a.get(i + 1)).and_then(|v| v.parse().ok())
}

/// `--stone <new|full|half|gibbous>`: start carrying that moonstone, so the
/// stone circle's winning branch can be reached without beating the Guardian.
fn stone_arg(a: &[String]) -> Option<henge_core::moon::Moonstone> {
    let name = a.iter().position(|s| s == "--stone").and_then(|i| a.get(i + 1))?;
    let found = henge_core::moon::Moonstone::ALL
        .into_iter()
        .find(|m| m.item().trim_start_matches("moonstone.") == name);
    if found.is_none() {
        eprintln!("no moonstone called {name}: try new, full, half or gibbous");
    }
    found
}

fn knight_arg(a: &[String]) -> Option<usize> {
    a.iter().position(|s| s == "--knight").and_then(|i| a.get(i + 1)).and_then(|v| v.parse().ok())
}

fn foe_arg(a: &[String]) -> Option<String> {
    a.iter().position(|s| s == "--foe").and_then(|i| a.get(i + 1)).cloned()
}

/// Which screen a headless run opens on.
///
/// The default is the map, so every recipe written before the shell existed
/// still does what it did. An interactive run opens on the title, because that
/// is where a game opens.
fn start_arg(a: &[String]) -> Option<Mode> {
    let name = a.iter().position(|s| s == "--start").and_then(|i| a.get(i + 1))?;
    match name.as_str() {
        "intro" => Some(Mode::Intro),
        "title" => Some(Mode::Title),
        "select" => Some(Mode::Select),
        "map" => Some(Mode::Map),
        "arena" | "combat" => Some(Mode::Combat),
        other => {
            eprintln!("no screen called {other}: try intro, title, select, map or arena");
            None
        }
    }
}

/// `--point x,y`: put the pointer there, for checking the gadgets headlessly.
fn point_arg(a: &[String]) -> Option<(i32, i32)> {
    let v = a.iter().position(|s| s == "--point").and_then(|i| a.get(i + 1))?;
    let (x, y) = v.split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

/// `--save <path>`: where a saved game goes. One slot, because the original
/// has none at all and a slot list is a thing to design once somebody wants
/// more than one.
fn save_path_arg(a: &[String]) -> String {
    a.iter()
        .position(|s| s == "--save")
        .and_then(|i| a.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "henge-save.json".to_string())
}

/// `--controls <path>`: where the binding table and the stick calibration live.
/// Beside the save, by the same reasoning: it is a setting, not a game.
fn controls_path_arg(a: &[String]) -> String {
    a.iter()
        .position(|s| s == "--controls")
        .and_then(|i| a.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "henge-controls.json".to_string())
}

#[derive(Default)]
struct Script {
    goto: Option<(i32, i32)>,
    keys: Vec<char>,
    at: Option<String>,
    /// Suppress ambushes. Checking how the map draws anywhere but the starting
    /// corner was otherwise impossible: the traveller is killed en route long
    /// before arriving, so half the map could never be looked at.
    peaceful: bool,
}

impl Script {
    fn from_args(a: &[String]) -> Script {
        let after = |flag: &str| {
            a.iter().position(|s| s == flag).and_then(|i| a.get(i + 1)).cloned()
        };
        Script {
            goto: after("--goto").and_then(|v| {
                let (x, y) = v.split_once(',')?;
                Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
            }),
            keys: after("--input").map(|s| s.chars().collect()).unwrap_or_default(),
            at: after("--at"),
            peaceful: a.iter().any(|s| s == "--peaceful"),
        }
    }

    fn active(&self) -> bool {
        self.goto.is_some() || !self.keys.is_empty() || self.at.is_some()
    }

    /// Drive one tick: steer while there is still ground to cover, then start
    /// feeding key presses.
    fn drive(&self, app: &mut App, fed: &mut usize) {
        if let Some((gx, gy)) = self.goto {
            match app.mode {
                // Ambushes happen on the way, and a traveller who never swings
                // dies to the first one, so swing while walking.
                Mode::Combat => {
                    app.keys = [false; 256];
                    app.keys[6] = app.tick % 23 < 4;
                    return;
                }
                Mode::Map => {
                    // `is_none_or` would read better but postdates the crate's
                    // minimum Rust version.
                    let there = match app.map.as_ref() {
                        Some(m) => m.state.x == gx && m.state.y == gy,
                        None => true,
                    };
                    if !there {
                        app.keys = [false; 256];
                        app.steer(gx, gy);
                        return;
                    }
                }
                // Arrived, or in front of a screen the script drives directly.
                _ => {}
            }
        }
        let c = self.keys.get(*fed).copied();
        if c.is_some() {
            *fed += 1;
        }
        app.drive(c);
    }
}

/// The flags both headless modes share.
///
/// Order matters: who you are first, because taking a knight sets the health the
/// rest of the run is measured against and moves you to their corner of the map;
/// then which screen; then whatever state the run is being posed in.
fn prepare(app: &mut App, a: &[String]) {
    if let Some(k) = knight_arg(a) {
        app.take_knight(k, App::roster_led_by(k));
    }
    if let Some(mode) = start_arg(a) {
        app.mode = mode;
        if mode == Mode::Select {
            app.begin_select();
        }
    }
    if let Some(hp) = hurt_arg(a) {
        app.run.health = hp;
    }
    if let Some(g) = gold_arg(a) {
        app.run.gold = g;
    }
    // The quest, posed. Four lairs and a Guardian is not something a headless
    // run can play through, so the tokens they hand over can be handed over
    // here instead. Nothing else about the run changes: the keys and the stone
    // are ordinary pack entries and every rule reads them the same way.
    if let Some(n) = keys_arg(a) {
        for key in henge_core::moon::Key::ALL.into_iter().take(n) {
            app.run.kit.take(key.item(), 1);
        }
    }
    if let Some(stone) = stone_arg(a) {
        app.run.kit.take(stone.item(), 1);
    }
    // `--won`: end the run as a victory, so the winning tally can be looked at
    // without beating the Guardian first. The losing one is one lost fight
    // away, but the winning one is the end of a whole quest, and a screen
    // nobody can reach in testing is a screen nobody checks.
    if a.iter().any(|s| s == "--won") {
        app.run.won = true;
    }
    if let Some(n) = lives_arg(a) {
        app.run.lives = n;
        app.run.max_lives = n.max(app.run.max_lives);
    }
    // Last, so it survives whatever taking a knight did to the seats.
    if let (Some(foe), Some(w)) = (foe_arg(a), app.world.as_mut()) {
        if !w.set_foe(&foe) {
            std::process::exit(2);
        }
    }
    // `--load` replaces everything above it: a save is the state, and posing a
    // run and then loading over it would be posing nothing.
    if a.iter().any(|s| s == "--load") {
        if !app.load_game() {
            std::process::exit(3);
        }
    }
    // `--point x,y` puts the pointer somewhere, which is the only way to reach
    // it with no mouse and no display.
    if let Some((x, y)) = point_arg(a) {
        app.point_at(x, y);
    }
    app.sheet = a.iter().any(|s| s == "--sheet");
    if a.iter().any(|s| s == "--bloodless") {
        app.title.state.gore = false;
        if let Some(w) = app.world.as_mut() {
            w.set_gore(false);
        }
    }
}

fn main() -> anyhow::Result<()> {
    let mut app = App::new()?;

    // Headless trace: run the simulation and report it, so combat can be checked
    // as behaviour rather than by squinting at screenshots.
    if let Some(i) = args_of().iter().position(|a| a == "--trace") {
        let a = args_of();
        let ticks: u64 = a.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(600);
        let arena: i32 = a.get(i + 2).and_then(|s| s.parse().ok()).unwrap_or(0);
        if let Some(w) = app.world.as_mut() { w.step_arena(arena); }
        let script = Script::from_args(&a);
        let scripts = a.iter().any(|s| s == "--scripts");
        app.peaceful = script.peaceful;
        prepare(&mut app, &a);
        if let Some(id) = script.at.as_deref() { app.enter(id); }
        // Travel while tracing, so the overworld loop is exercised too.
        app.keys[3] = true;
        let mut last = String::new();
        let mut fed = 0usize;
        for t in 0..ticks {
            if script.active() {
                script.drive(&mut app, &mut fed);
            } else {
                // Wander rather than walking into the edge and stopping, and swing
                // often enough to actually win some fights. A trace where the player
                // never fights back proves nothing about what carries between bouts.
                app.keys[3] = (t / 140) % 2 == 0;
                app.keys[2] = (t / 140) % 2 == 1;
                app.keys[6] = t % 23 < 4;
                // A menu reads presses, not held keys, so a wanderer with only
                // keys down parks on the first door it walks into and the rest
                // of the run proves nothing. Stepping the highlight and taking
                // it means a wandering run goes into a town or a lair, does
                // something there, and comes back out.
                app.pressed[1] = t % 11 == 0;
                app.pressed[6] = t % 23 == 0;
            }
            app.update();
            // A message box is modal, so the mode underneath it is not what is
            // on screen. Say which kind and what it says, or a trace would
            // report a town menu nobody can see.
            let line = if let Some(m) = app.showing.as_ref() {
                let said: Vec<&str> = m.shown().map(|l| l.text.trim()).collect();
                format!("{t:>5}  MESSAGE {:?}  {}", m.kind, said.join(" / "))
            } else { match app.mode {
                // The shell has no state worth a line of trace: what it does is
                // decided by looking at it, and it is checked by test in
                // `henge_core::shell` instead.
                Mode::Intro => format!("{t:>5}  INTRO   card {}", app.intro.card),
                Mode::Title => format!("{t:>5}  TITLE"),
                Mode::Select => format!("{t:>5}  SELECT"),
                Mode::Map if app.map.is_none() => break,
                Mode::Combat if app.world.is_none() => break,
                Mode::Place if app.visiting.is_none() => break,
                Mode::Map => {
                    let Some(m) = app.map.as_ref() else { break };
                    let r = &app.run;
                    // The three abilities and the experience ride on the
                    // line too, since what a level buys is checked by
                    // watching them move.
                    let k = &r.knight;
                    let aloft = match app.flight {
                        Some(Flight { returns: true, .. }) => " gem",
                        Some(Flight { returns: false, .. }) => " hawk",
                        None => "",
                    };
                    format!("{:>5}  MAP     day {:<3} at {:>3},{:<3} hp{:>4}  gold{:>5} {:<12} won {:<3} fought {:<3} {} on {} {} s{}c{}e{} xp{}{}{}",
                        t, m.state.day, m.state.x, m.state.y, r.health, r.gold, carrying(r),
                        r.victories, r.fights,
                        if r.alive() { "     " } else { "ENDED" }, m.last_terrain.name(),
                        // The moon, because four days move it and what waits in
                        // an arena moves with it: a calendar is only checkable
                        // if it is on the line.
                        r.moon.phase().key(),
                        k.strength, k.constitution, k.endurance, r.experience, aloft,
                        if app.sheet { format!(" SHEET > {}", app.sheet_rows().get(app.sheet_cursor).map_or("", |r| r.0.as_str())) } else { String::new() })
                }
                Mode::Place => {
                    let Some(s) = app.visiting.as_ref() else { break };
                    let Some(def) = app.places.get(&s.visit.place) else { break };
                    // The tune, when the room has one. Five of the original's
                    // rooms do and nothing else in the game does, so a trace is
                    // the way to check that the right one is on and that it
                    // stops at the door.
                    let tune = match app.audio.music() {
                        Some(id) => format!("  [{id}]"),
                        None => String::new(),
                    };
                    format!("{:>5}  PLACE   day {:<3} hp{:>4}  gold{:>5} {:<12} {:<34} {}{}",
                        t, app.run.day, app.run.health, app.run.gold, carrying(&app.run),
                        place::describe(def, &s.visit, &app.items), s.visit.said, tune)
                }
                Mode::Combat => {
                    let Some(w) = app.world.as_ref() else { break };
                    let mut who: Vec<String> = w
                        .bout
                        .fighters
                        .iter()
                        .map(|f| {
                            // The state, and the attack kind when there is one:
                            // `Attack:chop`, `Guard:block`, so a trace shows which
                            // of the eight the direction chose.
                            let state = match f.attack {
                                Some(a) if matches!(f.state, State::Attack | State::Guard) => {
                                    format!("{:?}:{}", f.state, a.name())
                                }
                                _ => format!("{:?}", f.state),
                            };
                            // `--scripts` adds the script each task is on, which
                            // is how a death variant is told from another.
                            let script = if scripts {
                                f.task.as_ref().map_or(String::new(), |t| format!(" {}", t.pc.script))
                            } else {
                                String::new()
                            };
                            format!("{:<6} {:<12}{:>4} @{:>3},{:>3}{}", f.actor, state, f.health, f.x, f.y, script)
                        })
                        .collect();
                    // A dagger in the air is a line of its own, and a blow
                    // stopped is said so, since nothing else would show it.
                    for m in &w.bout.missiles {
                        if m.attack.is_some() {
                            who.push(format!("knife @{:>3},{:>3}", m.task.x, m.depth));
                        }
                    }
                    for p in &w.bout.parries {
                        who.push(format!("{} blocked {} with {}", p.target, p.attacker, p.with.name()));
                    }
                    // The arena's own name as well as its family, because which of
                    // the eight a family rotates to is now a thing worth seeing.
                    format!("{:>5}  COMBAT  {:<5} {:<8} {}", t, w.name(), w.family(), who.join(" | "))
                }
            } };
            let key = line[7..].to_string();
            if key != last { println!("{line}"); last = key; }
        }
        return Ok(());
    }

    // Headless capture, for checking rendering without a display and for CI.
    //   moonstone --screenshot out.png [ticks] [arena-index]
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--screenshot") {
        let path = args.get(i + 1).cloned().unwrap_or_else(|| "shot.png".into());
        let ticks: u64 = args.get(i + 2).and_then(|s| s.parse().ok()).unwrap_or(0);
        let arena: i32 = args.get(i + 3).and_then(|s| s.parse().ok()).unwrap_or(0);
        if let Some(w) = app.world.as_mut() {
            w.step_arena(arena);
        }
        app.keys[3] = args.iter().any(|a| a == "--walk");
        app.keys[6] = args.iter().any(|a| a == "--fight");
        let script = Script::from_args(&args);
        app.peaceful = script.peaceful;
        prepare(&mut app, &args);
        if let Some(id) = script.at.as_deref() { app.enter(id); }
        let mut fed = 0usize;
        for _ in 0..ticks {
            if script.active() {
                script.drive(&mut app, &mut fed);
            }
            app.update();
            // Drawn every tick, not just at the end. A window redraws each
            // frame and so learns the palette each screen loads; a capture that
            // only drew once would tick a whole run of cycles and glows against
            // whatever palette happened to be up when it started.
            app.render();
        }
        if !args.iter().any(|a| a == "--fade") {
            app.settle_fade();
        }
        app.render();
        // --say draws a line of text over whatever was rendered, so the font
        // and its glyph mapping can be checked directly rather than by hunting
        // for a frame that happens to show the status bar.
        if let Some(j) = args.iter().position(|a| a == "--say") {
            if let Some(line) = args.get(j + 1).cloned() {
                app.say(&line);
            }
        }
        app.save_png(&path)?;
        println!("wrote {path} ({})", app.status);
        // The composed palette, which is the only way to see a cycle or a glow
        // on an entry the picture hardly uses. Item 75's own check.
        if args.iter().any(|a| a == "--palette") {
            let p = app.palette_now();
            let hex: Vec<String> = p.iter().map(|c| format!("{c:06x}")).collect();
            println!("palette {}", hex.join(" "));
        }
        return Ok(());
    }

    // A game opens on its title screen. `--start` overrides it, which is how the
    // window can still be pointed straight at a map or an arena.
    // The original runs `INTR.EXE` and then `MAIN.EXE`, so a window opens on
    // the intro and the title follows it. Fire, space or Tab skips it, and
    // `--start title` goes straight there. Every headless recipe is unchanged:
    // those go through `prepare`, which still defaults to the map.
    app.mode = start_arg(&args_of()).unwrap_or(Mode::Intro);
    if app.mode == Mode::Select {
        app.begin_select();
    }

    // Gamepads. No pad, and no way to look for one, are both normal: the game
    // says so once and plays on the keys, exactly as it does with no sound card.
    let mut pads = input::Pads::open();
    match pads.note.as_deref() {
        Some(note) => println!("{note}"),
        None => match pads.count() {
            0 => println!("gamepads: none plugged in; F11 calibrates one when there is"),
            n => println!("gamepads: {n} found; F11 calibrates player one's"),
        },
    }

    let event_loop = EventLoop::new()?;
    let window = Rc::new(
        WindowBuilder::new()
            .with_title("Moonstone")
            .with_inner_size(winit::dpi::LogicalSize::new(
                (SCREEN_W * 3) as u32,
                (SCREEN_W * 3 * 3 / 4) as u32,
            ))
            .build(&event_loop)?,
    );
    let context = softbuffer::Context::new(window.clone())
        .map_err(|e| anyhow::anyhow!("no graphics context: {e}"))?;
    let mut surface = softbuffer::Surface::new(&context, window.clone())
        .map_err(|e| anyhow::anyhow!("no drawing surface: {e}"))?;

    // The simulation runs at a fixed sixty ticks a second, whatever the
    // machine can draw. Without this the loop ran a tick per frame and a frame
    // as fast as the window could blit, so a fast machine played the whole
    // game several times too quickly: an opponent crossed the arena in a
    // blink and swung faster than a person can read, and practice was over in
    // two seconds. Time is accumulated and spent in whole ticks so the
    // simulation never sees a fractional step, which is what keeps two
    // machines agreeing on it.
    const TICK: std::time::Duration = std::time::Duration::from_micros(16_667);
    let mut last = std::time::Instant::now();
    let mut owed = std::time::Duration::ZERO;

    event_loop.run(move |event, elwt| {
        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => elwt.exit(),
            // A real mouse moves the pointer straight to where it is. The
            // stick still works; this is the same pointer either way.
            Event::WindowEvent { event: WindowEvent::CursorMoved { position, .. }, .. } => {
                let size = window.inner_size();
                if let Some((x, y)) = Framebuffer::to_screen(
                    size.width as usize, size.height as usize, position.x, position.y,
                ) {
                    app.point_at(x, y);
                }
            }
            Event::WindowEvent { event: WindowEvent::MouseInput { state, button, .. }, .. } => {
                if button == winit::event::MouseButton::Left {
                    let down = state == ElementState::Pressed;
                    // Straight onto seat two's fire, which is the button the
                    // gadgets already read.
                    if down && !app.keys[11] {
                        app.pressed[11] = true;
                    }
                    app.keys[11] = down;
                }
            }
            Event::WindowEvent {
                event: WindowEvent::KeyboardInput { event, .. }, ..
            } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    let down = event.state == ElementState::Pressed;
                    if down && code == KeyCode::Escape {
                        elwt.exit();
                    }
                    // `Fix_JoyStick`, on a key of our choosing because the
                    // original reached it from a menu this shell does not have.
                    if down && code == KeyCode::F11 {
                        app.calibrate(0);
                    }
                    app.key(code, down);
                }
            }
            Event::AboutToWait => {
                let now = std::time::Instant::now();
                owed += now - last;
                last = now;
                // A stall (a dragged window, a sleeping laptop) must not be
                // paid back as a burst of ticks; cap what can be owed.
                if owed > TICK * 6 {
                    owed = TICK * 6;
                }
                // The sticks, once a frame and before the ticks they feed.
                // The original reads them in its own frame loop and ORs them
                // into the key word, and this is the same place.
                pads.poll();
                app.pads_tick(&pads);
                let mut ticked = false;
                while owed >= TICK {
                    app.update();
                    owed -= TICK;
                    ticked = true;
                }
                elwt.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(last + (TICK - owed)));
                if !ticked {
                    return;
                }
                let size = window.inner_size();
                let (Some(w), Some(h)) =
                    (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                else {
                    return;
                };
                if surface.resize(w, h).is_err() {
                    return;
                }
                if let Ok(mut buffer) = surface.buffer_mut() {
                    app.render();
                    let palette = app.palette_now();
                    app.fb.present_into(
                        &mut buffer,
                        size.width as usize,
                        size.height as usize,
                        &palette,
                    );
                    let _ = buffer.present();
                }
            }
            _ => {}
        }
    })?;
    Ok(())
}

/// `MudmenGlowOn`: `COLOURGLOW(0x0e, 0x100, 2, 0)`. Palette entry fourteen
/// breathes towards a dark red every other frame for as long as the bout runs.
/// Recovered, and installed by `InitCombat` when mudmen are in the arena.
const MUDMEN_GLOW: henge_assets::Glow =
    henge_assets::Glow { index: 0x0e, target: 0x100, period: 2, repeat: 0 };

struct App {
    fb: Framebuffer,
    /// `COLCON`'s two tables and the fade, applied to the palette on its way
    /// to the screen. Item 75; see `henge_assets::palette`.
    fx: henge_assets::Effects,
    /// What each screen installs, out of the pack rather than out of code.
    fx_table: henge_assets::EffectTable,
    /// Which screen is up, so a change of screen can fade the new one in the
    /// way every one of the original's own loaders does.
    scene: String,
    /// Place id to tune id, recovered from `LOADMUSIC`'s callers.
    music_places: std::collections::BTreeMap<String, String>,
    tick: u64,
    keys: [bool; 256],
    /// Keys that went down this tick. A menu wants presses, not held keys, or
    /// one tap of Down would run the highlight off the bottom of the list.
    pressed: [bool; 256],
    reg: Registry,
    world: Option<World>,
    map: Option<MapScene>,
    /// Everywhere there is to go, and whether we are standing in one of them.
    places: Places,
    /// What there is to buy, carry and drink. Shared by every stall, because a
    /// flask is the same flask in Highwood as in Waterdeep.
    items: Items,
    /// The four knights, in select order.
    knights: Knights,
    title: shell::TitleScene,
    select: Option<shell::SelectScene>,
    /// Whether the character sheet is up over whatever else is on screen.
    sheet: bool,
    approach: Approach,
    visiting: Option<place::PlaceScene>,
    mode: Mode,
    audio: Box<dyn Sink>,
    voices: Voices,
    fonts: std::collections::BTreeMap<String, Font>,
    /// Suppress ambushes, so map rendering can be checked anywhere.
    peaceful: bool,
    /// Which key and which stick raise which action. Ours, and data: item 76.
    bindings: input::Bindings,
    /// Where the bindings and the calibration are kept.
    controls_path: String,
    /// The keyboard's half of the ten seat slots. A pad and a key can both hold
    /// the same action, exactly as the original ORs `JOY1` into its key word,
    /// so the two halves are kept apart and combined.
    kb: [bool; 256],
    /// The pads' half.
    pad_held: [bool; 256],
    /// `BOUNCEBUTTON`, one per seat, for the calibration screen.
    bounce: [input::Debounce; 2],
    /// `Fix_JoyStick`, when it is running.
    calibrating: Option<(usize, input::Calibrating)>,
    run: Run,
    /// The last thing a cutpurse took, for the map to say so. The map has no
    /// message line of its own, so the notice rides on the purse plate in the
    /// corner, beside the number it just changed.
    robbed: String,
    /// Ticks the robbery notice has left on screen.
    robbed_for: u32,
    /// Ticks since the run ended, so the tally can be read before it restarts.
    run_over_for: u32,
    /// Aloft on the gem or the hawk. Desktop state rather than the run's,
    /// like the map position it belongs with: `GemXY` in the original is
    /// beside the token, not on the knight record.
    flight: Option<Flight>,
    /// The highlighted line of the character sheet's menu.
    sheet_cursor: usize,
    /// The lair this bout is being fought for, if it is one. A raid returns to
    /// the lair's own page rather than to the map, because the floor is only
    /// yours once the guardian is down and the spoils are read there.
    raiding: Option<usize>,
    /// This bout is the Valley of the Gods' Guardian. Winning spends the four
    /// keys and pays a moonstone; losing costs two life points and leaves the
    /// keys where they are, so the gate stays open.
    questing: bool,
    /// The place to reopen when a raid or the Valley is over.
    raid_place: String,
    /// The between-days screen, and how many ticks it has left.
    ///
    /// `_MAP:NextWHICH` puts it up the moment the last knight has had his
    /// turn, so it sits between one day and the next and nothing else runs
    /// while it is there.
    interlude: u32,
    /// Which of the fourteen hints is next. `_LOADER:WaitCOUNT`, which the
    /// original steps every time it shows one and wraps at fourteen.
    hint: usize,
    /// A practice bout is not part of a run: nothing carries, nobody is slain,
    /// and it goes back to the title when it is over.
    practice: bool,
    /// The pointer, and the boxes on this screen it can be over.
    ///
    /// `MovePointer` and the gadget table, in `henge_core::pointer`. The
    /// original drives the pointer with the stick and so does this: seat two's
    /// keys steer it on any screen that has gadgets, and a real mouse moves it
    /// straight to where the mouse is, because a window with a mouse in it
    /// should behave like one.
    pointer: Pointer,
    gadgets: Gadgets,
    /// Every message the executable gives up, and the counter the wait
    /// messages come off. `WaitCOUNT` is `hint`, above.
    messages: Messages,
    /// A message box up over everything, and how many ticks it has left. Modal,
    /// which is what all three of `WAITMESSAGE`, `OCCURMESSAGE` and
    /// `INSTRUCTMESSAGE` are: they draw, they fade, and nothing else runs.
    showing: Option<Message>,
    showing_for: u32,
    /// The intro sequence.
    intro: Intro,
    /// The intro's own cast, out of the pack. Absent when the pack was baked
    /// without an unpacked `INTR.EXE`, in which case the plates simply hold.
    intro_cast: Option<std::rc::Rc<henge_core::content::IntroCast>>,
    /// Where a save is written and read. Relative to wherever the game is run
    /// from unless `--save` says otherwise, and never written into the save
    /// itself.
    save_path: String,
    status: String,
    #[cfg(feature = "research")]
    research: Option<research::Viewer>,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum Mode { Intro, Title, Select, Map, Combat, Place }

/// A flight over the map, `EffectFLAG+2` and `+4` in `_MAP`: the gem's comes
/// back to where it began, the hawk's lands where it is when fire is pressed.
#[derive(Clone, Copy, Debug)]
struct Flight {
    returns: bool,
    from: (i32, i32),
}

/// One line of the character sheet's menu: the original's `Increase`
/// gadgets (`HGAbility`) and a cast for each thing carried (`MagicCast`).
#[derive(Clone, Debug)]
enum SheetAction {
    Raise(Ability),
    Use(String),
}

/// Load every sound the packs offer, and open a device if there is one.
///
/// No device is a normal state, not a failure: containers, CI and plenty of
/// desktops have none. The game plays silently rather than refusing to start.
fn open_audio(reg: &mut henge_assets::Registry) -> Box<dyn Sink> {
    let ids: Vec<String> = reg
        .all_ids()
        .into_iter()
        .filter(|id| id.starts_with("sfx."))
        .map(str::to_string)
        .collect();

    let mut clips = Clips::default();
    for id in &ids {
        let Some(r) = reg.sound(id) else { continue };
        if let Ok(bytes) = std::fs::read(r.root.join(r.value)) {
            clips.insert(id.clone(), bytes);
        }
    }
    let loaded = clips.len();

    // The tunes, as the recovered note streams rather than as audio. They are
    // rendered when a room asks for one, because a run may never open a door
    // that has music behind it.
    let tunes: Vec<(String, henge_audio::Score)> = reg
        .all_ids()
        .into_iter()
        .filter(|id| id.starts_with("music."))
        .map(str::to_string)
        .collect::<Vec<_>>()
        .into_iter()
        .filter_map(|id| {
            let r = reg.music(&id)?;
            let text = std::fs::read_to_string(r.root.join(r.value)).ok()?;
            Some((id, henge_audio::Score::parse(&text)?))
        })
        .collect();

    #[cfg(feature = "audio")]
    match henge_audio::Native::new(clips) {
        Ok(mut n) => {
            let songs = tunes.len();
            for (id, score) in tunes {
                n.add_score(id, score);
            }
            println!("audio: {loaded} clips, {songs} tunes");
            return Box::new(n);
        }
        Err(e) => eprintln!(
            "audio: {loaded} clips and {} tunes loaded but no output device ({e}), playing silently",
            tunes.len()
        ),
    }
    #[cfg(not(feature = "audio"))]
    println!("audio: built without a backend; {loaded} clips and {} tunes go unheard", tunes.len());
    Box::new(henge_audio::Silent::default())
}

/// The ten slots the two seats own. Everything the game reads about walking and
/// swinging goes through one of these, which is what makes them the ten things
/// worth rebinding.
///
/// Slots 0 to 3, 6 to 11 are the seats; the rest are the developer keys, which
/// [`key_index`] still owns because they are not controls.
fn slot_of(seat: usize, a: input::Action) -> usize {
    use input::Action::*;
    match (seat, a) {
        (0, Up) => 0,
        (0, Down) => 1,
        (0, Left) => 2,
        (0, Right) => 3,
        (0, Fire) => 6,
        (1, Up) => 7,
        (1, Down) => 8,
        (1, Left) => 9,
        (1, Right) => 10,
        (1, Fire) => 11,
        _ => 255,
    }
}

/// The keys that are not controls, so are not in the bindings table.
///
/// The ten seat slots are gone from here: they come out of `input::Bindings`
/// now, so that changing them is editing a file rather than editing this match.
fn key_index(c: KeyCode) -> usize {
    match c {
        KeyCode::BracketLeft => 4,
        KeyCode::BracketRight => 5,
        // Enter takes a menu option. The original had only fire, but everyone
        // arriving at a menu presses Enter first, and finding that it does
        // nothing reads as a broken menu rather than as a different key.
        // Kept separate from fire so that Enter does not also swing a sword.
        KeyCode::Enter | KeyCode::NumpadEnter => 12,
        // The arena browser's creature cycle, beside the arena cycle on the
        // brackets.
        KeyCode::Comma => 13,
        KeyCode::Period => 14,
        _ => 255,
    }
}

/// Packs live next to the executable in a release build, or in the working
/// directory during development.
fn find_packs() -> Option<std::path::PathBuf> {
    let mut roots = vec![std::path::PathBuf::from("packs")];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.join("packs"));
            roots.push(dir.join("../../../packs"));
        }
    }
    roots.into_iter().find(|p| p.is_dir())
}

impl App {
    fn new() -> anyhow::Result<App> {
        let mut fb = Framebuffer::default();
        let mut pal = [0u32; 32];
        for (i, slot) in pal.iter_mut().enumerate() {
            let v = (i as u32 * 255 / 31) & 0xff;
            *slot = (v / 3) << 16 | (v / 2) << 8 | v;
        }
        fb.set_palette(&pal);

        let mut reg = Registry::new();
        let mut status = String::from(
            "no packs found. Bake one from your own copy of the game first:\n  cargo run --release -p henge-formats --bin henge-bake -- \"path/to/Moonstone\" packs/reference",
        );
        if let Some(dir) = find_packs() {
            for name in ["original", "reference"] {
                let p = dir.join(name);
                if p.join("manifest.json").exists() {
                    reg.push_pack(&p)?;
                }
            }
            let c = reg.coverage();
            status = format!("{} assets, {:.1}% ours", c.total, c.percent());
        }
        let map = match MapScene::load(&mut reg) {
            Ok(m) => Some(m),
            Err(e) => { eprintln!("no overworld: {e:#}"); None }
        };
        let world = match World::load(&reg) {
            Ok(w) => Some(w),
            Err(e) => { eprintln!("arena load failed: {e:#}"); None }
        };
        let audio = open_audio(&mut reg);
        let fonts = text::load(&reg);
        let places = place::load(&reg);
        let items = place::load_items(&reg);
        // A pack without knights is not an error; the title simply cannot offer
        // a quest, exactly as it could not before.
        let knights: Knights = reg.read_data("data.knights").unwrap_or_default();
        // Controls are ours and they are data: a file beside the save, read if
        // it is there and the documented defaults if it is not.
        let controls_path = controls_path_arg(&args_of());
        let args = args_of();
        let mut bindings = input::Bindings::load(&controls_path).unwrap_or_else(|| {
            if args.iter().any(|a| a == "--original-keys") {
                input::Bindings::as_the_original_had_them()
            } else {
                input::Bindings::default()
            }
        });
        // `--bind 0:fire=Space`. There is no settings screen yet, so a
        // rebinding is made here or in the file, and either way it is the
        // same table and the same names.
        let mut rebound = false;
        for (i, a) in args.iter().enumerate() {
            if a != "--bind" {
                continue;
            }
            match args.get(i + 1).and_then(|s| input::Bindings::parse_bind(s)) {
                Some((seat, action, src)) => {
                    bindings.bind(seat, action, src);
                    rebound = true;
                }
                None => eprintln!("--bind: cannot read {:?}", args.get(i + 1)),
            }
        }
        if rebound || args.iter().any(|a| a == "--controls-write") {
            match bindings.save(&controls_path) {
                Ok(()) => println!("controls written to {controls_path}"),
                Err(e) => eprintln!("controls not written: {e}"),
            }
        }
        // Which entries cycle and which glow on each screen: recovered
        // constants, and data rather than code, so a replacement pack can
        // animate its own palettes.
        let fx_table: henge_assets::EffectTable =
            reg.read_data("data.palette.effects").unwrap_or_default();
        // Which tune plays in which room. Recovered from `LOADMUSIC`'s five
        // callers, and data rather than code like everything else.
        let music_places: std::collections::BTreeMap<String, String> =
            reg.read_data("data.music.places").unwrap_or_default();
        if world.is_none() {
            eprintln!("no arena data: {status}");
        } else {
            println!("{status}");
            println!("menus: arrows move, enter or space takes. p1 arrows + space,");
            println!("p2 wasd + f, 1/2 set how many are playing,");
            println!("tab switches map/arena, [ and ] change arena, , and . change the opponent,");
            println!("R restarts, escape quits");
        }

        let intro_cast = reg.read_data("data.intro").ok().map(std::rc::Rc::new);
        Ok(App {
            fb,
            fx: henge_assets::Effects::new(),
            fx_table,
            scene: String::new(),
            music_places,
            tick: 0,
            keys: [false; 256],
            pressed: [false; 256],
            reg, world,
            mode: if map.is_some() { Mode::Map } else { Mode::Combat },
            map,
            places,
            items,
            knights,
            title: shell::TitleScene::default(),
            select: None,
            sheet: false,
            approach: Approach::default(),
            visiting: None,
            audio,
            voices: Voices::new(),
            fonts,
            peaceful: false,
            bindings,
            controls_path,
            kb: [false; 256],
            pad_held: [false; 256],
            bounce: [input::Debounce::default(); 2],
            calibrating: None,
            run: Run::new(100),
            robbed: String::new(),
            robbed_for: 0,
            run_over_for: 0,
            flight: None,
            sheet_cursor: 0,
            raiding: None,
            questing: false,
            raid_place: String::new(),
            interlude: 0,
            hint: 0,
            practice: false,
            pointer: Pointer::centred(),
            gadgets: Gadgets::default(),
            messages: Messages::recovered(),
            showing: None,
            showing_for: 0,
            intro: Intro::new(),
            intro_cast,
            save_path: save_path_arg(&args_of()),
            status,
            #[cfg(feature = "research")]
            research: research::Viewer::from_args()?,
        })
    }

    /// Space or Enter takes the highlighted option. Both, everywhere a menu
    /// asks, so there is never a screen where one of them silently does
    /// nothing.
    fn takes(&self) -> bool {
        self.pressed[6] || self.pressed[12]
    }

    fn key(&mut self, code: KeyCode, down: bool) {
        // Saving and loading are ours, and so are the keys: the original has
        // neither. They work on any screen, because refusing is how a screen
        // that cannot be saved from says so.
        if down {
            match code {
                KeyCode::F5 => {
                    self.save_game();
                }
                KeyCode::F9 => {
                    self.load_game();
                }
                _ => {}
            }
        }
        // The seats' ten slots come out of the binding table; everything else
        // is a developer key and stays where it is.
        let named = input::Source::key(&format!("{code:?}"));
        for (seat, action) in self.bindings.raised_by(&named) {
            let s = slot_of(seat, action);
            if s < 256 {
                if down && !self.keys[s] {
                    self.pressed[s] = true;
                }
                self.kb[s] = down;
                self.keys[s] = self.kb[s] || self.pad_held[s];
            }
        }
        let i = key_index(code);
        if i < 256 {
            if down && !self.keys[i] {
                self.pressed[i] = true;
            }
        }
        {
            if down && self.world.is_some() {
                match code {
                    KeyCode::BracketLeft => self.world.as_mut().unwrap().step_arena(-1),
                    KeyCode::BracketRight => self.world.as_mut().unwrap().step_arena(1),
                    // Comma and period cycle which creature fills the
                    // opponents' seats, so each of the bestiary can be looked
                    // at in the arena browser.
                    KeyCode::Comma => self.world.as_mut().unwrap().step_foe(-1),
                    KeyCode::Period => self.world.as_mut().unwrap().step_foe(1),
                    KeyCode::KeyR => self.world.as_mut().unwrap().reset(),
                    // 1 and 2 set how many people are at the keyboard; the rest
                    // of the four seats are filled by opponents.
                    KeyCode::Digit1 => self.world.as_mut().unwrap().set_players(1),
                    KeyCode::Digit2 => self.world.as_mut().unwrap().set_players(2),
                    // Tab flips between the overworld and the arena, which is
                    // how the arena browser stays reachable.
                    KeyCode::Tab => {
                        self.mode = match self.mode {
                            Mode::Map => Mode::Combat,
                            Mode::Combat => Mode::Map,
                            // Tab is also the way out of a place, so a broken
                            // menu can never trap you indoors.
                            Mode::Place => { self.visiting = None; Mode::Map }
                            // And the way out of the shell, so a pack with no
                            // knights in it cannot strand you on a select screen
                            // with nothing to select.
                            Mode::Select => { self.select = None; Mode::Title }
                            // Tab out of the intro too, so the sequence can
                            // never hold a player who wants to play.
                            Mode::Intro => { self.intro.skip(); Mode::Title }
                            Mode::Title => Mode::Map,
                        };
                    }
                    // The character sheet, over whatever is on screen.
                    KeyCode::KeyC => self.sheet = !self.sheet,
                    _ => {}
                }
            }
            if i < 256 {
                self.keys[i] = down;
            }
        }
        #[cfg(feature = "research")]
        if let Some(v) = self.research.as_mut() {
            v.key(code, down);
        }
    }

    /// The pads, once a frame, ORed into the same slots the keys feed.
    ///
    /// This is `GetInputDevice`'s shape: read the stick, read the keys, OR them
    /// and throw away opposite directions. It runs only in the window, because
    /// a headless run drives the slots itself.
    fn pads_tick(&mut self, pads: &input::Pads) {
        for seat in 0..self.bindings.seats.len().min(2) {
            let cal = self.bindings.calibration(seat);
            let word = pads.word(&self.bindings.seats[seat], &cal);
            for a in input::Action::ALL {
                let s = slot_of(seat, a);
                if s >= 256 {
                    continue;
                }
                let on = word & a.bit() != 0;
                if on && !self.keys[s] {
                    self.pressed[s] = true;
                }
                self.pad_held[s] = on;
                self.keys[s] = self.kb[s] || on;
            }
            // `BOUNCEBUTTON`, kept ticking whether or not anything is asking,
            // so a press held from before a calibration started is not counted.
            let edge = self.bounce[seat].edge(word);
            if let Some((who, state)) = self.calibrating {
                if who == seat {
                    let raw = pads.raw(self.bindings.seats[seat].pad).unwrap_or((0, 0));
                    match state.step(raw, edge) {
                        Ok(next) => {
                            if next != state {
                                self.calibrating = Some((seat, next));
                                self.notice(next.prompt().join(" "));
                            }
                        }
                        Err(cal) => {
                            self.bindings.set_calibration(seat, cal);
                            self.calibrating = None;
                            let note = match self.bindings.save(&self.controls_path) {
                                Ok(()) => format!("stick {seat} calibrated"),
                                Err(e) => format!("calibrated, but not saved: {e}"),
                            };
                            self.notice(note);
                        }
                    }
                }
            }
        }
    }

    /// Starts `Fix_JoyStick` for one seat, and says what it wants.
    fn calibrate(&mut self, seat: usize) {
        self.bounce[seat.min(1)].clear();
        let state = input::Calibrating::TopLeft;
        self.calibrating = Some((seat, state));
        self.notice(state.prompt().join(" "));
    }

    /// One tick. A key press is an edge: it lasts exactly this tick and is
    /// spent whether or not anything wanted it.
    fn update(&mut self) {
        self.simulate();
        self.palette_tick();
        self.music_tick();
        self.pressed = [false; 256];
        self.robbed_for = self.robbed_for.saturating_sub(1);
    }

    /// The name of the screen that is up. Compared frame to frame, so that
    /// arriving somewhere new installs that screen's palette effects and fades
    /// it in, which is what every one of the original's own loaders does:
    /// `FADEPALETTEOUT`, load, `FADEPALETTEIN`.
    fn scene_key(&self) -> String {
        if self.interlude > 0 {
            return "interlude".into();
        }
        if self.showing.is_some() {
            return "message".into();
        }
        match self.mode {
            // Each step of the intro is its own screen, because the original
            // fades between them: every scene routine calls the fade out, puts
            // its plate up and fades back in.
            Mode::Intro => format!("intro.{}", self.intro.card),
            Mode::Title => "title".into(),
            Mode::Select => "select".into(),
            Mode::Map => "map".into(),
            Mode::Place => match self.visiting.as_ref() {
                Some(s) => format!("place.{}", s.visit.place),
                None => "place".into(),
            },
            Mode::Combat => match self.world.as_ref() {
                Some(w) => format!("arena.{}", w.family()),
                None => "arena".into(),
            },
        }
    }

    /// One frame of `COLCON`, plus a fade whenever the screen changes.
    fn palette_tick(&mut self) {
        let key = self.scene_key();
        if key != self.scene {
            self.scene = key;
            // The base palette the effects work from is whatever the screen
            // last drew with, which is what `PALLOC` points at.
            let base = self.fb.palette;
            let fx = self.fx_table.get(&self.scene).cloned().unwrap_or_default();
            self.fx.install(&fx, &base);
            // `InitCombat` calls `MudmenGlowOn` when mudmen are in the bout,
            // and nothing else does, so it hangs off the fighters rather than
            // off the screen.
            if self.mudmen_present() {
                self.fx.install_glow(MUDMEN_GLOW, &base);
            }
            self.fx.set_fade(henge_assets::Fade::In(0));
        }
        self.fx.tick();
        // `FADEOUTDAY`, and the fade a message chain ends on. Both are screens
        // that go out on their own rather than being walked away from, which
        // is the only kind of fade out a shell with no loading time can honour:
        // the original's other fades cover a disk read that does not happen
        // here. `NextWHICH` fades the between days screen and all three of
        // `WAITMESSAGE`, `OCCURMESSAGE` and `INSTRUCTMESSAGE` fade the chain.
        let leaving = if self.interlude > 0 {
            Some(self.interlude)
        } else if self.showing.is_some() {
            Some(self.showing_for)
        } else {
            None
        };
        if let Some(left) = leaving {
            let steps = henge_assets::palette::FADE_STEPS;
            if left <= steps as u32 {
                self.fx.set_fade(henge_assets::Fade::Out(steps - left as u16));
            }
        }
    }

    /// The tune for the room you are standing in, and silence everywhere else.
    ///
    /// That is exactly what the original does: five of its rooms call
    /// `LOADMUSIC` and then `int 60h` with `ah = 0` on the way in, and every
    /// one of them calls it with `ah = 2` on the way out. Nothing else in the
    /// game has music, the map and the arenas included.
    fn music_tick(&mut self) {
        let want = if self.mode == Mode::Intro {
            // Tune 1, by elimination rather than by trace. MAIN.EXE loads tunes
            // 2 to 5 and never 1 or 6; disk A holds the intro and exactly two
            // tunes, 1 and 6; and the intro comes before the ending. Which of
            // the two INTR.EXE's own start call selects was not read out of the
            // code, so this is inference, not recovery, and is the one place
            // music plays that the table did not decide.
            self.music_places.get("intro").cloned()
        } else if self.mode == Mode::Place {
            self.visiting
                .as_ref()
                .and_then(|s| self.music_places.get(&s.visit.place))
                .cloned()
        } else {
            None
        };
        match want {
            Some(id) => self.audio.play_music(&id),
            None => self.audio.stop_music(),
        }
    }

    fn mudmen_present(&self) -> bool {
        self.world
            .as_ref()
            .is_some_and(|w| w.bout.fighters.iter().any(|f| f.actor == "mudmen"))
    }

    /// The palette as it should reach the screen. One source for the window
    /// and for a capture, so a screenshot shows the fade the player sees.
    fn palette_now(&self) -> [u32; 32] {
        self.fx.apply(&self.fb.palette)
    }

    /// Runs any fade in progress out to its sixteenth frame. A capture is a
    /// still, and a still of the second frame of a fade is a black picture, so
    /// every recipe written before fades existed still shows its screen.
    fn settle_fade(&mut self) {
        for _ in 0..henge_assets::palette::FADE_STEPS {
            if self.fx.fade().finished() {
                break;
            }
            self.fx.tick();
        }
    }

    fn simulate(&mut self) {
        self.tick += 1;
        #[cfg(feature = "research")]
        if let Some(v) = self.research.as_mut() {
            v.update();
            return;
        }
        let dx = self.keys[3] as i32 - self.keys[2] as i32;
        let dy = self.keys[1] as i32 - self.keys[0] as i32;

        // A message box is modal, the way all three of the original's are:
        // `MESSAGE.PIV` goes up, the chain is drawn over it, and nothing else
        // runs until it fades. Fire clears it, as `WaitFIRE` does.
        if self.showing.is_some() {
            self.showing_for = self.showing_for.saturating_sub(1);
            if self.showing_for == 0 || self.pressed.iter().any(|p| *p) {
                self.showing = None;
            }
            return;
        }

        // The between-days screen is modal, the way `NextWHICH` puts it up
        // before anything else runs. It clears on a press, or on its own after
        // a few seconds, so a run left alone still goes on.
        if self.interlude > 0 {
            self.interlude -= 1;
            if self.pressed.iter().any(|p| *p) {
                self.interlude = 0;
            }
            return;
        }

        // The pointer, and the boxes on this screen. Laid out before the
        // screen ticks, so what the pointer is over is the highlight that
        // screen then acts on.
        self.gadgets_tick(
            self.keys[10] as i32 - self.keys[9] as i32,
            self.keys[8] as i32 - self.keys[7] as i32,
        );

        match self.mode {
            Mode::Intro => self.intro_tick(),
            Mode::Title => self.title_tick(),
            Mode::Select => self.select_tick(),
            Mode::Map => {
                // A run that is over, won or lost. The tally holds until it is
                // taken, and then the game goes back to where the original's
                // own two endings go: `jmp StartAgain` for a loss, and for a
                // win an exit to DOS, which here is the same screen because
                // there is nowhere else to exit to.
                if self.run.ending().is_some() {
                    self.run_over_for += 1;
                    let taken = self.run_over_for > 30 && self.takes();
                    if self.run_over_for > 600 || taken {
                        self.run_over_for = 0;
                        self.run.restart();
                        // A new run is a new board: the lairs a dead knight
                        // emptied are full and back on the map, keys and all.
                        self.stock_lairs();
                        if let Some(w) = self.world.as_mut() {
                            w.set_player_health(self.run.health_for_fight());
                            w.set_moon(self.run.moon.phase().key());
                        }
                        self.title.touched();
                        self.mode = Mode::Title;
                    }
                    return;
                }
                // The sheet is the original's status screen: modal, and
                // where the casting and the levelling are done.
                if self.sheet {
                    self.sheet_tick();
                    return;
                }
                // A toad has no turn. `_MAP:NextWHICH` tests `[si+0x3a]` and
                // goes straight round to the next knight when it is set, so
                // the wizard's curse costs its three days rather than being a
                // counter nothing reads: the day turns over and no step is
                // taken.
                if self.run.is_toad() {
                    if let Some(m) = self.map.as_mut() {
                        m.state.pass_days(1);
                    }
                    self.run.new_day();
                    self.begin_interlude();
                    return;
                }
                // Aloft, the map is crossed without steps, ambushes or slow
                // ground: `MapMovement` skips its step count and `CheckSLOW`
                // its grid while either flag is up.
                if self.flight.is_some() {
                    self.fly(dx, dy);
                    return;
                }
                let mut start: Option<String> = None;
                let mut day_before = 0;
                let mut arrived: Option<String> = None;
                if let Some(m) = self.map.as_mut() {
                    day_before = m.state.day;
                    // `DistanceDONE`: the day is as long as the stride says,
                    // sixteen steps to the point, doubled by haste.
                    if self.run.knight.named() {
                        m.state.steps_per_day = self.run.day_steps(&self.items);
                    }
                    let step = m.update(dx, dy);
                    if step.encounter && !self.peaceful {
                        start = Some(m.last_terrain.family().to_string());
                    }
                    arrived = self.approach.step(&self.places, m.state.x, m.state.y);
                }
                // Walking is how you mend, and also how you meet trouble. The
                // same action both repairs and risks you.
                if dx != 0 || dy != 0 {
                    self.run.travelled();
                    // And trouble is not only the kind you can swing at. A
                    // cutpurse on the road is the world's half of
                    // `TAKEFROMKNIGHT`: what you carry can leave you.
                    if !self.peaceful {
                        match self.run.waylaid() {
                            Some(Loss::Gold(n)) => {
                                self.robbed = format!("{n} gold taken");
                                self.robbed_for = 180;
                            }
                            Some(Loss::Item(id)) => {
                                let what = self
                                    .items
                                    .get(&id)
                                    .map_or(id.clone(), |d| d.name.clone());
                                self.robbed = format!("{what} taken");
                                self.robbed_for = 180;
                                // A ring taken is twenty health gone with it.
                                self.run.refresh(&self.items);
                            }
                            None => {}
                        }
                    }
                }
                if let Some(m) = self.map.as_ref() {
                    if m.state.day != day_before {
                        self.run.new_day();
                        self.begin_interlude();
                    }
                }
                // A town is somewhere you arrive at, not somewhere you get
                // jumped outside of: walking through the gate beats the ambush
                // roll taken on the same step.
                if let Some(id) = arrived {
                    if self.enter(&id) {
                        return;
                    }
                }
                // A scroll of protection hanging over the run answers the
                // ambush first, the way `KnightProtection` is asked before
                // `InitKnightBattle`.
                let start = match start {
                    Some(family) => match self.run.challenged() {
                        Challenge::Averted => {
                            self.notice("The scroll turns it away");
                            None
                        }
                        Challenge::Backfired => {
                            self.notice("The scroll turns on you");
                            Some(family)
                        }
                        Challenge::Fight => Some(family),
                    },
                    None => None,
                };
                if start.is_some() {
                    // The sheet as it stands now: constitution bought since
                    // the last fight, a sword picked up, a curse to carry in.
                    self.sync_sheet();
                }
                if let (Some(family), Some(w)) = (start, self.world.as_mut()) {
                    // Which arena of that family comes next is the family's own
                    // turn counter, carried on the run: the original rotates
                    // through its eight in order rather than rolling for one.
                    let pick = self.run.next_arena(&family, w.rotation_len(&family));
                    w.set_player_health(self.run.health_for_fight());
                    w.set_player_daggers(self.run.knight.daggers);
                    // Who waits on this ground is the family's own list,
                    // brought round by the same counter as its arenas.
                    let foe = w.foe_for(&family, pick);
                    w.set_foe(&foe);
                    w.set_family(&family, pick);
                    self.voices.reset();
                    self.mode = Mode::Combat;
                }
            }
            Mode::Place => {
                // A run that ended while you were indoors, which is what a
                // lair's guardian or the Valley's does: back out onto the map,
                // where the tally is. Nothing in a place is any use to a
                // knight who has no life points left.
                if self.run.ending().is_some() {
                    self.visiting = None;
                    self.mode = Mode::Map;
                    return;
                }
                let (up, down, take) = (self.pressed[0], self.pressed[1], self.takes());
                let mut leave = false;
                let mut days = 0;
                let mut door: Option<String> = None;
                let mut raid: Option<(usize, String, String, String, u32)> = None;
                let mut quest: Option<(String, String, String, u32)> = None;
                if let Some(s) = self.visiting.as_mut() {
                    if let Some(def) = self.places.get(&s.visit.place) {
                        if up {
                            s.visit.move_by(def, -1);
                        }
                        if down {
                            s.visit.move_by(def, 1);
                        }
                        if take {
                            match s.visit.choose(def, &self.items, &mut self.run) {
                                Answer::Left => leave = true,
                                Answer::Stayed { days: d } => days = d,
                                Answer::Went { place } => door = Some(place),
                                Answer::Fight { lair, arena, family, guardian, count } => {
                                    raid = Some((lair, arena, family, guardian, count));
                                }
                                Answer::Guardian { arena, family, guardian, count } => {
                                    quest = Some((arena, family, guardian, count));
                                }
                            }
                        }
                    } else {
                        leave = true;
                    }
                }
                // Taking something off a lair's floor can be the thing that
                // empties it, and an empty beaten lair is off the map.
                if take {
                    self.refresh_lairs();
                }
                // A guardian is waiting on the other side of the door. The bout
                // is set up here rather than in `place.rs`, which knows nothing
                // about arenas, and the lair is remembered so that winning
                // comes back to its own page instead of to the map.
                // The Valley gate stood open. Same bout, same road back, and
                // the lair number is what tells them apart.
                if let Some((arena, family, guardian, count)) = quest {
                    self.questing = true;
                    raid = Some((usize::MAX, arena, family, guardian, count));
                }
                if let Some((lair, arena, family, guardian, count)) = raid {
                    let here = self.visiting.as_ref().map(|s| s.visit.place.clone());
                    if let Some(w) = self.world.as_mut() {
                        w.set_player_health(self.run.health_for_fight());
                        w.set_player_daggers(self.run.knight.daggers);
                        w.set_foe(&guardian);
                        // Name the layout if the pack has it; fall back to the
                        // family so a raid is never fought on no ground at all.
                        if !w.set_arena(&arena) {
                            let pick = self.run.next_arena(&family, w.rotation_len(&family));
                            w.set_family(&family, pick);
                        }
                        w.set_seats(self.title.state.players.max(1), count.max(1) as usize);
                        self.raiding = (lair != usize::MAX).then_some(lair);
                        self.raid_place = here.unwrap_or_default();
                        self.visiting = None;
                        self.voices.reset();
                        self.mode = Mode::Combat;
                        return;
                    }
                }
                // A door inside a place opens another place rather than putting
                // you back on the map: the merchant's stall is a room in the
                // town, not a walk away from it.
                if let Some(id) = door {
                    // What the room you came from was saying goes with you,
                    // which is how the tavern's throw reaches the dice table.
                    // Where it was saying nothing, the new room's own greeting
                    // stands instead.
                    let carried = self
                        .visiting
                        .as_ref()
                        .map(|s| s.visit.through(&id))
                        .filter(|v| !v.said.is_empty() || v.dice.is_some());
                    if !self.enter(&id) {
                        leave = true;
                    } else if let (Some(s), Some(v)) = (self.visiting.as_mut(), carried) {
                        s.visit = v;
                    }
                }
                // Time spent indoors has to move the map's calendar too, or the
                // day on the status bar would disagree with the day of the run.
                if days > 0 {
                    if let Some(m) = self.map.as_mut() {
                        m.state.pass_days(days);
                    }
                }
                if leave {
                    self.visiting = None;
                    self.mode = Mode::Map;
                }
            }
            Mode::Combat => {
                if let Some(w) = self.world.as_mut() {
                    let seats = [
                        Intent { dx, dy, attack: self.keys[6] },
                        Intent {
                            dx: self.keys[10] as i32 - self.keys[9] as i32,
                            dy: self.keys[8] as i32 - self.keys[7] as i32,
                            attack: self.keys[11],
                        },
                    ];
                    w.update(&seats);
                    for (_, cue) in self.voices.observe(&w.bout.fighters, &w.events) {
                        self.audio.play(cue.sound());
                    }
                    // A finished bout hands control back to the map, or restarts
                    // in place when there is no map to go back to.
                    // Record the outcome once, on the tick the fight settles.
                    let practice = self.practice;
                    if w.settled_for() == 1 && !practice {
                        let survivor = w.bout.fighters.first();
                        let health = survivor.map_or(0, |f| if f.alive() { f.health } else { 0 });
                        let won = w.bout.winner() == Some(0);
                        // What the fallen were carrying, and what they were
                        // worth. The run decides whether it is collected; a
                        // corpse collects nothing.
                        self.run.finished_fight_worth(health, won, w.purse(), w.experience());
                        w.set_player_cursed(false);
                        // And what was thrown is gone: the sheet's daggers are
                        // whatever is left on the belt.
                        self.run.knight.daggers = w.daggers_left(0);
                    }
                    // A finisher on a fallen knight is allowed to play out, as
                    // the original's `StopCombat` is at the end of that script
                    // and not at the moment of death.
                    if w.settled_for() > 120 && !w.finishing() {
                        self.voices.reset();
                        if practice {
                            // Nothing to carry back. Practice is over when the
                            // bout is, and the title is where it came from.
                            w.reset();
                            self.practice = false;
                            self.mode = Mode::Title;
                        } else {
                            if self.run.alive() {
                                w.set_player_health(self.run.health_for_fight());
                            }
                            let won = w.bout.winner() == Some(0);
                            w.reset();
                            // The Valley's Guardian, which is neither a lair
                            // nor the road: winning spends the four keys and
                            // pays a moonstone, losing costs two more life
                            // points on top of the one the death already took.
                            if self.questing {
                                self.questing = false;
                                self.raiding = None;
                                let place = self.raid_place.clone();
                                let beat = won && self.run.alive();
                                if self.enter(&place) {
                                    let mut visit =
                                        self.visiting.as_ref().map(|s| s.visit.clone());
                                    if let Some(v) = visit.as_mut() {
                                        if beat {
                                            v.won_valley(&self.items, &mut self.run);
                                        } else {
                                            v.lost_valley(&mut self.run);
                                        }
                                    }
                                    if let (Some(sc), Some(v)) = (self.visiting.as_mut(), visit) {
                                        sc.visit = v;
                                    }
                                } else {
                                    if beat {
                                        self.run.valley_won(&self.items);
                                    } else {
                                        self.run.valley_lost();
                                    }
                                    if self.map.is_some() {
                                        self.mode = Mode::Map;
                                    }
                                }
                                return;
                            }
                            // A raid won opens the floor. A raid lost leaves the
                            // lair as it was, with the guardian still in it.
                            match self.raiding.take() {
                                Some(lair) if won && self.run.alive() => {
                                    // Back to the lair's own page, and the
                                    // floor read there: `MOON:LairWon` marks
                                    // the lair, pays the one point of
                                    // experience the first win is worth, and
                                    // opens the page for the taking.
                                    let place = self.raid_place.clone();
                                    if self.enter(&place) {
                                        let mut visit =
                                            self.visiting.as_ref().map(|s| s.visit.clone());
                                        if let Some(v) = visit.as_mut() {
                                            v.won_lair(lair, &self.items, &mut self.run);
                                        }
                                        if let (Some(s), Some(v)) = (self.visiting.as_mut(), visit) {
                                            s.visit = v;
                                        }
                                    } else {
                                        self.run.lair_won(lair, &self.items);
                                        if self.map.is_some() {
                                            self.mode = Mode::Map;
                                        }
                                    }
                                    self.refresh_lairs();
                                }
                                _ => {
                                    if self.map.is_some() {
                                        self.mode = Mode::Map;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// The intro, one card at a time. Fire skips it, and when it runs out the
    /// game opens on its title the way it would have anyway.
    fn intro_tick(&mut self) {
        if self.takes() || self.pressed[11] {
            self.intro.skip();
        }
        self.intro.tick();
        if self.intro.done {
            self.mode = Mode::Title;
            self.title.touched();
        }
    }

    /// The title's option list, and the attract loop behind it.
    ///
    /// A press while it is showing off only wakes it. Letting the same press
    /// through would mean walking away from the keyboard and coming back to find
    /// the game had started itself.
    fn title_tick(&mut self) {
        let (up, down) = (self.pressed[0], self.pressed[1]);
        let (left, right, take) = (self.pressed[2], self.pressed[3], self.takes());
        let touched = up || down || left || right || take;
        let was_attracting = self.title.attracting();
        if touched {
            self.title.touched();
        } else {
            self.title.tick();
        }
        if was_attracting || !touched {
            return;
        }
        if up {
            self.title.state.move_by(-1);
        }
        if down {
            self.title.state.move_by(1);
        }
        if left {
            self.title.state.adjust(-1);
        }
        if right {
            self.title.state.adjust(1);
        }
        if take {
            match self.title.state.choose() {
                Some(Start::Quest) => self.begin_select(),
                Some(Start::Practice) => self.begin_practice(),
                None => {}
            }
        }
    }

    /// Choosing knights, one player at a time.
    fn select_tick(&mut self) {
        let (left, right, take) = (self.pressed[2], self.pressed[3], self.takes());
        let Some(select) = self.select.as_mut() else {
            self.mode = Mode::Title;
            return;
        };
        if left {
            select.state.move_by(-1);
        }
        if right {
            select.state.move_by(1);
        }
        if take {
            select.state.take();
        }
        if select.state.done() {
            self.begin_quest();
        }
    }

    fn begin_select(&mut self) {
        if self.knights.is_empty() {
            // Nothing to choose between, so there is nothing to show. Start the
            // quest as nobody rather than opening an empty screen.
            self.mode = Mode::Map;
            return;
        }
        let players = self.title.state.players.min(self.knights.len());
        self.select = Some(shell::SelectScene::new(&self.reg, players, &self.knights));
        self.mode = Mode::Select;
    }

    /// Practice: one bout, no map, and the first knight so the panel has a
    /// sheet to read. `StartPractice` in the original.
    fn begin_practice(&mut self) {
        self.practice = true;
        self.take_knight(0, Self::roster_led_by(0));
        // A duel when one person is at the keyboard; otherwise the people
        // who are, and nobody else.
        let humans = self.title.state.players.max(1);
        let gore = self.title.state.gore;
        if let Some(w) = self.world.as_mut() {
            // Practice is knight against knight, whatever the road last put
            // in the arena.
            w.set_gore(gore);
            w.set_foe("knight");
            w.set_seats(humans, if humans == 1 { 1 } else { 0 });
        }
        self.mode = Mode::Combat;
    }

    /// The quest proper. Seat zero is the person at this keyboard, so their
    /// knight is the one the run belongs to; the rest fill the other seats.
    fn begin_quest(&mut self) {
        let chosen = self.select.as_ref().map(|s| s.state.chosen()).unwrap_or_default();
        let mine = chosen.first().copied().unwrap_or(0);
        let mut roster = chosen.clone();
        for i in 0..henge_core::shell::SEATS {
            if !roster.contains(&i) {
                roster.push(i);
            }
        }
        self.practice = false;
        self.take_knight(mine, roster);
        self.select = None;
        self.mode = if self.map.is_some() { Mode::Map } else { Mode::Combat };
    }

    /// The four seats with a given knight in the first of them. Seat zero is the
    /// person at this keyboard, so whoever they chose has to sit in it.
    fn roster_led_by(knight: usize) -> Vec<usize> {
        let mut roster = vec![knight];
        for i in 0..henge_core::shell::SEATS {
            if i != knight {
                roster.push(i);
            }
        }
        roster
    }

    /// Begin a run as one of the four, and put the arena in their colours.
    fn take_knight(&mut self, knight: usize, roster: Vec<usize>) {
        let Some(def) = self.knights.get(knight) else { return };
        self.run = Run::for_knight(def, knight, &self.items);
        // The original starts each knight in their own corner. Ours is clamped
        // into the walkable part of the map, because two of the four corners it
        // names sit outside it.
        if let Some(m) = self.map.as_mut() {
            use henge_core::overworld::{MAX_X, MAX_Y};
            m.state.x = def.home[0].clamp(0, MAX_X);
            m.state.y = def.home[1].clamp(0, MAX_Y);
        }
        self.stock_lairs();
        let humans = self.title.state.players.max(1);
        let gore = self.title.state.gore;
        let daggers = self.run.knight.daggers;
        let sheet = self.sheet_of();
        self.flight = None;
        self.sheet_cursor = 0;
        let phase = self.run.moon.phase().key();
        if let Some(w) = self.world.as_mut() {
            // A quest opens on the full moon, which `InitGameStart` writes
            // before anything else runs, and the ratmen are already fielded
            // by it: the phase goes in with everything else the run decides.
            w.set_moon(phase);
            // One opponent on the road. Ambushes are creatures in the original,
            // and until the bestiary lands they are knights standing in; three
            // knights of equal strength on twenty health is not an ambush, it
            // is an execution.
            w.set_gore(gore);
            w.set_seats(humans, 1);
            w.set_roster(roster);
            w.set_sheet(sheet);
            w.set_player_daggers(daggers);
        }
    }

    /// The run's sheet as the arena needs it: what the knight can bear,
    /// what `CalcDamage` adds for him, what it adds for a fresh knight in
    /// any other seat, and whether his joystick is reversed.
    fn sheet_of(&self) -> Sheet {
        let fresh = self
            .knights
            .get(self.run.knight.seat)
            .map_or(1, |d| Knight::from_def(d, 0).damage_bonus(&self.items));
        Sheet {
            max_health: self.run.max_health,
            bonus: self.run.knight.damage_bonus(&self.items),
            fresh_bonus: fresh,
            cursed: self.run.is_cursed(),
        }
    }

    /// Push the sheet into the arena, after anything that changed it.
    fn sync_sheet(&mut self) {
        if !self.run.knight.named() {
            return;
        }
        let sheet = self.sheet_of();
        if let Some(w) = self.world.as_mut() {
            w.set_sheet(sheet);
        }
    }

    /// Write a save.
    ///
    /// **Ours entirely**: the original has no save. It is taken on the map and
    /// nowhere else, because a save is a serialization of the simulation
    /// between one step and the next; saving inside a bout would mean carrying
    /// every fighter's script pointer and the knives in the air for something
    /// nobody wants to resume mid-swing. `henge_core::save` says the rest.
    fn save_game(&mut self) -> bool {
        if self.mode != Mode::Map || !self.run.knight.named() {
            self.notice("only on the road");
            return false;
        }
        let Some(m) = self.map.as_ref() else {
            self.notice("no map to save");
            return false;
        };
        let save = Save::of(
            &self.run, &m.state, self.title.state.players, self.title.state.gore, self.hint,
        );
        let text = match serde_json::to_string_pretty(&save) {
            Ok(t) => t,
            Err(e) => {
                self.notice(format!("save failed: {e}"));
                return false;
            }
        };
        match std::fs::write(&self.save_path, text) {
            Ok(()) => {
                println!("saved to {}: {}", self.save_path, save.summary());
                self.notice("saved");
                true
            }
            Err(e) => {
                self.notice(format!("save failed: {e}"));
                false
            }
        }
    }

    /// Read a save back, or say clearly why not.
    ///
    /// Three distinguishable refusals, none of which loads half a game: not a
    /// save, a save this build cannot read, and a save that does not match its
    /// own fingerprint.
    fn load_game(&mut self) -> bool {
        let text = match std::fs::read_to_string(&self.save_path) {
            Ok(t) => t,
            Err(e) => {
                self.notice(format!("no save: {e}"));
                return false;
            }
        };
        let save: Save = match serde_json::from_str(&text) {
            Ok(s) => s,
            Err(_) => {
                self.notice(henge_core::save::SaveError::NotASave.message());
                eprintln!("{}: not a saved game", self.save_path);
                return false;
            }
        };
        if let Err(e) = save.check() {
            self.notice(e.message());
            eprintln!("{}: {e}", self.save_path);
            return false;
        }
        println!("loaded {}: {}", self.save_path, save.summary());
        self.apply(save);
        true
    }

    /// Put a checked save into the running game.
    fn apply(&mut self, save: Save) {
        self.run = save.run;
        self.title.state.players = save.players.clamp(1, henge_core::shell::SEATS);
        self.title.state.gore = save.gore;
        self.hint = save.wait_count % self.messages.wait_len();
        if let Some(m) = self.map.as_mut() {
            m.state = save.travel;
            m.last_terrain = m.terrain_here();
        }
        // Whatever screen the game was on is left behind: a loaded game stands
        // on the road, which is the only place a save is ever taken from.
        self.visiting = None;
        self.flight = None;
        self.raiding = None;
        self.sheet = false;
        self.interlude = 0;
        self.showing = None;
        self.run_over_for = 0;
        self.mode = Mode::Map;
        self.refresh_lairs();
        self.sync_sheet();
        if let Some(w) = self.world.as_mut() {
            w.set_player_health(self.run.health_for_fight());
            w.set_player_daggers(self.run.knight.daggers);
            w.set_moon(self.run.moon.phase().key());
            w.set_gore(self.title.state.gore);
        }
    }

    /// Put the pointer somewhere outright, rather than steering it.
    ///
    /// A window with a mouse in it should behave like one, and the pointer's
    /// hot spot is its top left corner, which is the point `CHECKGADGET` tests.
    fn point_at(&mut self, x: i32, y: i32) {
        self.pointer.x = x.clamp(0, henge_core::pointer::MAX_X);
        self.pointer.y = y.clamp(0, henge_core::pointer::MAX_Y);
        self.pointer.woken = true;
    }

    /// Put a message box up. Modal, as all three of the original's are.
    fn show_message(&mut self, msg: Message) {
        if msg.is_empty() {
            return;
        }
        self.showing = Some(msg);
        self.showing_for = Self::MESSAGE_TICKS;
    }

    /// Ticks a message box holds before it goes on its own. The original waits
    /// for fire; this does too, and gives up after a while so an unattended
    /// game is never stuck behind one.
    const MESSAGE_TICKS: u32 = 260;

    /// What arriving somewhere says, and which of the three routines says it.
    ///
    /// `WAITMESSAGE` at the wizard's tower, `OCCURMESSAGE` at either city and
    /// `INSTRUCTMESSAGE` at the stone circle, which is where the original puts
    /// each of them.
    fn announce(&mut self, place: &str) {
        if self.messages.waits_on_entering(place) {
            let msg = self.messages.wait(self.hint).clone();
            self.hint = (self.hint + 1) % self.messages.wait_len();
            self.show_message(msg);
            return;
        }
        if let Some(msg) = self.messages.on_entering(place).cloned() {
            self.show_message(msg);
        }
    }

    /// Lay out the boxes the pointer can be over on whatever screen is up, and
    /// let it drive them.
    ///
    /// `CLEARGADGETS` then a run of `ADDGADGET`s, exactly as the original's
    /// screens do it, and then `CHECKGADGET`: the row under the pointer becomes
    /// the highlighted row, and fire over it is the same press space would be.
    /// So the pointer never invents a way to do something; it only reaches the
    /// menus that were already there.
    fn gadgets_tick(&mut self, dx: i32, dy: i32) {
        self.gadgets.clear();
        let rows: Vec<(usize, i32, i32, i32, i32)> = if self.sheet && self.mode == Mode::Map {
            let n = self.sheet_rows().len();
            status::sheet_menu_rects(n, Some(self.sheet_cursor))
        } else {
            match self.mode {
                Mode::Title if !self.title.attracting() => shell::title_rects(),
                Mode::Select => shell::select_rects(),
                Mode::Place => self
                    .visiting
                    .as_ref()
                    .and_then(|s| self.places.get(&s.visit.place))
                    .map_or_else(Vec::new, place::menu_rects),
                _ => Vec::new(),
            }
        };
        if rows.is_empty() {
            return;
        }
        for (id, x, y, w, h) in rows {
            self.gadgets.add_box(id, x, y, w, h, "");
        }
        // The stick moves it two pixels a tick, which is `MovePointer`. Seat
        // two's keys, so seat one's still move the highlight and nothing that
        // worked before stops working.
        self.pointer.steer(dx, dy, self.keys[11]);
        // A pointer nobody has touched drives nothing. Without this the arrow
        // starts in the middle of the screen and silently moves the highlight
        // of every menu it happens to open over, which is a menu that chose
        // itself.
        if !self.pointer.woken {
            return;
        }
        let Some(over) = self.gadgets.hit_id(&self.pointer) else { return };
        // Being over a row is being on it, the way `GadgetHit` says the line
        // for whatever the pointer has reached.
        match self.mode {
            _ if self.sheet => self.sheet_cursor = over,
            Mode::Title => self.title.state.row = over,
            Mode::Select => {
                if let Some(sel) = self.select.as_mut() {
                    if sel.state.free(over) {
                        sel.state.cursor = over;
                    }
                }
            }
            Mode::Place => {
                if let Some(s) = self.visiting.as_mut() {
                    s.visit.cursor = over;
                }
            }
            _ => {}
        }
        // And fire over it takes it, through the same edge every menu reads.
        if self.pressed[11] {
            self.pressed[6] = true;
        }
    }

    /// A line on the map's corner plate, where the cutpurse's notice goes.
    fn notice(&mut self, line: impl Into<String>) {
        self.robbed = line.into();
        self.robbed_for = 180;
    }

    /// The character sheet's menu: the three `Increase` gadgets in the
    /// original's own order (`ab1`..`ab3`), lit only while the experience
    /// covers the cost and the ability is under five, as the status screen
    /// at `0xd3a7` lights them; then a line for everything carried.
    fn sheet_rows(&self) -> Vec<(String, bool, SheetAction)> {
        let mut rows = Vec::new();
        if !self.run.knight.named() {
            return rows;
        }
        let cost = self.run.level_cost();
        for a in [Ability::Strength, Ability::Endurance, Ability::Constitution] {
            let lit = self.run.can_level() && self.run.knight.ability(a) < MAX_ABILITY;
            rows.push((format!("{} ({cost} xp)", a.increase_line()), lit, SheetAction::Raise(a)));
        }
        for (id, n) in self.run.kit.iter() {
            // A key and a moonstone are carried, not cast: `MagicCast` has no
            // branch for either, and the panel's own line for one is `Take Key
            // to the Valley`, which is a trade and not a use. They are listed
            // so the pack is honest about what is in it, and unlit so nothing
            // offers to spend the quest.
            let token = henge_core::moon::is_token(id);
            let line = match self.items.get(id) {
                Some(d) if token => d.name.clone(),
                Some(d) => d.action_line(),
                None => format!("Use {id}"),
            };
            let line = if n > 1 { format!("{line} x{n}") } else { line };
            rows.push((line, !token, SheetAction::Use(id.to_string())));
        }
        rows
    }

    /// One tick of the sheet as a menu.
    fn sheet_tick(&mut self) {
        let rows = self.sheet_rows();
        if rows.is_empty() {
            return;
        }
        if self.pressed[0] {
            self.sheet_cursor = self.sheet_cursor.saturating_sub(1);
        }
        if self.pressed[1] {
            self.sheet_cursor += 1;
        }
        self.sheet_cursor = self.sheet_cursor.min(rows.len() - 1);
        if !self.takes() {
            return;
        }
        let (_, lit, action) = rows[self.sheet_cursor].clone();
        match action {
            SheetAction::Raise(a) => {
                if lit && self.run.spend_experience(a, &self.items) {
                    self.notice(format!("{} {}", a.name(), self.run.knight.ability(a)));
                    self.sync_sheet();
                }
            }
            SheetAction::Use(id) => {
                if !lit {
                    return;
                }
                let cast = self.run.cast(&id, &self.items);
                self.acted(cast);
            }
        }
    }

    /// What a cast did, made visible: a flight begun, a landing, a notice.
    fn acted(&mut self, cast: Cast) {
        match cast {
            Cast::Healed => self.notice("You are whole"),
            Cast::LifePoint => self.notice("A life point"),
            Cast::Hastened => self.notice("The day is doubled"),
            Cast::Warded => self.notice("A ward is up"),
            Cast::Worn => {
                self.sync_sheet();
                self.notice("Worn");
            }
            Cast::Aloft { returns } => {
                if let Some(m) = self.map.as_ref() {
                    self.flight = Some(Flight { returns, from: (m.state.x, m.state.y) });
                    self.sheet = false;
                    self.notice(if returns { "Aloft on the gem" } else { "Aloft on the hawk" });
                }
            }
            Cast::Astray { x, y } => {
                if let Some(m) = self.map.as_mut() {
                    use henge_core::overworld::{MAX_X, MAX_Y};
                    m.state.x = x.clamp(0, MAX_X);
                    m.state.y = y.clamp(0, MAX_Y);
                }
                self.sheet = false;
                self.notice("The hawk drops you");
            }
            Cast::Pointless => self.notice("Nothing comes of it"),
            Cast::HaveNone | Cast::Unknown => {}
        }
    }

    /// A tick aloft. The token moves where it is steered, a pixel a tick,
    /// inside `HawkBorders`; fire lands it: the gem's flight goes back to
    /// where it began, the hawk's stays put. In the original a gem flight
    /// ends only by looking into a lair, which restores the position on the
    /// way out (`LairGEM`); with no lair to look into, fire does it here.
    fn fly(&mut self, dx: i32, dy: i32) {
        use henge_core::overworld::{MAX_X, MAX_Y};
        let landing = self.takes();
        let Some(m) = self.map.as_mut() else {
            self.flight = None;
            return;
        };
        m.state.x = (m.state.x + dx).clamp(0, MAX_X);
        m.state.y = (m.state.y + dy).clamp(0, MAX_Y);
        if landing {
            if let Some(fl) = self.flight.take() {
                if fl.returns {
                    m.state.x = fl.from.0;
                    m.state.y = fl.from.1;
                }
            }
        }
    }

    /// The crystal or the hawk over the token while aloft: `_MAP:SHOW` draws
    /// the token's frame plus five with the gem flag up and plus ten with the
    /// hawk's, which in `MI.C` is the crystal row and the hawk row, one per
    /// knight's colour.
    fn draw_flight(&mut self) {
        let Some(fl) = self.flight else { return };
        let Some(m) = self.map.as_ref() else { return };
        let (x, y) = (m.state.x, m.state.y);
        let seat = self.run.knight.seat % 4;
        let frame = if fl.returns { 10 + seat } else { 15 + seat };
        let Some(rect) = self.reg.sheet("bank.mi").and_then(|r| r.value.frames.get(frame).copied()) else {
            return;
        };
        let Ok(img) = self.reg.image("bank.mi") else { return };
        let (w, h) = (rect.w as usize, rect.h as usize);
        let mut px = vec![0u8; w * h];
        for row in 0..h {
            let src = (rect.y as usize + row) * img.width + rect.x as usize;
            if src + w <= img.pixels.len() {
                px[row * w..(row + 1) * w].copy_from_slice(&img.pixels[src..src + w]);
            }
        }
        // Over the token, which the map has already drawn, and lifted a
        // little so it reads as above the ground rather than on it.
        self.fb.blit(&px, w, h, x, y - 4, false);
    }

    /// How long the between-days screen stays up on its own.
    ///
    /// The original waits on a key. Ours does too, but it also gives up after
    /// a few seconds: this game is walked with a direction held down, and a
    /// screen that needs a separate press to clear would stop the walk dead
    /// every day.
    const INTERLUDE_TICKS: u32 = 150;

    /// A day has turned over. Put the moon up, and take a hint off the pile.
    ///
    /// The moon's own numbers move here rather than in the drawing, because
    /// `_LOADER:WaitCOUNT` is a counter the original steps each time it shows
    /// one of the fourteen and wraps at fourteen, and a counter stepped by a
    /// renderer would step again on every frame.
    fn begin_interlude(&mut self) {
        self.interlude = Self::INTERLUDE_TICKS;
        self.hint = (self.hint + 1) % self.messages.wait_len();
        // What waits in an arena depends on the night the fight starts, so the
        // phase is pushed the moment it can change.
        if let Some(w) = self.world.as_mut() {
            w.set_moon(self.run.moon.phase().key());
        }
    }

    /// Lay the board, exactly where the original's lair initialiser runs: four
    /// keys planted one to a family, and then twenty four floors filled.
    ///
    /// Which lairs there are and what ground each stands on comes off the pack,
    /// so a pack with none simply stocks none and the map is what it was. This
    /// runs both when a knight is taken and when a dead run begins again,
    /// because `Run::restart` empties the table and a board left unstocked
    /// would be twenty four lairs with nothing but bones in them.
    fn stock_lairs(&mut self) {
        let families = henge_core::place::lair_families(&self.places);
        if !families.is_empty() {
            self.run.stock_lairs(&families, &self.items);
        }
        self.refresh_lairs();
    }

    /// Take off the map every lair that has been beaten and stripped.
    ///
    /// `MOON:CheckLairClear` writes 0xffff over a lair's coordinates when its
    /// gold is zero *and* all twenty four of its item counts are, and
    /// `DisplayLairs` then skips it; a lair you have beaten but could not carry
    /// out of is still there to go back to. Here that is the place's own
    /// `hidden` flag, which is already what keeps walking out of a room, so one
    /// idea covers both.
    fn refresh_lairs(&mut self) {
        for def in self.places.values_mut() {
            let lair = def.options.iter().find_map(|c| match &c.effect {
                henge_core::place::Effect::Raid { lair, .. } => Some(*lair),
                _ => None,
            });
            if let Some(n) = lair {
                def.hidden = !self.run.lair_on_the_map(n);
            }
        }
    }

    /// Every lair still on the map, as the icon and corner the map draws it at.
    fn map_icons(&self) -> Vec<(i32, i32, usize)> {
        self.places
            .values()
            .filter(|d| !d.hidden)
            .filter_map(|d| d.icon.map(|frame| (d.x, d.y, frame)))
            .collect()
    }

    /// Walk into a place and open its menu.
    fn enter(&mut self, id: &str) -> bool {
        let Some(def) = self.places.get(id) else {
            let known: Vec<&str> = self.places.keys().map(String::as_str).collect();
            eprintln!("no place called {id}. The pack has: {}", known.join(", "));
            return false;
        };
        match place::PlaceScene::open(&mut self.reg, def, id) {
            Ok(scene) => {
                self.visiting = Some(scene);
                self.mode = Mode::Place;
                // What arriving here says, if the original says anything.
                self.announce(id);
                true
            }
            Err(e) => {
                eprintln!("cannot enter {id}: {e:#}");
                false
            }
        }
    }

    /// One scripted key press, for the headless harness. Real input arrives the
    /// same way, through `pressed`, so this exercises the game and not a stub.
    fn drive(&mut self, c: Option<char>) {
        self.keys = [false; 256];
        match c {
            Some('u') => self.pressed[0] = true,
            Some('d') => self.pressed[1] = true,
            // Fire is held as well as pressed, so that in an arena it swings
            // and in a menu it takes.
            Some('s') => { self.keys[6] = true; self.pressed[6] = true; }
            // The numpad chords: a direction held with fire.
            Some(c @ '1'..='9') => {
                let n = c as u8 - b'0';
                self.keys[6] = true;
                self.keys[0] = matches!(n, 7 | 8 | 9);
                self.keys[1] = matches!(n, 1 | 2 | 3);
                self.keys[2] = matches!(n, 1 | 4 | 7);
                self.keys[3] = matches!(n, 3 | 6 | 9);
            }
            // 'e' is Enter, so the headless driver can prove Enter takes a menu
            // option and not only that space does.
            Some('e') => self.pressed[12] = true,
            // Seat two's fire, which is the pointer's button: `p` for point.
            Some('p') => { self.keys[11] = true; self.pressed[11] = true; }
            // Save and load, so both can be driven with no keyboard: the same
            // two calls F5 and F9 make.
            Some('S') => { self.save_game(); }
            Some('L') => { self.load_game(); }
            // Held and pressed both: walking reads the key, a menu reads the
            // edge, and the same letter has to drive either.
            Some('h') => { self.keys[2] = true; self.pressed[2] = true; }
            Some('l') => { self.keys[3] = true; self.pressed[3] = true; }
            Some('k') => { self.keys[0] = true; self.pressed[0] = true; }
            Some('j') => { self.keys[1] = true; self.pressed[1] = true; }
            _ => {}
        }
    }

    /// Steer towards a map position, one step a tick, the way a held key would.
    fn steer(&mut self, gx: i32, gy: i32) {
        let Some(m) = self.map.as_ref() else { return };
        let (x, y) = (m.state.x, m.state.y);
        self.keys[2] = x > gx;
        self.keys[3] = x < gx;
        self.keys[0] = y > gy;
        self.keys[1] = y < gy;
    }

    /// The end of a run, won or lost: the original's own heading and the tally
    /// under it. `ending::draw` is where the pixels go.
    fn draw_run_over(&mut self) {
        let Some(tally) = self.run.tally() else { return };
        let bold = self.fonts.remove("bold");
        let small = self.fonts.remove("small");
        ending::draw(&mut self.reg, &mut self.fb, bold.as_ref(), small.as_ref(), &tally);
        if let Some(f) = bold {
            self.fonts.insert("bold".into(), f);
        }
        if let Some(f) = small {
            self.fonts.insert("small".into(), f);
        }
    }

    /// Draw a line of text over the current frame, for checking the font.
    fn say(&mut self, line: &str) {
        let Some(font) = self.fonts.remove("bold") else {
            eprintln!("no font in the packs");
            return;
        };
        let luma = |c: u32| ((c >> 16) & 0xff) * 2 + ((c >> 8) & 0xff) * 3 + (c & 0xff);
        let (mut dark, mut light) = (0usize, 0usize);
        for i in 1..32 {
            if luma(self.fb.palette[i]) < luma(self.fb.palette[dark]) { dark = i; }
            if luma(self.fb.palette[i]) > luma(self.fb.palette[light]) { light = i; }
        }
        let mut y = 30;
        for part in line.split('|') {
            self.fb.rect(0, y - 4, 320, 26, dark as u8);
            font.draw_centred(&mut self.reg, &mut self.fb, part, y, light as u8);
            y += 30;
        }
        self.fonts.insert("bold".into(), font);
    }

    fn save_png(&self, path: &str) -> anyhow::Result<()> {
        let file = std::fs::File::create(path)?;
        let mut enc = png::Encoder::new(
            std::io::BufWriter::new(file), SCREEN_W as u32, SCREEN_H as u32,
        );
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut rgb = Vec::with_capacity(SCREEN_W * SCREEN_H * 3);
        let palette = self.palette_now();
        for p in &self.fb.pixels {
            let c = palette[(*p & 0x1f) as usize];
            rgb.extend_from_slice(&[(c >> 16) as u8, (c >> 8) as u8, c as u8]);
        }
        enc.write_header()?.write_image_data(&rgb)?;
        Ok(())
    }

    /// One fighter's plate, for each seat in the bout.
    ///
    /// Seat zero is the person at this keyboard, so it reads the live sheet on
    /// the run; the rest read the definitions, because nothing yet tracks what
    /// another knight has been through.
    fn combat_plates(&self) -> Vec<status::Plate> {
        let Some(w) = self.world.as_ref() else { return Vec::new() };
        w.bout
            .fighters
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let which = w.knight_at(i);
                let def = self.knights.get(which);
                let mine = i == 0 && self.run.knight.named();
                // A creature's plate carries its own name and no lives: a
                // troll has no sheet to read them off.
                let creature = w.creature_name(i);
                status::Plate {
                    name: if let Some(c) = creature.clone() {
                        c
                    } else if mine {
                        self.run.knight.name.clone()
                    } else {
                        def.map_or_else(|| format!("Knight {}", which + 1), |d| d.name.clone())
                    },
                    colour: w.seat_colour(i),
                    health: f.health,
                    max_health: f.max_health,
                    lives: if creature.is_some() {
                        0
                    } else if mine {
                        self.run.lives
                    } else {
                        def.map_or(0, |d| d.life)
                    },
                }
            })
            .collect()
    }

    /// The scene, and then the character sheet over it if it is up.
    fn render(&mut self) {
        self.draw_scene();
        // The screen has just loaded its own palette, which is the moment the
        // original installs a glow: `MapEffects` and `ChooseKnight` both do it
        // straight after the picture. Anything seeded a frame early is seeded
        // again here rather than breathing between the wrong two colours.
        self.fx.reseed(&self.fb.palette);
        if self.sheet {
            let seat = self.run.knight.seat;
            let colour = henge_assets::player_colours(&self.fb.palette)[seat % 4];
            let font = self.fonts.get("small").or_else(|| self.fonts.get("bold"));
            // The menu is live on the map, where the sheet is modal; in an
            // arena the sheet is a card held up over the fight.
            let rows: Vec<(String, bool)> =
                self.sheet_rows().into_iter().map(|(l, lit, _)| (l, lit)).collect();
            let cursor = (self.mode == Mode::Map && !rows.is_empty()).then_some(self.sheet_cursor);
            status::draw_sheet(
                &mut self.reg, &mut self.fb, font, &self.run, &self.items, colour, &rows, cursor,
            );
        }
        // The pointer goes on last, over whatever it is pointing at, and only
        // on a screen that has boxes for it to be over.
        if !self.gadgets.is_empty() {
            let p = self.pointer;
            shell::draw_pointer(&mut self.reg, &mut self.fb, &p);
        }
    }

    fn draw_scene(&mut self) {
        #[cfg(feature = "research")]
        if let Some(v) = self.research.as_ref() {
            v.render(&mut self.fb);
            return;
        }
        // The between-days screen sits over everything, because that is what
        // it is: the moment between one turn of the map and the next.
        if self.interlude > 0 {
            let fonts = shell::Fonts {
                bold: self.fonts.get("bold"),
                small: self.fonts.get("small"),
            };
            let note = self.run.is_toad().then_some("You are a toad, and a toad has no turn");
            shell::draw_interlude(
                &mut self.reg, &mut self.fb, &fonts, self.run.day, self.run.moon.phase(),
                self.messages.wait(self.hint), note,
            );
            return;
        }
        // A message box sits over everything else for the same reason: it is
        // what `WAITMESSAGE`, `OCCURMESSAGE` and `INSTRUCTMESSAGE` do.
        if let Some(msg) = self.showing.as_ref() {
            let fonts = shell::Fonts {
                bold: self.fonts.get("bold"),
                small: self.fonts.get("small"),
            };
            let msg = msg.clone();
            shell::draw_message(&mut self.reg, &mut self.fb, &fonts, &msg);
            return;
        }
        if self.mode == Mode::Intro {
            let fonts = shell::Fonts {
                bold: self.fonts.get("bold"),
                small: self.fonts.get("small"),
            };
            let intro = self.intro;
            let cast = self.intro_cast.clone();
            shell::draw_intro(&mut self.reg, &mut self.fb, &fonts, &intro, cast.as_deref());
            return;
        }
        if self.mode == Mode::Title {
            let fonts = shell::Fonts {
                bold: self.fonts.get("bold"),
                small: self.fonts.get("small"),
            };
            // The title owns the whole frame, so it is taken out of self for the
            // draw the way the map is.
            let title = std::mem::take(&mut self.title);
            title.render(&mut self.reg, &mut self.fb, &fonts);
            self.title = title;
            return;
        }
        if self.mode == Mode::Select {
            if let Some(select) = self.select.take() {
                let fonts = shell::Fonts {
                    bold: self.fonts.get("bold"),
                    small: self.fonts.get("small"),
                };
                select.render(&mut self.reg, &mut self.fb, &fonts, &self.knights, &self.items);
                self.select = Some(select);
                return;
            }
        }
        if self.mode == Mode::Place {
            if let Some(scene) = self.visiting.as_ref() {
                if let Some(def) = self.places.get(&scene.visit.place) {
                    // Places are drawn in the small font: their menus sit in
                    // panels the original painted only a few pixels wide.
                    let font = self.fonts.get("small").or_else(|| self.fonts.get("bold"));
                    if scene
                        .render(&mut self.reg, &mut self.fb, def, font, &self.run, &self.items)
                        .is_ok()
                    {
                        return;
                    }
                }
            }
        }
        if self.mode == Mode::Map {
            // Take the map out of self for the duration of the draw, so it can
            // borrow the registry and the fonts alongside it.
            if let Some(m) = self.map.take() {
                let near = henge_core::place::nearest(&self.places, m.state.x, m.state.y, 16)
                    .map(|(_, d)| d.name.clone());
                let notice = (self.robbed_for > 0).then_some(self.robbed.as_str());
                let icons = self.map_icons();
                let marks = map::Marks {
                    here: near.as_deref(),
                    notice,
                    icons: &icons,
                };
                let ok = m
                    .render(&mut self.reg, &mut self.fb, &self.fonts, &self.run, &marks)
                    .is_ok();
                self.map = Some(m);
                if ok {
                    self.draw_flight();
                    self.draw_run_over();
                    return;
                }
            }
        }
        let drawn = match self.world.as_mut() {
            Some(w) => w.render(&mut self.reg, &mut self.fb).is_ok(),
            None => false,
        };
        if drawn {
            // The plates go on after the arena, so they sit over the ground
            // rather than under the fighters.
            let plates = self.combat_plates();
            let font = self.fonts.get("small").or_else(|| self.fonts.get("bold"));
            status::draw_plates(&mut self.reg, &mut self.fb, font, &plates);
            return;
        }
        // Nothing loaded: a slow sweep, so it is obvious the window and timing
        // are alive and the problem is the data.
        self.fb.clear(0);
        for y in 0..SCREEN_H {
            for x in 0..SCREEN_W {
                self.fb.pixels[y * SCREEN_W + x] = ((x + y + (self.tick / 2) as usize) / 8 % 32) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The chain a key press actually walks: winit's `KeyCode`, its own name,
    /// the binding table, and the slot the game reads. Worth a test because the
    /// name is taken from the `Debug` impl, so a rename upstream would silently
    /// unbind the arrow keys rather than fail to compile.
    fn slots_for(b: &input::Bindings, code: KeyCode) -> Vec<usize> {
        b.raised_by(&input::Source::key(&format!("{code:?}")))
            .into_iter()
            .map(|(seat, action)| slot_of(seat, action))
            .collect()
    }

    #[test]
    fn the_default_bindings_reach_the_slots_the_game_reads() {
        let b = input::Bindings::default();
        for (code, slot) in [
            (KeyCode::ArrowUp, 0),
            (KeyCode::ArrowDown, 1),
            (KeyCode::ArrowLeft, 2),
            (KeyCode::ArrowRight, 3),
            (KeyCode::Space, 6),
            (KeyCode::KeyW, 7),
            (KeyCode::KeyS, 8),
            (KeyCode::KeyA, 9),
            (KeyCode::KeyD, 10),
            (KeyCode::KeyF, 11),
        ] {
            assert_eq!(slots_for(&b, code), vec![slot], "{code:?} lost its slot");
        }
    }

    #[test]
    fn the_developer_keys_are_not_in_the_binding_table() {
        let b = input::Bindings::default();
        for code in [KeyCode::BracketLeft, KeyCode::Comma, KeyCode::Enter, KeyCode::Tab] {
            assert!(slots_for(&b, code).is_empty(), "{code:?} is bound as a control");
        }
        assert_eq!(key_index(KeyCode::BracketLeft), 4);
        assert_eq!(key_index(KeyCode::Enter), 12);
        // And none of them is one of the ten seat slots.
        for code in [KeyCode::BracketLeft, KeyCode::BracketRight, KeyCode::Enter,
                     KeyCode::Comma, KeyCode::Period] {
            let i = key_index(code);
            assert!(!(0..=3).contains(&i) && !(6..=11).contains(&i), "{code:?} took slot {i}");
        }
    }

    #[test]
    fn rebinding_moves_the_slot_a_key_feeds() {
        let mut b = input::Bindings::default();
        let (seat, action, src) =
            input::Bindings::parse_bind("0:fire=Enter").expect("a readable spec");
        b.bind(seat, action, src);
        assert_eq!(slots_for(&b, KeyCode::Enter), vec![6]);
        // And the key it replaced still works, because a list is a list.
        assert_eq!(slots_for(&b, KeyCode::Space), vec![6]);
    }

    #[test]
    fn the_originals_own_keys_reach_the_same_slots() {
        let b = input::Bindings::as_the_original_had_them();
        assert_eq!(slots_for(&b, KeyCode::Enter), vec![6]);
        assert_eq!(slots_for(&b, KeyCode::Tab), vec![11]);
        assert_eq!(slots_for(&b, KeyCode::KeyX), vec![8]);
        assert_eq!(slots_for(&b, KeyCode::ArrowUp), vec![0]);
    }
}
