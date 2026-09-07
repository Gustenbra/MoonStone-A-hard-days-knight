//! Bakes the original 1991 game into a *reference pack*: ordinary indexed PNGs,
//! WAV files and a manifest.
//!
//! This is the seam that makes incremental replacement work. After baking, the
//! game engine only ever reads PNG, WAV and JSON. It has no idea the original
//! formats exist. Replacing a character therefore means dropping new PNGs into
//! the `original` pack, not touching a line of code.
//!
//! The pack it writes is marked `derived-from-original`, so a release build
//! refuses to start while anything still resolves to it.
//!
//!   henge-bake <game-data-dir> <packs-dir>/reference

use anyhow::Context;
use henge_assets::{FrameRect, Manifest, Provenance, Sheet};
use henge_formats::{piv, voc, Collide, Library, Sprite};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// Sprite banks grouped into the actors they actually belong to.
const ACTORS: &[(&str, &[&str])] = &[
    ("knight", &["KN1.OB", "KN2.OB", "KN3.OB", "KN4.OB", "KN5.OB"]),
    ("hero", &["HE1.OB", "HE2.OB", "HE3.OB"]),
    ("troll", &["TROLL1.CEL", "TROLL2.CEL"]),
    ("trogg_axe", &["TROGGAX1.CEL", "TROGGAX2.CEL"]),
    ("trogg_spear", &["TROGGSP1.CEL", "TROGGSP2.CEL"]),
    ("ratmen", &["RATMEN1.CEL", "RATMEN2.CEL"]),
    ("mudmen", &["MUDMEN1.CEL", "MUDMEN2.CEL"]),
    ("demon", &["DEMON1.CEL", "DEMON2.CEL", "DEMON3.CEL", "DEMON4.CEL"]),
    ("dragon", &["DRAGON1.CEL", "DRAGON2.CEL", "DRAGON5.CEL"]),
    ("balok", &["BALOK1.CEL", "BALOK2.CEL", "BALOK3.CEL"]),
    ("gore", &["BLO.CEL"]),
];

/// Arena families, each with its scenery sheet, full-screen backdrop, and the
/// eight arenas it rotates through.
///
/// **Recovered.** `_LOADER` holds four tables of eight filename pointers,
/// `PlainTable`, `ForestTable`, `SwampTable` and `WasteTable`, and picks the
/// sheet with `Table[counter]`, `inc counter`, `and counter, 7`. The scenery
/// sheet comes from `TileTable`, four words indexed by the landscape code,
/// which reads `FO1` for both plain and forest, `SW1` for swamp and `WA1` for
/// waste. The names below are those tables, in their order.
const ARENAS: &[(&str, &str, &str, [&str; 8])] = &[
    ("waste", "WA1.CMP", "WAB1.CMP",
     ["wa1", "wa2", "wa3", "wa4", "wa5", "wa6", "wa7", "wa8"]),
    ("forest", "FO1.CMP", "FOB1.CMP",
     ["fo1", "fo2", "fo3", "fo4", "fo5", "fo6", "fo7", "fo8"]),
    ("swamp", "SW1.CMP", "SWB1.CMP",
     ["sw1", "sw2", "sw3", "sw4", "sw5", "sw6", "sw7", "sw8"]),
    // The original calls this family "plain" and gives it the GL sheets over
    // the GLB1 backdrop. Its scenery comes from FO1 like the forest's, not
    // from FO2: FO2 is the sheet every family reaches for when a placement's
    // selector byte is 4.
    ("glade", "FO1.CMP", "GLB1.CMP",
     ["gl1", "gl2", "gl3", "gl4", "gl5", "gl6", "gl7", "gl8"]),
];

/// The sheet a placement asks for when its selector byte is 4, whatever the
/// family. See `Family::tiles` in `henge-core` for what established it.
const SHARED_TILES: &str = "FO2.CMP";

