//! Developer-only viewer for the original 1991 data. Compiled in only with
//! `--features research`, and never part of a release build.
//!
//!   cargo run --features research -- <path-to-original-game-data>
//!
//! Left and right step through arenas, up and down through sprite banks,
//! space cycles the frame.

use crate::framebuffer::Framebuffer;
use henge_formats::{piv, Cel, Library, Piv};
use winit::keyboard::KeyCode;

pub struct Viewer {
    lib: Library,
    arenas: Vec<String>,
    banks: Vec<String>,
    arena_idx: usize,
    bank_idx: usize,
    frame: usize,
    dirty: bool,
    cached: Option<(Piv, Piv, Cel)>,
}

impl Viewer {
    pub fn from_args() -> anyhow::Result<Option<Viewer>> {
        let Some(dir) = std::env::args().nth(1) else { return Ok(None) };
        let lib = Library::open(&dir)?;
        let arenas = lib.with_extension(&["t"]);
        let banks = lib.with_extension(&["cel", "ob"]);
        Ok(Some(Viewer {
            lib, arenas, banks,
            arena_idx: 0, bank_idx: 0, frame: 0,
            dirty: true, cached: None,
        }))
    }

    pub fn key(&mut self, code: KeyCode, down: bool) {
        if !down { return; }
        match code {
            KeyCode::ArrowRight => { self.arena_idx = (self.arena_idx + 1) % self.arenas.len().max(1); self.dirty = true; }
            KeyCode::ArrowLeft  => { self.arena_idx = (self.arena_idx + self.arenas.len().saturating_sub(1)) % self.arenas.len().max(1); self.dirty = true; }
            KeyCode::ArrowDown  => { self.bank_idx = (self.bank_idx + 1) % self.banks.len().max(1); self.frame = 0; self.dirty = true; }
            KeyCode::ArrowUp    => { self.bank_idx = (self.bank_idx + self.banks.len().saturating_sub(1)) % self.banks.len().max(1); self.frame = 0; self.dirty = true; }
            KeyCode::Space      => self.frame += 1,
            _ => {}
        }
    }

    pub fn update(&mut self) {
        if !self.dirty { return; }
        self.dirty = false;
        let name = self.arenas.get(self.arena_idx).cloned().unwrap_or_default();
        let prefix: String = name.chars().take_while(|c| c.is_alphabetic()).collect();
        let sheet = format!("{prefix}1.CMP");
        let backdrop = format!("{prefix}B1.CMP");
        let (Ok(sheet), Ok(backdrop)) = (self.lib.piv(&sheet), self.lib.piv(&backdrop)) else { return };
        let Ok(bank) = self.lib.cel(self.banks.get(self.bank_idx).map(String::as_str).unwrap_or("KN1.OB")) else { return };
        self.cached = Some((backdrop, sheet, bank));
    }

    pub fn render(&self, fb: &mut Framebuffer) {
        let Some((backdrop, sheet, bank)) = self.cached.as_ref() else {
            fb.clear(0);
            return;
        };
        fb.set_palette(&backdrop.palette);
        fb.pixels.copy_from_slice(&backdrop.pixels);

        if let Some(name) = self.arenas.get(self.arena_idx) {
            if let Ok(t) = self.lib.terrain(name) {
                let mut props = t.placements.clone();
                props.sort_by_key(|p| p.y);
                for p in props {
                    let c = sheet.cell(p.cell as usize);
                    fb.blit(&c.pixels, piv::CELL_W, piv::CELL_H, p.x as i32, p.y as i32, false);
                }
            }
        }
        if !bank.images.is_empty() {
            let img = &bank.images[self.frame % bank.images.len()];
            fb.blit(&img.pixels, img.width, img.height, 140, 60, false);
        }
    }
}
