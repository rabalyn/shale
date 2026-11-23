//! The vault dropdown (VAULT-030, rabalyn/shale#583): the vault label at the bottom left of the window
//! opens a list of the known vaults with **Open…** and **Create…**; a right-click on a vault renames
//! it (VAULT-032); a vault whose folder is missing is greyed out with **Locate…** and **Forget**
//! (VAULT-038). The folder picker of Open…, Create… and Locate… is the app's own dialog (a path
//! field and the folders below it): there is no native one yet.

use crate::app::{muted, Action, Modal, ShaleApp};
use crate::known_vaults::{canonical, KnownVault};
use crate::layout::shellexpand;
use crate::picker::ListKeys;
use crate::theme;
use eframe::egui::{self, Context, Key, Ui};
use std::path::{Path, PathBuf};

/// What a vault dialog shows.
pub enum VaultModal {
    /// the list of the known vaults; `missing` are the paths whose folder is not there, looked up when
    /// the list opens and not on every frame (a share may take long to answer)
    Menu {
        sel: usize,
        missing: Vec<PathBuf>,
    },
    Folder(Box<FolderPicker>),
    /// the name of a vault (VAULT-032)
    Rename {
        path: PathBuf,
        name: String,
        error: Option<String>,
    },
}

/// What the folder is chosen for.
#[derive(Clone, Debug, PartialEq)]
pub enum Purpose {
    /// VAULT-033
    Open,
    /// VAULT-034
    Create,
    /// the folder of the known vault at this path has moved (VAULT-038)
    Locate(PathBuf),
}

/// The folder picker: a path field, the folders below the one shown, and what the choice is for.
pub struct FolderPicker {
    pub purpose: Purpose,
    /// the path field: what is chosen
    pub text: String,
    /// the folder whose subfolders are listed
    dir: PathBuf,
    subfolders: Vec<String>,
    /// Create: start with sample notes (VAULT-034)
    pub sample: bool,
    pub error: Option<String>,
    /// Create refused a folder with content: offer to open it instead (VAULT-034)
    offer_open: bool,
}

impl FolderPicker {
    pub fn new(purpose: Purpose) -> Self {
        let start = match &purpose {
            Purpose::Locate(old) => old.ancestors().find(|a| a.is_dir()).map(Path::to_path_buf),
            _ => dirs::home_dir(),
        };
        let mut p = FolderPicker { purpose, text: String::new(), dir: PathBuf::new(), subfolders: Vec::new(), sample: false, error: None, offer_open: false };
        p.go(start.unwrap_or_else(|| PathBuf::from("/")));
        p
    }

    /// Show the folder `dir`: it is in the path field and its subfolders are listed (not the hidden ones).
    pub(crate) fn go(&mut self, dir: PathBuf) {
        self.text = dir.to_string_lossy().into_owned();
        self.subfolders = subfolders(&dir);
        self.dir = dir;
        self.error = None;
        self.offer_open = false;
    }

    #[cfg(test)]
    pub(crate) fn subfolders(&self) -> &[String] {
        &self.subfolders
    }
}

/// The names of the folders in `dir`, hidden ones left out, sorted without regard to case.
fn subfolders(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir).into_iter().flatten().flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| !n.starts_with('.')).collect();
    names.sort_by_key(|n| n.to_lowercase());
    names
}

/// Whether `dir` holds nothing but hidden files (a name starting with `.`, such as `.DS_Store`): a
/// folder a new vault may start in (VAULT-034).
pub fn is_empty_folder(dir: &Path) -> std::io::Result<bool> {
    for e in std::fs::read_dir(dir)? {
        if !e?.file_name().to_string_lossy().starts_with('.') {
            return Ok(false);
        }
    }
    Ok(true)
}

/// What a vault dialog does next.
enum Next {
    Keep,
    Close,
    Show(VaultModal),
}

impl ShaleApp {
    /// Open the dropdown: the open vault is the selected line.
    pub(crate) fn open_vault_menu(&mut self) {
        self.modal = Some(Modal::Vaults(Box::new(self.vault_menu())));
    }

    fn vault_menu(&self) -> VaultModal {
        let open = self.vault.as_ref().map(|v| canonical(&v.root));
        let entries = self.cfg.vaults.entries();
        let sel = open.and_then(|o| entries.iter().position(|e| e.path == o)).unwrap_or(0);
        VaultModal::Menu { sel, missing: entries.iter().filter(|e| !e.path.is_dir()).map(|e| e.path.clone()).collect() }
    }