/// Where the terrain grids live in the fully unpacked `MAIN.EXE` load image.
///
/// These are the addresses `tools/symbolmap.py` reports for `_MAP:MapType` and
/// `_MAP:MapSLOW`, and the checks below refuse anything that does not look like
/// those tables, so a wrong image is caught rather than baked.
const IMAGE_LEN: usize = 178_224;
const MAPTYPE_AT: usize = 125_890;
const MAPSLOW_AT: usize = 124_890;
/// 40 columns by 26 rows. The going grid is read with the same index, so it is
/// taken at the same size even though `MapSLOW` itself is only 25 rows: the
/// original reads that last row past the end of it, and so do we.
const GRID_LEN: usize = 40 * 26;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let src = args.next().unwrap_or_else(|| ".".into());
    let out = args.next().unwrap_or_else(|| "packs/reference".into());
    let out = Path::new(&out);

    let lib = Library::open(&src).context("opening the original game data")?;
    fs::create_dir_all(out.join("sheets"))?;
    fs::create_dir_all(out.join("sounds"))?;

    let mut m = Manifest::new("reference", Provenance::DerivedFromOriginal);

    // Palettes, named after the arena family that owns them.
    for (name, sheet, _, _) in ARENAS {
        if let Ok(p) = lib.piv(sheet) {
            m.palettes.insert(format!("palette.{name}"), p.palette);
        }
    }
    let fallback = m
        .palettes
        .values()
        .next()
        .cloned()
        .unwrap_or_else(|| (0..32).map(|i: u32| (i * 8) << 16 | (i * 8) << 8 | i * 8).collect());

    // One sheet per actor, all its banks packed together in bank order.
    for (actor, banks) in ACTORS {
        let mut frames: Vec<Sprite> = Vec::new();
        for b in *banks {
            if let Ok(c) = lib.cel(b) {
                frames.extend(c.images);
            }
        }
        if frames.is_empty() {
            continue;
        }
        let id = format!("actor.{actor}");
        let file = format!("sheets/{actor}.png");
        let sheet = pack_sheet(&frames);
        write_indexed(&out.join(&file), sheet.width, sheet.height, &sheet.pixels, &fallback)?;
        m.sheets.insert(id, Sheet { file, frames: sheet.rects });
    }

    // Everything else that is a sprite bank, so nothing is silently dropped.
    // The .C files (BE1, WI1, HEN1, MI) are banks too, in the same format.
    let claimed: Vec<String> = ACTORS
        .iter()
        .flat_map(|(_, banks)| banks.iter().map(|b| b.to_string()))
        .collect();
    for name in lib.with_extension(&["cel", "ob", "c", "f", "fon"]) {
        if claimed.contains(&name) {
            continue;
        }
        let Ok(c) = lib.cel(&name) else { continue };
        if c.images.is_empty() {
            continue;
        }
        let stem = name.split('.').next().unwrap_or(&name).to_lowercase();
        let file = format!("sheets/bank_{stem}.png");
        let sheet = pack_sheet(&c.images);
        write_indexed(&out.join(&file), sheet.width, sheet.height, &sheet.pixels, &fallback)?;
        m.sheets.insert(format!("bank.{stem}"), Sheet { file, frames: sheet.rects });
    }

    // Full-screen images: towns, the map, intro art. .P files are PIVs too.
    for name in lib.with_extension(&["piv", "cmp", "p"]) {
        if let Ok(p) = lib.piv(&name) {
            let stem = name.split('.').next().unwrap_or(&name).to_lowercase();
            let file = format!("sheets/scene_{stem}.png");
            write_indexed(&out.join(&file), piv::W, piv::H, &p.pixels, &p.palette)?;
            m.sheets.insert(
                format!("scene.{stem}"),
                Sheet {
                    file,
                    frames: vec![FrameRect {
                        x: 0, y: 0, w: piv::W as u32, h: piv::H as u32, ox: 0, oy: 0,
                    }],
                },
            );
            m.palettes.insert(format!("palette.scene.{stem}"), p.palette);
        }
    }

    // Samples, converted to plain WAV so the engine never learns about VOC.
    let mut sounds = 0;
    for name in lib.names() {
        let Ok(bytes) = lib.bytes(&name) else { continue };
        if bytes.len() < 20 || &bytes[..19] != b"Creative Voice File" {
            continue;
        }
        match voc::parse(&bytes) {
            Ok(s) => {
                let stem = name.split('.').next().unwrap_or(&name).to_lowercase();
                let file = format!("sounds/{stem}.wav");
                fs::write(out.join(&file), s.to_wav())?;
                m.sounds.insert(format!("sfx.{stem}"), file);
                sounds += 1;
            }
            Err(e) => eprintln!("  {name}: {e}"),
        }
    }

    // Arena layouts, keyed by the family whose sheets they draw from.
    fs::create_dir_all(out.join("data"))?;
    let mut arenas = BTreeMap::new();
    for name in lib.with_extension(&["t"]) {
        let Ok(t) = lib.terrain(&name) else { continue };
        // The F09/SW9 stubs carry garbage bounds and no usable placements.
        let sane = t.left < t.right
            && t.top < t.bottom
            && t.right < 640
            && t.bottom < 400;
        if t.placements.is_empty() || !sane {
            continue;
        }
        let stem = name.split('.').next().unwrap_or(&name).to_lowercase();
        let family = ARENAS
            .iter()
            .find(|(f, _, _, _)| stem.starts_with(&f[..2]))
            .map(|(f, _, _, _)| *f)
            .unwrap_or("forest");
        arenas.insert(stem, serde_json::json!({ "family": family, "terrain": t }));
    }
    fs::write(out.join("data/arenas.json"), serde_json::to_string(&arenas)?)?;
    m.data.insert("data.arenas".into(), "data/arenas.json".into());

    if let Ok(bytes) = lib.bytes("COLLIDE.HIT") {
        let c = Collide::parse(&bytes)?;
        fs::write(out.join("data/hitlines.json"), serde_json::to_string(&c)?)?;
        m.data.insert("data.hitlines".into(), "data/hitlines.json".into());
    }

    // Which sheets each arena family draws from, and the eight arenas its
    // counter rotates through.
    let key = |f: &str| format!("scene.{}", f.split('.').next().unwrap_or(f).to_lowercase());
    let families: BTreeMap<&str, serde_json::Value> = ARENAS
        .iter()
        .map(|(name, sheet, backdrop, rotation)| {
            (*name, serde_json::json!({
                "sheet": key(sheet),
                "backdrop": key(backdrop),
                "tiles": { "4": key(SHARED_TILES) },
                "arenas": rotation,
            }))
        })
        .collect();
    fs::write(out.join("data/families.json"), serde_json::to_string(&families)?)?;
    m.data.insert("data.families".into(), "data/families.json".into());

    fs::write(out.join("data/actors.json"), actor_definitions())?;
    m.data.insert("data.actors".into(), "data/actors.json".into());

    fs::write(out.join("data/fonts.json"), font_definitions())?;
    m.data.insert("data.fonts".into(), "data/fonts.json".into());

    // The overworld's two grids, lifted out of the unpacked executable.
    match overworld_tables(&src) {
        Ok(Some(land)) => {
            fs::write(out.join("data/overworld.json"), serde_json::to_string(&land)?)?;
            m.data.insert("data.overworld".into(), "data/overworld.json".into());
        }
        Ok(None) => eprintln!(
            "no unpacked MAIN.EXE image found: baking without the terrain grids.\n  \
             make one with `python3 tools/symbolmap.py MAIN.EXE symbols.json \
             --image research/main.final.bin`"
        ),
        Err(e) => eprintln!("overworld tables: {e:#}"),
    }

    // Place icons, so a town is the size the artwork drew it.
    let icons: BTreeMap<u8, (i32, i32)> = lib
        .cel("MI.C")
        .map(|c| {
            c.images
                .iter()
                .enumerate()
                .map(|(i, s)| (i as u8, (s.real_width as i32, s.height as i32)))
                .collect()
        })
        .unwrap_or_default();
    fs::write(out.join("data/places.json"), place_definitions(&icons))?;
    m.data.insert("data.places".into(), "data/places.json".into());

    fs::write(out.join("data/items.json"), item_definitions())?;
    m.data.insert("data.items".into(), "data/items.json".into());

    fs::write(out.join("data/knights.json"), knight_definitions())?;
    m.data.insert("data.knights".into(), "data/knights.json".into());

    fs::write(out.join("manifest.json"), serde_json::to_string_pretty(&m)?)?;
    println!(
        "baked {} sheets, {} sounds, {} palettes, {} data blobs into {}",
        m.sheets.len(), sounds, m.palettes.len(), m.data.len(), out.display()
    );
    println!("marked derived-from-original: a release build will refuse to ship it.");
    Ok(())
}

