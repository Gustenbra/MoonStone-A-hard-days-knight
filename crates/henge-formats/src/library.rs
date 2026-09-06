//! Locates the original game's files.
//!
//! Moonstone shipped the same file on more than one floppy, so a bare uppercase
//! name is indexed once, with the root copy preferred over the per-disk copies.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct Library {
    index: HashMap<String, PathBuf>,
    pub root: PathBuf,
}

const PRIORITY: [&str; 5] = ["", "DISKB", "DISKC", "DISKA", "SAMPLES"];

impl Library {
    pub fn open(root: impl AsRef<Path>) -> anyhow::Result<Library> {
        let root = root.as_ref().to_path_buf();
        anyhow::ensure!(root.is_dir(), "{} is not a directory", root.display());

        let mut files: Vec<PathBuf> = WalkDir::new(&root)
            .max_depth(3)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
            .map(|e| e.into_path())
            .collect();

        let rank = |p: &PathBuf| -> usize {
            let parent = p
                .strip_prefix(&root)
                .ok()
                .and_then(|r| r.parent())
                .map(|d| d.to_string_lossy().to_uppercase())
                .unwrap_or_default();
            PRIORITY.iter().position(|k| *k == parent).unwrap_or(99)
        };
        files.sort_by_key(rank);

        let mut index = HashMap::new();
        for p in files {
            let key = p.file_name().unwrap_or_default().to_string_lossy().to_uppercase();
            index.entry(key).or_insert(p);
        }
        anyhow::ensure!(
            index.contains_key("KN1.OB"),
            "{} does not look like Moonstone game data (no KN1.OB)",
            root.display()
        );
        Ok(Library { index, root })
    }

    pub fn has(&self, name: &str) -> bool {
        self.index.contains_key(&name.to_uppercase())
    }

    pub fn bytes(&self, name: &str) -> anyhow::Result<Vec<u8>> {
        let p = self
            .index
            .get(&name.to_uppercase())
            .ok_or_else(|| anyhow::anyhow!("no such game file: {name}"))?;
        Ok(std::fs::read(p)?)
    }

    pub fn piv(&self, name: &str) -> anyhow::Result<crate::Piv> {
        crate::Piv::parse(&self.bytes(name)?)
    }

    pub fn cel(&self, name: &str) -> anyhow::Result<crate::Cel> {
        crate::Cel::parse(&self.bytes(name)?)
    }

    pub fn terrain(&self, name: &str) -> anyhow::Result<crate::Terrain> {
        crate::Terrain::parse(&self.bytes(name)?)
    }

    /// Every indexed name, sorted.
    pub fn names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.index.keys().cloned().collect();
        v.sort();
        v
    }

    pub fn with_extension(&self, exts: &[&str]) -> Vec<String> {
        let mut v: Vec<String> = self
            .index
            .keys()
            .filter(|n| exts.iter().any(|e| n.ends_with(&format!(".{}", e.to_uppercase()))))
            .cloned()
            .collect();
        v.sort();
        v
    }
}
