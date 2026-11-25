//! Shale's own picker (PICK-004 to PICK-009): what the person gets when the setting "Use the
//! system's file dialog" is off, or when the system has no dialog to show (no portal). It chooses
//! a folder, a file, several files or the place of a file to write: a path field (with `~`), the
//! shortcuts on the side (PICK-006), the folders and files of the shown folder, the kinds of file
//! as a list, and the **New folder** field of #688 (PICK-008).

use super::names::{self, Made};
use super::places::{self, Group, Place};
use super::{Filter, Mode, Picked, Request};
use crate::clickable::{clickable, ComboBoxPointer};
use crate::layout::shellexpand;
use eframe::egui::{self, Context, Key, Label, Ui};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};

/// What a frame of the picker came to.
pub enum Step {
    Keep,
    Cancel,
    Done(Picked),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Path,
    NewFolder,
    Name,
}

/// What the person asked for in a frame; it is carried out after the dialog was drawn.
enum Intent {
    /// the button, or Enter in the file name
    Choose,
    /// Enter in the path field
    SubmitPath,
    /// the button or Enter of the New folder field
    MakeFolder,
    /// "Use that folder", when the new folder's name is taken
    UseExisting,
    /// the button of [`Request::offer`]
    Offer,
    Cancel,
}

pub struct OwnPicker {
    pub(super) req: Request,
    /// the path field: the folder that is shown, or the one that is typed
    pub(super) text: String,
    /// the folder whose content is listed
    pub(super) dir: PathBuf,
    pub(super) folders: Vec<String>,
    pub(super) files: Vec<String>,
    /// the names of the chosen files in `dir`
    pub(super) selected: Vec<String>,
    /// the kinds of file to choose from: the request's, and all files
    pub(super) kinds: Vec<Filter>,
    pub(super) filter: usize,
    /// the name of the file that is saved
    pub(super) name: String,
    pub(super) new_folder: String,
    /// a folder of the typed name is there already: it can be used
    existing: Option<PathBuf>,
    pub(super) option: bool,
    pub(super) error: Option<String>,
    /// the offer of the request is shown (after a refusal)
    offer_shown: bool,
    pub(super) places: Vec<Place>,
    places_rx: Option<Receiver<Result<Vec<Place>, String>>>,
    focus: Option<Field>,
}

