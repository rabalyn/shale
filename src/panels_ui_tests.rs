//! The dialogs, the task list and the file tree driven with real pointer and key events.
//! Each test opens one of them, changes a value through the keyboard or a click and looks at what
//! the app did: the actions it queued, the files on disk, the settings.
//! (A child module of `canvas_dialog_tests`, which has the frame helpers.)

use super::*;
use crate::app::{Options, SettingsTab};
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

// spec: NET-014
#[test]
fn net_014_the_quick_switcher_finds_a_note_typed_in_the_other_normal_form_and_offers_no_duplicate() {
    let mut p = P::with_files("quick-nfc", &[("Cafe\u{301}.md", "")]);
    p.app.modal = Some(Modal::QuickSwitch { query: String::new(), sel: 0 });
    p.look(Show::Modals);
    p.type_text(Show::Modals, "Caf\u{e9}");
    let shown = p.look(Show::Modals);
    assert!(shown.iter().any(|(_, l, _)| l.starts_with("Cafe\u{301}")), "the note is listed: {:?}", shown.iter().map(|x| &x.1).collect::<Vec<_>>());
    assert!(!shown.iter().any(|(_, l, _)| l.contains("Create")), "the same name is no new note");
}

// spec: VAULT-023, NAV-005
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

// spec: VAULT-023, VAULT-042
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

// spec: EDIT-037, CFG-020
#[test]
fn edit_037_the_size_is_a_setting_in_the_vim_tab_with_reset() {
    let mut p = P::new("preview-size-setting", &[]);
    p.app.open_settings(SettingsTab::Vim);
    assert!(p.shows(Show::Modals, "Link preview size"), "the setting is on the Vim tab");
    assert_eq!((p.app.cfg.link_preview_width, p.app.cfg.link_preview_height), (640, 480));
    (p.app.cfg.link_preview_width, p.app.cfg.link_preview_height) = (300, 200);
    p.click(Show::Modals, "Reset size");
    assert_eq!((p.app.cfg.link_preview_width, p.app.cfg.link_preview_height), (640, 480), "Reset brings back the default");
    p.app.open_settings(SettingsTab::Appearance);
    assert!(!p.shows(Show::Modals, "Link preview size"), "a setting is on one tab only");
}

// spec: CFG-040, CFG-020
#[test]
fn cfg_042_number_headings_is_a_setting_of_the_editor_tab_and_on_by_default() {
    let mut p = P::new("number-headings-setting", &[]);
    p.app.open_settings(SettingsTab::Editor);
    assert!(p.shows(Show::Modals, "Number headings"), "the setting is on the Editor tab");
    assert!(p.app.cfg.numbered_headings, "on by default");
    p.click(Show::Modals, "Number headings");
    assert!(!p.app.cfg.numbered_headings);
    p.click(Show::Modals, "Number headings");
    assert!(p.app.cfg.numbered_headings);
}

// spec: CFG-051
#[test]
fn cfg_051_the_appearance_tab_chooses_how_properties_show_in_the_editor_and_the_reading_view() {
    use crate::config::PropertiesView;
    let mut p = P::new("properties-setting", &[]);
    p.app.open_settings(SettingsTab::Appearance);
    assert!(p.shows(Show::Modals, "Properties in the editor") && p.shows(Show::Modals, "Properties in the reading view"));
    assert_eq!((p.app.cfg.properties_in_editor, p.app.cfg.properties_in_reader), (PropertiesView::Folded, PropertiesView::Card), "defaults");
    p.click(Show::Modals, "Card");
    p.click(Show::Modals, "Raw (all lines)");
    assert_eq!(p.app.cfg.properties_in_reader, PropertiesView::Raw);
    assert_eq!(p.app.cfg.properties_in_editor, PropertiesView::Folded, "the other setting is untouched");
}

// spec: CFG-062, CFG-065
#[test]
fn cfg_062_the_appearance_tab_chooses_the_theme_and_with_follow_system_a_light_and_a_dark_one() {
    let mut p = P::new("theme-setting", &[]);
    p.app.open_settings(SettingsTab::Appearance);
    assert!(p.shows(Show::Modals, "Follow the system's light or dark mode"), "the checkbox is there");
    assert!(p.shows(Show::Modals, "Open themes folder") && p.shows(Show::Modals, "Reload themes"));
    assert!(p.shows(Show::Modals, "Theme") && !p.shows(Show::Modals, "Light theme"), "one combo to start with");
    assert_eq!(p.app.cfg.theme.name, "Shale");
    p.click(Show::Modals, "Shale");
    assert!(p.shows(Show::Modals, "Dark") && p.shows(Show::Modals, "Light"), "the themes are grouped");
    for name in ["Dracula", "Nord", "Catppuccin Mocha", "Gruvbox Dark", "Catppuccin Latte", "Solarized Light", "GitHub Light", "Gruvbox Light"] {
        assert!(p.shows(Show::Modals, name), "{name} can be chosen");
    }
    p.click(Show::Modals, "Catppuccin Latte");
    assert_eq!(p.app.cfg.theme.name, "Catppuccin Latte", "a choice is the setting");
    assert!(!p.app.cfg.theme.follow_system);
    // follow the system: two combos instead of the one
    p.click(Show::Modals, "Follow the system's light or dark mode");
    assert!(p.app.cfg.theme.follow_system);
    assert!(p.shows(Show::Modals, "Light theme") && p.shows(Show::Modals, "Dark theme"), "a light and a dark theme");
    assert_eq!((p.app.cfg.theme.light.as_str(), p.app.cfg.theme.dark.as_str()), ("Catppuccin Latte", "Shale"));
    p.click(Show::Modals, "Shale");
    p.click(Show::Modals, "Nord");
    assert_eq!(p.app.cfg.theme.dark, "Nord");
    assert_eq!(p.app.cfg.theme.name, "Catppuccin Latte", "the single choice is kept for later");
    // a name that is no theme: a hint, and Shale's own is used
    p.app.cfg.theme.dark = "Nope".into();
    assert!(p.shows(Show::Modals, "There is no theme called \"Nope\""));
    p.app.cfg.theme.dark = "Nord".into();
    assert!(!p.shows(Show::Modals, "There is no theme called"));
}

