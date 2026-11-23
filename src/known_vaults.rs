//! The vaults Shale knows (VAULT-029, VAULT-031, CFG-075, rabalyn/shale#582): entries of a name and
//! a canonical path, kept in the global `config.toml`. They replace the old recent list.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

/// One known vault. The name is local to this device and never written to `.shale/` (VAULT-029).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct KnownVault {
    #[serde(default)]
    pub name: String,
    pub path: PathBuf,
}

/// The known vaults, the one opened last first. A folder is listed once and the list has no length cap.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(from = "Vec<KnownVault>", into = "Vec<KnownVault>")]
pub struct KnownVaults(Vec<KnownVault>);

/// What a hand-edited file may hold: an empty name becomes the folder's name (VAULT-031), an entry
/// without a path is dropped and a path that is listed twice keeps its first entry (VAULT-029).
impl From<Vec<KnownVault>> for KnownVaults {
    fn from(list: Vec<KnownVault>) -> Self {
        let mut out: Vec<KnownVault> = Vec::new();
        for mut e in list {
            if e.path.as_os_str().is_empty() || out.iter().any(|o| o.path == e.path) {
                continue;
            }
            if e.name.trim().is_empty() {
                e.name = folder_name(&e.path);
            }
            out.push(e);
        }
        KnownVaults(out)
    }
}

impl From<KnownVaults> for Vec<KnownVault> {
    fn from(k: KnownVaults) -> Self {
        k.0
    }
}

/// How a folder lies to another known vault (VAULT-039); the label is the other vault's.
#[derive(Clone, Debug, PartialEq)]
pub enum Nesting {
    /// the folder is inside the other vault
    Inside(String),
    /// the other vault is inside the folder
    Contains(String),
}

impl Nesting {
    /// The warning Shale shows once, when the folder is added.
    pub fn warning(&self) -> String {
        match self {
            Nesting::Inside(other) => format!("This folder lies inside the vault “{other}”, so its notes show up in both. Shale opens it anyway."),
            Nesting::Contains(other) => format!("This folder contains the vault “{other}”, so that vault's notes show up in both. Shale opens it anyway."),
        }
    }
}

impl KnownVaults {
    /// The entries, the vault opened last first.
    pub fn entries(&self) -> &[KnownVault] {
        &self.0
    }

    /// List the vault in `path` (canonical, VAULT-029) and move it to the front; an entry that is
    /// there keeps its name, a new one is named by its folder (VAULT-031).
    pub fn add(&mut self, path: &Path) {
        let path = canonical(path);
        let entry = match self.0.iter().position(|e| e.path == path) {
            Some(i) => self.0.remove(i),
            None => KnownVault { name: folder_name(&path), path },
        };
        self.0.insert(0, entry);
    }

    /// The name of an entry as the window shows it: the name, and the parent folder after it when
    /// another known vault has the same name (VAULT-031).
    pub fn label(&self, entry: &KnownVault) -> String {
        if self.0.iter().filter(|e| e.name == entry.name).count() > 1 {
            let parent = entry.path.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
            format!("{} ({parent})", entry.name)
        } else {
            entry.name.clone()
        }
    }

    /// Whether the vault in `path` is listed.
    pub fn contains(&self, path: &Path) -> bool {
        let path = canonical(path);
        self.0.iter().any(|e| e.path == path)
    }

    /// Give the entry of `path` another name (VAULT-032). An empty name is refused with its reason.
    pub fn rename(&mut self, path: &Path, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("The name cannot be empty.".into());
        }
        let path = canonical(path);
        match self.0.iter_mut().find(|e| e.path == path) {
            Some(e) => {
                e.name = name.to_string();
                Ok(())
            }
            None => Err("That vault is not in the list any more.".into()),
        }
    }

    /// Remove the entry of `path` from the list; the folder is not touched (VAULT-038).
    pub fn forget(&mut self, path: &Path) {
        let path = canonical(path);
        self.0.retain(|e| e.path != path);
    }

    /// Point the entry at `old` to the folder `new`, keeping its name and its place (VAULT-038).
    /// Refused when another entry has that folder already.
    pub fn relocate(&mut self, old: &Path, new: &Path) -> Result<(), String> {
        let (old, new) = (canonical(old), canonical(new));
        if let Some(other) = self.0.iter().find(|e| e.path == new && e.path != old) {
            return Err(format!("That folder is already known as “{}”.", other.name));
        }
        match self.0.iter_mut().find(|e| e.path == old) {
            Some(e) => {
                e.path = new;
                Ok(())
            }
            None => Err("That vault is not in the list any more.".into()),
        }
    }

    /// The known vault that `path` lies inside of, or that lies inside `path` (VAULT-039); the entry
    /// of `path` itself, and the one at `skip`, do not count.
    pub fn nested_with(&self, path: &Path, skip: Option<&Path>) -> Option<Nesting> {
        let (path, skip) = (canonical(path), skip.map(canonical));
        self.0.iter().filter(|e| e.path != path && Some(&e.path) != skip.as_ref()).find_map(|e| {
            if path.starts_with(&e.path) {
                Some(Nesting::Inside(self.label(e)))
            } else if e.path.starts_with(&path) {
                Some(Nesting::Contains(self.label(e)))
            } else {
                None
            }
        })
    }

    /// The label of the vault in `path`: its entry's, or its folder's name when it is not listed.
    pub fn label_of(&self, path: &Path) -> String {
        let path = canonical(path);
        match self.0.iter().find(|e| e.path == path) {
            Some(e) => self.label(e),
            None => folder_name(&path),
        }
    }
}