    /// The dialog of `m`; false when it is done.
    pub(crate) fn vault_modal(&mut self, ctx: &Context, m: &mut VaultModal) -> bool {
        let next = match m {
            VaultModal::Menu { sel, missing } => self.vault_menu_dialog(ctx, sel, missing),
            VaultModal::Folder(p) => self.folder_dialog(ctx, p),
            VaultModal::Rename { path, name, error } => self.vault_rename_dialog(ctx, path, name, error),
        };
        match next {
            Next::Keep => true,
            Next::Close => false,
            Next::Show(n) => {
                *m = n;
                true
            }
        }
    }

    fn vault_menu_dialog(&mut self, ctx: &Context, sel: &mut usize, missing: &[PathBuf]) -> Next {
        let entries: Vec<KnownVault> = self.cfg.vaults.entries().to_vec();
        let labels: Vec<String> = entries.iter().map(|e| self.cfg.vaults.label(e)).collect();
        let open = self.vault.as_ref().map(|v| canonical(&v.root));
        let n = entries.len() + 2;
        let mut next = Next::Keep;
        let mut pick: Option<usize> = None;
        let resp = crate::dialog::Dialog::new("vaults", "Vaults").width(460.0).show(ctx, |ui| {
            let keys = ListKeys::take(ui, &self.keymap);
            keys.select(sel, n);
            if keys.enter {
                pick = Some(*sel);
            }
            if keys.esc {
                next = Next::Close;
            }
            if entries.is_empty() {
                ui.label(muted("No vaults yet."));
            }
            for (i, e) in entries.iter().enumerate() {
                let (is_open, is_missing) = (open.as_ref() == Some(&e.path), missing.contains(&e.path));
                ui.horizontal(|ui| {
                    let shown = if is_open { format!("✓ {}", labels[i]) } else { format!("   {}", labels[i]) };
                    let text = if is_missing { egui::RichText::new(shown).color(theme::palette().muted) } else { egui::RichText::new(shown) };
                    let r = ui.selectable_label(i == *sel, text).on_hover_text(e.path.to_string_lossy());
                    if r.clicked() && !is_missing {
                        pick = Some(i);
                    }
                    r.context_menu(|ui| {
                        if ui.button("Rename").clicked() {
                            next = Next::Show(VaultModal::Rename { path: e.path.clone(), name: e.name.clone(), error: None });
                            ui.close();
                        }
                    });
                    if is_missing {
                        ui.label(muted("folder missing"));
                        if ui.button("Locate…").clicked() {
                            next = Next::Show(VaultModal::Folder(Box::new(FolderPicker::new(Purpose::Locate(e.path.clone())))));
                        }
                        if ui.button("Forget").clicked() {
                            self.cfg.vaults.forget(&e.path);
                            self.save_config();
                            next = Next::Show(self.vault_menu());
                        }
                    }
                });
            }
            ui.separator();
            let (open_row, create_row) = (entries.len(), entries.len() + 1);
            if ui.selectable_label(*sel == open_row, "Open…").clicked() {
                pick = Some(open_row);
            }
            if ui.selectable_label(*sel == create_row, "Create…").clicked() {
                pick = Some(create_row);
            }
            crate::theme::prose(ui, muted(ListKeys::hint(&self.keymap)).size(11.5));
        });
        if let Some(p) = pick {
            *sel = p;
            match p {
                p if p < entries.len() => {
                    if !missing.contains(&entries[p].path) {
                        // the open vault stays as it is
                        if open.as_ref() != Some(&entries[p].path) {
                            self.actions.push(Action::OpenVault(entries[p].path.clone()));
                        }
                        next = Next::Close;
                    }
                }
                p if p == entries.len() => next = Next::Show(VaultModal::Folder(Box::new(FolderPicker::new(Purpose::Open)))),
                _ => next = Next::Show(VaultModal::Folder(Box::new(FolderPicker::new(Purpose::Create)))),
            }
        }
        if resp.should_close() {
            next = Next::Close;
        }
        next
    }

