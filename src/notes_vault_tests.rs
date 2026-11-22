//! The test suite run against a real vault (BUILD-008): `notes/` next to `Cargo.toml` (kept out of git), or
//! the folder in `SHALE_TEST_VAULT`. Without one these tests pass with a note, so CI is not
//! affected. Put any notes, drawings and files there that Shale must handle.
//!
//! Every test works on its own **copy** of the vault and checks at the end that the original
//! was not touched. What is a failure here: a crash, a file Shale can't read, a drawing that
//! doesn't open or loses something when saved, links that stop resolving, broken task toggles.
//! Red links and known defects (marked ⚠ in `docs/spec/`) are only reported.

use crate::drawing::{DrawingDoc, Scene};
use crate::markdown;
use crate::vault::{rel_to_string, FileKind, Vault};
use crate::{editor, lists, reader};
use eframe::egui::{self, Pos2, RawInput, Rect, Vec2};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

fn source() -> Option<PathBuf> {
    let dir = std::env::var_os("SHALE_TEST_VAULT").map(PathBuf::from).unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("notes"));
    dir.is_dir().then_some(dir)
}

/// Every file below `dir` (also hidden ones) with its size and modification time.
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, (u64, Option<SystemTime>)> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(p),
                Ok(_) => {
                    let m = e.metadata().ok();
                    out.insert(p, (m.as_ref().map(|m| m.len()).unwrap_or(0), m.and_then(|m| m.modified().ok())));
                }
                Err(_) => {}
            }
        }
    }
    out
}

/// Copy the vault (without hidden folders such as `.obsidian` and `.git`, which Shale ignores).
fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap().flatten() {
        let name = e.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let (src, dst) = (e.path(), to.join(&name));
        match e.file_type() {
            Ok(t) if t.is_dir() => copy_dir(&src, &dst),
            Ok(t) if t.is_file() => {
                fs::copy(&src, &dst).unwrap();
            }
            _ => {} // symbolic links: Shale skips them too (VAULT-007)
        }
    }
}

/// Run `check` on a fresh copy of the test vault; afterwards the original must be unchanged.
fn with_vault(tag: &str, check: impl FnOnce(&Path, &mut Vault)) {
    let Some(src) = source() else {
        eprintln!("no test vault (notes/ or SHALE_TEST_VAULT): skipped");
        return;
    };
    let before = snapshot(&src);
    let copy = std::env::temp_dir().join(format!("shale-notes-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&copy);
    copy_dir(&src, &copy);
    let mut v = Vault::open(&copy).unwrap();
    check(&copy, &mut v);
    let _ = fs::remove_dir_all(&copy);
    assert_eq!(snapshot(&src), before, "the test vault itself was changed");
}

fn notes(v: &Vault) -> Vec<PathBuf> {
    v.files.iter().filter(|f| f.kind == FileKind::Note).map(|f| f.rel.clone()).collect()
}

fn fail_if_any(what: &str, problems: &[String]) {
    assert!(problems.is_empty(), "{what}:\n  {}", problems.join("\n  "));
}

// spec: VAULT-002, VAULT-003, DATA-008
#[test]
fn notes_every_file_is_listed_and_every_note_is_readable() {
    with_vault("files", |root, v| {
        // every visible file is in the file list, with the kind its name says
        let on_disk: BTreeSet<String> = snapshot(root).into_keys().map(|p| rel_to_string(p.strip_prefix(root).unwrap())).filter(|r| !r.split('/').any(|c| c.starts_with('.'))).collect();
        let listed: BTreeSet<String> = v.files.iter().map(|f| rel_to_string(&f.rel)).collect();
        assert_eq!(listed, on_disk, "the file list differs from the folder");
        let mut problems = Vec::new();
        for f in &v.files {
            let lower = rel_to_string(&f.rel).to_lowercase();
            let want = if crate::drawing::excalidraw_md::is_drawing_path(&f.rel) {
                FileKind::Drawing
            } else if lower.ends_with(".md") {
                FileKind::Note
            } else {
                FileKind::Other
            };
            if f.kind != want {
                problems.push(format!("{lower}: {:?}, expected {want:?}", f.kind));
            }
        }
        // a note that isn't UTF-8 would open empty and be overwritten (DATA-008 ⚠)
        for rel in notes(v) {
            if let Err(e) = fs::read_to_string(root.join(&rel)) {
                problems.push(format!("{}: can't be read as text ({e})", rel_to_string(&rel)));
            }
        }
        fail_if_any("files", &problems);
    });
}

// spec: DATA-006, LINK-001, LINK-004, TASK-001, TAG-001
#[test]
fn notes_every_note_parses_renders_and_exports() {
    with_vault("render", |_, v| {
        let ctx = egui::Context::default();
        let mut scenes = reader::SceneCache::default();
        let media = crate::media::MediaPlayer::default();
        let opts = crate::pdf::export::ExportOptions::for_note();
        let mut problems = Vec::new();
        for rel in notes(v) {
            let text = v.notes.get(&rel).map(|n| n.text.clone()).unwrap_or_default();
            let _ = (markdown::find_tasks(&text), markdown::find_tags(&text), markdown::frontmatter_tags(&text), markdown::find_headings(&text));
            for l in text.lines() {
                let _ = (lists::list_prefix(l), markdown::parse_task_line(l), markdown::heading_level(l));
            }
            for (i, _) in text.match_indices('\n') {
                let _ = lists::continue_list(&text, i);
            }
            let links: Vec<_> = markdown::find_links(&text)
                .into_iter()
                .map(|l| {
                    let ok = v.exists(&l.target, Some(&rel));
                    (l, ok)
                })
                .collect();
            let _ = editor::highlight(&text, &links, 15.0);
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(700.0, 900.0))), ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| {
                reader::show(ui, &text, v, &rel, &mut reader::Caches { scenes: &mut scenes, media: &media }, (15.0, true), &Default::default());
            });
            out.textures_delta.clear();
            match crate::pdf::export::export_note(v, &rel, &text, &opts) {
                Ok(pdf) if pdf.starts_with(b"%PDF") => {}
                Ok(_) => problems.push(format!("{}: PDF export wrote no PDF", rel_to_string(&rel))),
                Err(e) => problems.push(format!("{}: PDF export failed: {e}", rel_to_string(&rel))),
            }
        }
        fail_if_any("notes", &problems);
    });
}

