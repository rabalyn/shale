//! The vault dropdown (VAULT-030, rabalyn/shale#583): the vault label at the bottom left of the window
//! opens a list of the known vaults with **Open…** and **Create…**; a right-click on a vault renames
//! it (VAULT-032); a vault whose folder is missing is greyed out with **Locate…** and **Forget**
//! (VAULT-038). The folder of Open…, Create… and Locate… is chosen with the file picker (PICK-001):
//! the system's dialog, or Shale's own picker (`file_picker`).

use crate::app::vault_close::Remove;
use crate::app::{muted, Action, Modal, ShaleApp};
use crate::file_picker::Request;
use crate::known_vaults::{canonical, KnownVault};
use crate::picker::ListKeys;
use crate::theme;
use eframe::egui::{self, Context, Key, Pos2, Rect, Sense, StrokeKind, Ui, Vec2};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;

/// What a vault dialog shows.
pub enum VaultModal {
    /// the list of the known vaults
    Menu { sel: usize },
    /// the name of a vault (VAULT-032)
    Rename { path: PathBuf, name: String, error: Option<String> },
    /// Close asks which of Shale's folders to remove (VAULT-035); both are off at first
    Close { path: PathBuf, remove: Remove },
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

/// The key under which Open…, Create… and Locate… ask the file picker for their folder.
const PICK_KEY: &str = "vault-folder";

impl Purpose {
    /// What the picker is asked: the title, the button, and for Create the sample box and the hint (VAULT-033, VAULT-034).
    fn request(&self) -> Request {
        match self {
            Purpose::Open => Request::folder("Open a folder as vault").button("Open"),
            Purpose::Create => Request::folder("Create a vault in an empty folder").button("Create").option("Start with sample notes").hint("A folder that does not exist yet is made. Hidden files do not count as content."),
            Purpose::Locate(old) => {
                let r = Request::folder("Locate the folder of the vault").button("Locate");
                match old.ancestors().find(|a| a.is_dir()) {
                    Some(near) => r.start(near),
                    None => r,
                }
            }
        }
    }
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
        let mut dialog = crate::dialog::Dialog::new("vaults", "Vaults").width(460.0);
        if let Some(r) = self.side.vault_button {
            dialog = dialog.anchored_above(r.left_top());
        }
        let resp = dialog.show(ctx, |ui| {
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
                            self.ask_vault_folder(Purpose::Locate(e.path.clone()));
                            next = Next::Close;
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
                p if p == entries.len() => {
                    self.ask_vault_folder(Purpose::Open);
                    next = Next::Close;
                }
                p if p == entries.len() + 1 => {
                    self.ask_vault_folder(Purpose::Create);
                    next = Next::Close;
                }
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
            crate::clickable::pointer(ui.checkbox(&mut remove.shale, "Also remove .shale/ (this vault's settings and state)"));
            crate::clickable::pointer(ui.checkbox(&mut remove.trash, "Also remove .trash/"));
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

    /// Open…, Create… and Locate… ask for the folder through the file picker (VAULT-033, PICK-001);
    /// the answer is taken by [`ShaleApp::vault_pick_tick`].
    pub(crate) fn ask_vault_folder(&mut self, purpose: Purpose) {
        self.pick.ask(PICK_KEY, purpose.request());
        self.vault_pick = Some(purpose);
    }

    /// Take the folder the picker chose for Open…, Create… or Locate… and do what it is for. A
    /// folder that is refused comes back in Shale's own picker with the reason (the system's dialog
    /// cannot show one), and with the offer to open a folder with content as a vault (VAULT-034).
    pub(crate) fn vault_pick_tick(&mut self) {
        let Some(picked) = self.pick.take(PICK_KEY) else { return };
        let Some(mut purpose) = self.vault_pick.take() else { return };
        if picked.offered {
            purpose = Purpose::Open;
        }
        let path = picked.first().map(Path::to_path_buf).unwrap_or_default();
        if let Err(r) = self.choose_vault_folder(&purpose, path.clone(), picked.option) {
            let mut req = purpose.request().start(path).refused(r.message).option_checked(picked.option);
            if r.offer_open {
                req = req.offer("Open it as a vault instead");
            }
            self.pick.ask(PICK_KEY, req);
            self.vault_pick = Some(purpose);
        }
    }

    /// The folder `path` is chosen: do what the picker is for (`sample`: Create starts with the
    /// sample notes). Err: it is refused, with the reason.
    pub(crate) fn choose_vault_folder(&mut self, purpose: &Purpose, path: PathBuf, sample: bool) -> Result<(), Refusal> {
        let refuse = |m: String| Err(Refusal { message: m, offer_open: false });
        match purpose {
            Purpose::Open => {
                if !path.is_dir() {
                    return refuse("That folder does not exist.".into());
                }
            }
            Purpose::Create => {
                if path.exists() && !path.is_dir() {
                    return refuse("That is a file, not a folder.".into());
                }
                if path.is_dir() {
                    match is_empty_folder(&path) {
                        Ok(true) => {}
                        Ok(false) => return Err(Refusal { message: "That folder has content, so it cannot start a new vault.".into(), offer_open: true }),
                        Err(e) => return refuse(format!("Could not read the folder: {e}")),
                    }
                }
                let made = std::fs::create_dir_all(&path).and_then(|()| if sample { crate::vault::write_sample_vault(&path) } else { Ok(()) });
                if let Err(e) = made {
                    return refuse(format!("Could not create the vault: {e}"));
                }
            }
            Purpose::Locate(old) => {
                if !path.is_dir() {
                    return refuse("That folder does not exist.".into());
                }
                let nesting = self.cfg.vaults.nested_with(&path, Some(old));
                if let Err(e) = self.cfg.vaults.relocate(old, &path) {
                    return refuse(e);
                }
                self.save_config();
                if let Some(n) = nesting {
                    self.toast(n.warning());
                }
                return Ok(());
            }
        }
        self.actions.push(Action::OpenVault(path));
        Ok(())
    }
}

/// A folder that Open…, Create… or Locate… refuses.
pub struct Refusal {
    pub message: String,
    /// the folder has content: offer to open it as a vault instead (VAULT-034)
    pub offer_open: bool,
}

/// The room of the switcher strip at the bottom of the left sidebar: the button and its margin.
const STRIP_HEIGHT: f32 = 36.0;
/// The height of the switcher button.
const BUTTON_HEIGHT: f32 = 26.0;

impl ShaleApp {
    /// The name and the folder of the open vault; None on the start screen.
    fn vault_name_path(&self) -> Option<(String, String)> {
        let v = self.vault.as_ref()?;
        Some((self.cfg.vaults.label_of(&v.root), v.root.to_string_lossy().into_owned()))
    }

    /// The strip at the bottom of the left sidebar, below whichever pane is open: the vault
    /// switcher, a dropdown button (VAULT-030).
    pub(crate) fn vault_strip(&mut self, ui: &mut Ui) {
        let Some((name, path)) = self.vault_name_path() else { return };
        let open = matches!(self.modal, Some(Modal::Vaults(_)));
        egui::Panel::bottom("vault-strip").exact_size(STRIP_HEIGHT).resizable(false).frame(egui::Frame::new().inner_margin(egui::Margin { top: 5, ..Default::default() })).show(ui, |ui| {
            let resp = vault_button(ui, &name, &path, open);
            self.side.vault_button = Some(resp.rect);
            if resp.clicked() {
                self.open_vault_menu();
            }
        });
    }

    /// While the left sidebar is hidden the switcher is an icon of the ribbon that opens the same
    /// dropdown (VAULT-030).
    pub(crate) fn ribbon_vault(&mut self, ui: &mut Ui) {
        let Some((name, path)) = self.vault_name_path() else { return };
        if self.cfg.show_left {
            return;
        }
        let open = matches!(self.modal, Some(Modal::Vaults(_)));
        let resp = crate::layout::icon_button(ui, crate::layout::Icon::Vault, 30.0, open, &format!("Vault: {name}\n{path}"));
        self.side.vault_button = Some(resp.rect);
        if resp.clicked() {
            self.open_vault_menu();
        }
    }
}

/// The switcher: a framed dropdown button with the vault icon, the name (shortened with … when it
/// does not fit; the full name and the folder are the tooltip) and a ▾ (VAULT-030). Its look comes
/// from the visuals of the theme: rest, hover, pressed, and open while the dropdown is.
fn vault_button(ui: &mut Ui, name: &str, path: &str, open: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), BUTTON_HEIGHT), Sense::click());
    let w = &ui.visuals().widgets;
    let look = if resp.is_pointer_button_down_on() {
        w.active
    } else if open {
        w.open
    } else if resp.hovered() {
        w.hovered
    } else {
        w.inactive
    };
    let painter = ui.painter();
    painter.rect(rect, look.corner_radius, look.weak_bg_fill, look.bg_stroke, StrokeKind::Inside);
    let color = look.fg_stroke.color;
    let icon = Rect::from_min_size(rect.left_top() + Vec2::new(6.0, 3.0), Vec2::splat(BUTTON_HEIGHT - 6.0));
    crate::layout::paint_icon(painter, icon.shrink(2.0), crate::layout::Icon::Vault, color);
    let arrow = painter.layout_no_wrap("▾".into(), egui::FontId::proportional(13.0), color);
    let arrow_x = rect.right() - 8.0 - arrow.size().x;
    painter.galley(Pos2::new(arrow_x, rect.center().y - arrow.size().y / 2.0), arrow, color);
    let left = icon.right() + 6.0;
    let mut job = egui::text::LayoutJob::single_section(name.to_string(), egui::TextFormat::simple(egui::FontId::proportional(13.0), color));
    job.wrap = egui::text::TextWrapping { max_width: (arrow_x - 6.0 - left).max(10.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    let galley = painter.layout_job(job);
    let shortened = galley.rows.len() > 1 || galley.elided;
    painter.galley(Pos2::new(left, rect.center().y - galley.size().y / 2.0), galley, color);
    let tip = if shortened { format!("{name}\n{path}") } else { path.to_string() };
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Vault {name}")));
    crate::clickable::button_like(ui, resp).on_hover_text(tip)
}
