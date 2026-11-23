//! The vault dropdown (VAULT-030, rabalyn/shale#583): the vault label at the bottom left of the window
//! opens a list of the known vaults with **Open…** and **Create…**; a right-click on a vault renames
//! it (VAULT-032); a vault whose folder is missing is greyed out with **Locate…** and **Forget**
//! (VAULT-038). The folder picker of Open…, Create… and Locate… is the app's own dialog (a path
//! field and the folders below it): there is no native one yet.

use crate::app::vault_close::Remove;
use crate::app::{muted, Action, Modal, ShaleApp};
use crate::known_vaults::{canonical, KnownVault};
use crate::layout::shellexpand;
use crate::picker::ListKeys;
use crate::theme;
use eframe::egui::{self, Context, Key, Ui};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;

/// What a vault dialog shows.
pub enum VaultModal {
    /// the list of the known vaults
    Menu {
        sel: usize,
    },
    Folder(Box<FolderPicker>),
    /// the name of a vault (VAULT-032)
    Rename {
        path: PathBuf,
        name: String,
        error: Option<String>,
    },
    /// Close asks which of Shale's folders to remove (VAULT-035); both are off at first
    Close {
        path: PathBuf,
        remove: Remove,
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

/// A running look-up: the folders asked about and the answers to come.
type Lookup = (Vec<PathBuf>, Receiver<Result<Vec<bool>, String>>);

/// Whether the folders of the known vaults are there (VAULT-038). A folder on an unreachable share
/// can take seconds to answer, so the look-up runs on a worker thread, once per folder: the
/// answer is kept until [`Presence::clear`], and until it has come the folder is "checking".
pub struct Presence {
    /// true: the folder is there
    known: HashMap<PathBuf, bool>,
    job: Option<Lookup>,
    checker: Arc<dyn Fn(&Path) -> bool + Send + Sync>,
}

impl Default for Presence {
    fn default() -> Self {
        Self { known: HashMap::new(), job: None, checker: Arc::new(|p| p.is_dir()) }
    }
}

impl Presence {
    /// `Some(true)`: the folder is missing; `None`: not looked up yet.
    pub fn is_missing(&self, path: &Path) -> Option<bool> {
        self.known.get(path).map(|there| !there)
    }

    /// Forget the answers: the next [`Presence::request`] looks again (a drive may be mounted by now).
    pub fn clear(&mut self) {
        self.known.clear();
    }

    /// Whether a look-up runs (the headless tests wait for it, DEV-041).
    #[cfg(test)]
    pub(crate) fn pending(&self) -> bool {
        self.job.is_some()
    }

    /// Take in the answer of the running look-up.
    pub fn poll(&mut self) {
        let Some((paths, rx)) = &self.job else { return };
        let answer = match rx.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("stopped".to_string()),
        };
        // a look-up that crashed counts as "there": the folder is tried when it is opened
        let there = answer.unwrap_or_else(|_| vec![true; paths.len()]);
        for (p, t) in paths.iter().zip(there) {
            self.known.insert(p.clone(), t);
        }
        self.job = None;
    }

    /// Start a look-up for the folders that have no answer yet, unless one runs.
    pub fn request(&mut self, wanted: impl Iterator<Item = PathBuf>, ctx: Option<Context>) {
        if self.job.is_some() {
            return;
        }
        let todo: Vec<PathBuf> = wanted.filter(|p| !self.known.contains_key(p)).collect();
        if todo.is_empty() {
            return;
        }
        let (check, paths) = (self.checker.clone(), todo.clone());
        let rx = crate::jobs::spawn(
            move || paths.iter().map(|p| check(p)).collect::<Vec<bool>>(),
            move || {
                if let Some(c) = ctx {
                    c.request_repaint();
                }
            },
        );
        self.job = Some((todo, rx));
    }

    #[cfg(test)]
    pub(crate) fn with_checker(&mut self, f: impl Fn(&Path) -> bool + Send + Sync + 'static) {
        self.checker = Arc::new(f);
    }
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
        // the folders are looked at again: one may be back, or gone
        self.presence.clear();
        self.modal = Some(Modal::Vaults(Box::new(self.vault_menu())));
    }

    /// Look up the folders of the known vaults that have no answer yet (VAULT-038).
    pub(crate) fn check_vault_folders(&mut self) {
        let wanted = self.cfg.vaults.entries().iter().map(|e| e.path.clone());
        self.presence.request(wanted, self.ctx.clone());
    }

    fn vault_menu(&self) -> VaultModal {
        let open = self.vault.as_ref().map(|v| canonical(&v.root));
        let entries = self.cfg.vaults.entries();
        let sel = open.and_then(|o| entries.iter().position(|e| e.path == o)).unwrap_or(0);
        VaultModal::Menu { sel }
    }

