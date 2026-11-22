//! The search over all settings (CFG-072, CFG-073) and the filter of the Keybindings tab
//! (KEY-006) driven with real key and pointer events through the whole window.

use super::anchor;
use super::registry;
use crate::app::{Modal, Options, SettingsTab, ShaleApp};
use crate::keys::{Action, Keymap};
use eframe::egui::{self, Event, Key, Modifiers, Pos2, RawInput, Rect, Vec2};

struct W {
    app: ShaleApp,
    ctx: egui::Context,
    t: f64,
    dir: std::path::PathBuf,
    /// The text of the last frame with the top-left corner of each.
    text_pos: Vec<(String, Pos2)>,
    size: Vec2,
}

impl W {
    fn new(tag: &str, size: Vec2) -> W {
        let dir = std::env::temp_dir().join(format!("shale-find-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("A.md"), "# A\n").unwrap();
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
        // these tests drive the Linux/Windows defaults; macOS has other keys for six actions (KEY-019)
        app.keymap = Keymap::from_overrides_for(&app.cfg.keybindings, false);
        let mut w = W { app, ctx: egui::Context::default(), t: 0.0, dir, text_pos: vec![], size };
        crate::theme::apply(&w.ctx);
        w.frame(vec![]);
        w
    }

    fn frame(&mut self, events: Vec<Event>) {
        self.t += 0.05;
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)), time: Some(self.t), events, ..Default::default() };
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
        self.text_pos.clear();
        for c in &out.shapes {
            walk(&c.shape, &mut self.text_pos);
        }
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame(vec![]);
        }
    }

    fn open(&mut self, tab: SettingsTab) {
        self.app.open_settings(tab);
        self.frames(5);
    }

    fn key(&mut self, key: Key, modifiers: Modifiers) {
        self.frame(vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }]);
        self.frame(vec![Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers }]);
    }

    fn type_text(&mut self, text: &str) {
        self.frame(vec![Event::Text(text.to_string())]);
        self.frames(2);
    }

    fn tab(&self) -> SettingsTab {
        match &self.app.modal {
            Some(Modal::Settings { tab, .. }) => *tab,
            _ => panic!("Settings is not open"),
        }
    }

    fn painted(&self, text: &str) -> bool {
        self.text_pos.iter().any(|t| t.0 == text)
    }

    /// Some painted text starts with this (a result of the search has its keys after the label).
    fn painted_start(&self, text: &str) -> bool {
        self.text_pos.iter().any(|t| t.0.starts_with(text))
    }

    fn at(&self, text: &str) -> Pos2 {
        self.text_pos.iter().find(|t| t.0 == text).unwrap_or_else(|| panic!("{text:?} is not painted: {:?}", self.text_pos.iter().map(|t| &t.0).collect::<Vec<_>>())).1
    }

    fn click_at(&mut self, p: Pos2) {
        let b = |pressed| Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        self.frame(vec![Event::PointerMoved(p)]);
        self.frame(vec![b(true)]);
        self.frame(vec![b(false)]);
        self.frames(2);
    }

    /// Ctrl+F, then the text: the field of the dialog has the keyboard.
    fn search(&mut self, text: &str) {
        self.key(Key::F, Modifiers::COMMAND);
        self.type_text(text);
    }

    fn query(&self) -> String {
        match &self.app.modal {
            Some(Modal::Settings { find, .. }) => find.query().to_string(),
            _ => panic!("Settings is not open"),
        }
    }

    fn filter(&self) -> String {
        match &self.app.modal {
            Some(Modal::Settings { keys, .. }) => keys.filter.clone(),
            _ => panic!("Settings is not open"),
        }
    }

    fn dialog_rect(&self) -> Rect {
        self.ctx.memory(|m| m.area_rect(egui::Id::unique(("dialog", "settings")))).expect("settings dialog shown")
    }

    fn highlighted(&self) -> Option<String> {
        anchor::active(&self.ctx).map(|(id, _)| id)
    }
}

