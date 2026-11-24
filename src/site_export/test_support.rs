//! A vault on disk for the tests of the site export, and ways to look at what an export makes.
#![cfg(test)]

use super::plan::{self, Content, Export};
use crate::media::outside::Library;
use crate::vault::Vault;
use std::path::{Path, PathBuf};

/// A vault in a temporary folder (and a sibling `out/` for the site); gone when it is dropped.
pub struct Fixture {
    pub root: PathBuf,
    pub dir: PathBuf,
}

impl Fixture {
    pub fn new(tag: &str) -> Fixture {
        let root = std::env::temp_dir().join(format!("shale-site-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("vault");
        std::fs::create_dir_all(&dir).unwrap();
        Fixture { root, dir }
    }

    /// A vault with these files (path, text), publishing on with `rules` (JSON members of `publish`).
    pub fn with(tag: &str, rules: &str, files: &[(&str, &str)]) -> Fixture {
        let f = Fixture::new(tag);
        f.publish(rules);
        for (path, text) in files {
            f.file(path, text);
        }
        f
    }

    pub fn file(&self, rel: &str, text: &str) {
        self.bytes(rel, text.as_bytes());
    }

    pub fn bytes(&self, rel: &str, bytes: &[u8]) {
        let path = self.dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    /// `.shale/settings.json` with publishing on and these members of `publish`, e.g. `"all_notes": true`.
    pub fn publish(&self, rules: &str) {
        let sep = if rules.is_empty() { "" } else { ", " };
        self.file(".shale/settings.json", &format!("{{\"version\": 1, \"publish\": {{\"enabled\": true{sep}{rules}}}}}"));
    }

    pub fn out(&self) -> PathBuf {
        self.root.join("out")
    }

    pub fn export(&self) -> Export {
        self.export_with(&[])
    }

    pub fn export_with(&self, libraries: &[Library]) -> Export {
        let mut vault = Vault::open(&self.dir).unwrap();
        vault.set_libraries(libraries);
        plan::build(&vault).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// The text of the file `path` of the tree an export would write.
pub fn text(e: &Export, path: &str) -> String {
    match e.files.iter().find(|f| f.path == path).map(|f| &f.content) {
        Some(Content::Text(t)) => t.clone(),
        Some(Content::Copy(_)) => panic!("{path} is a copied file"),
        None => panic!("{path} is not written; the tree has {:?}", paths(e)),
    }
}

pub fn paths(e: &Export) -> Vec<&str> {
    e.files.iter().map(|f| f.path.as_str()).collect()
}

/// Whether the tree has a file at `path`.
pub fn has(e: &Export, path: &str) -> bool {
    e.files.iter().any(|f| f.path == path)
}

/// Every path and every text of the tree, and the bytes of every copied file, in one string: what
/// the privacy rows grep.
pub fn everything(e: &Export) -> String {
    let mut all = String::new();
    for f in &e.files {
        all.push_str(&f.path);
        all.push('\n');
        match &f.content {
            Content::Text(t) => all.push_str(t),
            Content::Copy(p) => all.push_str(&String::from_utf8_lossy(&std::fs::read(p).unwrap())),
        }
        all.push('\n');
    }
    all
}

/// The page of a note (the file below `content/`): its text with the front matter.
pub fn page(e: &Export, content: &str) -> String {
    text(e, &format!("content/{content}"))
}

/// The files below `dir` with their content and modification time: a snapshot to compare.
pub fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>, Option<std::time::SystemTime>)> {
    fn walk(dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>, Option<std::time::SystemTime>)>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().collect();
        entries.sort_by_key(|e| e.path());
        for e in entries {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else {
                out.push((p.clone(), std::fs::read(&p).unwrap(), e.metadata().unwrap().modified().ok()));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, &mut out);
    out
}