// spec: CFG-065
#[test]
fn cfg_065_the_appearance_tab_lists_the_themes_folder_reports_a_broken_file_and_opens_the_folder() {
    let mut p = P::new("theme-folder", &[]);
    let dir = p.dir.join("themes-of-the-user");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Broken.toml"), "[colors.primary]\nbackground = '#000000'\n").unwrap();
    p.app.themes = crate::themes::ThemeState::in_folder(Some(dir.clone()));
    p.app.open_settings(SettingsTab::Appearance);
    assert!(p.shows(Show::Modals, "Theme file skipped: Broken.toml: [colors.primary] is missing `foreground`"), "the error is listed under the picker");
    // a file added later is found by Reload themes, and a mended file stops being reported
    std::fs::write(dir.join("Broken.toml"), "[colors.primary]\nbackground = '#000000'\nforeground = '#ffffff'\n[colors.normal]\nblack = '#000000'\nred = '#ff0000'\ngreen = '#00ff00'\nyellow = '#ffff00'\nblue = '#0000ff'\nmagenta = '#ff00ff'\ncyan = '#00ffff'\nwhite = '#ffffff'\n").unwrap();
    assert!(p.app.themes.themes().get("Broken").is_none(), "not read before it is asked for");
    p.click(Show::Modals, "Reload themes");
    assert!(p.app.themes.themes().get("Broken").is_some_and(|t| t.user));
    assert!(!p.shows(Show::Modals, "Theme file skipped"));
    p.click(Show::Modals, "Shale");
    assert!(p.shows(Show::Modals, "Broken (yours)"), "a theme of the user's says so");
    // the folder is made when it is not there yet
    let missing = p.dir.join("not-there-yet");
    p.app.themes = crate::themes::ThemeState::in_folder(Some(missing.clone()));
    p.click(Show::Modals, "Open themes folder");
    if !missing.is_dir() {
        // the first click closed the list of themes that is still open
        p.click(Show::Modals, "Open themes folder");
    }
    assert!(missing.is_dir());
}

// spec: EDIT-038
#[test]
fn edit_038_the_lift_dialog_pastes_lifted_or_as_it_is_and_esc_pastes_as_it_is() {
    let deep = "## A\ntext\n### B\n";
    let pasted = |a: Vec<Action>| match a.as_slice() {
        [Action::PasteText(t)] => t.clone(),
        other => panic!("{} actions", other.len()),
    };
    let mut p = P::new("lift-paste", &[]);
    // the editor's question reaches the app and opens the dialog
    p.app.actions.push(Action::AskLiftHeadings(deep.into()));
    p.app.process_actions();
    assert!(matches!(&p.app.modal, Some(Modal::LiftPaste { text }) if text == deep));
    assert!(p.shows(Show::Modals, "The pasted text has no # heading. Lift its headings?"));
    p.click(Show::Modals, "Lift");
    assert!(p.app.modal.is_none());
    assert_eq!(pasted(p.actions()), "# A\ntext\n## B\n");
    // as it is
    p.app.modal = Some(Modal::LiftPaste { text: deep.into() });
    p.click(Show::Modals, "Paste as it is");
    assert!(p.app.modal.is_none());
    assert_eq!(pasted(p.actions()), deep);
    // Esc: as it is
    p.app.modal = Some(Modal::LiftPaste { text: deep.into() });
    p.look(Show::Modals);
    p.key(Show::Modals, Key::Escape);
    assert!(p.app.modal.is_none());
    assert_eq!(pasted(p.actions()), deep);
    // the answer is handed to the editor of the open note, which pastes it at the cursor
    p.app.open(Path::new("Welcome.md"), false);
    p.app.actions.push(Action::PasteText("# A\n".into()));
    p.app.process_actions();
    assert_eq!(p.app.tabbar.tabs[p.app.tabbar.active].editor.pending_paste.as_deref(), Some("# A\n"));
}

// spec: CFG-043, CFG-020
#[test]
fn cfg_043_date_prefix_for_new_notes_is_a_setting_of_the_editor_tab_and_on_by_default() {
    let mut p = P::new("date-prefix-setting", &[]);
    p.app.open_settings(SettingsTab::Editor);
    assert!(p.shows(Show::Modals, "Date prefix for new notes"), "the setting is on the Editor tab");
    assert!(p.app.cfg.date_prefix_new_notes, "on by default");
    p.click(Show::Modals, "Date prefix for new notes");
    assert!(!p.app.cfg.date_prefix_new_notes);
    p.click(Show::Modals, "Date prefix for new notes");
    assert!(p.app.cfg.date_prefix_new_notes);
}

// spec: EDIT-026, EDIT-028
#[test]
fn the_editing_settings_switch_the_toolbar_and_edit_the_colour_presets() {
    let mut p = P::new("editing-settings", &[]);
    p.app.open_settings(SettingsTab::Editor);
    for want in ["Editing", "Toolbar button size", "Font colours", "Highlight colours"] {
        assert!(p.shows(Show::Modals, want), "{want:?} missing");
    }
    assert!(p.app.cfg.format_toolbar, "on by default");
    p.click(Show::Modals, "Show formatting toolbar above the text of a note");
    assert!(!p.app.cfg.format_toolbar);
    p.click(Show::Modals, "Show formatting toolbar above the text of a note");
    assert!(p.app.cfg.format_toolbar);
    // a colour more in the font palette (the first row), a colour less, and back to the defaults
    let defaults = p.app.cfg.font_colours.clone();
    let top = |p: &mut P, label: &str| {
        let seen = p.look(Show::Modals);
        let r = seen.iter().filter(|(_, l, _)| l == label).map(|x| x.2).min_by(|a, b| a.top().total_cmp(&b.top())).unwrap_or_else(|| panic!("no {label:?}"));
        p.click_rect(Show::Modals, r, PointerButton::Primary);
    };
    top(&mut p, "+");
    assert_eq!(p.app.cfg.font_colours.len(), defaults.len() + 1);
    top(&mut p, "−");
    assert_eq!(p.app.cfg.font_colours.len(), defaults.len());
    top(&mut p, "−");
    assert_eq!(p.app.cfg.font_colours.len(), defaults.len() - 1);
    top(&mut p, "Defaults");
    assert_eq!(p.app.cfg.font_colours, defaults);
    assert_eq!(p.app.cfg.highlight_colours.len(), 4, "the other palette is untouched");
}

// spec: VAULT-023, CFG-020
#[test]
fn panel_settings_dialog_changes_settings_and_closes() {
    let mut p = P::new("settings", &[]);
    p.app.open_settings(SettingsTab::Vault);
    let shown = p.look(Show::Modals);
    for want in ["Vault folder", "People folder", "Done", "Close this vault (back to the start screen)"] {
        assert!(shown.iter().any(|(_, l, _)| l == want), "{want:?} missing");
    }
    // check boxes change the setting
    let (ctrl, dark) = (p.app.cfg.ctrl_to_follow, p.app.cfg.dark_canvas);
    p.click(Show::Modals, "Editor");
    p.click(Show::Modals, "Follow links only with Ctrl+click (plain click places the cursor)");
    assert_eq!(p.app.cfg.ctrl_to_follow, !ctrl);
    p.click(Show::Modals, "Update links in other notes when renaming");
    assert!(!p.app.cfg.update_links_on_rename);
    p.click(Show::Modals, "Appearance");
    p.click(Show::Modals, "Dark drawing canvas");
    assert_eq!(p.app.cfg.dark_canvas, !dark);
    // opening a vault that is not there is refused with a message, the dialog stays
    p.click(Show::Modals, "Vault");
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
    // the Keybindings tab, and Esc from it
    p.app.open_settings(SettingsTab::Vault);
    p.click(Show::Modals, "Keybindings");
    assert!(matches!(&p.app.modal, Some(Modal::Settings { tab: SettingsTab::Keybindings, .. })));
    p.key(Show::Modals, Key::Escape);
    assert!(p.app.modal.is_none());
    // Calendars… leaves the settings for the calendar dialog
    p.app.open_settings(SettingsTab::Calendar);
    p.click(Show::Modals, "Calendars…");
    assert!(matches!(&p.app.modal, Some(Modal::Calendars(_))));
    p.key(Show::Modals, Key::Escape);
    assert!(p.app.modal.is_none());
    // Done and Esc close the settings
    p.app.open_settings(SettingsTab::Vault);
    p.click(Show::Modals, "Done");
    assert!(p.app.modal.is_none());
    p.app.open_settings(SettingsTab::Vault);
    p.look(Show::Modals);
    p.key(Show::Modals, Key::Escape);
    assert!(p.app.modal.is_none());
    // closing the vault goes back to the start screen
    p.app.open_settings(SettingsTab::Vault);
    p.click(Show::Modals, "Close this vault (back to the start screen)");
    assert!(p.app.vault.is_none() && p.app.modal.is_none() && p.app.cfg.vault.is_none());
}

