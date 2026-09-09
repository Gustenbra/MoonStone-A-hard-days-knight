//! Desktop entry point.
//!
//! Release builds contain no original-game data. `--features research` adds a
//! viewer for studying the 1991 files, which is a development tool only.

mod framebuffer;
mod input;
mod map;
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
        other => {
            eprintln!("no screen called {other}: try intro, ending, title, select, map or arena");
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
                                format!(
                                    "{:<6} {:<12}{:>4} @{:>3},{:>3}{}{}",
                                    f.actor, state, f.health, f.x, f.y, face, script
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

    // One tick is one of the original's frames, and the original's frame is one
    // vertical retrace.
    //
    // **Recovered.** The wait is the unnamed public routine at image `0x5a24`,
    // between `AdjustJoy` and the start of `GFX`: `mov dx, 0x3da`, spin while
    // bit 3 is set, then spin until it is set again, which is exactly one
    // retrace. Every main loop calls it once a pass and nothing else paces
    // them: `Combat` at `0x0354` (the loop runs `0x0351` to `0x0374`),
    // `MapLOOP` at `0x0a306`, `ScanKEYS` at `0x0145a`, `FindLandscape` at
    // `0x0afed`, `ShakeScreen` at `0x0496b`, `KnightWonGame` at `0x01117`,
    // `FightDemon` at `0x01031`, and the palette fade loop at `0x05bb0`.
    //
    // So the rate is the video mode's refresh rate. The game never programs the
    // CRTC's timing or the Miscellaneous Output register: the only CRTC write in
    // the whole image is index 0x0c, the start address, at `0x5a34` and inside
    // `ShakeScreen` at `0x4965`. It therefore runs at the BIOS timing for a
    // 320x200 VGA mode, which is the 400 line timing: a 25.175 MHz dot clock
    // over 800 dots is 31468.75 lines a second, over 449 lines is **70.0863
    // frames a second**. That is the 70 Hz `henge_core::intro` already quotes.
    //
    // This used to be sixty, which ran every recovered frame count about
    // fourteen percent slow. The music is a different clock and is not this one:
    // `Install_Timer` at `0x584f` programs the 8253 with mode 3 and a divisor of
    // 0x5555 and its handler at `0x5934` does nothing but `int 60h` and `int
    // 61h` with `ah = 1`, so 54.62 Hz drives the tune and the sound effects and
    // never the game. `henge_audio::music` keeps that rate in the score itself.
    //
    // Time is accumulated and spent in whole ticks so the simulation never sees
    // a fractional step, which is what keeps two machines agreeing on it.
    const TICK: std::time::Duration = std::time::Duration::from_nanos(14_268_123);
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
                elwt.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(
                    last + (TICK - owed),
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
        _ => 255,
    }
}

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
            println!("map/arena, [ and ] change arena, , and . change the opponent, R restarts");
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
        self.pressed[6] || self.pressed[12]
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
                self.kb[s] = down;
                self.keys[s] = self.kb[s] || self.pad_held[s];
            }
        }
        let i = key_index(code);
        if i < 256 && down && !self.keys[i] {
            self.pressed[i] = true;
        }
        // `ASCIIKEY`, for `TypeName`. A key that the table has no character for
        // types nothing, and the screens that do not read letters never look.
        if down {
            if let Some(c) = typed_char(code) {
                self.typed = Some(c);
            }
        }
        // A key the binding table claims is a control, and a control is never
        // also a developer key: the original's ten are in that table now, and
        // player two pressing fire must not flip the screen out from under the
        // fight. This is also what keeps a rebinding safe.
        let is_control = !self.bindings.raised_by(&named).is_empty();
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

    /// One tick. A key press is an edge: it lasts exactly this tick and is
    /// spent whether or not anything wanted it.
    fn update(&mut self) {
        self.simulate();
        self.palette_tick();
        self.music_tick();
        self.pressed = [false; 256];
        self.typed = None;
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
        self.knight_glow_tick();
        // `0x50d`, which the ceremony's own script gosubs part way through
        // itself rather than the scene routine installing it up front.
        self.moonstone_glow_tick();
        self.fx.tick();
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
                            self.run.finished_fight_worth(health, won, w.purse(), xp);
                        }
                        // The routine at 0xcf6 on its way out: `_dragon_won`
                        // (0xd23) leaves it flying; 0xd38 to 0xd50 ground it
                        // for good. `add word [si+0x36], 2` at 0xd40 is the
                        // two points `w.experience()` has just paid.
                        if self.dragon_fight {
                            self.dragon_fight = false;
                            self.run.dragon.fight_over(won);
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
        let (left, right, take) = (self.pressed[2], self.pressed[3], self.takes());
        let typed = self.typed.take();
        let back = self.pressed[BACKSPACE_SLOT];
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
            } else {
                self.sheet = false;
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
            // `StatusDone` (0xbe57): the dragon's `+0x46` is the knight
            // picked, and it flies at his row from its next frame.
            Cast::Wyrm { seat } => {
                let name = self
                    .knights
                    .get(seat)
                    .map_or_else(|| "a knight".to_string(), |k| k.name.clone());
                self.notice(format!("The dragon is after {name}"));
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

    /// Close the trade page and go back to the map. `Knight1Won`'s trade
    /// page has no routine of its own the way `LairGEM+6` does for a lair's:
    /// what a gadget took has already been written back, gadget by gadget,
    /// so there is nothing left to settle on the way out.
    fn close_trade_page(&mut self) {
        self.trade_page = None;
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
                    loser: None,
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
                    loser: Some(loser),
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
}
