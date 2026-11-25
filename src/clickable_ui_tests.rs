//! A clickable element looks and acts like a button (LAY-038, rabalyn/shale#692): two guards.
//!
//! * the static one scans the sources for a label that was made clickable with a bare
//!   `Label::new(…).sense(Sense::click())`, which egui draws with the text cursor and lets the
//!   mouse select, instead of going through `clickable::clickable`;
//! * the UI-wide one runs the window headless on scenes with fixture data (each view of the
//!   sidebars, the main views, the dialogs), finds every widget that senses clicks in egui's
//!   accessibility tree, hovers it with a real pointer move and asserts the cursor (the pointing
//!   hand, or the open hand for what can be dragged too) and a changed look (hover background,
//!   outline or text colour) at its place.

use crate::app::{LeftPanel, Modal, Options, RightPanel, SettingsTab, ShaleApp, View};
use crate::ui_test_support::Headless;
use eframe::egui::{self, accesskit, Event, Pos2, Rect};
use std::path::{Path, PathBuf};

/// Where a click-sensing widget is, by design, not a button: `(label or part of it, why)`.
const NOT_A_BUTTON: &[(&str, &str)] = &[
    (crate::panels::EMPTY_LIST_SPACE, "empty space below the file list: only a right-click opens its menu, a click does nothing"),
    (crate::reader::TEXT_WITH_LINKS, "a paragraph: the links inside it are the buttons, and `follow_span` shows the hand over one"),
    ("Close window", "the close button of a dialog's title bar is egui's own and sets no cursor; replacing the title bar is rabalyn/shale#696"),
    (crate::graph::CANVAS, "the graph's drawing area: dragging pans it and a click on a node opens it, the cursor is its own"),
];

/// Label sites that stay a raw `Label::new(…).sense(Sense::click())`, with the reason.
const RAW_CLICKABLE_LABELS: &[(&str, &str)] = &[("src/social/feed_ui.rs", "a post's text: only its links, hashtags and mentions are clickable, and the pointing hand shows over those; the rest is text to select")];

struct W {
    app: ShaleApp,
    hl: Headless,
    dir: PathBuf,
}

