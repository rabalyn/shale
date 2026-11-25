//! Tests of the Unlinked notes view (NAV-072, NAV-073): the lists of the two modes, the order, the
//! click on an entry, the empty state and the list kept per state of the vault.

use super::{without_backlinks, Mode, Sort, Unlinked};
use crate::app::{Options, ShaleApp, View};
use eframe::egui::{self, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

struct W {
    app: ShaleApp,
    ctx: egui::Context,
    t: f64,
    dir: PathBuf,
    texts: Vec<(String, Pos2)>,
}

fn secs_of(date: &str) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(crate::note_props::parse_timestamp(date).unwrap() as u64)
}

fn drawing_json() -> String {
    crate::drawing::Scene::parse(crate::drawing::SAMPLE_SCENE).unwrap().to_json()
}

impl W {
    /// A vault of these files (a folder is made when the name has one), each with the time of its last change when given.
    fn new(tag: &str, files: &[(&str, &str, Option<&str>)]) -> W {
        let dir = std::env::temp_dir().join(format!("shale-unlinkedui-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (name, text, time) in files {
            let f = dir.join(name);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(&f, text).unwrap();
            if let Some(t) = time {
                std::fs::File::options().write(true).open(&f).unwrap().set_modified(secs_of(t)).unwrap();
            }
        }
        let app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
        let mut w = W { app, ctx: egui::Context::default(), t: 0.0, dir, texts: vec![] };
        crate::theme::apply(&w.ctx);
        // no file tree beside the list, so that the only names painted are the rows
        w.app.cfg.show_left = false;
        w.app.cfg.show_right = false;
        w.app.show_view(View::Unlinked, false);
        w.frame(vec![]);
        w.frame(vec![]);
        w
    }

    fn frame(&mut self, events: Vec<Event>) {
        self.t += 0.05;
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 800.0))), time: Some(self.t), events, ..Default::default() };
        let app = &mut self.app;
        let mut out = self.ctx.run_ui(raw, |ui| app.run_frame(ui));
        out.textures_delta.clear();
        fn walk(s: &egui::Shape, out: &mut Vec<(String, Pos2)>) {
            match s {
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                egui::Shape::Text(t) => out.push((t.galley.job.text.clone(), t.pos)),
                _ => {}
            }
        }
        self.texts.clear();
        for c in &out.shapes {
            walk(&c.shape, &mut self.texts);
        }
    }

    fn shows(&self, needle: &str) -> bool {
        self.texts.iter().any(|(t, _)| t.contains(needle))
    }

    fn click_at(&mut self, p: Pos2) {
        let b = |pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        self.frame(vec![Event::PointerMoved(p)]);
        self.frame(vec![b(true)]);
        self.frame(vec![b(false)]);
        self.frame(vec![]);
    }

    fn click_text(&mut self, text: &str) {
        let p = self.texts.iter().find(|(t, _)| t == text).unwrap_or_else(|| panic!("no {text:?} in {:?}", self.texts)).1;
        self.click_at(p + Vec2::new(4.0, 4.0));
    }

    fn click(&mut self, id: &str) {
        let r = self.app.unlinked.rects.iter().find(|r| r.0 == id).unwrap_or_else(|| panic!("no control {id}")).1;
        self.click_at(r.center());
    }

    /// The names of the rows, top to bottom (the note names are painted as they are, nothing else is a bare name).
    fn names(&self, all: &[&str]) -> Vec<String> {
        let mut rows: Vec<&(String, Pos2)> = self.texts.iter().filter(|(t, _)| all.contains(&t.as_str())).collect();
        rows.sort_by(|a, b| a.1.y.total_cmp(&b.1.y));
        rows.into_iter().map(|r| r.0.clone()).collect()
    }
}

