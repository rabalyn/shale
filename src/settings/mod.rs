//! The Settings dialog (VAULT-023, CFG-020): a list of tabs on the left, one page per domain on
//! the right, and a header row above the page for buttons that belong to the whole dialog.
//! Every page is a function of `ShaleApp` in its own file; this file holds the frame, the tab
//! list and what a page can ask the dialog to do.

pub(crate) mod anchor;
mod appearance;
mod data;
mod editor;
pub(crate) mod find;
mod font_picker;
pub(crate) mod fonts;
mod help;
pub(crate) mod import;
mod latex;
mod mail_social;
mod publishing;
pub(crate) mod registry;
mod search;
mod sync_calendar;
mod vault;
mod vim;

#[cfg(test)]
mod find_tests;

pub use find::Find;

use crate::app::ShaleApp;
use crate::keys_ui::KeyDialog;
use eframe::egui::{self, Context, Id, Ui};

/// The tabs of the Settings dialog (CFG-020, KEY-006).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettingsTab {
    /// The vault folder, closing it and the people folder: the short page Settings opens on.
    Vault,
    Editor,
    /// A sub-category of Editor: listed right after it and drawn indented.
    Vim,
    Appearance,
    Fonts,
    Sync,
    /// The vault as a Hugo site: the switch, the rules, the target and Publish (SITE-051).
    Publishing,
    Calendar,
    Mail,
    Social,
    Search,
    Import,
    Data,
    Latex,
    Updates,
    Help,
    Keybindings,
}

impl SettingsTab {
    /// How far the name of a sub-category is moved to the right in the tab list (points).
    pub const SUB_INDENT: f32 = 14.0;

    /// The tab is a sub-category of the one before it (drawn indented).
    pub fn is_sub(self) -> bool {
        self == SettingsTab::Vim
    }

    /// All tabs in the order of the list.
    pub const ALL: [SettingsTab; 17] = [
        SettingsTab::Vault,
        SettingsTab::Editor,
        SettingsTab::Vim,
        SettingsTab::Appearance,
        SettingsTab::Fonts,
        SettingsTab::Sync,
        SettingsTab::Publishing,
        SettingsTab::Calendar,
        SettingsTab::Mail,
        SettingsTab::Social,
        SettingsTab::Search,
        SettingsTab::Import,
        SettingsTab::Data,
        SettingsTab::Latex,
        SettingsTab::Updates,
        SettingsTab::Help,
        SettingsTab::Keybindings,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SettingsTab::Vault => "Vault",
            SettingsTab::Editor => "Editor",
            SettingsTab::Vim => "Vim",
            SettingsTab::Appearance => "Appearance",
            SettingsTab::Fonts => "Fonts",
            SettingsTab::Sync => "Sync",
            SettingsTab::Publishing => "Publishing",
            SettingsTab::Calendar => "Calendar",
            SettingsTab::Mail => "Mail",
            SettingsTab::Social => "Feeds",
            SettingsTab::Search => "Search",
            SettingsTab::Import => "Import",
            SettingsTab::Data => "Data",
            SettingsTab::Latex => "LaTeX",
            SettingsTab::Updates => "Updates",
            SettingsTab::Help => "Help",
            SettingsTab::Keybindings => "Keybindings",
        }
    }
}

impl SettingsTab {
    /// One line above the page: what it is for.
    fn about(self) -> &'static str {
        match self {
            SettingsTab::Vault => "The folder this vault lives in, where its contacts are, and its bibliography",
            SettingsTab::Editor => "How notes are edited: the cursor, links, tables and the toolbar",
            SettingsTab::Vim => "Vim-style editing: the switch, the cursor of normal mode and the link preview",
            SettingsTab::Appearance => "How notes, drawings and media look",
            SettingsTab::Fonts => "The font and text size of the editor, dialogs, mail, calendar and feeds",
            SettingsTab::Sync => "Keep this vault in a git repository",
            SettingsTab::Publishing => "Publish notes of this vault as a website: which notes, and where",
            SettingsTab::Calendar => "Calendars and the week",
            SettingsTab::Mail => "Fetching and reading mail",
            SettingsTab::Social => "Feeds, new posts and the post search",
            SettingsTab::Search => "Which sources the search looks in, and the letters that choose one in a query",
            SettingsTab::Import => "What the Import takes in, and what happens to files from outside the vault",
            SettingsTab::Data => "What Shale keeps on disk, and for how long",
            SettingsTab::Latex => "The TeX that exports notes to PDF through LaTeX templates",
            SettingsTab::Updates => "New releases: whether Shale looks for them, downloads them or installs them",
            SettingsTab::Help => "The user guide and tools to find problems",
            SettingsTab::Keybindings => "Which keys do what",
        }
    }
}

/// How far to either side of the border between the tab list and the page its handle reaches
/// (points, CFG-041).
const TAB_GRAB: f32 = 4.0;