impl Drop for W {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl W {
    fn new(tag: &str) -> W {
        let dir = std::env::temp_dir().join(format!("shale-clickable-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        crate::vault::write_sample_vault(&dir).unwrap();
        std::fs::write(dir.join("Tagged.md"), "# Tagged\n\n#alpha #beta #getting-started\n\nSee [[Welcome]] and [[Missing note]] and [[Ideas]].\n\n## A heading\n\ntext\n\n## Another heading\n").unwrap();
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
        app.cfg.cursor_blink = false;
        app.cfg.show_right = true;
        app.vault.as_mut().unwrap().rescan();
        app.open(Path::new("Tagged.md"), false);
        let mut w = W { app, hl: Headless::new(), dir };
        w.hl.ctx().enable_accesskit();
        // egui gives every selectable label a click sense (to start a text selection), which
        // would make all text look like a clickable widget; with selecting off, what still senses
        // a click is what the code made clickable
        w.hl.ctx().global_style_mut(|s| s.interaction.selectable_labels = false);
        w.hl.settle(&mut w.app);
        w
    }

    /// One frame: what it painted and the click-sensing widgets of its accessibility tree.
    fn tree(&mut self, events: Vec<Event>) -> (egui::FullOutput, Vec<(String, Rect, accesskit::Role)>) {
        let mut out = self.hl.frame(&mut self.app, events);
        let tree = out.platform_output.accesskit_update.take().expect("accessibility is on");
        let nodes = tree
            .nodes
            .iter()
            .filter(|(_, n)| n.supports_action(accesskit::Action::Click) && !n.is_disabled() && !matches!(n.role(), accesskit::Role::TextInput | accesskit::Role::MultilineTextInput | accesskit::Role::ScrollBar))
            .filter_map(|(_, n)| {
                let b = n.bounds()?;
                let r = Rect::from_min_max(Pos2::new(b.x0 as f32, b.y0 as f32), Pos2::new(b.x1 as f32, b.y1 as f32));
                Some((n.label().or_else(|| n.value()).unwrap_or("").to_string(), r, n.role()))
            })
            .collect();
        (out, nodes)
    }

    /// Hover every click-sensing widget; what is wrong with each, with its label and place.
    fn hover_all(&mut self, scene: &str, behind: &[(String, Rect)]) -> Vec<String> {
        self.hl.settle(&mut self.app);
        let screen = Rect::from_min_size(Pos2::ZERO, egui::vec2(1500.0, 900.0));
        // the layout of a page that looks something up (the git of the computer) moves until it is
        // answered: wait until two frames in a row have the same widgets (DEV-041)
        let mut last = vec![];
        let mut nodes = vec![];
        crate::ui_test_support::poll_until("a layout that stands still", || {
            nodes = self.tree(vec![]).1;
            let same = nodes == last;
            last = nodes.clone();
            same
        });
        nodes.retain(|(_, r, _)| screen.contains_rect(*r) && r.width() >= 2.0 && r.height() >= 2.0);
        nodes.sort_by(|a, b| (a.1.min.y, a.1.min.x).partial_cmp(&(b.1.min.y, b.1.min.x)).unwrap());
        nodes.dedup_by(|a, b| a.1 == b.1);
        assert!(!nodes.is_empty(), "{scene}: no widget senses clicks, the scene is not what it should be");
        let mut bad = vec![];
        // a widget that a dialog brought and that is over smaller click-sensing widgets (its title
        // bar, a backdrop) is a surface: the widgets on it are the buttons
        let all: Vec<Rect> = nodes.iter().map(|n| n.1).collect();
        nodes.retain(|(l, r, _)| behind.iter().any(|(bl, br)| bl == l && br == r) || !all.iter().any(|o| o != r && r.intersects(*o) && o.area() < r.area()));
        let modal = self.hl.ctx().memory(|m| m.top_modal_layer());
        for (label, rect, role) in nodes {
            // a widget of the window that a dialog (or any window in front of it) now covers
            let covered = self.hl.ctx().layer_id_at(rect.center()).is_some_and(|l| l.order == egui::Order::Foreground);
            if covered && behind.iter().any(|(l, r)| *l == label && *r == rect) {
                continue;
            }
            // behind a dialog the backdrop takes the pointer: only the dialog's own widgets count
            if modal.is_some_and(|m| self.hl.ctx().layer_id_at(rect.center()) == Some(m)) || (modal.is_some() && rect == screen) {
                continue;
            }
            if NOT_A_BUTTON.iter().any(|(l, _)| label == *l) {
                continue;
            }
            let at = rect.center();
            self.hl.frame(&mut self.app, vec![Event::PointerGone]);
            let before = self.hl.frame(&mut self.app, vec![]);
            // clipped away (below a scroll area's edge): nothing there to point at
            if !before.shapes.iter().any(|c| c.clip_rect.contains(at) && rect.expand(6.0).contains_rect(c.shape.visual_bounding_rect())) {
                continue;
            }
            let look_before = painted_at(&before, rect);
            self.hl.frame(&mut self.app, vec![Event::PointerMoved(at)]);
            let after = self.hl.frame(&mut self.app, vec![]);
            let cursor = self.hl.cursor;
            let look_after = painted_at(&after, rect);
            let name = format!("{scene}: {role:?} {label:?} at {:.0},{:.0} {:.0}x{:.0}", rect.min.x, rect.min.y, rect.width(), rect.height());
            // a widget with a cursor of its own (a resize grip, a crosshair) is not asked for the hand
            if matches!(cursor, egui::CursorIcon::Default | egui::CursorIcon::Text) {
                bad.push(format!("{name}: the cursor is {cursor:?}, not the pointing hand"));
            } else if matches!(cursor, egui::CursorIcon::PointingHand | egui::CursorIcon::Grab) && look_before == look_after {
                bad.push(format!("{name}: nothing changes when the mouse is over it (no hover look)"));
            }
        }
        self.hl.frame(&mut self.app, vec![Event::PointerGone]);
        bad
    }
}

/// What was painted over `rect` (and a little around it), as text to compare.
fn painted_at(out: &egui::FullOutput, rect: Rect) -> String {
    let zone = rect.expand(3.0);
    out.shapes.iter().filter(|c| c.shape.visual_bounding_rect().intersects(zone)).map(|c| format!("{:?}", c.shape)).collect::<Vec<_>>().join("\n")
}

fn check(scene: &str, setup: impl FnOnce(&mut W)) {
    let mut w = W::new(&scene.replace(' ', "-"));
    let (_, nodes) = w.tree(vec![]);
    let behind: Vec<_> = nodes.into_iter().map(|(label, rect, _)| (label, rect)).collect();
    setup(&mut w);
    let bad = w.hover_all(scene, &behind);
    assert!(bad.is_empty(), "clickable widgets that do not look like buttons (LAY-038):\n{}", bad.join("\n"));
}

#[test]
fn lay_038_the_files_view_and_the_ribbon_have_button_cursors_and_hover_looks() {
    check("files", |w| w.app.side.left = LeftPanel::Files);
}

#[test]
fn lay_038_the_tags_view_has_button_cursors_and_hover_looks() {
    check("tags", |w| w.app.side.left = LeftPanel::Tags);
}

#[test]
fn lay_038_the_search_view_has_button_cursors_and_hover_looks() {
    check("search", |w| {
        w.app.side.left = LeftPanel::Search;
        w.app.search.query = "Welcome".into();
    });
}

#[test]
fn lay_038_the_task_list_has_button_cursors_and_hover_looks() {
    check("tasks", |w| w.app.show_view(View::Tasks, false));
}

#[test]
fn lay_038_the_calendar_has_button_cursors_and_hover_looks() {
    check("calendar", |w| w.app.show_view(View::Calendar, false));
}

#[test]
fn lay_038_the_panels_of_a_note_have_button_cursors_and_hover_looks() {
    for panel in [RightPanel::Backlinks, RightPanel::Outgoing, RightPanel::Outline, RightPanel::Calendar, RightPanel::History] {
        check(&format!("{panel:?} panel"), |w| w.app.side.peek = Some(panel));
    }
}

#[test]
fn lay_038_the_main_views_have_button_cursors_and_hover_looks() {
    for (name, view) in [("graph", View::Graph), ("boards", View::Boards), ("mail", View::Mail), ("feeds", View::Social(None))] {
        check(name, |w| w.app.show_view(view, false));
    }
}

#[test]
fn lay_038_the_dialogs_have_button_cursors_and_hover_looks() {
    check("quick switcher", |w| w.app.modal = Some(Modal::QuickSwitch { query: String::new(), sel: 0 }));
    check("confirm trash", |w| w.app.modal = Some(Modal::ConfirmTrash { rel: "Ideas.md".into() }));
    check("data usage", |w| w.app.modal = Some(Modal::DataUsage));
    check("vault menu", |w| w.app.open_vault_menu());
    for tab in [
        SettingsTab::Vault,
        SettingsTab::Editor,
        SettingsTab::Vim,
        SettingsTab::Appearance,
        SettingsTab::Fonts,
        SettingsTab::Publishing,
        SettingsTab::Calendar,
        SettingsTab::Mail,
        SettingsTab::Social,
        SettingsTab::Search,
        SettingsTab::Import,
        SettingsTab::Data,
    ] {
        check(&format!("settings {tab:?}"), |w| w.app.open_settings(tab));
    }
}

/// Every `.rs` file of `src/` with its text, outside the test files.
fn sources() -> Vec<(String, String)> {
    fn walk(dir: &Path, out: &mut Vec<(String, String)>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                let name = p.file_name().unwrap().to_string_lossy().to_string();
                if !name.ends_with("_tests.rs") && name != "clickable.rs" {
                    out.push((p.strip_prefix(env!("CARGO_MANIFEST_DIR")).unwrap().to_string_lossy().replace('\\', "/"), std::fs::read_to_string(&p).unwrap()));
                }
            }
        }
    }
    let mut out = vec![];
    walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut out);
    out
}

