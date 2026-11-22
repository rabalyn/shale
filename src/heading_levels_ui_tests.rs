//! The bar for headings that skip levels (EDIT-115, EDIT-116, EDIT-117) driven in the whole window.

use crate::app::{Options, ShaleApp};
use crate::ui_test_support::Headless;
use eframe::egui::{Event, PointerButton};
use std::path::{Path, PathBuf};

const NOTE: &str = "Welcome.md";
const WARN: &str = "Headings: ";

struct Fx {
    app: ShaleApp,
    hl: Headless,
    dir: PathBuf,
}

impl Fx {
    fn new(tag: &str, text: &str) -> Fx {
        let dir = std::env::temp_dir().join(format!("shale-hlevels-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        crate::vault::write_sample_vault(&dir).unwrap();
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
        app.open(Path::new(NOTE), false);
        let mut hl = Headless::new();
        hl.frames(&mut app, 4);
        app.notes.get_mut(Path::new(NOTE)).unwrap().text = text.to_string();
        hl.frames(&mut app, 3);
        Fx { app, hl, dir }
    }

    fn text(&self) -> String {
        self.app.notes.get(Path::new(NOTE)).unwrap().text.clone()
    }

    /// What the bar says, when it is on screen.
    fn bar(&mut self) -> Option<String> {
        self.hl.frames(&mut self.app, 2);
        self.hl.text.iter().find(|(t, _)| t.starts_with(WARN)).map(|(t, _)| t.clone())
    }

    fn click(&mut self, label: &str) {
        let r = self.hl.find_label(&mut self.app, label);
        self.hl.click(&mut self.app, r.center(), PointerButton::Primary);
        self.hl.frames(&mut self.app, 3);
    }

    fn has(&mut self, label: &str) -> bool {
        self.hl.try_label(&mut self.app, label).is_some()
    }

    fn undo(&mut self) -> String {
        let mut text = self.text();
        assert!(self.app.undo.of(Path::new(NOTE)).undo(&mut text).is_some());
        text
    }

    fn set_reading(&mut self, on: bool) {
        self.app.tabbar.tabs[self.app.tabbar.active].reading = on;
        self.hl.frames(&mut self.app, 3);
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

const DEEP: &str = "## Plan\ntext\n### Steps\n```\n## code\n```\n$$\n### math\n$$\n";

// spec: EDIT-115
#[test]
fn edit_115_the_bar_shows_above_the_text_in_the_editor_and_in_the_reading_view_and_only_when_levels_are_missing() {
    let mut fx = Fx::new("shown", DEEP);
    assert_eq!(fx.bar().as_deref(), Some("Headings: no level 1 heading"));
    let bar_at = fx.hl.text.iter().find(|(t, _)| t.starts_with(WARN)).map(|(_, p)| *p).unwrap();
    let text_at = fx.hl.text.iter().find(|(t, _)| t.contains("Plan")).map(|(_, p)| *p).expect("the note text is painted");
    assert!(bar_at.y < text_at.y, "the bar is above the text");
    fx.set_reading(true);
    assert_eq!(fx.bar().as_deref(), Some("Headings: no level 1 heading"), "also in the reading view");
    fx.set_reading(false);
    // levels start at 1 and have no gap: quiet; a gap is named; no headings: quiet
    fx.app.notes.get_mut(Path::new(NOTE)).unwrap().text = "# A\n## B\n### C\n".into();
    assert_eq!(fx.bar(), None);
    fx.app.notes.get_mut(Path::new(NOTE)).unwrap().text = "# A\n### C\n".into();
    assert_eq!(fx.bar().as_deref(), Some("Headings: level 2 is skipped"));
    fx.app.notes.get_mut(Path::new(NOTE)).unwrap().text = "no headings\n".into();
    assert_eq!(fx.bar(), None);
}

// spec: EDIT-116
#[test]
fn edit_116_the_button_moves_all_headings_up_in_one_undo_step_and_a_gap_stays_named() {
    let mut fx = Fx::new("shift", DEEP);
    assert!(fx.has("Move all up by 1"));
    fx.click("Move all up by 1");
    assert_eq!(fx.text(), "# Plan\ntext\n## Steps\n```\n## code\n```\n$$\n### math\n$$\n", "code and math stay");
    assert_eq!(fx.bar(), None, "levels 1 and 2: nothing to warn about");
    assert!(fx.app.notes.get(Path::new(NOTE)).unwrap().dirty, "the note is saved like after any edit");
    assert_eq!(fx.undo(), DEEP, "one undo step takes the whole move back");
    // a gap that stays after the move is named
    let mut fx = Fx::new("gap", "## A\n#### B\n");
    assert_eq!(fx.bar().as_deref(), Some("Headings: no level 1 heading, level 3 is skipped"));
    fx.click("Move all up by 1");
    assert_eq!(fx.text(), "# A\n### B\n");
    assert_eq!(fx.bar().as_deref(), Some("Headings: level 2 is skipped"));
    assert!(!fx.has("Move all up by 1"), "no button when it would move nothing");
    // deeper by 2: the move is by 2
    let mut fx = Fx::new("by2", "### A\n#### B\n");
    fx.click("Move all up by 2");
    assert_eq!(fx.text(), "# A\n## B\n");
}

// spec: EDIT-116
#[test]
fn edit_116_a_gap_without_a_shift_has_no_button() {
    let mut fx = Fx::new("nobutton", "# A\n### B\n");
    assert_eq!(fx.bar().as_deref(), Some("Headings: level 2 is skipped"));
    assert!(!fx.has("Move all up by 0") && !fx.has("Move all up by 1"));
    assert!(fx.has("×"), "the cross is there");
}

// spec: EDIT-116
#[test]
fn edit_116_the_button_works_in_the_reading_view() {
    let mut fx = Fx::new("reading", DEEP);
    fx.set_reading(true);
    fx.click("Move all up by 1");
    assert!(fx.text().starts_with("# Plan\ntext\n## Steps\n"));
    assert_eq!(fx.bar(), None);
    assert_eq!(fx.undo(), DEEP);
}

// spec: EDIT-116
#[test]
fn edit_116_with_vim_lite_on_the_button_works_and_u_undoes_it() {
    let mut fx = Fx::new("vim", DEEP);
    fx.app.cfg.vim_mode = true;
    fx.hl.frames(&mut fx.app, 3);
    fx.app.tabbar.tabs[fx.app.tabbar.active].editor.request_focus = true;
    fx.hl.frames(&mut fx.app, 3);
    fx.click("Move all up by 1");
    assert!(fx.text().starts_with("# Plan\ntext\n## Steps\n"));
    fx.hl.frames(&mut fx.app, 3);
    assert_eq!(fx.app.tabbar.tabs[fx.app.tabbar.active].editor.vim_status().as_deref(), Some("NORMAL"));
    fx.hl.frame(&mut fx.app, vec![Event::Text("u".into())]);
    fx.hl.frames(&mut fx.app, 2);
    assert_eq!(fx.text(), DEEP, "u takes the whole move back");
}

// spec: EDIT-117
#[test]
fn edit_117_the_cross_hides_the_bar_for_the_note_in_every_tab_and_view_and_it_stays_hidden() {
    let mut fx = Fx::new("dismiss", "## A\n### B\n");
    assert!(fx.bar().is_some());
    fx.click("×");
    assert_eq!(fx.bar(), None, "hidden");
    // another warning, the other view, a second tab of the same note: still hidden
    fx.app.notes.get_mut(Path::new(NOTE)).unwrap().text = "#### A\n".into();
    assert_eq!(fx.bar(), None);
    fx.set_reading(true);
    assert_eq!(fx.bar(), None);
    fx.set_reading(false);
    fx.app.open(Path::new(NOTE), true);
    assert_eq!(fx.bar(), None, "also in a new tab of the note");
    // another note has its own bar
    fx.app.open(Path::new("Ideas.md"), false);
    fx.hl.frames(&mut fx.app, 3);
    fx.app.notes.get_mut(Path::new("Ideas.md")).unwrap().text = "## X\n".into();
    assert!(fx.bar().is_some(), "another note");
}