    /// The dialog of `m`; false when it is done.
    pub(crate) fn vault_modal(&mut self, ctx: &Context, m: &mut VaultModal) -> bool {
        let next = match m {
            VaultModal::Menu { sel } => self.vault_menu_dialog(ctx, sel),
            VaultModal::Folder(p) => self.folder_dialog(ctx, p),
            VaultModal::Rename { path, name, error } => self.vault_rename_dialog(ctx, path, name, error),
            VaultModal::Close { path, remove } => self.vault_close_dialog(ctx, path, remove),
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

    fn vault_menu_dialog(&mut self, ctx: &Context, sel: &mut usize) -> Next {
        self.check_vault_folders();
        let entries: Vec<KnownVault> = self.cfg.vaults.entries().to_vec();
        let labels: Vec<String> = entries.iter().map(|e| self.cfg.vaults.label(e)).collect();
        let open = self.vault.as_ref().map(|v| canonical(&v.root));
        // Close current vault is the last line, when a vault is open (VAULT-044)
        let n = entries.len() + 2 + usize::from(open.is_some());
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
                // (None: the look-up has not answered yet; the entry cannot be opened until it has)
                let (is_open, state) = (open.as_ref() == Some(&e.path), self.presence.is_missing(&e.path));
                let is_missing = state == Some(true);
                let usable = state == Some(false);
                ui.horizontal(|ui| {
                    let shown = if is_open { format!("✓ {}", labels[i]) } else { format!("   {}", labels[i]) };
                    let text = if !usable { egui::RichText::new(shown).color(theme::palette().muted) } else { egui::RichText::new(shown) };
                    let r = ui.selectable_label(i == *sel, text).on_hover_text(e.path.to_string_lossy());
                    if r.clicked() && usable {
                        pick = Some(i);
                    }
                    r.context_menu(|ui| {
                        if ui.button("Rename").clicked() {
                            next = Next::Show(VaultModal::Rename { path: e.path.clone(), name: e.name.clone(), error: None });
                            ui.close();
                        }
                        if ui.button("Close").clicked() {
                            next = Next::Show(VaultModal::Close { path: e.path.clone(), remove: Remove::default() });
                            ui.close();
                        }
                    });
                    if state.is_none() {
                        ui.label(muted("checking…"));
                    }
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
            if open.is_some() {
                ui.separator();
                if ui.selectable_label(*sel == create_row + 1, "Close current vault").clicked() {
                    pick = Some(create_row + 1);
                }
            }
            crate::theme::prose(ui, muted(ListKeys::hint(&self.keymap)).size(11.5));
        });
        if let Some(p) = pick {
            *sel = p;
            match p {
                p if p < entries.len() => {
                    if self.presence.is_missing(&entries[p].path) == Some(false) {
                        // the open vault stays as it is
                        if open.as_ref() != Some(&entries[p].path) {
                            self.actions.push(Action::OpenVault(entries[p].path.clone()));
                        }
                        next = Next::Close;
                    }
                }
                p if p == entries.len() => next = Next::Show(VaultModal::Folder(Box::new(FolderPicker::new(Purpose::Open)))),
                p if p == entries.len() + 1 => next = Next::Show(VaultModal::Folder(Box::new(FolderPicker::new(Purpose::Create)))),
                _ => {
                    if let Some(path) = open {
                        next = Next::Show(VaultModal::Close { path, remove: Remove::default() });
                    }
                }
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

    /// VAULT-035: Close forgets the vault; which of Shale's folders go with it is asked here.
    fn vault_close_dialog(&mut self, ctx: &Context, path: &Path, remove: &mut Remove) -> Next {
        let mut next = Next::Keep;
        let name = self.cfg.vaults.label_of(path);
        let resp = crate::dialog::Dialog::new("vault-close", "Close vault").width(460.0).show(ctx, |ui| {
            crate::theme::paragraph(ui, format!("Close “{name}”? It is removed from the list of vaults. Its notes and attachments stay where they are, and you can open the folder again later."));
            ui.add_space(6.0);
            ui.checkbox(&mut remove.shale, "Also remove .shale/ (this vault's settings and state)");
            ui.checkbox(&mut remove.trash, "Also remove .trash/");
            if remove.trash {
                crate::theme::problem(ui, ".trash/ holds the notes you deleted from this vault. Removing it deletes them for good.");
            }
            let enter = ui.input(|i| i.key_pressed(Key::Enter));
            ui.horizontal(|ui| {
                if ui.button(egui::RichText::new("Close vault").strong()).clicked() || enter {
                    self.actions.push(Action::CloseVault { path: path.to_path_buf(), remove: *remove });
                    next = Next::Close;
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
