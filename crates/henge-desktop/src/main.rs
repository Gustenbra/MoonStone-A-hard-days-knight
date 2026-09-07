//! Desktop entry point.
//!
//! Release builds contain no original-game data. `--features research` adds a
//! viewer for studying the 1991 files, which is a development tool only.

mod framebuffer;
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
use henge_core::combat::Intent;
use henge_core::item::{Items, Loss};
use henge_core::knight::Knights;
use henge_core::place::{Answer, Approach, Places};
use henge_core::run::Run;
use henge_core::shell::Start;
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
///   --start <screen>  title, select, map or arena. Defaults to the map, so
///                     every recipe written before the shell existed still does
///                     what it did
///   --knight <0..3>   begin the run as one of the four, without going through
///                     the select screen
///   --sheet           hold the character sheet open over whatever is drawn
fn hurt_arg(a: &[String]) -> Option<i32> {
    a.iter().position(|s| s == "--hurt").and_then(|i| a.get(i + 1)).and_then(|v| v.parse().ok())
}

fn gold_arg(a: &[String]) -> Option<u32> {
    a.iter().position(|s| s == "--gold").and_then(|i| a.get(i + 1)).and_then(|v| v.parse().ok())
}

fn knight_arg(a: &[String]) -> Option<usize> {
    a.iter().position(|s| s == "--knight").and_then(|i| a.get(i + 1)).and_then(|v| v.parse().ok())
}

/// Which screen a headless run opens on.
///
/// The default is the map, so every recipe written before the shell existed
/// still does what it did. An interactive run opens on the title, because that
/// is where a game opens.
fn start_arg(a: &[String]) -> Option<Mode> {
    let name = a.iter().position(|s| s == "--start").and_then(|i| a.get(i + 1))?;
    match name.as_str() {
        "title" => Some(Mode::Title),
        "select" => Some(Mode::Select),
        "map" => Some(Mode::Map),
        "arena" | "combat" => Some(Mode::Combat),
        other => {
            eprintln!("no screen called {other}: try title, select, map or arena");
            None
        }
    }
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
    app.sheet = a.iter().any(|s| s == "--sheet");
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
            }
            app.update();
            let line = match app.mode {
                // The shell has no state worth a line of trace: what it does is
                // decided by looking at it, and it is checked by test in
                // `henge_core::shell` instead.
                Mode::Title => format!("{t:>5}  TITLE"),
                Mode::Select => format!("{t:>5}  SELECT"),
                Mode::Map if app.map.is_none() => break,
                Mode::Combat if app.world.is_none() => break,
                Mode::Place if app.visiting.is_none() => break,
                Mode::Map => {
                    let Some(m) = app.map.as_ref() else { break };
                    let r = &app.run;
                    format!("{:>5}  MAP     day {:<3} at {:>3},{:<3} hp{:>4}  gold{:>5} {:<12} won {:<3} fought {:<3} {} on {}",
                        t, m.state.day, m.state.x, m.state.y, r.health, r.gold, carrying(r),
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
                    // The arena's own name as well as its family, because which of
                    // the eight a family rotates to is now a thing worth seeing.
                    format!("{:>5}  COMBAT  {:<5} {:<8} {}", t, w.name(), w.family(), who.join(" | "))
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
        prepare(&mut app, &args);
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

    // A game opens on its title screen. `--start` overrides it, which is how the
    // window can still be pointed straight at a map or an arena.
    app.mode = start_arg(&args_of()).unwrap_or(Mode::Title);
    if app.mode == Mode::Select {
        app.begin_select();
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
enum Mode { Title, Select, Map, Combat, Place }

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
        // A pack without knights is not an error; the title simply cannot offer
        // a quest, exactly as it could not before.
        let knights: Knights = reg.read_data("data.knights").unwrap_or_default();
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
                            // And the way out of the shell, so a pack with no
                            // knights in it cannot strand you on a select screen
                            // with nothing to select.
                            Mode::Select => { self.select = None; Mode::Title }
                            Mode::Title => Mode::Map,
                        };
                    }
                    // The character sheet, over whatever is on screen.
                    KeyCode::KeyC => self.sheet = !self.sheet,
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
            Mode::Title => self.title_tick(),
            Mode::Select => self.select_tick(),
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
                    // Which arena of that family comes next is the family's own
                    // turn counter, carried on the run: the original rotates
                    // through its eight in order rather than rolling for one.
                    let pick = self.run.next_arena(&family, w.rotation_len(&family));
                    w.set_player_health(self.run.health_for_fight());
                    w.set_family(&family, pick);
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

    /// The title's option list, and the attract loop behind it.
    ///
    /// A press while it is showing off only wakes it. Letting the same press
    /// through would mean walking away from the keyboard and coming back to find
    /// the game had started itself.
    fn title_tick(&mut self) {
        let (up, down) = (self.pressed[0], self.pressed[1]);
        let (left, right, take) = (self.pressed[2], self.pressed[3], self.pressed[6]);
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
        let (left, right, take) = (self.pressed[2], self.pressed[3], self.pressed[6]);
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
        self.take_knight(0, Self::roster_led_by(0));
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
        let humans = self.title.state.players.max(1);
        if let Some(w) = self.world.as_mut() {
            w.set_players(humans);
            w.set_roster(roster);
            w.set_sheet_health(self.run.max_health);
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
                status::Plate {
                    name: if mine {
                        self.run.knight.name.clone()
                    } else {
                        def.map_or_else(|| format!("Knight {}", which + 1), |d| d.name.clone())
                    },
                    colour: w.seat_colour(i),
                    health: f.health,
                    max_health: f.max_health,
                    lives: if mine { self.run.lives } else { def.map_or(0, |d| d.life) },
                }
            })
            .collect()
    }

    /// The scene, and then the character sheet over it if it is up.
    fn render(&mut self) {
        self.draw_scene();
        if self.sheet {
            let seat = self.run.knight.seat;
            let colour = henge_assets::player_colours(&self.fb.palette)[seat % 4];
            let font = self.fonts.get("small").or_else(|| self.fonts.get("bold"));
            status::draw_sheet(
                &mut self.reg, &mut self.fb, font, &self.run, &self.items, colour,
            );
        }
    }

    fn draw_scene(&mut self) {
        #[cfg(feature = "research")]
        if let Some(v) = self.research.as_ref() {
            v.render(&mut self.fb);
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