    fn vault_rename_dialog(&mut self, ctx: &Context, path: &Path, name: &mut String, error: &mut Option<String>) -> Next {
        let mut next = Next::Keep;
        let resp = crate::dialog::Dialog::new("vault-rename", "Rename vault").width(420.0).show(ctx, |ui| {
            let r = ui.add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY));
            r.request_focus();
            crate::theme::hint(ui, "The name is kept on this computer only; the folder keeps its name.");
            if let Some(e) = error.as_ref() {
                crate::theme::problem(ui, e.as_str());
            }
            let enter = ui.input(|i| i.key_pressed(Key::Enter));
            ui.horizontal(|ui| {
                if ui.button("Rename").clicked() || enter {
                    match self.cfg.vaults.rename(path, name) {
                        Ok(()) => {
                            self.save_config();
                            next = Next::Close;
                        }
                        Err(e) => *error = Some(e),
                    }
                }
                if ui.button("Cancel").clicked() {
                    next = Next::Close;
                }
            });
        });
        if resp.should_close() {
            next = Next::Close;
        }
        next
    }

    fn folder_dialog(&mut self, ctx: &Context, p: &mut FolderPicker) -> Next {
        let (title, button) = match &p.purpose {
            Purpose::Open => ("Open a folder as vault", "Open"),
            Purpose::Create => ("Create a vault in an empty folder", "Create"),
            Purpose::Locate(_) => ("Locate the folder of the vault", "Locate"),
        };
        let mut done = false;
        let mut cancel = false;
        let mut st = (p, false);
        let resp = crate::dialog::Dialog::new("vault-folder", title).width(520.0).show_with_footer(
            ctx,
            &mut st,
            |ui, (p, enter)| {
                let r = ui.add(egui::TextEdit::singleline(&mut p.text).desired_width(f32::INFINITY));
                r.request_focus();
                *enter = ui.input(|i| i.key_pressed(Key::Enter));
                if let Some(parent) = p.dir.parent().map(Path::to_path_buf) {
                    if ui.link("↑  Up").clicked() {
                        p.go(parent);
                    }
                }
                let mut enter_dir = None;
                egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                    for name in &p.subfolders {
                        if ui.link(format!("📁  {name}")).clicked() {
                            enter_dir = Some(p.dir.join(name));
                        }
                    }
                });
                if let Some(d) = enter_dir {
                    p.go(d);
                }
                if p.purpose == Purpose::Create {
                    ui.add_space(6.0);
                    ui.checkbox(&mut p.sample, "Start with sample notes");
                    crate::theme::hint(ui, "A folder that does not exist yet is made. Hidden files do not count as content.");
                }
                if let Some(e) = &p.error {
                    crate::theme::problem(ui, e.as_str());
                }
                if p.offer_open && ui.button("Open it as a vault instead").clicked() {
                    p.purpose = Purpose::Open;
                    p.offer_open = false;
                    *enter = true;
                }
            },
            |ui, (_, enter)| {
                ui.horizontal(|ui| {
                    if ui.button(egui::RichText::new(button).strong()).clicked() {
                        *enter = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            },
        );
        let (p, enter) = st;
        if enter {
            done = self.choose_folder(p);
        }
        if done || cancel || resp.should_close() {
            Next::Close
        } else {
            Next::Keep
        }
    }

    /// The folder in the path field is chosen: do what the picker is for. False: it is refused, the
    /// reason is in `p.error`.
    pub(crate) fn choose_folder(&mut self, p: &mut FolderPicker) -> bool {
        let path = PathBuf::from(shellexpand(&p.text));
        p.error = None;
        p.offer_open = false;
        match p.purpose.clone() {
            Purpose::Open => {
                if !path.is_dir() {
                    p.error = Some("That folder does not exist.".into());
                    return false;
                }
            }
            Purpose::Create => {
                if path.exists() && !path.is_dir() {
                    p.error = Some("That is a file, not a folder.".into());
                    return false;
                }
                if path.is_dir() {
                    match is_empty_folder(&path) {
                        Ok(true) => {}
                        Ok(false) => {
                            p.error = Some("That folder has content, so it cannot start a new vault.".into());
                            p.offer_open = true;
                            return false;
                        }
                        Err(e) => {
                            p.error = Some(format!("Could not read the folder: {e}"));
                            return false;
                        }
                    }
                }
                let made = std::fs::create_dir_all(&path).and_then(|()| if p.sample { crate::vault::write_sample_vault(&path) } else { Ok(()) });
                if let Err(e) = made {
                    p.error = Some(format!("Could not create the vault: {e}"));
                    return false;
                }
            }
            Purpose::Locate(old) => {
                if !path.is_dir() {
                    p.error = Some("That folder does not exist.".into());
                    return false;
                }
                let nesting = self.cfg.vaults.nested_with(&path, Some(&old));
                if let Err(e) = self.cfg.vaults.relocate(&old, &path) {
                    p.error = Some(e);
                    return false;
                }
                self.save_config();
                if let Some(n) = nesting {
                    self.toast(n.warning());
                }
                return true;
            }
        }
        self.actions.push(Action::OpenVault(path));
        true
    }
}

/// The vault label at the bottom left of the window: it opens the dropdown (VAULT-030).
pub(crate) fn vault_label(ui: &mut Ui, name: &str, path: &str) -> bool {
    ui.add(egui::Button::new(egui::RichText::new(format!("▣  {name}  ▾")).color(theme::palette().text).size(13.0)).frame(false)).on_hover_text(path).clicked()
}