/// The `Label::new(` expressions of a source that carry a click sense: `(line, first line)`.
fn raw_clickable_labels(src: &str) -> Vec<(usize, String)> {
    let mut found = vec![];
    let mut from = 0;
    while let Some(at) = src[from..].find("Label::new(") {
        let start = from + at;
        from = start + 1;
        // the constructor's parentheses, then the builder calls that follow, up to the end of the
        // expression (a `)` that closes `ui.add(`, a `;` or a `,`)
        let mut depth = 0i32;
        let mut end = src.len();
        let mut opened = false;
        for (i, c) in src[start..].char_indices() {
            match c {
                '(' => {
                    depth += 1;
                    opened = true;
                }
                ')' => {
                    depth -= 1;
                    if opened && depth < 0 {
                        end = start + i;
                        break;
                    }
                }
                ';' | ',' if opened && depth == 0 => {
                    end = start + i;
                    break;
                }
                _ => {}
            }
        }
        let text = &src[start..end];
        if text.contains(".sense(") && text.to_ascii_lowercase().contains("sense::click") {
            found.push((src[..start].matches('\n').count() + 1, text.lines().next().unwrap_or("").trim().to_string()));
        }
    }
    found
}

#[test]
fn lay_038_no_label_is_made_clickable_without_the_clickable_helper() {
    let mut bad = vec![];
    for (file, src) in sources() {
        for (line, text) in raw_clickable_labels(&src) {
            if !RAW_CLICKABLE_LABELS.iter().any(|(f, _)| *f == file) {
                bad.push(format!("{file}:{line}: {text}"));
            }
        }
    }
    assert!(bad.is_empty(), "a Label with a click sense must go through clickable::clickable (pointing hand, hover look, no text selection), or be listed with a reason in RAW_CLICKABLE_LABELS:\n{}", bad.join("\n"));
}

