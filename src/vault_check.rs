//! The vault check for other systems (VAULT-028): which files and folders of a vault have names
//! that break on Windows or macOS (VAULT-026 says what makes a name portable), with the portable
//! name each should get, and the renames that fix them through the one routine that rewrites
//! links (VAULT-016).
//!
//! [`check`] is a pure scan of a folder; the Settings → Vault page runs it in a background job
//! ([`VaultCheck`] holds its state) and renames through `ShaleApp::rename_path`.

use crate::app::ShaleApp;
use crate::fsutil::{portable_name, portable_problems, TYPED_NAME_MAX};
use crate::vault::rel_to_string;
use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};

/// Folders that are not part of the notes and are not looked at.
const SKIPPED: [&str; 3] = [".git", ".shale", ".trash"];
/// A vault-relative path longer than this (in characters) is a problem: Windows allows 260 for the
/// whole path, the rest is the margin for where the vault lives.
pub const MAX_PATH_CHARS: usize = 240;

/// A file or folder whose name would break on another system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// where it is now, relative to the vault
    pub rel: PathBuf,
    pub is_dir: bool,
    /// what is wrong, one phrase each
    pub reasons: Vec<String>,
    /// the file name it should get (in the same folder); `None` when no name can fix it (the
    /// folders above it are what makes the path too long)
    pub proposed: Option<String>,
}

/// Every file and folder of the vault at `root` that has a name that is not portable, in the order
/// of a walk through the folders. `.git`, `.shale` and `.trash` are skipped, and symbolic links are
/// listed but not followed.
pub fn check(root: &Path) -> io::Result<Vec<Finding>> {
    let _flow = tracing::info_span!("flow.vault_check", peer = "page").entered();
    let mut out = Vec::new();
    walk(root, Path::new(""), &mut out)?;
    Ok(out)
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<Finding>) -> io::Result<()> {
    let mut entries: Vec<(String, bool)> = Vec::new();
    let listing = tracing::info_span!("flow.vault_check.read_folder", peer = "disk").in_scope(|| std::fs::read_dir(root.join(dir)))?;
    for e in listing {
        let e = e?;
        // a name that is no text cannot be shown or renamed here
        let Ok(name) = e.file_name().into_string() else { continue };
        if SKIPPED.contains(&name.as_str()) {
            continue;
        }
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        entries.push((name, is_dir));
    }
    entries.sort();
    let mut found = check_folder(dir, &entries).into_iter().peekable();
    // each finding, then what is inside the folder it names: the order of a walk
    for (name, is_dir) in &entries {
        let rel = dir.join(name);
        if let Some(f) = found.next_if(|f| f.rel == rel) {
            out.push(f);
        }
        if *is_dir && !fs_is_link(&root.join(&rel)) {
            walk(root, &rel, out)?;
        }
    }
    Ok(())
}