// spec: LINK-002, LINK-007, LINK-011
#[test]
fn notes_links_backlinks_and_graph_agree() {
    with_vault("links", |_, v| {
        let mut problems = Vec::new();
        let mut red = BTreeSet::new();
        for rel in notes(v) {
            let n = &v.notes[&rel];
            for (l, target) in n.links.iter().zip(&n.resolved) {
                match target {
                    // a note that links to B is among B's backlinks
                    Some(t) if t != &rel => {
                        if !v.backlinks(t).iter().any(|(src, _)| src == &rel) {
                            problems.push(format!("{} → {}: not in its backlinks", rel_to_string(&rel), rel_to_string(t)));
                        }
                    }
                    Some(_) => {}
                    None => {
                        red.insert(format!("{} → [[{}]]", rel_to_string(&rel), l.target));
                    }
                }
            }
        }
        let (nodes, _) = v.graph(true);
        for f in v.files.iter().filter(|f| f.kind != FileKind::Other) {
            if !nodes.iter().any(|n| n.rel.as_deref() == Some(f.rel.as_path())) {
                problems.push(format!("{}: missing in the graph", rel_to_string(&f.rel)));
            }
        }
        let _ = (v.all_tags(), v.all_tasks());
        if !red.is_empty() {
            eprintln!("red links in the test vault (not an error):\n  {}", red.into_iter().collect::<Vec<_>>().join("\n  "));
        }
        fail_if_any("links", &problems);
    });
}

/// What must survive a save: shapes (by id and kind) and texts.
fn essence(s: &Scene) -> (BTreeSet<(String, String)>, BTreeSet<String>) {
    let shapes = s.live().filter(|e| e.kind != "text").map(|e| (e.id.clone(), e.kind.clone())).collect();
    let texts = s.live().filter_map(|e| e.text.clone()).collect();
    (shapes, texts)
}

