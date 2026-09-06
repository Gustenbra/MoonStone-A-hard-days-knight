use crate::manifest::{Manifest, Provenance, Sheet};
use crate::Image;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub struct Pack {
    pub root: PathBuf,
    pub manifest: Manifest,
}

/// What answered a lookup, and from where.
pub struct Resolved<'a, T> {
    pub value: &'a T,
    pub pack: &'a str,
    pub root: &'a Path,
    pub provenance: Provenance,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Coverage {
    pub total: usize,
    pub original: usize,
    pub derived: usize,
}

impl Coverage {
    pub fn percent(&self) -> f32 {
        if self.total == 0 {
            100.0
        } else {
            self.original as f32 * 100.0 / self.total as f32
        }
    }
}

/// Packs in priority order: first entry wins.
pub struct Registry {
    packs: Vec<Pack>,
    cache: BTreeMap<String, Image>,
}

impl Registry {
    pub fn new() -> Registry {
        Registry { packs: Vec::new(), cache: BTreeMap::new() }
    }

    /// Adds a pack at lowest priority. Call with the fallback pack last.
    pub fn push_pack(&mut self, root: impl AsRef<Path>) -> anyhow::Result<()> {
        let root = root.as_ref().to_path_buf();
        let text = std::fs::read_to_string(root.join("manifest.json"))
            .map_err(|e| anyhow::anyhow!("no manifest in {}: {e}", root.display()))?;
        let manifest: Manifest = serde_json::from_str(&text)?;
        self.packs.push(Pack { root, manifest });
        Ok(())
    }

    fn find<'a, T>(
        &'a self,
        id: &str,
        pick: impl Fn(&'a Manifest) -> Option<&'a T>,
    ) -> Option<Resolved<'a, T>> {
        let _ = id;
        self.packs.iter().find_map(|p| {
            pick(&p.manifest).map(|value| Resolved {
                value,
                pack: &p.manifest.pack,
                root: &p.root,
                provenance: p.manifest.provenance,
            })
        })
    }

    pub fn sheet(&self, id: &str) -> Option<Resolved<'_, Sheet>> {
        self.find(id, |m| m.sheets.get(id))
    }

    pub fn sound(&self, id: &str) -> Option<Resolved<'_, String>> {
        self.find(id, |m| m.sounds.get(id))
    }

    pub fn music(&self, id: &str) -> Option<Resolved<'_, String>> {
        self.find(id, |m| m.music.get(id))
    }

    pub fn palette(&self, id: &str) -> Option<Resolved<'_, Vec<u32>>> {
        self.find(id, |m| m.palettes.get(id))
    }

    /// A JSON blob: arena layouts, hit lines, animation scripts.
    pub fn data(&self, id: &str) -> Option<Resolved<'_, String>> {
        self.find(id, |m| m.data.get(id))
    }

    pub fn read_data<T: serde::de::DeserializeOwned>(&self, id: &str) -> anyhow::Result<T> {
        let r = self.data(id).ok_or_else(|| anyhow::anyhow!("no data for id {id}"))?;
        let path = r.root.join(r.value);
        let text = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
        Ok(serde_json::from_str(&text)?)
    }

    /// Loads and caches a sheet's image.
    pub fn image(&mut self, id: &str) -> anyhow::Result<&Image> {
        if !self.cache.contains_key(id) {
            let (root, file) = {
                let r = self
                    .sheet(id)
                    .ok_or_else(|| anyhow::anyhow!("no sheet for id {id}"))?;
                (r.root.to_path_buf(), r.value.file.clone())
            };
            let img = load_indexed_png(&root.join(&file))?;
            self.cache.insert(id.to_string(), img);
        }
        Ok(&self.cache[id])
    }

    /// Every id any pack declares.
    pub fn all_ids(&self) -> BTreeSet<&str> {
        self.packs
            .iter()
            .flat_map(|p| p.manifest.ids().map(String::as_str))
            .collect()
    }

    pub fn coverage(&self) -> Coverage {
        let mut c = Coverage::default();
        for id in self.all_ids() {
            c.total += 1;
            if self.provenance_of(id) == Some(Provenance::OriginalWork) {
                c.original += 1;
            } else {
                c.derived += 1;
            }
        }
        c
    }

    fn provenance_of(&self, id: &str) -> Option<Provenance> {
        self.packs
            .iter()
            .find(|p| p.manifest.ids().any(|k| k == id))
            .map(|p| p.manifest.provenance)
    }

    /// Ids still answered by material derived from the original game. Empty means
    /// the build is safe to distribute.
    pub fn shippable(&self) -> Result<(), Vec<String>> {
        let blocked: Vec<String> = self
            .all_ids()
            .into_iter()
            .filter(|id| self.provenance_of(id) == Some(Provenance::DerivedFromOriginal))
            .map(str::to_string)
            .collect();
        if blocked.is_empty() {
            Ok(())
        } else {
            Err(blocked)
        }
    }
}

