//! The dialogs, the task list and the file tree driven with real pointer and key events.
//! Each test opens one of them, changes a value through the keyboard or a click and looks at what
//! the app did: the actions it queued, the files on disk, the settings.
//! (A child module of `canvas_dialog_tests`, which has the frame helpers.)

use super::*;
use crate::app::Options;
use eframe::egui::{Event, PointerButton, RawInput};
use egui::accesskit::Role;

pub(super) struct P {
    ctx: egui::Context,
    t: f64,
    pub(super) app: ShaleApp,
    pub(super) dir: PathBuf,
    /// the text painted in the last frame, and where
    pub(super) painted: Vec<(String, Pos2)>,
    /// the window size (tall, so that a dialog shows whole; a test of a short window sets it)
    pub(super) size: Vec2,
}

fn collect_text(shape: &egui::Shape, out: &mut Vec<(String, Pos2)>) {
    match shape {
        egui::Shape::Text(t) => out.push((t.galley.text().to_string(), t.pos)),
        egui::Shape::Vec(v) => v.iter().for_each(|s| collect_text(s, out)),
        _ => {}
    }
}

/// What is on screen: role, label and place of every widget.
pub(super) type Seen = Vec<(Role, String, Rect)>;

#[derive(Clone, Copy)]
pub(super) enum Show {
    Modals,
    Tasks,
    Files,
}

impl P {
    /// The sample vault plus these files.
    pub(super) fn new(tag: &str, extra: &[(&str, &str)]) -> P {
        Self::build(tag, extra, true)
    }

    /// A vault with only these files.
    pub(super) fn with_files(tag: &str, files: &[(&str, &str)]) -> P {
        Self::build(tag, files, false)
    }

    pub(super) fn build(tag: &str, extra: &[(&str, &str)], sample: bool) -> P {
        let dir = std::env::temp_dir().join(format!("shale-panels-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        if sample {
            crate::vault::write_sample_vault(&dir).unwrap();
        }
        for (p, text) in extra {
            let f = dir.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, text).unwrap();
        }
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
        app.vault.as_mut().unwrap().rescan();
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        crate::theme::apply(&ctx);
        P { ctx, t: 0.0, app, dir, painted: vec![], size: Vec2::new(900.0, 2600.0) }
    }

    /// The window rect of the dialog that was made with `Dialog::new(id, …)`.
    pub(super) fn dialog_area(&self, id: &str) -> Rect {
        self.ctx.memory(|m| m.area_rect(egui::Id::unique(("dialog", id)))).unwrap_or_else(|| panic!("no dialog {id:?} shown"))
    }

    /// The place of a row of the file tree, found by the name painted on it.
    pub(super) fn row(&mut self, name: &str) -> Rect {
        self.look(Show::Files);
        self.painted_rect(name)
    }

    /// A small place on the text painted as `text` in the last frame.
    pub(super) fn painted_rect(&self, text: &str) -> Rect {
        let at = self.painted.iter().find(|(t, _)| t.contains(text)).unwrap_or_else(|| panic!("no {text:?} painted: {:?}", self.painted.iter().map(|x| &x.0).collect::<Vec<_>>())).1;
        Rect::from_center_size(at + Vec2::new(10.0, 6.0), Vec2::new(8.0, 4.0))
    }

    /// Whether this text is on screen once the layout is still, painted or as a widget's label.
    pub(super) fn shows(&mut self, show: Show, text: &str) -> bool {
        let seen = self.look(show);
        seen.iter().any(|(_, l, _)| l.contains(text)) || self.painted.iter().any(|(t, _)| t.contains(text))
    }

