//! Desktop entry point.
//!
//! Release builds contain no original-game data. `--features research` adds a
//! viewer for studying the 1991 files, which is a development tool only.

mod framebuffer;
mod input;
mod map;
mod online;
mod place;
#[cfg(feature = "research")]
mod research;
mod shell;
mod sprite;
mod status;
mod text;
mod town;
mod world;

use framebuffer::Framebuffer;
use henge_assets::Registry;
use henge_audio::{sfx, Clips, Sink};
use henge_core::combat::{Intent, State};
use henge_core::ending::Ending;
use henge_core::harness::Snapshot;
use henge_core::intro::Intro;
use henge_core::item::Items;
use henge_core::knight::{Ability, Knight, Knights};
use henge_core::message::{Message, Messages};
use henge_core::place::{Answer, Overlaps, Places};
use henge_core::pointer::{Gadgets, Pointer};
use henge_core::rival::{Board, Challenged, Frame, Settled};
use henge_core::run::{Cast, Run};
use henge_core::shell::Start;
use henge_core::status::{Op, Screen as SheetScreen};
use henge_core::stones::Stones;
use henge_core::{SCREEN_H, SCREEN_W};
use map::MapScene;
use std::num::NonZeroU32;
use std::rc::Rc;
use text::Font;
use winit::event::{ElementState, Event, WindowEvent};
use winit::event_loop::EventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::WindowBuilder;
use world::{RivalSheet, Sheet, World};

fn args_of() -> Vec<String> {
    std::env::args().collect()
}

