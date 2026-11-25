//! One file and folder picker for the whole of Shale (PICK-001, ADR-0043, #690, #688).
//!
//! A part of the window that needs a path (Open…, Create…, a library folder, a template set, an
//! attachment, the place of an export, the Import dialog) asks with [`ask`], naming itself by a
//! `key`, and takes the answer on a later frame with [`take`]. The window's [`Host`] answers: it
//! shows the system's own dialog first (the xdg-desktop-portal on Linux, the native dialog of
//! Windows and macOS) on a worker thread, so the window keeps drawing, and falls back to Shale's
//! own picker ([`own`]) when the person turned the system dialog off in the settings or when none
//! answers (no portal). The system's dialog is behind the [`Native`] trait, so that the tests run
//! the code around it with a pretend dialog.

pub mod confirm;
pub mod names;
pub mod own;
pub mod places;

use eframe::egui::{self, Context};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};

/// What is chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// one folder
    Folder,
    /// one file
    File,
    /// one file or more
    Files,
    /// the place and the name of a file to write
    Save,
}

/// A kind of file the picker can be narrowed to: a name for the list ("Pictures") and the
/// extensions (without the dot). No extensions: every file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filter {
    pub name: String,
    pub exts: Vec<String>,
}

impl Filter {
    pub fn new(name: &str, exts: &[&str]) -> Filter {
        Filter { name: name.to_string(), exts: exts.iter().map(|e| e.to_string()).collect() }
    }

    /// Whether the file `name` is of this kind (any case of the extension).
    pub fn matches(&self, name: &str) -> bool {
        self.exts.is_empty() || Path::new(name).extension().is_some_and(|x| self.exts.iter().any(|e| x.eq_ignore_ascii_case(e)))
    }
}

/// What a part of the window asks the picker for.
#[derive(Clone, Debug)]
pub struct Request {
    pub mode: Mode,
    pub title: String,
    /// the kinds of file (Files, File, Save); the first is the one chosen at first
    pub filters: Vec<Filter>,
    /// where to start: a folder, or a typed path that is not there (Shale's own picker shows it)
    pub start: Option<PathBuf>,
    /// the name offered to a file that is saved
    pub file_name: String,
    /// the text of the button that chooses
    pub button: String,
    /// a line of explanation under the list (Shale's own picker)
    pub hint: Option<String>,
    /// a box that only Shale's own picker has: its label (Create: "Start with sample notes")
    pub option: Option<String>,
    /// the box starts ticked
    pub option_on: bool,
    /// a button that only Shale's own picker shows after a refusal: its label
    pub offer: Option<String>,
    /// why the last choice was refused: the system's dialog cannot show it, so Shale's own does
    pub refused: Option<String>,
}

impl Request {
    fn new(mode: Mode, title: &str, button: &str) -> Request {
        Request { mode, title: title.to_string(), filters: vec![], start: None, file_name: String::new(), button: button.to_string(), hint: None, option: None, option_on: false, offer: None, refused: None }
    }

    /// One folder.
    pub fn folder(title: &str) -> Request {
        Request::new(Mode::Folder, title, "Choose")
    }

    /// One file.
    pub fn file(title: &str) -> Request {
        Request::new(Mode::File, title, "Choose")
    }

    /// One file or more.
    pub fn files(title: &str) -> Request {
        Request::new(Mode::Files, title, "Add")
    }

    /// The place and name of a file that is written; `name` is offered.
    pub fn save(title: &str, name: &str) -> Request {
        let mut r = Request::new(Mode::Save, title, "Save");
        r.file_name = name.to_string();
        r
    }

    pub fn filter(mut self, f: Filter) -> Request {
        self.filters.push(f);
        self
    }

    pub fn filters(mut self, fs: Vec<Filter>) -> Request {
        self.filters = fs;
        self
    }

    pub fn start(mut self, dir: impl Into<PathBuf>) -> Request {
        self.start = Some(dir.into());
        self
    }

    pub fn button(mut self, text: &str) -> Request {
        self.button = text.to_string();
        self
    }

    pub fn hint(mut self, text: &str) -> Request {
        self.hint = Some(text.to_string());
        self
    }

    pub fn option(mut self, label: &str) -> Request {
        self.option = Some(label.to_string());
        self
    }