fn shown_labels(p: &mut P) -> Vec<String> {
    p.look(Show::Modals).into_iter().map(|(_, l, _)| l).collect()
}

// spec: CFG-020, TEX-023, EDIT-044, IMP-036
#[test]
fn set_001_every_tab_of_the_settings_shows_the_settings_of_its_domain() {
    let mut p = P::new("settings-tabs", &[]);
    let wants: [(SettingsTab, &[&str]); 13] = [
        (SettingsTab::Vault, &["Vault folder", "People folder"]),
        (SettingsTab::Editor, &["Cursor while typing", "Blinking cursor", "Show line numbers in the editor", "Update links in other notes when renaming", "Font colours"]),
        (SettingsTab::Vim, &["Vim-style editing", "Cursor in normal mode", "Esc from insert mode", "Link preview size"]),
        (SettingsTab::Appearance, &["Readable line length", "Dark drawing canvas"]),
        (SettingsTab::Fonts, &["Use the same font and size everywhere", "Font and size"]),
        (SettingsTab::Sync, &["Git sync"]),
        (SettingsTab::Calendar, &["Calendars…", "Calendar weeks start on Monday (otherwise Sunday)"]),
        (SettingsTab::Mail, &["Mail accounts…", "Fetch new mail every"]),
        (SettingsTab::Social, &["Feed accounts…", "Look for new posts every", "Post search index"]),
        (SettingsTab::Import, &["Import Word files as notes (needs pandoc)", "pandoc program", "Check again"]),
        (SettingsTab::Data, &["Data usage…"]),
        (SettingsTab::Latex, &["Which TeX is used", "Check again"]),
        (SettingsTab::Help, &["Open user guide (F1)", "Troubleshooting"]),
    ];
    for (tab, labels) in wants {
        p.app.open_settings(tab);
        let shown = shown_labels(&mut p);
        for l in labels {
            assert!(shown.iter().any(|s| s == l), "{tab:?}: {l:?} missing in {shown:?}");
        }
        for t in SettingsTab::ALL {
            assert!(shown.iter().any(|s| s == t.label()), "{tab:?}: the tab {:?} is not listed", t.label());
        }
    }
    // a setting is on its own tab only
    p.app.open_settings(SettingsTab::Appearance);
    assert!(!shown_labels(&mut p).iter().any(|s| s.starts_with("Show line numbers")));
}

// spec: EDIT-044
#[test]
fn edit_044_the_vim_tab_follows_editor_indented_and_holds_the_vim_settings() {
    let mut p = P::new("settings-vim-tab", &[]);
    let all = SettingsTab::ALL;
    let at = all.iter().position(|t| *t == SettingsTab::Editor).unwrap();
    assert_eq!(all[at + 1], SettingsTab::Vim, "listed right after Editor");
    p.app.open_settings(SettingsTab::Vim);
    let shown = p.look(Show::Modals);
    let x = |l: &str| shown.iter().find(|(_, t, _)| t == l).map(|x| x.2.min.x).unwrap_or_else(|| panic!("no {l:?}"));
    assert!(x("Vim") > x("Editor") + 8.0, "the Vim tab is indented under Editor");
    assert!((x("Appearance") - x("Editor")).abs() < 1.0, "the other tabs are not");
    // the settings moved: the Editor tab has none of them, the Vim tab has all
    p.app.open_settings(SettingsTab::Editor);
    let shown = shown_labels(&mut p);
    assert!(shown.iter().all(|s| !s.starts_with("Link preview size") && s != "Cursor in normal mode"), "{shown:?}");
    assert!(shown.iter().any(|s| s.starts_with("Vim-style editing: off")), "{shown:?}");
    // the combos are off while vim mode is off
    p.app.cfg.vim_mode = false;
    p.app.open_settings(SettingsTab::Vim);
    let shown = p.look(Show::Modals);
    assert!(shown.iter().any(|(_, t, _)| t == "Esc from insert mode"));
    // a click on the link in the Editor tab goes to the Vim tab
    p.app.open_settings(SettingsTab::Editor);
    p.click(Show::Modals, "Vim tab");
    assert!(matches!(&p.app.modal, Some(Modal::Settings { tab, .. }) if *tab == SettingsTab::Vim));
    p.click(Show::Modals, "Keybindings tab");
    assert!(matches!(&p.app.modal, Some(Modal::Settings { tab, .. }) if *tab == SettingsTab::Keybindings));
}

// spec: EDIT-044
#[test]
fn edit_044_the_vim_cursor_combos_open_only_while_vim_mode_is_on() {
    let mut p = P::new("settings-vim-combos", &[]);
    let (stay, back) = (crate::vim::EscCursor::Stay.label(), crate::vim::EscCursor::StepBack.label());
    p.app.cfg.vim_mode = false;
    p.app.open_settings(SettingsTab::Vim);
    p.click(Show::Modals, stay);
    let off = shown_labels(&mut p);
    assert!(!off.iter().any(|s| s == back), "a disabled combo does not open: {off:?}");
    p.app.cfg.vim_mode = true;
    p.app.open_settings(SettingsTab::Vim);
    p.click(Show::Modals, stay);
    let on = shown_labels(&mut p);
    assert!(on.iter().any(|s| s == back), "the same click opens it while vim mode is on: {on:?}");
}

/// The labels of Settings until one starts with `text` (a background job answers after a moment).
/// The wait ends by the clock, not by a count of frames: on a loaded machine a frame takes long.
fn labels_until(p: &mut P, text: &str) -> Vec<String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let shown = shown_labels(p);
        if shown.iter().any(|s| s.starts_with(text)) {
            return shown;
        }
        assert!(std::time::Instant::now() < deadline, "no label starting with {text:?} after 60 s in {shown:?}");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// Frames until the job of the vault check is done and its answer is taken in (the page keeps showing the last answer while a new check runs).
fn wait_for_vault_check(p: &mut P) {
    crate::ui_test_support::settle_within(std::time::Duration::from_secs(60), || {
        p.frame(Show::Modals, vec![]);
        if p.app.vault_check.running() {
            vec!["the vault check"]
        } else {
            vec![]
        }
    });
}