struct Packed {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
    rects: Vec<FrameRect>,
}

/// Row packing: simple, stable, and the frame order stays the bank order, which
/// matters because the animation tables index into it.
fn pack_sheet(frames: &[Sprite]) -> Packed {
    const MAX_W: usize = 1024;
    const GAP: usize = 1;

    let mut rects = Vec::with_capacity(frames.len());
    let (mut x, mut y, mut row_h, mut width) = (0usize, 0usize, 0usize, 0usize);
    for f in frames {
        let (w, h) = (f.real_width.max(1), f.height.max(1));
        if x + w > MAX_W && x > 0 {
            x = 0;
            y += row_h + GAP;
            row_h = 0;
        }
        rects.push(FrameRect {
            x: x as u32, y: y as u32, w: w as u32, h: h as u32,
            // Anchor at the bottom centre: this game positions everything by feet.
            ox: -((w / 2) as i32), oy: -(h as i32),
        });
        x += w + GAP;
        row_h = row_h.max(h);
        width = width.max(x);
    }
    let height = y + row_h;
    let width = width.max(1);
    let height = height.max(1);

    let mut pixels = vec![0u8; width * height];
    for (f, r) in frames.iter().zip(&rects) {
        for row in 0..r.h as usize {
            for col in 0..r.w as usize {
                let v = f.pixels[row * f.width + col];
                pixels[(r.y as usize + row) * width + r.x as usize + col] = v;
            }
        }
    }
    Packed { width, height, pixels, rects }
}