    pub(super) fn frame(&mut self, show: Show, events: Vec<Event>) -> Seen {
        self.t += 0.05;
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)), time: Some(self.t), events, ..Default::default() };
        let app = &mut self.app;
        let mut out = self.ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default().show(ui, |ui| match show {
                Show::Modals => app.modals(ui),
                Show::Tasks => app.tasks_view(ui),
                Show::Files => app.file_explorer(ui),
            });
        });
        out.textures_delta.clear();
        self.painted.clear();
        out.shapes.iter().for_each(|c| collect_text(&c.shape, &mut self.painted));
        let tree = out.platform_output.accesskit_update.expect("accessibility is on");
        tree.nodes
            .iter()
            .filter_map(|(_, n)| {
                let b = n.bounds()?;
                Some((n.role(), n.label().map(str::to_string).or_else(|| n.value().map(str::to_string)).unwrap_or_default(), Rect::from_min_max(Pos2::new(b.x0 as f32, b.y0 as f32), Pos2::new(b.x1 as f32, b.y1 as f32))))
            })
            .collect()
    }

    /// Frames until the layout stands still, then what is on screen.
    pub(super) fn look(&mut self, show: Show) -> Seen {
        for _ in 0..5 {
            self.frame(show, vec![]);
        }
        self.frame(show, vec![])
    }

    pub(super) fn find(&mut self, show: Show, role: Option<Role>, label: &str) -> Rect {
        let seen = self.look(show);
        // buttons first (a dialog's title can have the name of its button), text boxes only when asked for
        let boxes = |r: &Role| matches!(r, Role::TextInput | Role::MultilineTextInput);
        let by = |r: &Role| role.map(|x| x == *r).unwrap_or(!boxes(r));
        seen.iter()
            .find(|(r, l, _)| l == label && *r == Role::Button && by(r))
            .or_else(|| seen.iter().find(|(r, l, _)| l == label && by(r)))
            .unwrap_or_else(|| panic!("no {label:?} on screen: {:?}", seen.iter().map(|x| &x.1).collect::<Vec<_>>()))
            .2
    }

    pub(super) fn click_rect(&mut self, show: Show, r: Rect, b: PointerButton) {
        let p = r.center();
        let ev = |pressed| Event::PointerButton { pos: p, button: b, pressed, modifiers: Modifiers::NONE };
        self.frame(show, vec![Event::PointerMoved(p)]);
        self.frame(show, vec![ev(true)]);
        self.frame(show, vec![ev(false)]);
        self.frame(show, vec![]);
    }

    pub(super) fn click(&mut self, show: Show, label: &str) {
        let r = self.find(show, None, label);
        self.click_rect(show, r, PointerButton::Primary);
    }

    pub(super) fn key(&mut self, show: Show, key: Key) {
        let ev = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
        self.frame(show, vec![ev(true)]);
        self.frame(show, vec![ev(false)]);
        self.frame(show, vec![]);
    }

    pub(super) fn type_text(&mut self, show: Show, text: &str) {
        self.frame(show, vec![Event::Text(text.into())]);
        self.frame(show, vec![]);
    }

    /// Select what the focused text box holds, so the next typing replaces it.
    pub(super) fn select_all(&mut self, show: Show) {
        let ev = |pressed| Event::Key { key: Key::A, physical_key: None, pressed, repeat: false, modifiers: Modifiers::COMMAND };
        self.frame(show, vec![ev(true)]);
        self.frame(show, vec![ev(false)]);
    }

    pub(super) fn actions(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.app.actions)
    }
}

