//! The overworld scene: travel across the map, watch the days pass, and get
//! dropped into a fight that matches the ground you are standing on.

use crate::framebuffer::Framebuffer;
use henge_assets::Registry;
use henge_core::overworld::{
    terrain_of_patch, Landscape, Overworld, Step, Terrain, MAP_H, MAP_W, TOKEN_H, TOKEN_W,
};

/// The map's icon set. Frames 0-9 are the knights' tokens in player colours,
/// 10-14 crystals, 43-46 creatures.
const TOKEN_SHEET: &str = "bank.mi";
/// The red knight, matching player one's colours in the arena.
const TOKEN_FRAME: usize = 3;
const MAP_SCENE: &str = "scene.map";

pub struct MapScene {
    pub state: Overworld,
    /// Cached map palette, so terrain can be read without borrowing the registry
    /// during the update.
    palette: Vec<u32>,
    pixels: Vec<u8>,
    /// The recovered `MapType` and `MapSLOW` grids. Empty only when the pack
    /// was baked without the unpacked executable to read them out of.
    land: Landscape,
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
        let land: Landscape = reg.read_data("data.overworld").unwrap_or_default();
        if land.is_empty() {
            eprintln!(
                "no overworld grid in the packs: terrain will be guessed from the map's colours"
            );
        }
        Ok(MapScene {
            state: Overworld::new(146, 115),
            palette,
            pixels,
            land,
            last_terrain: Terrain::Forest,
        })
    }

    /// Sample a patch around the traveller rather than a single pixel: the map
    /// art is dithered, so one pixel flips between two or three terrains as you
    /// walk and a fight would be picked at random.
    ///
    /// Fallback only. When the pack carries the real `MapType` grid the answer
    /// comes off that instead, and the art is never looked at.
    fn terrain_by_colour(&self) -> Terrain {
        const R: i32 = 4;
        let (cx, cy) = (self.state.x + TOKEN_W / 2, self.state.y + TOKEN_H / 2);
        let mut samples = Vec::with_capacity(((R * 2 + 1) * (R * 2 + 1)) as usize);
        for dy in -R..=R {
            for dx in -R..=R {
                let x = (cx + dx).clamp(0, MAP_W - 1) as usize;
                let y = (cy + dy).clamp(0, MAP_H - 1) as usize;
                let idx = self.pixels[y * MAP_W as usize + x] as usize;
                samples.push(*self.palette.get(idx).unwrap_or(&0));
            }
        }
        terrain_of_patch(samples)
    }

    pub fn terrain_here(&self) -> Terrain {
        if self.land.is_empty() {
            self.terrain_by_colour()
        } else {
            self.state.terrain(&self.land)
        }
    }

    /// One tick.
    pub fn update(&mut self, dx: i32, dy: i32) -> Step {
        let step = self.state.travel(dx, dy, &self.land);
        self.last_terrain = self.terrain_here();
        step
    }

    /// `here` is the place you are standing on or walking towards, if any. It
    /// takes the middle of the status bar off the terrain, because when a town
    /// is under your feet its name is the more useful of the two.
    pub fn render(&self, reg: &mut Registry, fb: &mut Framebuffer,
                  fonts: &std::collections::BTreeMap<String, crate::text::Font>,
                  run: &henge_core::run::Run,
                  here: Option<&str>, notice: Option<&str>) -> anyhow::Result<()> {
        fb.set_palette(&self.palette);
        fb.pixels.copy_from_slice(&self.pixels);

        // The status bar goes down first and the traveller on top of it. The
        // map is the whole screen in the original, and the recovered bound lets
        // the token walk to y=190, which is inside our bar; a token drawn first
        // simply vanishes down there. Ours is the bar, so ours is the one that
        // gives way.
        self.draw_status(reg, fb, fonts.get("bold"), run, here);
        self.draw_purse(reg, fb, fonts.get("small"), run, notice);
        self.draw_token(reg, fb);
        Ok(())
    }

    /// The purse, on a plate in the corner of the map.
    ///
    /// Not on the status bar, which is already full: the bold font is wide
    /// enough that "open ground" and "100 of 100" barely share a line as it is,
    /// and a third number would push one of them off. The corner is out of the
    /// way, and it is where the same number sits in the place screens, so what
    /// you are worth is always somewhere on the screen.
    ///
    /// A cutpurse's notice rides along beside it, because the map has no
    /// message line and something taken off you cannot go unsaid.
    fn draw_purse(&self, reg: &mut Registry, fb: &mut Framebuffer,
                  font: Option<&crate::text::Font>, run: &henge_core::run::Run,
                  notice: Option<&str>) {
        let Some(font) = font else { return };
        let luma = |c: u32| ((c >> 16) & 0xff) * 2 + ((c >> 8) & 0xff) * 3 + (c & 0xff);
        let (mut dark, mut light) = (0usize, 0usize);
        for i in 1..32 {
            if luma(fb.palette[i]) < luma(fb.palette[dark]) { dark = i; }
            if luma(fb.palette[i]) > luma(fb.palette[light]) { light = i; }
        }
        let mut line = format!("{} gold", run.gold);
        if let Some(n) = notice {
            line.push_str("   ");
            line.push_str(n);
        }
        let w = font.width(reg, &line);
        fb.rect(4, 4, w + 8, 12, dark as u8);
        font.draw(reg, fb, &line, 8, 7, light as u8);
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
        let Some(rect) = reg.sheet(TOKEN_SHEET).and_then(|r| r.value.frames.get(TOKEN_FRAME).copied())
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

        // The original's map position is the token's own top-left corner, which
        // is what `_MAP:SHOW` hands the blitter, so there is nothing to offset.
        let (x, y) = (self.state.x, self.state.y);

        // These icons are authored against the map's own palette, which is the
        // palette loaded here, so their indices already mean the right colours.
        // No translation, and no silhouette: the token draws as the little
        // helmeted knight it was drawn as.
        //
        // A dark halo first, so it does not dissolve into dithered ground.
        let shadow = Self::darkest(fb);
        for (ox, oy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            fb.blit_mask(&px, w, h, x + ox, y + oy, shadow);
        }
        fb.blit(&px, w, h, x, y, false);
    }

    /// The darkest entry in the loaded palette, for a halo that lifts a token
    /// off dithered ground without recolouring it.
    fn darkest(fb: &Framebuffer) -> u8 {
        let luma = |c: u32| (((c >> 16) & 0xff) * 2 + ((c >> 8) & 0xff) * 3 + (c & 0xff)) / 6;
        (1..32).min_by_key(|i| luma(fb.palette[*i])).unwrap_or(1) as u8
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
