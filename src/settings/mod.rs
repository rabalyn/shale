//! The Settings dialog (VAULT-023, CFG-020): a list of tabs on the left, one page per domain on
//! the right, and a header row above the page for buttons that belong to the whole dialog.
//! Every page is a function of `ShaleApp` in its own file; this file holds the frame, the tab
//! list and what a page can ask the dialog to do.

mod appearance;
mod data;
mod editor;
mod font_picker;
pub(crate) mod fonts;
mod help;
mod mail_social;
mod sync_calendar;
mod vault;

use crate::app::{muted, ShaleApp};
use crate::keys_ui::KeyDialog;
use crate::theme;
use eframe::egui::{self, Context, Id, Ui};

/// The tabs of the Settings dialog (CFG-020, KEY-006).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettingsTab {
    /// The vault folder, closing it and the people folder: the short page Settings opens on.
    Vault,
    Editor,
    Appearance,
    Fonts,
    Sync,
    Calendar,
    Mail,
    Social,
    Data,
    Help,
    Keybindings,
}

impl SettingsTab {
    /// All tabs in the order of the list.
    pub const ALL: [SettingsTab; 11] = [
        SettingsTab::Vault,
        SettingsTab::Editor,
        SettingsTab::Appearance,
        SettingsTab::Fonts,
        SettingsTab::Sync,
        SettingsTab::Calendar,
        SettingsTab::Mail,
        SettingsTab::Social,
        SettingsTab::Data,
        SettingsTab::Help,
        SettingsTab::Keybindings,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SettingsTab::Vault => "Vault",
            SettingsTab::Editor => "Editor",
            SettingsTab::Appearance => "Appearance",
            SettingsTab::Fonts => "Fonts",
            SettingsTab::Sync => "Sync",
            SettingsTab::Calendar => "Calendar",
            SettingsTab::Mail => "Mail",
            SettingsTab::Social => "Feeds",
            SettingsTab::Data => "Data",
            SettingsTab::Help => "Help",
            SettingsTab::Keybindings => "Keybindings",
        }
    }
}

impl SettingsTab {
    /// One line above the page: what it is for.
    fn about(self) -> &'static str {
        match self {
            SettingsTab::Vault => "The folder this vault lives in, and where its contacts are",
            SettingsTab::Editor => "How notes are edited: links, tables, Vim keys and the toolbar",
            SettingsTab::Appearance => "How notes, drawings and media look",
            SettingsTab::Fonts => "The font and text size of the editor, dialogs, mail, calendar and feeds",
            SettingsTab::Sync => "Keep this vault in a git repository",
            SettingsTab::Calendar => "Calendars and the week",
            SettingsTab::Mail => "Fetching and reading mail",
            SettingsTab::Social => "Feeds, new posts and the post search",
            SettingsTab::Data => "What Shale keeps on disk, and for how long",
            SettingsTab::Help => "The user guide and tools to find problems",
            SettingsTab::Keybindings => "Which keys do what",
        }
    }
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
    pub guide: bool,
}

fn last_tab_id() -> Id {
    Id::unique("settings-last-tab")
}

/// The tab Settings was last on in this session (the Vault tab at first).
pub fn last_tab(ctx: &Context) -> SettingsTab {
    ctx.data(|d| d.get_temp(last_tab_id())).unwrap_or(SettingsTab::Vault)
}

impl ShaleApp {
    /// The content of the dialog: header row, tab list and the page of the chosen tab.
    pub fn settings_body(&mut self, ui: &mut Ui, vault_path: &mut String, tab: &mut SettingsTab, keys: &mut KeyDialog) -> Asks {
        let mut asks = Asks::default();
        ui.ctx().data_mut(|d| d.insert_temp(last_tab_id(), *tab));
        self.settings_header(ui, *tab, &mut asks);
        ui.separator();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(100.0);
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                    for t in SettingsTab::ALL {
                        ui.selectable_value(tab, t, t.label());
                    }
                });
            });
            ui.separator();
            ui.vertical(|ui| {
                if *tab != SettingsTab::Keybindings {
                    self.keymap.recording = false;
                }
                let changed = match *tab {
                    SettingsTab::Vault => self.vault_tab(ui, vault_path, &mut asks),
                    SettingsTab::Editor => self.editor_tab(ui),
                    SettingsTab::Appearance => self.appearance_tab(ui),
                    SettingsTab::Fonts => self.fonts_tab(ui),
                    SettingsTab::Sync => self.sync_tab(ui),
                    SettingsTab::Calendar => self.calendar_tab(ui, &mut asks),
                    SettingsTab::Mail => self.mail_tab(ui, &mut asks),
                    SettingsTab::Social => self.social_tab(ui, &mut asks),
                    SettingsTab::Data => self.data_tab(ui, &mut asks),
                    SettingsTab::Help => self.help_tab(ui, &mut asks),
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
        asks
    }

    /// The row above the tabs: what the page is for, and room on the right for buttons of the
    /// whole dialog (add them in a `right_to_left` layout here).
    fn settings_header(&mut self, ui: &mut Ui, tab: SettingsTab, asks: &mut Asks) {
        ui.horizontal(|ui| {
            ui.label(muted(tab.about()).size(12.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Open settings file").on_hover_text("Edit all settings as text, with a description of each").clicked() {
                    self.open_settings_file();
                    asks.close = true;
                }
            });
        });
    }
}

/// A grey section heading inside a page.
pub(crate) fn section(ui: &mut Ui, title: &str) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(title).color(theme::MUTED));
}