impl Drop for P {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub(super) fn opened(a: &[Action]) -> Vec<String> {
    a.iter()
        .filter_map(|a| match a {
            Action::Open { rel, .. } => Some(rel_to_string(rel)),
            _ => None,
        })
        .collect()
}

// spec: VAULT-023
#[test]
fn panel_quick_switcher_finds_opens_creates_and_closes() {
    let mut p = P::new("quick", &[]);
    p.app.modal = Some(Modal::QuickSwitch { query: String::new(), sel: 0 });
    p.look(Show::Modals);
    p.type_text(Show::Modals, "idea");
    let shown = p.look(Show::Modals);
    assert!(shown.iter().any(|(_, l, _)| l.starts_with("Ideas")), "the note is listed: {:?}", shown.iter().map(|x| &x.1).collect::<Vec<_>>());
    assert!(shown.iter().any(|(_, l, _)| l.contains("Create “idea”")), "a name that is no note offers to create it");
    p.key(Show::Modals, Key::Enter);
    assert_eq!(opened(&p.actions()), ["Ideas.md"], "Enter opens the best match");
    assert!(p.app.modal.is_none());
    // the arrow keys move the choice and a click picks
    p.app.modal = Some(Modal::QuickSwitch { query: String::new(), sel: 0 });
    p.look(Show::Modals);
    p.key(Show::Modals, Key::ArrowDown);
    let Some(Modal::QuickSwitch { sel, .. }) = &p.app.modal else { panic!("still open") };
    assert_eq!(*sel, 1);
    p.type_text(Show::Modals, "Welcome");
    p.click(Show::Modals, "Welcome");
    assert_eq!(opened(&p.actions()), ["Welcome.md"]);
    assert!(p.app.modal.is_none());
    // a new name: Enter on the last entry follows it as a link (which creates the note)
    p.app.modal = Some(Modal::QuickSwitch { query: "Brand new".into(), sel: 0 });
    p.look(Show::Modals);
    p.click(Show::Modals, "＋ Create “Brand new”");
    assert!(matches!(p.actions().as_slice(), [Action::OpenLink { target, .. }] if target == "Brand new"));
    // Esc closes without doing anything
    p.app.modal = Some(Modal::QuickSwitch { query: "x".into(), sel: 0 });
    p.look(Show::Modals);
    p.key(Show::Modals, Key::Escape);
    assert!(p.app.modal.is_none() && p.app.actions.is_empty());
}

// spec: VAULT-023
#[test]
fn panel_rename_dialog_renames_shows_errors_and_cancels() {
    let mut p = P::new("rename", &[]);
    p.app.modal = Some(Modal::Rename { rel: PathBuf::from("Ideas.md"), name: "Ideas".into(), error: None });
    let shown = p.look(Show::Modals);
    assert!(shown.iter().any(|(_, l, _)| l.contains("Links in other notes will be updated")), "the hint shows with the setting on");
    // a name that is taken: the dialog stays and says why
    p.select_all(Show::Modals);
    p.type_text(Show::Modals, "Welcome");
    p.click(Show::Modals, "Rename");
    let Some(Modal::Rename { error, .. }) = &p.app.modal else { panic!("the dialog stays open") };
    assert!(error.is_some(), "an error is shown");
    assert!(p.dir.join("Ideas.md").exists());
    // a free name: Enter renames and closes
    p.select_all(Show::Modals);
    p.type_text(Show::Modals, "Thoughts");
    p.key(Show::Modals, Key::Enter);
    assert!(p.app.modal.is_none());
    assert!(p.dir.join("Thoughts.md").exists() && !p.dir.join("Ideas.md").exists());
    // Cancel changes nothing
    p.app.modal = Some(Modal::Rename { rel: PathBuf::from("Thoughts.md"), name: "Other".into(), error: None });
    p.click(Show::Modals, "Cancel");
    assert!(p.app.modal.is_none() && p.dir.join("Thoughts.md").exists());
    // with the setting off, no hint
    p.app.cfg.update_links_on_rename = false;
    p.app.modal = Some(Modal::Rename { rel: PathBuf::from("Thoughts.md"), name: "Thoughts".into(), error: None });
    let shown = p.look(Show::Modals);
    assert!(!shown.iter().any(|(_, l, _)| l.contains("Links in other notes")));
    p.key(Show::Modals, Key::Escape);
    assert!(p.app.modal.is_none());
}

// spec: VAULT-023
#[test]
fn panel_new_folder_dialog_creates_opens_the_path_and_refuses_nothing_silently() {
    let mut p = P::new("newfolder", &[]);
    p.app.modal = Some(Modal::NewFolder { parent: PathBuf::new(), name: String::new() });
    p.look(Show::Modals);
    // an empty name does nothing
    p.click(Show::Modals, "Create");
    assert!(p.app.modal.is_some());
    p.type_text(Show::Modals, "Archive");
    p.click(Show::Modals, "Create");
    assert!(p.app.modal.is_none());
    assert!(p.dir.join("Archive").is_dir());
    assert!(p.app.tree.expanded.contains(Path::new("Archive")), "the new folder is open in the tree");
    // Enter creates too, inside a folder
    p.app.modal = Some(Modal::NewFolder { parent: PathBuf::from("Archive"), name: String::new() });
    p.look(Show::Modals);
    p.type_text(Show::Modals, "2026");
    p.key(Show::Modals, Key::Enter);
    assert!(p.dir.join("Archive/2026").is_dir());
    // a name that cannot be made: the dialog closes with a message
    p.app.modal = Some(Modal::NewFolder { parent: PathBuf::new(), name: String::new() });
    p.look(Show::Modals);
    p.type_text(Show::Modals, "..");
    p.click(Show::Modals, "Create");
    assert!(p.app.modal.is_none());
    assert!(p.app.toasts.iter().any(|(m, _)| m.starts_with("Could not create folder")), "{:?}", p.app.toasts.iter().map(|t| &t.0).collect::<Vec<_>>());
    // Cancel
    p.app.modal = Some(Modal::NewFolder { parent: PathBuf::new(), name: "Nope".into() });
    p.click(Show::Modals, "Cancel");
    assert!(p.app.modal.is_none() && !p.dir.join("Nope").exists());
}

// spec: VAULT-023
#[test]
fn panel_trash_dialog_moves_to_trash_or_cancels() {
    let mut p = P::new("trash", &[]);
    p.app.modal = Some(Modal::ConfirmTrash { rel: PathBuf::from("Ideas.md") });
    let shown = p.look(Show::Modals);
    assert!(shown.iter().any(|(_, l, _)| l.contains("“Ideas.md” will be moved to the .trash folder")));
    p.click(Show::Modals, "Cancel");
    assert!(p.app.modal.is_none() && p.dir.join("Ideas.md").exists());
    p.app.modal = Some(Modal::ConfirmTrash { rel: PathBuf::from("Ideas.md") });
    p.click(Show::Modals, "Move to trash");
    assert!(p.app.modal.is_none());
    assert!(!p.dir.join("Ideas.md").exists() && p.dir.join(".trash").is_dir(), "the note is in the vault's trash folder");
    // Esc closes the question as well
    p.app.modal = Some(Modal::ConfirmTrash { rel: PathBuf::from("Welcome.md") });
    p.look(Show::Modals);
    p.key(Show::Modals, Key::Escape);
    assert!(p.app.modal.is_none() && p.dir.join("Welcome.md").exists());
}

// spec: VAULT-023
#[test]
fn panel_settings_dialog_changes_settings_and_closes() {
    let mut p = P::new("settings", &[]);
    p.app.open_settings(SettingsTab::General);
    let shown = p.look(Show::Modals);
    for want in ["Vault folder", "Links", "Appearance", "Done", "Calendars…"] {
        assert!(shown.iter().any(|(_, l, _)| l == want), "{want:?} missing");
    }
    // check boxes change the setting
    let (ctrl, dark) = (p.app.cfg.ctrl_to_follow, p.app.cfg.dark_canvas);
    p.click(Show::Modals, "Follow links only with Ctrl+click (plain click places the cursor)");
    assert_eq!(p.app.cfg.ctrl_to_follow, !ctrl);
    p.click(Show::Modals, "Dark drawing canvas");
    assert_eq!(p.app.cfg.dark_canvas, !dark);
    p.click(Show::Modals, "Update links in other notes when renaming");
    assert!(!p.app.cfg.update_links_on_rename);
    // opening a vault that is not there is refused with a message, the dialog stays
    if let Some(Modal::Settings { vault_path, .. }) = &mut p.app.modal {
        *vault_path = "/this/folder/does/not/exist".into();
    }
    p.click(Show::Modals, "Open");
    assert!(p.app.modal.is_some());
    assert!(p.app.toasts.iter().any(|(m, _)| m.contains("doesn't exist")));
    // a folder that is there is opened
    if let Some(Modal::Settings { vault_path, .. }) = &mut p.app.modal {
        *vault_path = p.dir.to_string_lossy().into_owned();
    }
    p.click(Show::Modals, "Open");
    assert!(p.app.modal.is_none());
    assert!(matches!(p.actions().as_slice(), [Action::OpenVault(d)] if *d == p.dir));
    // the Keybindings tab, and Done from it
    p.app.open_settings(SettingsTab::General);
    p.click(Show::Modals, "Keybindings");
    assert!(matches!(&p.app.modal, Some(Modal::Settings { tab: SettingsTab::Keybindings, .. })));
    p.key(Show::Modals, Key::Escape);
    assert!(p.app.modal.is_none());
    // Calendars… leaves the settings for the calendar dialog
    p.app.open_settings(SettingsTab::General);
    p.click(Show::Modals, "Calendars…");
    assert!(matches!(&p.app.modal, Some(Modal::Calendars(_))));
    p.key(Show::Modals, Key::Escape);
    assert!(p.app.modal.is_none());
    // Done and Esc close the settings
    p.app.open_settings(SettingsTab::General);
    p.click(Show::Modals, "Done");
    assert!(p.app.modal.is_none());
    p.app.open_settings(SettingsTab::General);
    p.look(Show::Modals);
    p.key(Show::Modals, Key::Escape);
    assert!(p.app.modal.is_none());
    // closing the vault goes back to the start screen
    p.app.open_settings(SettingsTab::General);
    p.click(Show::Modals, "Close this vault (back to the start screen)");
    assert!(p.app.vault.is_none() && p.app.modal.is_none() && p.app.cfg.vault.is_none());
}

const TASKS: &str = "# Plan\n\n- [ ] write the report 📅 2026-02-01\n- [x] book the room\n- [ ] call #urgent\n  - [ ] nested step\n";

// spec: TASK-005
#[test]
fn panel_task_list_filters_ticks_and_opens() {
    let mut p = P::with_files("tasks", &[("Plan.md", TASKS)]);
    assert!(p.shows(Show::Tasks, "3 open · 1 done"), "counts");
    assert!(p.shows(Show::Tasks, "write the report") && !p.shows(Show::Tasks, "book the room"), "done tasks are hidden at first");
    // Show done
    p.click(Show::Tasks, "Show done");
    assert!(p.app.tasks.show_done && p.shows(Show::Tasks, "book the room"));
    // the filter box: text or tag
    let r = p.find(Show::Tasks, Some(Role::TextInput), "");
    p.click_rect(Show::Tasks, r, PointerButton::Primary);
    p.type_text(Show::Tasks, "#urgent");
    assert!(p.shows(Show::Tasks, "call") && !p.shows(Show::Tasks, "write the report"));
    p.app.tasks.query = "zzz".into();
    assert!(p.shows(Show::Tasks, "Nothing matches"));
    p.app.tasks.query.clear();
    // only tasks with a due date
    p.click(Show::Tasks, "Only with due date");
    assert!(p.app.tasks.only_due && p.shows(Show::Tasks, "due 2026-02-01") && !p.shows(Show::Tasks, "call"));
    p.click(Show::Tasks, "Only with due date");
    // clicking the note's name opens it
    p.click(Show::Tasks, "Plan");
    assert_eq!(opened(&p.actions()), ["Plan.md"]);
    // the check box queues the tick
    let boxes: Vec<Rect> = p.look(Show::Tasks).iter().filter(|(r, l, _)| *r == Role::CheckBox && l.starts_with("Tick off")).map(|x| x.2).collect();
    assert!(!boxes.is_empty(), "a check box per task");
    p.click_rect(Show::Tasks, *boxes.last().unwrap(), PointerButton::Primary);
    assert!(p.actions().iter().any(|a| matches!(a, Action::ToggleTask { rel, .. } if rel == Path::new("Plan.md"))));
    // a click on the text goes to the line
    let r = p.painted_rect("nested step");
    p.click_rect(Show::Tasks, r, PointerButton::Primary);
    assert!(p.actions().iter().any(|a| matches!(a, Action::GotoLine { rel, line } if rel == Path::new("Plan.md") && *line == 5)));
}

// spec: TASK-005
#[test]
fn panel_task_list_says_so_when_there_are_none() {
    let mut p = P::with_files("notasks", &[("Empty.md", "nothing to do\n")]);
    assert!(p.shows(Show::Tasks, "No tasks yet"));
    assert!(p.shows(Show::Tasks, "0 open · 0 done"));
}

// spec: VAULT-024
#[test]
fn panel_file_tree_opens_folds_and_offers_menus() {
    let mut p = P::new("tree", &[("Box/Inner.md", "inner\n")]);
    p.app.tree.expanded.clear();
    // a folder opens and closes by a click
    let r = p.row("Box");
    p.click_rect(Show::Files, r, PointerButton::Primary);
    assert!(p.app.tree.expanded.contains(Path::new("Box")));
    p.look(Show::Files);
    assert!(p.painted.iter().any(|(t, _)| t == "Inner"), "its files are listed");
    let r = p.row("Box");
    p.click_rect(Show::Files, r, PointerButton::Primary);
    assert!(!p.app.tree.expanded.contains(Path::new("Box")));
    // a click on a note opens it
    let r = p.row("Ideas");
    p.click_rect(Show::Files, r, PointerButton::Primary);
    assert_eq!(opened(&p.actions()), ["Ideas.md"]);
    // the buttons above the tree
    for (tip, what) in [("New note", 0), ("New folder", 1), ("New drawing", 2)] {
        let r = p.find(Show::Files, None, tip);
        p.click_rect(Show::Files, r, PointerButton::Primary);
        let a = p.actions();
        assert!(
            match what {
                0 => matches!(a.as_slice(), [Action::NewNote { .. }]),
                1 => matches!(a.as_slice(), [Action::NewFolder(_)]),
                _ => matches!(a.as_slice(), [Action::NewDrawing { .. }]),
            },
            "{tip}: {} actions",
            a.len()
        );
    }
    p.app.tree.expanded.insert(PathBuf::from("Box"));
    let r = p.find(Show::Files, None, "Collapse all folders");
    p.click_rect(Show::Files, r, PointerButton::Primary);
    assert!(p.app.tree.expanded.is_empty());
    // the menu of a note
    let r = p.row("Ideas");
    p.click_rect(Show::Files, r, PointerButton::Secondary);
    p.actions();
    p.click(Show::Files, "Open in new tab");
    assert!(matches!(p.actions().as_slice(), [Action::Open { rel, new_tab: true }] if rel == Path::new("Ideas.md")));
    for (item, want) in [("Export to PDF…", 0), ("Rename…", 1), ("Move to trash", 2)] {
        let r = p.row("Ideas");
        p.click_rect(Show::Files, r, PointerButton::Secondary);
        p.actions();
        p.click(Show::Files, item);
        let a = p.actions();
        let ok = match want {
            0 => matches!(a.as_slice(), [Action::Export(r)] if r == Path::new("Ideas.md")),
            1 => matches!(a.as_slice(), [Action::Rename(r)] if r == Path::new("Ideas.md")),
            _ => matches!(a.as_slice(), [Action::Trash(r)] if r == Path::new("Ideas.md")),
        };
        assert!(ok, "{item}: {} actions", a.len());
    }
    // the menu of a folder
    for (item, want) in [("New note here", 0), ("New drawing here", 1), ("New folder here", 2), ("Rename…", 3), ("Move to trash", 4)] {
        let r = p.row("Box");
        p.click_rect(Show::Files, r, PointerButton::Secondary);
        p.actions();
        p.click(Show::Files, item);
        let a = p.actions();
        let ok = match want {
            0 => matches!(a.as_slice(), [Action::NewNote { folder }] if folder == Path::new("Box")),
            1 => matches!(a.as_slice(), [Action::NewDrawing { folder, .. }] if folder == Path::new("Box")),
            2 => matches!(a.as_slice(), [Action::NewFolder(f)] if f == Path::new("Box")),
            3 => matches!(a.as_slice(), [Action::Rename(f)] if f == Path::new("Box")),
            _ => matches!(a.as_slice(), [Action::Trash(f)] if f == Path::new("Box")),
        };
        assert!(ok, "{item}: {} actions", a.len());
    }
}