    pub fn option_checked(mut self, on: bool) -> Request {
        self.option_on = on;
        self
    }

    pub fn offer(mut self, label: &str) -> Request {
        self.offer = Some(label.to_string());
        self
    }

    pub fn refused(mut self, why: impl Into<String>) -> Request {
        self.refused = Some(why.into());
        self
    }
}

/// What the person chose.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Picked {
    pub paths: Vec<PathBuf>,
    /// the box of [`Request::option`] was ticked
    pub option: bool,
    /// the button of [`Request::offer`] was pressed
    pub offered: bool,
}

impl Picked {
    pub fn first(&self) -> Option<&Path> {
        self.paths.first().map(PathBuf::as_path)
    }
}

/// The answer of the system's dialog.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Picked(Vec<PathBuf>),
    Cancelled,
    /// no dialog could be shown (no portal, no desktop): Shale's own picker takes over
    Unavailable,
}

/// The system's own dialog. `pick` blocks until the person has answered: it is called from a worker thread.
pub trait Native: Send + Sync {
    fn pick(&self, req: &Request) -> Outcome;
}

/// The dialog of the system: the xdg-desktop-portal on Linux, the native one on Windows and macOS.
pub struct System;

impl Native for System {
    fn pick(&self, req: &Request) -> Outcome {
        let started = std::time::Instant::now();
        let mut d = rfd::FileDialog::new().set_title(&req.title);
        if let Some(dir) = req.start.as_ref().filter(|d| d.is_dir()) {
            d = d.set_directory(dir);
        }
        // a list that starts with "all files" adds none: the system's dialog opens on the first
        // filter, and it would hide the files the request wants to allow (the Import dialog)
        if req.filters.first().is_some_and(|f| !f.exts.is_empty()) {
            for f in req.filters.iter().filter(|f| !f.exts.is_empty()) {
                d = d.add_filter(&f.name, &f.exts);
            }
        }
        let chosen = match req.mode {
            Mode::Folder => d.pick_folder().map(|p| vec![p]),
            Mode::File => d.pick_file().map(|p| vec![p]),
            Mode::Files => d.pick_files(),
            Mode::Save => d.set_file_name(&req.file_name).save_file().map(|p| vec![p]),
        };
        match chosen {
            Some(paths) if !paths.is_empty() => Outcome::Picked(paths),
            // rfd answers "nothing" both for Cancel and for a dialog that could not open; nobody
            // cancels within a quarter of a second, so that is a dialog that did not open (SOC-058)
            _ if cfg!(target_os = "linux") && started.elapsed() < std::time::Duration::from_millis(250) => Outcome::Unavailable,
            _ => Outcome::Cancelled,
        }
    }
}

/// A pretend system dialog for the tests: it answers these paths.
#[cfg(test)]
pub(crate) struct Answers(pub Vec<PathBuf>);

#[cfg(test)]
impl Native for Answers {
    fn pick(&self, _: &Request) -> Outcome {
        Outcome::Picked(self.0.clone())
    }
}

/// No system dialog: what a test build has, so that no test ever opens a real dialog.
pub struct NoSystem;

impl Native for NoSystem {
    fn pick(&self, _: &Request) -> Outcome {
        Outcome::Unavailable
    }
}

/// The mailbox between the parts of the window that ask and the [`Host`] that answers. The host
/// puts a handle in the egui context every frame, so that code that has a `Ui` and no app can ask.
#[derive(Clone, Default)]
pub struct Hub(Arc<Mutex<HubState>>);

#[derive(Default)]
struct HubState {
    asked: Vec<(String, Request)>,
    answered: HashMap<String, Picked>,
}

impl Hub {
    /// Ask for a path; a second ask with the same key replaces the first.
    pub fn ask(&self, key: &str, req: Request) {
        let mut s = self.0.lock().unwrap_or_else(|e| e.into_inner());
        s.asked.retain(|(k, _)| k != key);
        s.asked.push((key.to_string(), req));
    }

    /// The answer to the ask `key`, once; none while the person has not answered, or after Cancel.
    pub fn take(&self, key: &str) -> Option<Picked> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).answered.remove(key)
    }

    fn drain_asks(&self) -> Vec<(String, Request)> {
        std::mem::take(&mut self.0.lock().unwrap_or_else(|e| e.into_inner()).asked)
    }

    fn answer(&self, key: &str, picked: Picked) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).answered.insert(key.to_string(), picked);
    }
}

