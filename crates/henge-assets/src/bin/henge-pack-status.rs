//! How far through the art replacement are we, and can we ship yet.
//!
//!   henge-pack-status [packs-dir]

use henge_assets::Registry;
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "packs".into());
    let dir = Path::new(&dir);

    let mut reg = Registry::new();
    let mut loaded = Vec::new();
    // Priority order. Our own work first, the baked reference last.
    for name in ["original", "reference"] {
        let p = dir.join(name);
        if p.join("manifest.json").exists() {
            reg.push_pack(&p)?;
            loaded.push(name);
        }
    }
    anyhow::ensure!(!loaded.is_empty(), "no packs found under {}", dir.display());
    println!("packs, in priority order: {}", loaded.join(" > "));

    let c = reg.coverage();
    let width = 40usize;
    let filled = (c.percent() / 100.0 * width as f32).round() as usize;
    println!(
        "\n  [{}{}] {:.1}%   {} of {} assets are our own\n",
        "#".repeat(filled),
        ".".repeat(width - filled),
        c.percent(),
        c.original,
        c.total
    );

    match reg.shippable() {
        Ok(()) => println!("SHIPPABLE. Nothing resolves to derived material."),
        Err(blocked) => {
            println!(
                "NOT SHIPPABLE. {} assets still come from the original game:\n",
                blocked.len()
            );
            for id in blocked.iter().take(25) {
                println!("  {id}");
            }
            if blocked.len() > 25 {
                println!("  ... and {} more", blocked.len() - 25);
            }
        }
    }
    Ok(())
}