// spec: IMP-036
#[test]
fn imp_036_the_import_tab_has_the_word_setting_and_says_where_pandoc_is_or_how_to_install_it() {
    let mut p = P::new("settings-import", &[]);
    assert!(!p.app.cfg.docx_import && p.app.cfg.pandoc_path.is_empty(), "off and searching by default");
    // a pandoc that is not there: said in words, with the way to install it on each system
    p.app.cfg.pandoc_path = p.dir.join("no-such-pandoc").to_string_lossy().into_owned();
    p.app.open_settings(SettingsTab::Import);
    let shown = labels_until(&mut p, "pandoc was not found");
    for how in ["Linux: sudo pacman -S pandoc-cli   or   sudo apt install pandoc", "macOS: brew install pandoc", "Windows: winget install JohnMacFarlane.Pandoc", "or see https://pandoc.org/installing.html"] {
        assert!(shown.iter().any(|s| s == how), "{how:?} missing in {shown:?}");
    }
    // the box can be ticked while pandoc is missing
    p.click(Show::Modals, "Import Word files as notes (needs pandoc)");
    assert!(p.app.cfg.docx_import);
    p.click(Show::Modals, "Import Word files as notes (needs pandoc)");
    assert!(!p.app.cfg.docx_import);
    // a pandoc that is there: its version and place, no install help
    #[cfg(unix)]
    {
        let fake = crate::import::docx_tests::fake_pandoc(&p.dir, "pandoc", "echo 'pandoc 9.9.1'");
        p.app.cfg.pandoc_path = fake.to_string_lossy().into_owned();
        p.click(Show::Modals, "Check again");
        let shown = labels_until(&mut p, "pandoc 9.9.1, found at");
        assert!(shown.iter().any(|s| *s == format!("pandoc 9.9.1, found at {}", fake.display())), "{shown:?}");
        assert!(!shown.iter().any(|s| s.starts_with("macOS:")), "no install help when it is found");
        // one that does not run: its words
        let bad = crate::import::docx_tests::fake_pandoc(&p.dir, "pandoc-bad", "echo 'Illegal instruction' >&2; exit 4");
        p.app.cfg.pandoc_path = bad.to_string_lossy().into_owned();
        p.click(Show::Modals, "Check again");
        labels_until(&mut p, &format!("{} does not work: Illegal instruction", bad.display()));
    }
}

// spec: CFG-060
#[test]
fn cfg_060_the_import_tab_has_the_header_list_setting() {
    let mut p = P::new("settings-header-list", &[]);
    assert!(!p.app.cfg.import_header_list, "off by default");
    p.app.open_settings(SettingsTab::Import);
    p.click(Show::Modals, "Header list in the note body");
    assert!(p.app.cfg.import_header_list);
    p.click(Show::Modals, "Header list in the note body");
    assert!(!p.app.cfg.import_header_list);
}

// spec: CFG-074
#[test]
fn cfg_074_the_import_tab_has_the_skip_signatures_setting() {
    let mut p = P::new("settings-skip-signatures", &[]);
    assert!(p.app.cfg.import_skip_signatures, "on by default");
    p.app.open_settings(SettingsTab::Import);
    p.click(Show::Modals, "Skip signatures");
    assert!(!p.app.cfg.import_skip_signatures);
    p.click(Show::Modals, "Skip signatures");
    assert!(p.app.cfg.import_skip_signatures);
}

// spec: CFG-076
#[test]
fn cfg_076_the_import_tab_has_the_leave_out_quoted_setting() {
    let mut p = P::new("settings-leave-out-quoted", &[]);
    assert!(!p.app.cfg.import_leave_out_quoted, "off by default");
    p.app.open_settings(SettingsTab::Import);
    p.click(Show::Modals, "Leave out quoted mails");
    assert!(p.app.cfg.import_leave_out_quoted);
    p.click(Show::Modals, "Leave out quoted mails");
    assert!(!p.app.cfg.import_leave_out_quoted);
}

// spec: CFG-021
#[test]
fn set_002_settings_open_on_a_short_page() {
    let mut p = P::new("settings-short", &[]);
    p.app.open_settings(SettingsTab::Vault);
    let shown = p.look(Show::Modals);
    for hidden in ["Show line numbers in the editor", "Readable line length", "Git sync", "Troubleshooting", "Data usage…"] {
        assert!(!shown.iter().any(|(_, l, _)| l == hidden), "{hidden:?} is not on the first page");
    }
    let rect = |l: &str| shown.iter().find(|(_, t, _)| t.starts_with(l)).map(|x| x.2).unwrap_or_else(|| panic!("no {l:?}"));
    let height = rect("Type [[p followed").bottom() - rect("Vault folder").top();
    // 200 points before the raster of NAV-042 (more room between the lines and the items)
    assert!(height < 240.0, "the first page is {height} points high");
    p.app.open_settings(SettingsTab::Editor);
    let shown = p.look(Show::Modals);
    let rect = |l: &str| shown.iter().find(|(_, t, _)| t.starts_with(l)).map(|x| x.2).unwrap_or_else(|| panic!("no {l:?}"));
    assert!(rect("The colours of the toolbar").bottom() - rect("Cursor while typing").top() > 300.0, "the editor page is the long one");
}

// spec: CFG-022
#[test]
fn set_003_the_tab_list_switches_pages_and_the_last_tab_is_remembered() {
    let mut p = P::new("settings-switch", &[]);
    p.app.open_settings(SettingsTab::Vault);
    for t in [SettingsTab::Mail, SettingsTab::Editor, SettingsTab::Keybindings] {
        p.click(Show::Modals, t.label());
        assert!(matches!(&p.app.modal, Some(Modal::Settings { tab, .. }) if *tab == t), "{t:?}");
    }
    p.key(Show::Modals, Key::Escape);
    assert!(p.app.modal.is_none());
    assert_eq!(crate::settings::last_tab(&p.ctx), SettingsTab::Keybindings, "Settings comes back where it was left");
}