/// The width of the tab list when no width is set (CFG-041): the widest tab name laid out on one
/// line in the font of the tab buttons, plus the padding of a button, rounded up.
pub(crate) fn auto_tab_width(ui: &Ui) -> f32 {
    let widest = SettingsTab::ALL
        .iter()
        .map(|t| {
            let indent = if t.is_sub() { SettingsTab::SUB_INDENT } else { 0.0 };
            indent + egui::WidgetText::from(t.label()).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Button).size().x
        })
        .fold(0.0, f32::max);
    (widest + 2.0 * ui.spacing().button_padding.x).ceil()
}

/// A tab list width kept between 60 points and half of the dialog (CFG-041); where half of the
/// dialog is less than 60, 60 wins.
pub(crate) fn clamp_tab_width(width: f32, dialog_width: f32) -> f32 {
    width.min(dialog_width / 2.0).max(60.0)
}

/// What a page asked of the dialog: it closes it, usually to open another one.
#[derive(Default)]
pub struct Asks {
    /// Close Settings (a vault was opened or closed).
    pub close: bool,
    pub calendars: bool,
    pub mail: bool,
    pub social: bool,
    pub usage: bool,
    /// Open the list of the deleted notes (SYNC-040).
    pub history_deleted: bool,
    pub guide: bool,
    /// Open the guided setup of the LaTeX export (TEX-017).
    pub tex_setup: bool,
}

fn last_tab_id() -> Id {
    Id::unique("settings-last-tab")
}

/// The tab Settings was last on in this session (the Vault tab at first).
pub fn last_tab(ctx: &Context) -> SettingsTab {
    ctx.data(|d| d.get_temp(last_tab_id())).unwrap_or(SettingsTab::Vault)
}