fn write_indexed(
    path: &Path, w: usize, h: usize, pixels: &[u8], palette: &[u32],
) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let file = fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Indexed);
    enc.set_depth(png::BitDepth::Eight);
    let mut pal = Vec::with_capacity(palette.len() * 3);
    for c in palette {
        pal.extend_from_slice(&[(c >> 16) as u8, (c >> 8) as u8, *c as u8]);
    }
    enc.set_palette(pal);
    let mut alpha = vec![255u8; palette.len()];
    if !alpha.is_empty() {
        alpha[0] = 0;
    }
    enc.set_trns(alpha);
    enc.write_header()?.write_image_data(pixels)?;
    Ok(())
}

/// Animation and combat definitions for the knight, authored by reading the
/// sprite bank frame by frame.
///
/// These are not recovered from the original. `MAIN.EXE` runs animations as
/// scripts on a small task VM, and characters are composed of several sprite
/// parts per frame. That VM is now decoded (`docs/TASKVM.md`) and the original
/// scripts can be lifted, but nothing here reads them yet: build-order item 27
/// replaces these. Every number here was chosen by looking at the frames and by
/// feel, which is what our own artwork will need anyway.
///
/// Frame indices only mean anything against the reference sheet, so this lands
/// in the reference pack and is marked derived along with everything else in it.
///
/// Hit lines are in the actor's own space: x forward, y up from the feet.
fn actor_definitions() -> String {
    // KN1.OB, read off the bank:
    //   10..17  an eight frame walk cycle, side on
    //   34      guard, sword held across the body
    //   43      the raise, striding in with the arm up
    //   41      the downswing
    //   40      the follow through
    //   35      recoiling
    //   49, 46, 48, 45  the death collapse
    serde_json::json!({
        "knight": {
            "sheet": "actor.knight",
            "health": 100,
            "speed_x": 2,
            "speed_y": 1,
            "reach": 38,
            "depth_tolerance": 6,
            "attack_cooldown": 45,
            // What a fallen knight is carrying, for whoever is left standing.
            // Not recovered: the original names `BESTOWGOLD` and a `GOLD`
            // readout but no table of what anything is worth, so this is a
            // number chosen against the prices below. Three foes put down pays
            // for a flask and leaves change.
            "bounty": 15,
            "body": [-9, 0, 9, 50],
            "sequences": {
                "idle": { "name": "idle", "end": "Loop", "frames": [
                    { "sprite": 10, "ticks": 10 }
                ]},
                "walk": { "name": "walk", "end": "Loop", "frames": [
                    { "sprite": 10, "ticks": 4 }, { "sprite": 11, "ticks": 4 },
                    { "sprite": 12, "ticks": 4 }, { "sprite": 13, "ticks": 4 },
                    { "sprite": 14, "ticks": 4 }, { "sprite": 15, "ticks": 4 },
                    { "sprite": 16, "ticks": 4 }, { "sprite": 17, "ticks": 4 }
                ]},
                // The swing carries the fighter forward through its own dx, so
                // spacing is decided when you commit rather than while you swing.
                "attack": { "name": "attack", "end": "HoldLast", "frames": [
                    { "sprite": 34, "ticks": 4 },
                    { "sprite": 43, "ticks": 3, "dx": 2 },
                    { "sprite": 41, "ticks": 3, "dx": 3,
                      "hit": [[14, 46], [34, 34], [40, 20]] },
                    { "sprite": 40, "ticks": 4,
                      "hit": [[16, 30], [38, 22], [44, 12]] },
                    { "sprite": 34, "ticks": 6 }
                ]},
                "hurt": { "name": "hurt", "end": "HoldLast", "frames": [
                    { "sprite": 35, "ticks": 10 }
                ]},
                "death": { "name": "death", "end": "HoldLast", "frames": [
                    { "sprite": 49, "ticks": 4 }, { "sprite": 46, "ticks": 4 },
                    { "sprite": 48, "ticks": 5 }, { "sprite": 45, "ticks": 200 }
                ]}
            }
        }
    })
    .to_string()
}