// spec: CFG-023
#[test]
fn set_004_the_simple_mail_calendar_and_feed_settings_are_on_their_tabs() {
    let mut p = P::new("settings-domains", &[]);
    p.app.open_settings(SettingsTab::Mail);
    assert!(p.app.cfg.mail_mark_read);
    p.click(Show::Modals, "Opening a message marks it read on the server");
    assert!(!p.app.cfg.mail_mark_read);
    p.click(Show::Modals, "Mail accounts…");
    assert!(matches!(&p.app.modal, Some(Modal::MailAccounts(_))));
    p.app.open_settings(SettingsTab::Calendar);
    assert!(p.app.cfg.week_starts_monday);
    p.click(Show::Modals, "Calendar weeks start on Monday (otherwise Sunday)");
    assert!(!p.app.cfg.week_starts_monday);
    p.app.open_settings(SettingsTab::Social);
    p.click(Show::Modals, "Feed accounts…");
    assert!(matches!(&p.app.modal, Some(Modal::Social(_))));
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

// spec: PERF-001
#[test]
fn perf_001_the_task_view_of_a_big_vault_draws_only_the_rows_in_view() {
    let notes: Vec<(String, String)> = (0..300).map(|n| (format!("Note {n:03}.md"), (0..10).map(|t| format!("- [ ] task {n}-{t}\n")).collect::<String>())).collect();
    let files: Vec<(&str, &str)> = notes.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
    let mut p = P::with_files("tasksmany", &files);
    assert!(p.shows(Show::Tasks, "3000 open · 0 done"), "the counts are of all the tasks");
    assert!(p.shows(Show::Tasks, "task 0-0") && !p.shows(Show::Tasks, "task 299-9"), "the first rows, not the last");
    let drawn = p.app.tasks.rows.drawn();
    assert!((10..200).contains(&drawn), "{drawn} of 3300 rows drawn");
}

/// 400 notes in a folder of their own and 400 in the vault's top folder, in a short window.
fn big_file_list(tag: &str) -> P {
    let notes: Vec<(String, String)> = (0..400).flat_map(|n| [(format!("Note {n:03}.md"), "x\n".to_string()), (format!("Box/Item {n:03}.md"), "x\n".to_string())]).collect();
    let files: Vec<(&str, &str)> = notes.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
    let mut p = P::with_files(tag, &files);
    p.size = Vec2::new(900.0, 600.0);
    p.app.tree.expanded.insert(PathBuf::from("Box"));
    p
}

/// How many rows of the file list were given a place for the pointer to hit in the last frame.
fn tree_rows_with_a_place(p: &mut P) -> usize {
    p.app.drag.tree_targets.clear();
    p.frame(Show::Files, vec![]);
    p.app.drag.tree_targets.iter().filter(|(r, _, _)| r.height() > 0.0).count()
}

// spec: PERF-011
#[test]
fn perf_011_the_file_list_of_a_big_vault_draws_only_the_rows_in_view() {
    let mut p = big_file_list("treemany");
    assert!(p.shows(Show::Files, "Item 000") && p.shows(Show::Files, "Item 005"), "the first rows");
    assert!(!p.shows(Show::Files, "Item 399") && !p.shows(Show::Files, "Note 399"), "not the last rows, 801 rows down");
    let drawn = p.painted.iter().filter(|(t, _)| t.starts_with("Item ") || t.starts_with("Note ")).count();
    assert!((10..80).contains(&drawn), "{drawn} of 800 rows drawn");
    assert!(p.app.tree_rows().len() >= 800, "all rows are there for the keys and the scroll bar");
}

// spec: PERF-011
#[test]
fn perf_011_the_rows_are_made_once_per_state_of_the_list() {
    let mut p = big_file_list("treeonce");
    p.look(Show::Files);
    let made = p.app.tree.rows.builds;
    assert_eq!(made, 1, "once for the first frames");
    p.look(Show::Files);
    assert_eq!(p.app.tree.rows.builds, made, "not again for the next frames");
    // a folder closes
    p.app.tree.expanded.clear();
    p.look(Show::Files);
    assert_eq!(p.app.tree.rows.builds, made + 1, "again when a folder is opened or closed");
    // the sort setting
    p.app.cfg.file_sort = crate::config::FileSort::NameDesc;
    p.look(Show::Files);
    assert_eq!(p.app.tree.rows.builds, made + 2, "again for another sort setting");
    let first = p.app.tree_rows().iter().find(|r| !r.is_folder()).map(|r| r.name.clone());
    assert_eq!(first.as_deref(), Some("Note 399"), "in the new order");
    // a new note on disk
    std::fs::write(p.dir.join("Zeta.md"), "x\n").unwrap();
    p.app.vault.as_mut().unwrap().rescan();
    p.look(Show::Files);
    assert_eq!(p.app.tree.rows.builds, made + 3, "again when the vault changed");
    assert_eq!(p.app.tree_rows().iter().find(|r| !r.is_folder()).map(|r| r.name.as_str()), Some("Zeta"));
}

// spec: PERF-011
#[test]
fn perf_011_the_keys_move_the_selection_and_scroll_to_a_row_that_was_not_drawn() {
    let mut p = big_file_list("treekeys");
    p.look(Show::Files);
    assert!(!p.shows(Show::Files, "Note 399"));
    p.app.tree.sel = Some(PathBuf::from("Box/Item 000.md"));
    p.key(Show::Files, Key::J);
    assert_eq!(p.app.tree.sel.as_deref(), Some(Path::new("Box/Item 001.md")), "j goes to the next file");
    // the selection far below the view (the open note after Ctrl+H): it is scrolled to
    p.app.tree.sel = Some(PathBuf::from("Note 399.md"));
    p.app.tree.scroll = true;
    assert!(p.shows(Show::Files, "Note 399"), "the row far below is drawn after the scroll");
    assert!(!p.app.tree.scroll, "the scroll is done");
    assert!(!p.shows(Show::Files, "Item 000"), "the top rows are out of the drawn rows now");
    p.key(Show::Files, Key::K);
    assert_eq!(p.app.tree.sel.as_deref(), Some(Path::new("Note 398.md")), "k goes to the previous file");
}

// spec: PERF-011
#[test]
fn perf_011_a_row_in_view_still_opens_and_is_a_drop_target_and_the_others_are_not() {
    let mut p = big_file_list("treerows");
    let r = p.row("Item 002");
    p.click_rect(Show::Files, r, PointerButton::Primary);
    assert_eq!(opened(&p.actions()), ["Box/Item 002.md"]);
    let places = tree_rows_with_a_place(&mut p);
    assert!((10..80).contains(&places), "{places} rows with a place of their own");
}

// spec: TASK-005
#[test]
fn panel_task_list_says_so_when_there_are_none() {
    let mut p = P::with_files("notasks", &[("Empty.md", "nothing to do\n")]);
    assert!(p.shows(Show::Tasks, "No tasks yet"));
    assert!(p.shows(Show::Tasks, "0 open · 0 done"));
}

// spec: VAULT-024, BOOK-010
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
    for (item, want) in [("New note here", 0), ("New drawing here", 1), ("New folder here", 2), ("Rename…", 3), ("Move to trash", 4), ("Export to PDF…", 5)] {
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
            4 => matches!(a.as_slice(), [Action::Trash(f)] if f == Path::new("Box")),
            _ => matches!(a.as_slice(), [Action::ExportFolder(f)] if f == Path::new("Box")),
        };
        assert!(ok, "{item}: {} actions", a.len());
    }
}

// spec: DEV-020, KEY-013
#[test]
fn dev_020_the_help_button_in_settings_opens_the_user_guide_as_the_vault() {
    let mut p = P::new("userguide-help", &[]);
    let root = p.dir.join("guide-data");
    p.app.guide = crate::userguide::Places { files: crate::userguide::tests::SAMPLE, root: Some(root.clone()) };
    p.app.open_settings(SettingsTab::Help);
    let shown = p.look(Show::Modals);
    assert!(shown.iter().any(|(_, l, _)| l == "Help"), "the Help section is shown");
    p.click(Show::Modals, "Open user guide (F1)");
    let copy = root.join(format!("userguide-{}", env!("CARGO_PKG_VERSION")));
    assert!(p.app.modal.is_none(), "Settings closes");
    // the guide is read in the background (NET-019)
    // (this harness draws only the dialogs: it takes in the vault itself)
    crate::ui_test_support::settle_within(std::time::Duration::from_secs(60), || {
        p.frame(Show::Modals, vec![]);
        p.app.vault_load_tick(&p.ctx);
        if p.app.vault_load.is_loading() {
            vec!["the loading of a vault"]
        } else {
            vec![]
        }
    });
    assert_eq!(p.app.vault.as_ref().map(|v| v.root.clone()), Some(copy));
}