fn hub_id() -> egui::Id {
    egui::Id::unique("shale-file-picker-hub")
}

/// Ask for a path from a part of the window that has the egui context only (the host has put the
/// hub there). Take the answer with [`take`] on a later frame.
pub fn ask(ctx: &Context, key: &str, req: Request) {
    if let Some(h) = ctx.data(|d| d.get_temp::<Hub>(hub_id())) {
        h.ask(key, req);
        ctx.request_repaint();
    }
}

/// The answer to the ask `key` (see [`ask`]).
pub fn take(ctx: &Context, key: &str) -> Option<Picked> {
    ctx.data(|d| d.get_temp::<Hub>(hub_id())).and_then(|h| h.take(key))
}

/// A **Browse** button for a text field that holds a path (PICK-001): a click asks the picker with
/// the request `req` makes from the text of the field, and the answer, on a later frame, replaces
/// that text. Returns the button.
pub fn browse_button(ui: &mut egui::Ui, key: &str, label: &str, req: impl FnOnce(&str) -> Request, field: &mut String) -> egui::Response {
    let button = ui.button(label);
    if button.clicked() {
        ask(ui.ctx(), key, req(field));
    }
    if let Some(p) = take(ui.ctx(), key).and_then(|p| p.paths.into_iter().next()) {
        *field = p.to_string_lossy().into_owned();
    }
    button
}

/// The request for a file that is written to the path typed in `field` (an export): its folder is
/// where the picker starts when it is there, its name is offered, and `filter` is the kind of file.
pub fn save_request(title: &str, field: &str, filter: Filter) -> Request {
    let typed = PathBuf::from(crate::layout::shellexpand(field));
    let name = typed.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let req = Request::save(title, &name).filter(filter);
    match typed.parent().filter(|d| d.is_dir()) {
        Some(dir) => req.start(dir),
        None => req,
    }
}

/// What is shown for the ask that is being answered.
enum Stage {
    /// the system's dialog is open on a worker thread
    System(Receiver<Result<Outcome, String>>),
    Own(Box<own::OwnPicker>),
    /// the system's dialog chose, and the box of the request is asked (PICK-011)
    Confirm(Box<confirm::Confirm>),
}

/// What a step of a dialog of Shale's means for the ask: it goes on, is dropped, or is answered.
fn finish(hub: &Hub, key: &str, step: own::Step) -> After {
    match step {
        own::Step::Keep => After::Stay,
        own::Step::Cancel => After::Done,
        own::Step::Done(picked) => {
            hub.answer(key, picked);
            After::Done
        }
    }
}

struct Active {
    key: String,
    req: Request,
    stage: Stage,
}

/// What a frame leaves of the ask that is being answered.
enum After {
    Stay,
    Done,
    /// the system's dialog chose these paths, and the request has a box to ask
    Confirm(Vec<PathBuf>),
    /// the system's dialog could not be shown
    Own,
}

/// Answers the asks of the window, one at a time: the system's dialog first, Shale's own picker
/// when that is off or has no dialog to show (PICK-002, PICK-003).
pub struct Host {
    hub: Hub,
    native: Arc<dyn Native>,
    queue: VecDeque<(String, Request)>,
    active: Option<Active>,
}

impl Default for Host {
    fn default() -> Self {
        // a test build has no system dialog, and nor has a run with `SHALE_OWN_FILE_PICKER` set, which
        // the screenshots of Shale's own picker use (docs/screenshots/recipes.conf)
        let own_only = cfg!(test) || std::env::var_os("SHALE_OWN_FILE_PICKER").is_some();
        Host::with_native(if own_only { Arc::new(NoSystem) } else { Arc::new(System) })
    }
}

impl Host {
    pub fn with_native(native: Arc<dyn Native>) -> Host {
        Host { hub: Hub::default(), native, queue: VecDeque::new(), active: None }
    }

    /// Ask for a path.
    pub fn ask(&self, key: &str, req: Request) {
        self.hub.ask(key, req);
    }

    /// The answer to the ask `key`, once.
    pub fn take(&self, key: &str) -> Option<Picked> {
        self.hub.take(key)
    }