// spec: DRAW-001, DRAW-007, DRAW-008, DRAW-011, DATA-005
#[test]
fn notes_every_drawing_opens_and_saves_without_loss() {
    with_vault("drawings", |root, v| {
        let mut problems = Vec::new();
        let drawings: Vec<PathBuf> = v.files.iter().filter(|f| f.kind == FileKind::Drawing).map(|f| f.rel.clone()).collect();
        for rel in &drawings {
            let name = rel_to_string(rel);
            let original = match fs::read_to_string(root.join(rel)) {
                Ok(t) => t,
                Err(e) => {
                    problems.push(format!("{name}: can't be read ({e})"));
                    continue;
                }
            };
            let doc = DrawingDoc::new(rel.clone(), &original);
            if let Some(e) = &doc.error {
                problems.push(format!("{name}: does not open in the drawing editor: {e}"));
                continue;
            }
            if doc.read_only.is_some() {
                // a canvas: shown, never written
                if doc.file_text().is_ok() {
                    problems.push(format!("{name}: a read-only drawing would be written"));
                }
                continue;
            }
            // save (to the copy) and read again: nothing may be lost
            let text = match doc.file_text() {
                Ok(t) => t,
                Err(e) => {
                    problems.push(format!("{name}: can't be saved: {e}"));
                    continue;
                }
            };
            fs::write(root.join(rel), &text).unwrap();
            match Scene::read_file(&root.join(rel)) {
                Ok(back) if essence(&back) == essence(&doc.scene) => {}
                Ok(_) => problems.push(format!("{name}: shapes or texts changed when saved")),
                Err(e) => problems.push(format!("{name}: unreadable after saving: {e}")),
            }
            if crate::drawing::excalidraw_md::is_plugin_file(rel) {
                // the plugin's layout stays: front matter, compression, the other sections
                let head = |t: &str| t.split("# Excalidraw Data").next().unwrap_or_default().to_string();
                if head(&text) != head(&original) {
                    problems.push(format!("{name}: the part before the drawing data changed"));
                }
                if text.contains("```compressed-json\n") != original.contains("```compressed-json\n") {
                    problems.push(format!("{name}: compression changed"));
                }
                for section in ["## Embedded Files", "## Element Links"] {
                    let part = |t: &str| t.split(section).nth(1).map(|s| s.split("\n#").next().unwrap_or_default().to_string());
                    if part(&text) != part(&original) {
                        problems.push(format!("{name}: {section} changed"));
                    }
                }
            }
        }
        // drawings stay drawings after the save, and embeds of them resolve
        v.rescan();
        for rel in &drawings {
            if v.file(rel).map(|f| f.kind) != Some(FileKind::Drawing) {
                problems.push(format!("{}: no longer a drawing after saving", rel_to_string(rel)));
            }
        }
        for rel in notes(v) {
            let n = &v.notes[&rel];
            for (l, t) in n.links.iter().zip(&n.resolved) {
                if let (true, Some(t)) = (l.embed, t) {
                    if v.file(t).map(|f| f.kind) == Some(FileKind::Drawing) && Scene::read_file(&root.join(t)).is_err() {
                        problems.push(format!("{}: embedded drawing {} can't be shown", rel_to_string(&rel), rel_to_string(t)));
                    }
                }
            }
        }
        fail_if_any("drawings", &problems);
    });
}

// spec: TASK-003, EDIT-007
#[test]
fn notes_ticking_a_task_twice_restores_the_note() {
    with_vault("tasks", |_, v| {
        let mut problems = Vec::new();
        for rel in notes(v) {
            let text = v.notes[&rel].text.clone();
            for t in &v.notes[&rel].tasks {
                let mut once = text.clone();
                if !markdown::toggle_task_at(&mut once, t.check_offset) {
                    problems.push(format!("{}:{}: task can't be ticked", rel_to_string(&rel), t.line + 1));
                    continue;
                }
                let mut twice = once.clone();
                markdown::toggle_task_at(&mut twice, t.check_offset);
                if twice != text || once.len() != text.len() {
                    problems.push(format!("{}:{}: ticking changed more than the box", rel_to_string(&rel), t.line + 1));
                }
            }
        }
        fail_if_any("tasks", &problems);
    });
}

// spec: VAULT-013
#[test]
fn notes_renaming_a_linked_note_keeps_its_links_working() {
    with_vault("rename", |_, v| {
        // the 20 most linked-to notes
        let mut targets: Vec<(usize, PathBuf)> = notes(v).into_iter().map(|r| (v.backlinks(&r).len(), r)).filter(|(n, _)| *n > 0).collect();
        targets.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        let mut problems = Vec::new();
        let mut known = Vec::new();
        for (_, rel) in targets.into_iter().take(20) {
            // every link (in every note) that points here now
            let before: Vec<(PathBuf, String)> = notes(v)
                .into_iter()
                .flat_map(|src| {
                    let n = &v.notes[&src];
                    n.links.iter().zip(&n.resolved).filter(|(_, t)| t.as_deref() == Some(rel.as_path())).map(|(l, _)| (src.clone(), l.target.clone())).collect::<Vec<_>>()
                })
                .collect();
            let stem = markdown::strip_md_ext(&rel.file_name().unwrap().to_string_lossy()).to_string();
            let to = rel.with_file_name(format!("{stem} renamed.md"));
            if let Err(e) = v.rename(&rel, &to, true) {
                problems.push(format!("{}: rename failed: {e}", rel_to_string(&rel)));
                continue;
            }
            for (src, target) in before {
                let src = if src == rel { to.clone() } else { src };
                let still = v.notes.get(&src).map(|n| n.resolved.iter().any(|t| t.as_deref() == Some(to.as_path()))).unwrap_or(false);
                if !still {
                    let line = format!("{} → [[{target}]] no longer points to {}", rel_to_string(&src), rel_to_string(&to));
                    // path links lose their folder on rename: VAULT-013 ⚠, reported only
                    if target.contains('/') {
                        known.push(line)
                    } else {
                        problems.push(line)
                    }
                }
            }
            // put it back for the next round
            let _ = v.rename(&to, &rel, true);
        }
        if !known.is_empty() {
            eprintln!("known defect VAULT-013 (path links on rename):\n  {}", known.join("\n  "));
        }
        fail_if_any("rename", &problems);
    });
}
