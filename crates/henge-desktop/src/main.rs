//! Desktop entry point.
//!
//! Release builds contain no original-game data. `--features research` adds a
//! viewer for studying the 1991 files, which is a development tool only.

mod framebuffer;
mod map;
mod place;
mod text;
mod world;
#[cfg(feature = "research")]
mod research;

use framebuffer::Framebuffer;
use henge_assets::Registry;
use henge_audio::{Clips, Sink, Voices};
use map::MapScene;
use text::Font;
use henge_core::combat::Intent;
use henge_core::item::{Items, Loss};
use henge_core::place::{Answer, Approach, Places};
use henge_core::run::Run;
use henge_core::{SCREEN_H, SCREEN_W};
use world::World;
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
fn hurt_arg(a: &[String]) -> Option<i32> {
    a.iter().position(|s| s == "--hurt").and_then(|i| a.get(i + 1)).and_then(|v| v.parse().ok())
}

fn gold_arg(a: &[String]) -> Option<u32> {
    a.iter().position(|s| s == "--gold").and_then(|i| a.get(i + 1)).and_then(|v| v.parse().ok())
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
                // Arrived: the script takes over.
                Mode::Place => {}
            }
        }
        let c = self.keys.get(*fed).copied();
        if c.is_some() {
            *fed += 1;
        }
        app.drive(c);
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
        app.peaceful = script.peaceful;
        if let Some(hp) = hurt_arg(&a) { app.run.health = hp; }
        if let Some(g) = gold_arg(&a) { app.run.gold = g; }
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
            }
            app.update();
            let line = match app.mode {
                Mode::Map if app.map.is_none() => break,
                Mode::Combat if app.world.is_none() => break,
                Mode::Place if app.visiting.is_none() => break,
                Mode::Map => {
                    let Some(m) = app.map.as_ref() else { break };
                    let r = &app.run;
                    format!("{:>5}  MAP     day {:<3} hp{:>4}  gold{:>5} {:<12} won {:<3} fought {:<3} {} on {}",
                        t, m.state.day, r.health, r.gold, carrying(r),
                        r.victories, r.fights,
                        if r.alive() { "     " } else { "ENDED" }, m.last_terrain.name())
                }
                Mode::Place => {
                    let Some(s) = app.visiting.as_ref() else { break };
                    let Some(def) = app.places.get(&s.visit.place) else { break };
                    format!("{:>5}  PLACE   day {:<3} hp{:>4}  gold{:>5} {:<12} {:<34} {}",
                        t, app.run.day, app.run.health, app.run.gold, carrying(&app.run),
                        place::describe(def, &s.visit, &app.items), s.visit.said)
                }
                Mode::Combat => {
                    let Some(w) = app.world.as_ref() else { break };
                    let who: Vec<String> = w
                        .bout
                        .fighters
                        .iter()
                        .map(|f| format!("{:<6}{:>4} @{:>3},{:>3}",
                            format!("{:?}", f.state), f.health, f.x, f.y))
                        .collect();
                    format!("{:>5}  COMBAT  {:<8} {}", t, w.family(), who.join(" | "))
                }
            };
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
        if let Some(hp) = hurt_arg(&args) { app.run.health = hp; }
        if let Some(g) = gold_arg(&args) { app.run.gold = g; }
        if let Some(id) = script.at.as_deref() { app.enter(id); }
        let mut fed = 0usize;
        for _ in 0..ticks {
            if script.active() {
                script.drive(&mut app, &mut fed);
            }
            app.update();
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
        return Ok(());
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

    event_loop.run(move |event, elwt| {
        elwt.set_control_flow(winit::event_loop::ControlFlow::Poll);
        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => elwt.exit(),
            Event::WindowEvent {
                event: WindowEvent::KeyboardInput { event, .. }, ..
            } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    let down = event.state == ElementState::Pressed;
                    if down && code == KeyCode::Escape {
                        elwt.exit();
                    }
                    app.key(code, down);
                }
            }
            Event::AboutToWait => {
                app.update();
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
                    let palette = app.fb.faded_palette(app.fade);
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

struct App {
    fb: Framebuffer,
    fade: u8,
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
    approach: Approach,
    visiting: Option<place::PlaceScene>,
    mode: Mode,
    encounter_pick: u32,
    audio: Box<dyn Sink>,
    voices: Voices,
    fonts: std::collections::BTreeMap<String, Font>,
    /// Suppress ambushes, so map rendering can be checked anywhere.
    peaceful: bool,
    run: Run,
    /// The last thing a cutpurse took, for the map to say so. The map has no
    /// message line of its own, so the notice rides on the purse plate in the
    /// corner, beside the number it just changed.
    robbed: String,
    /// Ticks the robbery notice has left on screen.
    robbed_for: u32,
    /// Ticks since the run ended, so the tally can be read before it restarts.
    run_over_for: u32,
    status: String,
    #[cfg(feature = "research")]
    research: Option<research::Viewer>,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum Mode { Map, Combat, Place }

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

    #[cfg(feature = "audio")]
    match henge_audio::Native::new(clips) {
        Ok(n) => {
            println!("audio: {loaded} clips");
            return Box::new(n);
        }
        Err(e) => eprintln!("audio: {loaded} clips loaded but no output device ({e}), playing silently"),
    }
    #[cfg(not(feature = "audio"))]
    let _ = loaded;
    Box::new(henge_audio::Silent)
}

fn key_index(c: KeyCode) -> usize {
    match c {
        KeyCode::ArrowUp => 0,
        KeyCode::ArrowDown => 1,
        KeyCode::ArrowLeft => 2,
        KeyCode::ArrowRight => 3,
        KeyCode::BracketLeft => 4,
        KeyCode::BracketRight => 5,
        KeyCode::Space => 6,
        // Seat two shares the keyboard, which is how this game was played.
        KeyCode::KeyW => 7,
        KeyCode::KeyS => 8,
        KeyCode::KeyA => 9,
        KeyCode::KeyD => 10,
        KeyCode::KeyF => 11,
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
        if world.is_none() {
            eprintln!("no arena data: {status}");
        } else {
            println!("{status}");
            println!("p1 arrows + space, p2 wasd + f, 1/2 set how many are playing,");
            println!("tab switches map/arena, [ and ] change arena, R restarts, escape quits");
        }

        Ok(App {
            fb, fade: 255, tick: 0,
            keys: [false; 256],
            pressed: [false; 256],
            reg, world,
            mode: if map.is_some() { Mode::Map } else { Mode::Combat },
            map,
            places,
            items,
            approach: Approach::default(),
            visiting: None,
            encounter_pick: 0,
            audio,
            voices: Voices::new(),
            fonts,
            peaceful: false,
            run: Run::new(100),
            robbed: String::new(),
            robbed_for: 0,
            run_over_for: 0,
            status,
            #[cfg(feature = "research")]
            research: research::Viewer::from_args()?,
        })
    }

    fn key(&mut self, code: KeyCode, down: bool) {
        let i = key_index(code);
        if i < 256 {
            if down && !self.keys[i] {
                self.pressed[i] = true;
            }
            if down && self.world.is_some() {
                match code {
                    KeyCode::BracketLeft => self.world.as_mut().unwrap().step_arena(-1),
                    KeyCode::BracketRight => self.world.as_mut().unwrap().step_arena(1),
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
                        };
                    }
                    _ => {}
                }
            }
            self.keys[i] = down;
        }
        #[cfg(feature = "research")]
        if let Some(v) = self.research.as_mut() {
            v.key(code, down);
        }
    }

    /// One tick. A key press is an edge: it lasts exactly this tick and is
    /// spent whether or not anything wanted it.
    fn update(&mut self) {
        self.simulate();
        self.pressed = [false; 256];
        self.robbed_for = self.robbed_for.saturating_sub(1);
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

        match self.mode {
            Mode::Map => {
                if !self.run.alive() {
                    // The run is over. Hold a moment so the tally can be read,
                    // then begin again.
                    self.run_over_for += 1;
                    if self.run_over_for > 180 {
                        self.run.restart();
                        self.run_over_for = 0;
                        if let Some(w) = self.world.as_mut() {
                            w.set_player_health(self.run.health_for_fight());
                        }
                    }
                    return;
                }
                let mut start: Option<String> = None;
                let mut day_before = 0;
                let mut arrived: Option<String> = None;
                if let Some(m) = self.map.as_mut() {
                    day_before = m.state.day;
                    let ambushed = m.update(dx, dy);
                    if ambushed && !self.peaceful {
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
                            }
                            None => {}
                        }
                    }
                }
                if let Some(m) = self.map.as_ref() {
                    if m.state.day != day_before {
                        self.run.new_day();
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
                if let (Some(family), Some(w)) = (start, self.world.as_mut()) {
                    self.encounter_pick = self.encounter_pick.wrapping_add(1);
                    w.set_player_health(self.run.health_for_fight());
                    w.set_family(&family, self.encounter_pick);
                    self.voices.reset();
                    self.mode = Mode::Combat;
                }
            }
            Mode::Place => {
                let (up, down, take) = (self.pressed[0], self.pressed[1], self.pressed[6]);
                let mut leave = false;
                let mut days = 0;
                let mut door: Option<String> = None;
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
                            }
                        }
                    } else {
                        leave = true;
                    }
                }
                // A door inside a place opens another place rather than putting
                // you back on the map: the merchant's stall is a room in the
                // town, not a walk away from it.
                if let Some(id) = door {
                    if !self.enter(&id) {
                        leave = true;
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
                    if w.settled_for() == 1 {
                        let survivor = w.bout.fighters.first();
                        let health = survivor.map_or(0, |f| if f.alive() { f.health } else { 0 });
                        let won = w.bout.winner() == Some(0);
                        // What the fallen were carrying. The run decides whether
                        // it is collected; a corpse collects nothing.
                        self.run.finished_fight(health, won, w.purse());
                    }
                    if w.settled_for() > 120 {
                        self.voices.reset();
                        if self.run.alive() {
                            w.set_player_health(self.run.health_for_fight());
                        }
                        w.reset();
                        if self.map.is_some() {
                            self.mode = Mode::Map;
                        }
                    }
                }
            }
        }
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
            Some('s') => self.pressed[6] = true,
            Some('h') => self.keys[2] = true,
            Some('l') => self.keys[3] = true,
            Some('k') => self.keys[0] = true,
            Some('j') => self.keys[1] = true,
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

    /// The tally at the end of a run. Without words this was a blank screen and
    /// a pause, which told the player nothing about what they had just done.
    fn draw_run_over(&mut self) {
        let Some(font) = self.fonts.remove("bold") else { return };
        let luma = |c: u32| ((c >> 16) & 0xff) * 2 + ((c >> 8) & 0xff) * 3 + (c & 0xff);
        let (mut dark, mut light) = (0usize, 0usize);
        for i in 1..32 {
            if luma(self.fb.palette[i]) < luma(self.fb.palette[dark]) { dark = i; }
            if luma(self.fb.palette[i]) > luma(self.fb.palette[light]) { light = i; }
        }
        self.fb.rect(40, 66, 240, 68, dark as u8);
        font.draw_centred(&mut self.reg, &mut self.fb, "You are slain", 74, light as u8);
        let tally = format!("Day {}  Won {} of {}", self.run.day, self.run.victories, self.run.fights);
        font.draw_centred(&mut self.reg, &mut self.fb, &tally, 100, light as u8);
        self.fonts.insert("bold".into(), font);
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
        for p in &self.fb.pixels {
            let c = self.fb.palette[(*p & 0x1f) as usize];
            rgb.extend_from_slice(&[(c >> 16) as u8, (c >> 8) as u8, c as u8]);
        }
        enc.write_header()?.write_image_data(&rgb)?;
        Ok(())
    }

    fn render(&mut self) {
        #[cfg(feature = "research")]
        if let Some(v) = self.research.as_ref() {
            v.render(&mut self.fb);
            return;
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
                let ok = m
                    .render(&mut self.reg, &mut self.fb, &self.fonts, &self.run,
                            near.as_deref(), notice)
                    .is_ok();
                self.map = Some(m);
                if ok {
                    if !self.run.alive() {
                        self.draw_run_over();
                    }
                    return;
                }
            }
        }
        if let Some(w) = self.world.as_mut() {
            if w.render(&mut self.reg, &mut self.fb).is_ok() {
                return;
            }
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
