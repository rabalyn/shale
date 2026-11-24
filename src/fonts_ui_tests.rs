//! The fonts of the areas (CFG-030 to CFG-040), driven through the whole window (headless).
//! The installed fonts are a list of our own (the bundled Virgil), never what is on the machine.

use crate::app::{Options, SettingsTab, ShaleApp, View};
use crate::config::testing::TestConfig;
use crate::fonts::{Area, FontsCfg};
use crate::ui_test_support::Headless;
use eframe::egui::{self, Event, FontFamily, Key, Modifiers, PointerButton, TextStyle};
use std::path::PathBuf;

struct F {
    app: ShaleApp,
    hl: Headless,
    dir: PathBuf,
    /// the family name Virgil has among the "installed" fonts
    virgil: String,
    _t: TestConfig,
}

impl F {
    fn new(tag: &str) -> F {
        let t = TestConfig::new(tag);
        let dir = std::env::temp_dir().join(format!("shale-fonts-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        crate::vault::write_sample_vault(&dir).unwrap();
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
        app.persist_config = true;
        app.save_config();
        app.settings_file_baseline();
        let (db, virgil) = crate::fonts::tests::virgil_db();
        app.fonts.system.set(db);
        let mut f = F { app, hl: Headless::new(), dir, virgil, _t: t };
        f.hl.frames(&mut f.app, 3);
        f
    }

    fn frames(&mut self, n: usize) {
        self.hl.frames(&mut self.app, n);
    }

    /// Every text of a frame with the size and family it is drawn in.
    fn texts(&mut self) -> Vec<(String, f32, FontFamily)> {
        fn walk(s: &egui::Shape, out: &mut Vec<(String, f32, FontFamily)>) {
            match s {
                egui::Shape::Text(t) => {
                    if let Some(sec) = t.galley.job.sections.first() {
                        out.push((t.galley.text().to_string(), sec.format.font_id.size, sec.format.font_id.family.clone()));
                    }
                }
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let out = self.hl.frame(&mut self.app, vec![]);
        let mut texts = vec![];
        out.shapes.iter().for_each(|c| walk(&c.shape, &mut texts));
        texts
    }

    fn key(&mut self, key: Key) {
        self.hl.frame(&mut self.app, vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
        self.hl.frame(&mut self.app, vec![Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE }]);
        self.frames(2);
    }

    fn click(&mut self, label: &str) {
        let r = self.hl.find_label(&mut self.app, label);
        self.hl.click(&mut self.app, r.center(), PointerButton::Primary);
    }

    fn body(&self) -> f32 {
        self.hl.ctx().global_style().text_styles[&TextStyle::Body].size
    }

    fn toasts(&self) -> Vec<String> {
        self.app.toasts.iter().map(|(m, _)| m.clone()).collect()
    }
}

// spec: CFG-031
#[test]
fn cfg_031_the_checkbox_shares_one_size_or_gives_every_area_its_own() {
    let mut f = F::new("same");
    assert!(f.app.cfg.fonts.same_everywhere);
    f.app.open_settings(SettingsTab::Fonts);
    f.frames(3);
    // shared: one row
    assert!(f.hl.try_label(&mut f.app, "Editor: raw markdown").is_none(), "one row while everything is shared");
    f.app.cfg.fonts.all.size = 20.0;
    f.frames(4);
    let sizes: Vec<f32> = Area::ALL.iter().map(|a| f.app.fonts.area(*a).size).collect();
    assert_eq!(sizes, [20.0, 20.0 * 15.5 / 14.0, 20.0 * 15.5 / 14.0, 20.0, 20.0, 20.0], "every area follows");
    assert_eq!(f.body(), 20.0);
    assert!(f.texts().iter().any(|(t, s, _)| t == "Use the same font and size everywhere" && *s == 20.0), "the dialog is drawn at the size");
    // the checkbox off: a row for every area, starting from what they had
    f.click("Use the same font and size everywhere");
    assert!(!f.app.cfg.fonts.same_everywhere);
    assert_eq!(f.app.cfg.fonts.mail.size, 20.0);
    for area in Area::ALL {
        assert!(f.hl.try_label(&mut f.app, area.label()).is_some() || f.texts().iter().any(|(t, _, _)| t == area.label()), "a row for {}", area.label());
    }
    f.app.cfg.fonts.editor_raw.size = 12.0;
    f.app.cfg.fonts.editor_rendered.size = 24.0;
    f.app.cfg.fonts.ui.size = 16.0;
    f.frames(4);
    let e = f.app.fonts.editor();
    assert_eq!((e.raw.size, e.rendered.size), (12.0, 24.0), "the editor's two fonts differ");
    assert_eq!(f.body(), 16.0, "dialogs have their own size");
    assert_eq!(f.app.fonts.area(Area::Mail).size, 20.0);
    // on again: the shared size is used again
    f.click("Use the same font and size everywhere");
    f.frames(3);
    assert!(f.app.cfg.fonts.same_everywhere);
    assert_eq!((f.app.fonts.editor().raw.size, f.body()), (20.0 * 15.5 / 14.0, 20.0));
    let _ = std::fs::remove_dir_all(&f.dir);
}

// spec: CFG-033
#[test]
fn cfg_033_mail_calendar_and_feeds_have_their_own_size() {
    for (area, view) in [(Area::Mail, View::Mail), (Area::Calendar, View::Calendar), (Area::Social, View::Social(None))] {
        let mut f = F::new(&format!("area-{}", area.label()));
        f.app.show_view(view, false);
        f.frames(4);
        let biggest = |t: &[(String, f32, FontFamily)]| t.iter().map(|(_, s, _)| *s).fold(0.0, f32::max);
        let before = biggest(&f.texts());
        assert!(before < 26.0, "{area:?}: {before}");
        f.app.cfg.fonts.separate();
        f.app.cfg.fonts.slot_mut(area).size = 30.0;
        f.frames(4);
        let after = f.texts();
        assert!(biggest(&after) >= 28.0, "{area:?}: the view is drawn at its size, the biggest text is {}", biggest(&after));
        assert_eq!(f.body(), 14.0, "{area:?}: the dialogs keep theirs");
        let _ = std::fs::remove_dir_all(&f.dir);
    }
}

// spec: CFG-032
#[test]
fn cfg_032_the_picker_filters_and_picks_by_keyboard() {
    let mut f = F::new("picker");
    f.app.open_settings(SettingsTab::Fonts);
    f.frames(3);
    // the button shows the font: Default
    f.click("Default  ⏷");
    f.frames(3);
    let listed: Vec<String> = f.texts().into_iter().map(|(t, _, _)| t).collect();
    assert!(listed.iter().any(|t| t == "Default  ✓") && listed.contains(&f.virgil), "the list has Default and the installed fonts: {listed:?}");
    // Esc closes it and picks nothing
    f.key(Key::Escape);
    assert!(f.app.cfg.fonts.all.family.is_none());
    assert!(f.app.modal.is_some(), "the dialog stays");
    // type to filter, Enter picks the first match
    f.click("Default  ⏷");
    f.frames(3);
    let part = f.virgil.to_lowercase().chars().take(3).collect::<String>();
    f.hl.frame(&mut f.app, vec![Event::Text(part)]);
    f.frames(2);
    let listed: Vec<String> = f.texts().into_iter().map(|(t, _, _)| t).collect();
    assert!(!listed.iter().any(|t| t == "Default  ✓"), "the Default entry is filtered out: {listed:?}");
    f.key(Key::Enter);
    assert_eq!(f.app.cfg.fonts.all.family.as_deref(), Some(f.virgil.as_str()));
    f.frames(3);
    // used at once, and not just in the picker
    assert_eq!(f.app.fonts.area(Area::Ui).family, FontFamily::Name(format!("system:{}", f.virgil).into()));
    assert!(f.texts().iter().any(|(t, _, fam)| t == "Use the same font and size everywhere" && *fam == FontFamily::Name(format!("system:{}", f.virgil).into())), "the dialog is drawn in the font");
    // the list opens on the chosen font (CFG-040); arrow keys move and wrap around, Enter picks:
    // down from the last font is Default
    f.click(&format!("{}  ⏷", f.virgil));
    f.frames(3);
    f.key(Key::ArrowDown);
    f.key(Key::Enter);
    assert_eq!(f.app.cfg.fonts.all.family, None);
    f.click("Default  ⏷");
    f.frames(3);
    f.key(Key::ArrowDown);
    f.key(Key::Enter);
    assert_eq!(f.app.cfg.fonts.all.family.as_deref(), Some(f.virgil.as_str()), "down from Default is the first font");
    let _ = std::fs::remove_dir_all(&f.dir);
}

// spec: CFG-077
#[test]
fn cfg_077_an_open_picker_draws_its_entries_in_their_own_fonts_once_the_list_rests() {
    let mut f = F::new("list-fonts");
    f.app.open_settings(SettingsTab::Fonts);
    f.frames(3);
    f.click("Default  ⏷");
    f.frames(3);
    let own = FontFamily::Name(format!("system:{}", f.virgil).into());
    // the name waits in Shale's font until the list has rested (CFG-077 unit tests); then the worker has read the face and egui has it
    let mut drawn = false;
    for _ in 0..200 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        f.frames(2);
        drawn = f.texts().iter().any(|(t, _, fam)| *t == f.virgil && *fam == own);
        if drawn {
            break;
        }
    }
    assert!(drawn, "the entry is drawn in its own font");
    assert!(f.texts().iter().any(|(t, _, fam)| t == "Default  ✓" && *fam != own), "Default stays in Shale's font");
    let _ = std::fs::remove_dir_all(&f.dir);
}

// spec: CFG-035
#[test]
fn cfg_035_a_font_that_is_not_installed_falls_back_and_says_so() {
    let mut f = F::new("missing");
    f.app.cfg.fonts.all.family = Some("No Such Font".into());
    f.frames(5);
    assert!(f.toasts().iter().any(|m| m.contains("No Such Font") && m.contains("not installed")), "{:?}", f.toasts());
    assert_eq!(f.app.fonts.area(Area::Ui).family, FontFamily::Proportional);
    assert!(f.texts().iter().all(|(_, _, fam)| *fam != FontFamily::Name("system:No Such Font".into())));
    // the dialog marks it
    f.app.open_settings(SettingsTab::Fonts);
    f.frames(3);
    assert!(f.texts().iter().any(|(t, _, _)| t == "not installed"));
    let _ = std::fs::remove_dir_all(&f.dir);
}

// spec: CFG-036
#[test]
fn cfg_036_editing_the_fonts_in_the_settings_file_and_saving_applies_them() {
    let mut f = F::new("file");
    f.app.open_settings_file();
    assert!(f.app.sf.text.contains("[fonts.all]"), "{}", f.app.sf.text);
    let edited = f.app.sf.text.replacen("size = 14.0", "size = 21.0", 1);
    assert_ne!(edited, f.app.sf.text);
    f.app.sf.text = edited.replace("[fonts.all]\n", &format!("[fonts.all]\nfamily = \"{}\"\n", f.virgil));
    f.app.sf.dirty = true;
    assert!(f.app.save_settings_file(true));
    f.frames(4);
    assert_eq!(f.app.cfg.fonts.all.size, 21.0);
    assert_eq!(f.body(), 21.0, "used at once");
    assert_eq!(f.app.fonts.area(Area::Ui).family, FontFamily::Name(format!("system:{}", f.virgil).into()));
    // the settings file keeps what was typed
    let on_disk = std::fs::read_to_string(crate::config::config_path().unwrap()).unwrap();
    assert!(on_disk.contains("size = 21.0") && on_disk.contains(&f.virgil), "{on_disk}");
    assert_eq!(FontsCfg::default().all.size, 14.0);
    let _ = std::fs::remove_dir_all(&f.dir);
}

// spec: CFG-037
#[test]
fn cfg_037_the_editor_draws_raw_text_and_the_reading_view_rendered_text_at_their_own_sizes() {
    let mut f = F::new("editor");
    f.app.open(std::path::Path::new("Welcome.md"), false);
    f.frames(4);
    f.app.notes.get_mut(std::path::Path::new("Welcome.md")).unwrap().text = "plain words here\n".into();
    f.app.cfg.fonts.separate();
    f.app.cfg.fonts.editor_raw.size = 12.0;
    f.app.cfg.fonts.editor_rendered.size = 26.0;
    f.frames(4);
    let size_of = |f: &mut F| f.texts().into_iter().find(|(t, _, _)| t.starts_with("plain words")).map(|(_, s, _)| s);
    assert_eq!(size_of(&mut f), Some(12.0), "the editor shows raw text at the raw size");
    f.app.tabbar.tabs[f.app.tabbar.active].reading = true;
    f.frames(4);
    assert_eq!(size_of(&mut f), Some(26.0), "the reading view shows rendered text at the rendered size");
    let _ = std::fs::remove_dir_all(&f.dir);
}

// spec: CFG-040
#[test]
fn cfg_040_every_row_has_a_sample_that_follows_the_highlighted_font_until_one_is_picked() {
    let mut f = F::new("sample");
    f.app.open_settings(SettingsTab::Fonts);
    f.frames(3);
    let virgil = FontFamily::Name(format!("system:{}", f.virgil).into());
    let samples = |f: &mut F| -> Vec<(f32, FontFamily)> { f.texts().into_iter().filter(|(t, _, _)| t.starts_with(crate::settings::fonts::SAMPLE[0].trim_end())).map(|(_, s, fam)| (s, fam)).collect() };
    assert_eq!(samples(&mut f), [(14.0, FontFamily::Proportional)], "one row, Shale's own font at the row's size");
    // the list open: the mouse over an entry shows it in the sample, without picking it
    f.click("Default  ⏷");
    f.frames(3);
    let at = f.hl.find_label(&mut f.app, &f.virgil.clone()).center();
    f.hl.frame(&mut f.app, vec![Event::PointerMoved(at)]);
    // (the font is loaded for the sample first, or the list has its face already, CFG-077)
    f.frames(3);
    assert_eq!(samples(&mut f), [(14.0, virgil.clone())]);
    assert!(f.app.cfg.fonts.all.family.is_none(), "nothing is picked");
    assert_eq!(f.app.fonts.area(Area::Ui).family, FontFamily::Proportional, "the areas keep their font");
    // the keyboard moves the highlight too; Esc goes back to the chosen font
    f.key(Key::ArrowUp);
    assert_eq!(samples(&mut f), [(14.0, FontFamily::Proportional)], "up to Default");
    f.key(Key::ArrowDown);
    assert_eq!(samples(&mut f), [(14.0, virgil.clone())]);
    f.key(Key::Escape);
    assert_eq!(samples(&mut f), [(14.0, FontFamily::Proportional)], "closed: the chosen font again");
    assert!(f.app.cfg.fonts.all.family.is_none());
    // a row for every area: each sample in its own font and size, all at once; a large size is capped
    f.app.cfg.fonts.separate();
    f.app.cfg.fonts.mail.family = Some(f.virgil.clone());
    f.app.cfg.fonts.editor_raw.size = 40.0;
    f.frames(4);
    let s = samples(&mut f);
    assert_eq!(s.len(), Area::ALL.len(), "{s:?}");
    assert_eq!(s.iter().filter(|(_, fam)| *fam == virgil).count(), 1, "the mail row: {s:?}");
    assert!(s.contains(&(28.0, FontFamily::Proportional)), "the raw editor's 40 points are drawn at 28: {s:?}");
    // a chosen font that is not installed has no sample, only the hint
    f.app.cfg.fonts.calendar.family = Some("No Such Font".into());
    f.frames(4);
    assert_eq!(samples(&mut f).len(), Area::ALL.len() - 1);
    assert!(f.texts().iter().any(|(t, _, _)| t == "not installed"));
    let _ = std::fs::remove_dir_all(&f.dir);
}

impl F {
    /// The place of a tab button of the Settings dialog.
    fn tab(&mut self, label: &str) -> egui::Rect {
        self.hl.find_label(&mut self.app, label)
    }

    /// The border between the tab list and the page: where it is drawn.
    fn tab_border(&mut self) -> egui::Pos2 {
        let r = self.tab("Vault");
        egui::pos2(r.right() + 6.5, r.center().y)
    }

    fn drag_tab_border(&mut self, by: f32) {
        let from = self.tab_border();
        let to = from + egui::vec2(by, 0.0);
        let path = Headless::steps(from, to, 5);
        self.hl.press_and_move(&mut self.app, from, &path);
    }
}

// spec: CFG-041
#[test]
fn cfg_041_no_tab_name_wraps_at_any_font_size_and_the_list_follows_the_size() {
    let mut f = F::new("tab-auto");
    f.app.open_settings(SettingsTab::Vault);
    f.frames(3);
    let widths = |f: &mut F| -> (f32, f32) {
        let heights = ["Vault", "Appearance", "Keybindings"].map(|l| f.tab(l).height());
        assert!(heights.iter().all(|h| *h == heights[0]), "every name on one line: {heights:?}");
        (f.tab("Keybindings").width(), heights[0])
    };
    let (w14, h14) = widths(&mut f);
    assert_eq!(w14, f.tab("Appearance").width().max(w14), "the list is as wide as its widest name");
    f.app.cfg.fonts.all.size = 30.0;
    f.frames(5);
    let (w30, h30) = widths(&mut f);
    assert!(w30 > w14 && h30 > h14, "a bigger font: the list is wider ({w14} -> {w30}) and no name wraps");
    let _ = std::fs::remove_dir_all(&f.dir);
}

// spec: CFG-041
#[test]
fn cfg_041_dragging_the_border_sets_the_width_when_the_drag_ends_and_a_double_click_resets_it() {
    let mut f = F::new("tab-drag");
    f.app.open_settings(SettingsTab::Vault);
    f.frames(3);
    let auto = f.tab("Vault").width();
    assert_eq!(f.app.cfg.settings_tab_width, None);
    // the resize cursor on the border
    let b = f.tab_border();
    f.hl.frame(&mut f.app, vec![Event::PointerMoved(b)]);
    f.hl.frame(&mut f.app, vec![Event::PointerMoved(b)]);
    assert_eq!(f.hl.cursor, egui::CursorIcon::ResizeHorizontal);
    // wider: the width follows the pointer, and is written only when the drag ends
    f.drag_tab_border(60.0);
    assert!((f.tab("Vault").width() - (auto + 60.0)).abs() <= 1.5, "{} vs {}", f.tab("Vault").width(), auto + 60.0);
    assert_eq!(f.app.cfg.settings_tab_width, None, "not written while dragging");
    let to = f.tab_border();
    f.hl.release(&mut f.app, to);
    let set = f.app.cfg.settings_tab_width.expect("written when the drag ends");
    assert!((set as f32 - (auto + 60.0)).abs() <= 1.5, "{set}");
    assert_eq!(f.tab("Vault").width(), set as f32, "and the list keeps it");
    assert!(crate::config::text::to_toml(&f.app.cfg).contains(&format!("settings_tab_width = {set}")), "and the file has it");
    // too far: half of the dialog at most
    f.drag_tab_border(2000.0);
    let to = f.tab_border();
    f.hl.release(&mut f.app, to);
    let most = f.app.cfg.settings_tab_width.unwrap();
    assert!((300..=400).contains(&most) && most > set, "about half of the 740 points of the dialog: {most}");
    // too narrow: 60 at least
    f.drag_tab_border(-2000.0);
    let to = f.tab_border();
    f.hl.release(&mut f.app, to);
    assert_eq!(f.app.cfg.settings_tab_width, Some(60));
    // a double-click on the border: automatic again
    let b = f.tab_border();
    f.hl.click(&mut f.app, b, PointerButton::Primary);
    f.hl.click(&mut f.app, b, PointerButton::Primary);
    assert_eq!(f.app.cfg.settings_tab_width, None);
    assert_eq!(f.tab("Vault").width(), auto);
    let _ = std::fs::remove_dir_all(&f.dir);
}