// spec: VAULT-028
#[test]
fn vault_028_settings_vault_checks_names_in_the_background_and_renames_one_or_all_after_a_question() {
    // names over 200 bytes (two bytes a letter): wrong on every system, and every system holds them
    let (a, c) = ("α".repeat(101), "γ".repeat(101));
    let (a_md, c_md, linker) = (format!("{a}.md"), format!("{c}.md"), format!("[[{a}]] [[{c}]]\n"));
    let trashed = format!(".trash/2026-11-17/{}.md", "ε".repeat(101));
    let mut p = P::with_files("names-check", &[(&a_md, "A\n"), (&c_md, "C\n"), ("Fine.md", "F\n"), ("Linker.md", &linker), (&trashed, "gone\n")]);
    let (a2, c2) = ("α".repeat(98), "γ".repeat(98));
    p.app.open_settings(SettingsTab::Vault);
    let shown = shown_labels(&mut p);
    assert!(shown.iter().any(|l| l == "Check names for Windows and macOS…"));
    assert!(!shown.iter().any(|l| l.contains("would not work")), "nothing is listed before the check");
    p.click(Show::Modals, "Check names for Windows and macOS…");
    let shown = labels_until(&mut p, "2 names would not work on Windows or macOS");
    assert!(shown.iter().any(|l| l.starts_with(&a_md)), "{shown:?}");
    assert!(shown.iter().any(|l| l == "the name is longer than 200 bytes"), "{shown:?}");
    assert!(shown.iter().any(|l| *l == format!("→ {a2}.md")) && shown.iter().any(|l| *l == format!("→ {c2}.md")), "{shown:?}");
    assert!(!shown.iter().any(|l| l.contains('ε') || l.contains("Fine.md")), "the trash and good names are not listed");
    // one at a time
    // the topmost one: the order of the widgets in the accessibility tree follows their ids, not the page
    let seen = p.look(Show::Modals);
    let first = seen.iter().filter(|(r, l, _)| *r == egui::accesskit::Role::Button && l == "Rename").map(|x| x.2).min_by(|a, b| a.top().total_cmp(&b.top())).expect("a Rename button");
    p.click_rect(Show::Modals, first, PointerButton::Primary);
    let shown = labels_until(&mut p, "1 name would not work on Windows or macOS");
    assert!(p.dir.join(format!("{a2}.md")).exists() && !p.dir.join(&a_md).exists());
    assert_eq!(std::fs::read_to_string(p.dir.join("Linker.md")).unwrap(), format!("[[{a2}]] [[{c}]]\n"), "links are rewritten");
    assert!(shown.iter().any(|l| l.starts_with("Renamed 1 file or folder, updated links in 1 file(s)")), "{shown:?}");
    // all of them: first a question, Cancel changes nothing
    p.click(Show::Modals, "Rename all…");
    let shown = shown_labels(&mut p);
    assert!(shown.iter().any(|l| l == "Rename 1 file or folder? Links to them are updated."), "{shown:?}");
    p.click(Show::Modals, "Cancel");
    assert!(p.dir.join(&c_md).exists());
    p.click(Show::Modals, "Rename all…");
    p.click(Show::Modals, "Rename all");
    labels_until(&mut p, "All names work on Windows and macOS.");
    assert!(p.dir.join(format!("{c2}.md")).exists() && !p.dir.join(&c_md).exists());
    assert_eq!(std::fs::read_to_string(p.dir.join("Linker.md")).unwrap(), format!("[[{a2}]] [[{c2}]]\n"));
}