impl Default for Registry {
    fn default() -> Self {
        Registry::new()
    }
}

fn load_indexed_png(path: &Path) -> anyhow::Result<Image> {
    let file = std::fs::File::open(path)
        .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    let decoder = png::Decoder::new(std::io::BufReader::new(file));
    let mut reader = decoder.read_info()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf)?;
    anyhow::ensure!(
        info.color_type == png::ColorType::Indexed,
        "{} must be an indexed PNG (this engine is palette-based)",
        path.display()
    );
    buf.truncate(info.buffer_size());
    Ok(Image { width: info.width as usize, height: info.height as usize, pixels: buf })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{FrameRect, Sheet};

    fn write_pack(dir: &Path, name: &str, prov: Provenance, ids: &[&str]) {
        std::fs::create_dir_all(dir).unwrap();
        let mut m = Manifest::new(name, prov);
        for id in ids {
            m.sheets.insert(
                id.to_string(),
                Sheet {
                    file: format!("{id}.png"),
                    frames: vec![FrameRect { x: 0, y: 0, w: 1, h: 1, ox: 0, oy: 0 }],
                },
            );
        }
        std::fs::write(dir.join("manifest.json"), serde_json::to_string(&m).unwrap()).unwrap();
    }

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("henge-assets-test-{name}"));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    #[test]
    fn the_first_pack_wins() {
        let base = tmp("priority");
        write_pack(&base.join("original"), "original", Provenance::OriginalWork, &["actor.knight.walk"]);
        write_pack(
            &base.join("reference"), "reference", Provenance::DerivedFromOriginal,
            &["actor.knight.walk", "actor.troll.walk"],
        );

        let mut r = Registry::new();
        r.push_pack(base.join("original")).unwrap();
        r.push_pack(base.join("reference")).unwrap();

        assert_eq!(r.sheet("actor.knight.walk").unwrap().pack, "original");
        assert_eq!(r.sheet("actor.troll.walk").unwrap().pack, "reference");
    }

    #[test]
    fn coverage_counts_what_has_been_replaced() {
        let base = tmp("coverage");
        write_pack(&base.join("original"), "original", Provenance::OriginalWork, &["a"]);
        write_pack(&base.join("reference"), "reference", Provenance::DerivedFromOriginal, &["a", "b", "c"]);

        let mut r = Registry::new();
        r.push_pack(base.join("original")).unwrap();
        r.push_pack(base.join("reference")).unwrap();

        let c = r.coverage();
        assert_eq!(c, Coverage { total: 3, original: 1, derived: 2 });
        assert!((c.percent() - 33.333).abs() < 0.01);
    }

    #[test]
    fn a_build_is_blocked_while_derived_assets_remain() {
        let base = tmp("ship");
        write_pack(&base.join("original"), "original", Provenance::OriginalWork, &["a"]);
        write_pack(&base.join("reference"), "reference", Provenance::DerivedFromOriginal, &["a", "b"]);

        let mut r = Registry::new();
        r.push_pack(base.join("original")).unwrap();
        r.push_pack(base.join("reference")).unwrap();
        assert_eq!(r.shippable(), Err(vec!["b".to_string()]));

        // Replace the last one and the block clears.
        write_pack(&base.join("original"), "original", Provenance::OriginalWork, &["a", "b"]);
        let mut r = Registry::new();
        r.push_pack(base.join("original")).unwrap();
        r.push_pack(base.join("reference")).unwrap();
        assert_eq!(r.shippable(), Ok(()));
    }
}
