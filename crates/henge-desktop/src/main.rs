//! Desktop entry point.
//!
//! Release builds contain no original-game data. `--features research` adds a
//! viewer for studying the 1991 files, which is a development tool only.

mod framebuffer;
mod map;
mod world;
#[cfg(feature = "research")]
mod research;

use framebuffer::Framebuffer;
use henge_assets::Registry;
use map::MapScene;
use henge_core::combat::Intent;
use henge_core::{SCREEN_H, SCREEN_W};
use world::World;
use std::num::NonZeroU32;
use std::rc::Rc;
use winit::event::{ElementState, Event, WindowEvent};
use winit::event_loop::EventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::WindowBuilder;

fn args_of() -> Vec<String> { std::env::args().collect() }

fn main() -> anyhow::Result<()> {
    let mut app = App::new()?;

    // Headless trace: run the simulation and report it, so combat can be checked
    // as behaviour rather than by squinting at screenshots.
    if let Some(i) = args_of().iter().position(|a| a == "--trace") {
        let a = args_of();
        let ticks: u64 = a.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(600);
        let arena: i32 = a.get(i + 2).and_then(|s| s.parse().ok()).unwrap_or(0);
        if let Some(w) = app.world.as_mut() { w.step_arena(arena); }
        // Travel while tracing, so the overworld loop is exercised too.
        app.keys[3] = true;
        let mut last = String::new();
        for t in 0..ticks {
            // Wander rather than walking into the edge and stopping.
            app.keys[3] = (t / 140) % 2 == 0;
            app.keys[2] = (t / 140) % 2 == 1;
            app.update();
            let line = match app.mode {
                Mode::Map if app.map.is_none() => break,
                Mode::Combat if app.world.is_none() => break,
                Mode::Map => {
                    let Some(m) = app.map.as_ref() else { break };
                    format!("{:>5}  MAP     day {:<3} step {:>4}  at {:>4},{:>4}  on {}",
                        t, m.state.day, m.state.steps, m.state.x, m.state.y,
                        m.last_terrain.name())
                }
                Mode::Combat => {
                    let Some(w) = app.world.as_ref() else { break };
                    format!("{:>5}  COMBAT  {:<8} player {:<6} hp{:>4}   foe {:<6} hp{:>4}",
                        t, w.family(), format!("{:?}", w.player.state), w.player.health,
                        format!("{:?}", w.foe.state), w.foe.health)
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
        for _ in 0..ticks {
            app.update();
        }
        app.render();
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
    reg: Registry,
    world: Option<World>,
    map: Option<MapScene>,
    mode: Mode,
    encounter_pick: u32,
    status: String,
    #[cfg(feature = "research")]
    research: Option<research::Viewer>,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum Mode { Map, Combat }

fn key_index(c: KeyCode) -> usize {
    match c {
        KeyCode::ArrowUp => 0,
        KeyCode::ArrowDown => 1,
        KeyCode::ArrowLeft => 2,
        KeyCode::ArrowRight => 3,
        KeyCode::BracketLeft => 4,
        KeyCode::BracketRight => 5,
        KeyCode::Space => 6,
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
        if world.is_none() {
            eprintln!("no arena data: {status}");
        } else {
            println!("{status}");
            println!("arrows travel and fight, space attacks, tab switches map/arena,");
            println!("[ and ] change arena, R restarts the bout, escape quits");
        }

        Ok(App {
            fb, fade: 255, tick: 0,
            keys: [false; 256],
            reg, world,
            mode: if map.is_some() { Mode::Map } else { Mode::Combat },
            map,
            encounter_pick: 0,
            status,
            #[cfg(feature = "research")]
            research: research::Viewer::from_args()?,
        })
    }

    fn key(&mut self, code: KeyCode, down: bool) {
        let i = key_index(code);
        if i < 256 {
            if down && self.world.is_some() {
                match code {
                    KeyCode::BracketLeft => self.world.as_mut().unwrap().step_arena(-1),
                    KeyCode::BracketRight => self.world.as_mut().unwrap().step_arena(1),
                    KeyCode::KeyR => self.world.as_mut().unwrap().reset(),
                    // Tab flips between the overworld and the arena, which is
                    // how the arena browser stays reachable.
                    KeyCode::Tab => {
                        self.mode = if self.mode == Mode::Map { Mode::Combat } else { Mode::Map };
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

    fn update(&mut self) {
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
                let mut start: Option<String> = None;
                if let Some(m) = self.map.as_mut() {
                    if m.update(dx, dy) {
                        start = Some(m.last_terrain.family().to_string());
                    }
                }
                if let (Some(family), Some(w)) = (start, self.world.as_mut()) {
                    self.encounter_pick = self.encounter_pick.wrapping_add(1);
                    w.set_family(&family, self.encounter_pick);
                    self.mode = Mode::Combat;
                }
            }
            Mode::Combat => {
                if let Some(w) = self.world.as_mut() {
                    w.update(Intent { dx, dy, attack: self.keys[6] });
                    // A finished bout hands control back to the map, or restarts
                    // in place when there is no map to go back to.
                    if w.over_for > 120 {
                        w.reset();
                        if self.map.is_some() {
                            self.mode = Mode::Map;
                        }
                    }
                }
            }
        }
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
        if self.mode == Mode::Map {
            if let Some(m) = self.map.as_ref() {
                if m.render(&mut self.reg, &mut self.fb).is_ok() {
                    return;
                }
            }
        }
        if let Some(w) = self.world.as_ref() {
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