// spec: JOB-012
#[test]
fn job_012_the_vault_check_shows_what_it_does_is_off_while_it_runs_and_cancel_stops_it() {
    let mut p = P::with_files("names-cancel", &[("Fine.md", "F\n")]);
    p.app.open_settings(SettingsTab::Vault);
    p.look(Show::Modals);
    // a check that waits until it is cancelled
    p.app.start_vault_check_with(|_, cancel| {
        let start = std::time::Instant::now();
        while !cancel.is_cancelled() {
            assert!(start.elapsed().as_secs() < 30, "Cancel reached the check");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        Err(std::io::ErrorKind::Interrupted.into())
    });
    assert!(p.app.vault_check.running());
    let task = p.app.busy.info(&crate::vault_check::VAULT_CHECK).expect("a task in the registry");
    assert_eq!((task.label.as_str(), task.cancellable), ("Checking file names and links…", true));
    let shown = shown_labels(&mut p);
    assert!(shown.iter().any(|l| l == "Checking file names and links…") && shown.iter().any(|l| l == "Cancel"), "{shown:?}");
    // the button is off, and a second start does nothing
    p.click(Show::Modals, "Check names for Windows and macOS…");
    p.app.start_vault_check();
    assert_eq!(p.app.busy.list().len(), 1);
    p.click(Show::Modals, "Cancel");
    let shown = labels_until(&mut p, "The check was cancelled.");
    assert!(!shown.iter().any(|l| l == "Checking file names and links…") && p.app.busy.list().is_empty() && !p.app.vault_check.running(), "{shown:?}");
    assert!(p.app.vault_check.result.is_none(), "a cancelled check leaves no result");
    // the button is back and the real check runs to the end
    p.click(Show::Modals, "Check names for Windows and macOS…");
    labels_until(&mut p, "All names work on Windows and macOS.");
    assert!(p.app.busy.list().is_empty());
}

// spec: MEDIA-022, VAULT-028
#[test]
fn media_022_the_vault_check_lists_outside_links_by_note_in_the_same_background_job() {
    let outside = std::env::temp_dir().join(format!("shale-panels-outside-check-media-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&outside);
    std::fs::create_dir_all(outside.join("Media")).unwrap();
    std::fs::write(outside.join("Media/a.png"), b"p").unwrap();
    std::fs::write(outside.join("loose.png"), b"p").unwrap();
    let loose = crate::media::testdata::file_address(&outside.join("loose.png"));
    let note = format!("![[@Media/a.png]] ![[@Media/gone.png]] ![[@Nowhere/x.mp3]] ![](<{}>)\n", loose.replace("%20", " "));
    let mut p = P::with_files("outside-check", &[("Links.md", note.as_str()), ("Plain.md", "no media\n")]);
    p.app.cfg.media_libraries = vec![crate::media::outside::Library { name: "Media".into(), path: outside.join("Media") }];
    p.app.vault.as_mut().unwrap().set_libraries(&p.app.cfg.media_libraries);
    p.app.open_settings(SettingsTab::Vault);
    let shown = shown_labels(&mut p);
    assert!(!shown.iter().any(|l| l.contains("outside the vault")), "nothing is listed before the check: {shown:?}");
    p.app.start_vault_check();
    assert!(p.app.vault_check.running(), "the files are looked up by the job, not by the frame");
    let shown = labels_until(&mut p, "4 links to media outside the vault");
    assert!(shown.iter().any(|l| l.contains("the files are not synced") && l.contains("3 need a look:")), "{shown:?}");
    assert!(shown.iter().any(|l| l == "Links.md"), "by note: {shown:?}");
    assert!(!shown.iter().any(|l| l == "Plain.md"), "a note without outside links is not listed");
    assert!(shown.iter().any(|l| l == "@Media/gone.png") && shown.iter().any(|l| l.starts_with("not found: ") && l.ends_with("gone.png")), "{shown:?}");
    assert!(shown.iter().any(|l| l == "@Nowhere/x.mp3") && shown.iter().any(|l| l == "library “Nowhere” is not set up on this device"), "{shown:?}");
    assert!(shown.iter().any(|l| l.contains("a file link") && l.contains("this computer only")), "{shown:?}");
    assert!(shown.iter().any(|l| l.starts_with("Library “Nowhere” is not set up on this device (Settings → Import): 1 link")), "{shown:?}");
    assert!(!shown.iter().any(|l| l == "@Media/a.png"), "the library link that works is not listed");
    // the names part is there as before
    assert!(shown.iter().any(|l| l == "All names work on Windows and macOS."), "{shown:?}");
    // a vault without outside links says so
    let mut q = P::with_files("outside-check-none", &[("Plain.md", "no media [[Plain]]\n")]);
    q.app.open_settings(SettingsTab::Vault);
    q.click(Show::Modals, "Check names for Windows and macOS…");
    labels_until(&mut q, "No note links a picture, PDF, sound or video from outside the vault.");
    // a library whose folder is set but not there
    p.app.cfg.media_libraries = vec![crate::media::outside::Library { name: "Media".into(), path: outside.join("unplugged") }];
    p.app.vault.as_mut().unwrap().set_libraries(&p.app.cfg.media_libraries);
    p.click(Show::Modals, "Check names for Windows and macOS…");
    // the page still shows the answer of the first check until the new job is done: the label of it is no sign of the new one
    wait_for_vault_check(&mut p);
    let shown = labels_until(&mut p, "4 links to media outside the vault");
    assert!(shown.iter().any(|l| l.starts_with("Library “Media” has its folder ") && l.contains("unplugged") && l.contains("but it is not there: 2 links")), "{shown:?}");
    let _ = std::fs::remove_dir_all(&outside);
}

// spec: MEDIA-023
#[test]
fn media_023_settings_sync_says_how_many_media_files_are_linked_from_outside_and_not_synced() {
    let mut p = P::with_files("outside-sync-note", &[("Trip.md", "![[@Videos/a.mp4]] ![[@Videos/a.mp4|200]] [[@Videos/b.pdf]]\n"), ("Other.md", "![](<file:///elsewhere/pic.png>) [[Trip]]\n")]);
    p.app.open_settings(SettingsTab::Sync);
    let shown = shown_labels(&mut p);
    let note = shown.iter().find(|l| l.contains("linked from outside the vault")).unwrap_or_else(|| panic!("{shown:?}"));
    assert!(note.starts_with("3 media files are linked from outside the vault and are not synced"), "each file once, however often it is linked: {note}");
    assert!(note.contains("file history"), "{note}");
    assert!(shown.iter().any(|l| l.contains("Settings → Import")), "the way to set the library up: {shown:?}");
    // one file: the singular
    let mut q = P::with_files("outside-sync-note-one", &[("Trip.md", "![[@Videos/a.mp4]]\n")]);
    q.app.open_settings(SettingsTab::Sync);
    assert!(shown_labels(&mut q).iter().any(|l| l.starts_with("1 media file is linked from outside the vault and is not synced")));
    // no links: no note; and the count follows the notes
    let mut r = P::with_files("outside-sync-note-none", &[("Plain.md", "no media\n")]);
    r.app.open_settings(SettingsTab::Sync);
    assert!(!shown_labels(&mut r).iter().any(|l| l.contains("linked from outside the vault")));
    std::fs::write(r.dir.join("Plain.md"), "![[@Videos/c.mp4]]\n").unwrap();
    r.app.vault.as_mut().unwrap().rescan();
    assert!(labels_until(&mut r, "1 media file is linked from outside the vault").len() > 1);
}

/// The text boxes on screen, top to bottom and left to right.
fn text_boxes(p: &mut P) -> Vec<Rect> {
    let mut boxes: Vec<Rect> = p.look(Show::Modals).into_iter().filter(|(r, _, _)| matches!(r, Role::TextInput | Role::MultilineTextInput)).map(|(_, _, rect)| rect).collect();
    boxes.sort_by(|a, b| (a.min.y.round(), a.min.x).partial_cmp(&(b.min.y.round(), b.min.x)).unwrap());
    // the search over all settings is the first box of the Settings window: the numbers are those of the page
    if matches!(p.app.modal, Some(Modal::Settings { .. })) && !boxes.is_empty() {
        boxes.remove(0);
    }
    boxes
}

/// Click into text box number `n` and replace its text.
fn type_into(p: &mut P, n: usize, text: &str) {
    let r = text_boxes(p)[n];
    p.click_rect(Show::Modals, r, PointerButton::Primary);
    p.select_all(Show::Modals);
    if text.is_empty() {
        p.key(Show::Modals, Key::Backspace);
    } else {
        p.type_text(Show::Modals, text);
    }
}

// spec: MEDIA-019, CFG-070
#[test]
fn media_019_the_import_tab_has_the_setting_for_files_from_outside_the_vault_and_it_asks_by_default() {
    use crate::media::intake::OutsideFiles;
    let mut p = P::new("settings-outside", &[]);
    assert_eq!(p.app.cfg.outside_files, OutsideFiles::Ask, "Ask by default");
    p.app.open_settings(SettingsTab::Import);
    for l in ["Files from outside the vault", "Ask", "Copy into the vault", "Link where they are", "Library folders", "Add library"] {
        assert!(p.shows(Show::Modals, l), "{l:?} is on the Import tab");
    }
    p.click(Show::Modals, "Link where they are");
    assert_eq!(p.app.cfg.outside_files, OutsideFiles::Link);
    p.click(Show::Modals, "Copy into the vault");
    assert_eq!(p.app.cfg.outside_files, OutsideFiles::Copy);
    p.click(Show::Modals, "Ask");
    assert_eq!(p.app.cfg.outside_files, OutsideFiles::Ask);
    // another tab does not have it
    p.app.open_settings(SettingsTab::Appearance);
    assert!(!p.shows(Show::Modals, "Link where they are"));
}

// spec: MEDIA-019, CFG-069
#[test]
fn media_019_library_folders_are_added_renamed_and_removed_in_settings_and_a_bad_name_is_said() {
    use crate::media::outside::Library;
    let mut p = P::new("settings-libraries", &[]);
    let folder = p.dir.join("..").join(format!("shale-libs-{}", std::process::id()));
    std::fs::create_dir_all(folder.join("Videos")).unwrap();
    let videos = folder.join("Videos").to_string_lossy().into_owned();
    p.app.open_settings(SettingsTab::Import);
    assert!(p.app.cfg.media_libraries.is_empty());
    // nothing to add yet: the button is off
    let add = |p: &mut P| p.click(Show::Modals, "Add library");
    add(&mut p);
    assert!(p.app.cfg.media_libraries.is_empty(), "no name, no folder");
    // a name that is no name: said, and not added even with a folder
    for (bad, said) in [("a/b", "A name has no / in it"), ("a|b", "A name has none of # | ^ [ ]"), ("CON", "reserved name on Windows")] {
        type_into(&mut p, 0, bad);
        type_into(&mut p, 1, &videos);
        assert!(p.shows(Show::Modals, said), "{bad:?}: {said:?}");
        add(&mut p);
        assert!(p.app.cfg.media_libraries.is_empty(), "{bad:?} is not added");
    }
    // a good name and a folder
    type_into(&mut p, 0, "Videos");
    assert!(!p.shows(Show::Modals, "A name has"), "the message is gone");
    add(&mut p);
    assert_eq!(p.app.cfg.media_libraries, vec![Library { name: "Videos".into(), path: PathBuf::from(&videos) }]);
    assert!(!p.shows(Show::Modals, "This folder is not there") && !p.shows(Show::Modals, "Give the folder"));
    // the same name again, in another case: refused
    type_into(&mut p, 2, "VIDEOS");
    type_into(&mut p, 3, &videos);
    assert!(p.shows(Show::Modals, "Another library has this name"));
    add(&mut p);
    assert_eq!(p.app.cfg.media_libraries.len(), 1);
    // a second library
    type_into(&mut p, 2, "Pics");
    type_into(&mut p, 3, &folder.join("nowhere").to_string_lossy());
    add(&mut p);
    assert_eq!(p.app.cfg.media_libraries.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["Videos", "Pics"]);
    assert!(p.shows(Show::Modals, "This folder is not there"), "a folder that is not there is said");
    // rename: in place; a name taken by the other library or an empty one is said, and a path can be changed
    type_into(&mut p, 0, "pics");
    assert!(p.shows(Show::Modals, "Another library has this name"));
    type_into(&mut p, 0, "Films");
    assert_eq!(p.app.cfg.media_libraries[0].name, "Films");
    assert!(!p.shows(Show::Modals, "Another library has this name"));
    type_into(&mut p, 0, "");
    assert!(p.shows(Show::Modals, "Give the library a name"));
    type_into(&mut p, 0, "Films");
    type_into(&mut p, 3, &videos);
    assert_eq!(p.app.cfg.media_libraries[1].path, PathBuf::from(&videos));
    assert!(!p.shows(Show::Modals, "This folder is not there"));
    type_into(&mut p, 3, "");
    assert!(p.shows(Show::Modals, "Give the folder"));
    // remove the first
    let mut removes: Vec<Rect> = p.look(Show::Modals).into_iter().filter(|(r, l, _)| *r == Role::Button && l == "Remove").map(|x| x.2).collect();
    removes.sort_by(|a, b| a.min.y.total_cmp(&b.min.y));
    assert_eq!(removes.len(), 2);
    p.click_rect(Show::Modals, removes[0], PointerButton::Primary);
    assert_eq!(p.app.cfg.media_libraries.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["Pics"]);
    let _ = std::fs::remove_dir_all(&folder);
}

// spec: MEDIA-019, EDIT-019
#[test]
fn media_019_the_question_for_pasted_files_copies_links_or_cancels_and_the_answer_reaches_the_editor() {
    use crate::media::intake::Choice;
    let mut p = P::new("outside-ask", &[]);
    let out = p.dir.join("..").join(format!("shale-ask-{}", std::process::id()));
    std::fs::create_dir_all(&out).unwrap();
    for f in ["a.mp4", "b.pdf"] {
        std::fs::write(out.join(f), f).unwrap();
    }
    let one = out.join("a.mp4").to_string_lossy().into_owned();
    let two = format!("{one}\n{}", out.join("b.pdf").to_string_lossy());
    let answers = |a: Vec<Action>| match a.as_slice() {
        [] => None,
        [Action::PasteFiles(t, how)] => Some((t.clone(), *how)),
        other => panic!("{} actions", other.len()),
    };
    // the editor's question reaches the app and opens the dialog, which names the files
    p.app.actions.push(Action::AskOutsideFiles(one.clone()));
    p.app.process_actions();
    assert!(matches!(&p.app.modal, Some(Modal::OutsideFiles { text }) if *text == one));
    assert!(p.shows(Show::Modals, "“a.mp4” is outside the vault."));
    for l in ["Copy into the vault", "Link where they are", "Cancel", "C copy · L link · Esc cancel"] {
        assert!(p.shows(Show::Modals, l), "{l:?}");
    }
    p.click(Show::Modals, "Copy into the vault");
    assert!(p.app.modal.is_none());
    assert_eq!(answers(p.actions()), Some((one.clone(), Choice::Copy)));
    // several files, each answer on its own
    for (how, press) in [(Choice::Link, "button"), (Choice::Link, "key"), (Choice::Copy, "key"), (Choice::Copy, "enter")] {
        p.app.modal = Some(Modal::OutsideFiles { text: two.clone() });
        assert!(p.shows(Show::Modals, "2 files outside the vault: a.mp4, b.pdf."));
        match (how, press) {
            (Choice::Link, "button") => p.click(Show::Modals, "Link where they are"),
            (Choice::Link, _) => p.key(Show::Modals, Key::L),
            (Choice::Copy, "enter") => p.key(Show::Modals, Key::Enter),
            _ => p.key(Show::Modals, Key::C),
        }
        assert!(p.app.modal.is_none(), "{how:?} {press}");
        assert_eq!(answers(p.actions()), Some((two.clone(), how)));
    }
    // Esc and Cancel: nothing happens
    p.app.modal = Some(Modal::OutsideFiles { text: one.clone() });
    p.look(Show::Modals);
    p.key(Show::Modals, Key::Escape);
    assert!(p.app.modal.is_none() && p.actions().is_empty(), "Esc");
    p.app.modal = Some(Modal::OutsideFiles { text: one.clone() });
    p.click(Show::Modals, "Cancel");
    assert!(p.app.modal.is_none() && p.actions().is_empty(), "Cancel");
    // the editor's question becomes the app's action; the answer is handed to the editor of the open note
    let mut asked = vec![];
    crate::app::handle_editor_action(&mut asked, crate::editor::EditorAction::AskOutsideFiles(one.clone()), Path::new("Welcome.md"));
    assert!(matches!(asked.as_slice(), [Action::AskOutsideFiles(t)] if *t == one));
    p.app.open(Path::new("Welcome.md"), false);
    p.app.actions.push(Action::PasteFiles(one.clone(), Choice::Link));
    p.app.process_actions();
    assert_eq!(p.app.tabbar.tabs[p.app.tabbar.active].editor.pending_files, Some((one, Choice::Link)));
    let _ = std::fs::remove_dir_all(&out);
}