/// The findings among the entries of one folder, `dir` relative to the vault, given by name and
/// whether each is a folder, sorted by name. This is the whole check of a folder without the file
/// system, so names that the running system cannot hold (`a:b.md` on Windows, `Plan.md` next to
/// `plan.md` on macOS) are checked the same everywhere.
pub fn check_folder(dir: &Path, entries: &[(String, bool)]) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut taken: HashSet<String> = entries.iter().map(|(n, _)| n.to_lowercase()).collect();
    let parent_chars = if dir.as_os_str().is_empty() { 0 } else { rel_to_string(dir).chars().count() + 1 };
    // the first name of a group that differs only in letter case stays as it is
    let mut first_of: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for (name, _) in entries {
        first_of.entry(name.to_lowercase()).or_insert_with(|| name.clone());
    }
    for (name, is_dir) in entries {
        let mut reasons = portable_problems(name, TYPED_NAME_MAX);
        if name.ends_with(' ') {
            reasons.push("a name can't end in a space on Windows".into());
        }
        let clash = first_of.get(&name.to_lowercase()).filter(|first| *first != name);
        if let Some(first) = clash {
            reasons.push(format!("differs only in letter case from `{first}`"));
        }
        // only the first one over the limit: what is below it is over the limit because of it
        let path_chars = parent_chars + name.chars().count();
        let too_long = path_chars > MAX_PATH_CHARS && parent_chars.saturating_sub(1) <= MAX_PATH_CHARS;
        if too_long {
            reasons.push(format!("the path is {path_chars} characters long, more than {MAX_PATH_CHARS}"));
        }
        if reasons.is_empty() {
            continue;
        }
        let budget = if too_long { MAX_PATH_CHARS.saturating_sub(parent_chars).min(TYPED_NAME_MAX) } else { TYPED_NAME_MAX };
        let cut = portable_name(name, budget);
        let fallback = if *is_dir { "folder" } else { "file" };
        // nothing left of the name: a name for it, unless even that does not fit
        let portable = if cut.is_empty() { (budget >= fallback.len()).then(|| fallback.to_string()) } else { Some(cut) };
        // a name that clashes keeps its own letters and only gets a number
        let wanted = if clash.is_some() { Some(portable.unwrap_or_else(|| name.clone())) } else { portable.filter(|p| p != name) };
        let proposed = wanted.map(|w| free_name(&w, *is_dir, |n| taken.contains(&n.to_lowercase())));
        if let Some(p) = &proposed {
            taken.insert(p.to_lowercase());
        }
        out.push(Finding { rel: dir.join(name), is_dir: *is_dir, reasons, proposed });
    }
    out
}

fn fs_is_link(p: &Path) -> bool {
    std::fs::symlink_metadata(p).map(|m| m.file_type().is_symlink()).unwrap_or(false)
}

/// `name`, or `name 2`, `name 3` … (before the extension; for a folder at the end) until `taken`
/// no longer says the name is. The stem of a numbered name is cut (at a character boundary) so
/// that it stays within 200 bytes.
pub fn free_name(name: &str, is_dir: bool, taken: impl Fn(&str) -> bool) -> String {
    if !taken(name) {
        return name.to_string();
    }
    let split = if is_dir {
        None
    } else if name.to_ascii_lowercase().ends_with(".excalidraw.md") {
        Some(name.len() - ".excalidraw.md".len())
    } else {
        name.rfind('.').filter(|i| *i > 0)
    };
    let (stem, ext) = name.split_at(split.unwrap_or(name.len()));
    (2..)
        .map(|n| {
            let number = format!(" {n}");
            // the stem gives way, so that a name within 200 bytes stays within them
            let mut keep = stem.len().min(TYPED_NAME_MAX.saturating_sub(number.len() + ext.len()).max(1));
            while !stem.is_char_boundary(keep) {
                keep -= 1;
            }
            format!("{}{number}{ext}", &stem[..keep])
        })
        .find(|c| !taken(c))
        .expect("a free number exists")
}

/// The state of the check on the Settings → Vault page.
#[derive(Default)]
pub struct VaultCheck {
    job: Option<Receiver<Result<io::Result<Vec<Finding>>, String>>>,
    /// the last result, `None` until the check ran
    pub result: Option<Result<Vec<Finding>, String>>,
    /// "Rename all" waits for a yes
    pub confirm_all: bool,
    /// what the last rename did, for the page
    pub report: Option<String>,
}

impl VaultCheck {
    pub fn running(&self) -> bool {
        self.job.is_some()
    }

    /// Take the result of a finished job; true while one still runs.
    pub fn poll(&mut self) -> bool {
        let Some(rx) = &self.job else { return false };
        match rx.try_recv() {
            Ok(r) => self.result = Some(r.and_then(|r| r.map_err(|e| format!("The vault could not be read: {e}")))),
            Err(TryRecvError::Empty) => return true,
            Err(TryRecvError::Disconnected) => self.result = Some(Err("The check stopped unexpectedly".into())),
        }
        self.job = None;
        false
    }
}