/// What the pack holds, for one column of the trace. Ids rather than names,
/// because a trace is read against the data and the data is keyed by id.
fn carrying(run: &Run) -> String {
    if run.kit.is_empty() {
        return "-".into();
    }
    run.kit
        .iter()
        .map(|(id, n)| {
            if n == 1 {
                id.to_string()
            } else {
                format!("{id}x{n}")
            }
        })
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
///                     h/j/k/l walk, c the character sheet, . waits
///   --hurt <hp>       start the run already wounded, so a healer has something
///                     to do without first having to win and lose a fight
///   --gold <n>        start the run with coin, so a stall can be reached
///                     without first winning the fights that pay for it
///   --keys <0..4>     start holding that many of the four lair keys
///   --stone <name>    start carrying one of the four moonstones
///   --floor <n>       open lair n's own page, which is `MOON:LairGEM`.
///                     With --scouted it is the page a gem flight ends on
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
///   --sounds          print a line for every sound a script asks for: the
///                     tick, the id, the sample it translates to and the script
///                     that asked. A sound cannot be seen in a screenshot
///   --save <path>     where the **test harness** puts a posed run. Not a game
///                     feature: the original has no save, so no key and no menu
///                     item reaches this. See `henge_core::harness`
///   --load            read one at start, before anything else is posed
///
/// In `--input`, a digit is the joystick held with fire, laid out like a
/// numpad: 8 up, 2 down, 4 left, 6 right, 7 9 1 3 the diagonals, 5 fire on
/// its own. That is how the direction chosen attacks are reached headlessly.
fn hurt_arg(a: &[String]) -> Option<i32> {
    a.iter()
        .position(|s| s == "--hurt")
        .and_then(|i| a.get(i + 1))
        .and_then(|v| v.parse().ok())
}

fn gold_arg(a: &[String]) -> Option<u32> {
    a.iter()
        .position(|s| s == "--gold")
        .and_then(|i| a.get(i + 1))
        .and_then(|v| v.parse().ok())
}

/// `--keys <0..4>`: start holding that many of the four lair keys, so the
/// Valley of the Gods can be reached without first clearing four lairs.
fn keys_arg(a: &[String]) -> Option<usize> {
    a.iter()
        .position(|s| s == "--keys")
        .and_then(|i| a.get(i + 1))
        .and_then(|v| v.parse().ok())
}

/// `--lives <n>`: how many life points to ride out with, so the game-over
/// screen is one lost fight away instead of five.
fn lives_arg(a: &[String]) -> Option<i32> {
    a.iter()
        .position(|s| s == "--lives")
        .and_then(|i| a.get(i + 1))
        .and_then(|v| v.parse().ok())
}

/// `--magic <item>`: start carrying one of the ten magic items, so the stone
/// circle's offering, which the wizard is otherwise the only source of, can be
/// reached in one step. The id is `service::magic_item`'s, e.g.
/// `scroll_of_haste`.
fn magic_arg(a: &[String]) -> Option<String> {
    let name = a
        .iter()
        .position(|s| s == "--magic")
        .and_then(|i| a.get(i + 1))?
        .clone();
    if henge_core::service::magic_slot(&name).is_none() {
        eprintln!("no magic item called {name}");
        return None;
    }
    Some(name)
}

/// `--stone <new|full|half|gibbous>`: start carrying that moonstone, so the
/// stone circle's winning branch can be reached without beating the Guardian.
fn stone_arg(a: &[String]) -> Option<henge_core::moon::Moonstone> {
    let name = a
        .iter()
        .position(|s| s == "--stone")
        .and_then(|i| a.get(i + 1))?;
    let found = henge_core::moon::Moonstone::ALL
        .into_iter()
        .find(|m| m.item().trim_start_matches("moonstone.") == name);
    if found.is_none() {
        eprintln!("no moonstone called {name}: try new, full, half or gibbous");
    }
    found
}

fn knight_arg(a: &[String]) -> Option<usize> {
    a.iter()
        .position(|s| s == "--knight")
        .and_then(|i| a.get(i + 1))
        .and_then(|v| v.parse().ok())
}

fn foe_arg(a: &[String]) -> Option<String> {
    a.iter()
        .position(|s| s == "--foe")
        .and_then(|i| a.get(i + 1))
        .cloned()
}

/// Which screen a headless run opens on.
///
/// The default is the map, so every recipe written before the shell existed
/// still does what it did. An interactive run opens on the title, because that
/// is where a game opens.
fn start_arg(a: &[String]) -> Option<Mode> {
    let name = a
        .iter()
        .position(|s| s == "--start")
        .and_then(|i| a.get(i + 1))?;
    match name.as_str() {
        "intro" => Some(Mode::Intro),
        "ending" => Some(Mode::Ending),
        "title" => Some(Mode::Title),
        "select" => Some(Mode::Select),
        "map" => Some(Mode::Map),
        "arena" | "combat" => Some(Mode::Combat),
        // Ours: the lobby, so it can be looked at without a friend.
        "online" | "lobby" => Some(Mode::Online),
        other => {
            eprintln!(
                "no screen called {other}: try intro, ending, title, select, map, arena or online"
            );
            None
        }
    }
}

/// `--point x,y`: put the pointer there, for checking the gadgets headlessly.
fn point_arg(a: &[String]) -> Option<(i32, i32)> {
    let v = a
        .iter()
        .position(|s| s == "--point")
        .and_then(|i| a.get(i + 1))?;
    let (x, y) = v.split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

/// `--save <path>`: where the **test harness** puts a posed run.
///
/// **Not a game feature.** The original has no save and neither does this game:
/// there is no key, no menu item and nothing on any screen that reaches this. It
/// exists so a headless test can pose a run once and load it instead of walking
/// the whole way there again, and it is reachable from the command line and from
/// nowhere else. See `henge_core::harness`.
fn snapshot_path_arg(a: &[String]) -> String {
    a.iter()
        .position(|s| s == "--save")
        .and_then(|i| a.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "henge-harness.json".to_string())
}

/// `--controls <path>`: where the binding table and the stick calibration live.
/// A file, because the table is a setting rather than part of the game.
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
}

impl Script {
    fn from_args(a: &[String]) -> Script {
        let after = |flag: &str| {
            a.iter()
                .position(|s| s == flag)
                .and_then(|i| a.get(i + 1))
                .cloned()
        };
        Script {
            goto: after("--goto").and_then(|v| {
                let (x, y) = v.split_once(',')?;
                Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
            }),
            keys: after("--input")
                .map(|s| s.chars().collect())
                .unwrap_or_default(),
            at: after("--at"),
        }
    }

    fn active(&self) -> bool {
        self.goto.is_some() || !self.keys.is_empty() || self.at.is_some()
    }

    /// Drive one tick: steer while there is still ground to cover, then start
    /// feeding key presses.
    fn drive(&self, app: &mut App, fed: &mut usize) {
        // The between-days screen waits on fire, as `WaitFIRE` does, so a walk
        // long enough to turn a day over needs someone to press it. The script
        // stands in for the player here as it does everywhere else.
        if app.interlude > 0 {
            app.keys = [false; 256];
            app.pressed[6] = true;
            return;
        }
        if let Some((gx, gy)) = self.goto {
            match app.mode {
                // A fight on the way, which only a lair or the dragon can
                // start, is swung at rather than stood in.
                Mode::Combat => {
                    app.keys = [false; 256];
                    app.keys[6] = app.tick % 23 < 4;
                    return;
                }
                Mode::Map => {
                    // `is_none_or` would read better but postdates the crate's
                    // minimum Rust version. A flight is not a walk: once the
                    // gem or the hawk is up, the script stops steering towards
                    // the point it was given, or it would drag the token back
                    // out of the air every tick.
                    let there = match app.map.as_ref() {
                        Some(m) => m.state.x == gx && m.state.y == gy,
                        None => true,
                    };
                    if !there && app.flight.is_none() {
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
        if mode == Mode::Online {
            app.begin_online();
        }
    }
    // **Ours**: the lobby, skipped. `--host <name>` opens one and sits down in
    // it, `--join <address>` dials one, and `--begin` starts a game as soon as
    // the lobby will allow it. They exist so that a game can be got into from a
    // shortcut or a script rather than through the menu, and so that the whole
    // lockstep path can be driven by `--trace` with nobody at a keyboard.
    if let Some(name) = after_arg(a, "--host") {
        app.mode = Mode::Online;
        app.begin_online();
        let you = after_arg(a, "--name").unwrap_or_else(|| "HOST".into());
        app.online_ask(online::Ask::Open {
            game: name,
            you,
            password: after_arg(a, "--password").unwrap_or_default(),
        });
        // Sat down and ready: a host driven from the command line has nobody to
        // press the row for them.
        app.online_ask(online::Ask::Seat { ready: true });
        if let Some(o) = app.online.as_mut() {
            o.ready = true;
        }
    }
    if let Some(address) = after_arg(a, "--join") {
        app.mode = Mode::Online;
        app.begin_online();
        let you = after_arg(a, "--name").unwrap_or_else(|| "GUEST".into());
        app.online_ask(online::Ask::Dial {
            address,
            you,
            password: after_arg(a, "--password").unwrap_or_default(),
        });
    }
    if a.iter().any(|x| x == "--begin") {
        let want = after_arg(a, "--players")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(1);
        app.auto_begin = Some(want.clamp(1, henge_core::shell::SEATS));
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
    if let Some(item) = magic_arg(a) {
        app.run.kit.take(&item, 1);
    }
    // `--won`: end the run as a victory, so the winning tally can be looked at
    // without beating the Guardian first. The losing one is one lost fight
    // away, but the winning one is the end of a whole quest, and a screen
    // nobody can reach in testing is a screen nobody checks.
    if a.iter().any(|s| s == "--won") {
        app.run.won = true;
    }
    // `--floor <n>`: put the lair's own page up, which in play is one won
    // guardian away and, on the gem, a whole flight away. `--scouted` with it
    // is the same page as `EffectFLAG+4` leaves it: `Identify` rather than
    // `Take`, and nothing on the floor can be lifted.
    if let Some(n) = a
        .iter()
        .position(|s| s == "--floor")
        .and_then(|i| a.get(i + 1))
        .and_then(|v| v.parse::<usize>().ok())
    {
        if app.run.lairs.is_empty() {
            app.stock_lairs();
        }
        let scouted = a.iter().any(|s| s == "--scouted");
        if !scouted {
            app.run.lair_beaten(n);
        }
        app.open_lair_page(n, scouted);
    }
    // `--stones`: put the stone circle's set piece up straight away. The
    // offering that reaches it in play is the wizard's gift given back, which
    // is a whole quest away, and a screen nobody can reach in testing is a
    // screen nobody checks.
    if a.iter().any(|s| s == "--stones") {
        app.stones = Some(Stones::new(app.run.knight.seat as u8));
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
    // `--load` replaces everything above it: a snapshot is the state, and posing
    // a run and then loading over it would be posing nothing. Harness only; see
    // `snapshot_path_arg`.
    if a.iter().any(|s| s == "--load") && !app.harness_load() {
        std::process::exit(3);
    }
    // `--point x,y` puts the pointer somewhere, which is the only way to reach
    // it with no mouse and no display.
    if let Some((x, y)) = point_arg(a) {
        app.point_at(x, y);
    }
    // `--pace <percent>`, ours: see [`App::pace`]. Out of range is clamped
    // rather than refused, so a recipe cannot fail on it.
    if let Some(p) = a
        .iter()
        .position(|s| s == "--pace")
        .and_then(|i| a.get(i + 1))
        .and_then(|v| v.parse::<u32>().ok())
    {
        app.pace = p.clamp(PACE_MIN, PACE_MAX);
    }
    app.sheet = a.iter().any(|s| s == "--sheet");
    if a.iter().any(|s| s == "--bloodless") {
        app.title.state.gore = false;
        if let Some(w) = app.world.as_mut() {
            w.set_gore(false);
        }
    }
}

/// `ShakeScreen`'s own `mov cx, 0xf` (0x4962).
const SHAKE_FRAMES: u32 = 15;

fn main() -> anyhow::Result<()> {
    let mut app = App::new()?;

    // Headless trace: run the simulation and report it, so combat can be checked
    // as behaviour rather than by squinting at screenshots.
    if let Some(i) = args_of().iter().position(|a| a == "--trace") {
        let a = args_of();
        let ticks: u64 = a.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(600);
        let arena: i32 = a.get(i + 2).and_then(|s| s.parse().ok()).unwrap_or(0);
        if let Some(w) = app.world.as_mut() {
            w.step_arena(arena);
        }
        let script = Script::from_args(&a);
        let scripts = a.iter().any(|s| s == "--scripts");
        prepare(&mut app, &a);
        if let Some(id) = script.at.as_deref() {
            app.enter(id);
        }
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
            // **Ours**: a trace runs flat out, which is right for a fight and
            // wrong for anything that is waiting on a socket. A lobby with
            // nobody in it yet, and a lockstep tick whose peers have not sent
            // their word, both have nothing to do until the wire says
            // otherwise, and spinning through the whole tick budget in a
            // millisecond would mean a scripted host had exited before a
            // scripted guest could knock. Only online runs ever reach this.
            if app.mode == Mode::Online || app.net.is_some() {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            // A message box is modal, so the mode underneath it is not what is
            // on screen. Say which kind and what it says, or a trace would
            // report a town menu nobody can see.
            let line = if let Some(m) = app.showing.as_ref() {
                let said: Vec<&str> = m.shown().map(|l| l.text.trim()).collect();
                format!("{t:>5}  MESSAGE {:?}  {}", m.kind, said.join(" / "))
            } else {
                match app.mode {
                    // The shell has no state worth a line of trace: what it does is
                    // decided by looking at it, and it is checked by test in
                    // `henge_core::shell` instead.
                    Mode::Intro => format!("{t:>5}  INTRO   card {}", app.intro.card),
                    Mode::Ending => format!(
                        "{t:>5}  ENDING  scene {} code {:#04x}",
                        app.ending.card, app.ending.code
                    ),
                    Mode::Title => format!("{t:>5}  TITLE"),
                    Mode::Select => format!("{t:>5}  SELECT"),
                    // The lobby, as much of it as there is to say: who is in
                    // it, whether they are ready, and the last thing that
                    // happened, which is where a game that will not start says
                    // why.
                    Mode::Online => {
                        let o = app.online.as_ref();
                        let who: Vec<String> = o
                            .map(|o| {
                                o.roster
                                    .players
                                    .iter()
                                    .map(|p| {
                                        format!("{}{}", p.name, if p.ready { "*" } else { "" })
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        format!(
                            "{t:>5}  LOBBY   [{}]  {}",
                            who.join(" "),
                            o.map(|o| o.note.as_str()).unwrap_or("")
                        )
                    }
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
                        // And the dragon, when it is in the air: where it is
                        // and whom it is after, since the swoop is only
                        // checkable if its shadow can be watched.
                        let dragon = if r.dragon.aloft {
                            format!(
                                " dragon@{},{} after {}",
                                r.dragon.x,
                                r.dragon.z,
                                r.dragon.target.map_or("-".to_string(), |t| t.to_string())
                            )
                        } else {
                            String::new()
                        };
                        format!("{:>5}  MAP     day {:<3} at {:>3},{:<3} hp{:>4}  gold{:>5} {:<12} won {:<3} fought {:<3} {} on {} {} s{}c{}e{} xp{}{}{}{}",
                        t, m.state.day, m.state.x, m.state.y, r.health, r.gold, carrying(r),
                        r.victories, r.fights,
                        if r.alive() { "     " } else { "ENDED" }, m.last_terrain.name(),
                        // The moon, because four days move it and what waits in
                        // an arena moves with it: a calendar is only checkable
                        // if it is on the line.
                        r.moon.phase().key(),
                        k.strength, k.constitution, k.endurance, r.experience, aloft, dragon,
                        if app.sheet { format!(" SHEET > {}", app.sheet_said.as_deref().unwrap_or("-")) } else { String::new() })
                    }
                    Mode::Place => {
                        let Some(s) = app.visiting.as_ref() else {
                            break;
                        };
                        let Some(def) = app.places.get(&s.visit.place) else {
                            break;
                        };
                        // The tune, when the room has one. Five of the original's
                        // rooms do and nothing else in the game does, so a trace is
                        // the way to check that the right one is on and that it
                        // stops at the door.
                        let tune = match app.audio.music() {
                            Some(id) => format!("  [{id}]"),
                            None => String::new(),
                        };
                        let what = match app.door.as_ref() {
                            Some(open) => town::describe(open, &app.run),
                            None => place::describe(def, &s.visit),
                        };
                        format!(
                            "{:>5}  PLACE   day {:<3} hp{:>4}  gold{:>5} {:<12} {:<34} {}{}{}",
                            t,
                            app.run.day,
                            app.run.health,
                            app.run.gold,
                            carrying(&app.run),
                            what,
                            s.visit.said,
                            if app.sheet_said.is_some() {
                                format!(" > {}", app.sheet_said.as_deref().unwrap_or(""))
                            } else {
                                String::new()
                            },
                            tune
                        )
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
                                    f.task
                                        .as_ref()
                                        .map_or(String::new(), |t| format!(" {}", t.pc.script))
                                } else {
                                    String::new()
                                };
                                // The facing, as the record's `+8` would read: `>`
                                // is 1 and `<` is 3. Which way a creature faces is
                                // decided by its controller and nothing else, so a
                                // trace is the place to see it turn.
                                let face = if f.facing < 0 { '<' } else { '>' };
                                // `TASKSTANDBY` has taken this one's task off
                                // the draw list and somebody else is drawing
                                // him: a held knight, a tossed corpse, or the
                                // knight the demon's zap has off the board.
                                let standby = if f.hidden { "~" } else { "" };
                                format!(
                                    "{}{:<6} {:<12}{:>4} @{:>3},{:>3}{}{}",
                                    standby, f.actor, state, f.health, f.x, f.y, face, script
                                )
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
                            who.push(format!(
                                "{} blocked {} with {}",
                                p.target,
                                p.attacker,
                                p.with.name()
                            ));
                        }
                        // How many more the fight owes and how many it holds at
                        // once, which is `TotalMonsters`, `MaxMonsters` and
                        // `NumberInCombat`: a wave arriving is only checkable if
                        // the three counts are on the line.
                        let wave = &w.bout.wave;
                        let owing = if wave.max > 0 {
                            format!(
                                " owed{:>3} max{} in{} ",
                                wave.total, wave.max, wave.in_combat
                            )
                        } else {
                            String::new()
                        };
                        // The arena's own name as well as its family, because which of
                        // the eight a family rotates to is now a thing worth seeing.
                        format!(
                            "{:>5}  COMBAT  {:<5} {:<8}{} {}",
                            t,
                            w.name(),
                            w.family(),
                            owing,
                            who.join(" | ")
                        )
                    }
                }
            };
            let key = line[7..].to_string();
            if key != last {
                println!("{line}");
                last = key;
            }
        }
        return Ok(());
    }

    // Headless capture, for checking rendering without a display and for CI.
    //   moonstone --screenshot out.png [ticks] [arena-index]
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--screenshot") {
        let path = args
            .get(i + 1)
            .cloned()
            .unwrap_or_else(|| "shot.png".into());
        let ticks: u64 = args.get(i + 2).and_then(|s| s.parse().ok()).unwrap_or(0);
        let arena: i32 = args.get(i + 3).and_then(|s| s.parse().ok()).unwrap_or(0);
        if let Some(w) = app.world.as_mut() {
            w.step_arena(arena);
        }
        app.keys[3] = args.iter().any(|a| a == "--walk");
        app.keys[6] = args.iter().any(|a| a == "--fight");
        let script = Script::from_args(&args);
        prepare(&mut app, &args);
        if let Some(id) = script.at.as_deref() {
            app.enter(id);
        }
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
            // **Ours**: the same wait `--trace` takes, and for the same reason.
            // A capture of a lobby has to let a peer actually knock, and one of
            // a lockstep game has to let the other end speak; both have nothing
            // to do until the wire says otherwise, and spinning the whole tick
            // budget in a millisecond would photograph an empty room. Only a
            // capture of an online screen ever reaches this.
            if app.mode == Mode::Online || app.net.is_some() {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
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
    // the intro and the title follows it. Either seat's fire skips it, and
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
            0 => println!("gamepads: none plugged in; J on the title calibrates one"),
            n => println!("gamepads: {n} found; J on the title calibrates player one's"),
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

    // The game does not have one clock, and which loop waits on which is
    // recovered per loop in [`TIMER_TICK`] and [`RETRACE_TICK`]. A pass of the
    // loop that is up costs a whole number of ticks of its own clock, so the
    // length of a tick is looked up every pass from [`App::tick_len`].
    //
    // Time is accumulated and spent in whole ticks so the simulation never sees
    // a fractional step, which is what keeps two machines agreeing on it. One
    // accumulator is enough for that: what is left over on a change of clock is
    // always less than one tick of the clock that was just in use, so the worst
    // it can do is pay the first tick of the new clock early. It can never hand
    // the simulation a part of a tick.
    let mut last = std::time::Instant::now();
    let mut owed = std::time::Duration::ZERO;

    event_loop.run(move |event, elwt| {
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => elwt.exit(),
            // A real mouse moves the pointer straight to where it is. The
            // stick still works; this is the same pointer either way.
            Event::WindowEvent {
                event: WindowEvent::CursorMoved { position, .. },
                ..
            } => {
                let size = window.inner_size();
                if let Some((x, y)) = Framebuffer::to_screen(
                    size.width as usize,
                    size.height as usize,
                    position.x,
                    position.y,
                ) {
                    app.point_at(x, y);
                }
            }
            // There is no mouse button here. A real mouse moving the pointer is
            // inherent in having a window, but a click standing in for fire was
            // ours: the original drives the pointer with the stick and takes a
            // gadget with the stick's own fire button, which is the key the
            // seat already has.
            Event::WindowEvent {
                event: WindowEvent::KeyboardInput { event, .. },
                ..
            } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    let down = event.state == ElementState::Pressed;
                    // `OptionKeys` at `0x128d` tests scancode 0x01, Escape, and
                    // returns; `StartAgain` at `0x00ba` tests it again and
                    // returns out of the program. So Escape quits, and it quits
                    // from the options screen, which is this shell's title. A
                    // window also has a close button, which is the other way out
                    // and not ours to remove.
                    if down && code == KeyCode::Escape && app.quits_on_escape() {
                        elwt.exit();
                    }
                    app.key(code, down);
                }
            }
            Event::AboutToWait => {
                let now = std::time::Instant::now();
                owed += now - last;
                last = now;
                // A stall (a dragged window, a sleeping laptop) must not be
                // paid back as a burst of ticks; cap what can be owed, in ticks
                // of the clock the loop that is up runs on.
                if owed > app.tick_len() * 6 {
                    owed = app.tick_len() * 6;
                }
                // The sticks, once a frame and before the ticks they feed.
                // The original reads them in its own frame loop and ORs them
                // into the key word, and this is the same place.
                pads.poll();
                app.pads_tick(&pads);
                let mut ticked = false;
                // The clock is read again every pass because a tick can change
                // which loop is up: walking into an arena moves the game off the
                // retrace and onto the timer between one tick and the next.
                while owed >= app.tick_len() {
                    let tick = app.tick_len();
                    app.update();
                    owed -= tick;
                    ticked = true;
                }
                elwt.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(
                    last + (app.tick_len() - owed),
                ));
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
                    let shake = app.shake_rows;
                    app.fb.present_into(
                        &mut buffer,
                        size.width as usize,
                        size.height as usize,
                        &palette,
                        shake,
                    );
                    let _ = buffer.present();
                }
            }
            _ => {}
        }
    })?;
    Ok(())
}

/// One tick of the **programmed timer**, which is what paces the fight.
///
/// **Recovered.** `Combat` at image `0x351` opens with `call 0x96e1` and closes
/// with `call 0x96f1` at `0x36c`, and the whole loop body sits between them.
/// `0x96e1` is `sub ax, ax; mov es, ax; mov ax, es:[0x46c]; inc ax; inc ax; mov
/// [0xc2f6], ax`: the BIOS tick counter at `0000:046c` plus two, stored as a
/// deadline. `0x96f1` reads the same counter and spins `jb` until it has reached
/// that deadline. So **one combat frame is two BIOS ticks**, and the retrace wait
/// the loop also makes at `0x354` never dominates, being about 14 ms inside a
/// much longer budget. These two routines are called from nowhere else in the
/// image: one site each, both in `Combat`.
///
/// The rate of `0000:046c` is still the standard one, and that is the step this
/// engine used to miss. `Install_Timer` at `0x584f` saves the old `int 8` vector
/// into `[0x7c1b]`/`[0x7c1d]` (`0x5865`, `0x586c`), points the vector at its own
/// handler (`0x5874`, entry `0x5928`), and programs the 8253 with `mov al, 0x36;
/// out 0x43, al` and divisor `0x5555` at `0x58b9`..`0x58c4`. That interrupts at
/// 1193182 / 21845 = **54.6204 Hz**. The handler calls the two drivers (`int
/// 60h`, `int 61h`) and then at `0x5945` decrements `[0x7c19]`; only when that
/// reaches zero does it reload it with 3 and `lcall [0x7c1b]` at `0x5951`,
/// chaining to the original BIOS handler. So it chains **every third tick** and
/// `0000:046c` keeps ticking at the usual **18.2068 Hz**.
///
/// A combat frame is therefore 2 / 18.2068 = **109.849 ms, 9.1034 frames a
/// second**, which is exactly **six ticks of the 54.6204 Hz timer** — and six is
/// the number every routine that sets a fight up writes into `DELAY`
/// (`DS:0x91c`). A byte scan for `mov word [0x091c], 6` finds thirteen sites and
/// nothing else touches the word at all: the eleven `InitKnightvs*` routines
/// (`0x208c`, `0x20e2`, `0x216e`, `0x220b`, `0x22b6`, `0x2366`, `0x2525`,
/// `0x25a3`, `0x261a`, `0x26e8`, `0x27b9`) plus `InitGameStart+0xda` (`0x1ce7`)
/// and `InitPractice+0x41` (`0x202f`). **Nothing reads `DELAY` back** — the same
/// scan finds no read — because the loop hardcodes the same duration as two BIOS
/// ticks.
/// It is `henge_core::content::ActorDef::script_ticks`.
///
/// This is derived rather than written out: the divisor over the 8253's input
/// clock, in nanoseconds.
const TIMER_TICK: std::time::Duration =
    std::time::Duration::from_nanos(21_845 * 1_000_000_000 / 1_193_182);

/// One tick of the **vertical retrace**, which is what paces everything else.
///
/// **Recovered.** The wait is the unnamed public routine at image `0x5a24`,
/// between `AdjustJoy` and the start of `GFX`: `mov dx, 0x3da`, spin while bit 3
/// is set, then spin until it is set again, which is exactly one retrace. The one
/// other pacing helper in the image is at `0xafeb`: `mov cx, ax; call 0x5a24;
/// loop`, which waits `ax` retraces. A byte scan for `mov dx, 0x3da` finds only
/// three sites in the whole image — this one, `BlackScreen+7` and the fade at
/// `0x5b6d` — so there is no third wait hiding anywhere.
///
/// Every loop that is not `Combat` waits on one of those two, or on nothing:
///
/// * `MapLOOP` (`0xa306`): one retrace, first call of the pass.
/// * `ChooseLoop` (`0x15a0`), the knight select: one retrace, first call.
/// * `ScanKEYS` (`0x142e`), name entry: `0xafeb` with `ax = 5`, five retraces.
/// * `HengeLOOP` (`0xb3f6`), the stone circle: `0xafeb` with `ax = 3`.
/// * `TavernLoop` (`0xb137`), `StatLOOP` (`0xbe13`), `DonateLoop` (`0xbbe6`),
///   the two stall loops `WDLOOP` (`0xd7a`) and `HWLOOP` (`0xe35`), and
///   `DoOptions`/`OptionKeys` (`0x1241`/`0x1282`), which is this shell's title:
///   **no wait at all**. They blit, page-flip and spin as fast as the machine
///   manages. See the TODO on [`App::tick_len`].
///
/// The rate is the video mode's refresh rate. The game never programs the CRTC's
/// timing or the Miscellaneous Output register: the only CRTC write in the whole
/// image is index 0x0c, the start address, at `0x5a34` and inside `ShakeScreen`
/// at `0x4965`. It therefore runs at the BIOS timing for a 320x200 VGA mode,
/// which is the 400 line timing: a 25.175 MHz dot clock over 800 dots is
/// 31468.75 lines a second, over 449 lines is **70.0863 frames a second**. That
/// is the 70 Hz `henge_core::intro`, `henge_core::ending` and
/// `henge_core::dice` already quote, and their recovered counts are counts of
/// *this* tick and are not rescaled by anything here.
///
/// Derived the same way: 449 lines of 800 dots over the dot clock.
const RETRACE_TICK: std::time::Duration =
    std::time::Duration::from_nanos(800 * 449 * 1_000_000_000 / 25_175_000);

/// Which clock a screen's loop is paced by. See [`App::tick_len`], which is
/// where the per-loop evidence is written down.
/// How many retraces one pass of `MapLOOP` (0xa306) actually took.
///
/// **This one is observed, not recovered, and it is the only number in this
/// file that is.** Everything around it is in the image and this is not, so
/// it is written down rather than folded into something that looks derived.
///
/// What the image does say, exhaustively: the map loop waits for exactly one
/// retrace, at `0xa306`. Every call in its whole body, `0xa306` to `0xa4c0`,
/// was scanned with the targets shift-corrected for both `0x5a24` and the
/// multi-retrace helper `0xafeb`, and there is one hit and no other. Nothing
/// in it counts, divides or defers. `MapEffects` (0xa508) is a one-time
/// install, and `SHOW` (0xa1f0) draws the token and steps the tasks with no
/// wait of its own. So the loop is specified at one pass per retrace.
///
/// But a retrace wait is a floor and not a rate. `0x5a72` blits the compose
/// page with the VGA in write mode 1, sixteen thousand latched byte-moves,
/// and on the hardware of the day that ate most or all of a 14.268 ms
/// retrace on its own. Whenever the pass overran, `0x5a24` caught the *next*
/// retrace and the whole loop, movement and colour cycling together, halved.
/// A fast machine ran it at 70 and a slow one at 35 or worse, which is why
/// there is no constant to find: the original's map speed was its hardware's,
/// and it never existed as a number anybody wrote.
///
/// Two is what Carl reports the original looking like against this build,
/// which had been running the specified 70. Change it if a measurement ever
/// says otherwise; a five second capture of the original's map is enough,
/// since the cycle on entries 0x15..0x17 has a recovered period of 12 passes
/// and counting colour steps against video frames reads the divisor straight
/// off.
const MAP_PASS_RETRACES: u32 = 2;

/// A hundred percent: the rate recovered from the image, which is the floor
/// `Combat` holds every pass to and nothing else.
const PACE_FULL: u32 = 100;

/// What the game actually opens at, and it is **not** [`PACE_FULL`].
///
/// Sixty percent of the recovered floor, which Carl set against his memory of
/// playing the original. That is not a contradiction: the floor is the fastest
/// the original could run and the overrun above it was the machine's, so a
/// number below a hundred is what a real 1991 fight looked like and a hundred
/// is only what the code permits at its quickest. See [`App::pace`].
///
/// **A hundred, which is the recovered rate and no dial at all.**
///
/// It was sixty, then eighty, both of them attempts to make the game feel right
/// by slowing every clock in it at once. That was the wrong knob. The rate in
/// [`tick_len_for`] comes out of the image and a fight that feels too quick at
/// that rate is not evidence the rate is wrong: it is evidence that something
/// the fight is made of moves too far in a pass, and that something is a
/// recovered number too, so it can be found and fixed rather than covered over.
///
/// The dial stays, because a floor is not a rate (see [`App::pace`]) and because
/// it is useful for looking at a fight in slow motion. It simply opens at the
/// number the image says.
const PACE_DEFAULT: u32 = 100;

/// The narrowest and widest the dial goes. Half speed is about where a busy
/// fight on a 1991 machine would have landed; a quarter again over the
/// recovered rate is as fast as it goes, and there is nothing in the original
/// that argues for either bound, so they are round numbers.
const PACE_MIN: u32 = 40;
const PACE_MAX: u32 = 125;

/// One step of the dial.
const PACE_STEP: u32 = 5;

fn tick_len_for(mode: Mode) -> std::time::Duration {
    match mode {
        Mode::Combat => TIMER_TICK,
        Mode::Map => RETRACE_TICK * MAP_PASS_RETRACES,
        Mode::Intro | Mode::Ending | Mode::Title | Mode::Select | Mode::Place => RETRACE_TICK,
        // The lobby is a menu, so it is on the rate every menu in the image is on.
        Mode::Online => RETRACE_TICK,
    }
}

/// How many ticks one pass of this mode's own loop takes.
///
/// A loop's per-pass work (`COLCON`, `KnightGlowOn`) happens once a pass and
/// not once a tick, and the two are only the same thing where a pass is a
/// single wait. In a fight a pass is `DELAY`'s six timer ticks, the 109.849 ms
/// `Combat` (0x351) spends between its deadline at 0x96e1 and paying it at
/// 0x96f1. Every other loop that does per-pass work waits one retrace, which
/// is one tick here.
const COMBAT_PASS_TICKS: u32 = 6;

fn ticks_per_pass(mode: Mode) -> u32 {
    match mode {
        Mode::Combat => COMBAT_PASS_TICKS,
        Mode::Intro
        | Mode::Ending
        | Mode::Title
        | Mode::Select
        | Mode::Map
        | Mode::Place
        | Mode::Online => 1,
    }
}

/// `MudmenGlowOn`: `COLOURGLOW(0x0e, 0x100, 2, 0)`. Palette entry fourteen
/// breathes towards a dark red every other frame for as long as the bout runs.
/// Recovered, and installed by `InitCombat` when mudmen are in the arena.
///
/// The other glow a fight installs is `KnightGlowOn`, which the combat loop
/// calls every frame and which puts three glows on the knight's own entries
/// once he is down to ten health; see [`App::knight_glow_tick`].
const MUDMEN_GLOW: henge_assets::Glow = henge_assets::Glow {
    index: 0x0e,
    target: 0x100,
    period: 2,
    repeat: 0,
};

struct App {
    fb: Framebuffer,
    /// `COLCON`'s two tables and the fade, applied to the palette on its way
    /// to the screen. Item 75; see `henge_assets::palette`.
    fx: henge_assets::Effects,
    /// What each screen installs, out of the pack rather than out of code.
    fx_table: henge_assets::EffectTable,
    /// The seats whose `KnightGlowOn` glows are installed, and the entries
    /// they sit on. The original keeps the handles in `KGT`, `KGT1`, `KGT2`
    /// and `KnightHANDLE1` to `3`, and tests them before installing again.
    knight_glows: std::collections::BTreeMap<usize, Vec<u8>>,
    /// Which screen is up, so a change of screen can fade the new one in the
    /// way every one of the original's own loaders does.
    scene: String,
    /// Place id to tune id, recovered from `LOADMUSIC`'s callers.
    music_places: std::collections::BTreeMap<String, String>,
    tick: u64,
    /// `ShakeScreen` (0x495b) has `mov cx, 0xf`: fifteen passes of the
    /// retrace, and the game loop is stopped for all of them.
    shake: u32,
    /// How many rows the picture is displaced this tick, nought to three.
    /// The renderer's, not the simulation's -- `ShakeScreen` rolls its own
    /// numbers off the game's RNG but nothing reads the result back, so a
    /// lockstep peer that shook differently would still agree on the fight.
    shake_rows: i32,
    shake_rng: u32,
    /// **Ours, and the only number in the pacing that is.** How fast the game
    /// runs, as a percentage of the rate recovered in [`tick_len_for`]: a
    /// hundred is that rate exactly, and a smaller number is slower. It opens
    /// at [`PACE_DEFAULT`], which is a hundred, so by default this changes
    /// nothing at all.
    ///
    /// It is universal on purpose. Every loop's tick goes through here, so the
    /// knight, the creatures, the animation, the colour cycling and the walk
    /// across the map all move together and nothing drifts out of step with
    /// anything else.
    ///
    /// It exists because **the original's frame wait is a floor and not a
    /// rate.** `Combat` (0x351) opens by putting a deadline two BIOS ticks
    /// ahead (0x96e1) and closes by spinning until the counter reaches it
    /// (0x96f1) -- `jb`, so a pass that has already overrun the deadline waits
    /// for nothing at all and the frame simply runs long. On the hardware of
    /// 1991 the compose blit, the sprite pile and the palette work regularly
    /// did overrun it, so what anybody actually played was
    /// `max(109.849 ms, whatever that machine took)`. The floor is in the
    /// image and can be recovered; the overrun was the machine's and cannot.
    ///
    /// So this is a dial, not a discovery, and it is kept apart from the
    /// recovered constants for that reason. It scales the wall clock only:
    /// nothing in `henge_core` reads it, the tick counts are unchanged, and two
    /// peers running it at different settings still agree on the fight.
    pace: u32,
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
    /// potion is the same potion in Highwood as in Waterdeep.
    items: Items,
    /// The four knights, in select order.
    knights: Knights,
    title: shell::TitleScene,
    select: Option<shell::SelectScene>,
    /// Whether the character sheet is up over whatever else is on screen.
    sheet: bool,
    /// Everything the traveller's token overlaps, rebuilt every frame, which is
    /// the five-entry stack at `DS:043c`.
    overlaps: Overlaps,
    /// Whether `_MAP:CreatePaper`'s panel is up, waiting on a number key.
    paper: bool,
    visiting: Option<place::PlaceScene>,
    mode: Mode,
    audio: Box<dyn Sink>,
    /// `--sounds`: say which sample each `TASKSOUND` fires and which script
    /// asked, since a sound cannot be checked by looking at a screenshot.
    trace_sounds: bool,
    fonts: std::collections::BTreeMap<String, Font>,
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
    /// **Ours**: the lobby screen, while it is up. See [`online`].
    online: Option<online::Online>,
    /// **Ours**: the socket a lobby is held open on, before a game starts.
    lobby: Option<Waiting>,
    /// **Ours**: the game running across machines, once one is.
    net: Option<henge_net::Session>,
    /// **Ours**: the router being asked to open the port, while it is being
    /// asked. It answers on a thread of its own because SSDP waits two seconds
    /// and the lobby has to be drawable before then.
    opener: Option<henge_net::Opener>,
    /// **Ours**: the mapping the router gave, so it can be taken down again.
    mapping: Option<henge_net::Mapping>,
    /// **Ours**: last tick's word for every seat, which is what turns a held
    /// word into a press. The original does the same with `BOUNCEBUTTON`.
    seat_was: [henge_net::SeatInput; henge_core::shell::SEATS],
    /// **Ours**: this tick's presses for every seat, worked out in
    /// [`App::apply_turn`] where last tick's word is still to hand.
    ///
    /// [`App::pressed`] holds the same edges for the four seats' five controls,
    /// because [`slot_of`] gives all four a slot. The three keys that are not
    /// controls have one slot between them, the one this keyboard writes, so a
    /// screen whose turn belongs to a seat that is not this one cannot read them
    /// there. `ChooseKnight` is that screen, and this is where it reads them.
    seat_pressed: [SeatPress; henge_core::shell::SEATS],
    /// **Ours**: what this keyboard is actually doing, before the wire has had
    /// its say.
    ///
    /// [`App::keys`] is what the simulation reads, and in a lockstep game it is
    /// overwritten every tick with what came off the wire, including seat zero's
    /// when somebody else is in seat zero. So it cannot also be the thing that
    /// *goes* onto the wire: this is, and it is written by the key handler and by
    /// the pads and by nothing else.
    raw: [bool; 256],
    raw_pressed: [bool; 256],
    raw_typed: Option<char>,
    /// **Ours**: `--begin` with `--players n`: start as soon as that many are in
    /// the lobby and all of them are ready. For a scripted host, which has
    /// nobody to press the row.
    auto_begin: Option<usize>,
    /// **Ours**: whether [`App::select_auto`]'s key is down, so that a scripted
    /// run presses and lets go rather than holding.
    select_held: bool,
    /// **Ours**: whether a script is at the keyboard rather than a person.
    ///
    /// `--trace`, `--screenshot` and `--input` drive [`App::keys`] directly,
    /// because that is what the simulation reads and they were written long
    /// before there was a wire. So in those runs, and only those, `keys` is
    /// copied into [`App::raw`] at the top of the tick: the script's word is then
    /// what goes onto the wire, and an online game can be driven headlessly.
    driven: bool,
    /// Ticks since the run ended. Only a debounce: `WaitFIRE` at 0x8251 waits
    /// for a press and then for a release, so the press that ended the run
    /// cannot also clear the message it put up.
    run_over_for: u32,
    /// The character `ASCIIKEY` gave for whatever was pressed this tick, for the
    /// one screen in the game that reads letters.
    typed: Option<char>,
    /// What each player typed over their knight's name, in the order they chose.
    named: Vec<(usize, String)>,
    /// Aloft on the gem or the hawk. Desktop state rather than the run's,
    /// like the map position it belongs with: `GemXY` in the original is
    /// beside the token, not on the knight record.
    flight: Option<Flight>,
    /// One of a town's five doors standing open: `HWLOOP`'s rung is running
    /// and the town is what it comes back to through `HWINIT`. Each is a loop
    /// of its own in the original and modal here for the same reason.
    door: Option<town::Open>,
    /// Which of the bowl's four gadgets the keys have stepped to. The original
    /// has only the pointer; a keyboard needs a way to reach four boxes.
    door_cursor: usize,
    /// `AddDonation`'s and `SubDonation`'s two retraces, counted down before
    /// a held fire takes the next coin.
    door_wait: u32,
    /// What the gadget under the pointer says on the character sheet, which is
    /// `GadgetHit` drawing `Response1[STID >> 1]` across the top of it. The
    /// sheet has no cursor and no rows: see [`status`].
    sheet_said: Option<String>,
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
    /// while it is there. Non-zero is up; it waits on fire rather than counting
    /// down, which is what `WaitFIRE` at 0x8251 does.
    interlude: u32,
    /// `FADEOUTDAY`, the fade the between-days screen goes out on, counted down
    /// once fire has been pressed: `NextWHICH` calls the screen, then `WaitFIRE`,
    /// then the fade out at 0x5b65, and only then is the map back.
    interlude_out: u32,
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
    /// Zero is "waiting on fire" for a chain whose caller waits, which is
    /// `WaitFIRE` at 0x8251 and has no timer in it; see `show_message`.
    showing: Option<Message>,
    showing_for: u32,
    /// The intro sequence.
    intro: Intro,
    /// The intro's own cast, out of the pack. Absent when the pack was baked
    /// without an unpacked `INTR.EXE`, in which case the plates simply hold.
    intro_cast: Option<std::rc::Rc<henge_core::content::IntroCast>>,
    /// The ending, which is the same executable run with an argument.
    ending: Ending,
    /// The ending's own cast. The same scripts read through a different bank
    /// table, so it is a second entry in the pack rather than a re-use.
    ending_cast: Option<std::rc::Rc<henge_core::content::IntroCast>>,
    /// Whether `0x50d`'s three glows are in, which `0x3531` gosubs part way
    /// through the ceremony and `0x547` takes out again at the end of it.
    ending_glows: bool,
    /// The lair page, while it is up. `MOON` image 0x05c3 `LairGEM`: the panel
    /// opened on `StatTYPE` 2, which is the only place a lair's floor is
    /// handed over, and the only place a gem flight ends.
    lair_page: Option<henge_core::lair::Page>,
    /// The stone circle's set piece, while `HengeLOOP` is running it. Modal,
    /// like everything else the original puts up and sits in a loop over.
    stones: Option<Stones>,
    /// The circle's own one-bank table, `DiceHANDLE` with `Hen1.c` in it.
    stones_banks: Option<henge_core::taskvm::BankTables>,
    /// `DrBuffer`, the five slots `ContinueDragon` (0xa5cd) points at `MI.C`:
    /// the dragon's bank table 5, which its eight flight scripts draw from.
    dragon_banks: Option<henge_core::taskvm::BankTables>,
    /// The fight on is the dragon's, which the routine at 0xcf3 answers for
    /// on its way out: `_dragon_won` or the `0xffff` into the dragon's `+0x31`.
    dragon_fight: bool,
    /// The fight on is a knight against knight: `[0x8979]` and `[0x897b]` as
    /// `Combat+115` (0x3c4) wrote them, the attacker first.
    duel: Option<(usize, usize)>,
    /// How the last knight fight settled, kept from the tick it settled on
    /// to the tick the bout ends on, when `Knight1Won` and the rest run.
    duel_settled: Option<henge_core::rival::Settled>,
    /// The trade page, `StatTYPE` 1, while it is up: the loser's record and
    /// `TakeCNT`. `ReDisplay+0x73` (0xbeda) reads `[StatHAND2+0x31]`, the
    /// loser's own lives, before `TakeCNT`: a dead loser (a grave) has it
    /// forced back to nought every redraw (0xbee0), so a grave can be taken
    /// from more than once; a living one does not, so the `cmp [TakeCNT], 0`
    /// at 0xbee6 finds it set the moment one thing has been taken and turns
    /// the page into the winner's own plain sheet (`StatTYPE = 9`, 0xbeed),
    /// for good. See `sheet_tick`, where a successful take closes this the
    /// same way for a living loser and keeps it open for a dead one.
    trade_page: Option<(usize, bool)>,
    /// The dragon's hoard, `StatTYPE` 0xa, up once the dragon is dead for
    /// good. The routine at 0xcf3/0xcf6 (`DragonEncounter+54`, 0xa41b, calls
    /// it) settles the bout the same way [`Self::knight_fight_settled`]'s
    /// caller does; its own `_dragon_won` branch either leaves the dragon
    /// flying (`0xd23`, a life point and one thing taken, already
    /// `Rival::dragon_on_rival`'s shape for a computer knight) or, when the
    /// bout the player just won killed it for good, grounds it
    /// (`0xd35` to `0xd50`, [`henge_core::dragon::Flight::fight_over`]) and
    /// opens this with `0xd44 mov ax, 0xa; call 0xbdd3`, exactly as
    /// `Knight1Won+6` (0x46b) opens the trade page on `StatTYPE` 1. No
    /// `TakeCNT` of its own the trade page has one for, so `EXIT` alone
    /// closes it; see `sheet_tick`.
    dragon_page: bool,
    /// The Scroll of the Wyrm's own knight picker, `StatTYPE` 0xb
    /// (`Screen::AcquirePair`), while it is up: the seat currently
    /// highlighted. `InitAKnight`/`NextKnight` (0xc90d/0xc913) pick and step
    /// it, skipping the caster's own; `close_wyrm_picker` is `StatusDone`
    /// (0xbe57), run when `EXIT` closes the page. Stands in for `WyrmFLAG`
    /// (DS:`0xf37a`): `Some` here is the flag up, and both come down
    /// together.
    wyrm_picker: Option<usize>,
    /// The dice table's own one-bank table, `dice.cel` in `DiceHANDLE`.
    dice_banks: Option<henge_core::taskvm::BankTables>,
    /// Every animation script, which the circle's two tasks run on. The bout
    /// has its own copy; this is the shell's.
    scripts: henge_core::taskvm::ScriptSet,
    /// Where the harness's snapshot is written and read. Relative to wherever the
    /// game is run from unless `--save` says otherwise, and never written into it
    /// itself.
    snapshot_path: String,
    status: String,
    #[cfg(feature = "research")]
    research: Option<research::Viewer>,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum Mode {
    Intro,
    Ending,
    Title,
    Select,
    Map,
    Combat,
    Place,
    /// Ours: the lobby, which is the only screen in the game that is not the
    /// original's. See [`online`] and `henge_net`.
    Online,
}

/// **Ours**: the socket a lobby is held open on before a game begins. Once it
/// does, it becomes a `henge_net::Session` and this is empty again.
///
/// A host carries a listener, a roster and possibly a list-server connection and
/// a guest carries one socket, so the two halves are nothing like the same size.
/// There is one of these per running game, so boxing it would save a few hundred
/// bytes once and cost a pointer chase every tick.
#[allow(clippy::large_enum_variant)]
enum Waiting {
    Host(henge_net::Host),
    Guest(henge_net::Guest),
}

/// The value after a flag, for the handful of arguments that take one.
fn after_arg(a: &[String], flag: &str) -> Option<String> {
    let at = a.iter().position(|x| x == flag)?;
    a.get(at + 1).filter(|v| !v.starts_with("--")).cloned()
}

/// **Ours.** Where the game looks for a list server when nothing else says.
///
/// Ours in every sense: this is Fjord3D's own machine, an `e2-micro` in Google's
/// `europe-north2` (Stockholm), which is the nearest region to the people
/// playing. It holds the list of open games and carries the ones whose hosts
/// cannot be reached directly, which behind Starlink's carrier NAT is all of
/// them. See `docs/list-server.md`.
///
/// **The address is ephemeral**, which is what keeps it free while the machine
/// is off: it survives a running machine indefinitely and changes when the
/// machine is stopped and started again. When that happens, either put the new
/// one in [`LIST_FILE`] or change this line; nothing else in the game knows the
/// number.
///
/// Empty means there is no list server, and the browse page says so rather than
/// pretending to look. It is only the default: [`list_server`] prefers
/// `--list <address>` and then the one line in [`LIST_FILE`].
const LIST_SERVER: &str = "34.51.244.53";

/// A file beside the game holding one line: the list server's address. Written
/// by hand, read at every look, and absent by default.
const LIST_FILE: &str = "henge-list.txt";

/// Where to look for open games: the flag, then the file, then [`LIST_SERVER`].
fn list_server(a: &[String]) -> String {
    if let Some(v) = after_arg(a, "--list") {
        return v;
    }
    if let Ok(text) = std::fs::read_to_string(LIST_FILE) {
        let line = text
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty() && !l.starts_with('#'))
            .unwrap_or_default();
        if !line.is_empty() {
            return line.to_string();
        }
    }
    LIST_SERVER.to_string()
}

/// What a build calls itself on the list, so a browser is not offered a game its
/// own copy of the game cannot join. The crate version and the game protocol
/// together: either changing is a reason not to sit down at the same table.
fn build_name() -> String {
    format!("{}+{}", env!("CARGO_PKG_VERSION"), henge_net::PROTOCOL)
}

/// `--port <n>`: which port to host on. The default is `henge_net`'s own.
fn online_port_arg(args: &[String]) -> Option<u16> {
    let at = args.iter().position(|a| a == "--port")?;
    args.get(at + 1)?.parse().ok()
}

/// `--delay <ticks>`: the input delay to start a game with, for testing one end
/// of a bad line. Without it the host picks from a round trip it assumes.
fn online_delay_arg(args: &[String]) -> Option<u32> {
    let at = args.iter().position(|a| a == "--delay")?;
    args.get(at + 1)?.parse().ok()
}

/// A token's `(x, y, colour or frame)`, as [`map::Marks`] draws one: `SHOW`
/// (0xa1f0) for the record whose turn it is, `DisplayOtherKnights` (0xa22c)
/// for the rest.
type KnightMark = (i32, i32, usize);

/// A flight over the map, `EffectFLAG+2` and `+4` in `_MAP`: the gem's comes
/// back to where it began, the hawk's lands where it is when fire is pressed.
#[derive(Clone, Copy, Debug)]
struct Flight {
    returns: bool,
    from: (i32, i32),
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
    println!(
        "audio: built without a backend; {loaded} clips and {} tunes go unheard",
        tunes.len()
    );
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
        // Seats two and three have no keys of their own: the original's third
        // and fourth players are on the game port, and there is one keyboard.
        // They are here because a lockstep game writes every seat's word into
        // these slots, so all four need somewhere to land. They are clear of the
        // ten above, of the nine number keys at [`NUMBER_SLOT`] and of
        // [`CALIBRATE_SLOT`], so nothing a person can press reaches them.
        (2, Up) => 30,
        (2, Down) => 31,
        (2, Left) => 32,
        (2, Right) => 33,
        (2, Fire) => 34,
        (3, Up) => 35,
        (3, Down) => 36,
        (3, Left) => 37,
        (3, Right) => 38,
        (3, Fire) => 39,
        _ => 255,
    }
}

/// **Ours**: one seat's word turned into the slots the simulation reads, with
/// every press worked out as a rising edge against the tick before.
///
/// Pure, and tested, because this is the one place a lockstep game can go wrong
/// without the check noticing: two machines that derived presses differently
/// would diverge, and the whole reason the wire carries held words rather than
/// presses is that this function is the same function on every machine.
///
/// `shared` says whether this seat is the one that fills the slots the four
/// seats have between them: Enter, backspace and the nine number keys, which are
/// `ScanKEYS` and `DisplayStack` and which the original reads off its one
/// keyboard. Every machine has to pass it for the same seat or the machines come
/// apart, so [`App::apply_turn`] passes it for seat zero and says why.
/// **Ours**: one seat's presses for one tick, for the keys that are not
/// controls and so have no per-seat slot.
///
/// Left and right are here as well, because a screen reading this is reading one
/// seat's whole word and should not have to take half of it from somewhere else.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct SeatPress {
    left: bool,
    right: bool,
    /// Fire or Enter, which every menu in this build takes as the same press.
    take: bool,
    /// Backspace, which `ScanKEYS` tests before `ASCIIKEY` is ever called.
    back: bool,
    typed: Option<char>,
}

fn seat_slots(
    seat: usize,
    now: henge_net::SeatInput,
    was: henge_net::SeatInput,
    shared: bool,
) -> Vec<(usize, bool, bool)> {
    let mut out = Vec::with_capacity(16);
    for a in input::Action::ALL {
        let slot = slot_of(seat, a);
        if slot >= 256 {
            continue;
        }
        let on = now.pad & a.bit() != 0;
        out.push((slot, on, on && (was.pad & a.bit() == 0)));
    }
    if shared {
        out.push((ENTER_SLOT, now.take(), now.take() && !was.take()));
        out.push((BACKSPACE_SLOT, now.back(), now.back() && !was.back()));
        for n in 1..=9u8 {
            let on = now.number == Some(n);
            out.push((
                NUMBER_SLOT + n as usize - 1,
                on,
                on && was.number != Some(n),
            ));
        }
    }
    out
}

/// Enter. `ScanKEYS` tests scancode 0x1c as the end of a name, and every menu in
/// this build takes it as well as fire; it is not a control, so it is not in the
/// bindings table and has a slot of its own here.
const ENTER_SLOT: usize = 12;

/// The keys that are not controls, so are not in the bindings table.
///
/// The ten seat slots are gone from here: they come out of `input::Bindings`
/// now, so that changing them is editing a file rather than editing this match.
/// Where the nine number keys start. `_MAP:DisplayStack`'s reader takes scan
/// codes 2 to 0x0a, which are the top row's `1` to `9`, so the nine of them are
/// a block of slots of their own, clear of the ten the seats own and of the
/// developer keys beside them.
const NUMBER_SLOT: usize = 16;

/// `ScanKEYS` at 0x1447: `cmp ax, 0xe; je BACKSPACE`, which is the scancode of
/// the backspace key, tested before `ASCIIKEY` is ever called.
const BACKSPACE_SLOT: usize = 15;

/// `_STATUS:AddClickSound` (image `0xd508`), the click a gadget makes when it is
/// taken.
///
/// The routine is two instructions, `mov al, 0x0f` and into the sample
/// dispatcher at `0x5964`, and seventeen gadget handlers call it: `HotGadget` at
/// `0xca11` and `0xca23` for the exit and the next knight, and then
/// `HGCastMagic`, `MagicCast`, `HGTakeMagic`, `HGAbility`, `TKAR`, `TKWP`,
/// `TKGP`, `BuyArmour` three times, `BuyWeapon` twice, `BuyDagger`, `TTemple`
/// and `SellToTemple`. All seventeen are in `_STATUS`, which is the status
/// screen, the shops and the temple: the screens that are gadget lists.
///
/// **Which sound it is, is recovered rather than picked.** The dispatcher at
/// `0x5964` translates the sample number through `samptab` (`DS:0x7e6e`, image
/// `0x1a21e`) when the card is a digital one, and entry 0x0f of that table is
/// 0x15. The sample name table at `DS:0x8783` (image `0x1ab33`) is the
/// forty-nine names in order, and the twenty-second of them is `hit3`. The same
/// table read at 0x0b, which is the knight's `KnightGruntSound`, gives 0x2e and
/// the forty-seventh name, `swish`, which is what this project already plays for
/// a swing: the chain checks out against something chosen independently of it.
const CLICK_SOUND: &str = "sfx.hit3";

/// `J`, which is how the options screen reaches the stick calibration.
///
/// `OptionKeys` at image `0x1282` is the options screen's key loop, and the
/// first thing it does every pass is `mov ax, 0x24` into `KEYPRESSED` and, if
/// that key is down, `jmp` to `Fix_JoyStick` at `0x128a`. Scancode 0x24 is `J`.
/// `Fix_JoyStick` ends at `0x59d7` with a `jmp` back into the options screen, so
/// calibrating is a thing done from the menu and returned from. Slot 25, clear of
/// the ten the seats own and of the nine number keys at [`NUMBER_SLOT`].
const CALIBRATE_SLOT: usize = 25;

fn key_index(c: KeyCode) -> usize {
    match c {
        KeyCode::BracketLeft => 4,
        KeyCode::BracketRight => 5,
        KeyCode::Backspace => BACKSPACE_SLOT,
        // Scancode 0x24, which `OptionKeys` tests first of all.
        KeyCode::KeyJ => CALIBRATE_SLOT,
        // The number keys the map's paper is answered with.
        KeyCode::Digit1 => NUMBER_SLOT,
        KeyCode::Digit2 => NUMBER_SLOT + 1,
        KeyCode::Digit3 => NUMBER_SLOT + 2,
        KeyCode::Digit4 => NUMBER_SLOT + 3,
        KeyCode::Digit5 => NUMBER_SLOT + 4,
        KeyCode::Digit6 => NUMBER_SLOT + 5,
        KeyCode::Digit7 => NUMBER_SLOT + 6,
        KeyCode::Digit8 => NUMBER_SLOT + 7,
        KeyCode::Digit9 => NUMBER_SLOT + 8,
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

/// `ASCIIKEY` at image 0x14e8, which is `mov bx, ASCIIT; xlatb`: the scancode to
/// character table the name typing reads its letters through.
///
/// `ASCIIT` at image 0x14f2 is that table, and these are its own entries, in its
/// own scancode order. It is uppercase throughout, it has no shift and it maps
/// **the space bar to 0x5f**, the underscore, which `TextASCII` draws as the
/// blank: a space typed into a name is an underscore, which is why the default
/// names carry one. A scancode with a zero entry types nothing.
///
/// The entries for 0x0e and 0x1c, backspace and Enter, are never reached:
/// `ScanKEYS` tests both before it calls this.
// Hand-aligned: the arms are grouped into the keyboard's own rows, which is the
// shape of the scancode table this reproduces.
#[rustfmt::skip]
fn typed_char(c: KeyCode) -> Option<char> {
    use KeyCode::*;
    Some(match c {
        Digit1 => '1', Digit2 => '2', Digit3 => '3', Digit4 => '4', Digit5 => '5',
        Digit6 => '6', Digit7 => '7', Digit8 => '8', Digit9 => '9', Digit0 => '0',
        Minus => '-', Equal => '=',
        KeyQ => 'Q', KeyW => 'W', KeyE => 'E', KeyR => 'R', KeyT => 'T',
        KeyY => 'Y', KeyU => 'U', KeyI => 'I', KeyO => 'O', KeyP => 'P',
        BracketLeft => '[', BracketRight => ']',
        KeyA => 'A', KeyS => 'S', KeyD => 'D', KeyF => 'F', KeyG => 'G',
        KeyH => 'H', KeyJ => 'J', KeyK => 'K', KeyL => 'L',
        Semicolon => ';', Quote => '\'', Backslash => '\\',
        KeyZ => 'Z', KeyX => 'X', KeyC => 'C', KeyV => 'V', KeyB => 'B',
        KeyN => 'N', KeyM => 'M',
        Comma => ',', Period => '.', Slash => '/',
        // Scancode 0x39, and the table's entry for it is 0x5f.
        Space => '_',
        _ => return None,
    })
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
            Err(e) => {
                eprintln!("no overworld: {e:#}");
                None
            }
        };
        let world = match World::load(&reg) {
            Ok(w) => Some(w),
            Err(e) => {
                eprintln!("arena load failed: {e:#}");
                None
            }
        };
        let audio = open_audio(&mut reg);
        let fonts = text::load(&reg);
        let places = place::load(&reg);
        let items = place::load_items(&reg);
        // A pack without knights is not an error; the title simply cannot offer
        // a quest, exactly as it could not before.
        let knights: Knights = reg.read_data("data.knights").unwrap_or_default();
        // The rebinding table is ours and it is data: a file, read if it is
        // there, and otherwise the original's own ten keys. There is no flag to
        // ask for those any more, because they are what it ships with.
        let controls_path = controls_path_arg(&args_of());
        let args = args_of();
        let mut bindings = input::Bindings::load(&controls_path).unwrap_or_default();
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
            println!("the original's keys: p1 enter + arrows, p2 tab + w x a d.");
            println!("menus: move with the direction keys, take with fire.");
            println!("J on the title screen calibrates a stick, escape there quits.");
            println!("ours: 1/2 set how many are playing, C the sheet, F2 switches");
            println!("map/arena, [ and ] change arena, , and . change the opponent, R restarts,");
            println!("- and = slow the game down and speed it up (--pace <percent> too);");
            println!("it opens at 100%, the rate the image says, which is a floor and not a rate.");
        }

        let intro_cast = reg.read_data("data.intro").ok().map(std::rc::Rc::new);
        let ending_cast = reg.read_data("data.ending").ok().map(std::rc::Rc::new);
        // The stone circle runs two of the game's own animation scripts on a
        // one-bank table, which the baker writes beside every creature's.
        let scripts: henge_core::taskvm::ScriptSet =
            reg.read_data("data.scripts").unwrap_or_default();
        let dice_banks = reg
            .read_data::<std::collections::BTreeMap<String, henge_core::taskvm::BankTables>>(
                "data.banks",
            )
            .ok()
            .and_then(|mut b| b.remove(henge_core::dice::BANKS));
        let stones_banks = reg
            .read_data::<std::collections::BTreeMap<String, henge_core::taskvm::BankTables>>(
                "data.banks",
            )
            .ok()
            .and_then(|mut b| b.remove(henge_core::stones::BANKS));
        let dragon_banks = reg
            .read_data::<std::collections::BTreeMap<String, henge_core::taskvm::BankTables>>(
                "data.banks",
            )
            .ok()
            .and_then(|mut b| b.remove("dragon"));
        Ok(App {
            fb,
            fx: henge_assets::Effects::new(),
            fx_table,
            knight_glows: std::collections::BTreeMap::new(),
            scene: String::new(),
            music_places,
            tick: 0,
            shake: 0,
            shake_rows: 0,
            // Any seed: nothing reads the result back into the fight.
            shake_rng: 0x2f1d,
            pace: PACE_DEFAULT,
            keys: [false; 256],
            pressed: [false; 256],
            reg,
            world,
            mode: if map.is_some() {
                Mode::Map
            } else {
                Mode::Combat
            },
            map,
            places,
            items,
            knights,
            title: shell::TitleScene::default(),
            select: None,
            sheet: false,
            overlaps: Overlaps::default(),
            paper: false,
            visiting: None,
            audio,
            trace_sounds: args_of().iter().any(|a| a == "--sounds"),
            fonts,
            bindings,
            controls_path,
            kb: [false; 256],
            pad_held: [false; 256],
            bounce: [input::Debounce::default(); 2],
            calibrating: None,
            run: Run::new(100),
            online: None,
            lobby: None,
            net: None,
            opener: None,
            mapping: None,
            seat_was: [henge_net::SeatInput::default(); henge_core::shell::SEATS],
            seat_pressed: [SeatPress::default(); henge_core::shell::SEATS],
            raw: [false; 256],
            raw_pressed: [false; 256],
            raw_typed: None,
            auto_begin: None,
            select_held: false,
            driven: args_of().iter().any(|a| {
                matches!(
                    a.as_str(),
                    "--trace" | "--screenshot" | "--input" | "--goto" | "--walk" | "--fight"
                )
            }),
            run_over_for: 0,
            typed: None,
            named: Vec::new(),
            flight: None,
            door: None,
            door_cursor: 0,
            door_wait: 0,
            sheet_said: None,
            raiding: None,
            questing: false,
            raid_place: String::new(),
            interlude: 0,
            interlude_out: 0,
            hint: 0,
            practice: false,
            pointer: Pointer::centred(),
            gadgets: Gadgets::default(),
            messages: Messages::recovered(),
            showing: None,
            showing_for: 0,
            intro: Intro::new(),
            intro_cast,
            // The seat the ending opens on is the one a run's tally names; the
            // byte is written in when the run is actually won. `--start ending`
            // wants something to look at, and seat 0 on the full moon is the
            // byte `Tally::code` produces there.
            ending: Ending::new(0x24),
            ending_cast,
            ending_glows: false,
            lair_page: None,
            duel: None,
            duel_settled: None,
            trade_page: None,
            dragon_page: false,
            wyrm_picker: None,
            stones: None,
            stones_banks,
            dragon_banks,
            dragon_fight: false,
            dice_banks,
            scripts,
            snapshot_path: snapshot_path_arg(&args_of()),
            status,
            #[cfg(feature = "research")]
            research: research::Viewer::from_args()?,
        })
    }

    /// Space or Enter takes the highlighted option. Both, everywhere a menu
    /// asks, so there is never a screen where one of them silently does
    /// nothing.
    fn takes(&self) -> bool {
        self.pressed[6] || self.pressed[ENTER_SLOT]
    }

    fn key(&mut self, code: KeyCode, down: bool) {
        // There is no save key and no load key. The original has no save at all,
        // so a player cannot reach one here either; the serialisation that used
        // to be behind F5 and F9 is now the test harness's alone and is reached
        // only from the command line. See `henge_core::harness`.
        //
        // The seats' ten slots come out of the binding table; everything else
        // is a developer key and stays where it is.
        let named = input::Source::key(&format!("{code:?}"));
        for (seat, action) in self.bindings.raised_by(&named) {
            let s = slot_of(seat, action);
            if s < 256 {
                if down && !self.keys[s] {
                    self.pressed[s] = true;
                }
                if down && !self.raw[s] {
                    self.raw_pressed[s] = true;
                }
                self.kb[s] = down;
                self.keys[s] = self.kb[s] || self.pad_held[s];
                self.raw[s] = self.keys[s];
            }
        }
        let i = key_index(code);
        if i < 256 && down && !self.keys[i] {
            self.pressed[i] = true;
        }
        if i < 256 {
            if down && !self.raw[i] {
                self.raw_pressed[i] = true;
            }
            self.raw[i] = down;
        }
        // `ASCIIKEY`, for `TypeName`. A key that the table has no character for
        // types nothing, and the screens that do not read letters never look.
        if down {
            if let Some(c) = typed_char(code) {
                self.typed = Some(c);
                self.raw_typed = Some(c);
            }
        }
        // A key the binding table claims is a control, and a control is never
        // also a developer key: the original's ten are in that table now, and
        // player two pressing fire must not flip the screen out from under the
        // fight. This is also what keeps a rebinding safe.
        let is_control = !self.bindings.raised_by(&named).is_empty();
        // The pace dial, which is ours and not the original's: see
        // [`App::pace`]. Outside the world borrow because it belongs to every
        // screen, not only to a fight.
        if down && !is_control && matches!(code, KeyCode::Minus | KeyCode::Equal) {
            let step = if code == KeyCode::Minus {
                self.pace.saturating_sub(PACE_STEP)
            } else {
                self.pace + PACE_STEP
            };
            self.pace = step.clamp(PACE_MIN, PACE_MAX);
            let ms = self.tick_len().as_secs_f64() * 1000.0 * ticks_per_pass(self.mode) as f64;
            self.notice(format!(
                "speed {}%  ({:.1} ms a frame, the original's is {:.1})",
                self.pace,
                ms,
                ms * self.pace as f64 / PACE_FULL as f64
            ));
        }
        if down && !is_control {
            if let Some(world) = self.world.as_mut() {
                match code {
                    KeyCode::BracketLeft => world.step_arena(-1),
                    KeyCode::BracketRight => world.step_arena(1),
                    // Comma and period cycle which creature fills the
                    // opponents' seats, so each of the bestiary can be looked
                    // at in the arena browser.
                    KeyCode::Comma => world.step_foe(-1),
                    KeyCode::Period => world.step_foe(1),
                    KeyCode::KeyR => world.reset(),
                    // 1 and 2 set how many people are at the keyboard; the rest
                    // of the four seats are filled by opponents.
                    KeyCode::Digit1 => world.set_players(1),
                    KeyCode::Digit2 => world.set_players(2),
                    // F2 flips between the overworld and the arena, which is how
                    // the arena browser stays reachable. It used to be Tab, which
                    // the original spends on player two's fire.
                    KeyCode::F2 => {
                        self.mode = match self.mode {
                            Mode::Map => Mode::Combat,
                            Mode::Combat => Mode::Map,
                            // It is also the way out of a place, so a broken
                            // menu can never trap you indoors.
                            Mode::Place => {
                                self.visiting = None;
                                Mode::Map
                            }
                            // And the way out of the shell, so a pack with no
                            // knights in it cannot strand you on a select screen
                            // with nothing to select.
                            Mode::Select => {
                                self.select = None;
                                Mode::Title
                            }
                            // Out of the intro too, so the sequence can never
                            // hold a player who wants to play.
                            Mode::Intro => {
                                self.intro.skip();
                                Mode::Title
                            }
                            // And out of the ending, which the original's own
                            // scene loop also lets fire cut short.
                            Mode::Ending => {
                                self.ending.skip();
                                Mode::Title
                            }
                            Mode::Title => Mode::Map,
                            // And out of the lobby, which has a socket to close
                            // on the way.
                            Mode::Online => {
                                self.leave_online("left the lobby");
                                Mode::Title
                            }
                        };
                    }
                    // The character sheet, over whatever is on screen.
                    KeyCode::KeyC => self.sheet = !self.sheet,
                    _ => {}
                }
            }
        }
        if i < 256 {
            self.keys[i] = down;
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
                if on && !self.raw[s] {
                    self.raw_pressed[s] = true;
                }
                self.pad_held[s] = on;
                self.keys[s] = self.kb[s] || on;
                self.raw[s] = self.keys[s];
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

    /// Whether Escape means quit, which it does on the options screen and
    /// nowhere else.
    ///
    /// `OptionKeys` (`0x1282`) tests scancode 0x01 at `0x128d` and returns, and
    /// `StartAgain` (`0x00a8`) tests it again at `0x00ba` and returns out of the
    /// program. No other screen in the original reads it, so no other screen
    /// here does either.
    fn quits_on_escape(&self) -> bool {
        self.mode == Mode::Title
    }

    /// Starts `Fix_JoyStick` for one seat, and says what it wants.
    fn calibrate(&mut self, seat: usize) {
        self.bounce[seat.min(1)].clear();
        let state = input::Calibrating::TopLeft;
        self.calibrating = Some((seat, state));
        self.notice(state.prompt().join(" "));
    }

    /// How long one tick of the loop that is up lasts.
    ///
    /// The original's loops do not share a clock, and each one's own wait says
    /// which it is on: see [`TIMER_TICK`] and [`RETRACE_TICK`] for the wait each
    /// of them makes and where it is.
    ///
    /// `Combat` (`0x351`) is the one loop that waits on a deadline in the BIOS
    /// tick counter, so the arena and only the arena runs on the timer. The map
    /// (`MapLOOP`), the knight select (`ChooseLoop`), the stone circle
    /// (`HengeLOOP`), name entry (`ScanKEYS`) and the whole of `INTR.EXE`'s intro
    /// and ending wait on vertical retraces, so they stay on the retrace.
    ///
    // TODO: `TavernLoop` (0xb137), `StatLOOP` (0xbe13), `DonateLoop` (0xbbe6),
    // `WDLOOP` (0xd7a), `HWLOOP` (0xe35) and `DoOptions`/`OptionKeys`
    // (0x1241/0x1282) make no wait of any kind: they blit, page-flip and go
    // round again at whatever speed the machine manages, so the original has no
    // rate here to recover. They are left on the retrace, which is the rate
    // every other non-combat loop in the image does name, rather than given a
    // number nothing in the image supports. The pointer on those screens is
    // rate-limited by its own acceleration table (`_STATUS:StatACEL`) and not by
    // a frame wait, which is presumably why they needed none.
    fn tick_len(&self) -> std::time::Duration {
        // A panel up is `StatLOOP` (0xbe13) running in place of whatever is
        // underneath, and that loop makes no wait at all. So a panel opened
        // over the map does not inherit the map's own doubled pass: the
        // original's panel was the fastest loop in the game, not the
        // slowest, and halving the pointer under a sheet would be ours and
        // not the original's.
        let base = if self.panel_now().is_some() {
            RETRACE_TICK
        } else {
            tick_len_for(self.mode)
        };
        // [`App::pace`], which is ours: a hundred leaves the recovered tick
        // exactly as it stands.
        if self.pace == PACE_FULL {
            return base;
        }
        base * PACE_FULL / self.pace.max(1)
    }

    /// One tick. A key press is an edge: it lasts exactly this tick and is
    /// spent whether or not anything wanted it.
    fn update(&mut self) {
        // `ShakeScreen` (0x495b) is called from inside `COLCON`, which is
        // `Combat`'s second call (0x357), and it does not return for fifteen
        // retraces: the task step, the blit, the page flip and the collision
        // pass at 0x35a to 0x366 all wait for it. So the fight really does
        // stop dead while the screen shudders. The count is fixed, so every
        // peer of a lockstep fight stops for the same fifteen ticks.
        if self.shake > 0 {
            self.palette_tick();
            self.pressed = [false; 256];
            self.raw_pressed = [false; 256];
            self.typed = None;
            self.raw_typed = None;
            return;
        }
        // A scripted run pokes `keys` rather than pressing anything, so that is
        // where its word for the wire has to come from. See [`App::driven`].
        if self.driven {
            self.select_auto();
            self.raw = self.keys;
            self.raw_pressed = self.pressed;
            self.raw_typed = self.typed;
        }
        // **Ours**: a game running across machines does not tick until every
        // seat's word for this tick has arrived. See `henge_net::lockstep`.
        // Nothing below this line knows the difference.
        if self.net.is_some() && !self.net_step() {
            self.palette_tick();
            self.pressed = [false; 256];
            self.raw_pressed = [false; 256];
            self.typed = None;
            self.raw_typed = None;
            return;
        }
        self.simulate();
        self.palette_tick();
        self.music_tick();
        self.pressed = [false; 256];
        self.raw_pressed = [false; 256];
        self.typed = None;
        self.raw_typed = None;
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
        // The circle brings its own picture and its own palette, and `0xb3ea`
        // fades up to it.
        if self.stones.is_some() {
            return "stones".into();
        }
        match self.mode {
            // Each step of the intro is its own screen, because the original
            // fades between them: every scene routine calls the fade out, puts
            // its plate up and fades back in.
            Mode::Intro => format!("intro.{}", self.intro.card),
            // And every scene of the ending, for the same reason: `0x547`,
            // `0x628`, `0x6db` and `0x778` all open with `0xc02` and close on a
            // fade.
            Mode::Ending => format!("ending.{}", self.ending.card),
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
            // The lobby is drawn over the title's own plate, so it keeps the
            // title's palette and does not fade when it opens.
            Mode::Online => "title".into(),
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
            // `install` emptied the glow table, and the knight's went with it,
            // which is `KnightGlowOff` at the end of every bout.
            self.knight_glows.clear();
            // `0x547` zeroes the three handles `0x50d` left in `[0x13b1]`,
            // `[0x13b3]` and `[0x13b5]` when its first scene ends, and a new
            // scene is where that happens.
            self.ending_glows = false;
            self.fx.set_fade(henge_assets::Fade::In(0));
        }
        // `COLCON` (0x4988) and `KnightGlowOn` (0x8f8) are the calling loop's
        // own per-pass work, not per-tick work, and in a fight a pass is six
        // ticks rather than one. `Combat` (0x351) calls both, at 0x357 and
        // 0x369; `MapLOOP+3` (0xa309) and `ChooseLoop+3` (0x15a3) call
        // `COLCON` on a pass that is one retrace, which is one tick here, and
        // a scan of the image finds no other caller of either. Ungated, a
        // fight ran every colour cycle and the dying knight's own breathing
        // six times too fast.
        //
        // And they are the calling loop's work, so a screen whose loop does
        // not call them does not get them. `StatLOOP` (0xbe13) is
        // `MovePointer`, the blit, the page flip, `HotGadget` and back to the
        // top: it calls neither. So while a panel is up the original is not
        // cycling anything, because it is running that loop and not the one
        // underneath. Ours kept the map's cycle turning under the panel, and
        // the arch's ivy is painted in the entries it rotates, which is what
        // made the leaves flicker on the knight's own sheet.
        if self.panel_now().is_none()
            && self
                .tick
                .is_multiple_of(u64::from(ticks_per_pass(self.mode)))
        {
            self.knight_glow_tick();
            self.fx.tick_effects();
            // `COLCON`'s own first three lines, before any of the colour work:
            //
            //   04988  cmp word ptr [ShakeCOUNT], 0
            //   0498d  je  04998
            //   0498f  dec word ptr [ShakeCOUNT]
            //   04993  jne 04998
            //   04995  call ShakeScreen
            //
            // The count lives in the bout, because it is simulation state two
            // peers of a lockstep fight have to agree on. What it looks like
            // does not, and does not come from the simulation's seed.
            if self.world.as_mut().is_some_and(|w| w.bout.take_shake()) {
                self.shake = SHAKE_FRAMES;
            }
        }
        // `ShakeScreen` (0x495b) itself: fifteen passes of the retrace, each
        // one jerking the display's own start address by a random nought to
        // three rows.
        //
        //   04962  mov cx, 0xf
        //   04965  mov dx, 0x3d4; mov al, 0xd; out dx, al   ; CRTC start low
        //   0496b  call <wait one retrace>
        //   0496e  call <rnd>; and al, 3; mov bl, 0x50; mul bl
        //   04977  mov dx, 0x3d5; out dx, al
        //   0497b  loop 0496b
        //
        // 0x50 is eighty bytes, which is one row of a four-plane 320 wide
        // screen, so the offset is nought to three rows and no more. The
        // original blocks the whole game loop for those fifteen retraces;
        // here the count is the same and fixed, so every peer pauses for the
        // same number of ticks, and only the offsets differ.
        if self.shake > 0 {
            self.shake -= 1;
            self.shake_rng = self.shake_rng.wrapping_mul(1103515245).wrapping_add(12345);
            self.shake_rows = ((self.shake_rng >> 16) & 3) as i32;
        } else {
            self.shake_rows = 0;
        }
        // `0x50d`, which the ceremony's own script gosubs part way through
        // itself rather than the scene routine installing it up front.
        self.moonstone_glow_tick();
        // Not `VBLQUE`'s, and so not gated with it: see `Effects::tick_fade`.
        self.fx.tick_fade();
        // `FADEOUTDAY`, and the fade a message chain ends on. Both are screens
        // that go out when they are dismissed rather than being walked away
        // from, which is the only kind of fade out a shell with no loading time
        // can honour: the original's other fades cover a disk read that does not
        // happen here. `NextWHICH` calls the between-days screen, then
        // `WaitFIRE`, then the fade, which is `FADEOUTDAY`; all three of
        // `WAITMESSAGE`, `OCCURMESSAGE` and `INSTRUCTMESSAGE` fade the chain.
        let leaving = if self.interlude > 0 {
            // Zero while it is still waiting, which leaves the screen fully lit.
            (self.interlude_out > 0).then_some(self.interlude_out)
        } else if self.showing.is_some() {
            // Zero while a chain is still waiting on fire, which leaves the
            // box fully lit; the fade begins when fire sets it to sixteen.
            (self.showing_for > 0).then_some(self.showing_for)
        } else {
            None
        };
        if let Some(left) = leaving {
            let steps = henge_assets::palette::FADE_STEPS;
            if left <= steps as u32 {
                self.fx
                    .set_fade(henge_assets::Fade::Out(steps - left as u16));
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
            // A door's tune first: the tavern, the healer and the mystic each
            // load one on the way in and stop it on the way out.
            self.door
                .as_ref()
                .and_then(|d| d.music_key())
                .or_else(|| self.visiting.as_ref().map(|s| s.visit.place.as_str()))
                .and_then(|key| self.music_places.get(key))
                .cloned()
        } else {
            None
        };
        match want {
            Some(id) => self.audio.play_music(&id),
            None => self.audio.stop_music(),
        }
    }

    /// `KnightGlowOn` (0x8f8), which the combat loop calls once a frame after
    /// the fighters have moved.
    ///
    /// **Recovered.** With the handle in `KGT` still zero and the main
    /// knight's health at ten or less, it takes his `KnightGlowColours` triple
    /// and installs `COLOURGLOW(6, glow[0], 2, 0)`, `COLOURGLOW(7, glow[1],
    /// 1, 0)` and `COLOURGLOW(8, glow[2], 1, 0)`; when `COLOURS` says a
    /// second knight is in the bout and `KnightHANDLE1` is zero, the same on
    /// 9, 10 and 11 for him, every frame. A glow walks the entry towards the
    /// brighter shade a step a period and swaps its ends on arrival, so the
    /// armour breathes between its colour and the brighter one until the
    /// bout ends and `KnightGlowOff` writes zero into every handle. Here a
    /// knight whose health has come back, which is a restart, loses his glow
    /// the same way, since the scene does not change.
    fn knight_glow_tick(&mut self) {
        let Some(w) = self.world.as_ref() else { return };
        if self.mode != Mode::Combat {
            return;
        }
        let base = self.fb.palette;
        for seat in 0..w.bout.fighters.len() {
            let low = w.knight_is_low(seat);
            let on = self.knight_glows.contains_key(&seat);
            if low && !on {
                let glows = w.knight_glow(seat);
                for g in &glows {
                    self.fx.install_glow(*g, &base);
                }
                self.knight_glows
                    .insert(seat, glows.iter().map(|g| g.index).collect());
            } else if !low && on {
                if let Some(entries) = self.knight_glows.remove(&seat) {
                    for e in entries {
                        self.fx.remove_glow(e);
                    }
                }
            }
        }
    }

    /// `0x50d`, the ending's moonstone glow.
    ///
    /// The ceremony's own script `0x3531` gosubs it on its third frame, and it
    /// installs three `COLOURGLOW` records through `0xa74`: entry 12 towards
    /// `[0x4489]`, 15 towards `[0x448b]` and 23 towards `[0x448d]`, period one,
    /// repeating for ever. Those are exactly the three entries
    /// `ColourMoonstone` wrote, and the three targets are the other half of
    /// what it wrote, so the stone pulses between the two shades the moon
    /// chose. `0x547` takes all three out again when the scene ends, which is
    /// what the scene change in `palette_tick` does.
    fn moonstone_glow_tick(&mut self) {
        use henge_core::ending;
        if self.mode != Mode::Ending || self.ending_glows {
            return;
        }
        let Some(scene) = self.ending.showing() else {
            return;
        };
        if scene.overlay != ending::Overlay::Near
            || self.ending.frame() < ending::MOONSTONE_GLOW_FRAME
        {
            return;
        }
        let Some((_, targets)) = ending::moonstone_ink(self.ending.code) else {
            return;
        };
        let base = self.fb.palette;
        for (at, target) in ending::MOONSTONE_AT.iter().zip(targets) {
            self.fx.install_glow(
                henge_assets::palette::Glow {
                    index: *at as u8,
                    target,
                    period: ending::MOONSTONE_GLOW_PERIOD,
                    repeat: 0,
                },
                &base,
            );
        }
        self.ending_glows = true;
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
        // runs until it fades. What takes it down is the caller's, not the
        // routine's (`henge_core::message`): a chain followed by `WaitFIRE`
        // at 0x8251 stays until fire is pressed and then goes out on the
        // sixteen-step fade at 0x5b65, exactly as the between-days screen
        // does below; a chain that covers a disk read goes out when the read
        // is done and fire does nothing to it. No timer ever clears the
        // first kind: one used to, and nothing in the original does.
        if let Some(msg) = self.showing.as_ref() {
            match msg.until {
                henge_core::message::Until::Fire => {
                    if self.showing_for > 0 {
                        self.showing_for -= 1;
                        if self.showing_for == 0 {
                            self.showing = None;
                        }
                    } else if self.takes() {
                        self.showing_for = henge_assets::palette::FADE_STEPS as u32;
                    }
                }
                henge_core::message::Until::Loaded => {
                    self.showing_for = self.showing_for.saturating_sub(1);
                    if self.showing_for == 0 {
                        self.showing = None;
                    }
                }
            }
            return;
        }

        // `HengeLOOP` is a loop of its own too: the circle's picture goes up,
        // its two tasks run, and nothing behind them moves until
        // `Knight_LiftMagic` reaches its end and `HengeControl` raises
        // `HengeFLAG`.
        if self.stones.is_some() {
            self.stones_tick();
            return;
        }

        // The between-days screen is modal, and it waits. `_MAP:NextWHICH` at
        // image 0xa454 is three calls in a row: the screen at 0x8e5b, then
        // `WaitFIRE` at 0x8251, which is `call 0x81ec; test bx, 0x10; je` until
        // fire is down and then the same until it is up again, and then the fade
        // out at 0x5b65, which is `FADEOUTDAY`. So it stays up for as long as
        // nobody presses anything and then goes out over sixteen frames.
        if self.interlude > 0 {
            if self.interlude_out > 0 {
                self.interlude_out -= 1;
                if self.interlude_out == 0 {
                    self.interlude = 0;
                }
            } else if self.takes() {
                // `test bx, 0x10`: fire, and nothing else, is what it reads.
                self.interlude_out = henge_assets::palette::FADE_STEPS as u32;
            }
            return;
        }

        // `_MAP:CreatePaper`'s panel is modal too, and more so: `DisplayStack`
        // draws it and then sits in a key loop with exactly one way out, which
        // is a number naming something the token is standing on.
        if self.paper {
            self.paper_tick();
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
            Mode::Ending => self.ending_tick(),
            Mode::Title => self.title_tick(),
            Mode::Online => self.online_tick(),
            Mode::Select => self.select_tick(),
            Mode::Map => {
                // A run that is over, won or lost. Both of the original's
                // endings are one message and then `WaitFIRE` at 0x8251, which
                // waits for a press and then for a release, so the box is up
                // until somebody presses fire and no longer. A loss then does
                // `jmp StartAgain`, which is the title. A win exits to DOS with
                // [`henge_core::quest::Tally::code`] in `al`, and the thing that
                // reads that byte is `INTR.EXE` run with it as its command tail,
                // which is the ending: `henge_core::ending`. So a win hands the
                // byte to the ending here, exactly as `MOON:KnightWonGame`
                // hands it to DOS, and a loss still goes to the title. A six
                // hundred tick timeout used to clear the screen on its own, and
                // there is no such thing in either routine.
                if self.run.ending().is_some() {
                    self.run_over_for += 1;
                    if self.run_over_for > 1 && self.takes() {
                        self.run_over_for = 0;
                        let code = self.run.tally().and_then(|t| t.code());
                        match code {
                            Some(code) => {
                                self.ending = Ending::new(code);
                                self.ending_glows = false;
                                self.mode = Mode::Ending;
                            }
                            None => {
                                self.run.restart();
                                // A new run is a new board: the lairs a dead
                                // knight emptied are full and back on the map,
                                // keys and all, and the three computer knights
                                // are back in their corners (`InitGameStart`).
                                self.stock_lairs();
                                self.run.seat_the_rivals(&self.items);
                                if let Some(w) = self.world.as_mut() {
                                    w.set_player_health(self.run.health_for_fight());
                                    w.set_moon(self.run.moon.phase().key());
                                }
                                self.mode = Mode::Title;
                            }
                        }
                    }
                    return;
                }
                // The sheet is the original's status screen: modal, and
                // where the casting and the levelling are done. So is the
                // lair's page, which is the same panel on `StatTYPE` 2.
                if self.sheet || self.lair_page.is_some() || self.trade_page.is_some() {
                    self.sheet_tick();
                    return;
                }
                // `MapLOOP+13` (0xa313): a kind 4 record's frame is the
                // computer knight's own day, and nothing the keyboard does
                // reaches it.
                if self.run.which != 0 {
                    self.rival_tick();
                    return;
                }
                // A toad has no turn. `_MAP:NextWHICH` tests `[si+0x3a]` and
                // goes straight round to the next knight when it is set, so
                // the wizard's curse costs its three days rather than being a
                // counter nothing reads: the turn passes and no step is taken.
                if self.run.is_toad() {
                    if let Some(m) = self.map.as_mut() {
                        m.state.pass_days(1);
                    }
                    self.end_turn();
                    return;
                }
                // Aloft, the map is crossed without steps or slow ground:
                // `MapMovement` skips its step count and `CheckSLOW` its grid
                // while either flag is up.
                if self.flight.is_some() {
                    self.fly(dx, dy);
                    // `FOLLOW` still calls the walk while a map effect flag is
                    // up, and the walk skips `MapIconsTABLE` and the rival
                    // knights for `CheckLairEncounter` when it is
                    // (`cmp [0xcca0], 0; je`, image 0x6f9), so aloft only the
                    // lairs are stacked. Nothing reads the stack in the air
                    // here, and the lairs are all it is drawn from, so gathering
                    // the lot leaves the same marks on the same ground.
                    if let Some(m) = self.map.as_ref() {
                        let (x, y) = (m.state.x, m.state.y);
                        self.overlaps
                            .gather(&self.places, x, y, self.run.knight.seat);
                    }
                    // The dragon flies on while the gem or the hawk is up:
                    // `DragonWander` is a task the loop steps whatever the
                    // flags say. It is the swoop that `DragonEncounter+26`
                    // (0xa3fc, 0xa403) keeps back.
                    self.dragon_frame();
                    return;
                }
                let mut day_before = 0;
                let mut at: Option<(i32, i32)> = None;
                if let Some(m) = self.map.as_mut() {
                    day_before = m.state.day;
                    // `DistanceDONE+12` (0xa4be): the turn is as long as the
                    // stride byte says, `[di+0x3e] << 4`, doubled by haste.
                    // The original writes it on every return to the map and
                    // nothing changes the stride mid-walk, so every frame is
                    // the same as that.
                    m.state.steps_per_day = self.run.day_steps(&self.items);
                    // `PlayerKnight` (0xa355) round to `DistanceDONE`: the
                    // slow ground, the step, the distance, the move. Nothing
                    // on that path rolls; the original has no ambush on the
                    // road, and the one that stood here was ours.
                    m.update(dx, dy);
                    at = Some((m.state.x, m.state.y));
                }
                // `_MAP:FOLLOW` walks the whole overlap table once a frame and
                // pushes what the token is standing on; nothing is opened here.
                // `CheckEncounterDone` (0x798) pushes the other three records
                // after the places.
                if let Some((x, y)) = at {
                    self.overlaps
                        .gather(&self.places, x, y, self.run.knight.seat);
                    let tokens = self.knight_tokens();
                    self.overlaps.gather_knights(&tokens, x, y);
                }
                // `GoTheDistance` (0xa422) into `NextWHICH` (0xa434): the
                // turn passes when the distance is spent. The three computer
                // knights have theirs, and when `WHICH` wraps the routine at
                // 0x1148 and the between-days screen at 0x8e5b follow.
                if let Some(m) = self.map.as_ref() {
                    if m.state.day != day_before {
                        self.end_turn();
                        return;
                    }
                }
                // The dragon over the map, in the frame's own order: `FOLLOW`
                // has moved the token, `CheckEncounterDone+128` (0x816) notes
                // whom the shadow is over, and `ScrollINPUT` reaches
                // `DragonEncounter` (0xa3e2) before `GoTheDistance`. A fight
                // ends with `EncounterAllDone` spending the day, so the frame
                // after one turns the day above and never gets here.
                self.dragon_frame();
                if self.run.dragon_comes_down(false) {
                    self.begin_dragon_fight();
                    return;
                }
                // `_MAP:ScrollINPUT`, image 0xa3c6: `mov ax, [JOYS];
                // test ax, 0x10; je` and, with fire down, `call DisplayStack`.
                // So nothing is ever walked into. A town is somewhere you stand
                // on and then ask to enter.
                if self.takes() {
                    self.display_stack();
                }
            }
            Mode::Place => {
                // A door's routine is a loop of its own (`TavernLoop`,
                // `DonateLoop`, `StatLOOP`) and nothing in the town runs
                // while it is up.
                if self.door.is_some() {
                    self.door_tick();
                    return;
                }
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
                let mut door: Option<henge_core::town::Door> = None;
                let mut raid: Option<(usize, String, String, String, u32)> = None;
                let mut quest: Option<(String, String, String, u32)> = None;
                let mut floor: Option<usize> = None;
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
                                Answer::Door(d) => door = Some(d),
                                Answer::Fight {
                                    lair,
                                    arena,
                                    family,
                                    guardian,
                                    count,
                                } => {
                                    raid = Some((lair, arena, family, guardian, count));
                                }
                                Answer::Guardian {
                                    arena,
                                    family,
                                    guardian,
                                    count,
                                } => {
                                    quest = Some((arena, family, guardian, count));
                                }
                                // `0x0574` for a lair already beaten runs
                                // straight on into `LairGEM`, which is the
                                // panel on `StatTYPE` 2 over the map.
                                Answer::Floor { lair } => floor = Some(lair),
                            }
                            // `MOON:Henge` does not hand a line back and stop:
                            // an offering the druids took runs the circle's own
                            // set piece at `0xb35e` before the life point is
                            // given. `henge_core::stones` is that loop.
                            if s.visit.rite {
                                s.visit.rite = false;
                                self.stones = Some(Stones::new(self.run.knight.seat as u8));
                                // `noswap+31` (0xb375): `HengeWait` through
                                // `INSTRUCTMESSAGE`, and then `Hen1.p` is
                                // loaded straight over it. The box is modal
                                // ahead of the set piece, so it is what is
                                // seen first.
                                if let Some(m) = self.messages.named("henge.ritual").cloned() {
                                    self.show_message(m);
                                }
                            }
                        }
                    } else {
                        leave = true;
                    }
                }
                // `LairGEM`: `mov ax, 2` and the panel, which owns the
                // screen. The place the lair's door was is left behind,
                // because in the original there is no such door: the map
                // walks onto the lair and `StackDecision` runs it.
                if let Some(lair) = floor {
                    self.open_lair_page(lair, false);
                    return;
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
                        // `AdjustLevel` (0x287e) takes a lair's own head count
                        // out of the lair record and writes it over
                        // `TotalMonsters`. How many of them stand in front of
                        // you at once is `MaxMonsters`, which is the creature's
                        // own and never this; the rest walk in as the ones
                        // before them fall. See `henge_core::wave`.
                        w.set_seats(self.title.state.players.max(1), 1);
                        w.set_heads(Some(count.max(1) as i32));
                        self.raiding = (lair != usize::MAX).then_some(lair);
                        self.raid_place = here.unwrap_or_default();
                        self.visiting = None;
                        self.mode = Mode::Combat;
                        return;
                    }
                }
                // One of a town's five: `HWLOOP`'s rung, `AddClickSound` and
                // the routine, over the town rather than in place of it.
                if let Some(d) = door {
                    self.audio.play(CLICK_SOUND);
                    self.door = town::Open::through(&mut self.reg, d, &self.run);
                    if let Some(open) = self.door.as_ref() {
                        // `0xbdd3` puts the pointer at (0xa0, 0x64) before
                        // `StatLOOP`; `TavernOpenScene` writes x 0x118 and
                        // leaves y; the two counters wait on fire first and
                        // put it at (0xa0, 0xaa) before the bowl.
                        match open {
                            town::Open::Panel(_) => self.point_at(0xa0, 0x64),
                            town::Open::Tavern { .. } => {
                                self.pointer.x = henge_core::town::TAVERN_POINTER_X;
                            }
                            town::Open::Counter { .. } => {}
                        }
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
                    // Every way out of a place is `EncounterAllDone` (0x113e):
                    // `CEXIT+6` for a town, `Wizard+9`, `Henge+122`, the
                    // Valley's `FightDemon+47` and `+108`. The rest of the
                    // day's distance goes with it.
                    self.spend_day();
                }
            }
            Mode::Combat => {
                if let Some(w) = self.world.as_mut() {
                    let seats = [
                        Intent {
                            dx,
                            dy,
                            attack: self.keys[6],
                        },
                        Intent {
                            dx: self.keys[10] as i32 - self.keys[9] as i32,
                            dy: self.keys[8] as i32 - self.keys[7] as i32,
                            attack: self.keys[11],
                        },
                    ];
                    w.update(&seats);
                    // What the scripts asked for on this tick, in the order
                    // they asked. Nothing is inferred from a state change: the
                    // `TASKSOUND` at 0x9b38 and the 23 sound routines the
                    // scripts call are the whole of the noise a fight makes,
                    // and `PLAY_SFX` plays every one of them as it arrives.
                    for call in &w.bout.sounds {
                        let id = sfx::asset(call.id);
                        if self.trace_sounds {
                            let on = w.bout.fighters.get(call.who);
                            println!(
                                // `simulate` has already counted this tick, and
                                // the trace loop counts from zero, so one off
                                // lines a sound up with the COMBAT line beside
                                // it.
                                "{:>5}  SOUND   {} id {:#04x} sample {:>2} {:<9} {}",
                                self.tick.saturating_sub(1),
                                call.who,
                                call.id,
                                sfx::sample(call.id),
                                sfx::name(call.id),
                                on.map_or(String::new(), |f| format!(
                                    "{} on {}",
                                    f.actor,
                                    f.task.as_ref().map_or("", |t| t.pc.script.as_str())
                                )),
                            );
                        }
                        self.audio.play(&id);
                    }
                    // A finished bout hands control back to the map, or restarts
                    // in place when there is no map to go back to.
                    // Record the outcome once, on the tick the fight settles.
                    let practice = self.practice;
                    if w.settled_for() == 1 && !practice {
                        let survivor = w.bout.fighters.first();
                        let health = survivor.map_or(0, |f| if f.alive() { f.health } else { 0 });
                        let won = w.bout.winner() == Some(0);
                        if let Some((attacker, defender)) = self.duel {
                            // A knight against a knight: `WhoLived` (0xabe)
                            // on both records and `InitKnightBattle+65`
                            // (0x440) onwards. Nothing is collected off the
                            // ground: a computer knight takes through
                            // `BKwon` and a person through the trade page.
                            let other = w.bout.fighters.get(1).map_or(0, |f| {
                                if f.alive() {
                                    f.health
                                } else {
                                    0
                                }
                            });
                            let (a, d) = if attacker == 0 {
                                (health, other)
                            } else {
                                (other, health)
                            };
                            let settled =
                                self.run
                                    .knight_fight_over(attacker, defender, a, d, &self.items);
                            self.duel_settled = Some(settled);
                            if let Some(r) = self.run.rivals.get_mut(defender.max(attacker) - 1) {
                                r.knight.daggers = w.daggers_left(1);
                            }
                        } else {
                            // What the fallen were carrying, and what they were
                            // worth. The run decides whether it is collected; a
                            // corpse collects nothing.
                            let xp = w.experience();
                            // 042a1: a ratman's bite outlives the bout.
                            let bitten = w.bout.bitten;
                            self.run
                                .finished_fight_worth(health, won, w.purse(), xp, bitten);
                        }
                        w.set_player_cursed(false);
                        // And what was thrown is gone: the sheet's daggers are
                        // whatever is left on the belt.
                        self.run.knight.daggers = w.daggers_left(0);
                        // `Combat+60` (0x38d): `mov dx, [0xccac]; mov [0xcc98],
                        // dx` the moment `WhoLived` has been asked, so a fight
                        // is the last thing the turn holds. `[0xcc98]` is the
                        // turn's, so a computer knight's challenge spends his
                        // day (`BKCollision+94`, 0xab0f) and not the player's.
                        if self.run.which == 0 {
                            let budget = self.run.day_steps(&self.items);
                            if let Some(m) = self.map.as_mut() {
                                m.state.steps_per_day = budget;
                                m.state.end_turn();
                            }
                        }
                    }
                    // A finisher on a fallen knight is allowed to play out, as
                    // the original's `StopCombat` is at the end of that script
                    // and not at the moment of death.
                    if w.settled_for() > 120 && !w.finishing() {
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
                            w.set_rival(None);
                            w.reset();
                            // `Knight1Won` (0x465) and `BothKnightsDied`
                            // (0x48d), once the bout is done.
                            if self.duel.take().is_some() {
                                let settled = self.duel_settled.take();
                                self.knight_fight_settled(settled);
                                return;
                            }
                            // The routine at 0xcf6 on its way out, held here
                            // for the finishing beat the same way the trade
                            // page and the lair's floor are.
                            if self.dragon_fight {
                                self.dragon_fight = false;
                                self.dragon_fight_settled(won);
                                return;
                            }
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
                                    let mut visit = self.visiting.as_ref().map(|s| s.visit.clone());
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
                                    // `MOON:LairWon` at 0x05ac marks the lair
                                    // and pays the one point of experience the
                                    // first win is worth, and then falls
                                    // straight into `LairGEM`, which puts the
                                    // panel up on `StatTYPE` 2. The floor is
                                    // handed over there, a gadget at a time,
                                    // and nowhere else.
                                    self.run.lair_beaten(lair);
                                    self.open_lair_page(lair, false);
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
        }
    }

    /// The ending, scene by scene. Fire skips it, and when it runs out the
    /// program is over, which here means the title.
    fn ending_tick(&mut self) {
        if self.takes() || self.pressed[11] {
            self.ending.skip();
        }
        self.ending.tick();
        if self.ending.done {
            self.mode = Mode::Title;
            // A new quest from the beginning, which is what running `MAIN.EXE`
            // again after the ending amounts to: `play.bat` runs the game, the
            // game exits with its byte, the ending runs, and the batch file goes
            // round to the game again.
            self.run.restart();
            self.stock_lairs();
            if let Some(w) = self.world.as_mut() {
                w.set_player_health(self.run.health_for_fight());
                w.set_moon(self.run.moon.phase().key());
            }
        }
    }

    /// One pass of the loop behind an open door.
    ///
    /// * The tavern: `TavernLoop` at 0xb137 steps the hand and, on the frame
    ///   `DiceTHROW` reaches 2, `DiceRND`; the dice picture then sits in
    ///   `DiceWait` until fire (0x8251). Fire over one of the six gadgets on
    ///   the table is `ThrowDice` as the handler reaches it.
    /// * The healer and the mystic: the greeting waits on fire, the bowl is
    ///   `DonateLoop` at 0xbbe6 (`MovePointer`, `CHECKGADGET`, and the op in
    ///   `es:[si+0x10]` when `PointerFLAG` is clear), the verdict waits its
    ///   fifty retraces and then on fire.
    /// * The merchant and the temple: `StatLOOP`, which is [`App::sheet_tick`].
    ///
    /// When the routine returns, `HWINIT`: the town picture again and the
    /// pointer back where the town keeps it.
    ///
    /// Direction keys step between the bowl's four boxes as well, because the
    /// original's pointer is a stick and a keyboard has to reach four gadgets
    /// somehow; the pointer itself is `MovePointer` unchanged.
    fn door_tick(&mut self) {
        let fire = self.takes() || self.pressed[11];
        let hit = self
            .gadgets
            .hit(self.pointer.x, self.pointer.y)
            .filter(|_| self.pointer.woken)
            .cloned();
        if matches!(self.door, Some(town::Open::Panel(_))) {
            self.sheet_tick();
            return;
        }
        if let Some(open) = self.door.as_mut() {
            match open {
                town::Open::Panel(_) => {}
                town::Open::Tavern { state, .. } => {
                    if state.screen == henge_core::town::TavernScreen::Table {
                        if let (true, Some(g)) = (fire, hit.as_ref()) {
                            state.press(g.id, g.payload.strp, &mut self.run);
                        }
                        // `ShakeDiceSnd` (0xb32f), which `DD_ThrowDice` calls
                        // six times through `TASKGOSUB` and nothing else in
                        // the game calls at all: the rattle of the cup.
                        let frame = state.tick(&self.scripts, &mut self.run);
                        let rattled = frame.is_some_and(|f| {
                            f.effects.iter().any(|e| {
                                matches!(e, henge_core::taskvm::Effect::Gosub { routine, .. }
                                    if routine == "ShakeDiceSnd")
                            })
                        });
                        if rattled {
                            self.audio.play(&sfx::asset(henge_core::sound::DICE_SHAKE));
                        }
                    } else {
                        state.tick(&self.scripts, &mut self.run);
                        if fire {
                            state.fire(&self.run);
                        }
                    }
                }
                town::Open::Counter { state, .. } => {
                    match &state.stage {
                        henge_core::town::Stage::Bowl(_) => {
                            // Which of the four: whatever the pointer is over,
                            // or the one the keys have stepped to.
                            if self.pressed[2] || self.pressed[0] {
                                self.door_cursor = self.door_cursor.saturating_sub(1);
                            }
                            if self.pressed[3] || self.pressed[1] {
                                self.door_cursor = (self.door_cursor + 1)
                                    .min(henge_core::status::DONATE_GADGETS.len() - 1);
                            }
                            if let Some(g) = hit.as_ref() {
                                self.door_cursor = g.id;
                            }
                            // `DonateLoop` reads `PointerFLAG`, which
                            // `MovePointer` clears while fire is *down*, so a
                            // held button keeps taking: `AddDonation` and
                            // `SubDonation` move a coin, wait two retraces
                            // (`mov ax, 2; call 0xafeb`) and go round again,
                            // which is how a bowl is filled by holding fire
                            // over the coin. None of the four calls
                            // `AddClickSound`.
                            self.door_wait = self.door_wait.saturating_sub(1);
                            let held = self.keys[6] || self.keys[11];
                            if held && self.door_wait == 0 {
                                use henge_core::status::DonateOp;
                                let op = henge_core::status::DONATE_GADGETS[self.door_cursor].4;
                                if matches!(op, DonateOp::Less | DonateOp::More) {
                                    self.door_wait = 2;
                                }
                                state.press(op, &mut self.run, &self.items);
                            }
                        }
                        _ => {
                            state.tick();
                            if fire {
                                state.fire(&self.run);
                                if let henge_core::town::Stage::Bowl(_) = state.stage {
                                    // `mov word ptr [PointerX], 0xa0; mov word
                                    // ptr [PointerY], 0xaa` before `InitDonation`.
                                    let (x, y) = henge_core::town::BOWL_POINTER;
                                    self.pointer.x = x;
                                    self.pointer.y = y;
                                    self.door_cursor = 0;
                                }
                            }
                        }
                    }
                }
            }
        }
        if self.door.as_ref().is_some_and(|d| d.closed()) {
            self.close_door();
        }
    }

    /// `HWINIT` (0xe14) or `WDINIT` (0xd5c): the door's routine has returned,
    /// the town's picture is drawn again and the pointer is put back where the
    /// town keeps it. The music the door started stops with it
    /// (`LeaveTavern` and `MysticFini` both end on `mov ah, 2; int 60h`).
    fn close_door(&mut self) {
        self.door = None;
        self.door_cursor = 0;
        self.door_wait = 0;
        self.sheet_said = None;
        let home = self
            .visiting
            .as_ref()
            .and_then(|s| self.places.get(&s.visit.place))
            .and_then(|d| d.pointer);
        if let Some([x, y]) = home {
            self.pointer.x = x;
            self.pointer.y = y;
        }
    }

    fn stones_tick(&mut self) {
        let Some(stones) = self.stones.as_mut() else {
            return;
        };
        stones.tick(&self.scripts);
        if stones.done() {
            self.stones = None;
        }
    }

    /// The title's option list, and the attract loop behind it.
    ///
    /// A press while it is showing off only wakes it. Letting the same press
    /// through would mean walking away from the keyboard and coming back to find
    /// the game had started itself.
    fn title_tick(&mut self) {
        // `OptionKeys` tests scancode 0x24 first of all and jumps to
        // `Fix_JoyStick` when it is down, so `J` on this screen and on no other
        // starts the calibration, and the calibration comes back here.
        if self.pressed[CALIBRATE_SLOT] {
            self.calibrate(0);
            return;
        }
        let (up, down) = (self.pressed[0], self.pressed[1]);
        let (left, right, take) = (self.pressed[2], self.pressed[3], self.takes());
        let touched = up || down || left || right || take;
        if !touched {
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
                Some(Start::Online) => self.begin_online(),
                None => {}
            }
        }
    }

    /// Choosing knights, one player at a time, and typing a name over each.
    ///
    /// `ChooseLoop` at 0x15a0 reads the stick; fire goes to `ChooseFIRE`, which
    /// hands the knight's own name buffer to `TypeName` at 0x13f0. `TypeName`
    /// owns the input from there: `ScanKEYS` at 0x142e takes fire (`bx & 0x10`)
    /// or scancode 0x1c, Enter, as the end of it, scancode 0x0e as a backspace,
    /// and anything `ASCIIKEY` gives a character for as a character.
    fn select_tick(&mut self) {
        // Whose keys drive the screen. `choose_player` says which seat is
        // choosing, and at one keyboard the answer is always this one: the four
        // take their turns on the same keys. Across machines it is that seat's
        // own word off the wire, which is the whole of what being online changes
        // here. Everything below is the same code either way, so the screen
        // cannot behave differently in a networked game.
        let seat = self.select.as_ref().map(|s| s.state.seat).unwrap_or(0);
        let (left, right, take, back, typed) = if self.net.is_some() {
            let p = self.seat_pressed.get(seat).copied().unwrap_or_default();
            self.typed = None;
            (p.left, p.right, p.take, p.back, p.typed)
        } else {
            (
                self.pressed[2],
                self.pressed[3],
                self.takes(),
                self.pressed[BACKSPACE_SLOT],
                self.typed.take(),
            )
        };
        let defaults: Vec<String> = self.knights.iter().map(|k| k.name.clone()).collect();
        let Some(select) = self.select.as_mut() else {
            self.mode = Mode::Title;
            return;
        };
        if select.state.typing.is_some() {
            // Fire or Enter is `NameDone`; everything else goes into the buffer.
            if take {
                if let Some((knight, name)) = select.state.name_done() {
                    self.named.push((knight, name));
                }
            } else if let Some(t) = select.state.typing.as_mut() {
                if back {
                    t.backspace();
                }
                if let Some(c) = typed {
                    t.type_char(c);
                }
            }
            if select.state.done() {
                self.begin_quest();
            }
            return;
        }
        if left {
            select.state.move_by(-1);
        }
        if right {
            select.state.move_by(1);
        }
        if take {
            let default = defaults
                .get(select.state.cursor)
                .cloned()
                .unwrap_or_default();
            select.state.take(&default);
        }
        if select.state.done() {
            self.begin_quest();
        }
    }

    // ------------------------------------------------------------- online play
    //
    // **Ours, every line of it.** The original has no network code; see
    // `henge_net`'s own note. What is here is the lobby screen's wiring and the
    // one gate in [`App::update`] that stops a tick until every seat's word for
    // it has arrived. The simulation below that gate is never told a peer
    // exists, and no recovered number changes because of anything here.

    /// The fifth row of the title, which is ours: open the lobby.
    fn begin_online(&mut self) {
        self.online = Some(online::Online::new());
        self.mode = Mode::Online;
    }

    /// One tick of the lobby: the keys, then whatever the wire said.
    fn online_tick(&mut self) {
        let Some(mut screen) = self.online.take() else {
            self.mode = Mode::Title;
            return;
        };
        // A scripted run works the lobby through `--host`, `--join` and
        // `--begin` and has no business on its rows: the wanderer `--trace`
        // drives the map with presses whatever screen is up, and on a menu those
        // presses are a person mashing the keyboard. So they are dropped here,
        // and only here, because every other screen is what a trace is for.
        if self.driven {
            self.raw_pressed = [false; 256];
            self.raw_typed = None;
        }
        // The same five bits every other screen reads, off this keyboard rather
        // than off the wire: nothing in the lobby is in lockstep yet.
        let (up, down) = (self.raw_pressed[0], self.raw_pressed[1]);
        let (left, right) = (self.raw_pressed[2], self.raw_pressed[3]);
        let take = self.raw_pressed[6] || self.raw_pressed[ENTER_SLOT];
        let back = self.raw_pressed[BACKSPACE_SLOT];
        let typed = self.raw_typed.take();

        let mut ask = None;
        if up {
            screen.move_by(-1);
        }
        if down {
            screen.move_by(1);
        }
        if left {
            ask = ask.or(screen.adjust(-1));
        }
        if right {
            ask = ask.or(screen.adjust(1));
        }
        if back {
            screen.backspace();
        }
        if let Some(c) = typed {
            // The caret's own key ends the typing, so a character is only a
            // character. `TypeName` reads Enter the same way.
            screen.type_char(c);
        }
        if take {
            ask = ask.or(screen.take());
        }
        self.online = Some(screen);
        if let Some(ask) = ask {
            self.online_ask(ask);
        }
        self.online_poll();
        self.online_port();
        self.online_auto();
    }

    /// `--begin`, and a scripted guest sitting itself down. Both exist so the
    /// whole path can be driven with nobody at either keyboard; neither does
    /// anything in a game a person is playing.
    fn online_auto(&mut self) {
        let Some(want) = self.auto_begin else {
            // A guest started from `--join` says it is ready as soon as it has a
            // seat, because a script has no row to press.
            if self.driven {
                let sat = self
                    .online
                    .as_ref()
                    .is_some_and(|o| o.seat.is_some() && !o.ready);
                if sat && matches!(self.lobby, Some(Waiting::Guest(_))) {
                    if let Some(o) = self.online.as_mut() {
                        o.ready = true;
                    }
                    self.online_ask(online::Ask::Seat { ready: true });
                }
            }
            return;
        };
        let ready = match self.lobby.as_ref() {
            Some(Waiting::Host(h)) => h.lobby.players() >= want && h.lobby.can_start(),
            _ => false,
        };
        if ready {
            self.auto_begin = None;
            self.online_ask(online::Ask::Begin);
        }
    }

    /// `--trace` through `ChooseKnight`: the seat whose turn it is takes the
    /// knight under the highlight and keeps its own name.
    ///
    /// **Ours, and only for a scripted run.** [`App::driven`] is set by `--trace`
    /// and `--frames` and by nothing a person does. `ChooseKnight` is modal and a
    /// script has no keyboard, so without this a headless online run would sit on
    /// it forever and the lockstep path could not be driven end to end any more.
    /// It presses this machine's own seat and no other, so the press goes onto
    /// the wire like any other press and every machine still walks the same
    /// turns.
    fn select_auto(&mut self) {
        if self.mode != Mode::Select || self.net.is_none() {
            self.select_held = false;
            return;
        }
        let mine = self.net.as_ref().map(|n| n.seat()).unwrap_or(0);
        let turn = self.select.as_ref().map(|s| s.state.seat).unwrap_or(0);
        // Down, then up. `ChooseFIRE` and `NameDone` are two presses of the one
        // key, and a key that is never let go is one press.
        self.select_held = !self.select_held && turn == mine;
        self.keys[ENTER_SLOT] = self.select_held;
        self.pressed[ENTER_SLOT] = self.select_held;
    }

    /// Act on what the lobby screen asked for.
    fn online_ask(&mut self, ask: online::Ask) {
        match ask {
            online::Ask::Open {
                game,
                you,
                password,
            } => {
                let port = online_port_arg(&args_of()).unwrap_or(henge_net::DEFAULT_PORT);
                match henge_net::Host::open(&game, &you, port, self.title.state.gore) {
                    Ok(mut host) => {
                        let port = host.port();
                        host.lock(&password);
                        // On the list, if there is one to be on. The server
                        // answers with the address it saw us come from and
                        // whether it could get back in, which is a better
                        // answer than the router's own.
                        let server = list_server(&args_of());
                        let listed = if server.is_empty() {
                            Some("no list server: friends will need your address".to_string())
                        } else {
                            match host.list_on(&server, &build_name()) {
                                Ok(()) => None,
                                Err(e) => Some(format!("not on the list ({server}): {e}")),
                            }
                        };
                        self.lobby = Some(Waiting::Host(host));
                        // The router, on its own thread: the lobby is drawable
                        // now and the address fills in when it answers.
                        self.opener = Some(henge_net::Opener::start(port));
                        if let Some(o) = self.online.as_mut() {
                            o.sat_down(0, true);
                            o.note = listed
                                .unwrap_or_else(|| format!("opening port {port} and listing it"));
                        }
                    }
                    Err(e) => {
                        if let Some(o) = self.online.as_mut() {
                            o.note = format!("could not open a game: {e}");
                        }
                    }
                }
            }
            online::Ask::Look => {
                let server = list_server(&args_of());
                if server.is_empty() {
                    if let Some(o) = self.online.as_mut() {
                        o.listed(
                            Vec::new(),
                            &format!("no list server set: put one in {LIST_FILE}, or --list"),
                        );
                    }
                    return;
                }
                // This one stops the game for as long as the answer takes, which
                // is a second at worst. It happens because somebody pressed a
                // key and is waiting for a list.
                match henge_net::browse(&server, &build_name()) {
                    Ok(games) => {
                        let note = if games.is_empty() {
                            format!("no games open on {server}")
                        } else {
                            format!("{} open on {server}", games.len())
                        };
                        if let Some(o) = self.online.as_mut() {
                            o.listed(games, &note);
                        }
                    }
                    Err(e) => {
                        if let Some(o) = self.online.as_mut() {
                            o.listed(Vec::new(), &format!("could not reach {server}: {e}"));
                        }
                    }
                }
            }
            online::Ask::Take {
                game,
                you,
                password,
            } => {
                // A game the list server is carrying is reached through the
                // server; anything else is dialled directly. Either way what
                // comes back is an ordinary game link and nothing downstream can
                // tell the difference.
                let joined = if game.relayed() {
                    let server = list_server(&args_of());
                    henge_net::list::reach(&server, &game.code)
                        .map_err(henge_net::JoinError::from)
                        .and_then(|link| henge_net::Guest::over(link, &you, &password))
                } else {
                    henge_net::Guest::join(game.at.as_str(), &you, &password)
                };
                match joined {
                    Ok(guest) => {
                        self.lobby = Some(Waiting::Guest(guest));
                        if let Some(o) = self.online.as_mut() {
                            o.note = format!("joining {}", game.name);
                        }
                    }
                    Err(e) => {
                        if let Some(o) = self.online.as_mut() {
                            o.note = format!("could not join {}: {e}", game.name);
                        }
                    }
                }
            }
            online::Ask::Dial {
                address,
                you,
                password,
            } => {
                // A bare address means the usual port, because nobody wants to
                // type a number they were never told.
                let with_port = if address.contains(':') {
                    address.clone()
                } else {
                    format!("{address}:{}", henge_net::DEFAULT_PORT)
                };
                match henge_net::Guest::join(with_port.as_str(), &you, &password) {
                    Ok(guest) => {
                        self.lobby = Some(Waiting::Guest(guest));
                        if let Some(o) = self.online.as_mut() {
                            o.note = format!("joining {with_port}");
                        }
                    }
                    Err(e) => {
                        if let Some(o) = self.online.as_mut() {
                            o.note = format!("could not join {with_port}: {e}");
                        }
                    }
                }
            }
            online::Ask::Seat { ready } => match self.lobby.as_mut() {
                Some(Waiting::Host(h)) => h.seat(ready),
                Some(Waiting::Guest(g)) => g.seat_request(ready),
                None => {}
            },
            online::Ask::Begin => {
                // The delay comes from what the lobby's round trips actually
                // measured, not from an assumption about the line. With a relay
                // in the path, which is every game when both ends are behind a
                // carrier's NAT, the two are nothing like each other.
                let delay = match (online_delay_arg(&args_of()), self.lobby.as_ref()) {
                    (Some(given), _) => given,
                    (None, Some(Waiting::Host(h))) => h.suggested_delay(tick_len_for(Mode::Map)),
                    (None, _) => henge_net::lockstep::MIN_DELAY,
                };
                let terms = match self.lobby.as_mut() {
                    Some(Waiting::Host(h)) => h.start(delay, henge_net::lockstep::CHECK_EVERY),
                    _ => None,
                };
                match terms {
                    Some(ev) => self.online_start(ev, delay),
                    None => {
                        if let Some(o) = self.online.as_mut() {
                            o.note = "everybody has to be ready first".into();
                        }
                    }
                }
            }
            online::Ask::Leave => {
                self.leave_online("left the lobby");
                self.mode = Mode::Title;
            }
        }
    }

    /// Whatever the lobby's socket has to say.
    fn online_poll(&mut self) {
        let Some(side) = self.lobby.as_mut() else {
            return;
        };
        let events = match side {
            Waiting::Host(h) => h.poll(),
            Waiting::Guest(g) => g.poll(),
        };
        // The roster as it stands now, which is the only copy a guest has.
        let roster = match self.lobby.as_ref() {
            Some(Waiting::Host(h)) => Some(h.lobby.clone()),
            Some(Waiting::Guest(g)) => Some(g.lobby.clone()),
            None => None,
        };
        let trips: std::collections::BTreeMap<u8, u32> = match self.lobby.as_ref() {
            Some(Waiting::Host(h)) => (0..henge_core::shell::SEATS as u8)
                .filter_map(|s| h.trip(s).map(|ms| (s, ms)))
                .collect(),
            _ => std::collections::BTreeMap::new(),
        };
        if let (Some(o), Some(r)) = (self.online.as_mut(), roster) {
            o.roster = r;
            o.trips = trips;
        }
        let mut start = None;
        let mut delay = 0;
        for e in events {
            match e {
                henge_net::Event::Seated { seat } => {
                    if let Some(o) = self.online.as_mut() {
                        o.sat_down(seat, false);
                        o.note = format!("in seat {}", seat + 1);
                    }
                }
                henge_net::Event::Refused { why } => {
                    self.leave_online("");
                    if let Some(o) = self.online.as_mut() {
                        o.back_to_menu(&why);
                    }
                }
                henge_net::Event::Joined { name, seat } => {
                    if let Some(o) = self.online.as_mut() {
                        o.note = format!("{name} took seat {}", seat + 1);
                    }
                }
                henge_net::Event::Left { name, .. } => {
                    if let Some(o) = self.online.as_mut() {
                        o.note = format!("{name} left");
                    }
                }
                henge_net::Event::Lost { why, .. } => {
                    // A guest losing the host is out of the lobby; a host losing
                    // one guest is not.
                    if matches!(self.lobby, Some(Waiting::Guest(_))) {
                        self.leave_online("");
                        if let Some(o) = self.online.as_mut() {
                            o.back_to_menu(&why);
                        }
                    } else if let Some(o) = self.online.as_mut() {
                        o.note = why;
                    }
                }
                ev @ henge_net::Event::Start { .. } => {
                    if let henge_net::Event::Start { delay: d, .. } = &ev {
                        delay = *d as u32;
                    }
                    start = Some(ev);
                }
                // The roster has already been taken above, and a lobby has no
                // ticks to run.
                henge_net::Event::Note { text } => {
                    if let Some(o) = self.online.as_mut() {
                        o.note = text;
                    }
                }
                henge_net::Event::Roster
                | henge_net::Event::Input { .. }
                | henge_net::Event::Turn { .. }
                | henge_net::Event::Check { .. }
                | henge_net::Event::Desync { .. } => {}
            }
        }
        if let Some(ev) = start {
            self.online_start(ev, delay);
        }
    }

    /// The router's answer, once it has one.
    fn online_port(&mut self) {
        let Some(opener) = self.opener.as_mut() else {
            return;
        };
        let Some(map) = opener.ready().cloned() else {
            return;
        };
        self.opener = None;
        // The list server's answer, if there is one, is a measurement and the
        // router's is a claim. The measurement wins.
        let measured = match self.lobby.as_ref() {
            Some(Waiting::Host(h)) => h.address(),
            _ => None,
        };
        if let Some(o) = self.online.as_mut() {
            o.reachable = measured.clone().unwrap_or_else(|| map.address());
            if measured.is_some() {
                self.mapping = Some(map);
                return;
            }
            o.note = if map.how.opened() {
                format!("friends can join at {}", map.address())
            } else if map.local.is_some() {
                // Honest: the port may be open anyway, and saying it is when it
                // is not is how somebody spends an evening wondering why.
                format!(
                    "the router would not open the port, so {} works on this network only ({})",
                    map.address(),
                    map.note
                )
            } else {
                format!("no network to host on: {}", map.note)
            };
        }
        self.mapping = Some(map);
    }

    /// The game begins: the lobby's socket becomes a session, and the four land
    /// on `ChooseKnight`.
    ///
    /// **The lobby settles nothing about knights.** Pressing Begin does what the
    /// title's own Start does: it opens `ChooseKnight`, the same screen a game at
    /// one keyboard gets, and the seats take their turns on it in `choose_player`
    /// order. The difference is only where the keys come from: the screen is
    /// inside the lockstep gate from its first tick, so every machine walks the
    /// same turns in the same order and [`App::begin_quest`] builds the same run
    /// out of `choose_knight` on all of them.
    fn online_start(&mut self, ev: henge_net::Event, delay: u32) {
        let henge_net::Event::Start {
            seats, gore, check, ..
        } = ev
        else {
            return;
        };
        let seats = (seats as usize).clamp(1, henge_core::shell::SEATS);
        let delay = delay.max(henge_net::lockstep::MIN_DELAY);
        let mine = self.online.as_ref().and_then(|o| o.seat).unwrap_or(0) as usize;
        let session = match self.lobby.take() {
            Some(Waiting::Host(h)) => Some(henge_net::Session::host(h, seats, delay, check)),
            Some(Waiting::Guest(g)) => {
                Some(henge_net::Session::guest(g, seats, mine, delay, check))
            }
            None => None,
        };
        let Some(session) = session else {
            return;
        };
        self.net = Some(session);
        self.seat_was = [henge_net::SeatInput::default(); henge_core::shell::SEATS];
        self.seat_pressed = [SeatPress::default(); henge_core::shell::SEATS];
        self.online = None;
        // The title's own player count is overruled by the lobby's, which is
        // what `Adjplayers` would have been told, and it is what `choose_loop`
        // counts down on the screen we are about to open.
        self.title.state.players = seats;
        self.title.state.gore = gore;
        self.named.clear();
        self.practice = false;
        self.begin_select();
    }

    /// One tick of a game running across machines. Returns whether the
    /// simulation may run this tick.
    fn net_step(&mut self) -> bool {
        let local = self.local_input();
        let Some(mut net) = self.net.take() else {
            return true;
        };
        net.poll();
        // The fingerprint is only wanted on the ticks a check falls on, and it
        // walks the whole run, so it is not taken on the others.
        let hashed = if net.step.check_due() {
            self.net_hash()
        } else {
            0
        };
        let got = net.advance(local, &mut || hashed);
        let notes = net.notes();
        let over = net.over().map(str::to_string);
        self.net = Some(net);
        for note in notes {
            println!("online: {note}");
        }
        if let Some(why) = over {
            // The game stops being a shared one. It is not stopped dead: this
            // machine keeps its own run, which is the least surprising thing to
            // do to somebody halfway through a quest.
            self.net = None;
            self.mapping.take().inspect(|m| m.close());
            self.notice(format!("the online game ended: {why}"));
            return true;
        }
        match got {
            Some((_, turn)) => {
                self.apply_turn(&turn);
                true
            }
            None => false,
        }
    }

    /// This machine's own word for the tick it belongs to.
    ///
    /// Read off [`App::raw`] rather than [`App::keys`], because `keys` is what
    /// the wire has already written and reading it back would send this machine
    /// whatever the last tick said somebody else was holding.
    fn local_input(&self) -> henge_net::SeatInput {
        let mut pad = 0u8;
        for a in input::Action::ALL {
            let slot = slot_of(0, a);
            if slot < 256 && self.raw[slot] {
                pad |= a.bit();
            }
        }
        let mut keys = 0u8;
        // Enter, which every menu in this build takes as well as fire. Fire
        // itself is already a bit of the pad.
        if self.raw[ENTER_SLOT] {
            keys |= henge_net::key::TAKE;
        }
        if self.raw[BACKSPACE_SLOT] {
            keys |= henge_net::key::BACK;
        }
        let number = (1..=9u8).find(|n| self.raw[NUMBER_SLOT + *n as usize - 1]);
        henge_net::SeatInput {
            pad,
            keys,
            typed: self.raw_typed,
            number,
        }
    }

    /// Write a tick's words into the slots the simulation reads.
    ///
    /// A press is the rising edge against the tick before, computed here rather
    /// than sent, so two machines cannot disagree about whether one happened.
    /// That is `BOUNCEBUTTON`'s own rule.
    fn apply_turn(&mut self, turn: &henge_net::Turn) {
        for (seat, now) in turn.iter().enumerate() {
            // Enter, backspace and the nine number keys have one slot between
            // the four seats, because they are not controls and the original has
            // no table for them. So they are written from **seat zero's** word
            // and not from this machine's.
            //
            // That is not a shortcut, it is the only answer that keeps the
            // machines together. A slot filled from `mine` would hold a
            // different word on every machine, and every screen that reads one
            // would then act on a different tick on each: a message box is
            // dismissed by `WaitFIRE`, which is [`App::takes`], so one machine
            // would still have the box up while another had walked on. It is
            // also what the run already says: the quest belongs to seat zero's
            // knight and the map's turn is `WHICH`'s, which is seat zero's, so
            // seat zero is whose Enter closes a box on it. A screen where every
            // seat needs its own is read out of [`App::seat_pressed`] instead,
            // which is per seat and which `ChooseKnight` uses.
            let shared = seat == 0;
            for (slot, held, pressed) in seat_slots(seat, *now, self.seat_was[seat], shared) {
                self.keys[slot] = held;
                self.pressed[slot] = pressed;
            }
            if shared {
                self.typed = now.typed;
            }
            // The same rising edges, kept per seat, for the keys that have one
            // slot between the four of them. Worked out here because this is
            // where last tick's word is still to hand.
            let was = self.seat_was[seat];
            let edge = |a: input::Action| now.pad & a.bit() != 0 && was.pad & a.bit() == 0;
            self.seat_pressed[seat] = SeatPress {
                left: edge(input::Action::Left),
                right: edge(input::Action::Right),
                take: edge(input::Action::Fire) || (now.take() && !was.take()),
                back: now.back() && !was.back(),
                typed: now.typed,
            };
        }
        self.seat_was = *turn;
    }

    /// The fingerprint the machines compare: the whole run, where the traveller
    /// is, and the bout if there is one.
    ///
    /// All three are the simulation's own, and all three are already proved to
    /// round-trip: `Run::state_hash`, `Overworld::state_hash` and
    /// `Bout::state_hash`.
    fn net_hash(&self) -> u64 {
        let mut h: u64 = self.run.state_hash();
        for v in [
            self.map.as_ref().map(|m| m.state.state_hash()).unwrap_or(0),
            self.world
                .as_ref()
                .map(|w| w.bout.state_hash())
                .unwrap_or(0),
            self.mode as u64,
        ] {
            h ^= v;
            h = h.wrapping_mul(0x1000_0000_01b3);
        }
        h
    }

    /// Out of a lobby or a game: the socket, the router's mapping, and the
    /// screen.
    fn leave_online(&mut self, why: &str) {
        match self.lobby.take() {
            Some(Waiting::Host(mut h)) => h.close(why),
            Some(Waiting::Guest(mut g)) => g.close(why),
            None => {}
        }
        if let Some(mut net) = self.net.take() {
            net.close(why);
        }
        self.opener = None;
        if let Some(m) = self.mapping.take() {
            m.close();
        }
        self.online = None;
    }

    fn begin_select(&mut self) {
        if self.knights.is_empty() {
            // Nothing to choose between, so there is nothing to show. Start the
            // quest as nobody rather than opening an empty screen.
            self.mode = Mode::Map;
            return;
        }
        let players = self.title.state.players.min(self.knights.len());
        self.select = Some(shell::SelectScene::new(&self.reg, players));
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
        let chosen = self
            .select
            .as_ref()
            .map(|s| s.state.chosen())
            .unwrap_or_default();
        let mine = chosen.first().copied().unwrap_or(0);
        let mut roster = chosen.clone();
        for i in 0..henge_core::shell::SEATS {
            if !roster.contains(&i) {
                roster.push(i);
            }
        }
        self.practice = false;
        self.take_knight(mine, roster);
        // What was typed over the knight's own name, which is the whole point of
        // `TypeName`. Only seat zero's reaches the run, because a run belongs to
        // one knight here; an emptied name is left as the default, because
        // `Knight::named` uses the name as the flag for a knight having been
        // chosen at all, which the original keeps as a separate bitmask.
        if let Some((_, name)) = self.named.iter().find(|(k, _)| *k == mine) {
            if !name.is_empty() {
                self.run.knight.name = name.clone();
            }
        }
        self.named.clear();
        self.select = None;
        self.mode = if self.map.is_some() {
            Mode::Map
        } else {
            Mode::Combat
        };
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
        let Some(def) = self.knights.get(knight) else {
            return;
        };
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
        let phase = self.run.moon.phase().key();
        if let Some(w) = self.world.as_mut() {
            // A quest opens on the full moon, which `InitGameStart` writes
            // before anything else runs, and the ratmen are already fielded
            // by it: the phase goes in with everything else the run decides.
            w.set_moon(phase);
            // One opponent's seat to begin with; a lair's raid sets its own
            // head count when the door opens.
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
            // `AdjustLevel` reads `+0x2e` and `+0x3c` off the knight record
            // before it decides how many creatures a fight holds.
            strength: self.run.knight.strength,
            experience: self.run.experience as i32,
            talismans: self.run.kit.count("talisman_of_the_wyrm") as i32,
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

    /// **Test harness only.** Write a snapshot of the posed run.
    ///
    /// The original has no save and nothing a player can reach calls this: the
    /// only two routes in are `--input`'s `S` and the `--save` path, both of which
    /// are command line. It is taken on the map and nowhere else, because a
    /// snapshot is a serialisation of the simulation between one step and the
    /// next; inside a bout it would mean carrying every fighter's script pointer
    /// and the knives in the air. `henge_core::harness` says the rest.
    fn harness_save(&mut self) -> bool {
        if self.mode != Mode::Map || !self.run.knight.named() {
            self.notice("only on the road");
            return false;
        }
        let Some(m) = self.map.as_ref() else {
            self.notice("no map to snapshot");
            return false;
        };
        let snap = Snapshot::of(
            &self.run,
            &m.state,
            self.title.state.players,
            self.title.state.gore,
            self.hint,
        );
        let text = match serde_json::to_string_pretty(&snap) {
            Ok(t) => t,
            Err(e) => {
                self.notice(format!("snapshot failed: {e}"));
                return false;
            }
        };
        match std::fs::write(&self.snapshot_path, text) {
            Ok(()) => {
                println!("wrote {}: {}", self.snapshot_path, snap.summary());
                self.notice("snapshot written");
                true
            }
            Err(e) => {
                self.notice(format!("snapshot failed: {e}"));
                false
            }
        }
    }

    /// **Test harness only.** Read a snapshot back, or say clearly why not.
    ///
    /// Three distinguishable refusals, none of which loads half a run: not a
    /// snapshot, a snapshot this build cannot read, and one that does not match
    /// its own fingerprint.
    fn harness_load(&mut self) -> bool {
        let text = match std::fs::read_to_string(&self.snapshot_path) {
            Ok(t) => t,
            Err(e) => {
                self.notice(format!("no snapshot: {e}"));
                return false;
            }
        };
        let snap: Snapshot = match serde_json::from_str(&text) {
            Ok(s) => s,
            Err(_) => {
                self.notice(henge_core::harness::SnapshotError::NotASnapshot.message());
                eprintln!("{}: not a harness snapshot", self.snapshot_path);
                return false;
            }
        };
        if let Err(e) = snap.check() {
            self.notice(e.message());
            eprintln!("{}: {e}", self.snapshot_path);
            return false;
        }
        println!("loaded {}: {}", self.snapshot_path, snap.summary());
        self.apply(snap);
        true
    }

    /// Put a checked snapshot into the running game. Harness only.
    fn apply(&mut self, save: Snapshot) {
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
        self.interlude_out = 0;
        self.paper = false;
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
    ///
    /// `showing_for` is the ticks left before it goes: for a chain the
    /// original waits on it starts at zero, which is "waiting", and fire sets
    /// it to the fade's sixteen; for a chain that covers a load it starts at
    /// [`Self::LOAD_TICKS`] and counts down, the last sixteen of them the fade.
    fn show_message(&mut self, msg: Message) {
        if msg.is_empty() {
            return;
        }
        self.showing_for = match msg.until {
            henge_core::message::Until::Fire => 0,
            henge_core::message::Until::Loaded => Self::LOAD_TICKS,
        };
        self.showing = Some(msg);
    }

    /// **Ours.** How long a box that covers a disk read stays up. The original
    /// holds it for exactly as long as the read takes, and nothing here reads
    /// a disk, so some length has to stand in for it; the fourteen `WaitMES`
    /// chains, the two city welcomes and `HengeWait` are the ones it covers.
    /// A chain the original follows with `WaitFIRE` never uses this.
    const LOAD_TICKS: u32 = 260;

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
        self.sheet_said = None;
        // The status screen is the pointer's own screen and has nothing else on
        // it. Every icon `PlaceIcons` blits is a gadget of the same size, every
        // pillar is the way out, and `MovePointer` reads the knight's own
        // control rather than the other seat's, so seat one steers it here.
        // Modal on the map, which is where `ReDisplay` and `StatLOOP` sit; over
        // an arena or a doorway the sheet is a card held up, and whatever is
        // behind it keeps its own gadgets.
        let panel_up = self.mode == Mode::Map || self.door_panel().is_some();
        if let Some((screen, other)) = self.panel_now().filter(|_| panel_up) {
            let panel = status::lay_out(&self.run, screen, other.as_ref());
            for g in panel.gadgets(&mut self.reg) {
                self.gadgets.add(g);
            }
            let dx = self.keys[3] as i32 - self.keys[2] as i32;
            let dy = self.keys[1] as i32 - self.keys[0] as i32;
            self.pointer.steer(dx, dy, self.keys[6]);
            if self.pointer.woken {
                self.sheet_said = self
                    .gadgets
                    .hit(self.pointer.x, self.pointer.y)
                    .map(|g| g.label.clone())
                    .filter(|l| !l.is_empty());
            }
            return;
        }
        // A door's own loop: the tavern's six on the table (0xb053) and
        // `InitDonation`'s four (0xbb48), which `DonateLoop` reads by their
        // payload word rather than by an id.
        // `TavernLoop+0` and `DonateLoop+0` both call 0xcea5, which is
        // `push [0x77e8]; pop [StatHAND1]` and then `MovePointer`: the
        // knight's own stick, which is seat one's keys here as on the panel.
        if let Some(open) = self.door.as_ref() {
            open.gadgets(&mut self.gadgets);
            let dx = self.keys[3] as i32 - self.keys[2] as i32;
            let dy = self.keys[1] as i32 - self.keys[0] as i32;
            self.pointer.steer(dx, dy, self.keys[6]);
            return;
        }
        let rows: Vec<(usize, i32, i32, i32, i32)> = match self.mode {
            // The title and the select have no gadgets and no pointer:
            // `DoOptions` and `ChooseLoop` poll the stick themselves and
            // the six routines that blit `PO.CEL` are the town menus, the
            // tavern, the wizard's donation and the status screen. See
            // `shell::draw_pointer`.
            Mode::Place => self
                .visiting
                .as_ref()
                .and_then(|s| self.places.get(&s.visit.place))
                .map_or_else(Vec::new, place::menu_rects),
            _ => Vec::new(),
        };
        if rows.is_empty() {
            return;
        }
        for (id, x, y, w, h) in rows {
            self.gadgets.add_box(id, x, y, w, h, "");
        }
        // `AddClickSound`, which every one of the original's gadget handlers
        // calls the moment a gadget's action is taken. See [`CLICK_SOUND`].
        if self.pressed[6] || self.pressed[11] || self.pressed[12] {
            self.audio.play(CLICK_SOUND);
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
        let Some(over) = self.gadgets.hit_id(&self.pointer) else {
            return;
        };
        // Being over a row is being on it, the way `GadgetHit` says the line
        // for whatever the pointer has reached.
        if self.mode == Mode::Place {
            if let Some(s) = self.visiting.as_mut() {
                s.visit.cursor = over;
            }
        }
        // And fire over it takes it, through the same edge every menu reads.
        if self.pressed[11] {
            self.pressed[6] = true;
        }
    }

    /// One line to say something happened, in the box the original says things
    /// in.
    ///
    /// This used to be a line on a plate in the corner of the map, beside a
    /// purse. The original has neither: `MAP.CMP` is one 320x200 picture and
    /// nothing is drawn over it but the tokens, the lairs and the paper. What it
    /// does have is `OCCURMESSAGE`, a chain over `MESSAGE.PIV` that is modal
    /// until fire clears it, so a one-line chain at y 95 is where a line goes.
    fn notice(&mut self, line: impl Into<String>) {
        use henge_core::message::{Kind, Line, Message, Until, FLAG_CENTRE};
        self.show_message(Message {
            kind: Kind::Occurrence,
            until: Until::Fire,
            lines: vec![Line::new(&line.into(), 0, 95, FLAG_CENTRE)],
        });
    }

    /// One tick of the status screen, which is `StatLOOP` at `0xbe13`:
    /// `MovePointer`, `CHECKGADGET`, and `HotGadget` on the one under the
    /// pointer when fire goes down.
    ///
    /// **`HotGadget` at `0xca0a`, transcribed.** Id 7 is the way out and the
    /// three pillars all carry it; otherwise the payload's low nibble decides,
    /// and what each arm does here is what the arm does there:
    ///
    /// * 3 `HGAbility`: spend experience, and only while the `0x40` bit is on,
    ///   which `SetUpID` sets when `[ARR+18]` is no more than the knight's
    ///   experience and the value is under five.
    /// * 5 `HGCastMagic` into `MagicCast`, which is `dec byte [bx+si]` and then
    ///   the field's own routine. `Run::cast` is that family.
    /// * 1 `HGTakeMagic` and 2 `TakeGold`: both move a thing from one record to
    ///   another, and `HotGadget` only reaches the second at all through
    ///   `test ax, 0x20`, the right arch's permission bit. One traveller on this
    ///   map means there is no other record, so both do nothing, which is also
    ///   what they do in the original with one knight in the game.
    /// * 0xa `BuyGoods`: the merchant's page, `Run::buy_goods`.
    /// * On `StatTYPE` 6, `HGCastMagic` (0xca83) and `HGTakeMagic` (0xcbaa)
    ///   both `cmp word ptr [StatTYPE], 6; je TTemple` before any permission
    ///   bit is read, so every magic gadget on the temple's page is a trade:
    ///   `Run::trade_at_temple`, which divides the screen at the pointer's x.
    fn sheet_tick(&mut self) {
        if !self.takes() {
            return;
        }
        let Some(hit) = self
            .gadgets
            .hit(self.pointer.x, self.pointer.y)
            .filter(|_| self.pointer.woken)
            .cloned()
        else {
            return;
        };
        // `cmp word ptr es:[si + 0xe], 7`, the first thing the routine tests:
        // `AddClickSound` and `ExitFLAG = 1`, which ends `StatLOOP`.
        if hit.id == henge_core::status::EXIT_ID {
            self.audio.play(CLICK_SOUND);
            if self.door_panel().is_some() {
                // `StatusDone`, and the rung's `jmp HWINIT`.
                self.close_door();
            } else if self.lair_page.is_some() {
                self.close_lair_page();
            } else if self.trade_page.is_some() {
                self.close_trade_page();
            } else if self.dragon_page {
                self.dragon_page = false;
            } else if let Some(seat) = self.wyrm_picker {
                self.close_wyrm_picker(seat);
            } else {
                self.sheet = false;
            }
            return;
        }
        // `HotGadget` (0xca1b) recognises `NEXT` by its own record rather
        // than by id (`status::NEXT_ID` is not a real gadget id, only the
        // marker this crate matches on): the only page it can appear on is
        // the one `wyrm_picker` is up for.
        if hit.id == status::NEXT_ID {
            if let Some(seat) = self.wyrm_picker {
                self.audio.play(CLICK_SOUND);
                self.wyrm_picker = Some(henge_core::dragon::next_wyrm_seat(
                    seat,
                    self.run.knight.seat,
                ));
            }
            return;
        }
        let Some(op) = hit.op() else { return };
        // `TTemple`: `cmp word ptr [PointerX], 0xa0; jl SellToTemple`.
        if self.door_panel() == Some(SheetScreen::Temple) && matches!(op, Op::Cast | Op::TakeMagic)
        {
            let moved = self.run.trade_at_temple(
                hit.payload.field,
                hit.mask(),
                self.pointer.x,
                &self.items,
            );
            if moved {
                self.audio.play(CLICK_SOUND);
                self.sync_sheet();
            }
            return;
        }
        // `BuyGoods` at 0xcd33, on whichever page carries a 0xa gadget,
        // which is the merchant's alone.
        if op == Op::Buy {
            if self
                .run
                .buy_goods(hit.payload.field, hit.mask(), &self.items)
            {
                self.audio.play(CLICK_SOUND);
                self.sync_sheet();
            }
            return;
        }
        // The lair's page. `HotGadget` reaches `HGTakeMagic` for `STRP` 1 and
        // `TakeGold` for the rest through `test ax, 0x20`, and `HGCastMagic`
        // at 0xca8d falls into `HGTakeMagic` itself when the array carries no
        // `0x10`, which the right arch's `Take` does not. So every lit gadget
        // on this page is a take, whatever its low nibble says.
        if let Some(page) = self.lair_page {
            if !hit.lit {
                // `Identify` has no permission bit: a lair seen from the air.
                return;
            }
            let moved = match op {
                Op::Take => self.run.take_lair_gold(page.lair) > 0,
                Op::TakeMagic | Op::Cast => {
                    self.run
                        .take_from_lair(page.lair, hit.payload.field, &self.items)
                }
                Op::Raise | Op::Buy => false,
            };
            if moved {
                // `HGTakeDone`: `inc [TakeCNT]`, the knight's numbers redone
                // and `ReDisplay`.
                self.audio.play(CLICK_SOUND);
                self.sync_sheet();
            }
            return;
        }
        // The trade page. Gold, the weapon and the armour all carry `STRP`
        // 2 off `display_knight`, the rest of the magic record `STRP` 1 or
        // 5, and every one of them reaches `Run::trade_take` off its own
        // `STPL`; only the left arch, the winner's own sheet, is `Identify`
        // and so never lit. `Op::Raise`/`Op::Buy` are the ability, dagger
        // and life-point icons `display_knight` also draws on this side,
        // which `HotGadget`'s own decode has no case for either.
        if let Some((loser, _)) = self.trade_page {
            if !hit.lit {
                return;
            }
            let moved = match op {
                Op::Take | Op::TakeMagic | Op::Cast => {
                    self.run.trade_take(loser, hit.payload.field, &self.items)
                }
                Op::Raise | Op::Buy => false,
            };
            if moved {
                self.audio.play(CLICK_SOUND);
                self.trade_took(loser);
                self.sync_sheet();
            }
            return;
        }
        // The dragon's hoard, `StatTYPE` 0xa: `Screen::right_table` marks
        // its gadgets `Take` the same as the lair's floor, but `WhoLived`
        // put the whole hoard into `dragon_hoard` in one write and nothing
        // here reads it back out gadget by gadget. TODO: a take mechanic of
        // its own, the lair page's shape, once one is wanted; until then
        // `EXIT`, above, is the only thing this page does.
        if self.dragon_page {
            return;
        }
        let Some(slot) = henge_core::status::slot_of(hit.id) else {
            return;
        };
        match op {
            Op::Raise => {
                if !hit.lit {
                    return;
                }
                // The three slots are strength, constitution and endurance, in
                // that order: `SetUpStatus` writes `ab1`, `ab3`, `ab2`.
                let a = match slot {
                    0 => Ability::Strength,
                    1 => Ability::Constitution,
                    _ => Ability::Endurance,
                };
                if self.run.spend_experience(a, &self.items) {
                    self.audio.play(CLICK_SOUND);
                    self.notice(format!("{} {}", a.name(), self.run.knight.ability(a)));
                    self.sync_sheet();
                }
            }
            Op::Cast => {
                // `HGCastMagic` 0xca8d: `test ax, 0x10; jne` and otherwise
                // into `HGTakeMagic`, which wants `0x20` and has no other
                // record to take from. `Identify` carries neither bit, so a
                // potion on the merchant's page is looked at and not drunk.
                if !hit.lit {
                    return;
                }
                let Some(id) = henge_core::status::SLOT_TABLE[slot].item else {
                    return;
                };
                self.audio.play(CLICK_SOUND);
                let cast = self.run.cast(id, &self.items);
                self.acted(cast);
            }
            // Nothing on one knight's own sheet: see above.
            Op::TakeMagic | Op::Take | Op::Buy => {}
        }
    }

    /// The page of the panel an open door is, if the door is one of the two
    /// that are pages: `MERC` and `HTEM`.
    fn door_panel(&self) -> Option<SheetScreen> {
        self.door.as_ref().and_then(|d| d.panel())
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
                    self.flight = Some(Flight {
                        returns,
                        from: (m.state.x, m.state.y),
                    });
                    self.sheet = false;
                    self.notice(if returns {
                        "Aloft on the gem"
                    } else {
                        "Aloft on the hawk"
                    });
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
            // `InitAKnight`/`NextKnight` (0xc90d/0xc913): the picker's page
            // opens on this seat, the caster's own already skipped over.
            // Nothing is committed yet; that is `close_wyrm_picker`,
            // `StatusDone` (0xbe57), once `EXIT` closes the page.
            Cast::Wyrm { seat } => {
                self.wyrm_picker = Some(seat);
            }
            Cast::Pointless => self.notice("Nothing comes of it"),
            Cast::HaveNone | Cast::Unknown => {}
        }
    }

    /// A tick aloft. The token moves where it is steered, a pixel a tick,
    /// inside `HawkBorders`, and fire is `_MAP:ScrollINPUT` at 0xa3c6:
    ///
    /// ```text
    /// a3c6  mov ax, [JOYS]
    /// a3c9  test ax, 0x10; je DragonEncounter    ; no fire, fly on
    /// a3ce  cmp word ptr [EffectFLAG+2], 0       ; the hawk
    /// a3d3  je 0xa3d8
    /// a3d5  call 0xa962                          ; inside RESTOREGEM, past
    ///                                            ; the two words that put the
    ///                                            ; position back: the hawk
    ///                                            ; lands where it is
    /// a3d8  call DisplayStack
    /// ```
    ///
    /// and `DisplayStack` at 0xae45 sends a stack of more than one to
    /// `GEMEncounter` while the gem's flag is up, which walks the stack for the
    /// entry of type 2 and hands it to `StackDecision`. **So a gem flight ends
    /// nowhere but at a lair**: `RESTOREGEM` has one caller, `LairGEM+19`, and
    /// fire over open ground does nothing at all.
    fn fly(&mut self, dx: i32, dy: i32) {
        use henge_core::overworld::{MAX_X, MAX_Y};
        let landing = self.takes();
        let Some(m) = self.map.as_mut() else {
            self.flight = None;
            return;
        };
        m.state.x = (m.state.x + dx).clamp(0, MAX_X);
        m.state.y = (m.state.y + dy).clamp(0, MAX_Y);
        if !landing {
            return;
        }
        match self.flight {
            // `a3d5`: the hawk's three flags down, and no `GemXY` written
            // back, so it comes down where it is.
            Some(fl) if !fl.returns => self.flight = None,
            // `GEMEncounter` at 0xae81: the lair under the token, or nothing.
            Some(_) => {
                if let Some(lair) = self.lair_under_the_token() {
                    self.open_lair_page(lair, true);
                }
            }
            None => {}
        }
    }

    /// `GEMEncounter`'s `cmp word ptr ds:[bp+4], 2`: the entry of the stack
    /// that is a lair, which is the only kind a gem flight can land on.
    fn lair_under_the_token(&self) -> Option<usize> {
        self.overlaps.ids().into_iter().find_map(|id| {
            self.places
                .get(id)?
                .options
                .iter()
                .find_map(|c| match c.effect {
                    henge_core::place::Effect::Raid { lair, .. } => Some(lair),
                    _ => None,
                })
        })
    }

    /// The four knight records' tokens as the map draws them this frame:
    /// `DisplayOtherKnights` (0xa22c) for every record but `[0x77e8]`, in
    /// its colour's frame, 0x21 for a grave and `+0x2b` for a toad; and
    /// `SHOW` (0xa1f0) for `[0x77e8]`, whose colour the scene adds five to.
    fn map_knights(&self, m: &MapScene) -> (Vec<KnightMark>, KnightMark) {
        let which = self.run.which;
        let mut others = Vec::new();
        let mut shown = (m.state.x, m.state.y, self.run.knight.seat);
        for record in 0..henge_core::rival::RECORDS {
            let (x, y, colour, alive, toad) = if record == 0 {
                (
                    m.state.x,
                    m.state.y,
                    self.run.knight.seat,
                    !self.run.over,
                    self.run.is_toad(),
                )
            } else {
                match self.run.rivals.get(record - 1) {
                    Some(r) => (r.x, r.y, r.knight.seat, r.alive(), r.toad > 0),
                    None => continue,
                }
            };
            if record == which {
                shown = (x, y, colour);
                continue;
            }
            // 0a247  cmp byte [si+0x31], 0; jg; mov ax, 0x21
            // 0a252  cmp byte [si+0x3a], 0; je; add ax, 0x2b
            let frame = if !alive {
                map::GRAVE_FRAME
            } else if toad {
                colour + map::TOAD_FRAME
            } else {
                colour
            };
            others.push((x, y, frame));
        }
        (others, shown)
    }

    /// One frame of `DragonWander` (0xa66b) and the shadow table
    /// `CheckEncounterDone+128` (0x816) rebuilds, on the player's turn, for
    /// the token where it stands now and the other three where their turns
    /// left them.
    fn dragon_frame(&mut self) {
        if let Some(m) = self.map.as_ref() {
            let at = (m.state.x, m.state.y);
            self.run.dragon_frame(at);
        }
    }

    /// The routine at 0xcf6, which `DragonEncounter+54` (0xa41b) calls: the
    /// glows taken down, `InitKnightvsDragon`, `InitCombat`. The ground is
    /// the map square's, as `SetUpDKL` loads it for every fight.
    fn begin_dragon_fight(&mut self) {
        self.sync_sheet();
        let family = self.map.as_ref().map_or_else(
            || "forest".to_string(),
            |m| m.last_terrain.family().to_string(),
        );
        if let Some(w) = self.world.as_mut() {
            let pick = self.run.next_arena(&family, w.rotation_len(&family));
            w.set_player_health(self.run.health_for_fight());
            w.set_player_daggers(self.run.knight.daggers);
            if !w.set_foe("dragon") {
                return;
            }
            w.set_family(&family, pick);
            self.dragon_fight = true;
            self.mode = Mode::Combat;
        }
    }

    /// `Combat+102` (0x3b7) up to `InitKnightBattle`: `attacker` is `si` and
    /// `defender` is `di`, record indices 0 to 3, one of which is always the
    /// human (`Combat+159`, 0x3f0, sends two computer knights straight to
    /// `HeadBattleDone` before `FindKnight` can ever name one for the other,
    /// so [`Challenged::Nothing`] cannot actually arise off either caller of
    /// this).
    ///
    /// `Combat+60` (0x38d) is `mov dx, [0xccac]; mov [0xcc98], dx`, on the
    /// way in and ahead of every branch below, so the player's own entry
    /// spends his day whatever the challenge comes to; a computer knight's
    /// own entry is through `BKCollision+85` (0xab06), past `Combat+60`
    /// entirely, because `BKCollision` has already spent his day itself
    /// (0xab0f) before making this call. [`Self::spend_day`] is
    /// `EncounterAllDone`, the same move [`Self::end_turn`]'s caller uses to
    /// end a toad's turn.
    fn knight_fight(&mut self, attacker: usize, defender: usize) {
        if attacker == 0 {
            self.spend_day();
        }
        match self.run.challenge(attacker, defender) {
            // `Combat+159` (0x3f0): nothing happens, and whoever's day this
            // was has already had it spent, above or in `bk_collision`.
            // `KnightProtection` turned him away; likewise nothing further.
            Challenged::Nothing | Challenged::Averted => {}
            // `Knight1Won` entered directly at 0x3d5 or 0x3cc: the defender
            // never fought, so there is a `Settled` with no bout behind it.
            Challenged::Walkover => {
                let settled = self.run.walkover(attacker, defender, &self.items);
                self.knight_fight_settled(Some(settled));
            }
            Challenged::Fight { cursed } => self.begin_knight_fight(attacker, defender, cursed),
        }
    }

    /// `InitKnightBattle` (0x440): the arena set up for a real bout between
    /// two knight records, on the ground the map square gives every fight
    /// (`SetUpDKL`), exactly as [`Self::begin_dragon_fight`] does for the
    /// dragon.
    fn begin_knight_fight(&mut self, attacker: usize, defender: usize, cursed: bool) {
        self.sync_sheet();
        // Whichever of the two is not the human: `Combat+115` (0x3c4) wrote
        // both into `[0x8979]`/`[0x897b]`, and one of them always is, per
        // `knight_fight`'s own doc.
        let rival_index = if attacker == 0 { defender } else { attacker };
        let sheet = self
            .run
            .rivals
            .get(rival_index.wrapping_sub(1))
            .map(|r| RivalSheet {
                health: r.health,
                max_health: r.max_health,
                bonus: r.knight.damage_bonus(&self.items),
                daggers: r.knight.daggers,
                talismans: i32::from(r.hoard.talismans),
                challenger: attacker != 0,
            });
        let family = self.map.as_ref().map_or_else(
            || "forest".to_string(),
            |m| m.last_terrain.family().to_string(),
        );
        if let Some(w) = self.world.as_mut() {
            let pick = self.run.next_arena(&family, w.rotation_len(&family));
            w.set_player_health(self.run.health_for_fight());
            w.set_player_daggers(self.run.knight.daggers);
            // `KnightProtection`'s backfire: the caster's controls reversed
            // for this one bout, which [`Run::finished_fight_worth`] (called
            // from [`Self::knight_fight_settled`]'s way in, through
            // `Run::knight_fight_over`) already clears when it settles.
            w.set_player_cursed(cursed);
            w.set_rival(sheet);
            if !w.set_foe("knight") {
                return;
            }
            w.set_family(&family, pick);
            w.set_seats(self.title.state.players.max(1), 1);
            self.duel = Some((attacker, defender));
            self.mode = Mode::Combat;
        }
    }

    /// `Knight1Won` (0x465) and `BothKnightsDied` (0x48d), once a knight
    /// against knight challenge has settled, whether or not a bout was
    /// fought for it: [`Challenged::Walkover`] settles at once, and
    /// [`Challenged::Fight`] settles on the tick its bout ends, in the
    /// combat tick's own cleanup right above where this is called.
    fn knight_fight_settled(&mut self, settled: Option<Settled>) {
        match settled {
            // `BothKnightsDied`: both records' own bookkeeping already ran,
            // inside `Run::knight_fight_over`. Nothing for a person to do.
            None | Some(Settled::BothDied) => {
                if self.map.is_some() {
                    self.mode = Mode::Map;
                }
            }
            // `Knight1Won+6` (0x46b): a person takes through the trade
            // page, `StatTYPE` 1, and nothing is taken for him. It opens
            // with `TakeCNT` clear; what happens once it is not is
            // `HotGadget`'s own affair, in `sheet_tick`.
            Some(Settled::PlayerWon { loser }) => {
                self.trade_page = Some((loser, false));
                self.mode = Mode::Map;
                // `StatusSetup` opens with the pointer at (0xa0, 0x64).
                self.point_at(0xa0, 0x64);
            }
            // `BKwon`: the computer knight already took his point and his
            // loot, inside `Run::bk_won`/`Run::walkover`. `BKAddstuff` and
            // `WhoLived+57` show nothing on screen for it, so neither does
            // this.
            Some(Settled::RivalWon { .. }) => {
                if self.map.is_some() {
                    self.mode = Mode::Map;
                }
            }
        }
    }

    /// The routine at 0xcf3/0xcf6 on its way out, once a dragon fight this
    /// session began (`begin_dragon_fight`) has settled: `won` is the same
    /// `w.bout.winner() == Some(0)` [`Self::knight_fight_settled`]'s own
    /// caller reads.
    ///
    /// ```text
    /// 00d1b  test word [KnightDeath], 1; je 00d35
    /// _dragon_won:
    /// 00d23  mov si, 0x6e26; mov di, [KnightTable]; call 0xaf7   ; taken
    /// 00d2d  or word [KnightDeath], 1
    /// 00d32  jmp EncounterAllDone
    /// 00d35  mov si, 0x6e26; mov byte [si+0x31], 0xff            ; dead
    /// 00d3c  mov si, [KnightTable]; add word [si+0x36], 2        ; 2 xp
    /// 00d44  mov ax, 0xa; call 0xbdd3       ; the panel on StatTYPE 0xa
    /// 00d4a  mov word [0xccb0], 0; 00d50 mov word [0xccb2], 0xffff
    /// 00d56  jmp EncounterAllDone
    /// ```
    ///
    /// A dragon lost to keeps flying and `ContinueDragon` rolls again next
    /// turn (`Flight::fight_over`'s own `false` arm, which does nothing);
    /// nothing here takes a life point off the player for it, because
    /// `finished_fight_worth`, called above off the same `won`, already
    /// has. One thing missing: `WhoLived+57` (0xaf7) at `_dragon_won` takes
    /// one thing off the loser into the winner's magic record whichever
    /// kind lost, the same call `Rival::dragon_on_rival` already makes for
    /// a computer knight, but nothing here makes the equivalent call for a
    /// human losing this same bout, so `dragon_hoard` does not grow on a
    /// loss the way it does when a computer knight loses to the dragon on
    /// the map. TODO: build that call once a lost dragon fight is wanted to
    /// cost the loser a magic item or a suit of armour, the way it already
    /// does for a rival.
    fn dragon_fight_settled(&mut self, won: bool) {
        self.run.dragon.fight_over(won);
        if won {
            // `Knight1Won+6` (0x46b) opens the trade page on `StatTYPE` 1
            // the same way; `StatusSetup` opens either with the pointer at
            // (0xa0, 0x64).
            self.dragon_page = true;
            self.point_at(0xa0, 0x64);
        }
        if self.map.is_some() {
            self.mode = Mode::Map;
        }
    }

    /// Close the trade page and go back to the map. `Knight1Won`'s trade
    /// page has no routine of its own the way `LairGEM+6` does for a lair's:
    /// what a gadget took has already been written back, gadget by gadget,
    /// so there is nothing left to settle on the way out.
    fn close_trade_page(&mut self) {
        self.trade_page = None;
    }

    /// `StatusDone`, image 0xbe57, run by `EXIT` closing the Wyrm picker:
    /// the seat last highlighted becomes the dragon's own target. See
    /// [`henge_core::dragon::Flight::wyrm_picked`] for why nothing here
    /// spends a charge or sets the dragon flying — both already happened,
    /// the first at `Op::Cast`, the second whenever `ContinueDragon` next
    /// runs on somebody's turn.
    fn close_wyrm_picker(&mut self, seat: usize) {
        self.run.dragon.wyrm_picked(seat);
        self.wyrm_picker = None;
        let name = self
            .knights
            .get(seat)
            .map_or_else(|| "a knight".to_string(), |k| k.name.clone());
        self.notice(format!("The dragon is after {name}"));
    }

    /// `HGTakeDone`: `inc [TakeCNT]`, then `call ReDisplay` (0xbe6b) at
    /// once. `ReDisplay+0x73` (0xbeda) reads `[StatHAND2+0x31]`, the
    /// loser's own lives, before it ever looks at `TakeCNT`: a dead one (a
    /// grave) has `TakeCNT` forced back to nought right there (0xbee0), so
    /// the `cmp [TakeCNT], 0` at 0xbee6 always finds it clear and the trade
    /// page simply redraws for another take. A loser still standing does
    /// not get that reset, so the same compare finds `TakeCNT` set and
    /// takes `StatTYPE = SaveTYPE = 9` (0xbeed): the page becomes the
    /// winner's own plain sheet, single arch, and does not come back. One
    /// thing taken from a body still breathing is all a person is shown.
    fn trade_took(&mut self, loser: usize) {
        if self
            .run
            .knights_alive()
            .get(loser)
            .copied()
            .unwrap_or(false)
        {
            self.trade_page = None;
            self.sheet = true;
        } else {
            self.trade_page = Some((loser, true));
        }
    }

    /// `MapLOOP+19` (0xa319) round to `NextWHICH`, for whichever computer
    /// knight's turn `self.run.which` names: one frame of
    /// [`Run::rival_frame`], and then whatever the frame came to.
    ///
    /// `going` is `SlowDELAY`, "one word for everybody", which is
    /// [`henge_core::overworld::Overworld::going_counter`] — the same
    /// counter the player's own walk uses, not a second one. It is copied
    /// out and back in rather than borrowed straight through, because
    /// [`Board::land`] has to borrow `self.map` for the whole call and a
    /// live borrow of the same field cannot also be taken mutably for the
    /// counter.
    fn rival_tick(&mut self) {
        let Some(m) = self.map.as_ref() else { return };
        let land = m.land();
        let player_at = (m.state.x, m.state.y);
        let mut going = m.state.going_counter;
        let board = Board {
            land,
            places: &self.places,
            items: &self.items,
            player_at,
        };
        let frame = self.run.rival_frame(&board, &mut going);
        if let Some(m) = self.map.as_mut() {
            m.state.going_counter = going;
        }
        match frame {
            // The map's own drawing reads `Run::positions` fresh every
            // frame, so a pixel walked needs nothing further here.
            Frame::Walked => {}
            // `GoTheDistance` (0xa422): the day is spent.
            Frame::TurnOver => self.end_turn(),
            // `BKCollision+40` (0xaad9) and `+117` (0xab26): the day is
            // spent and, per their own doc comments in `rival.rs`, the
            // original shows nothing on screen for either.
            Frame::Shopped | Frame::AtLair => self.end_turn(),
            // `DragonEncounter+54` (0xa41b) into 0xcf3: the day is spent,
            // and a computer knight's own brush with the dragon has no
            // fight or animation the way the player's does.
            Frame::Dragon(_loot) => self.end_turn(),
            // `BKCollision+85` (0xab06): standing on the knight he is
            // after. `self.run.which` is captured before the call because
            // `knight_fight` can enter a real bout, and nothing past this
            // point should read `which` assuming it is still the same seat.
            Frame::Challenge { target } => {
                let attacker = self.run.which;
                self.knight_fight(attacker, target);
            }
        }
    }

    /// `NextWHICH` (0xa434): the next living, non-toad seat's turn begins,
    /// and the between-days screen goes up first when the four have had
    /// theirs. `None` is a lone human player dead, which the top of the
    /// map's own tick already catches on `Run::ending` and needs nothing
    /// further here.
    fn end_turn(&mut self) {
        if let Some(begins) = self.run.next_which() {
            if begins.day_turned {
                self.begin_interlude();
            }
        }
    }

    /// The dragon over the map, drawn after the tokens the way its task is
    /// stepped after `SHOW`: `DrAnim[DR_WALK]` on `DrBuffer`, at `DR_X`,
    /// `DR_Z`, facing `DR_DIR`.
    fn draw_map_dragon(&mut self) {
        if !self.run.dragon.aloft {
            return;
        }
        let Some(banks) = self.dragon_banks.as_ref() else {
            return;
        };
        let d = &self.run.dragon;
        let mut task = henge_core::taskvm::Task::new(d.script(), d.x, d.z, d.dir);
        task.table = 5;
        let mut record = henge_core::taskvm::TaskActor::default();
        task.step(&self.scripts, &mut record, false);
        shell::draw_task(&mut self.reg, &mut self.fb, &task, banks);
    }

    fn draw_flight(&mut self) {
        let Some(fl) = self.flight else { return };
        let Some(m) = self.map.as_ref() else { return };
        let (x, y) = (m.state.x, m.state.y);
        let seat = self.run.knight.seat % 4;
        let frame = if fl.returns { 10 + seat } else { 15 + seat };
        let Some(rect) = self
            .reg
            .sheet("bank.mi")
            .and_then(|r| r.value.frames.get(frame).copied())
        else {
            return;
        };
        let Ok(img) = self.reg.image("bank.mi") else {
            return;
        };
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

    /// A day has turned over. Put the moon up.
    ///
    /// `WaitCOUNT` is not stepped here. The only `inc word ptr [WaitCOUNT]` in
    /// the image is at 0x8ecf, inside `WAITMESSAGE`, and the between-days
    /// routine at 0x8e5b walks `NextDayMes` and never calls it; a step here
    /// was left over from when the screen showed one of the fourteen.
    fn begin_interlude(&mut self) {
        self.interlude = 1;
        self.interlude_out = 0;
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

    /// Every lair the token is standing on, which takes `MI.C` frame 0x1f on
    /// top of its own: `MOON:CheckLairEncounter` blits it at the lair's corner
    /// before it pushes the entry, image 0x88f.
    fn lairs_here(&self) -> Vec<(i32, i32)> {
        self.overlaps
            .ids()
            .into_iter()
            .filter_map(|id| self.places.get(id))
            .filter(|d| d.icon == Some(map::LAIR_FRAME))
            .map(|d| (d.x, d.y))
            .collect()
    }

    /// `_MAP:DisplayStack`, image 0xae27.
    ///
    /// It counts the live slots of the five-entry stack at `DS:043c` and does
    /// one of three things: none and it returns zero, so the map carries on
    /// (`NoEncounter`, 0xae7e); one and it falls straight into `StackDecision`
    /// (`cmp ax, 1; je`, 0xae40) and enters it with nothing drawn; more and it
    /// draws `CreatePaper` and sits in its key loop. Returns whether the map
    /// gave way to something, which is the `or ax, ax` `ScrollINPUT` tests.
    /// `EncounterAllDone` (0x113e): `mov ax, [0xccac]; mov [0xcc98], ax`, the
    /// rest of the day's distance spent. `[0xccac]` is refreshed first, as
    /// `TakingMoon+64` (0xc92) calls `DistanceDONE+12` before it dispatches
    /// an icon and the map entry at 0xa2d7 calls it before every frame, so
    /// the budget a visit spends is the one the knight's stride gives today.
    fn spend_day(&mut self) {
        let budget = self.run.day_steps(&self.items);
        if let Some(m) = self.map.as_mut() {
            m.state.steps_per_day = budget;
            m.state.end_turn();
        }
    }

    fn display_stack(&mut self) -> bool {
        if self.overlaps.is_empty() {
            return false;
        }
        if let Some(entry) = self.overlaps.only_entry().cloned() {
            return self.stack_decision(&entry);
        }
        self.paper = true;
        true
    }

    /// `_MAP:StackDecision`, image 0xae9f: kind 1 or 0x21 to `Combat+102`
    /// (0x3b7), the knight fight or the grave; everything else to its place.
    ///
    /// Both kinds call `knight_fight` alike, and that is already right: a
    /// grave is `alive: false` and `Run::challenge` reads exactly that
    /// (`003d5` in its own doc comment) to answer `Challenged::Walkover`
    /// without a blow struck, which is `_MAP:StackDecision`'s "the grave"
    /// arm. There is no separate pillage path to add.
    fn stack_decision(&mut self, entry: &henge_core::place::Entry) -> bool {
        match entry {
            henge_core::place::Entry::Place(id) => {
                let id = id.clone();
                self.enter(&id)
            }
            henge_core::place::Entry::Knight { record, .. } => {
                self.knight_fight(0, *record);
                true
            }
        }
    }

    /// The other three records' tokens, for `CheckEncounterDone`.
    fn knight_tokens(&self) -> Vec<henge_core::place::KnightToken> {
        self.run
            .rivals
            .iter()
            .enumerate()
            .map(|(n, r)| henge_core::place::KnightToken {
                record: n + 1,
                x: r.x,
                y: r.y,
                alive: r.alive(),
            })
            .collect()
    }

    /// The paper's key loop: `call 0x8149` until the scan code is one of the top
    /// row's `1` to `9` and names a live slot, image 0xae58.
    ///
    /// There is no way out of it in the original and there is none here: the
    /// loop has exactly one exit and it is a number that names something under
    /// your feet. Every entry on the stack is a place with its own way out, so
    /// the paper can never strand you.
    fn paper_tick(&mut self) {
        for n in 1..=9u32 {
            if !self.pressed[NUMBER_SLOT + n as usize - 1] {
                continue;
            }
            let Some(entry) = self.overlaps.answer_entry(n).cloned() else {
                continue;
            };
            self.paper = false;
            self.stack_decision(&entry);
            return;
        }
    }

    /// Walk into a place and open its menu.
    ///
    /// A village has no menu and no picture: `_MAP:StackDecision` hands its kind
    /// to `TakingMoon`, which jumps frames 0x15 to 0x18 straight to
    /// `ForestVillage` (0xc99 to 0xcb6, all four to 0x112a), and that gives the
    /// life point and returns through `ColourStatus` to the map. Nothing on the
    /// path loads a backdrop, so this does not change screens either.
    fn enter(&mut self, id: &str) -> bool {
        let Some(def) = self.places.get(id) else {
            let known: Vec<&str> = self.places.keys().map(String::as_str).collect();
            eprintln!("no place called {id}. The pack has: {}", known.join(", "));
            return false;
        };
        if let Some(henge_core::place::Effect::Village { said, refused }) =
            def.options.first().map(|o| &o.effect)
        {
            let (said, refused) = (said.clone(), refused.clone());
            let line = if self.run.rest_at_village() {
                said
            } else {
                refused
            };
            self.notice(line);
            // `EncounterDone` (0x1138) runs on into `EncounterAllDone`
            // (0x113e), which spends the rest of the day's distance.
            self.spend_day();
            return true;
        }
        let home = def.pointer;
        match place::PlaceScene::open(&mut self.reg, def, id) {
            Ok(scene) => {
                self.visiting = Some(scene);
                self.mode = Mode::Place;
                self.door = None;
                // `HWINIT` and `WDINIT` write the pointer before the gadgets.
                if let Some([x, y]) = home {
                    self.pointer.x = x;
                    self.pointer.y = y;
                }
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
        // The name typing is the one screen that reads letters, and the driver
        // has one character per press and no modifiers, so while it is up an
        // uppercase letter or a digit is that character and `<` is the
        // backspace. Fire and Enter still end it, as `ScanKEYS` does.
        if self
            .select
            .as_ref()
            .is_some_and(|s| s.state.typing.is_some())
        {
            match c {
                Some(ch @ ('A'..='Z' | '0'..='9')) => {
                    self.typed = Some(ch);
                    return;
                }
                Some('<') => {
                    self.pressed[BACKSPACE_SLOT] = true;
                    return;
                }
                _ => {}
            }
        }
        match c {
            Some('u') => self.pressed[0] = true,
            Some('d') => self.pressed[1] = true,
            // Fire is held as well as pressed, so that in an arena it swings
            // and in a menu it takes.
            Some('s') => {
                self.keys[6] = true;
                self.pressed[6] = true;
            }
            // The numpad chords: a direction held with fire. The same character
            // also presses the number key of that digit, which is what the map's
            // paper is answered with: a real keyboard has two keys for a digit
            // and this driver has one character, and the two cannot collide
            // because the chord holds fire without pressing it and the paper is
            // modal over the walking the chord would do.
            Some(c @ '1'..='9') => {
                let n = c as u8 - b'0';
                self.keys[6] = true;
                self.keys[0] = matches!(n, 7..=9);
                self.keys[1] = matches!(n, 1..=3);
                self.keys[2] = matches!(n, 1 | 4 | 7);
                self.keys[3] = matches!(n, 3 | 6 | 9);
                self.pressed[NUMBER_SLOT + n as usize - 1] = true;
            }
            // 'e' is Enter, so the headless driver can prove Enter takes a menu
            // option and not only that space does.
            Some('e') => self.pressed[12] = true,
            // 'c' is the sheet, which is the C key on a keyboard here and
            // space on the original's map (`ScrollINPUT` 0xa399: scancode
            // 0x39, then `mov ax, 9` and the panel). Without it a headless
            // recipe cannot reach anything the panel casts, and the gem is
            // cast from there and nowhere else.
            Some('c') => self.sheet = !self.sheet,
            // Seat two's fire, which is the pointer's button: `p` for point.
            Some('p') => {
                self.keys[11] = true;
                self.pressed[11] = true;
            }
            // The test harness's snapshot, written and read. **Not a game
            // feature**: there is no key for either and no menu item, and these
            // two letters are only reachable from `--input` on the command line.
            Some('S') => {
                self.harness_save();
            }
            Some('L') => {
                self.harness_load();
            }
            // Held and pressed both: walking reads the key, a menu reads the
            // edge, and the same letter has to drive either.
            Some('h') => {
                self.keys[2] = true;
                self.pressed[2] = true;
            }
            Some('l') => {
                self.keys[3] = true;
                self.pressed[3] = true;
            }
            Some('k') => {
                self.keys[0] = true;
                self.pressed[0] = true;
            }
            Some('j') => {
                self.keys[1] = true;
                self.pressed[1] = true;
            }
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

    /// The end of a run, won or lost, which in the original is one message and
    /// nothing else.
    ///
    /// `KnightWonGame` hands `VICTORY` to `OCCURMESSAGE` and the routine at
    /// 0x617 hands `GameOverMes` to `INSTRUCTMESSAGE`, so both go over
    /// `MESSAGE.PIV` through the same door every other message in the game uses
    /// and there is no ending screen to draw. A page of seven counted lines over
    /// `bg8.piv` used to be here. `Tally::message` is the chain.
    fn draw_run_over(&mut self) {
        let Some(tally) = self.run.tally() else {
            return;
        };
        let fonts = shell::Fonts {
            bold: self.fonts.get("bold"),
            small: self.fonts.get("small"),
        };
        shell::draw_message(&mut self.reg, &mut self.fb, &fonts, &tally.message());
    }

    /// Draw a line of text over the current frame, for checking the font.
    ///
    /// In the glyphs' own five indices like every other line in the game, so
    /// what this shows is what a screen would show. Over a palette that is not
    /// the font bank's, `text::Font` translates them; the darkest entry behind
    /// the line is this harness's, so a light face over a light picture can
    /// still be read.
    fn say(&mut self, line: &str) {
        let Some(font) = self.fonts.remove("bold") else {
            eprintln!("no font in the packs");
            return;
        };
        let luma = |c: u32| ((c >> 16) & 0xff) * 2 + ((c >> 8) & 0xff) * 3 + (c & 0xff);
        let mut dark = 0usize;
        for i in 1..32 {
            if luma(self.fb.palette[i]) < luma(self.fb.palette[dark]) {
                dark = i;
            }
        }
        let mut y = 30;
        for part in line.split('|') {
            self.fb.rect(0, y - 4, 320, 26, dark as u8);
            font.draw_own_centred(&mut self.reg, &mut self.fb, part, y);
            y += 30;
        }
        self.fonts.insert("bold".into(), font);
    }

    fn save_png(&self, path: &str) -> anyhow::Result<()> {
        let file = std::fs::File::create(path)?;
        let mut enc = png::Encoder::new(
            std::io::BufWriter::new(file),
            SCREEN_W as u32,
            SCREEN_H as u32,
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

    /// `mov ax, 2; call ColourStatus`, which is all three of `LairGEM`'s own
    /// instructions. The panel owns the screen while it is up, so the map is
    /// what is behind it and the place the lair's door was is gone.
    fn open_lair_page(&mut self, lair: usize, scouted: bool) {
        self.lair_page = Some(henge_core::lair::Page { lair, scouted });
        self.visiting = None;
        self.sheet = false;
        self.mode = Mode::Map;
        // `StatusSetup` opens with the pointer at (0xa0, 0x64), which is what
        // `0xbe01` writes into `Address` before `SetUpStatus` runs.
        self.point_at(0xa0, 0x64);
    }

    /// `LairGEM+6` onwards: the panel has left, so `CheckLairClear` runs and
    /// then, if this page was reached from the air, `RESTOREGEM`.
    ///
    /// ```text
    /// 0x05c9  call CheckLairClear
    /// 0x05cf  cmp word ptr [EffectFLAG+4], 0
    /// 0x05d4  je 0x05d9
    /// 0x05d6  call 0xa950            ; RESTOREGEM
    /// ```
    fn close_lair_page(&mut self) {
        let Some(page) = self.lair_page.take() else {
            return;
        };
        // `CheckLairClear`: an emptied, beaten lair leaves the map.
        self.refresh_lairs();
        if !page.scouted {
            return;
        }
        // `RESTOREGEM` at 0xa950: `GemXY` back into the knight's `+0x5c` and
        // `+0x5e`, and the three flags down.
        if let Some(fl) = self.flight.take() {
            if let Some(m) = self.map.as_mut() {
                m.state.x = fl.from.0;
                m.state.y = fl.from.1;
            }
        }
    }

    /// Which page of the panel is up, and what is in its right arch.
    ///
    /// `ColourStatus` is handed one `StatTYPE` and `ReDisplay` branches on it,
    /// so there is one panel and one page of it at a time. The lair's page
    /// wins because `LairGEM` puts it up over whatever the map was doing, the
    /// way every other modal loop in the original does.
    fn panel_now(&self) -> Option<(SheetScreen, Option<status::Other>)> {
        if let Some(screen) = self.door_panel() {
            return Some((screen, None));
        }
        if let Some(page) = self.lair_page {
            let (hoard, gold) = self.run.lair_floor(page.lair);
            return Some((
                SheetScreen::Lair,
                Some(status::Other {
                    hoard,
                    gold,
                    scouted: page.scouted,
                    second: None,
                }),
            ));
        }
        if let Some((loser, _)) = self.trade_page {
            return Some((
                SheetScreen::Trade,
                Some(status::Other {
                    hoard: henge_core::status::Hoard::default(),
                    gold: 0,
                    scouted: false,
                    second: Some(loser),
                }),
            ));
        }
        if self.dragon_page {
            return Some((
                SheetScreen::Dragon,
                Some(status::Other {
                    hoard: self.run.dragon_hoard,
                    gold: 0,
                    scouted: false,
                    second: None,
                }),
            ));
        }
        // `ResetStatus`'s dispatch on `StatTYPE` 0xb: the picker's own page,
        // up over the plain sheet exactly as the trade and dragon pages sit
        // over it, with the highlighted candidate in the other arch.
        if let Some(seat) = self.wyrm_picker {
            return Some((
                SheetScreen::AcquirePair,
                Some(status::Other {
                    hoard: henge_core::status::Hoard::default(),
                    gold: 0,
                    scouted: false,
                    second: Some(seat),
                }),
            ));
        }
        self.sheet.then_some((SheetScreen::Sheet, None))
    }

    /// The scene, and then the character sheet over it if it is up.
    fn render(&mut self) {
        self.draw_scene();
        // The screen has just loaded its own palette, which is the moment the
        // original installs a glow: `MapEffects` and `ChooseKnight` both do it
        // straight after the picture. Anything seeded a frame early is seeded
        // again here rather than breathing between the wrong two colours.
        self.fx.reseed(&self.fb.palette);
        if let Some((screen, other)) = self.panel_now() {
            // The sheet brings its own palette (`_STATUS:STAPAL`), so the ink
            // does not come off whatever was on screen behind it. `ReDisplay`
            // lays the panel out and draws it in one pass; this does the same,
            // off the same routine `gadgets_tick` registered from.
            let small = self.fonts.get("small").or_else(|| self.fonts.get("bold"));
            let bold = self.fonts.get("bold");
            let panel = status::lay_out(&self.run, screen, other.as_ref());
            let said = self.sheet_said.clone();
            status::draw_sheet(
                &mut self.reg,
                &mut self.fb,
                small,
                bold,
                &self.run,
                &panel,
                said.as_deref(),
            );
        }
        // The pointer goes on last, over whatever it is pointing at, and only
        // on a screen that has boxes for it to be over. Behind a town's door
        // it is the only cursor there is, so it is drawn whether or not
        // anyone has steered it yet, which is what `SHOWPOINTER` does.
        if !self.gadgets.is_empty() {
            let p = self.pointer;
            if self.door.is_some() {
                shell::draw_pointer_at(&mut self.reg, &mut self.fb, &p);
            } else {
                shell::draw_pointer(&mut self.reg, &mut self.fb, &p);
            }
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
            let chain = self.messages.named("next.day").cloned();
            shell::draw_interlude(
                &mut self.reg,
                &mut self.fb,
                &fonts,
                self.run.moon.phase(),
                chain.as_ref(),
            );
            return;
        }
        // A message box sits over everything else for the same reason: it is
        // what `WAITMESSAGE`, `OCCURMESSAGE` and `INSTRUCTMESSAGE` do. Ahead
        // of the circle's set piece, because `noswap+31` puts `HengeWait` up
        // before it loads `Hen1.p`.
        if let Some(msg) = self.showing.as_ref() {
            let fonts = shell::Fonts {
                bold: self.fonts.get("bold"),
                small: self.fonts.get("small"),
            };
            let msg = msg.clone();
            shell::draw_message(&mut self.reg, &mut self.fb, &fonts, &msg);
            return;
        }
        // The stone circle's set piece sits over everything, because
        // `HengeLOOP` is a loop of its own with the whole screen to itself.
        if let Some(stones) = self.stones.clone() {
            let banks = self.stones_banks.clone();
            shell::draw_stones(&mut self.reg, &mut self.fb, &stones, banks.as_ref());
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
        if self.mode == Mode::Ending {
            let fonts = shell::Fonts {
                bold: self.fonts.get("bold"),
                small: self.fonts.get("small"),
            };
            let ending = self.ending;
            let cast = self.ending_cast.clone();
            shell::draw_ending(
                &mut self.reg,
                &mut self.fb,
                &fonts,
                &ending,
                cast.as_deref(),
            );
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
        if self.mode == Mode::Online {
            if let Some(screen) = self.online.take() {
                let fonts = shell::Fonts {
                    bold: self.fonts.get("bold"),
                    small: self.fonts.get("small"),
                };
                shell::draw_online(&mut self.reg, &mut self.fb, &fonts, &screen);
                self.online = Some(screen);
                return;
            }
        }
        if self.mode == Mode::Select {
            if let Some(select) = self.select.take() {
                let fonts = shell::Fonts {
                    bold: self.fonts.get("bold"),
                    small: self.fonts.get("small"),
                };
                select.render(&mut self.reg, &mut self.fb, &fonts);
                self.select = Some(select);
                return;
            }
        }
        if self.mode == Mode::Place {
            // Places are drawn in the small font: their menus sit in panels
            // the original painted only a few pixels wide, and the chains the
            // town's counters write carry no bold bit.
            let font = self.fonts.get("small").or_else(|| self.fonts.get("bold"));
            // A door's own screen, over the town. The panel pages are drawn
            // by `render` like every other page of the panel.
            if let Some(open) = self.door.as_ref() {
                if open.panel().is_none() {
                    let banks = self.dice_banks.clone();
                    let hand = |reg: &mut henge_assets::Registry, fb: &mut Framebuffer| {
                        // `TavernLoop` draws the tasks over the picture every
                        // frame, and the hand is the only one on this screen.
                        if let town::Open::Tavern { state, .. } = open {
                            shell::draw_dice_hand(reg, fb, &state.table, banks.as_ref());
                        }
                    };
                    open.render(&mut self.reg, &mut self.fb, font, &self.run, hand);
                    return;
                }
            }
            if let Some(scene) = self.visiting.as_ref() {
                if let Some(def) = self.places.get(&scene.visit.place) {
                    if scene
                        .render(&mut self.reg, &mut self.fb, def, font, &self.pointer)
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
                let icons = self.map_icons();
                let marked = self.lairs_here();
                let names = |n: usize| self.run.record_name(n);
                let lines = self.overlaps.paper_with(&self.places, &names);
                // `DisplayOtherKnights` (0xa22c) for the records that are not
                // `[0x77e8]`, and `SHOW` (0xa1f0) for the one that is.
                let (others, shown) = self.map_knights(&m);
                let marks = map::Marks {
                    icons: &icons,
                    others: &others,
                    shown,
                    marked: &marked,
                    paper: self.paper.then(|| map::Paper {
                        knight: self.run.knight.name.as_str(),
                        seat: self.run.knight.seat,
                        lines: &lines,
                    }),
                };
                let ok = m
                    .render(&mut self.reg, &mut self.fb, &self.fonts, &self.run, &marks)
                    .is_ok();
                self.map = Some(m);
                if ok {
                    self.draw_map_dragon();
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
            // Nothing goes over the arena. The original's fight loop, `Combat`
            // at image 0x351, is ten calls and not one of them draws a readout;
            // the whole of the screen from the tree line to row 199 is ground a
            // fighter can stand on.
            return;
        }
        // Nothing loaded: a slow sweep, so it is obvious the window and timing
        // are alive and the problem is the data.
        self.fb.clear(0);
        for y in 0..SCREEN_H {
            for x in 0..SCREEN_W {
                self.fb.pixels[y * SCREEN_W + x] =
                    ((x + y + (self.tick / 2) as usize) / 8 % 32) as u8;
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
    fn the_developer_keys_are_not_in_the_binding_table() {
        let b = input::Bindings::default();
        // Tab and Enter are gone from this list: they are two of the original's own
        // ten keys now, and `F2` took Tab's developer job for exactly that reason.
        for code in [
            KeyCode::BracketLeft,
            KeyCode::Comma,
            KeyCode::F2,
            KeyCode::KeyC,
            KeyCode::KeyR,
        ] {
            assert!(
                slots_for(&b, code).is_empty(),
                "{code:?} is bound as a control"
            );
        }
        assert_eq!(key_index(KeyCode::BracketLeft), 4);
        assert_eq!(key_index(KeyCode::Enter), 12);
        // And none of them is one of the ten seat slots.
        for code in [
            KeyCode::BracketLeft,
            KeyCode::BracketRight,
            KeyCode::Enter,
            KeyCode::Comma,
            KeyCode::Period,
        ] {
            let i = key_index(code);
            assert!(
                !(0..=3).contains(&i) && !(6..=11).contains(&i),
                "{code:?} took slot {i}"
            );
        }
    }

    /// The map's paper is answered with the top row's `1` to `9`, which is what
    /// `_MAP:DisplayStack` reads: scan codes 2 to 0x0a, minus two for the slot.
    /// They have to be nine slots of their own, clear of the ten the seats own,
    /// or answering the paper would also walk or swing.
    #[test]
    fn the_number_keys_are_nine_slots_of_their_own() {
        let b = input::Bindings::default();
        let digits = [
            KeyCode::Digit1,
            KeyCode::Digit2,
            KeyCode::Digit3,
            KeyCode::Digit4,
            KeyCode::Digit5,
            KeyCode::Digit6,
            KeyCode::Digit7,
            KeyCode::Digit8,
            KeyCode::Digit9,
        ];
        for (n, code) in digits.iter().enumerate() {
            assert_eq!(key_index(*code), NUMBER_SLOT + n, "{code:?} lost its slot");
            assert!(
                slots_for(&b, *code).is_empty(),
                "{code:?} is bound as a control"
            );
            let i = key_index(*code);
            assert!(
                !(0..=3).contains(&i) && !(6..=11).contains(&i),
                "{code:?} took slot {i}"
            );
            assert!(i < 256, "and it has to be a slot that exists");
        }
    }

    #[test]
    fn rebinding_moves_the_slot_a_key_feeds() {
        let mut b = input::Bindings::default();
        let (seat, action, src) =
            input::Bindings::parse_bind("0:fire=Space").expect("a readable spec");
        b.bind(seat, action, src);
        assert_eq!(slots_for(&b, KeyCode::Space), vec![6]);
        // And the key it joined still works, because a list is a list.
        assert_eq!(slots_for(&b, KeyCode::Enter), vec![6]);
    }

    /// The whole of the reader at `0x81ec`, as slots, straight out of the
    /// defaults: ten keys where this test used to check four.
    #[test]
    fn the_originals_own_keys_are_what_ships_and_reach_the_right_slots() {
        let b = input::Bindings::default();
        for (key, slot) in [
            (KeyCode::ArrowUp, 0),
            (KeyCode::ArrowDown, 1),
            (KeyCode::ArrowLeft, 2),
            (KeyCode::ArrowRight, 3),
            (KeyCode::Enter, 6),
            (KeyCode::KeyW, 7),
            (KeyCode::KeyX, 8),
            (KeyCode::KeyA, 9),
            (KeyCode::KeyD, 10),
            (KeyCode::Tab, 11),
        ] {
            assert_eq!(slots_for(&b, key), vec![slot], "{key:?}");
        }
        // And the keys that were ours reach nothing.
        for key in [KeyCode::Space, KeyCode::KeyF, KeyCode::KeyS] {
            assert_eq!(slots_for(&b, key), Vec::<usize>::new(), "{key:?}");
        }
    }

    /// `J` is the options screen's route into `Fix_JoyStick` (`OptionKeys` at
    /// `0x1282`, scancode 0x24), so it has to be a key that arrives somewhere.
    #[test]
    fn j_is_the_calibration_key_and_nothing_else() {
        assert_eq!(key_index(KeyCode::KeyJ), CALIBRATE_SLOT);
        const { assert!(CALIBRATE_SLOT < 256) };
        // Clear of the seats' ten and of the nine numbers.
        const { assert!(CALIBRATE_SLOT > NUMBER_SLOT + 8) };
        let b = input::Bindings::default();
        assert_eq!(slots_for(&b, KeyCode::KeyJ), Vec::<usize>::new());
    }

    /// `J` starts `Fix_JoyStick` from the options screen and from no other, which
    /// is `OptionKeys` at `0x1282` testing scancode 0x24 before anything else.
    #[test]
    fn j_starts_the_calibration_on_the_title_and_nowhere_else() {
        let mut app = match App::new() {
            Ok(a) => a,
            Err(_) => return,
        };
        app.mode = Mode::Title;
        app.pressed[CALIBRATE_SLOT] = true;
        app.title_tick();
        assert!(app.calibrating.is_some(), "J on the title calibrates");
        // And it does not also take whatever the title's cursor was sitting on.
        app.calibrating = None;
        for m in [Mode::Map, Mode::Combat, Mode::Place] {
            app.mode = m;
            app.pressed[CALIBRATE_SLOT] = true;
            app.simulate();
            assert!(app.calibrating.is_none(), "{m:?} has no calibration key");
        }
    }

    /// Escape is the options screen's own quit (`OptionKeys` at `0x128d`,
    /// `StartAgain` at `0x00ba`) and is not a key anywhere else.
    #[test]
    fn escape_quits_from_the_title_and_from_nowhere_else() {
        let mut app = match App::new() {
            Ok(a) => a,
            Err(_) => return,
        };
        app.mode = Mode::Title;
        assert!(app.quits_on_escape());
        for m in [Mode::Map, Mode::Combat, Mode::Place, Mode::Select] {
            app.mode = m;
            assert!(!app.quits_on_escape(), "{m:?}");
        }
    }

    /// A quest's three seats, seated fresh, for the tests below. Not
    /// `take_knight`'s full setup: these tests want a plain, named knight
    /// and nothing more.
    fn quest_app() -> Option<App> {
        let mut app = App::new().ok()?;
        app.run.knight.name = "SIR TEST".into();
        app.run.seat_the_rivals(&app.items);
        Some(app)
    }

    /// The same, but only when the pack it loaded is a real one baked from
    /// the user's own copy of the game: `App::new` always succeeds, packless
    /// or not, so a test that needs an actual arena to enter (`self.map` and
    /// `self.world` both `Some`, which only a real `data.overworld` and
    /// `data.arenas` give it) has to check for that itself rather than take
    /// `Some(app)` as proof of it. Absent in a fresh checkout and in CI,
    /// since the original game's data is never committed; skipped there the
    /// way `henge-bake`'s own pack-dependent checks already are.
    fn quest_app_with_a_real_pack() -> Option<App> {
        let app = quest_app()?;
        (app.map.is_some() && app.world.is_some()).then_some(app)
    }

    /// `knight_fight`'s `Challenged::Walkover` arm: a grave under the
    /// player's own token wins outright and opens the trade page, with no
    /// bout ever fought (`self.mode` stays `Map`, never `Combat`).
    #[test]
    fn a_grave_is_a_walkover_straight_to_the_trade_page() {
        let Some(mut app) = quest_app() else { return };
        app.mode = Mode::Map;
        app.run.rivals[0].lives = 0;
        app.knight_fight(0, 1);
        assert_eq!(app.mode, Mode::Map, "003d5: no bout is fought");
        assert_eq!(app.trade_page, Some((1, false)));
    }

    /// `knight_fight`'s `Challenged::Fight` arm: a live rival is a real
    /// bout, on the map's own ground, with the duel recorded so the combat
    /// tick's own settling code (main.rs, above) knows whose fight it is.
    #[test]
    fn a_live_rival_is_a_real_bout_set_up_as_a_duel() {
        let Some(mut app) = quest_app_with_a_real_pack() else {
            return;
        };
        app.mode = Mode::Map;
        app.knight_fight(0, 1);
        assert_eq!(app.mode, Mode::Combat);
        assert_eq!(app.duel, Some((0, 1)));
        assert!(app.trade_page.is_none());
    }

    /// `Knight1Won`/`BothKnightsDied` once a challenge has settled, whether
    /// or not a bout was fought for it: every branch returns to the map,
    /// except a person's own win, which opens the trade page instead.
    #[test]
    fn a_settled_challenge_opens_the_trade_page_only_for_a_players_win() {
        let Some(mut app) = quest_app_with_a_real_pack() else {
            return;
        };
        app.mode = Mode::Combat;
        app.knight_fight_settled(None);
        assert_eq!(app.mode, Mode::Map);
        assert!(app.trade_page.is_none());

        app.mode = Mode::Combat;
        app.knight_fight_settled(Some(Settled::BothDied));
        assert_eq!(app.mode, Mode::Map);
        assert!(app.trade_page.is_none());

        app.mode = Mode::Combat;
        app.knight_fight_settled(Some(Settled::RivalWon {
            winner: 1,
            loot: henge_core::rival::Loot::Nothing,
        }));
        assert_eq!(app.mode, Mode::Map);
        assert!(app.trade_page.is_none(), "BKAddstuff shows nothing");

        app.mode = Mode::Combat;
        app.knight_fight_settled(Some(Settled::PlayerWon { loser: 2 }));
        assert_eq!(app.mode, Mode::Map);
        assert_eq!(app.trade_page, Some((2, false)));
    }

    /// The routine at 0xcf3/0xcf6 on its way out: the panel opens on
    /// `StatTYPE` 0xa only for the branch that grounds the dragon for
    /// good, never for the one that leaves it flying.
    #[test]
    fn dragon_fight_settled_opens_the_hoard_page_only_on_a_win() {
        let Some(mut app) = quest_app_with_a_real_pack() else {
            return;
        };
        app.mode = Mode::Combat;
        app.dragon_fight_settled(false);
        assert_eq!(app.mode, Mode::Map);
        assert!(
            !app.dragon_page,
            "0xd23: the dragon flies on, not the panel"
        );
        assert!(!app.run.dragon.dead, "no write to the dragon's own +0x31");

        app.mode = Mode::Combat;
        app.dragon_fight_settled(true);
        assert_eq!(app.mode, Mode::Map);
        assert!(app.dragon_page, "0xd44: mov ax, 0xa; call 0xbdd3");
        assert!(app.run.dragon.dead, "0xd38: [si+0x31] = 0xff");
    }

    /// `panel_now`'s dispatch for the dragon's hoard: `Screen::Dragon` with
    /// `dragon_hoard` in the other arch and no gold, since the dragon's own
    /// record's kind (0x14) never matches `TakeGold`'s `0xa` at 0xafa.
    #[test]
    fn panel_now_shows_the_dragon_hoard_with_no_gold() {
        let Some(mut app) = quest_app() else { return };
        app.run.dragon_hoard.gems = 3;
        app.dragon_page = true;
        let (screen, other) = app.panel_now().expect("dragon_page is up");
        assert_eq!(screen, SheetScreen::Dragon);
        let other = other.expect("the hoard and the gold");
        assert_eq!(other.hoard.gems, 3);
        assert_eq!(other.gold, 0);
        assert!(!other.scouted);
        assert!(other.second.is_none());
    }

    /// `EXIT` closes the dragon's hoard page back to the map, the same way
    /// it closes every other page `StatLOOP`'s own gadget handling reaches.
    #[test]
    fn the_exit_gadget_closes_the_dragon_hoard_page() {
        let Some(mut app) = quest_app() else { return };
        app.dragon_page = true;
        app.gadgets
            .add_box(henge_core::status::EXIT_ID, 0, 0, 10, 10, "EXIT");
        app.pointer.x = 5;
        app.pointer.y = 5;
        app.pointer.woken = true;
        app.pressed[6] = true;
        app.sheet_tick();
        assert!(!app.dragon_page);
    }

    /// The trade page's own gadgets: a take moves something, and `EXIT`
    /// closes it back to the map.
    #[test]
    fn the_trade_page_takes_and_the_exit_gadget_closes_it() {
        let Some(mut app) = quest_app() else { return };
        app.run.rivals[0].gold = 40;
        app.trade_page = Some((1, false));
        app.mode = Mode::Map;
        // TKGP, off the gadget `display_knight` gives the loser's gold: the
        // same `field` the trade page's own drawing hands `HotGadget`.
        assert!(app.run.trade_take(1, 0x32, &app.items));
        assert_eq!((app.run.gold, app.run.rivals[0].gold), (40, 0), "0cd1a");
        app.close_trade_page();
        assert!(app.trade_page.is_none());
    }

    /// `ReDisplay+0x73` (0xbeda): one thing taken off a loser who is still
    /// standing turns the page into the winner's own sheet at once, rather
    /// than staying open for another (`StatTYPE = SaveTYPE = 9`, 0xbeed).
    #[test]
    fn taking_one_thing_from_a_living_loser_closes_the_trade_page_to_the_sheet() {
        let Some(mut app) = quest_app() else { return };
        app.run.rivals[0].lives = 1;
        app.trade_page = Some((1, false));
        app.sheet = false;
        app.trade_took(1);
        assert!(app.trade_page.is_none(), "0bee6: TakeCNT was not cleared");
        assert!(app.sheet, "0beed: StatTYPE = 9");
    }

    /// `ReDisplay+0x1b` (0xbee0): a dead loser (a grave) has `TakeCNT`
    /// forced back to nought on every redraw, so the page never turns into
    /// the sheet and a grave can be picked clean, one thing at a time.
    #[test]
    fn taking_from_a_grave_leaves_the_trade_page_open() {
        let Some(mut app) = quest_app() else { return };
        app.run.rivals[0].lives = 0;
        app.trade_page = Some((1, false));
        app.sheet = false;
        app.trade_took(1);
        assert_eq!(
            app.trade_page,
            Some((1, true)),
            "0bee0: TakeCNT is forced clear"
        );
        assert!(!app.sheet);
    }

    /// `MagicCast` slot 0x10 (0xcb60) through `acted`: casting opens the
    /// picker on `InitAKnight`'s own first candidate (0xc90d), the
    /// caster's own seat skipped, and nothing is committed to the dragon
    /// until the page closes.
    #[test]
    fn casting_the_scroll_of_the_wyrm_opens_the_picker_on_the_first_candidate() {
        let Some(mut app) = quest_app() else { return };
        // `Run::apply`'s own arm picks the seat; here the wiring is what is
        // under test, so the seat `Run::cast` would have picked is handed
        // straight to `acted`, the way `Op::Cast` does it.
        app.acted(Cast::Wyrm { seat: 1 });
        assert_eq!(app.wyrm_picker, Some(1), "the page is up on that seat");
        assert!(
            app.run.dragon.target.is_none(),
            "0be57 has not run yet: EXIT alone commits"
        );
    }

    /// `ResetStatus`'s dispatch on `StatTYPE` 0xb: `panel_now` puts the
    /// highlighted candidate in the other arch, the same slot the trade
    /// page's loser sits in.
    #[test]
    fn panel_now_shows_the_picker_with_the_candidate_in_the_other_arch() {
        let Some(mut app) = quest_app() else { return };
        app.wyrm_picker = Some(2);
        let (screen, other) = app.panel_now().expect("wyrm_picker is up");
        assert_eq!(screen, SheetScreen::AcquirePair);
        assert_eq!(
            other.expect("the candidate is in the other arch").second,
            Some(2)
        );
    }

    /// `HotGadget` (0xca1b): `NEXT` steps the picker exactly the way
    /// `NextKnight` does, and the caster's own seat — whichever one it is —
    /// is never offered, however many times it is clicked.
    #[test]
    fn the_next_gadget_cycles_the_picker_and_never_offers_the_caster() {
        let Some(mut app) = quest_app() else { return };
        app.mode = Mode::Map;
        app.run.knight.seat = 2;
        app.wyrm_picker = Some(3);
        app.gadgets.add_box(status::NEXT_ID, 0, 0, 10, 10, "");
        app.pointer.x = 5;
        app.pointer.y = 5;
        app.pointer.woken = true;
        app.pressed[6] = true;
        let mut seen = Vec::new();
        for _ in 0..8 {
            app.sheet_tick();
            let seat = app.wyrm_picker.expect("NEXT never closes the page");
            assert_ne!(seat, 2, "0c927: the caster's own seat, skipped");
            seen.push(seat);
        }
        // Only the other three seats ever come up, cycling.
        assert!(seen.iter().all(|s| [0, 1, 3].contains(s)));
        assert_eq!(seen[0..3], seen[3..6], "0c918: mod four, round again");
    }

    /// `StatusDone` (0xbe57): `EXIT` closes the picker and commits whichever
    /// seat was last highlighted into the dragon's own target — no charge
    /// spent here (`MagicCast`'s `dec byte [bx+si]` already did that, at
    /// the top, before the page ever opened) and no `ContinueDragon` call,
    /// so the dragon does not leap into the air on the close alone; it
    /// only actually flies at the seat once some knight's ordinary turn
    /// runs `ContinueDragon` next.
    #[test]
    fn the_exit_gadget_closes_the_picker_and_commits_the_seat() {
        let Some(mut app) = quest_app() else { return };
        app.wyrm_picker = Some(1);
        app.gadgets
            .add_box(henge_core::status::EXIT_ID, 0, 0, 10, 10, "EXIT");
        app.pointer.x = 5;
        app.pointer.y = 5;
        app.pointer.woken = true;
        app.pressed[6] = true;
        app.sheet_tick();
        assert!(app.wyrm_picker.is_none(), "the page is down");
        assert_eq!(app.run.dragon.target, Some(1), "0be60");
        assert!(!app.run.dragon.aloft, "no ContinueDragon call from here");
    }

    /// Whichever seat the player happens to be sitting in, casting never
    /// offers the picker that same seat as its first candidate.
    #[test]
    fn the_picker_never_opens_on_the_casters_own_seat() {
        for caster in 0..4 {
            let Some(mut app) = quest_app() else { return };
            app.run.knight.seat = caster;
            let seat = henge_core::dragon::next_wyrm_seat(0, caster);
            app.acted(Cast::Wyrm { seat });
            assert_ne!(app.wyrm_picker, Some(caster));
        }
    }

    /// `NextWHICH` (0xa434): the day turns and the between-days screen
    /// opens the moment the fourth seat's turn wraps back to the player's.
    #[test]
    fn end_turn_opens_the_interlude_when_the_day_turns() {
        let Some(mut app) = quest_app() else { return };
        app.run.which = 3;
        app.interlude = 0;
        app.end_turn();
        assert_eq!(app.run.which, 0, "the wrap lands back on the player");
        assert_ne!(app.interlude, 0, "0x1148 ran, so the moon screen goes up");
    }

    /// `MapLOOP+19` (0xa319): a computer knight's own frame walks the map
    /// the same as the player's, and the turn ends within a bounded number
    /// of frames rather than running forever.
    #[test]
    fn rival_tick_walks_a_computer_knight_and_his_turn_ends() {
        let Some(mut app) = quest_app_with_a_real_pack() else {
            return;
        };
        app.mode = Mode::Map;
        app.run.which = 1;
        for frame in 0..10_000 {
            if app.run.which != 1 {
                return;
            }
            app.rival_tick();
            assert!(frame < 9_999, "his turn never ended");
        }
    }

    /// The two clocks, derived rather than written out.
    ///
    /// `Install_Timer` (0x584f) programs the 8253 with mode 0x36 and divisor
    /// 0x5555, so a timer tick is 21845 / 1193182 of a second. The retrace is
    /// the 320x200 VGA mode's 400-line timing: 449 lines of 800 dots at the
    /// 25.175 MHz dot clock.
    #[test]
    fn the_two_clocks_are_the_rates_the_hardware_is_set_to() {
        // 1193182 / 21845 = 54.6204 Hz.
        let timer_hz = 1.0 / TIMER_TICK.as_secs_f64();
        assert!(
            (timer_hz - 54.6204).abs() < 0.001,
            "the timer ticked at {timer_hz} Hz"
        );
        // 25175000 / (800 * 449) = 70.0863 Hz.
        let retrace_hz = 1.0 / RETRACE_TICK.as_secs_f64();
        assert!(
            (retrace_hz - 70.0863).abs() < 0.001,
            "the retrace came at {retrace_hz} Hz"
        );
        // And the one that used to be the only clock is unchanged to the
        // nanosecond, because every recovered retrace count is against it.
        assert_eq!(RETRACE_TICK, std::time::Duration::from_nanos(14_268_123));
    }

    /// `Combat` (0x351) waits for a deadline two ticks of the BIOS counter at
    /// 0000:046c ahead, and `Install_Timer`'s handler chains to the BIOS one
    /// every third tick (0x5945..0x5951), so that counter keeps its standard
    /// 18.2068 Hz. Two of those is 109.849 ms, and `DELAY`'s six timer ticks is
    /// the same duration.
    #[test]
    fn a_combat_frame_is_two_bios_ticks_and_delays_six_timer_ticks() {
        // `DELAY` = 6, which is `ActorDef::script_ticks` for everything that
        // fights.
        let frame = TIMER_TICK * 6;
        let ms = frame.as_secs_f64() * 1000.0;
        assert!((ms - 109.849).abs() < 0.01, "a combat frame took {ms} ms");
        // The same thing measured the other way: two ticks of 0000:046c, which
        // is the 54.6204 Hz timer chained every third tick.
        let bios_tick = TIMER_TICK * 3;
        assert_eq!(frame, bios_tick * 2);
        let bios_hz = 1.0 / bios_tick.as_secs_f64();
        assert!(
            (bios_hz - 18.2068).abs() < 0.001,
            "0000:046c ticked at {bios_hz} Hz"
        );
        // 9.1034 frames a second.
        let fps = 1.0 / frame.as_secs_f64();
        assert!((fps - 9.1034).abs() < 0.001, "the fight ran at {fps} fps");
    }

    /// The dial that exists because the original's wait is a **floor**, not a
    /// rate: `0x96fe` is `jb`, so a pass that has already overrun its deadline
    /// waits for nothing. See [`App::pace`].
    ///
    /// A hundred must leave every recovered tick alone to the nanosecond,
    /// because everything above this is measured against them.
    #[test]
    fn the_pace_dial_is_ours_and_a_hundred_changes_nothing() {
        let Some(mut app) = quest_app() else { return };
        assert_eq!(app.pace, PACE_DEFAULT);
        // It opens on the recovered rate, so a combat pass out of the box is
        // `Combat`'s own two BIOS ticks and nothing else.
        app.mode = Mode::Combat;
        let pass = app.tick_len() * ticks_per_pass(Mode::Combat);
        let ms = pass.as_secs_f64() * 1000.0;
        assert!(
            (ms - 109.849).abs() < 0.1,
            "a pass out of the box took {ms} ms"
        );
        // Turned down, it is the wall clock that stretches and nothing else:
        // half speed is twice the time and the same six ticks.
        app.pace = 80;
        let ms = (app.tick_len() * ticks_per_pass(Mode::Combat)).as_secs_f64() * 1000.0;
        assert!((ms - 137.31).abs() < 0.1, "a pass at eighty took {ms} ms");
        // A hundred must leave every recovered tick alone to the nanosecond.
        app.pace = PACE_FULL;
        for mode in [Mode::Combat, Mode::Map, Mode::Select, Mode::Title] {
            app.mode = mode;
            assert_eq!(
                app.tick_len(),
                tick_len_for(mode),
                "{mode:?} at a hundred is the recovered tick untouched"
            );
        }
        // Half speed is twice the wall clock a tick takes, and the tick counts
        // themselves never move: `ticks_per_pass` is the image's six either way.
        app.mode = Mode::Combat;
        app.pace = 50;
        assert_eq!(app.tick_len(), TIMER_TICK * 2);
        assert_eq!(ticks_per_pass(Mode::Combat), 6);
        // And it opens on the recovered rate, so a game nobody has touched the
        // dial on is running at exactly what the image says.
        assert_eq!(PACE_DEFAULT, PACE_FULL, "the default is no dial at all");
        // And it is bounded, so no key press can stop the game or run it away.
        for _ in 0..200 {
            app.key(KeyCode::Minus, true);
            app.key(KeyCode::Minus, false);
        }
        assert_eq!(app.pace, PACE_MIN);
        for _ in 0..200 {
            app.key(KeyCode::Equal, true);
            app.key(KeyCode::Equal, false);
        }
        assert_eq!(app.pace, PACE_MAX);
    }

    /// `COLCON` (0x4988) and `KnightGlowOn` (0x8f8) run once a loop pass. A
    /// pass is one tick everywhere the loop waits one retrace, and six in a
    /// fight, so the per-pass work must be gated on the same six `script_ticks`
    /// and `DELAY` name and not on the tick.
    #[test]
    fn per_pass_work_is_gated_to_the_loops_own_pass() {
        assert_eq!(ticks_per_pass(Mode::Combat), 6, "0x357 and 0x369, per pass");
        // `MapLOOP+3` (0xa309) and `ChooseLoop+3` (0x15a3) call `COLCON` on a
        // pass that is a single retrace, which is this engine's tick there.
        for mode in [Mode::Map, Mode::Select] {
            assert_eq!(ticks_per_pass(mode), 1, "{mode:?} passes once a retrace");
        }
        // A pass is the whole of a frame either way round.
        assert_eq!(
            tick_len_for(Mode::Combat) * ticks_per_pass(Mode::Combat),
            TIMER_TICK * 6
        );
        // One tick of the map is one pass of it, and that pass is a whole
        // number of retraces rather than one: [`MAP_PASS_RETRACES`].
        assert_eq!(
            tick_len_for(Mode::Map) * ticks_per_pass(Mode::Map),
            RETRACE_TICK * MAP_PASS_RETRACES
        );
    }

    /// `StatLOOP` (0xbe13) calls neither `COLCON` nor `KnightGlowOn`, so
    /// nothing cycles while a panel is up: the original is running that loop
    /// and not the one underneath. The arch's ivy is painted in the entries
    /// the map's own cycle rotates, so leaving it turning under a panel is
    /// what made the leaves flicker on the knight's sheet.
    #[test]
    fn nothing_cycles_while_a_panel_is_up() {
        let Some(mut app) = quest_app() else { return };
        app.mode = Mode::Map;
        app.sheet = false;
        assert!(app.panel_now().is_none(), "no panel: the map's own loop");
        // The sheet is `StatLOOP`, and so is every page that opens over it.
        app.sheet = true;
        assert!(app.panel_now().is_some(), "0xbe13 runs instead of 0xa306");
        app.sheet = false;
        app.dragon_page = true;
        assert!(
            app.panel_now().is_some(),
            "the dragon's hoard, StatTYPE 0xa"
        );
        app.dragon_page = false;
        app.wyrm_picker = Some(1);
        assert!(app.panel_now().is_some(), "the picker, StatTYPE 0xb");
    }

    /// The fade is not a `VBLQUE` entry, so splitting the effects off must
    /// leave it stepping on its own: `tick_effects` alone never advances it,
    /// and `tick` still does both for anything that wants the pair.
    #[test]
    fn the_fade_steps_apart_from_the_effects() {
        let mut fx = henge_assets::Effects::new();
        fx.set_fade(henge_assets::Fade::In(0));
        let before = fx.fade();
        fx.tick_effects();
        assert_eq!(fx.fade(), before, "0x4988 does not step a fade");
        fx.tick_fade();
        assert_ne!(fx.fade(), before, "and this is the half that does");
    }

    /// One stride of `Knight_SwWalkOn` is four script frames and
    /// `K_WalkRValue` (0x77fe) carries it 25 + 3 + 23 + 4 = 55 pixels. On the
    /// retrace it took 342.435 ms, 160.6 pixels a second; on the clock `Combat`
    /// actually waits on it takes 439.396, four of the 109.849 ms frames above,
    /// which is 125.2 pixels a second.
    #[test]
    fn a_stride_lasts_four_combat_frames() {
        const SCRIPT_TICKS: u32 = 6;
        const FRAMES_PER_STRIDE: u32 = 4;
        let stride = TIMER_TICK * SCRIPT_TICKS * FRAMES_PER_STRIDE;
        let ms = stride.as_secs_f64() * 1000.0;
        assert!((ms - 439.396).abs() < 0.01, "a stride took {ms} ms");
        assert_eq!(stride, (TIMER_TICK * SCRIPT_TICKS) * FRAMES_PER_STRIDE);
        // What it used to be, and the factor the fight was running fast by.
        let was = RETRACE_TICK * SCRIPT_TICKS * FRAMES_PER_STRIDE;
        let factor = stride.as_secs_f64() / was.as_secs_f64();
        assert!((factor - 1.2832).abs() < 0.001, "it was {factor}x fast");
    }

    /// Which loop is on which clock. `Combat` (0x351) is the only loop in the
    /// image that waits on the BIOS counter; everything else waits on a retrace
    /// or on nothing.
    #[test]
    fn only_the_arena_runs_on_the_timer() {
        assert_eq!(tick_len_for(Mode::Combat), TIMER_TICK);
        for mode in [
            Mode::Intro,
            Mode::Ending,
            Mode::Title,
            Mode::Select,
            Mode::Place,
        ] {
            assert_eq!(
                tick_len_for(mode),
                RETRACE_TICK,
                "{mode:?} was taken off the retrace"
            );
        }
        // The map is on the retrace too, but a pass of it is more than one:
        // see [`MAP_PASS_RETRACES`], the one observed number in this file.
        assert_eq!(
            tick_len_for(Mode::Map),
            RETRACE_TICK * MAP_PASS_RETRACES,
            "the map is a whole number of retraces and nothing else"
        );
    }

    // --------------------------------------------------------- online play

    fn word(pad: u8) -> henge_net::SeatInput {
        henge_net::SeatInput {
            pad,
            ..henge_net::SeatInput::default()
        }
    }

    /// A held word stays held and a press lasts exactly the tick the word
    /// arrived on. Two machines compute this from the same stream, which is why
    /// the wire carries no press at all.
    #[test]
    fn a_press_is_the_rising_edge_of_a_held_word() {
        let right = input::RIGHT;
        let first = seat_slots(0, word(right), henge_net::SeatInput::default(), false);
        let slot = slot_of(0, input::Action::Right);
        assert_eq!(
            first.iter().find(|(s, _, _)| *s == slot),
            Some(&(slot, true, true)),
            "held and pressed on the tick it arrived"
        );
        // Held on, and no longer a press.
        let again = seat_slots(0, word(right), word(right), false);
        assert_eq!(
            again.iter().find(|(s, _, _)| *s == slot),
            Some(&(slot, true, false))
        );
        // Let go: neither.
        let gone = seat_slots(0, word(0), word(right), false);
        assert_eq!(
            gone.iter().find(|(s, _, _)| *s == slot),
            Some(&(slot, false, false))
        );
        // And pressing again after letting go is a press again.
        let retaken = seat_slots(0, word(right), word(0), false);
        assert_eq!(
            retaken.iter().find(|(s, _, _)| *s == slot),
            Some(&(slot, true, true))
        );
    }

    /// All four seats land somewhere, and no two of them land on the same slot or
    /// on a slot anything else owns.
    #[test]
    fn every_seat_has_its_own_slots_and_they_collide_with_nothing() {
        let mut seen: std::collections::BTreeMap<usize, (usize, input::Action)> =
            std::collections::BTreeMap::new();
        for seat in 0..henge_core::shell::SEATS {
            for a in input::Action::ALL {
                let slot = slot_of(seat, a);
                assert!(slot < 256, "seat {seat} {a:?} has nowhere to land");
                assert!(
                    seen.insert(slot, (seat, a)).is_none(),
                    "slot {slot} is claimed twice"
                );
            }
        }
        for taken in [ENTER_SLOT, BACKSPACE_SLOT, CALIBRATE_SLOT] {
            assert!(!seen.contains_key(&taken), "slot {taken} is a seat's");
        }
        for n in 0..9 {
            assert!(!seen.contains_key(&(NUMBER_SLOT + n)));
        }
    }

    /// Enter, backspace and the number keys land in one slot each for all four
    /// seats, so exactly one seat fills them and the other three do not.
    #[test]
    fn the_keyboard_half_is_filled_by_one_seat_and_no_other() {
        let typing = henge_net::SeatInput {
            keys: henge_net::key::TAKE | henge_net::key::BACK,
            number: Some(4),
            typed: Some('Q'),
            ..henge_net::SeatInput::default()
        };
        let other = seat_slots(1, typing, henge_net::SeatInput::default(), false);
        for slot in [ENTER_SLOT, BACKSPACE_SLOT, NUMBER_SLOT + 3] {
            assert!(
                !other.iter().any(|(s, _, _)| *s == slot),
                "slot {slot} was filled by a seat that does not fill it"
            );
        }
        let shared = seat_slots(1, typing, henge_net::SeatInput::default(), true);
        assert_eq!(
            shared.iter().find(|(s, _, _)| *s == ENTER_SLOT),
            Some(&(ENTER_SLOT, true, true))
        );
        assert_eq!(
            shared.iter().find(|(s, _, _)| *s == NUMBER_SLOT + 3),
            Some(&(NUMBER_SLOT + 3, true, true))
        );
    }

    /// A number held down types one number, and a different one types again.
    #[test]
    fn a_held_number_key_is_read_once() {
        let three = henge_net::SeatInput {
            number: Some(3),
            ..henge_net::SeatInput::default()
        };
        let slot = NUMBER_SLOT + 2;
        let first = seat_slots(0, three, henge_net::SeatInput::default(), true);
        assert_eq!(
            first.iter().find(|(s, _, _)| *s == slot),
            Some(&(slot, true, true))
        );
        let held = seat_slots(0, three, three, true);
        assert_eq!(
            held.iter().find(|(s, _, _)| *s == slot),
            Some(&(slot, true, false))
        );
    }

    /// The port and delay arguments, which are the two knobs a bad line needs.
    #[test]
    fn the_online_arguments_are_read() {
        let args: Vec<String> = ["henge", "--port", "25000", "--delay", "9"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(online_port_arg(&args), Some(25_000));
        assert_eq!(online_delay_arg(&args), Some(9));
        let none: Vec<String> = vec!["henge".into()];
        assert_eq!(online_port_arg(&none), None);
        assert_eq!(online_delay_arg(&none), None);
    }

    /// The fifth row of the title opens the lobby and starts nothing else.
    #[test]
    fn the_title_has_a_fifth_row_and_it_is_the_lobby() {
        let mut t = henge_core::shell::Title::default();
        t.move_by(9);
        assert_eq!(t.selected(), henge_core::shell::Row::Online);
        assert_eq!(t.choose(), Some(Start::Online));
    }
}
