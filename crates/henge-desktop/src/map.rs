//! The overworld scene: travel across the map, watch the days pass, and get
//! dropped into a fight that matches the ground you are standing on.

use crate::framebuffer::Framebuffer;
use henge_assets::Registry;
use henge_core::overworld::{Landscape, Overworld, Step, Terrain};

/// The map's icon set. Frames 0-4 are the four knights' tokens and a fifth in
/// dark purple, each in its own colour out of `MAP.CMP`'s palette: 30 blue, 29
/// gold, 8 green, 28 red. Frames 5-9 are the same five with index 1 and index
/// 31 added, and 31 is the entry `_MAP:MapEffects` puts a glow on
/// (`mov ax, 0x1f; mov bx, 0xff; mov cx, 1`), so the token that carries it
/// breathes. That is how the original says which of the tokens is you.
const TOKEN_SHEET: &str = "bank.mi";
/// `_MAP:SHOW`: `mov ax, [di+0x20]; add ax, 5`, so the traveller's own token is
/// his seat plus five, which is the glowing one. `DisplayOtherKnights` blits
/// the same seat without the five, so the other three do not glow.
const TOKEN_FIRST: usize = 5;
const MAP_SCENE: &str = "scene.map";

/// `MI.C` frame 0x14, which `_MAP:DisplayLairs` blits at every lair still on
/// the map: `mov ax, 0x14; mov bx, [si+0xa]; mov cx, [si+0xc]` at image 0xa286,
/// skipped when the record's x has gone negative.
pub const LAIR_FRAME: usize = 0x14;

/// `MI.C` frame 0x1f, which `MOON:CheckLairEncounter` blits at the lair the
/// traveller is standing on: `mov bx, [si+0xa]; mov cx, [si+0xc];
/// mov ax, 0x1f; les si, [0x8975]; call <blit>` at image 0x88f. It is nine by
/// five, the same shape as the lair marker it covers, and it is the only thing
/// on the map that says which of them is under your feet.
const LAIR_HERE_FRAME: usize = 0x1f;

/// `MI.C` frame 0x20, the paper. One hundred and seventy four by fifty one, and
/// the only frame in the bank that size: `_MAP:InitPaper` blits it with
/// `mov ax, 0x20; mov bx, 0x32; mov cx, 0x64` at image 0xafbc, having just set
/// `PaperX` and `PaperY` to the same 0x32 and 0x64.
const PAPER_FRAME: usize = 0x20;
const PAPER_X: i32 = 0x32;
const PAPER_Y: i32 = 0x64;
/// `_MAP:knightopt` at `DS:0xc31f`, which `CreatePaper` copies straight after
/// the knight's name to make the heading: `dec bp; mov di, 0xc31f; call CopyText`.
const PAPER_OPT: &str = " may ... ";

/// The panel the paper lays out, as `_MAP:CreatePaper` does it.
///
/// The knight's own token goes at `PaperX + 5, PaperY + 5` in the frame his
/// seat names with nothing added (`mov ax, [si+0x20]`, image 0xaeed), the
/// heading fifteen pixels right of the corner and five down, and then `PaperY`
/// takes one step of fifteen before the first line and a step of six after
/// every one of them.
pub struct Paper<'a> {
    /// The knight's name, which `InitPaper` copies out of `[si+0x4c]`.
    pub knight: &'a str,
    pub seat: usize,
    /// The numbered lines, already composed by `henge_core::place::Overlaps`.
    pub lines: &'a [String],
}