impl ShaleApp {
    /// The content of the dialog: the tab list and the page of the chosen tab. What the page asks
    /// of the dialog is added to `asks`.
    pub fn settings_body(&mut self, ui: &mut Ui, vault_path: &mut String, tab: &mut SettingsTab, keys: &mut KeyDialog, asks: &mut Asks) {
        ui.ctx().data_mut(|d| d.insert_temp(last_tab_id(), *tab));
        anchor::begin(ui.ctx());
        let dialog_width = ui.available_width();
        let auto = auto_tab_width(ui);
        let drag_id = Id::unique("settings-tab-width-drag");
        let dragged: Option<f32> = ui.ctx().data(|d| d.get_temp(drag_id));
        let width = dragged.unwrap_or_else(|| self.cfg.settings_tab_width.map_or(auto, |w| clamp_tab_width(w as f32, dialog_width)));
        ui.horizontal_top(|ui| {
            let list = ui.vertical(|ui| {
                ui.set_width(width);
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                    for t in SettingsTab::ALL {
                        if t.is_sub() {
                            // a sub-category of the tab before it: moved to the right, no guide line
                            ui.scope(|ui| {
                                ui.spacing_mut().indent = SettingsTab::SUB_INDENT;
                                ui.visuals_mut().indent_has_left_vline = false;
                                ui.indent(("settings-sub-tab", t.label()), |ui| ui.selectable_value(tab, t, t.label()));
                            });
                        } else {
                            ui.selectable_value(tab, t, t.label());
                        }
                    }
                });
            });
            // the border between the list and the page: a handle (CFG-041)
            let (line, _) = ui.allocate_exact_size(egui::vec2(1.0, ui.available_height()), egui::Sense::hover());
            let grip = ui.interact(line.expand2(egui::vec2(TAB_GRAB, 0.0)), drag_id.with("grip"), egui::Sense::click_and_drag());
            let stroke = if grip.hovered() || grip.dragged() { ui.visuals().widgets.hovered.bg_stroke } else { ui.visuals().widgets.noninteractive.bg_stroke };
            ui.painter().vline(line.center().x, line.y_range(), stroke);
            if grip.hovered() || grip.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if grip.dragged() {
                if let Some(p) = grip.interact_pointer_pos() {
                    // the border stays under the pointer
                    let w = clamp_tab_width(p.x - (line.center().x - list.response.rect.right()) - list.response.rect.left(), dialog_width);
                    ui.ctx().data_mut(|d| d.insert_temp(drag_id, w));
                }
            }
            if grip.drag_stopped() {
                if let Some(w) = ui.ctx().data_mut(|d| d.remove_temp::<f32>(drag_id)) {
                    self.cfg.settings_tab_width = Some(w.round() as u32);
                    self.save_config();
                }
            } else if grip.double_clicked() {
                self.cfg.settings_tab_width = None;
                self.save_config();
            }
            ui.vertical(|ui| {
                if *tab != SettingsTab::Keybindings {
                    self.keymap.recording = false;
                }
                let changed = match *tab {
                    SettingsTab::Vault => self.vault_tab(ui, vault_path, asks),
                    SettingsTab::Editor => self.editor_tab(ui, tab),
                    SettingsTab::Vim => self.vim_tab(ui, tab),
                    SettingsTab::Appearance => self.appearance_tab(ui),
                    SettingsTab::Fonts => self.fonts_tab(ui),
                    SettingsTab::Sync => self.sync_tab(ui, asks),
                    SettingsTab::Publishing => self.publishing_tab(ui),
                    SettingsTab::Calendar => self.calendar_tab(ui, asks),
                    SettingsTab::Mail => self.mail_tab(ui, asks),
                    SettingsTab::Social => self.social_tab(ui, asks),
                    SettingsTab::Search => self.search_tab(ui),
                    SettingsTab::Import => self.import_tab(ui),
                    SettingsTab::Data => self.data_tab(ui, asks),
                    SettingsTab::Latex => self.latex_tab(ui, asks),
                    SettingsTab::Updates => self.updates_tab(ui),
                    SettingsTab::Help => self.help_tab(ui, asks),
                    SettingsTab::Keybindings => {
                        let edited = egui::ScrollArea::horizontal().auto_shrink([false, true]).show(ui, |ui| crate::keys_ui::show(ui, keys, &mut self.keymap)).inner;
                        if edited {
                            self.cfg.keybindings = self.keymap.overrides();
                        }
                        edited
                    }
                };
                if changed {
                    self.save_config();
                }
            });
        });
        anchor::end(ui.ctx());
    }

    /// What stays above the tabs and the page when the page scrolls: the search over all settings
    /// (CFG-072), and a row with what the page is for and room on the right for buttons of the
    /// whole dialog (add them in a `right_to_left` layout here). A search result that is picked
    /// changes `tab`.
    pub fn settings_header(&mut self, ui: &mut Ui, tab: &mut SettingsTab, keys: &mut KeyDialog, find: &mut Find, asks: &mut Asks) {
        // while a key is recorded or asked about, the keys belong to that (KEY-006)
        let locked = keys.capturing() || self.keymap.recording;
        find.show(ui, &self.keymap, tab, keys, locked);
        let tab = *tab;
        ui.horizontal(|ui| {
            crate::theme::hint(ui, tab.about());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Open settings file").on_hover_text("Edit all settings as text, with a description of each").clicked() {
                    self.open_settings_file();
                    asks.close = true;
                }
            });
        });
        ui.separator();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme;
    use eframe::egui::{RawInput, TextStyle};

    /// The automatic width with the tab buttons' font at this size, and the widest names' width.
    fn widths(size: f32) -> (f32, f32, f32) {
        let ctx = Context::default();
        ctx.global_style_mut(|s| s.text_styles.get_mut(&TextStyle::Button).unwrap().size = size);
        let (mut auto, mut keys, mut appearance) = (0.0, 0.0, 0.0);
        let mut out = ctx.run_ui(RawInput::default(), |ui| {
            auto = auto_tab_width(ui);
            let w = |s: &str| egui::WidgetText::from(s).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, TextStyle::Button).size().x;
            (keys, appearance) = (w("Keybindings"), w("Appearance"));
        });
        out.textures_delta.clear();
        (auto, keys, appearance)
    }

    // spec: NAV-042
    #[test]
    fn nav_042_a_section_heading_has_the_section_gap_above_it_and_the_item_gap_below_it() {
        let ctx = Context::default();
        let mut gaps = (0.0, 0.0);
        let mut out = ctx.run_ui(RawInput::default(), |ui| {
            theme::dialog_spacing(ui);
            let item = ui.label("An item");
            theme::section(ui, "A section");
            let next = ui.label("Its first item");
            let heading_height = ui.label(egui::RichText::new("A section").color(theme::palette().muted)).rect.height();
            // item, SECTION_GAP, heading, ITEM_GAP, next item
            let rest = next.rect.top() - item.rect.bottom() - heading_height;
            gaps = (rest - theme::ITEM_GAP, theme::ITEM_GAP);
        });
        out.textures_delta.clear();
        assert!((gaps.0 - theme::SECTION_GAP).abs() < 0.6, "the gap above the heading: {} of {}", gaps.0, theme::SECTION_GAP);
        assert!(gaps.0 > gaps.1, "the heading is closer to its own items than to the section before it");
    }

    // spec: CFG-041
    #[test]
    fn cfg_041_the_tab_width_stays_between_60_points_and_half_of_the_dialog() {
        assert_eq!(clamp_tab_width(100.0, 740.0), 100.0);
        assert_eq!(clamp_tab_width(10.0, 740.0), 60.0, "not under 60");
        assert_eq!(clamp_tab_width(500.0, 740.0), 370.0, "not over half of the dialog");
        assert_eq!(clamp_tab_width(80.0, 100.0), 60.0, "where half of the dialog is under 60, 60 wins");
    }

    // spec: CFG-041
    #[test]
    fn cfg_041_the_automatic_width_fits_the_widest_name_and_follows_the_font() {
        let (small, keys, appearance) = widths(14.0);
        assert!(small >= keys && small >= appearance, "{small} fits {keys} and {appearance}");
        assert_eq!(small, small.ceil(), "a whole number of points");
        let (big, keys_big, _) = widths(30.0);
        assert!(big >= keys_big && big > small, "a bigger font: {big} > {small}");
    }
}