    /// Whether the system's dialog is open or Shale's own picker is looking at the disk: what the
    /// headless tests wait for (DEV-041).
    #[cfg(test)]
    pub(crate) fn running(&self) -> bool {
        match &self.active {
            Some(Active { stage: Stage::System(_), .. }) => true,
            Some(Active { stage: Stage::Own(p), .. }) => p.scanning(),
            _ => false,
        }
    }

    /// Shale's own picker, when it is open (the tests drive it).
    #[cfg(test)]
    pub(crate) fn own_picker(&mut self) -> Option<&mut own::OwnPicker> {
        match &mut self.active {
            Some(Active { stage: Stage::Own(p), .. }) => Some(p),
            _ => None,
        }
    }

    /// Whether the step after the system's dialog (the box of the request) is open (the tests look).
    #[cfg(test)]
    pub(crate) fn confirm_open(&self) -> bool {
        matches!(&self.active, Some(Active { stage: Stage::Confirm(_), .. }))
    }

    /// One frame: take in new asks, start the next one, and carry the open one on. `system` is the
    /// setting "Use the system's file dialog"; `places` makes the sources of the shortcuts, which
    /// only a picker that opens now needs.
    pub fn frame(&mut self, ctx: &Context, system: bool, places: impl FnOnce() -> places::Inputs) {
        ctx.data_mut(|d| d.insert_temp(hub_id(), self.hub.clone()));
        for (key, req) in self.hub.drain_asks() {
            self.queue.retain(|(k, _)| *k != key);
            self.queue.push_back((key, req));
        }
        let mut places = Some(places);
        if self.active.is_none() {
            if let Some((key, req)) = self.queue.pop_front() {
                self.active = Some(self.start(ctx, key, req, system, &mut places));
            }
        }
        let Some(mut active) = self.active.take() else { return };
        let next = match &mut active.stage {
            Stage::System(rx) => match rx.try_recv() {
                // a request with a box the system's dialog cannot hold asks it in a step of Shale's own (PICK-011)
                Ok(Ok(Outcome::Picked(paths))) if active.req.option.is_some() => After::Confirm(paths),
                Ok(Ok(Outcome::Picked(paths))) => {
                    self.hub.answer(&active.key, Picked { paths, ..Picked::default() });
                    After::Done
                }
                Ok(Ok(Outcome::Cancelled)) => After::Done,
                // no dialog could be shown, or it crashed: Shale's own picker
                Ok(Ok(Outcome::Unavailable)) | Ok(Err(_)) | Err(TryRecvError::Disconnected) => After::Own,
                Err(TryRecvError::Empty) => After::Stay,
            },
            Stage::Own(p) => finish(&self.hub, &active.key, p.show(ctx)),
            Stage::Confirm(c) => finish(&self.hub, &active.key, c.show(ctx)),
        };
        match next {
            After::Stay => self.active = Some(active),
            After::Done => {}
            After::Confirm(paths) => {
                ctx.request_repaint();
                active.stage = Stage::Confirm(Box::new(confirm::Confirm::new(active.req.clone(), paths)));
                self.active = Some(active);
            }
            After::Own => {
                let Active { key, req, .. } = active;
                self.active = Some(self.start_own(ctx, key, req, &mut places));
            }
        }
    }

    fn start(&self, ctx: &Context, key: String, req: Request, system: bool, places: &mut Option<impl FnOnce() -> places::Inputs>) -> Active {
        // a refused choice is shown with its reason, which the system's dialog cannot do
        if !system || req.refused.is_some() {
            return self.start_own(ctx, key, req, places);
        }
        let (native, job_req, repaint) = (self.native.clone(), req.clone(), ctx.clone());
        let rx = crate::jobs::spawn(move || native.pick(&job_req), move || repaint.request_repaint());
        Active { key, req, stage: Stage::System(rx) }
    }

    fn start_own(&self, ctx: &Context, key: String, req: Request, places: &mut Option<impl FnOnce() -> places::Inputs>) -> Active {
        let inputs = places.take().map(|f| f()).unwrap_or_default();
        let picker = own::OwnPicker::open(req.clone(), inputs, ctx.clone());
        Active { key, req, stage: Stage::Own(Box::new(picker)) }
    }
}

#[cfg(test)]
mod host_tests;
#[cfg(test)]
mod own_ui_tests;
