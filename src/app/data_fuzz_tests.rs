//! Randomized data-safety test (ADR-0003: "we never lose or corrupt data").
//!
//! Random sequences of operations run against the real app on a real folder: create, edit and
//! autosave, a second program changing, deleting and creating files, files Shale can't read, a
//! rename, a move, trash, link rewrites, and any of them while a folder is read-only (a failing
//! write). The seeds are fixed, so a failure repeats. After every step these must hold:
//!
//! 1. **Nothing is lost.** Every version of a file (each has a unique token `<n>`) that existed
//!    before the step is still there after it: as a file (also in the trash or a conflict copy),
//!    or in a buffer that is still unsaved. The only versions allowed to go are those the step
//!    replaced on purpose: the other program's own overwrites and deletes, the user typing over
//!    an unsaved text, and a save over exactly the version the buffer had loaded.
//! 2. **No torn files.** Every file is empty or complete (starts with its token, ends with
//!    `END`); bytes that are not UTF-8 are only ever the ones the test wrote.
//! 3. **Nothing outside the vault** is created, changed or removed (also not the folder beside
//!    it, where a temporary file of a bad path would land).
//! 4. **No temporary files** (`.shale-tmp-*`) are left anywhere.
//! 5. No unsaved buffer is for a note that could not be read.

use super::*;
use crate::vault::Stamp;
use std::collections::BTreeMap;

/// xorshift64*: small, fixed, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            None
        } else {
            Some(&items[self.below(items.len())])
        }
    }
}

const STEMS: [&str; 6] = ["Alpha", "Beta", "Gamma", "Delta", "Ärger", "Plan"];
const FOLDERS: [&str; 4] = ["", "a", "a/b", "c"];

/// The first `<n>` in `bytes`.
fn token(bytes: &[u8]) -> Option<u64> {
    let start = bytes.iter().position(|b| *b == b'<')? + 1;
    let len = bytes[start..].iter().position(|b| *b == b'>')?;
    std::str::from_utf8(&bytes[start..start + len]).ok()?.parse().ok()
}

/// The bytes of a file as the test wrote them: Shale's `created:` and `modified:` (VAULT-045) taken off.
fn plain(bytes: &[u8]) -> Vec<u8> {
    match std::str::from_utf8(bytes) {
        Ok(t) => crate::note_props::unstamped(t).into_bytes(),
        Err(_) => bytes.to_vec(),
    }
}

fn well_formed(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return true;
    }
    let body = bytes.strip_prefix(b"\xff\xfe").unwrap_or(bytes);
    let t = token(body);
    t.is_some() && body.starts_with(b"<") && body.ends_with(format!("<{}> END\n", t.unwrap()).as_bytes()) && (body.len() == bytes.len() || std::str::from_utf8(bytes).is_err())
}

#[derive(Clone, Debug, PartialEq)]
enum Holder {
    File(PathBuf, Stamp),
    Mem,
}

/// Every file under `root` (the trash and `.shale` too): path and content.
fn all_files(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn walk(root: &Path, rel: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        let mut entries: Vec<_> = std::fs::read_dir(root.join(rel)).unwrap().map(|e| e.unwrap()).collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let r = rel.join(e.file_name());
            if e.file_type().unwrap().is_dir() {
                walk(root, &r, out);
            } else {
                match crate::fsutil::read(root.join(&r)) {
                    Ok(b) => out.push((r.clone(), b)),
                    Err(e) => {
                        panic!("{e}; type {:?} meta {:?} entries {:?}", e.kind(), std::fs::symlink_metadata(root.join(&r)), std::fs::read_dir(root.join(&r).parent().unwrap()).map(|d| d.map(|e| e.unwrap().file_name()).collect::<Vec<_>>()))
                    }
                }
            }
        }
    }
    let mut out = vec![];
    walk(root, Path::new(""), &mut out);
    out
}

fn holders(app: &ShaleApp, root: &Path) -> BTreeMap<u64, Vec<Holder>> {
    let mut m: BTreeMap<u64, Vec<Holder>> = BTreeMap::new();
    for (p, bytes) in all_files(root) {
        if let Some(t) = token(&plain(&bytes)) {
            m.entry(t).or_default().push(Holder::File(p, Stamp::of_bytes(&bytes)));
        }
    }
    for d in app.notes.values().filter(|d| d.dirty) {
        if let Some(t) = token(d.text.as_bytes()) {
            m.entry(t).or_default().push(Holder::Mem);
        }
    }
    m
}