impl ShaleApp {
    /// Run the check in the background (VAULT-028).
    pub(crate) fn start_vault_check(&mut self) {
        let Some(root) = self.vault.as_ref().map(|v| v.root.clone()) else { return };
        self.flush_all();
        let ctx = self.ctx.clone();
        self.vault_check.confirm_all = false;
        self.vault_check.report = None;
        self.vault_check.job = Some(crate::jobs::spawn(
            move || check(&root),
            move || {
                if let Some(c) = ctx {
                    c.request_repaint();
                }
            },
        ));
    }

    /// Rename the findings `which` (indexes into the last result) as VAULT-016 does: deepest
    /// first, so a folder is renamed after what is in it. A name that is taken meanwhile gets a
    /// number. Tells how many were renamed and what failed, then checks again.
    pub(crate) fn rename_findings(&mut self, which: &[usize]) {
        let Some(Ok(found)) = self.vault_check.result.clone() else { return };
        let mut todo: Vec<&Finding> = which.iter().filter_map(|i| found.get(*i)).filter(|f| f.proposed.is_some()).collect();
        todo.sort_by_key(|f| std::cmp::Reverse(f.rel.components().count()));
        let (mut done, mut links, mut errors) = (0, 0, Vec::new());
        for f in todo {
            match self.rename_finding(f) {
                Ok(n) => {
                    done += 1;
                    links += n;
                }
                Err(e) => errors.push(format!("“{}”: {e}", rel_to_string(&f.rel))),
            }
        }
        let mut report = format!("Renamed {done} {}", if done == 1 { "file or folder" } else { "files and folders" });
        if links > 0 {
            report.push_str(&format!(", updated links in {links} file(s)"));
        }
        if !errors.is_empty() {
            report.push_str(&format!(". Not renamed: {}", errors.join(", ")));
        }
        self.toast(report.clone());
        self.start_vault_check();
        self.vault_check.report = Some(report);
    }