impl Drop for W {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

const TALL: Vec2 = Vec2::new(900.0, 2000.0);

// spec: CFG-072
#[test]
fn cfg_072_typing_lists_the_results_with_their_tabs() {
    let mut w = W::new("list", TALL);
    w.open(SettingsTab::Vault);
    w.search("blinking");
    assert_eq!(w.query(), "blinking", "Ctrl+F gave the field the keyboard");
    assert!(w.painted("Blinking cursor"), "the setting is listed");
    assert!(w.text_pos.iter().filter(|t| t.0 == "Editor").count() >= 2, "with its tab (the tab list has one, the result the other)");
    assert_eq!(w.tab(), SettingsTab::Vault, "listing does not change the tab");
    w.type_text("zzz");
    assert!(w.painted("No setting matches"));
}

// spec: CFG-072
#[test]
fn cfg_072_ctrl_f_and_slash_focus_the_field_and_the_slash_is_not_typed() {
    let mut w = W::new("focus", TALL);
    w.open(SettingsTab::Editor);
    w.key(Key::Slash, Modifiers::NONE);
    w.type_text("cur");
    assert_eq!(w.query(), "cur");
    // a slash typed in the field is a slash
    w.type_text("/");
    assert_eq!(w.query(), "cur/");
}

// spec: CFG-072
#[test]
fn cfg_072_the_arrow_keys_and_enter_choose_a_result_and_go_to_its_tab() {
    let mut w = W::new("keys", TALL);
    w.open(SettingsTab::Vault);
    w.search("cursor");
    let hits = registry::search("cursor", &w.app.keymap);
    assert!(hits.len() >= 3, "{hits:?}");
    w.key(Key::ArrowDown, Modifiers::NONE);
    w.key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(w.query(), "cursor", "the arrows moved the choice, not the text cursor");
    w.key(Key::Enter, Modifiers::NONE);
    w.frames(2);
    assert_eq!(w.tab(), hits[2].tab, "Enter went to the third result {:?}", hits[2].id);
    assert_eq!(w.highlighted().as_deref(), Some(hits[2].id.as_str()));
    // the list is closed after the pick: Esc now closes the dialog
    w.key(Key::Escape, Modifiers::NONE);
    assert!(w.app.modal.is_none());
}

// spec: CFG-072
#[test]
fn cfg_072_esc_closes_the_list_first_and_the_dialog_after() {
    let mut w = W::new("esc", TALL);
    w.open(SettingsTab::Vault);
    w.search("cursor");
    assert!(w.painted("Blinking cursor"));
    w.key(Key::Escape, Modifiers::NONE);
    assert!(w.app.modal.is_some(), "the first Esc only closes the list");
    assert!(!w.painted("Blinking cursor"));
    w.key(Key::ArrowDown, Modifiers::NONE);
    assert!(w.painted("Blinking cursor"), "↓ opens it again");
    w.key(Key::Escape, Modifiers::NONE);
    w.key(Key::Escape, Modifiers::NONE);
    assert!(w.app.modal.is_none());
}

// spec: CFG-072
#[test]
fn cfg_072_a_click_on_a_result_goes_to_its_tab() {
    let mut w = W::new("click", TALL);
    w.open(SettingsTab::Vault);
    w.search("blinking");
    let p = w.at("Blinking cursor");
    w.click_at(p + Vec2::new(6.0, 6.0));
    assert_eq!(w.tab(), SettingsTab::Editor);
    assert_eq!(w.highlighted().as_deref(), Some("editor.cursor_blink"));
}

// spec: CFG-073
#[test]
fn cfg_073_a_picked_setting_is_boxed_for_a_moment_and_the_box_fades() {
    let mut w = W::new("pulse", TALL);
    w.open(SettingsTab::Vault);
    w.search("blinking");
    w.key(Key::Enter, Modifiers::NONE);
    assert_eq!(w.tab(), SettingsTab::Editor);
    let (id, k) = anchor::active(&w.ctx).expect("a box");
    assert_eq!(id, "editor.cursor_blink");
    assert!(k > 0.0 && k <= 1.0);
    let rect = anchor::seen(&w.ctx).into_iter().find(|(i, _)| i == "editor.cursor_blink").expect("the setting was drawn with its anchor").1;
    assert!(rect.width() > 20.0 && w.dialog_rect().contains_rect(rect), "{rect:?} is in the dialog {:?}", w.dialog_rect());
    // it lasts, then it is gone
    w.frames(10);
    assert!(w.highlighted().is_some(), "still there after half a second");
    w.frames((anchor::DURATION / 0.05) as usize + 5);
    assert_eq!(w.highlighted(), None, "gone after {} seconds", anchor::DURATION);
    // a new pick starts it again
    w.open(SettingsTab::Vault);
    w.search("number headings");
    w.key(Key::Enter, Modifiers::NONE);
    assert_eq!(w.highlighted().as_deref(), Some("editor.numbered_headings"));
}

// spec: CFG-073
#[test]
fn cfg_073_without_animation_the_box_stays_as_it_is() {
    let mut w = W::new("still", TALL);
    w.ctx.global_style_mut(|s| s.animation_time = 0.0);
    w.open(SettingsTab::Vault);
    w.search("blinking");
    w.key(Key::Enter, Modifiers::NONE);
    for _ in 0..12 {
        w.frames(1);
        assert_eq!(anchor::active(&w.ctx).map(|a| a.1), Some(1.0), "at every moment, whatever the phase of a pulse would be");
    }
}

// spec: CFG-073
#[test]
fn cfg_073_a_setting_far_down_a_page_is_scrolled_into_view() {
    let mut w = W::new("scroll", Vec2::new(900.0, 520.0));
    w.open(SettingsTab::Vault);
    w.search("highlight colours");
    w.key(Key::Enter, Modifiers::NONE);
    w.frames(30);
    assert_eq!(w.tab(), SettingsTab::Editor);
    let rect = anchor::seen(&w.ctx).into_iter().find(|(i, _)| i == "editor.highlight_colours").expect("drawn").1;
    let dialog = w.dialog_rect();
    assert!(dialog.height() < 520.0 && rect.top() > dialog.top() && rect.bottom() < dialog.bottom(), "{rect:?} is inside the dialog {dialog:?}");
}

// spec: CFG-073
#[test]
fn cfg_073_a_setting_that_is_not_drawn_pulses_the_one_that_shows_it() {
    let mut w = W::new("fallback", TALL);
    assert!(w.app.cfg.fonts.same_everywhere, "one font for all areas: the font of mail is not drawn");
    w.open(SettingsTab::Vault);
    w.search("font: mail");
    w.key(Key::Enter, Modifiers::NONE);
    w.frames(3);
    assert_eq!(w.tab(), SettingsTab::Fonts);
    assert_eq!(w.highlighted().as_deref(), Some("fonts.same"));
}

// spec: CFG-072, CFG-073
#[test]
fn cfg_072_a_key_action_is_found_by_its_keys_and_the_jump_shows_its_row() {
    let mut w = W::new("action", TALL);
    w.open(SettingsTab::Vault);
    w.search("F6");
    assert!(w.painted_start("Focus the next pane"), "found by the key it is bound to");
    w.key(Key::Enter, Modifiers::NONE);
    w.frames(2);
    assert_eq!(w.tab(), SettingsTab::Keybindings);
    assert_eq!(w.highlighted().as_deref(), Some("key:global.next_pane"));
    assert!(anchor::seen(&w.ctx).iter().any(|(i, r)| i == "key:global.next_pane" && r.width() > 100.0), "the row of the action is the anchor");
}

// spec: CFG-072
#[test]
fn cfg_072_a_result_hidden_by_the_filter_of_the_keybindings_tab_clears_it() {
    let mut w = W::new("reveal", TALL);
    w.open(SettingsTab::Keybindings);
    w.key(Key::F, Modifiers::COMMAND);
    w.type_text("ctrl+h");
    assert_eq!(w.filter(), "ctrl+h");
    // a result of the search of the whole dialog for another action
    let hits = registry::search("F6", &w.app.keymap);
    let hit = hits.iter().find(|h| h.action == Some(Action::NextPane)).unwrap().clone();
    if let Some(Modal::Settings { find, tab, keys, .. }) = &mut w.app.modal {
        find.pick(&w.ctx, &hit, tab, keys, &w.app.keymap);
    }
    w.frames(3);
    assert_eq!(w.filter(), "", "the filter hid the row, so it is cleared");
    assert!(w.painted("Focus the next pane"));
    // a filter that shows the row stays
    w.key(Key::F, Modifiers::COMMAND);
    w.type_text("next pane");
    let hit = hits.iter().find(|h| h.action == Some(Action::NextPane)).unwrap().clone();
    if let Some(Modal::Settings { find, tab, keys, .. }) = &mut w.app.modal {
        find.pick(&w.ctx, &hit, tab, keys, &w.app.keymap);
    }
    w.frames(3);
    assert_eq!(w.filter(), "next pane");
}

// spec: KEY-006
#[test]
fn key_006_the_filter_keeps_the_actions_that_fit_and_the_groups_with_them() {
    let mut w = W::new("filter", TALL);
    w.open(SettingsTab::Keybindings);
    assert!(w.painted("Mail view") && w.painted("Everywhere") && w.painted("Edit / reading view"), "no filter: the whole list");
    w.key(Key::F, Modifiers::COMMAND);
    w.type_text("ctrl+h");
    assert!(w.painted("Keyboard one place to the left (pane, then left sidebar)"), "found by its keys, case ignored");
    assert!(!w.painted("Edit / reading view"));
    assert!(w.painted("Everywhere"), "the group of a match keeps its heading");
    assert!(!w.painted("Mail view"), "a group with no match is hidden");
    // by label, case ignored
    w.key(Key::A, Modifiers::COMMAND);
    w.type_text("READING");
    assert!(w.painted("Edit / reading view") && !w.painted("Keyboard one place to the left (pane, then left sidebar)"));
    // F6, the way it is shown
    w.key(Key::A, Modifiers::COMMAND);
    w.type_text("F6");
    assert!(w.painted("Focus the next pane"));
    // nothing fits
    w.key(Key::A, Modifiers::COMMAND);
    w.type_text("qqqq");
    assert!(w.painted("No action matches") && !w.painted("Everywhere"));
    // empty: all again
    w.key(Key::A, Modifiers::COMMAND);
    w.key(Key::Backspace, Modifiers::NONE);
    assert!(w.painted("Mail view") && w.painted("Edit / reading view") && !w.painted("No action matches"));
}

// spec: KEY-006
#[test]
fn key_006_the_columns_stay_aligned_when_a_filter_hides_rows() {
    let mut w = W::new("aligned", TALL);
    w.open(SettingsTab::Keybindings);
    w.key(Key::F, Modifiers::COMMAND);
    w.type_text("ctrl");
    let x = |w: &W, k: &str| match &w.app.modal {
        Some(Modal::Settings { keys, .. }) => keys.rects.iter().find(|r| r.0 == k).map(|r| r.1.left()),
        _ => None,
    };
    let (a, b) = (x(&w, "key:global.focus_sidebar").unwrap(), x(&w, "key:global.toggle_reading").unwrap());
    assert_eq!(a, b, "the Change buttons of two rows are in one column");
}

// spec: KEY-006
#[test]
fn key_006_change_and_reset_work_on_filtered_rows_and_reset_all_resets_every_action() {
    let mut w = W::new("rows", TALL);
    w.open(SettingsTab::Keybindings);
    let click = |w: &mut W, k: &str| {
        let r = match &w.app.modal {
            Some(Modal::Settings { keys, .. }) => keys.rects.iter().find(|r| r.0 == k).unwrap_or_else(|| panic!("no control {k}")).1,
            _ => panic!(),
        };
        w.click_at(r.center());
    };
    // clear another action while all are shown, then filter to a single one
    click(&mut w, "clear:global.toggle_reading");
    assert!(w.app.keymap.bindings(Action::ToggleReading).is_empty());
    w.key(Key::F, Modifiers::COMMAND);
    w.type_text("next pane");
    click(&mut w, "clear:global.next_pane");
    assert!(w.app.keymap.bindings(Action::NextPane).is_empty(), "Clear works on a filtered row");
    click(&mut w, "reset:global.next_pane");
    assert_eq!(w.app.keymap.display(Action::NextPane), "F6", "Reset works on it too");
    click(&mut w, "clear:global.next_pane");
    // Reset all puts back every action, also the one that the filter does not show
    click(&mut w, "reset-all");
    assert_eq!(w.app.keymap.display(Action::NextPane), "F6");
    assert_eq!(w.app.keymap.display(Action::ToggleReading), "Ctrl+E");
    assert!(w.app.cfg.keybindings.is_empty(), "nothing is left over in the settings");
}

// spec: KEY-006
#[test]
fn key_006_while_a_binding_is_recorded_the_keys_go_to_the_recorder_not_the_fields() {
    let mut w = W::new("record", TALL);
    w.open(SettingsTab::Keybindings);
    w.key(Key::F, Modifiers::COMMAND);
    w.type_text("reading");
    let r = match &w.app.modal {
        Some(Modal::Settings { keys, .. }) => keys.rects.iter().find(|r| r.0 == "key:global.toggle_reading").unwrap().1,
        _ => panic!(),
    };
    w.click_at(r.center());
    assert!(matches!(&w.app.modal, Some(Modal::Settings { keys, .. }) if keys.recording.is_some()));
    // Ctrl+F is a chord now; the text is no text of a field
    w.key(Key::F, Modifiers::COMMAND);
    w.key(Key::Slash, Modifiers::NONE);
    w.type_text("x");
    assert_eq!(w.filter(), "reading", "the filter was not typed into");
    assert_eq!(w.query(), "", "the search of the dialog was not typed into");
    let chords = match &w.app.modal {
        Some(Modal::Settings { keys, .. }) => keys.recording.as_ref().map(|r| r.1.len()),
        _ => None,
    };
    assert_eq!(chords, Some(2), "Ctrl+F and / were recorded: {chords:?}");
    w.key(Key::Escape, Modifiers::NONE);
    assert!(matches!(&w.app.modal, Some(Modal::Settings { keys, .. }) if keys.recording.is_none()), "Esc cancels the recording");
    assert!(w.app.modal.is_some(), "and does not close the dialog");
}

// spec: KEY-006
#[test]
fn key_006_esc_in_the_filter_clears_it_before_the_dialog_closes() {
    let mut w = W::new("esc-filter", TALL);
    w.open(SettingsTab::Keybindings);
    w.key(Key::F, Modifiers::COMMAND);
    w.type_text("reading");
    w.key(Key::Escape, Modifiers::NONE);
    assert_eq!(w.filter(), "");
    assert!(w.app.modal.is_some());
    w.key(Key::Escape, Modifiers::NONE);
    assert!(w.app.modal.is_none());
}

// spec: CFG-071
#[test]
fn cfg_071_every_anchor_a_page_draws_belongs_to_the_tab_of_its_row() {
    let mut w = W::new("tabs", TALL);
    let mut total = 0;
    for tab in SettingsTab::ALL {
        w.open(tab);
        w.frames(3);
        let seen = anchor::seen(&w.ctx);
        for (id, rect) in &seen {
            let row_tab = match id.strip_prefix("key:") {
                Some(_) => SettingsTab::Keybindings,
                None => registry::entry(id).unwrap_or_else(|| panic!("{id} is drawn on {tab:?} but has no row")).tab,
            };
            assert_eq!(row_tab, tab, "{id} is drawn on the tab {tab:?} but its row says {row_tab:?}");
            assert!(rect.is_finite() && rect.width() > 0.0, "{id}: {rect:?}");
        }
        total += seen.len();
        if tab == SettingsTab::Keybindings {
            assert_eq!(seen.len(), crate::keys::DEFS.len(), "one anchor per action");
        } else {
            assert!(!seen.is_empty(), "{tab:?} draws no anchor");
        }
    }
    assert!(total > 100, "{total} anchors in all");
}