#[test]
fn lay_038_the_guard_finds_a_raw_clickable_label_and_not_a_plain_one() {
    let src = "fn a(ui: &mut Ui) {\n    ui.label(\"x\");\n    ui.add(egui::Label::new(\"y\").sense(Sense::click()));\n    ui.add(egui::Label::new(\"z\"));\n    clickable(ui, Label::new(\"w\"));\n    ui.add(Label::new(\"v\").sense(Sense::hover()));\n}\n";
    let found = raw_clickable_labels(src);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, 3);
}

#[test]
fn lay_038_every_allow_listed_site_still_has_a_raw_clickable_label() {
    let all = sources();
    for (file, why) in RAW_CLICKABLE_LABELS {
        assert!(!why.is_empty());
        let src = &all.iter().find(|(f, _)| f == file).unwrap_or_else(|| panic!("{file} is gone: drop it from the allow-list")).1;
        assert!(!raw_clickable_labels(src).is_empty(), "{file} has no raw clickable label any more: drop it from the allow-list");
    }
}

/// The Sync settings page in a state of its own (LAY-038): what the page shows depends on the
/// git of the computer, on what the look for git found (a job that ends at an unpredictable
/// moment and moves the widgets below it) and on the vault, so the test sets all of them and the
/// page has the same widgets on every machine.
#[cfg(unix)]
fn check_sync_page(scene: &str, git_missing: bool, detection: crate::gitsync::managed::Detection, managed_folder: bool, remote: Option<&str>, advanced: bool) {
    use crate::gitsync::install_ui::ManagedState;
    use crate::gitsync::managed::{managed_root, GitChoice};
    use crate::gitsync::sources::{Platform, Sources};
    check(scene, |w| {
        let data = w.dir.join("data");
        if managed_folder {
            std::fs::create_dir_all(managed_root(&data)).unwrap();
        }
        if git_missing {
            w.app.sync.git = crate::gitsync::Git { program: "/nonexistent/shale-no-git".into(), env: vec![] };
        }
        w.app.sync.forget_git_version();
        let mut managed = ManagedState::for_test(data.clone(), Some(data.join("no-bin").into_os_string()), Sources::real(), Platform::new("linux", "x86_64"));
        managed.busy = w.app.busy.clone();
        managed.set_detection(detection, GitChoice::Auto);
        w.app.sync.managed = managed;
        w.app.sync.remote = remote.map(String::from);
        w.app.sync.setup.advanced = advanced;
        w.app.open_settings(SettingsTab::Sync);
    });
}

#[cfg(unix)]
#[test]
fn lay_038_the_sync_page_has_button_cursors_and_hover_looks_in_each_of_its_states() {
    use crate::gitsync::managed::{Detection, Installed, ManagedInfo, Origin, Part};
    let found = |git: &str| Detection { system_git: Some(git.into()), system_lfs: None, managed: None, git_in_use: Some((git.into(), Origin::System)), lfs_in_use: None };
    let none = Detection { system_git: None, system_lfs: None, managed: None, git_in_use: None, lfs_in_use: None };
    let part = Part { tag: "v2.56.1".into(), version: "2.56.1".into(), program: "bin/git".into(), path: vec!["bin".into()] };
    let own = Detection { managed: Some(ManagedInfo { installed: Installed { git: Some(part), lfs: None }, bytes: 1000 }), ..found("git version 2.47.0") };
    check_sync_page("sync page without git", true, none, false, None, false);
    check_sync_page("sync page with git, no remote", false, found("git version 2.47.0"), false, None, false);
    check_sync_page("sync page with git, advanced", false, found("git version 2.47.0"), false, None, true);
    check_sync_page("sync page with a remote", false, found("git version 2.47.0"), false, Some("https://example.org/notes.git"), true);
    check_sync_page("sync page with Shale's own git", false, own, true, None, false);
}