/// Which character each glyph in a font bank draws.
///
/// The game looks glyphs up through a table inside `MAIN.EXE` that is not
/// recovered, so this was read off the artwork: the banks turned out to run
/// A-Z, then a-z, then 0-9, then punctuation.
///
/// A few glyphs near the end are ornaments whose meaning is not obvious. They
/// are left unmapped rather than guessed at, which costs nothing: an unmapped
/// glyph is simply never drawn.
fn font_definitions() -> String {
    const LETTERS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!?.,";
    // Six more, read off the artwork the same way the letters were. Glyph 69 is
    // the blank the `space` field already points at; 70 is an apostrophe; the
    // two banks then diverge, with the small font's last glyph a diagonal stroke
    // and the bold font's a horizontal bar. A status panel prints health as
    // `have/most`, which is why the small font's slash is worth having.
    const SMALL_TAIL: &str = "#$% '/";
    const BOLD_TAIL: &str = "#$% '-";
    serde_json::json!({
        "bold": {
            "sheet": "bank.bold",
            // glyphs[i] is the character glyph i draws.
            "glyphs": format!("{LETTERS}{BOLD_TAIL}"),
            "space": 69,
            "space_width": 7,
            "tracking": 1,
            "line_height": 20
        },
        "small": {
            "sheet": "bank.small",
            "glyphs": format!("{LETTERS}{SMALL_TAIL}"),
            "space": 69,
            "space_width": 4,
            "tracking": 1,
            "line_height": 8
        }
    })
    .to_string()
}

/// The places on the map, and what each of them offers.
///
/// The original's overworld is a node graph inside `MAIN.EXE` that is not
/// recovered, so these coordinates were not lifted from it: they were read off
/// the map image by eye, and each one sits on the landmark the artist already
/// drew there. Highwood is the white castle in the northern snow, Waterdeep is
/// the walled port on the eastern shore, the healer keeps the ruin in the
/// southern forest, and the stones are the circle the game is named after.
///
/// The backdrops are the original's own town screens, which is why this lives
/// in the reference pack along with everything else derived from it.
///
/// `menu` is where the words go on that particular backdrop. Highwood and
/// Waterdeep painted their menu onto a panel at the edge of the picture, so the
/// box is put exactly over that panel and the live menu replaces the painted
/// one. The others have no panel, so the box goes where the art is quietest.
///
/// **A stall is a room, not a menu line.** The merchant is its own place, marked
/// `hidden` so walking can never find it, reached through the town's own menu
/// and leaving back into it. That keeps a town's front door short and lets the
/// shop have a box of its own, wide enough for goods and their prices, which a
/// 62-pixel painted panel is not.
///
/// **Two prices.** The hermit in the woods takes only days. The healers inside
/// the walls want coin as well, which is the difference between the two worth
/// having now that there is coin: the free one costs you a week of the calendar
/// the moon and the ambushes are hung on.

/// `_MAP:MapType` and `_MAP:MapSLOW`, read straight out of the fully unpacked
/// `MAIN.EXE` load image.
///
/// The image is not something this crate can make: `MAIN.EXE` is PKLITE
/// outside and EXEPACK inside, and both layers are peeled by running their own
/// stubs under emulation in `tools/symbolmap.py`. So this looks for the file
/// that tool writes and says plainly when it is not there, rather than pretending.
///
/// Everything about the result is checked before it is used. The image has to
/// be the right length; every terrain byte has to be one of the four codes
/// `MOON:ColourBackdrop` branches on; and all four codes have to appear, since
/// a table with only one value in it would be a table read from the wrong
/// place. A wrong image fails these rather than baking a plausible lie.
fn overworld_tables(src: &str) -> anyhow::Result<Option<serde_json::Value>> {
    let candidates = [
        std::env::args().nth(3).unwrap_or_default(),
        "research/main.final.bin".into(),
        format!("{src}/main.final.bin"),
    ];
    let Some(bytes) = candidates
        .iter()
        .filter(|p| !p.is_empty())
        .find_map(|p| fs::read(p).ok())
    else {
        return Ok(None);
    };
    anyhow::ensure!(
        bytes.len() == IMAGE_LEN,
        "the unpacked image is {} bytes, expected {IMAGE_LEN}",
        bytes.len()
    );
    let terrain = bytes[MAPTYPE_AT..MAPTYPE_AT + GRID_LEN].to_vec();
    let going = bytes[MAPSLOW_AT..MAPSLOW_AT + GRID_LEN].to_vec();
    anyhow::ensure!(
        terrain.iter().all(|c| matches!(c, 0 | 2 | 4 | 6)),
        "MapType holds a code the game never branches on"
    );
    for code in [0u8, 2, 4, 6] {
        anyhow::ensure!(
            terrain.contains(&code),
            "MapType has no cells of terrain {code}, so it is not MapType"
        );
    }
    // Only the 1,000 bytes MapSLOW actually owns are checked. The 40 after
    // them are the first row of MapType, which the original reads because it
    // indexes both grids the same way and MapSLOW is one row shorter. Those
    // bytes are terrain codes being used as delay masks, which is nonsense but
    // is the nonsense the game plays, and it only shows on the very bottom row
    // of the map.
    anyhow::ensure!(
        going[..40 * 25].iter().all(|c| *c < 4),
        "MapSLOW holds a mask wider than the two bits CheckSLOW uses"
    );
    Ok(Some(serde_json::json!({ "terrain": terrain, "going": going })))
}

