//! The overworld scene: travel across the map, watch the days pass, and get
//! dropped into a fight that matches the ground you are standing on.

use crate::framebuffer::Framebuffer;
use henge_assets::Registry;
use henge_core::arena::Bounds;
use henge_core::overworld::{terrain_of_patch, Overworld, Terrain};

const TOKEN_SHEET: &str = "bank.ki";
const MAP_SCENE: &str = "scene.map";

pub struct MapScene {
    pub state: Overworld,
    /// Cached map palette, so terrain can be read without borrowing the registry
    /// during the update.
    palette: Vec<u32>,
    pixels: Vec<u8>,
    pub last_terrain: Terrain,
}

impl MapScene {
    pub fn load(reg: &mut Registry) -> anyhow::Result<MapScene> {
        let palette = reg
            .palette(&format!("palette.{MAP_SCENE}"))
            .map(|r| r.value.clone())
            .ok_or_else(|| anyhow::anyhow!("no palette for {MAP_SCENE}"))?;
        let img = reg.image(MAP_SCENE)?;
        anyhow::ensure!(
            img.width == 320 && img.height == 200,
            "{MAP_SCENE} is {}x{}, expected a full screen", img.width, img.height
        );
        let pixels = img.pixels.clone();
        Ok(MapScene {
            state: Overworld::new(150, 120),
            palette,
            pixels,
            last_terrain: Terrain::Forest,
        })
    }

    /// Travel is confined to the map area; the bottom strip is the status bar.
    pub fn bounds() -> Bounds {
        Bounds { left: 6, right: 313, top: 6, bottom: 176 }
    }

    /// Sample a patch around the traveller rather than a single pixel: the map
    /// art is dithered, so one pixel flips between two or three terrains as you
    /// walk and a fight would be picked at random.
    pub fn terrain_here(&self) -> Terrain {
        const R: i32 = 4;
        let mut samples = Vec::with_capacity(((R * 2 + 1) * (R * 2 + 1)) as usize);
        for dy in -R..=R {
            for dx in -R..=R {
                let x = (self.state.x + dx).clamp(0, 319) as usize;
                let y = (self.state.y + dy).clamp(0, 199) as usize;
                let idx = self.pixels[y * 320 + x] as usize;
                samples.push(*self.palette.get(idx).unwrap_or(&0));
            }
        }
        terrain_of_patch(samples)
    }

    /// One tick. Returns true when an encounter starts.
    pub fn update(&mut self, dx: i32, dy: i32) -> bool {
        let encounter = self.state.travel(dx, dy, Self::bounds());
        self.last_terrain = self.terrain_here();
        encounter
    }

    pub fn render(&self, reg: &mut Registry, fb: &mut Framebuffer) -> anyhow::Result<()> {
        fb.set_palette(&self.palette);
        fb.pixels.copy_from_slice(&self.pixels);

        // The traveller's token, anchored so its feet sit on the map position.
        if let Some(rect) = reg
            .sheet(TOKEN_SHEET)
            .and_then(|r| r.value.frames.first().copied())
        {
            if let Ok(img) = reg.image(TOKEN_SHEET) {
                let (w, h) = (rect.w as usize, rect.h as usize);
                let mut px = vec![0u8; w * h];
                for row in 0..h {
                    let src = (rect.y as usize + row) * img.width + rect.x as usize;
                    if src + w <= img.pixels.len() {
                        px[row * w..(row + 1) * w].copy_from_slice(&img.pixels[src..src + w]);
                    }
                }
                fb.blit(&px, w, h, self.state.x - w as i32 / 2, self.state.y - h as i32, false);
            }
        }

        self.draw_status(fb);
        Ok(())
    }

    fn draw_status(&self, fb: &mut Framebuffer) {
        let luma = |c: u32| ((c >> 16) & 0xff) * 2 + ((c >> 8) & 0xff) * 3 + (c & 0xff);
        let (mut dark, mut light) = (0usize, 0usize);
        for i in 1..32 {
            if luma(fb.palette[i]) < luma(fb.palette[dark]) { dark = i; }
            if luma(fb.palette[i]) > luma(fb.palette[light]) { light = i; }
        }
        fb.rect(0, 182, 320, 18, dark as u8);

        // No font is wired yet, so the day reads as a row of marks and the
        // terrain as a bar whose length names it. Crude, but it is honest about
        // what it knows rather than printing nothing.
        let day = self.state.day.min(40) as i32;
        for i in 0..day {
            fb.rect(6 + i * 3, 186, 2, 4, light as u8);
        }
        let width = match self.last_terrain {
            Terrain::Forest => 20,
            Terrain::Glade => 40,
            Terrain::Swamp => 60,
            Terrain::Waste => 80,
        };
        fb.rect(6, 193, width, 3, light as u8);

        // Progress through the current day.
        let frac = (self.state.steps * 300 / self.state.steps_per_day.max(1)) as i32;
        fb.rect(10, 178, frac, 2, light as u8);
    }
}