struct Sim {
    app: ShaleApp,
    sandbox: PathBuf,
    root: PathBuf,
    rng: Rng,
    next: u64,
    log: Vec<String>,
    locked: Vec<PathBuf>,
}

impl Sim {
    fn new(seed: u64) -> Sim {
        let sandbox = std::env::temp_dir().join(format!("shale-fuzz-{seed}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&sandbox);
        let root = sandbox.join("vault");
        for f in FOLDERS {
            std::fs::create_dir_all(root.join(f)).unwrap();
        }
        std::fs::create_dir_all(sandbox.join("outside")).unwrap();
        crate::fsutil::write_atomic(&sandbox.join("outside/sentinel"), b"do not touch\n").unwrap();
        let mut sim =
            Sim { app: ShaleApp::build(Options { screenshot: None, vault: Some(root.clone()), view: None, keys: None }, false), sandbox, root, rng: Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1), next: 1, log: vec![], locked: vec![] };
        sim.app.cfg.date_prefix_new_notes = false;
        for stem in STEMS.iter().take(4) {
            let folder = FOLDERS[sim.rng.below(FOLDERS.len())];
            let text = sim.content();
            let p = sim.root.join(folder).join(format!("{stem}.md"));
            crate::fsutil::write_atomic(&p, text.as_bytes()).unwrap();
        }
        sim.app.vault.as_mut().unwrap().rescan();
        sim
    }

    fn content(&mut self) -> String {
        let n = self.next;
        self.next += 1;
        let link = STEMS[self.rng.below(STEMS.len())];
        format!("<{n}> note [[{link}]]\n<{n}> END\n")
    }

    fn notes_on_disk(&self) -> Vec<PathBuf> {
        all_files(&self.root).into_iter().map(|f| f.0).filter(|p| p.extension().is_some_and(|e| e == "md") && !p.starts_with(".trash") && !p.starts_with(".shale")).collect()
    }

    fn folders(&self) -> Vec<PathBuf> {
        let mut f = vec![PathBuf::new()];
        f.extend(self.app.vault.as_ref().unwrap().folders.iter().cloned());
        f
    }

    /// Close a folder for writing. Only Unix folders can be closed that way: on Windows a
    /// read-only folder still takes files, so there this operation of the run does nothing.
    fn lock(&mut self, dir: &Path) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(self.root.join(dir), std::fs::Permissions::from_mode(0o555)).unwrap();
        }
        self.locked.push(dir.to_path_buf());
    }

    /// Make every folder writable again (a locked folder may have been renamed meanwhile).
    fn unlock_all(&mut self) {
        fn walk(p: &Path) {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755));
            }
            if let Ok(rd) = std::fs::read_dir(p) {
                for e in rd.flatten() {
                    if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        walk(&e.path());
                    }
                }
            }
        }
        self.locked.clear();
        walk(&self.root);
    }

    fn open_doc(&mut self, rel: &Path) -> bool {
        if !self.app.notes.contains_key(rel) {
            self.app.open(rel, false);
        }
        self.app.notes.contains_key(rel)
    }

    /// One random step. Returns the versions it replaces on purpose.
    fn step(&mut self) -> Vec<u64> {
        let notes = self.notes_on_disk();
        let mut allowed = vec![];
        let op = self.rng.below(100);
        // some steps run with a read-only folder, so that a write fails
        if self.rng.below(100) < 12 && !(55..=68).contains(&op) {
            let mut dirs = self.folders();
            if self.root.join(".trash").is_dir() {
                dirs.push(PathBuf::from(".trash"));
            }
            let d = dirs[self.rng.below(dirs.len())].clone();
            self.log.push(format!("lock {}", d.display()));
            self.lock(&d);
        }
        match op {
            // create
            0..=5 => {
                let folder = PathBuf::from(FOLDERS[self.rng.below(FOLDERS.len())]);
                let name = STEMS[self.rng.below(STEMS.len())];
                self.log.push(format!("create {}/{name}", folder.display()));
                if let Some(rel) = self.app.create_note(&folder, name, false) {
                    // the new note gets text the way typing does
                    let t = self.content();
                    let doc = self.app.notes.get_mut(&rel).unwrap();
                    doc.text = t;
                    doc.dirty = true;
                    doc.last_edit = Instant::now() - Duration::from_secs(5);
                }
            }
            // open and type
            6..=26 => {
                if let Some(rel) = self.rng.pick(&notes).cloned() {
                    if self.open_doc(&rel) && self.app.notes[&rel].unreadable.is_none() {
                        let t = self.content();
                        let doc = self.app.notes.get_mut(&rel).unwrap();
                        if doc.dirty {
                            allowed.extend(token(doc.text.as_bytes()));
                        }
                        self.log.push(format!("type {} {}", rel.display(), t.lines().next().unwrap()));
                        doc.text = t.clone();
                        doc.dirty = true;
                        doc.last_edit = Instant::now() - Duration::from_secs(5);
                        self.app.vault.as_mut().unwrap().set_note_text(&rel, &t);
                    }
                }
            }
            27..=40 => {
                self.log.push("autosave".into());
                self.app.autosave();
            }
            41..=46 => {
                self.log.push("flush".into());
                self.app.flush_all();
            }
            47..=54 => {
                self.log.push("rescan".into());
                self.app.rescan();
            }
            // another program writes a note
            55..=62 => {
                let rel = self.rng.pick(&notes).cloned().unwrap_or_else(|| PathBuf::from("New.md"));
                let old = crate::fsutil::read(self.root.join(&rel)).ok();
                allowed.extend(old.as_deref().and_then(token));
                let t = self.content();
                self.log.push(format!("outside write {} {}", rel.display(), t.lines().next().unwrap()));
                crate::fsutil::write_atomic(&self.root.join(&rel), t.as_bytes()).unwrap();
            }
            // another program leaves bytes that are not text
            63..=65 => {
                let rel = self.rng.pick(&notes).cloned().unwrap_or_else(|| PathBuf::from("Bad.md"));
                let old = crate::fsutil::read(self.root.join(&rel)).ok();
                allowed.extend(old.as_deref().and_then(token));
                let n = self.next;
                self.next += 1;
                self.log.push(format!("outside unreadable {} <{n}>", rel.display()));
                let mut bytes = vec![0xff, 0xfe];
                bytes.extend_from_slice(format!("<{n}> x\n<{n}> END\n").as_bytes());
                crate::fsutil::write_atomic(&self.root.join(&rel), &bytes).unwrap();
            }
            66..=68 => {
                if let Some(rel) = self.rng.pick(&notes).cloned() {
                    let old = crate::fsutil::read(self.root.join(&rel)).ok();
                    allowed.extend(old.as_deref().and_then(token));
                    self.log.push(format!("outside delete {}", rel.display()));
                    std::fs::remove_file(self.root.join(&rel)).unwrap();
                }
            }
            // rename (also with names that must be refused)
            69..=80 => {
                if let Some(rel) = self.rng.pick(&notes).cloned() {
                    let hostile = ["../outside/sentinel", "a/b", "..", "/etc/x", "../../x", "  "];
                    let name = if self.rng.below(5) == 0 { hostile[self.rng.below(hostile.len())].to_string() } else { STEMS[self.rng.below(STEMS.len())].to_string() };
                    self.log.push(format!("rename {} -> {name}", rel.display()));
                    let _ = self.app.rename(&rel, &name);
                }
            }
            81..=89 => {
                let target = self.rng.pick(&notes).cloned().or_else(|| self.folders().into_iter().nth(1));
                if let Some(rel) = target {
                    let mut dirs: Vec<String> = self.folders().iter().map(|p| p.to_string_lossy().into_owned()).collect();
                    dirs.push("../outside".into());
                    let into = PathBuf::from(&dirs[self.rng.below(dirs.len())]);
                    self.log.push(format!("move {} -> {}", rel.display(), into.display()));
                    let _ = self.app.move_into(&rel, &into);
                }
            }
            90..=96 => {
                if let Some(rel) = self.rng.pick(&notes).cloned() {
                    self.log.push(format!("trash {}", rel.display()));
                    self.app.trash(&rel);
                }
            }
            // a folder is renamed
            _ => {
                let dirs: Vec<PathBuf> = self.folders().into_iter().filter(|p| !p.as_os_str().is_empty()).collect();
                if let (Some(rel), 0) = (self.rng.pick(&dirs).cloned(), self.rng.below(2)) {
                    self.log.push(format!("trash folder {}", rel.display()));
                    self.app.trash(&rel);
                } else if let Some(rel) = self.rng.pick(&dirs).cloned() {
                    let name = format!("F{}", self.rng.below(3));
                    self.log.push(format!("rename folder {} -> {name}", rel.display()));
                    let _ = self.app.rename(&rel, &name);
                }
            }
        }
        self.unlock_all();
        allowed
    }

    fn check(&self, before: &BTreeMap<u64, Vec<Holder>>, bases: &HashMap<PathBuf, Stamp>, allowed: &[u64]) -> Result<(), String> {
        let after = holders(&self.app, &self.root);
        for (t, hs) in before {
            if after.contains_key(t) || allowed.contains(t) {
                continue;
            }
            // a save over exactly the version the buffer had loaded replaces it on purpose
            let justified = hs.iter().all(|h| matches!(h, Holder::File(p, st) if bases.get(p) == Some(st)));
            if !justified {
                return Err(format!("version <{t}> is lost (held by {hs:?})"));
            }
        }
        let files = all_files(&self.root);
        for (p, bytes) in &files {
            if !well_formed(&plain(bytes)) {
                return Err(format!("{} is torn or foreign: {:?}", p.display(), String::from_utf8_lossy(bytes)));
            }
            if p.file_name().is_some_and(|n| n.to_string_lossy().contains(".shale-tmp-")) {
                return Err(format!("temporary file left: {}", p.display()));
            }
        }
        let mut outer: Vec<String> = std::fs::read_dir(&self.sandbox).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        outer.sort();
        if outer != ["outside", "vault"] {
            return Err(format!("something appeared next to the vault: {outer:?}"));
        }
        let outside = all_files(&self.sandbox.join("outside"));
        if outside.len() != 1 || outside[0].1 != b"do not touch\n" {
            return Err("the folder outside the vault changed".into());
        }
        if let Some((p, _)) = self.app.notes.iter().find(|(_, d)| d.dirty && d.unreadable.is_some()) {
            return Err(format!("{} is unreadable and has unsaved edits", p.display()));
        }
        Ok(())
    }

    fn run(&mut self, steps: usize) {
        for i in 0..steps {
            let before = holders(&self.app, &self.root);
            let bases: HashMap<PathBuf, Stamp> = self.app.notes.iter().map(|(p, d)| (p.clone(), d.base)).collect();
            let from = self.log.len();
            let allowed = self.step();
            let checked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.check(&before, &bases, &allowed)));
            let checked = checked.unwrap_or_else(|_| Err("the check itself failed".into()));
            if let Err(e) = checked {
                let ops = self.log[from.saturating_sub(12)..].join("\n  ");
                panic!("step {i}: {e}\nlast operations:\n  {ops}");
            }
        }
    }

    /// With every problem gone, one flush must save everything that is still unsaved.
    fn settle(&mut self) {
        self.unlock_all();
        self.app.flush_all();
        self.app.flush_all();
        let dirty: Vec<_> = self.app.notes.iter().filter(|(_, d)| d.dirty).map(|(p, _)| p.clone()).collect();
        assert!(dirty.is_empty(), "still unsaved after the problems are gone: {dirty:?}");
        let before = holders(&self.app, &self.root);
        self.check(&before, &HashMap::new(), &[]).unwrap();
    }
}

impl Drop for Sim {
    fn drop(&mut self) {
        self.unlock_all();
        let _ = std::fs::remove_dir_all(&self.sandbox);
    }
}

// spec: DATA-002, DATA-003, DATA-004, DATA-008, DATA-009, VAULT-014, VAULT-016, VAULT-018
#[test]
fn random_vault_operations_never_lose_data() {
    // (Running as root, a read-only folder does not stop a write, so the lock steps are plain
    // steps there; the other checks are the same.)
    let mut steps = 0;
    for seed in 1..=12u64 {
        let mut sim = Sim::new(seed);
        sim.run(1000);
        sim.settle();
        steps += 1000;
    }
    assert!(steps >= 10_000);
}