/// Where each place sits, and how big it is.
///
/// **Two of the five coordinates are recovered and the rest are not, and the
/// difference is worth stating.** `_MAP:KnightGoesToTown` carries the two towns
/// as literals: a knight heading for Highwood walks to map (94, 47) and for
/// Waterdeep to (297, 157), and the same routine works out which is nearer from
/// the grid cells (12, 7) and (37, 20). Those cells are exactly what the
/// recovered grid formula turns those pixels into, which is what makes both
/// pairs trustworthy rather than merely present.
///
/// What those numbers are is the spot a knight is sent to, not the corner of
/// the picture. The corner lives in `MOON:MapIconsTABLE`, which is
/// uninitialised data and so is not in the load image at all: the first 2,906
/// bytes of DGROUP in the unpacked file are a stale duplicate of another region
/// and cannot be read. So the box is **built** here rather than recovered: the
/// icon's size comes from the `MI.C` bank, which is real, and it is hung so
/// that the recovered destination sits in the middle of it. Both towns land on
/// their own artwork when it is drawn, which is the check that it is not
/// nonsense, but it remains a construction.
///
/// The healer and the stones have no recovered coordinates at all. They are
/// placed on the landmarks the map already draws.
fn place_definitions(icons: &BTreeMap<u8, (i32, i32)>) -> String {
    // MI.C frame numbers, which are also the kinds the original's menu table is
    // indexed by: 0x19 Highwood, 0x1a Waterdeep, 0x1b Stonehenge.
    let icon = |frame: u8, fallback: (i32, i32)| *icons.get(&frame).unwrap_or(&fallback);
    // A place's box, hung so that `goal` is the middle of it. `goal` is where
    // the traveller's own 8x10 token stands, so its middle is offset by half of
    // that before the icon is centred on it.
    let at = |goal: (i32, i32), size: (i32, i32)| {
        (goal.0 + 4 - size.0 / 2, goal.1 + 5 - size.1 / 2, size.0, size.1)
    };
    let highwood = at((94, 47), icon(0x19, (25, 32)));
    let waterdeep = at((297, 157), icon(0x1a, (32, 28)));
    // Ours, not theirs: the ruin in the southern woods, and the stone ring the
    // map draws in the middle of it all. Sized like the original's own
    // Stonehenge icon where there is one to borrow.
    let healer = (89, 159, 10, 10);
    let stones = {
        let (w, h) = icon(0x1b, (18, 12));
        (158 - w / 2, 102 - h / 2, w, h)
    };

    let heal = |days: u32, gold: u32| {
        serde_json::json!({
            "do": "heal", "days": days, "gold": gold,
            "said": "Rest well. You are whole again.",
            "refused": "You are unmarked. Keep your days.",
            "too_poor": "I keep no man for nothing."
        })
    };
    let closed = |said: &str| serde_json::json!({ "do": "closed", "said": said });
    let leave = serde_json::json!({ "do": "leave" });
    let go = |place: &str| serde_json::json!({ "do": "go", "place": place });
    let buy = |item: &str| {
        serde_json::json!({
            "do": "buy", "item": item,
            "said": "A fair trade. Keep it dry.",
            "too_dear": "Come back when your purse is heavier.",
            "no_room": "You are carrying all you can."
        })
    };
    let drink = |item: &str| {
        serde_json::json!({
            "do": "use", "item": item,
            "said": "You drain it, and the ache goes out of you.",
            "refused": "You have none, or no need of one."
        })
    };
    // A stall sells the same goods wherever it stands; only the box moves,
    // because it has to sit where that particular painting has room.
    let stall = |name: &str, scene: &str, menu: serde_json::Value, back: &str| {
        serde_json::json!({
            "name": name,
            "scene": scene,
            "hidden": true,
            "x": 0, "y": 0, "w": 0, "h": 0,
            "menu": menu,
            "options": [
                { "label": "Flask of healing", "effect": buy("potion") },
                { "label": "Draught of life",  "effect": buy("elixir") },
                { "label": "Iron key",         "effect": buy("key") },
                { "label": "Drink a flask",    "effect": drink("potion") },
                { "label": "Drink a draught",  "effect": drink("elixir") },
                { "label": "Back",             "effect": go(back) }
            ]
        })
    };

    serde_json::json!({
        "highwood": {
            "name": "Highwood",
            "scene": "scene.highwood",
            "x": highwood.0, "y": highwood.1, "w": highwood.2, "h": highwood.3,
            "menu": [256, 0, 62, 200],
            "options": [
                { "label": "Merchant", "effect": go("highwood.merchant") },
                { "label": "Tavern",   "effect": closed("No one is pouring tonight.") },
                { "label": "Healer",   "effect": heal(3, 10) },
                { "label": "Temple",   "effect": closed("The doors are barred.") },
                { "label": "Leave",    "effect": leave }
            ]
        },
        // A stall's box swallows the town's own painted menu as well as the
        // art beside it. Leaving that painted list of doors showing next to a
        // live one would offer the player two menus and honour only the live
        // one, and it is wide enough here for goods and their prices, which the
        // painted panel alone is not.
        "highwood.merchant": stall(
            "Merchant", "scene.highwood", serde_json::json!([140, 0, 178, 200]), "highwood"),
        "waterdeep": {
            "name": "Waterdeep",
            "scene": "scene.waterdee",
            "x": waterdeep.0, "y": waterdeep.1, "w": waterdeep.2, "h": waterdeep.3,
            "menu": [2, 0, 62, 200],
            "options": [
                { "label": "Merchant", "effect": go("waterdeep.merchant") },
                { "label": "Tavern",   "effect": closed("No one is pouring tonight.") },
                { "label": "Healer",   "effect": heal(3, 10) },
                { "label": "Mystic",   "effect": closed("Mythral will not see you.") },
                { "label": "Leave",    "effect": leave }
            ]
        },
        // Waterdeep's painted panel is on the left, so its stall grows to the
        // right off it rather than to the left.
        "waterdeep.merchant": stall(
            "Merchant", "scene.waterdee", serde_json::json!([2, 0, 178, 200]), "waterdeep"),
        "healer": {
            "name": "The Healer",
            "scene": "scene.hea",
            "x": healer.0, "y": healer.1, "w": healer.2, "h": healer.3,
            "menu": [6, 112, 154, 66],
            "options": [
                { "label": "Tend my wounds", "effect": heal(3, 0) },
                { "label": "Drink a flask",  "effect": drink("potion") },
                { "label": "Leave",          "effect": leave }
            ]
        },
        "stones": {
            "name": "The Stones",
            "scene": "scene.hen1",
            "x": stones.0, "y": stones.1, "w": stones.2, "h": stones.3,
            "menu": [8, 18, 132, 74],
            "options": [
                { "label": "Listen", "effect": closed("The stones keep their counsel.") },
                { "label": "Wait",   "effect": closed("The moon is not yet full.") },
                { "label": "Leave",  "effect": leave }
            ]
        }
    })
    .to_string()
}