/// The path as it is stored: symlinks resolved and no trailing slash (VAULT-029). A folder that is
/// not there (yet) keeps its path, without a trailing slash.
pub fn canonical(path: &Path) -> PathBuf {
    match std::fs::canonicalize(path) {
        Ok(p) => crate::fsutil::plain_path(p),
        Err(_) => path.components().collect(),
    }
}

/// The name of a folder, the default name of a vault (VAULT-031).
pub fn folder_name(p: &Path) -> String {
    p.file_name().map_or_else(|| p.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned())
}

/// Migration 2 → 3 of the config (CFG-075): the paths of `recent` become entries named by their
/// folder, in the same order, and `recent` is gone. A file that already has `vaults` keeps them.
pub fn migrate_recent(json: &mut Map<String, Value>) {
    let Some(Value::Array(recent)) = json.remove("recent") else { return };
    if json.contains_key("vaults") {
        return;
    }
    let mut known = KnownVaults::default();
    // `add` puts a path at the front, so the oldest goes first
    for p in recent.iter().rev().filter_map(Value::as_str) {
        known.add(Path::new(p));
    }
    json.insert("vaults".into(), serde_json::to_value(known).unwrap_or_default());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, path: &str) -> KnownVault {
        KnownVault { name: name.into(), path: path.into() }
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("shale-known-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    // spec: VAULT-029
    #[test]
    fn vault_029_a_folder_is_listed_once_whatever_way_it_is_spelled() {
        let d = tmp("canonical");
        let real = d.join("real");
        std::fs::create_dir_all(&real).unwrap();
        let mut k = KnownVaults::default();
        k.add(&real);
        k.add(&PathBuf::from(format!("{}/", real.display())));
        k.add(&d.join("real").join("..").join("real"));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&real, d.join("link")).unwrap();
            k.add(&d.join("link"));
        }
        assert_eq!(k.entries().len(), 1, "{k:?}");
        assert_eq!(k.entries()[0].path, canonical(&real));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: VAULT-029
    #[test]
    fn vault_029_the_list_has_no_length_cap_and_the_last_opened_is_first() {
        let mut k = KnownVaults::default();
        for i in 0..30 {
            k.add(Path::new(&format!("/nowhere/v{i}")));
        }
        k.add(Path::new("/nowhere/v3"));
        assert_eq!(k.entries().len(), 30);
        assert_eq!(k.entries()[0].path, PathBuf::from("/nowhere/v3"));
        assert_eq!(k.entries()[1].path, PathBuf::from("/nowhere/v29"));
    }

    // spec: VAULT-031
    #[test]
    fn vault_031_the_default_name_is_the_folder_and_a_name_is_kept_when_added_again() {
        let mut k = KnownVaults::default();
        k.add(Path::new("/nowhere/Work notes"));
        assert_eq!(k.entries()[0].name, "Work notes");
        k.0[0].name = "Mine".into();
        k.add(Path::new("/nowhere/Work notes/"));
        assert_eq!(k.entries(), [entry("Mine", "/nowhere/Work notes")]);
    }

    // spec: VAULT-031
    #[test]
    fn vault_031_vaults_with_the_same_name_show_their_parent_folder() {
        let mut k = KnownVaults::default();
        k.add(Path::new("/a/notes"));
        k.add(Path::new("/b/notes"));
        k.add(Path::new("/a/other"));
        assert_eq!(k.label_of(Path::new("/a/notes")), "notes (/a)");
        assert_eq!(k.label_of(Path::new("/b/notes")), "notes (/b)");
        assert_eq!(k.label_of(Path::new("/a/other")), "other");
        assert_eq!(k.label_of(Path::new("/c/unlisted")), "unlisted");
    }

    // spec: VAULT-031
    #[test]
    fn vault_031_an_empty_name_in_the_file_is_refused_and_the_folder_name_is_used() {
        let k: KnownVaults = serde_json::from_str(r#"[{"name": "", "path": "/x/one"}, {"name": "  ", "path": "/x/two"}, {"path": "/x/three"}, {"name": "dup", "path": "/x/one"}, {"name": "n", "path": ""}]"#).unwrap();
        assert_eq!(k.entries(), [entry("one", "/x/one"), entry("two", "/x/two"), entry("three", "/x/three")]);
    }

    // spec: VAULT-032
    #[test]
    fn vault_032_rename_refuses_an_empty_name_and_keeps_the_entry_in_place() {
        let mut k = KnownVaults::default();
        k.add(Path::new("/nowhere/a"));
        k.add(Path::new("/nowhere/b"));
        assert_eq!(k.rename(Path::new("/nowhere/a"), "  "), Err("The name cannot be empty.".to_string()));
        assert_eq!(k.rename(Path::new("/nowhere/a"), " Mine "), Ok(()));
        assert_eq!(k.entries(), [entry("b", "/nowhere/b"), entry("Mine", "/nowhere/a")]);
        assert!(k.rename(Path::new("/nowhere/zzz"), "x").is_err());
    }

    // spec: VAULT-038
    #[test]
    fn vault_038_forget_and_relocate_change_the_list_only() {
        let mut k = KnownVaults::default();
        k.add(Path::new("/nowhere/a"));
        k.add(Path::new("/nowhere/b"));
        k.0[1].name = "Mine".into();
        assert_eq!(k.relocate(Path::new("/nowhere/a"), Path::new("/elsewhere/a2")), Ok(()));
        assert_eq!(k.entries(), [entry("b", "/nowhere/b"), entry("Mine", "/elsewhere/a2")], "the name and the place stay");
        assert!(k.relocate(Path::new("/elsewhere/a2"), Path::new("/nowhere/b")).unwrap_err().contains("already known as “b”"));
        assert!(k.relocate(Path::new("/nowhere/gone"), Path::new("/x")).is_err());
        assert!(k.contains(Path::new("/nowhere/b/")));
        k.forget(Path::new("/nowhere/b"));
        assert!(!k.contains(Path::new("/nowhere/b")));
        assert_eq!(k.entries().len(), 1);
    }

    // spec: VAULT-039
    #[test]
    fn vault_039_a_folder_in_or_around_a_known_vault_is_found_and_the_same_folder_is_not() {
        let mut k = KnownVaults::default();
        k.add(Path::new("/v/outer"));
        k.add(Path::new("/w/lone"));
        assert_eq!(k.nested_with(Path::new("/v/outer/inner"), None), Some(Nesting::Inside("outer".into())));
        assert_eq!(k.nested_with(Path::new("/v"), None), Some(Nesting::Contains("outer".into())));
        assert_eq!(k.nested_with(Path::new("/v/outer"), None), None, "a folder is not nested in itself");
        assert_eq!(k.nested_with(Path::new("/v/outer-two"), None), None, "a name that only starts alike is not inside");
        assert_eq!(k.nested_with(Path::new("/v/outer/inner"), Some(Path::new("/v/outer"))), None, "the entry being moved does not count");
        assert!(Nesting::Inside("outer".into()).warning().contains("inside the vault “outer”"));
        assert!(Nesting::Contains("outer".into()).warning().contains("contains the vault “outer”"));
    }

    // spec: CFG-075
    #[test]
    fn cfg_075_recent_becomes_entries_named_by_their_folder_and_is_gone() {
        let d = tmp("migrate");
        let (a, b) = (d.join("a"), d.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let mut json = serde_json::json!({ "recent": [a.to_string_lossy(), b.to_string_lossy(), format!("{}/", a.display()), "/gone/c"] });
        migrate_recent(json.as_object_mut().unwrap());
        assert!(json.get("recent").is_none());
        let k: KnownVaults = serde_json::from_value(json["vaults"].clone()).unwrap();
        assert_eq!(k.entries(), [entry("a", &canonical(&a).to_string_lossy()), entry("b", &canonical(&b).to_string_lossy()), entry("c", "/gone/c")]);
        let _ = std::fs::remove_dir_all(&d);
    }
}
