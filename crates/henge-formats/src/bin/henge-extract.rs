//! Research tool. Decodes the original game's data into PNGs and JSON so the
//! design can be studied: arena composition, sprite banks, palettes, bounds.
//!
//! Nothing this produces may ship in a commercial game. It is reference material.
//!
//!   henge-extract <game-data-dir> <output-dir>

use anyhow::Context;
use henge_formats::{piv, Library, Sprite};
use std::fs;
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let src = args.next().unwrap_or_else(|| ".".into());
    let dst = args.next().unwrap_or_else(|| "reference".into());

    let lib = Library::open(&src).context("opening game data")?;
    let out = Path::new(&dst);
    fs::create_dir_all(out.join("backgrounds"))?;
    fs::create_dir_all(out.join("sprites"))?;
    fs::create_dir_all(out.join("arenas"))?;

    let mut report = Vec::new();

    // Full-screen images. CMP sheets are PIVs too, so they come along.
    for name in lib.with_extension(&["piv", "cmp"]) {
        match lib.piv(&name) {
            Ok(p) => {
                let path = out.join("backgrounds").join(format!("{name}.png"));
                write_indexed(&path, piv::W, piv::H, &p.pixels, &p.palette, false)?;
                report.push(format!(
                    "{name}: {} colours, {} planes",
                    p.palette.len(),
                    p.planes
                ));
            }
            Err(e) => report.push(format!("{name}: SKIPPED ({e})")),
        }
    }
    // MINDSCAP has no extension but is a PIV.
    for name in ["MINDSCAP"] {
        if let Ok(p) = lib.piv(name) {
            write_indexed(
                &out.join("backgrounds").join(format!("{name}.png")),
                piv::W,
                piv::H,
                &p.pixels,
                &p.palette,
                false,
            )?;
        }
    }

    // Sprite banks, laid out as contact sheets against a neutral palette.
    let palette = lib
        .piv("WA1.CMP")
        .map(|p| p.palette)
        .unwrap_or_else(|_| grey_ramp());
    for name in lib.with_extension(&["cel", "ob", "f", "fon"]) {
        match lib.cel(&name) {
            Ok(c) => {
                let sheet = contact_sheet(&c.images, 8);
                write_indexed(
                    &out.join("sprites").join(format!("{name}.png")),
                    sheet.width,
                    sheet.height,
                    &sheet.pixels,
                    &palette,
                    true,
                )?;
                report.push(format!("{name}: {} frames", c.images.len()));
            }
            Err(e) => report.push(format!("{name}: SKIPPED ({e})")),
        }
    }

    // Arena layouts, as data. This is the part worth studying.
    let mut arenas = serde_json::Map::new();
    for name in lib.with_extension(&["t"]) {
        if let Ok(t) = lib.terrain(&name) {
            let borders: Vec<String> = t
                .borders
                .iter()
                .map(|b| format!("x {}..{} down to {}", b.left, b.right, b.bottom))
                .collect();
            report.push(format!(
                "{name}: {} pieces, {} impassable [{}]",
                t.placements.len(),
                t.borders.len(),
                borders.join("; ")
            ));
            arenas.insert(name.clone(), serde_json::to_value(&t)?);
        }
    }
    fs::write(
        out.join("arenas/arenas.json"),
        serde_json::to_string_pretty(&arenas)?,
    )?;

    report.sort();
    fs::write(out.join("catalogue.txt"), report.join("\n"))?;
    println!("wrote {} entries to {}", report.len(), out.display());
    Ok(())
}

fn grey_ramp() -> Vec<u32> {
    (0..32)
        .map(|i| {
            let v = (i * 8) as u32;
            v << 16 | v << 8 | v
        })
        .collect()
}

fn contact_sheet(images: &[Sprite], cols: usize) -> Sprite {
    let cw = images.iter().map(|i| i.width).max().unwrap_or(1).max(1);
    let ch = images.iter().map(|i| i.height).max().unwrap_or(1).max(1);
    let rows = images.len().div_ceil(cols);
    let (w, h) = (cw * cols, ch * rows);
    let mut px = vec![0u8; w * h];
    for (n, img) in images.iter().enumerate() {
        let (ox, oy) = ((n % cols) * cw, (n / cols) * ch);
        for y in 0..img.height {
            for x in 0..img.width {
                let v = img.pixels[y * img.width + x];
                if v != 0 {
                    px[(oy + y) * w + ox + x] = v;
                }
            }
        }
    }
    Sprite {
        width: w,
        height: h,
        real_width: w,
        pixels: px,
    }
}

fn write_indexed(
    path: &Path,
    w: usize,
    h: usize,
    pixels: &[u8],
    palette: &[u32],
    transparent0: bool,
) -> anyhow::Result<()> {
    let file = fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Indexed);
    enc.set_depth(png::BitDepth::Eight);

    let mut pal = Vec::with_capacity(palette.len() * 3);
    for c in palette {
        pal.extend_from_slice(&[(c >> 16) as u8, (c >> 8) as u8, *c as u8]);
    }
    enc.set_palette(pal);
    if transparent0 {
        let mut alpha = vec![255u8; palette.len()];
        alpha[0] = 0;
        enc.set_trns(alpha);
    }
    enc.write_header()?.write_image_data(pixels)?;
    Ok(())
}