impl OwnPicker {
    /// Open the picker for `req`; the shortcuts are looked up on a worker thread.
    pub fn open(req: Request, inputs: places::Inputs, ctx: Context) -> OwnPicker {
        let start = req.start.clone().or_else(crate::userdirs::home_dir).unwrap_or_else(|| PathBuf::from("/"));
        let mut kinds = req.filters.clone();
        if !kinds.iter().any(|k| k.exts.is_empty()) {
            kinds.push(Filter::new("All files", &[]));
        }
        let rx = crate::jobs::spawn(move || places::scan(&inputs), move || ctx.request_repaint());
        let mut p = OwnPicker {
            text: String::new(),
            dir: PathBuf::new(),
            folders: vec![],
            files: vec![],
            selected: vec![],
            kinds,
            filter: 0,
            name: req.file_name.clone(),
            new_folder: String::new(),
            existing: None,
            option: req.option_on,
            error: None,
            offer_shown: false,
            places: vec![],
            places_rx: Some(rx),
            focus: Some(if req.mode == Mode::Save { Field::Name } else { Field::Path }),
            req,
        };
        // a typed path that is not there yet (a refused one) is kept in the field, and the nearest folder is shown
        let shown = start.ancestors().find(|a| a.is_dir()).map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("/"));
        p.go(shown);
        if !start.is_dir() {
            p.text = start.to_string_lossy().into_owned();
        }
        p.error = p.req.refused.clone();
        p.offer_shown = p.req.refused.is_some() && p.req.offer.is_some();
        p
    }

    /// Whether the shortcuts are still being looked up.
    #[cfg(test)]
    pub(crate) fn scanning(&self) -> bool {
        self.places_rx.is_some()
    }

    fn poll_places(&mut self) {
        let Some(rx) = &self.places_rx else { return };
        match rx.try_recv() {
            Ok(Ok(list)) => self.places = list,
            Ok(Err(_)) | Err(TryRecvError::Disconnected) => {}
            Err(TryRecvError::Empty) => return,
        }
        self.places_rx = None;
    }

    /// Show the folder `dir`: it is in the path field and what is in it is listed (not the hidden
    /// ones). A choice of files is dropped.
    pub fn go(&mut self, dir: PathBuf) {
        self.text = dir.to_string_lossy().into_owned();
        self.dir = dir;
        self.selected.clear();
        self.error = None;
        self.existing = None;
        self.offer_shown = false;
        self.relist();
    }

    /// List `dir` again (the kind of file changed).
    fn relist(&mut self) {
        let kind = self.kinds.get(self.filter).cloned().unwrap_or_else(|| Filter::new("All files", &[]));
        let (mut folders, mut files) = (vec![], vec![]);
        for e in std::fs::read_dir(&self.dir).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            if e.path().is_dir() {
                folders.push(name);
            } else if self.req.mode != Mode::Folder && kind.matches(&name) {
                files.push(name);
            }
        }
        folders.sort_by_key(|n| n.to_lowercase());
        files.sort_by_key(|n| n.to_lowercase());
        self.selected.retain(|s| files.contains(s));
        (self.folders, self.files) = (folders, files);
    }

    /// Click on a file of the list: Files takes it in or out of the choice, File and Save take it alone.
    pub(super) fn click_file(&mut self, name: &str) {
        self.error = None;
        match self.req.mode {
            Mode::Files => match self.selected.iter().position(|s| s == name) {
                Some(i) => {
                    self.selected.remove(i);
                }
                None => self.selected.push(name.to_string()),
            },
            _ => self.selected = vec![name.to_string()],
        }
        if self.req.mode == Mode::Save {
            self.name = name.to_string();
        }
    }

    /// The path field says what to choose (Enter in it).
    pub(super) fn submit_path(&mut self) -> Option<Picked> {
        let path = PathBuf::from(shellexpand(&self.text));
        match self.req.mode {
            Mode::Folder => self.choose(),
            Mode::File | Mode::Files => {
                if path.is_dir() {
                    self.go(path);
                    None
                } else if path.is_file() {
                    Some(self.picked(vec![path]))
                } else {
                    self.error = Some("That file or folder does not exist.".into());
                    None
                }
            }
            Mode::Save => {
                if path.is_dir() {
                    self.go(path);
                } else {
                    self.error = Some("That folder does not exist.".into());
                }
                None
            }
        }
    }

    fn picked(&self, paths: Vec<PathBuf>) -> Picked {
        Picked { paths, option: self.option, offered: false }
    }

    /// The button: choose what the dialog shows. A refusal puts its reason in `error`.
    pub(super) fn choose(&mut self) -> Option<Picked> {
        self.error = None;
        self.offer_shown = false;
        let typed = PathBuf::from(shellexpand(&self.text));
        match self.req.mode {
            Mode::Folder => {
                if self.text.trim().is_empty() {
                    self.error = Some("Choose a folder.".into());
                    return None;
                }
                Some(self.picked(vec![typed]))
            }
            Mode::File | Mode::Files => {
                if !self.selected.is_empty() {
                    let dir = self.dir.clone();
                    return Some(self.picked(self.selected.iter().map(|n| dir.join(n)).collect()));
                }
                if typed.is_file() {
                    return Some(self.picked(vec![typed]));
                }
                self.error = Some(if self.req.mode == Mode::Files { "Click the files to add." } else { "Click a file first." }.into());
                None
            }
            Mode::Save => {
                let name = self.name.trim();
                if name.is_empty() {
                    self.error = Some("Type a name for the file.".into());
                    return None;
                }
                if name.contains('/') || name.contains('\\') || name == "." || name == ".." {
                    self.error = Some("A file name cannot contain / or \\.".into());
                    return None;
                }
                if !typed.is_dir() {
                    self.error = Some("That folder does not exist.".into());
                    return None;
                }
                Some(self.picked(vec![typed.join(name)]))
            }
        }
    }

    /// The New folder field (#688): make the folder inside the shown one and use it: Folder chooses
    /// it, Save goes into it. A name that is refused, or a folder that cannot be made, shows its reason.
    pub(super) fn make_folder(&mut self) -> Option<Picked> {
        self.error = None;
        self.existing = None;
        match names::make_folder(&self.dir, &self.new_folder) {
            Err(e) => {
                self.error = Some(e);
                None
            }
            Ok(Made::Exists(p)) => {
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.error = Some(format!("“{name}” is there already in this folder."));
                self.existing = Some(p);
                None
            }
            Ok(Made::New(p)) => self.use_folder(p),
        }
    }

    fn use_folder(&mut self, p: PathBuf) -> Option<Picked> {
        self.new_folder.clear();
        if self.req.mode == Mode::Folder {
            Some(self.picked(vec![p]))
        } else {
            self.go(p);
            None
        }
    }

    fn apply(&mut self, intent: Intent) -> Option<Picked> {
        match intent {
            Intent::Choose => {
                let r = self.choose();
                if r.is_none() {
                    self.focus = Some(if self.req.mode == Mode::Save { Field::Name } else { Field::Path });
                }
                r
            }
            Intent::SubmitPath => {
                let r = self.submit_path();
                self.focus = Some(Field::Path);
                r
            }
            Intent::MakeFolder => {
                let r = self.make_folder();
                if r.is_none() {
                    self.focus = Some(Field::NewFolder);
                }
                r
            }
            Intent::UseExisting => self.existing.take().and_then(|p| self.use_folder(p)),
            Intent::Offer => Some(Picked { paths: vec![PathBuf::from(shellexpand(&self.text))], option: self.option, offered: true }),
            Intent::Cancel => None,
        }
    }

    fn wants(&mut self, f: Field) -> bool {
        let yes = self.focus == Some(f);
        if yes {
            self.focus = None;
        }
        yes
    }

    /// Draw the dialog and carry out what was asked in it.
    pub fn show(&mut self, ctx: &Context) -> Step {
        self.poll_places();
        let title = self.req.title.clone();
        let mut st = (self, None::<Intent>);
        let resp = crate::dialog::Dialog::new("file-picker", title).width(700.0).show_with_footer(ctx, &mut st, |ui, (p, intent)| p.body(ui, intent), |ui, (p, intent)| p.footer(ui, intent));
        let (p, intent) = st;
        let cancelled = matches!(intent, Some(Intent::Cancel)) || resp.should_close();
        if let Some(i) = intent {
            if let Some(done) = p.apply(i) {
                return Step::Done(done);
            }
        }
        if cancelled {
            Step::Cancel
        } else {
            Step::Keep
        }
    }

    fn body(&mut self, ui: &mut Ui, intent: &mut Option<Intent>) {
        let enter = ui.input(|i| i.key_pressed(Key::Enter));
        ui.horizontal(|ui| {
            if ui.add_enabled(self.dir.parent().is_some(), egui::Button::new("↑ Up")).on_hover_text("The folder around this one").clicked() {
                if let Some(parent) = self.dir.parent().map(Path::to_path_buf) {
                    self.go(parent);
                    self.focus = Some(Field::Path);
                }
            }
            let r = ui.add(egui::TextEdit::singleline(&mut self.text).hint_text("Path of the folder, ~ is your home folder").desired_width(f32::INFINITY));
            if self.wants(Field::Path) {
                r.request_focus();
            }
            if r.lost_focus() && enter {
                *intent = Some(Intent::SubmitPath);
            }
        });
        let mut go_to: Option<PathBuf> = None;
        let mut click: Option<String> = None;
        let rest = ui.available_width();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(150.0);
                egui::ScrollArea::vertical().id_salt("pick-places").max_height(240.0).auto_shrink([false, true]).show(ui, |ui| self.places_list(ui, &mut go_to));
            });
            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.set_width((rest - 150.0 - 30.0).max(120.0));
                egui::ScrollArea::vertical().id_salt("pick-entries").max_height(240.0).auto_shrink([false, true]).show(ui, |ui| self.entries_list(ui, &mut go_to, &mut click));
            });
        });
        if let Some(d) = go_to {
            self.go(d);
            self.focus = Some(if self.req.mode == Mode::Save { Field::Name } else { Field::Path });
        }
        if let Some(name) = click {
            self.click_file(&name);
        }
        if matches!(self.req.mode, Mode::File | Mode::Files | Mode::Save) && self.kinds.len() > 1 {
            ui.horizontal(|ui| {
                ui.label(crate::app::muted("Show"));
                let before = self.filter;
                let shown = self.kinds.get(self.filter).map(|k| k.name.clone()).unwrap_or_default();
                egui::ComboBox::from_id_salt("pick-filter").selected_text(shown).show_pointer(ui, |ui| {
                    for (i, k) in self.kinds.iter().enumerate() {
                        ui.selectable_value(&mut self.filter, i, &k.name);
                    }
                });
                if before != self.filter {
                    self.relist();
                }
            });
        }
        if self.req.mode == Mode::Files && !self.selected.is_empty() {
            crate::theme::hint(ui, format!("{} selected", self.selected.len()));
        }
        if self.req.mode == Mode::Save {
            ui.horizontal(|ui| {
                ui.label("File name");
                let r = ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(f32::INFINITY));
                if self.wants(Field::Name) {
                    r.request_focus();
                }
                if r.lost_focus() && enter {
                    *intent = Some(Intent::Choose);
                }
            });
        }
        if matches!(self.req.mode, Mode::Folder | Mode::Save) {
            ui.horizontal(|ui| {
                ui.label("New folder");
                let r = ui.add(egui::TextEdit::singleline(&mut self.new_folder).hint_text("Name, made in the folder shown above").desired_width(260.0));
                if self.wants(Field::NewFolder) {
                    r.request_focus();
                }
                let go = ui.button("Make and use").on_hover_text("Make the folder inside the folder shown above and use it").clicked();
                if go || (r.lost_focus() && enter) {
                    *intent = Some(Intent::MakeFolder);
                }
            });
        }
        if let Some(label) = self.req.option.clone() {
            crate::clickable::pointer(ui.checkbox(&mut self.option, label));
        }
        if let Some(h) = &self.req.hint {
            crate::theme::hint(ui, h.as_str());
        }
        if let Some(e) = &self.error {
            crate::theme::problem(ui, e.as_str());
        }
        if self.existing.is_some() && ui.button("Use that folder").clicked() {
            *intent = Some(Intent::UseExisting);
        }
        if let (true, Some(label)) = (self.offer_shown, self.req.offer.clone()) {
            if ui.button(label).clicked() {
                *intent = Some(Intent::Offer);
            }
        }
    }

    fn places_list(&mut self, ui: &mut Ui, go_to: &mut Option<PathBuf>) {
        if self.places.is_empty() {
            crate::theme::hint(ui, if self.places_rx.is_some() { "Looking…" } else { "No shortcuts" });
        }
        for group in [Group::Places, Group::Vaults, Group::Libraries, Group::Drives] {
            let mut first = true;
            for place in self.places.iter().filter(|p| p.group == group) {
                if std::mem::take(&mut first) {
                    ui.add_space(4.0);
                    crate::theme::hint(ui, group.title());
                }
                if clickable(ui, Label::new(&place.label).truncate()).on_hover_text(place.path.to_string_lossy()).clicked() {
                    *go_to = Some(place.path.clone());
                }
            }
        }
    }

    fn entries_list(&mut self, ui: &mut Ui, go_to: &mut Option<PathBuf>, click: &mut Option<String>) {
        if self.folders.is_empty() && self.files.is_empty() {
            crate::theme::hint(ui, "Nothing to choose in this folder.");
        }
        for name in &self.folders {
            if clickable(ui, Label::new(format!("📁  {name}")).truncate()).clicked() {
                *go_to = Some(self.dir.join(name));
            }
        }
        for name in &self.files {
            if ui.selectable_label(self.selected.contains(name), format!("📄  {name}")).clicked() {
                *click = Some(name.clone());
            }
        }
    }

    fn footer(&mut self, ui: &mut Ui, intent: &mut Option<Intent>) {
        ui.horizontal(|ui| {
            if ui.button(egui::RichText::new(self.req.button.as_str()).strong()).clicked() {
                *intent = Some(Intent::Choose);
            }
            if ui.button("Cancel").clicked() {
                *intent = Some(Intent::Cancel);
            }
        });
    }
}