    /// One finding: its proposed name, or the next free one. Returns how many files had links rewritten.
    fn rename_finding(&mut self, f: &Finding) -> Result<usize, String> {
        let proposed = f.proposed.as_deref().ok_or("no name fits")?;
        let root = self.vault.as_ref().ok_or("no vault")?.root.clone();
        let parent = f.rel.parent().unwrap_or(Path::new(""));
        // names in the folder now, for a name that was taken after the check
        let existing: HashSet<String> = std::fs::read_dir(root.join(parent)).map_err(|e| e.to_string())?.flatten().map(|e| e.file_name().to_string_lossy().to_lowercase()).collect();
        let name = free_name(proposed, f.is_dir, |n| existing.contains(&n.to_lowercase()));
        let (changed, _failed) = self.rename_path(&f.rel, &parent.join(name))?;
        Ok(changed.changed.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Options;
    use std::fs;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("shale-vcheck-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn touch(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }

    fn found<'a>(all: &'a [Finding], rel: &str) -> &'a Finding {
        all.iter().find(|f| f.rel == Path::new(rel)).unwrap_or_else(|| panic!("no finding for {rel}: {:?}", all.iter().map(|f| &f.rel).collect::<Vec<_>>()))
    }

    /// The check of one folder `dir` holding these names (`true`: a folder), without files: names
    /// that the running file system cannot hold are checked the same on every system.
    fn names(dir: &str, entries: &[(&str, bool)]) -> Vec<Finding> {
        let mut e: Vec<(String, bool)> = entries.iter().map(|(n, d)| (n.to_string(), *d)).collect();
        e.sort();
        check_folder(Path::new(dir), &e)
    }

    /// Whether the file system of the temp folder holds files of exactly these names side by side.
    /// Windows refuses `:` and `?` (or makes `a:b` a stream of the file `a`) and drops a trailing
    /// dot; macOS and Windows keep one file of `Plan.md` and `plan.md`.
    fn fs_holds(tag: &str, wanted: &[&str]) -> bool {
        let d = temp(&format!("holds-{tag}"));
        let made = wanted.iter().all(|n| fs::write(d.join(n), "x").is_ok());
        let mut listed: Vec<String> = fs::read_dir(&d).map(|r| r.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
        listed.sort();
        let mut want: Vec<String> = wanted.iter().map(|n| n.to_string()).collect();
        want.sort();
        let _ = fs::remove_dir_all(&d);
        made && listed == want
    }

    fn skipped(test: &str, wanted: &[&str]) {
        println!("{test}: the part on disk is skipped, this file system cannot hold {wanted:?} (the names are checked without files)");
    }

    // spec: VAULT-028
    #[test]
    fn the_check_names_each_reason_with_the_portable_name_and_leaves_good_names_out() {
        let long = format!("{}.md", "n".repeat(205));
        let root = [
            ("Fine.md", false),
            ("Ümlaut – ok.md", false),
            ("a:b.md", false),
            ("what?.md", false),
            ("tab\there.md", false),
            ("dots...", false),
            ("NUL.txt", false),
            ("con.md", false),
            ("space .md", false),
            ("end ", false),
            (long.as_str(), false),
            ("Dir*", true),
            ("Notes", true),
        ];
        let all = names("", &root);
        assert_eq!(found(&all, "a:b.md").reasons, vec!["`:` is not allowed on Windows"]);
        assert_eq!(found(&all, "a:b.md").proposed.as_deref(), Some("a-b.md"));
        assert_eq!(found(&all, "what?.md").proposed.as_deref(), Some("what-.md"));
        assert_eq!(found(&all, "tab\there.md").reasons, vec!["control characters are not allowed on Windows"]);
        assert_eq!(found(&all, "tab\there.md").proposed.as_deref(), Some("tab-here.md"));
        assert_eq!(found(&all, "dots...").reasons, vec!["a name can't end in a dot on Windows"]);
        assert_eq!(found(&all, "dots...").proposed.as_deref(), Some("dots"));
        assert_eq!(found(&all, "NUL.txt").reasons, vec!["`NUL` is a reserved name on Windows"]);
        assert_eq!(found(&all, "NUL.txt").proposed.as_deref(), Some("NUL_.txt"));
        assert_eq!(found(&all, "con.md").proposed.as_deref(), Some("con_.md"));
        assert_eq!(found(&all, "end ").reasons, vec!["a name can't end in a space on Windows"]);
        assert_eq!(found(&all, "end ").proposed.as_deref(), Some("end"));
        let f = found(&all, &long);
        assert_eq!(f.reasons, vec!["the name is longer than 200 bytes"]);
        let p = f.proposed.clone().unwrap();
        assert!(p.len() <= 200 && p.ends_with(".md"), "{p}");
        // a folder and what is in it, each by its own name
        assert!(found(&all, "Dir*").is_dir);
        assert_eq!(found(&all, "Dir*").proposed.as_deref(), Some("Dir-"));
        let inner = names("Dir*", &[("in.md", false), ("x?.md", false)]);
        assert_eq!(found(&inner, "Dir*/x?.md").proposed.as_deref(), Some("x-.md"));
        assert_eq!(inner.len(), 1, "Dir*/in.md is fine");
        assert!(names("Notes", &[("Good.md", false)]).is_empty());
        for ok in ["Fine.md", "Ümlaut – ok.md", "Notes", "space .md"] {
            assert!(all.iter().all(|f| f.rel != Path::new(ok)), "{ok} is fine");
        }
        // on disk the walk finds the same, the findings inside a folder right after the folder
        let odd = ["a:b.md", "what?.md", "tab\there.md", "dots...", "NUL.txt", "con.md", "end ", "Dir*", "x?.md"];
        if !fs_holds("reasons", &odd) {
            return skipped("the_check_names_each_reason_with_the_portable_name_and_leaves_good_names_out", &odd);
        }
        let d = temp("reasons");
        for (name, is_dir) in root {
            if !is_dir {
                touch(&d, name, "x");
            }
        }
        for f in ["Dir*/in.md", "Dir*/x?.md", "Notes/Good.md"] {
            touch(&d, f, "x");
        }
        let mut walked = Vec::new();
        for f in all {
            let is_dir_star = f.rel == Path::new("Dir*");
            walked.push(f);
            if is_dir_star {
                walked.extend(inner.clone());
            }
        }
        assert_eq!(check(&d).unwrap(), walked);
        let _ = fs::remove_dir_all(&d);
    }

    // spec: VAULT-028
    #[test]
    fn names_that_differ_only_in_letter_case_are_found_per_folder() {
        let root = [("Plan.md", false), ("plan.md", false), ("PLAN.md", false), ("Other", true)];
        let all = names("", &root);
        // the first of the group (by name) stays, the others get a number
        assert!(all.iter().all(|f| f.rel != Path::new("PLAN.md")), "PLAN.md sorts first and stays: {all:?}");
        let plan = found(&all, "Plan.md");
        assert_eq!(plan.reasons, vec!["differs only in letter case from `PLAN.md`"]);
        assert_eq!(plan.proposed.as_deref(), Some("Plan 2.md"));
        assert_eq!(found(&all, "plan.md").proposed.as_deref(), Some("plan 3.md"));
        assert_eq!(all.len(), 2);
        assert!(names("Other", &[("plan.md", false), ("Notes.md", false)]).is_empty(), "another folder is another namespace");
        // a folder and a file clash as well
        let all = names("", &[("Docs.d", true), ("docs.D", false)]);
        assert_eq!(found(&all, "docs.D").reasons, vec!["differs only in letter case from `Docs.d`"]);
        // on disk, where the file system tells them apart (Linux; not macOS or Windows)
        let group = ["Plan.md", "plan.md", "PLAN.md"];
        if !fs_holds("case", &group) {
            return skipped("names_that_differ_only_in_letter_case_are_found_per_folder", &group);
        }
        let d = temp("case");
        for f in ["Plan.md", "plan.md", "PLAN.md", "Other/plan.md", "Other/Notes.md"] {
            touch(&d, f, "x");
        }
        assert_eq!(check(&d).unwrap(), names("", &root));
        let _ = fs::remove_dir_all(&d);
    }

    // spec: VAULT-028
    #[test]
    fn a_taken_name_gets_a_number_also_among_the_proposals() {
        let all = names("", &[("a-b.md", false), ("a:b.md", false), ("a?b.md", false), ("x:y.excalidraw.md", false), ("x-y.excalidraw.md", false), ("q*", false), ("q-", false)]);
        assert_eq!(found(&all, "a:b.md").proposed.as_deref(), Some("a-b 2.md"));
        assert_eq!(found(&all, "a?b.md").proposed.as_deref(), Some("a-b 3.md"));
        assert_eq!(found(&all, "x:y.excalidraw.md").proposed.as_deref(), Some("x-y 2.excalidraw.md"), "the number goes before the double extension");
        assert_eq!(found(&all, "q*").proposed.as_deref(), Some("q- 2"));
        assert_eq!(free_name("n.md", false, |n| n == "n.md" || n == "n 2.md"), "n 3.md");
        assert_eq!(free_name("n.md", false, |_| false), "n.md");
        assert_eq!(free_name("dir.x", true, |n| n == "dir.x"), "dir.x 2", "a folder has no extension");
        // a name cut to 200 bytes stays within them with its number (two bytes a letter)
        let cut = format!("{}.md", "λ".repeat(98));
        let numbered = free_name(&cut, false, |n| n == cut);
        assert_eq!(numbered, format!("{} 2.md", "λ".repeat(97)));
        assert!(numbered.len() <= TYPED_NAME_MAX);
    }

    // spec: VAULT-028
    #[test]
    fn a_path_over_240_characters_is_found_once_at_the_place_where_it_gets_too_long() {
        let d = temp("longpath");
        let seg = "d".repeat(60);
        let rel = format!("{seg}/{seg}/{seg}/{seg}");
        touch(&d, &format!("{rel}/file.md"), "x");
        touch(&d, &format!("{rel}/inner/deeper/file.md"), "x");
        let all = check(&d).unwrap();
        // 60*4 + 3 slashes = 243 characters: the fourth folder is the first one over the limit
        assert_eq!(all.len(), 1, "{all:?}");
        assert_eq!(all[0].rel, PathBuf::from(&rel));
        assert!(all[0].reasons[0].starts_with("the path is 243 characters long, more than 240"), "{:?}", all[0].reasons);
        let p = all[0].proposed.clone().unwrap();
        assert_eq!(p.len(), 240 - 183, "cut to what is left of the 240");
        // a file whose own name is the end of a long path is cut but keeps its extension
        let d2 = temp("longpath-file");
        let dir = format!("{seg}/{seg}/{seg}");
        touch(&d2, &format!("{dir}/{}.md", "f".repeat(80)), "x");
        let all = check(&d2).unwrap();
        assert_eq!(all.len(), 1);
        let p = all[0].proposed.clone().unwrap();
        assert!(p.ends_with(".md") && p.len() == 240 - 183, "{p}");
        // folders so long that no name helps: the finding has no proposal
        let d3 = temp("longpath-none");
        let (a, b) = ("a".repeat(119), "b".repeat(120));
        touch(&d3, &format!("{a}/{b}/c/x.md"), "x");
        let all = check(&d3).unwrap();
        assert_eq!(all.len(), 1, "{all:?}");
        assert_eq!(all[0].rel, PathBuf::from(format!("{a}/{b}/c")));
        assert_eq!(all[0].proposed, None);
        let _ = (fs::remove_dir_all(&d), fs::remove_dir_all(&d2), fs::remove_dir_all(&d3));
    }

    // spec: VAULT-028
    #[test]
    fn the_check_skips_git_shale_and_trash_and_does_not_follow_links() {
        // a name over 200 bytes: wrong on every system, and every system can hold it
        let long = format!("{}.md", "n".repeat(205));
        let d = temp("skipped");
        for f in [".git", ".shale", ".trash/2026-11-17", "Real"] {
            touch(&d, &format!("{f}/{long}"), "x");
        }
        touch(&d, "Notes/ok.md", "x");
        #[cfg(unix)]
        std::os::unix::fs::symlink(d.join("Real"), d.join("Link:dir")).unwrap();
        let all = check(&d).unwrap();
        let rels: Vec<String> = all.iter().map(|f| rel_to_string(&f.rel)).collect();
        let inside = format!("Real/{long}");
        #[cfg(unix)]
        assert_eq!(rels, vec!["Link:dir".to_string(), inside], "the link itself is listed, what it points to is listed once");
        #[cfg(not(unix))]
        assert_eq!(rels, vec![inside]);
        assert!(check(&d.join("missing")).is_err());
        let _ = fs::remove_dir_all(&d);
    }

    fn app_in(d: &Path) -> ShaleApp {
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(d.to_path_buf()), view: None, keys: None }, false);
        app.vault.as_mut().unwrap().rescan();
        app
    }