impl Drop for W {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn linked_vault(tag: &str) -> W {
    let scene = crate::drawing::Scene::parse(&crate::drawing::SAMPLE_SCENE.replace("\"Idea\"", "\"see [[Leaf]]\"")).unwrap().to_json();
    W::new(
        tag,
        &[
            ("Hub.md", "to [[Leaf]]\n", Some("2021-01-01T00:00:00")),
            ("Leaf.md", "leaf\n", Some("2021-01-02T00:00:00")),
            ("Alone.md", "nothing\n", Some("2021-01-03T00:00:00")),
            ("Myself.md", "see [[Myself]]\n", Some("2021-01-04T00:00:00")),
            ("Sketch.excalidraw", &drawing_json(), Some("2021-01-05T00:00:00")),
            ("Linking.excalidraw", &scene, Some("2021-01-06T00:00:00")),
            ("pic.png", "x", None),
        ],
    )
}

// spec: NAV-072
#[test]
fn nav_072_the_view_lists_the_orphans_and_the_notes_without_backlinks_one_group_at_a_time() {
    let mut w = linked_vault("modes");
    let names = ["Hub", "Leaf", "Alone", "Myself", "Sketch", "Linking", "pic.png"];
    assert!(w.shows("Unlinked notes") && w.shows("Orphans") && w.shows("No backlinks"));
    assert_eq!(w.app.unlinked.mode, Mode::Orphan, "orphans first");
    // newest change first: the drawing, the note that links to itself, the note without any link (Hub, Leaf and Linking have a link)
    assert_eq!(w.names(&names), ["Sketch", "Myself", "Alone"], "a link to itself is no link, a drawing counts, an image is no entry");
    w.click("mode:no-backlinks");
    w.frame(vec![]);
    assert_eq!(w.app.unlinked.mode, Mode::NoBacklinks);
    assert_eq!(w.names(&names), ["Linking", "Sketch", "Myself", "Alone", "Hub"], "nothing links to them; Leaf is linked, one group is shown at a time");
    assert!(w.shows("drawing"), "a drawing says so");
    w.click("mode:orphan");
    w.frame(vec![]);
    assert_eq!(w.names(&names), ["Sketch", "Myself", "Alone"]);
}

// spec: NAV-072
#[test]
fn nav_072_a_click_on_an_entry_opens_the_note_or_the_drawing() {
    let mut w = linked_vault("open");
    w.click_text("Alone");
    assert_eq!(w.app.tabbar.tabs[w.app.tabbar.active].view, View::Note(PathBuf::from("Alone.md")));
    w.app.show_view(View::Unlinked, false);
    w.frame(vec![]);
    w.frame(vec![]);
    w.click_text("Sketch");
    assert_eq!(w.app.tabbar.tabs[w.app.tabbar.active].view, View::Drawing(PathBuf::from("Sketch.excalidraw")));
}

// spec: NAV-072
#[test]
fn nav_072_a_vault_without_unlinked_files_says_so_in_each_mode() {
    let mut w = W::new("empty", &[("A.md", "[[B]]\n", None), ("B.md", "[[A]]\n", None)]);
    assert!(w.shows("No orphans") && w.shows("0 files"), "{:?}", w.texts);
    w.click("mode:no-backlinks");
    w.frame(vec![]);
    assert!(w.shows("Every note and drawing is linked from another one"));
    assert!(!w.shows("No orphans"));
}

// spec: NAV-072
#[test]
fn nav_072_the_list_is_made_once_per_state_of_the_vault_and_draws_only_the_rows_in_view() {
    let files: Vec<(String, String)> = (0..300).map(|i| (format!("Note {i:03}.md"), "x\n".to_string())).collect();
    let refs: Vec<(&str, &str, Option<&str>)> = files.iter().map(|(n, t)| (n.as_str(), t.as_str(), None)).collect();
    let mut w = W::new("once", &refs);
    for _ in 0..5 {
        w.frame(vec![]);
    }
    assert_eq!(w.app.unlinked.reads, 1, "the vault is read once for all those frames");
    assert!(w.app.unlinked.rows.drawn() < 60, "only the rows in view are drawn, not {}", w.app.unlinked.rows.drawn());
    assert!(w.shows("300 files"));
    // another mode or sort is made from the same read; a change of the vault is read again
    w.click("mode:no-backlinks");
    w.app.unlinked.sort = Sort::NameDesc;
    w.frame(vec![]);
    assert_eq!(w.app.unlinked.reads, 1);
    std::fs::write(w.dir.join("Extra.md"), "x\n").unwrap();
    w.app.vault.as_mut().unwrap().rescan();
    w.frame(vec![]);
    assert_eq!(w.app.unlinked.reads, 2);
    assert!(w.shows("301 files"));
}

// spec: NAV-073
#[test]
fn nav_073_date_changed_is_modified_in_the_front_matter_else_the_file_time_and_the_default_is_newest_first() {
    let w = W::new(
        "dates",
        &[
            ("Old.md", "---\nmodified: 2020-01-01T00:00:00\n---\nbody\n", Some("2030-01-01T00:00:00")),
            ("Mid.md", "no front matter\n", Some("2022-06-01T00:00:00")),
            ("New.md", "---\nmodified: 2025-03-01T00:00:00\n---\nbody\n", Some("2000-01-01T00:00:00")),
            ("Draw.excalidraw", &drawing_json(), Some("2023-01-01T00:00:00")),
        ],
    );
    let entries = without_backlinks(w.app.vault.as_ref().unwrap());
    let changed = |n: &str| entries.iter().find(|e| e.name == n).unwrap().changed.unwrap();
    assert_eq!(changed("Old"), crate::note_props::parse_timestamp("2020-01-01T00:00:00").unwrap(), "modified: wins over the file time, which a sync would move");
    assert_eq!(changed("Mid"), crate::note_props::parse_timestamp("2022-06-01T00:00:00").unwrap(), "no modified: the file time");
    assert_eq!(changed("Draw"), crate::note_props::parse_timestamp("2023-01-01T00:00:00").unwrap(), "a drawing: the file time");
    assert_eq!(w.app.unlinked.sort, Sort::ChangedDesc);
    let names = ["Old", "Mid", "New", "Draw"];
    assert_eq!(w.names(&names), ["New", "Draw", "Mid", "Old"]);
    assert!(w.shows("2025-03-01") && w.shows("2020-01-01"));
}

// spec: NAV-073
#[test]
fn nav_073_the_sort_menu_orders_by_name_and_by_date_both_ways() {
    let mut w = W::new("sorts", &[("beta.md", "x\n", Some("2021-01-01T00:00:00")), ("Alpha.md", "x\n", Some("2022-01-01T00:00:00")), ("Gamma.md", "x\n", Some("2020-01-01T00:00:00"))]);
    let names = ["beta", "Alpha", "Gamma"];
    assert_eq!(w.names(&names), ["Alpha", "beta", "Gamma"]);
    for (label, sort, want) in [
        ("Date changed (old to new)", Sort::ChangedAsc, ["Gamma", "beta", "Alpha"]),
        ("Name (A to Z)", Sort::NameAsc, ["Alpha", "beta", "Gamma"]),
        ("Name (Z to A)", Sort::NameDesc, ["Gamma", "beta", "Alpha"]),
        ("Date changed (new to old)", Sort::ChangedDesc, ["Alpha", "beta", "Gamma"]),
    ] {
        w.click("sort");
        w.frame(vec![]);
        w.click_text(label);
        w.frame(vec![]);
        assert_eq!(w.app.unlinked.sort, sort, "{label}");
        assert_eq!(w.names(&names), want, "{label}");
    }
}

// spec: NAV-073
#[test]
fn nav_073_a_file_without_a_date_comes_last_in_both_directions_and_names_ignore_case() {
    let e = |name: &str, changed: Option<i64>| super::Entry { rel: PathBuf::from(format!("{name}.md")), name: name.into(), drawing: false, orphan: true, changed };
    let mut list = [e("b", Some(1)), e("none", None), e("A", Some(2))];
    list.sort_by(|a, b| super::order(Sort::ChangedDesc, a, b));
    assert_eq!(list.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["A", "b", "none"]);
    list.sort_by(|a, b| super::order(Sort::ChangedAsc, a, b));
    assert_eq!(list.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["b", "A", "none"]);
    list.sort_by(|a, b| super::order(Sort::NameAsc, a, b));
    assert_eq!(list.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["A", "b", "none"]);
}

// spec: NAV-074
#[test]
fn nav_074_the_saved_names_of_the_switch_and_the_sort_round_trip_and_an_unknown_name_is_the_default() {
    // the names of the switch and the sort that are saved
    for m in Mode::ALL {
        assert_eq!(Mode::from_name(m.name()), Some(m));
    }
    for s in Sort::ALL {
        assert_eq!(Sort::from_name(s.name()), Some(s));
    }
    let r = Unlinked::restored("no-backlinks", "bogus");
    assert_eq!((r.mode, r.sort), (Mode::NoBacklinks, Sort::ChangedDesc), "an unknown name is the default");
}