/// What there is to carry, and what a stall asks for it.
///
/// None of this is recovered. The original's symbols name `DRINKPOTIONHEAL`,
/// `TAKEFROMKNIGHT` and a `GOLD` readout on the status art, which is enough to
/// know that potions, an inventory and a purse all existed, and not enough to
/// know a single price or a single strength. Every number here was chosen
/// against the others: a flask is about two won fights, a draught is about
/// five, and a town healer is cheaper than either but costs you the week.
///
/// It lands in the reference pack for now because it is authored alongside the
/// places that sell it, and those carry the original's own town art. Nothing in
/// this file is derived from the original, so it moves to the shippable pack
/// the moment there is a town screen of our own to sell it in.
fn item_definitions() -> String {
    serde_json::json!({
        "potion": {
            "name": "Flask of healing",
            "price": 25,
            "consumed": true,
            "virtue": { "does": "heal", "health": 40 }
        },
        "elixir": {
            "name": "Draught of life",
            "price": 70,
            "consumed": true,
            "virtue": { "does": "heal", "health": 100 }
        },
        // Carried, worth coin, and honest about doing nothing yet: the lairs it
        // is for do not exist. An inert item is still a real item, and a thief
        // can still take it off you.
        "key": {
            "name": "Iron key",
            "price": 120,
            "consumed": false,
            "virtue": { "does": "inert" }
        },

        // Swords and armour, and these *are* recovered.
        //
        // The names are the strings `_STATUS` prints beside them: `ab9`..`ab17`.
        // The prices are the merchant's own lines, `pu1`..`pu5`, which carry the
        // number in the text. What each is worth in a fight comes from the code:
        // `CalcDamage` adds two, three and five for the three better blades and
        // nothing for the long sword, and the derivation routine at 0x28d adds
        // ten, twenty and thirty health for the three better suits and nothing
        // for padded. Only chain mail and battle armour add to the stride, which
        // looks like an oversight in the original and is kept because it is what
        // the original does.
        //
        // Nothing sells these yet: the merchant's list is flasks, and putting
        // swords on it is the economy's business rather than the shell's. They
        // are here because a knight starts wearing two of them and the status
        // panel has to be able to name what they are worth.
        "dagger": {
            "name": "Dagger", "price": 2, "consumed": false,
            "virtue": { "does": "weapon", "damage": 0 }
        },
        "long_sword": {
            "name": "Long sword", "price": 0, "consumed": false,
            "virtue": { "does": "weapon", "damage": 0 }
        },
        "broad_sword": {
            "name": "Broad sword", "price": 10, "consumed": false,
            "virtue": { "does": "weapon", "damage": 2 }
        },
        "claymore": {
            "name": "Claymore sword", "price": 25, "consumed": false,
            "virtue": { "does": "weapon", "damage": 3 }
        },
        "sword_of_sharpness": {
            "name": "Sword of Sharpness", "price": 100, "consumed": false,
            "virtue": { "does": "weapon", "damage": 5 }
        },
        "padded_armour": {
            "name": "Padded armour", "price": 0, "consumed": false,
            "virtue": { "does": "armour", "health": 0, "stride": 0 }
        },
        "chain_mail": {
            "name": "Chain mail", "price": 30, "consumed": false,
            "virtue": { "does": "armour", "health": 10, "stride": 2 }
        },
        "plate_armour": {
            "name": "Plate armour", "price": 50, "consumed": false,
            "virtue": { "does": "armour", "health": 20, "stride": 0 }
        },
        "battle_armour": {
            "name": "Battle armour", "price": 75, "consumed": false,
            "virtue": { "does": "armour", "health": 30, "stride": 2 }
        }
    })
    .to_string()
}