    fn run_check(app: &mut ShaleApp) -> Vec<Finding> {
        app.start_vault_check();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while app.vault_check.poll() {
            assert!(std::time::Instant::now() < deadline, "the check did not finish");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        app.vault_check.result.clone().unwrap().unwrap()
    }

    // The renames below use names over 200 bytes (two bytes a letter), which every system holds; in
    // the temp folder their paths are longer than Windows' 260 characters.

    // spec: VAULT-028
    #[test]
    fn rename_all_renames_deepest_first_rewrites_links_and_checks_again() {
        let d = temp("rename-all");
        let (x, dir, inner) = ("λ".repeat(101), "δ".repeat(101), "ι".repeat(99));
        touch(&d, &format!("{x}.md"), "x\n");
        touch(&d, &format!("{dir}/{inner}.md"), "inside\n");
        touch(&d, "Linker.md", &format!("[[{x}]] [[{inner}]]\n"));
        let mut app = app_in(&d);
        let all = run_check(&mut app);
        assert_eq!(all.len(), 3, "{all:?}");
        let which: Vec<usize> = (0..all.len()).collect();
        app.rename_findings(&which);
        let (x2, dir2, inner2) = ("λ".repeat(98), "δ".repeat(100), "ι".repeat(98));
        assert!(d.join(format!("{x2}.md")).exists() && d.join(format!("{dir2}/{inner2}.md")).exists(), "the folder and what was in it");
        assert!(!d.join(format!("{x}.md")).exists() && !d.join(&dir).exists());
        assert_eq!(fs::read_to_string(d.join("Linker.md")).unwrap(), format!("[[{x2}]] [[{inner2}]]\n"), "links follow");
        let report = app.vault_check.report.clone().unwrap();
        assert!(report.starts_with("Renamed 3 files and folders, updated links in "), "{report}");
        assert!(app.toasts.iter().any(|(t, _)| *t == report));
        // checked again in the background: nothing is left
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while app.vault_check.poll() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(app.vault_check.result, Some(Ok(vec![])));
        let _ = fs::remove_dir_all(&d);
    }

    // spec: VAULT-028
    #[test]
    fn one_finding_is_renamed_alone_and_a_name_taken_since_the_check_gets_a_number() {
        let d = temp("rename-one");
        let (a, c) = ("α".repeat(101), "γ".repeat(101));
        touch(&d, &format!("{a}.md"), "A\n");
        touch(&d, &format!("{c}.md"), "C\n");
        touch(&d, "Linker.md", &format!("[[{a}]] [[{c}]]\n"));
        let mut app = app_in(&d);
        let all = run_check(&mut app);
        let i = all.iter().position(|f| f.rel == Path::new(&format!("{a}.md"))).unwrap();
        assert_eq!(all[i].proposed, Some(format!("{}.md", "α".repeat(98))));
        // somebody makes that name (in capitals) after the check
        let other = format!("{}.md", "Α".repeat(98));
        touch(&d, &other, "other\n");
        app.rename_findings(&[i]);
        let got = format!("{} 2", "α".repeat(97));
        assert_eq!(fs::read_to_string(d.join(format!("{got}.md"))).unwrap(), "A\n");
        assert_eq!(fs::read_to_string(d.join(&other)).unwrap(), "other\n", "nothing is replaced");
        assert!(d.join(format!("{c}.md")).exists(), "the other finding stays");
        let linker = fs::read_to_string(d.join("Linker.md")).unwrap();
        assert!(linker.starts_with(&format!("[[{got}]] [[{c}]]")), "{linker}");
        assert!(app.vault_check.report.clone().unwrap().starts_with("Renamed 1 file or folder"));
        // a finding without a proposal is not renamed
        let none = Finding { rel: PathBuf::from(format!("{c}.md")), is_dir: false, reasons: vec![], proposed: None };
        app.vault_check.result = Some(Ok(vec![none]));
        app.rename_findings(&[0]);
        assert!(d.join(format!("{c}.md")).exists());
        assert!(app.vault_check.report.clone().unwrap().starts_with("Renamed 0 files and folders"));
        let _ = fs::remove_dir_all(&d);
    }

    // spec: DEV-031
    #[test]
    fn dataflow_vault_check() {
        let d = temp("dataflow");
        touch(&d, "Notes/Fine.md", "x");
        touch(&d, "Notes/Deep/Also fine.md", "x");
        touch(&d, "Other/bad name .md", "x");
        let flow = crate::dataflow::record(|| {
            check(&d).unwrap();
        });
        crate::dataflow::verify(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/dataflow/vault-check.md"), "vault_check", &crate::dataflow::render_mermaid(&flow));
        let _ = fs::remove_dir_all(&d);
    }
}
