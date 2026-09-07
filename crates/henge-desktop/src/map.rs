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

    /// `here` is the place you are standing on or walking towards, if any. It
    /// takes the middle of the status bar off the terrain, because when a town
    /// is under your feet its name is the more useful of the two.
    pub fn render(&self, reg: &mut Registry, fb: &mut Framebuffer,
                  font: Option<&crate::text::Font>, run: &henge_core::run::Run,
                  here: Option<&str>) -> anyhow::Result<()> {
        fb.set_palette(&self.palette);
        fb.pixels.copy_from_slice(&self.pixels);

        self.draw_token(reg, fb);

        self.draw_status(reg, fb, font, run, here);
        Ok(())
    }

    /// Draw the traveller.
    ///
    /// Not with the sprite's own colours. Sheet pixels are palette *indices*,
    /// and nothing records which palette they were baked against, so blitting
    /// them onto the map writes indices that mean entirely different colours
    /// here. The result was a smear of browns and blues that read as map
    /// dithering, which is why the traveller looked absent rather than wrong.
    ///
    /// A map marker wants to be found at a glance anyway, so it is drawn as a
    /// silhouette in whichever colour stands furthest from the ground beneath
    /// it, outlined in the opposite extreme. That keeps it legible over dark
    /// forest and over snow without either being a special case.
    fn draw_token(&self, reg: &mut Registry, fb: &mut Framebuffer) {
        let Some(rect) = reg.sheet(TOKEN_SHEET).and_then(|r| r.value.frames.first().copied())
        else { return };
        let Ok(img) = reg.image(TOKEN_SHEET) else { return };

        let (w, h) = (rect.w as usize, rect.h as usize);
        let mut px = vec![0u8; w * h];
        for row in 0..h {
            let src = (rect.y as usize + row) * img.width + rect.x as usize;
            if src + w <= img.pixels.len() {
                px[row * w..(row + 1) * w].copy_from_slice(&img.pixels[src..src + w]);
            }
        }

        let (x, y) = (self.state.x - w as i32 / 2, self.state.y - h as i32);
        let (fill, outline) = Self::ink_over(fb, x, y, w, h);

        // Outline first, as the silhouette nudged one pixel each way, then the
        // fill on top. Without it the marker dissolves into dithered ground.
        for (ox, oy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            fb.blit_mask(&px, w, h, x + ox, y + oy, outline);
        }
        fb.blit_mask(&px, w, h, x, y, fill);
    }

    /// Two colours that stand out against the ground under a rectangle: one far
    /// from its average brightness, and one far from that.
    fn ink_over(fb: &Framebuffer, x: i32, y: i32, w: usize, h: usize) -> (u8, u8) {
        let luma = |c: u32| (((c >> 16) & 0xff) * 2 + ((c >> 8) & 0xff) * 3 + (c & 0xff)) / 6;
        let mut total = 0u32;
        let mut n = 0u32;
        for yy in y..y + h as i32 {
            for xx in x..x + w as i32 {
                if (0..320).contains(&xx) && (0..200).contains(&yy) {
                    total += luma(fb.palette[(fb.pixels[yy as usize * 320 + xx as usize] & 0x1f) as usize]);
                    n += 1;
                }
            }
        }
        let ground = if n == 0 { 128 } else { total / n };
        let furthest = |from: u32| {
            (1..32)
                .max_by_key(|i| luma(fb.palette[*i]).abs_diff(from))
                .unwrap_or(1) as u8
        };
        let fill = furthest(ground);
        let outline = furthest(luma(fb.palette[fill as usize]));
        (fill, outline)
    }

    fn draw_status(&self, reg: &mut Registry, fb: &mut Framebuffer,
                   font: Option<&crate::text::Font>, run: &henge_core::run::Run,
                   here: Option<&str>) {
        let luma = |c: u32| ((c >> 16) & 0xff) * 2 + ((c >> 8) & 0xff) * 3 + (c & 0xff);
        let (mut dark, mut light) = (0usize, 0usize);
        for i in 1..32 {
            if luma(fb.palette[i]) < luma(fb.palette[dark]) { dark = i; }
            if luma(fb.palette[i]) > luma(fb.palette[light]) { light = i; }
        }
        fb.rect(0, 178, 320, 22, dark as u8);

        let Some(font) = font else { return };
        let left = format!("Day {}", self.state.day);
        let lw = font.width(reg, &left);
        font.draw(reg, fb, &left, 6, 182, light as u8);
        let right = format!("{} of {}", run.health.max(0), run.max_health);
        let rw = font.width(reg, &right);
        font.draw(reg, fb, &right, 314 - rw, 182, light as u8);

        // The middle is centred in what is left over, not on the screen: this
        // font is wide, and "open ground" centred on 320 runs straight through
        // the health readout.
        let mid = here.unwrap_or_else(|| self.last_terrain.name());
        let (from, to) = (6 + lw + 6, 314 - rw - 6);
        let mw = font.width(reg, mid);
        if mw <= to - from {
            font.draw(reg, fb, mid, from + (to - from - mw) / 2, 182, light as u8);
        }
    }
}