/// The four knights.
///
/// **Recovered, nearly all of it.** `InitKnights` walks the four player records
/// and gives each one a name buffer and a starting square on the map:
///
/// ```text
/// knight 0  BNAME   (10, 10)     knight 1  GNAME   (300, 5)
/// knight 2  ENAME   (26, 180)    knight 3  RNAME   (300, 185)
/// ```
///
/// One corner each. `KnightGlowColours` gives the colours those initials stand
/// for, as three 12-bit shades apiece: blue, gold, emerald, red, in knight
/// order, which is why `BNAME`, `GNAME`, `ENAME` and `RNAME` are not a guess.
/// The fifth entry in that routine, a dark purple, is the one every computer
/// knight wears.
///
/// The stat block is `SetKnightEquipment`: one of each ability, five life
/// points, ten daggers, ten gold, a long sword and padded armour. It writes
/// ninety-nine into the health field and the routine at 0x28d overwrites it a
/// moment later, so the twenty a knight really starts with is left to that
/// arithmetic here as well.
///
/// **Not recovered: which name belongs to which knight.** The four names are in
/// the data as `Enemy1Name`..`Enemy4Name`, `SIR BANNER`, `SIR DWAIN`,
/// `SIR BALAIN` and `SIR GUNTHER`, and the original hands them to its four
/// computer knights while a person types their own over the top. They are paired
/// with the four in the order they sit in memory, which is an assumption and
/// nothing more.
///
/// **And the four do not differ.** `InitKnights` separates them by name, colour
/// and corner; every stat block it produces is the same. The shape allows four
/// different ones, because that is what the data ought to allow, but shipping
/// four different ones would be an invention dressed as a recovery.
fn knight_definitions() -> String {
    // 12-bit RGB, as the original stores it, widened by the usual nibble * 17.
    let knight = |name: &str, shades: [u32; 3], home: [i32; 2]| {
        let widen = |v: u32| {
            (((v >> 8) & 0xf) * 17) << 16 | (((v >> 4) & 0xf) * 17) << 8 | (v & 0xf) * 17
        };
        serde_json::json!({
            "name": name,
            "shades": shades.iter().map(|c| widen(*c)).collect::<Vec<u32>>(),
            "home": home,
            "strength": 1,
            "constitution": 1,
            "endurance": 1,
            "life": 5,
            "daggers": 10,
            "gold": 10,
            "weapon": "long_sword",
            "armour": "padded_armour"
        })
    };
    serde_json::json!([
        knight("Sir Banner",  [0x00c, 0x009, 0x006], [10, 10]),
        knight("Sir Dwain",   [0xfa0, 0xe70, 0xc50], [300, 5]),
        knight("Sir Balain",  [0xae8, 0x6b5, 0x473], [26, 180]),
        knight("Sir Gunther", [0xd00, 0x900, 0x500], [300, 185]),
    ])
    .to_string()
}