/// What the map has to be told about the world before it can draw it.
///
/// The map picture is the whole screen and `_MAP:SHOW`, `_MAP:DisplayLairs` and
/// `_MAP:DisplayOtherKnights` are nearly all of what goes on top of it; the one
/// thing beside them is `MOON:CheckLairEncounter`'s mark on the lair you are
/// standing on, and the paper when `_MAP:DisplayStack` has put it up. There is
/// no status bar, no purse and no message line anywhere on it.
#[derive(Default)]
pub struct Marks<'a> {
    /// `_MAP:DisplayLairs`: every place whose pack gives it an `MI.C` frame, as
    /// `[x, y, frame]`.
    pub icons: &'a [(i32, i32, usize)],
    /// The lairs the traveller's token overlaps, which take frame 0x1f on top.
    pub marked: &'a [(i32, i32)],
    /// The paper, when there is more than one thing under your feet.
    pub paper: Option<Paper<'a>>,
}

pub struct MapScene {
    pub state: Overworld,
    /// Cached map palette, so terrain can be read without borrowing the registry
    /// during the update.
    palette: Vec<u32>,
    pixels: Vec<u8>,
    /// The recovered `MapType` and `MapSLOW` grids.
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
            "{MAP_SCENE} is {}x{}, expected a full screen",
            img.width,
            img.height
        );
        let pixels = img.pixels.clone();
        // `_MAP:MapType` and `_MAP:MapSLOW`, read out of the image at bake time.
        // The baker refuses a pack whose map picture and grids disagree, so this
        // is always the original's own table and there is nothing to guess from.
        let land: Landscape = reg.read_data("data.overworld")?;
        anyhow::ensure!(
            !land.is_empty(),
            "the pack carries no overworld grid, so `_MAP:FindLandscape` has nothing to read"
        );
        Ok(MapScene {
            state: Overworld::new(146, 115),
            palette,
            pixels,
            land,
            last_terrain: Terrain::Forest,
        })
    }

    pub fn terrain_here(&self) -> Terrain {
        self.state.terrain(&self.land)
    }

    /// One tick.
    pub fn update(&mut self, dx: i32, dy: i32) -> Step {
        let step = self.state.travel(dx, dy, &self.land);
        self.last_terrain = self.terrain_here();
        step
    }

    /// One frame of the map, in the order the original's own frame draws it.
    ///
    /// The loop body at image 0xa2d7: restore the picture, `DisplayLairs`,
    /// `DisplayOtherKnights`, then `FOLLOW`, whose encounter walk marks the lair
    /// under your feet, then `SHOW`. `DisplayStack` adds `CreatePaper` after
    /// `SHOW` when it has more than one entry to offer.
    pub fn render(
        &self,
        reg: &mut Registry,
        fb: &mut Framebuffer,
        fonts: &std::collections::BTreeMap<String, crate::text::Font>,
        run: &henge_core::run::Run,
        world: &Marks,
    ) -> anyhow::Result<()> {
        fb.set_palette(&self.palette);
        fb.pixels.copy_from_slice(&self.pixels);
        self.draw_icons(reg, fb, world.icons, world.marked);
        self.draw_token(reg, fb, run.knight.seat);
        if let Some(paper) = world.paper.as_ref() {
            self.draw_paper(reg, fb, fonts.get("small"), paper);
        }
        Ok(())
    }

    /// The places the map has to draw for itself.
    ///
    /// The towns, Stonehenge, the Valley of the Gods and the wizard's tower are
    /// painted into `MAP.CMP` and need nothing. A lair is not: `_MAP:DisplayLairs`
    /// walks the lair table and blits `MI.C` frame 0x14 at every one whose x is
    /// not negative, which is how a lair leaves the map when it has been beaten
    /// and stripped. Frame 0x1f goes on top of the one you are standing on,
    /// which is `MOON:CheckLairEncounter`'s own blit.
    ///
    /// Both are authored against the map's own palette, which is the palette
    /// loaded here, so they draw in their own colours with nothing translated
    /// and nothing flattened. **The rest of `MI.C` from 0x15 up are outlines,
    /// not pictures**: frame 0x19 is the silhouette of a town wall, 0x1b a ring
    /// of stones, 0x1c the Valley of the Gods and 0x1e the wizard's tower, each
    /// one pixel wide and drawn entirely in palette index 31, which `MAP.CMP`
    /// holds as magenta. Nothing in the original blits them; they are there for
    /// `MOON:GetWIDTH` to measure a place's box out of, and `MAP.CMP` already
    /// paints every place they name. This used to draw them as flat silhouettes
    /// in one colour, which was a marker the original does not have, in a
    /// colour the frame does not carry.
    fn draw_icons(
        &self,
        reg: &mut Registry,
        fb: &mut Framebuffer,
        icons: &[(i32, i32, usize)],
        marked: &[(i32, i32)],
    ) {
        for (x, y, frame) in icons {
            crate::sprite::draw(reg, fb, TOKEN_SHEET, *frame, *x, *y, false);
        }
        for (x, y) in marked {
            crate::sprite::draw(reg, fb, TOKEN_SHEET, LAIR_HERE_FRAME, *x, *y, false);
        }
    }

    /// Draw the traveller, as `_MAP:SHOW` draws him: his own token, in his own
    /// colours, at his own top left corner, with nothing added.
    ///
    /// The halo this used to draw underneath was a compensation for the wrong
    /// frame. `MI.C` is authored against `MAP.CMP`'s own palette, which is the
    /// palette loaded here, and the seat plus five carries index 31, the entry
    /// `MapEffects` glows. So the token lifts off the ground by breathing,
    /// which is the original's answer and not an outline.
    fn draw_token(&self, reg: &mut Registry, fb: &mut Framebuffer, seat: usize) {
        let frame = TOKEN_FIRST + seat.min(4);
        let (x, y) = (self.state.x, self.state.y);
        // The original's map position is the token's own top-left corner, which
        // is what `_MAP:SHOW` hands the blitter, so there is nothing to offset.
        crate::sprite::draw(reg, fb, TOKEN_SHEET, frame, x, y, false);
    }

    /// `_MAP:CreatePaper`, image 0xaed4, line for line.
    ///
    /// ```text
    /// 0xaed5  call InitPaper          ; PaperX = 0x32, PaperY = 0x64, blit cel 0x20 there
    /// 0xaed8  dec bp / mov di, knightopt / call CopyText     the heading
    /// 0xaee7  add bx, 5 / add cx, 5 / mov ax, [si+0x20]      his token at +5, +5
    /// 0xaefe  add ax, 0xf / add bx, 5                        the heading at +15, +5
    /// 0xaf0f  add [PaperY], 0xf                              one step before the list
    /// 0xaf14  mov [KEYNUM], 0x31                             the digit, as a character
    /// 0xaf3b  add ax, 5 / add bx, 5                          each line at +5, PaperY + 5
    /// 0xaf52  add [PaperY], 6                                and six pixels to the next
    /// ```
    ///
    /// Six pixels a line is the small face's step and not the bold one's, which
    /// is twenty tall; `docs/COMPLETE.md` had this as fifteen pixels a line,
    /// which is the single step taken before the first line and not the step
    /// between them.
    fn draw_paper(
        &self,
        reg: &mut Registry,
        fb: &mut Framebuffer,
        font: Option<&crate::text::Font>,
        paper: &Paper,
    ) {
        crate::sprite::draw(reg, fb, TOKEN_SHEET, PAPER_FRAME, PAPER_X, PAPER_Y, false);
        crate::sprite::draw(
            reg,
            fb,
            TOKEN_SHEET,
            paper.seat.min(4),
            PAPER_X + 5,
            PAPER_Y + 5,
            false,
        );
        let Some(font) = font else { return };
        let heading = format!("{}{PAPER_OPT}", paper.knight);
        font.draw_own(reg, fb, &heading, PAPER_X + 0xf, PAPER_Y + 5);
        let mut y = PAPER_Y + 0xf + 5;
        for line in paper.lines {
            font.draw_own(reg, fb, line, PAPER_X + 5, y);
            y += 6;
        }
    }
}
