//! Application state, actions and the main window layout.

use crate::config::Config;
use crate::drawing::DrawingDoc;
use crate::editor::{EditorAction, NoteEditor};
use crate::graph::{GraphClick, GraphView};
use crate::markdown;
use crate::reader::{ReaderAction, SceneCache};
use crate::theme;
use crate::vault::{self, rel_to_string, FileKind, Saved, Vault};
use crate::vault_path::VaultPath;
use eframe::egui::{self, Key, Modifiers, Ui};
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Gives each app built without persistence (tests, screenshots) its own mail folder.
static NEXT_MAIL_ROOT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub struct Options {
    pub screenshot: Option<PathBuf>,
    pub vault: Option<PathBuf>,
    /// Testing aid: which view to show first (tasks, graph, drawing, reading).
    pub view: Option<String>,
    /// Testing aid: keys typed into the window (see `--keys`).
    pub keys: Option<String>,
}

#[derive(Clone, PartialEq, Debug)]
pub enum View {
    Empty,
    Note(PathBuf),
    Drawing(PathBuf),
    Tasks,
    Graph,
    Calendar,
    Mail,
}

impl View {
    pub fn path(&self) -> Option<&Path> {
        match self {
            View::Note(p) | View::Drawing(p) => Some(p),
            _ => None,
        }
    }
}

pub struct Tab {
    pub id: u64,
    pub view: View,
    pub back: Vec<View>,
    pub fwd: Vec<View>,
    pub reading: bool,
    pub editor: NoteEditor,
    pub title_buf: String,
    pub title_for: Option<PathBuf>,
}

pub struct NoteDoc {
    pub text: String,
    pub dirty: bool,
    pub last_edit: Instant,
    /// What the file held when this buffer was loaded or last saved; a different content on
    /// disk at save time is a conflict (DATA-003).
    pub base: crate::vault::Stamp,
    /// Why the file could not be read: the note is read-only and never written (DATA-008).
    pub unreadable: Option<String>,
    /// Why the last save failed; the buffer stays dirty and the tab is marked (DATA-009).
    pub save_error: Option<String>,
}

impl NoteDoc {
    pub fn loaded(text: String, unreadable: Option<String>) -> NoteDoc {
        NoteDoc { base: crate::vault::Stamp::of(&text), text, dirty: false, last_edit: Instant::now(), unreadable, save_error: None }
    }

    /// Take the text now on disk (the buffer had no edits to keep, or they are saved elsewhere).
    pub fn reload(&mut self, text: String, unreadable: Option<String>) {
        self.base = crate::vault::Stamp::of(&text);
        self.text = text;
        self.dirty = false;
        self.unreadable = unreadable;
        self.save_error = None;
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LeftPanel {
    Files,
    Search,
    Tags,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RightPanel {
    Backlinks,
    Outgoing,
    Outline,
    LocalGraph,
    Calendar,
}

mod drops;
mod mail;
mod sync;

/// A file "dropped" by a `--keys` script.
#[derive(Debug)]
struct ScriptDrop(PathBuf);

impl egui::DroppedFile for ScriptDrop {
    fn path(&self) -> &Path {
        &self.0
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        crate::fsutil::read(&self.0).map_err(|e| e.to_string())
    }
}

pub enum Modal {
    QuickSwitch { query: String, sel: usize },
    Rename { rel: PathBuf, name: String, error: Option<String> },
    NewFolder { parent: PathBuf, name: String },
    ConfirmTrash { rel: PathBuf },
    Settings { vault_path: String },
    Export(Box<crate::export_dialog::ExportDialog>),
    Calendars(Box<crate::cal_dialog::CalendarDialog>),
    Event(Box<crate::cal_dialog::EventDialog>),
    Import(Box<crate::import_dialog::ImportDialog>),
    MailAccounts(Box<crate::mail_dialog::MailDialog>),
}

pub enum Action {
    /// open the "Export to PDF" dialog for a note or drawing
    Export(PathBuf),
    Open {
        rel: PathBuf,
        new_tab: bool,
    },
    OpenLink {
        target: String,
        heading: Option<String>,
        from: Option<PathBuf>,
        new_tab: bool,
    },
    /// "Fix link": point the red links `target` in `note` at `to` (LINK-012)
    FixLink {
        note: PathBuf,
        target: String,
        to: PathBuf,
    },
    NewNote {
        folder: PathBuf,
    },
    NewDrawing {
        folder: PathBuf,
        embed_into: Option<PathBuf>,
    },
    ToggleTask {
        rel: PathBuf,
        offset: usize,
    },
    GotoLine {
        rel: PathBuf,
        line: usize,
    },
    /// open a note with the cursor at `col` (byte offset) of `line`, keyboard in the editor
    GotoMatch {
        rel: PathBuf,
        line: usize,
        col: usize,
    },
    ShowView(View),
    CloseTab(usize),
    Rename(PathBuf),
    Trash(PathBuf),
    NewFolder(PathBuf),
    OpenVault(PathBuf),
    /// Look at the vault folder again (e.g. after a file was copied in).
    Rescan,
    /// open a web or mail address (checked, see `opener`)
    OpenUrl(String),
    /// make a contact note in the people folder (from `[[p Name`)
    CreatePerson(String),
    /// move a file or folder into a folder (drag and drop in the file list)
    Move {
        from: PathBuf,
        into: PathBuf,
    },
}

#[derive(Default)]
pub struct TasksFilter {
    pub show_done: bool,
    pub query: String,
    pub only_due: bool,
}

pub struct ShaleApp {
    pub cfg: Config,
    pub persist_config: bool,
    /// The open vault's own settings and session state (`<vault>/.shale/`, ADR-0005).
    pub vdata: crate::vault_state::VaultData,
    /// Git sync of the open vault (ADR-0008).
    pub sync: sync::SyncState,
    pub vault: Option<Vault>,
    pub tabs: Vec<Tab>,
    pub active: usize,
    next_tab_id: u64,
    pub notes: HashMap<PathBuf, NoteDoc>,
    pub drawings: HashMap<PathBuf, DrawingDoc>,
    pub left: LeftPanel,
    pub right: RightPanel,
    pub expanded: HashSet<PathBuf>,
    pub search: String,
    /// the selected stop in the search results while they are browsed with the keyboard
    pub search_sel: Option<usize>,
    /// scroll the selected search result into view on the next frame
    pub(crate) search_scroll: bool,
    /// the selected row of the file list while it has the keyboard (Ctrl+H, Ctrl+Shift+B)
    pub tree_sel: Option<PathBuf>,
    /// scroll the selected file list row into view on the next frame
    pub(crate) tree_scroll: bool,
    /// focus the search box (and select its text) in the next frame
    pub focus_search: bool,
    pub tasks: TasksFilter,
    pub graph: GraphView,
    pub local_graph: GraphView,
    pub calendar: crate::cal_store::CalendarStore,
    /// The mail view (ADR-0007 phase 1b).
    pub mail: crate::mail_ui::MailView,
    pub cal_side: crate::cal_ui::CalendarView,
    pub cal_main: crate::cal_ui::CalendarView,
    /// deletes started from the event popup: (job, calendar)
    pub cal_jobs: Vec<(crate::cal_store::Job, String)>,
    /// the calendar the last event was saved to (the default for the next new one)
    pub last_event_calendar: Option<String>,
    /// for background jobs to wake the window up when they finish
    ctx: Option<egui::Context>,
    /// where things can be dropped this frame
    drop_zones: Vec<drops::DropZone>,
    /// file list drop targets this frame: (row, folder to move into, a closed folder row)
    pub(crate) tree_targets: Vec<(egui::Rect, PathBuf, bool)>,
    /// a closed folder hovered while dragging, since when (it opens after a moment)
    tree_hover: Option<(PathBuf, Instant)>,
    calendar_areas: Vec<egui::Rect>,
    /// something from another program is dragged over the window (Wayland)
    ext_hover: Option<egui::Pos2>,
    /// the kinds the last drag from another program offered
    last_offered: Vec<String>,
    /// something Shale can't take is dragged over the window: where, and what it offers
    ext_refused: Option<(egui::Pos2, Vec<String>)>,
    /// test drops from `--keys` (handled like drops from other programs)
    #[cfg(target_os = "linux")]
    fake_drops: Vec<crate::wl_drop::DropEvent>,
    /// drops being read in the background, and where they were dropped
    /// a drop from another program is being read
    drop_busy: bool,
    /// words typical for each tag (learned from the vault, see tagwords)
    pub tag_words: Option<crate::tagwords::TagWords>,
    tag_job: Option<std::sync::mpsc::Receiver<Result<crate::tagwords::TagWords, String>>>,
    tag_root: Option<PathBuf>,
    tag_gen: u64,
    tag_at: Option<Instant>,
    pending_drops: Vec<(std::sync::mpsc::Receiver<Result<(Vec<crate::import::Item>, Vec<String>), String>>, Option<(PathBuf, Option<PathBuf>)>)>,
    pub scenes: SceneCache,
    pub modal: Option<Modal>,
    pub actions: Vec<Action>,
    pub toasts: Vec<(String, Instant)>,
    last_scan: Instant,
    pub welcome_path: String,
    // screenshot mode
    screenshot: Option<PathBuf>,
    frames: u32,
    shot_requested: bool,
    shot_done: Arc<AtomicBool>,
    /// the last failure to save the global settings (shown once)
    cfg_error: Option<String>,
    /// scripted input steps for tests of the real window
    script: std::collections::VecDeque<String>,
    /// where text that could not be saved at exit is put (DATA-011)
    pub recovery_root: PathBuf,
    /// recovery folders from earlier runs the person has not dismissed yet
    pub recovery_notice: Vec<PathBuf>,
}

impl ShaleApp {
    pub fn new(cc: &eframe::CreationContext<'_>, opts: Options) -> Self {
        theme::apply(&cc.egui_ctx);
        #[cfg(target_os = "linux")]
        {
            // drops from other programs on Wayland (a second listener; see wl_drop)
            crate::wl_drop::set_context(&cc.egui_ctx);
            crate::wl_drop::attach(cc);
        }
        let persist = opts.screenshot.is_none();
        Self::build(opts, persist)
    }

    /// Set up the app without a window. `persist` = read/write the user's config file.
    pub fn build(opts: Options, persist: bool) -> Self {
        let screenshot = opts.screenshot.clone();
        let (mut cfg, cfg_problem) = if persist { Config::load() } else { (Config::default(), None) };
        let persist_config = persist;
        // screenshots and scripted checks of vim-lite: SHALE_VIM=1 turns it on
        if opts.screenshot.is_some() && std::env::var("SHALE_VIM").as_deref() == Ok("1") {
            cfg.vim_mode = true;
        }
        // Which vault to open: --vault, else (normal start) the last one, else a demo for screenshots.
        let vault_path = match (&opts.vault, &screenshot) {
            (Some(v), _) => Some(v.clone()),
            (None, Some(_)) => {
                let d = std::env::temp_dir().join("shale-demo-vault");
                let _ = std::fs::remove_dir_all(&d);
                let _ = vault::write_sample_vault(&d);
                Some(d)
            }
            (None, None) => cfg.vault.clone().filter(|p| p.is_dir()),
        };
        let default_welcome = dirs::document_dir().or_else(dirs::home_dir).unwrap_or_default().join("Notes");
        let mut app = ShaleApp {
            cfg: cfg.clone(),
            persist_config,
            vdata: crate::vault_state::VaultData::default(),
            sync: sync::SyncState::default(),
            vault: None,
            tabs: vec![],
            active: 0,
            next_tab_id: 1,
            notes: HashMap::new(),
            drawings: HashMap::new(),
            left: LeftPanel::Files,
            right: RightPanel::Backlinks,
            expanded: HashSet::new(),
            search: String::new(),
            search_sel: None,
            search_scroll: false,
            tree_sel: None,
            tree_scroll: false,
            focus_search: false,
            tasks: TasksFilter::default(),
            graph: GraphView::new(false),
            local_graph: GraphView::new(true),
            calendar: crate::cal_store::CalendarStore::new(cfg.calendars.clone(), if persist { crate::cal_store::default_local_dir() } else { std::env::temp_dir().join(format!("shale-calendars-{}", std::process::id())) }),
            mail: crate::mail_ui::MailView::new(if persist { crate::mail_store::default_root() } else { std::env::temp_dir().join(format!("shale-mail-{}-{}", std::process::id(), NEXT_MAIL_ROOT.fetch_add(1, Ordering::SeqCst))) }),
            cal_side: crate::cal_ui::CalendarView::new(crate::cal_ui::Size::Small),
            cal_main: crate::cal_ui::CalendarView::new(crate::cal_ui::Size::Medium),
            cal_jobs: vec![],
            last_event_calendar: None,
            ctx: None,
            drop_zones: vec![],
            tree_targets: vec![],
            tree_hover: None,
            calendar_areas: vec![],
            ext_hover: None,
            last_offered: vec![],
            ext_refused: None,
            #[cfg(target_os = "linux")]
            fake_drops: vec![],
            pending_drops: vec![],
            drop_busy: false,
            tag_words: None,
            tag_job: None,
            tag_root: None,
            tag_gen: u64::MAX,
            tag_at: None,
            scenes: SceneCache::default(),
            modal: None,
            actions: vec![],
            toasts: vec![],
            last_scan: Instant::now(),
            welcome_path: default_welcome.to_string_lossy().into_owned(),
            screenshot,
            frames: 0,
            shot_requested: false,
            shot_done: Arc::new(AtomicBool::new(false)),
            cfg_error: None,
            script: opts.keys.as_deref().map(|k| k.split('|').map(|x| x.to_string()).collect()).unwrap_or_default(),
            recovery_root: if persist { crate::recovery::default_root() } else { std::env::temp_dir().join(format!("shale-recovery-{}", std::process::id())) },
            recovery_notice: vec![],
        };
        app.recovery_notice = crate::recovery::unseen(&app.recovery_root);
        if let Some(p) = cfg_problem {
            app.toast(p);
        }
        if let Some(p) = vault_path {
            app.open_vault(&p);
            if app.screenshot.is_some() {
                app.open(Path::new("Welcome.md"), false);
            }
            // screenshots: a demo calendar full of sample events
            if app.screenshot.is_some() && app.cfg.calendars.is_empty() {
                let src = app.calendar.new_local_source("Demo calendar", &[]);
                let id = src.id.clone();
                app.cfg.calendars = vec![src];
                app.calendar.set_sources(app.cfg.calendars.clone());
                let _ = app.calendar.fill_sample(&id);
            }
            match opts.view.as_deref() {
                Some("calendar") => app.show_view(View::Calendar, false),
                Some("mail") => app.show_view(View::Mail, false),
                Some("calendar-side") => app.right = RightPanel::Calendar,
                Some("tasks") => app.show_view(View::Tasks, false),
                Some("graph") => app.show_view(View::Graph, false),
                Some("drawing") => app.open(Path::new("Sketch.excalidraw"), false),
                // any file of the vault, e.g. `--view "open:Excalidraw/Plan.excalidraw.md"`
                Some(v) if v.starts_with("open:") => app.open(Path::new(&v["open:".len()..]), false),
                Some("reading") => {
                    if let Some(t) = app.tabs.get_mut(app.active) {
                        t.reading = true;
                    }
                }
                _ => {}
            }
        }
        // SHALE_INPUT_LOG=1 turns the key recording on from the command line too
        if std::env::var_os("SHALE_INPUT_LOG").is_some() {
            app.cfg.input_log = true;
        }
        if app.cfg.input_log {
            app.start_input_log();
        }
        cfg = app.cfg.clone();
        let _ = cfg;
        app
    }

    pub fn toast(&mut self, msg: impl Into<String>) {
        self.toasts.push((msg.into(), Instant::now()));
    }

    /// Save the global config and the open vault's `.shale/` files.
    pub fn save_config(&mut self) {
        if self.persist_config {
            // shown once per message, not at every save
            if let Err(e) = self.cfg.save() {
                if self.cfg_error.as_deref() != Some(e.as_str()) {
                    self.toast(e.clone());
                }
                self.cfg_error = Some(e);
            } else {
                self.cfg_error = None;
            }
            if let Err(e) = self.vdata.save() {
                self.toast(e);
            }
        }
    }

    /// Load the vault's own settings and state; the first vault opened after an upgrade takes
    /// over the per-vault fields of the old global config (CFG-008).
    fn load_vault_data(&mut self, root: &Path) {
        let (mut d, problems) = crate::vault_state::VaultData::load(root);
        for p in problems {
            self.toast(p);
        }
        let legacy = self.cfg.take_legacy();
        if !legacy.is_empty() && !d.writable() {
            // keep them in the global config until a vault can take them
            self.cfg.restore_legacy(legacy);
        } else if !legacy.is_empty() {
            d.adopt_legacy(&legacy);
        }
        self.vdata = d;
    }

    pub fn open_vault(&mut self, path: &Path) {
        self.flush_all();
        // closing the buffers would lose edits that could not be saved (DATA-009)
        if let Some(bad) = self.notes.iter().find(|(_, d)| d.dirty).map(|(k, _)| k).or_else(|| self.drawings.iter().find(|(_, d)| d.dirty).map(|(k, _)| k)) {
            let bad = rel_to_string(bad);
            self.toast(format!("“{bad}” has changes that could not be saved, so the vault is not switched"));
            return;
        }
        match Vault::open(path) {
            Ok(v) => {
                let had_files = !v.files.is_empty();
                // the vault being left keeps its open tabs
                self.remember_open_files();
                if self.persist_config {
                    if let Err(e) = self.vdata.save() {
                        self.toast(e);
                    }
                }
                self.load_vault_data(&v.root);
                self.vault = Some(v);
                self.tabs.clear();
                self.notes.clear();
                self.drawings.clear();
                self.active = 0;
                self.cfg.remember_vault(path.to_path_buf());
                let reopen: Vec<String> = self.vdata.state.open_files.clone();
                for (i, f) in reopen.iter().enumerate() {
                    let rel = PathBuf::from(f);
                    if self.vault.as_ref().map(|v| v.file(&rel).is_some()).unwrap_or(false) {
                        self.open(&rel, i > 0);
                    }
                }
                if self.tabs.is_empty() {
                    let first = self.vault.as_ref().and_then(|v| v.files.iter().find(|f| f.link_name.eq_ignore_ascii_case("welcome")).or_else(|| v.files.iter().find(|f| f.kind == FileKind::Note)).map(|f| f.rel.clone()));
                    match first {
                        Some(r) => self.open(&r, false),
                        None => self.new_tab(View::Empty),
                    }
                }
                if !had_files {
                    self.toast("Empty vault opened. Create a note with Ctrl+N.");
                }
                self.sync_vault_opened();
                self.save_config();
            }
            Err(e) => self.toast(format!("Could not open vault: {e}")),
        }
    }

    pub fn new_tab(&mut self, view: View) {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        self.tabs.push(Tab { id, view, back: vec![], fwd: vec![], reading: false, editor: NoteEditor::new(egui::Id::unique(("note-editor", id))), title_buf: String::new(), title_for: None });
        self.active = self.tabs.len() - 1;
    }

    pub fn show_view(&mut self, view: View, new_tab: bool) {
        if self.tabs.is_empty() || new_tab {
            self.new_tab(view);
            return;
        }
        // reuse a tab already showing it
        if let Some(i) = self.tabs.iter().position(|t| t.view == view) {
            self.active = i;
            return;
        }
        let tab = &mut self.tabs[self.active];
        if tab.view != view {
            let old = std::mem::replace(&mut tab.view, view);
            if old != View::Empty {
                tab.back.push(old);
            }
            tab.fwd.clear();
        }
    }

    pub fn open(&mut self, rel: &Path, new_tab: bool) {
        let Some(v) = &self.vault else { return };
        let Some(f) = v.file(rel) else {
            self.toast(format!("“{}” no longer exists", rel_to_string(rel)));
            return;
        };
        // expand parent folders in the explorer
        let mut p = rel.parent();
        while let Some(dir) = p {
            if dir.as_os_str().is_empty() {
                break;
            }
            self.expanded.insert(dir.to_path_buf());
            p = dir.parent();
        }
        match f.kind {
            FileKind::Note => {
                // a buffer without edits that is behind the index (the file was replaced since it
                // was loaded) takes the index text instead of showing a stale one
                if let (Some(doc), Some(n)) = (self.notes.get_mut(rel), self.vault.as_ref().and_then(|v| v.notes.get(rel))) {
                    if !doc.dirty && (doc.unreadable != n.unreadable || doc.base != crate::vault::Stamp::of(&n.text)) {
                        doc.reload(n.text.clone(), n.unreadable.clone());
                    }
                }
                if !self.notes.contains_key(rel) {
                    // red path links whose file name exists exactly once are fixed (LINK-012);
                    // there is no buffer for this note yet, so nothing unsaved is in the way
                    let fixed = self.vault.as_mut().map(|v| v.auto_fix_links(rel));
                    // a note that could not be read opens read-only, never as an empty note (DATA-008)
                    let (text, unreadable) = self.vault.as_ref().and_then(|v| v.notes.get(rel)).map(|n| (n.text.clone(), n.unreadable.clone())).unwrap_or_default();
                    self.notes.insert(rel.to_path_buf(), NoteDoc::loaded(text, unreadable));
                    match fixed {
                        Some(Ok(n)) if n > 0 => self.toast(format!("Fixed {n} link(s) whose note had moved")),
                        Some(Err(e)) => self.toast(format!("Could not fix links: {e}")),
                        _ => {}
                    }
                }
                self.show_view(View::Note(rel.to_path_buf()), new_tab);
            }
            FileKind::Drawing => {
                if !self.drawings.contains_key(rel) {
                    // a file that can't be read opens read-only (never an empty drawing that overwrites it)
                    let doc = match crate::fsutil::read_to_string(v.abs(rel)) {
                        Ok(text) => DrawingDoc::new(rel.to_path_buf(), &text),
                        Err(e) => DrawingDoc::unreadable(rel.to_path_buf(), &e.to_string()),
                    };
                    self.drawings.insert(rel.to_path_buf(), doc);
                }
                self.show_view(View::Drawing(rel.to_path_buf()), new_tab);
            }
            FileKind::Other => {
                let abs = v.abs(rel);
                let opened = crate::opener::open_vault_file(&abs, &v.root);
                if let Err(e) = opened {
                    self.toast(e);
                }
            }
        }
        self.remember_open_files();
    }

    pub(crate) fn remember_open_files(&mut self) {
        if self.vault.is_none() {
            return;
        }
        self.vdata.state.open_files = self.tabs.iter().filter_map(|t| t.view.path().map(rel_to_string)).collect();
    }

    pub fn current_path(&self) -> Option<PathBuf> {
        self.tabs.get(self.active).and_then(|t| t.view.path().map(|p| p.to_path_buf()))
    }

    pub fn current_folder(&self) -> PathBuf {
        self.current_path().and_then(|p| p.parent().map(|x| x.to_path_buf())).unwrap_or_default()
    }

    /// Follow a link: open the note if it exists, otherwise create it and open it.
    pub fn open_link(&mut self, target: &str, heading: Option<String>, from: Option<PathBuf>, new_tab: bool) {
        let Some(v) = &self.vault else { return };
        if let Some(rel) = v.resolve(target, from.as_deref()) {
            let line = heading.and_then(|h| v.notes.get(&rel).and_then(|n| markdown::find_headings(&n.text).into_iter().find(|(_, _, t)| t.eq_ignore_ascii_case(&h)).map(|x| x.0)));
            self.open(&rel, new_tab);
            if let Some(line) = line {
                self.goto_line(&rel, line);
            }
            return;
        }
        let t = target.trim();
        if t.starts_with("./") || t.starts_with("../") {
            // a red `./` / `../` link creates the note where it points (LINK-010)
            let base = from.as_ref().and_then(|f| f.parent()).map(|p| p.to_path_buf()).unwrap_or_default();
            match vault::normalize_path(&base, t) {
                Some(p) if !p.is_empty() => {
                    let (folder, name) = match p.rsplit_once('/') {
                        Some((dir, name)) => (PathBuf::from(dir), name.to_string()),
                        None => (PathBuf::new(), p),
                    };
                    self.create_note(&folder, &name, new_tab);
                }
                _ => self.toast("This link points outside the vault"),
            }
            return;
        }
        let folder = if self.cfg.new_notes_next_to_current { from.as_ref().and_then(|f| f.parent()).map(|p| p.to_path_buf()).unwrap_or_default() } else { PathBuf::new() };
        self.create_note(&folder, target, new_tab);
    }

    pub fn create_note(&mut self, folder: &Path, name: &str, new_tab: bool) -> Option<PathBuf> {
        let v = self.vault.as_mut()?;
        let rel = v.new_note_path(folder, name);
        let content = if crate::excalidraw_md::is_plugin_file(&rel) {
            crate::excalidraw_md::new_file(&crate::drawing::Scene::default())
        } else if crate::excalidraw_md::is_drawing_path(&rel) {
            crate::drawing::Scene::default().to_json()
        } else {
            String::new()
        };
        match v.create_file(&rel, &content) {
            Ok(rel) => {
                let shown = v.file(&rel).map(|f| f.display_name()).unwrap_or_default();
                self.toast(format!("Created “{shown}”"));
                self.open(&rel, new_tab);
                if let Some(t) = self.tabs.get_mut(self.active) {
                    t.editor.request_focus = true;
                }
                Some(rel)
            }
            Err(e) => {
                self.toast(format!("Could not create note: {e}"));
                None
            }
        }
    }

    pub fn new_untitled(&mut self, folder: &Path) {
        self.create_note(folder, "Untitled", false);
    }

    pub fn new_drawing(&mut self, folder: &Path, embed_into: Option<PathBuf>) {
        let Some(v) = self.vault.as_mut() else { return };
        let rel = v.new_note_path(folder, "Drawing.excalidraw");
        if let Err(e) = v.create_file(&rel, &crate::drawing::Scene::default().to_json()) {
            self.toast(format!("Could not create drawing: {e}"));
            return;
        }
        let name = v.file(&rel).map(|f| f.link_name.clone()).unwrap_or_default();
        if let Some(note) = embed_into {
            if let Some(doc) = self.notes.get_mut(&note).filter(|d| d.unreadable.is_none()) {
                if !doc.text.ends_with('\n') && !doc.text.is_empty() {
                    doc.text.push('\n');
                }
                doc.text.push_str(&format!("![[{name}]]\n"));
                doc.dirty = true;
                doc.last_edit = Instant::now();
                let text = doc.text.clone();
                if let Some(v) = self.vault.as_mut() {
                    v.set_note_text(&note, &text);
                }
            }
        }
        self.open(&rel, false);
    }

    pub fn goto_line(&mut self, rel: &Path, line: usize) {
        self.goto_match(rel, line, 0);
    }

    /// Open `rel` with the cursor at byte `col` of `line` (clamped to the line) and the keyboard
    /// in the editor.
    pub fn goto_match(&mut self, rel: &Path, line: usize, col: usize) {
        self.open(rel, false);
        if let (Some(doc), Some(tab)) = (self.notes.get(rel), self.tabs.get_mut(self.active)) {
            let start = markdown::line_start_byte(&doc.text, line);
            let line_len = doc.text[start..].find('\n').unwrap_or(doc.text.len() - start);
            let mut byte = start + col.min(line_len);
            while !doc.text.is_char_boundary(byte) {
                byte -= 1;
            }
            tab.editor.pending_cursor = Some(markdown::byte_to_char(&doc.text, byte));
            tab.reading = false;
        }
    }

    /// Toggle a checkbox in any note (open buffer or file on disk).
    pub fn toggle_task(&mut self, rel: &Path, offset: usize) {
        self.wait_for_sync_tree();
        if let Some(doc) = self.notes.get_mut(rel) {
            if doc.unreadable.is_some() {
                self.toast(format!("“{}” could not be read, so it is not changed", rel_to_string(rel)));
            } else if markdown::toggle_task_at(&mut doc.text, offset) {
                doc.dirty = true;
                doc.last_edit = Instant::now();
                let text = doc.text.clone();
                if let Some(v) = self.vault.as_mut() {
                    v.set_note_text(rel, &text);
                }
                self.save_note(rel);
            }
        } else if let Some(v) = self.vault.as_mut() {
            // no buffer: the file as it is on disk now, and only if it is what the index showed
            let seen = v.notes.get(rel).map(|n| n.text.clone());
            let r = v.edit_note(rel, |disk| {
                if Some(disk) != seen.as_deref() {
                    return None;
                }
                let mut t = disk.to_string();
                markdown::toggle_task_at(&mut t, offset).then_some(t)
            });
            if let Err(e) = r {
                self.toast(format!("Could not change “{}”: {e}", rel_to_string(rel)));
            }
        }
    }

    /// Save the buffer of the note `rel` now (DATA-003, DATA-008, DATA-009). A conflict with a
    /// change made by another program is saved as a copy and the buffer takes the other
    /// version; a failure keeps the buffer dirty, marks the tab and is shown once.
    pub fn save_note(&mut self, rel: &Path) {
        let (Some(v), Some(doc)) = (self.vault.as_mut(), self.notes.get_mut(rel)) else { return };
        if !doc.dirty {
            return;
        }
        let name = rel_to_string(rel);
        if doc.unreadable.is_some() {
            // not editable in the UI; if a buffer got edits anyway they are never written over the file
            doc.save_error = Some(format!("“{name}” could not be read, so it is never overwritten"));
            return;
        }
        let msg = match v.save_checked(rel, &doc.text, doc.base) {
            Ok(Saved::Written(stamp)) => {
                doc.base = stamp;
                doc.dirty = false;
                doc.save_error = None;
                None
            }
            Ok(Saved::Conflict { copy, theirs }) => {
                let (text, unreadable) = match theirs {
                    Ok(t) => (t, None),
                    Err(why) => (String::new(), Some(why)),
                };
                doc.reload(text, unreadable);
                Some(conflict_message(&name, &rel_to_string(&copy)))
            }
            Err(e) => {
                let m = format!("Could not save “{name}”: {e}. Your changes are kept in the open note.");
                let first = doc.save_error.as_deref() != Some(m.as_str());
                doc.save_error = Some(m.clone());
                doc.last_edit = Instant::now(); // try again in a moment
                first.then_some(m)
            }
        };
        if let Some(m) = msg {
            self.toast(m);
        }
    }

    /// Like [`ShaleApp::save_note`], for an open drawing.
    pub fn save_drawing(&mut self, rel: &Path) {
        let (Some(v), Some(d)) = (self.vault.as_mut(), self.drawings.get_mut(rel)) else { return };
        if !d.dirty {
            return;
        }
        let name = rel_to_string(rel);
        let msg = match d.file_text().map_err(io::Error::other).and_then(|text| v.save_checked(rel, &text, d.base)) {
            Ok(Saved::Written(stamp)) => {
                d.base = stamp;
                d.dirty = false;
                d.save_error = None;
                self.scenes.put(rel, d.scene.clone());
                None
            }
            Ok(Saved::Conflict { copy, theirs }) => {
                *d = match theirs {
                    Ok(t) => DrawingDoc::new(rel.to_path_buf(), &t),
                    Err(why) => DrawingDoc::unreadable(rel.to_path_buf(), &why),
                };
                self.scenes.put(rel, d.scene.clone());
                Some(conflict_message(&name, &rel_to_string(&copy)))
            }
            Err(e) => {
                let m = format!("Could not save “{name}”: {e}. Your changes are kept in the open drawing.");
                let first = d.save_error.as_deref() != Some(m.as_str());
                d.save_error = Some(m.clone());
                d.last_edit = Instant::now();
                first.then_some(m)
            }
        };
        if let Some(m) = msg {
            self.toast(m);
        }
    }

    pub fn flush_all(&mut self) {
        self.wait_for_sync_tree();
        let dirty_notes: Vec<PathBuf> = self.notes.iter().filter(|(_, d)| d.dirty).map(|(r, _)| r.clone()).collect();
        for rel in dirty_notes {
            self.save_note(&rel);
        }
        let dirty_drawings: Vec<PathBuf> = self.drawings.iter().filter(|(_, d)| d.dirty).map(|(r, _)| r.clone()).collect();
        for rel in dirty_drawings {
            self.save_drawing(&rel);
        }
    }

    /// At exit: what still could not be saved goes to the recovery folder instead of being lost
    /// (DATA-011). Returns where each text went. Failures are the last resort: stderr.
    pub fn rescue_unsaved(&mut self) -> Vec<PathBuf> {
        let date = crate::mail_store::today_string();
        let mut texts: Vec<(PathBuf, Vec<u8>)> = self.notes.iter().filter(|(_, d)| d.dirty && d.unreadable.is_none()).map(|(r, d)| (r.clone(), d.text.clone().into_bytes())).collect();
        for (rel, d) in &self.drawings {
            if d.dirty {
                if let Ok(t) = d.file_text() {
                    texts.push((rel.clone(), t.into_bytes()));
                }
            }
        }
        let mut out = Vec::new();
        for (rel, bytes) in texts {
            match crate::recovery::rescue(&self.recovery_root, &date, &rel, &bytes) {
                Ok(p) => out.push(p),
                Err(e) => eprintln!("could not save “{}” and could not rescue it either: {e}", rel_to_string(&rel)),
            }
        }
        out
    }

    fn autosave(&mut self) {
        // never save into the middle of a git merge (SYNC-013); the buffers stay dirty meanwhile
        if self.sync.tree_busy.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        let wait = Duration::from_millis(800);
        let notes: Vec<PathBuf> = self.notes.iter().filter(|(_, d)| d.dirty && d.last_edit.elapsed() > wait).map(|(r, _)| r.clone()).collect();
        for rel in notes {
            self.save_note(&rel);
        }
        let drawings: Vec<PathBuf> = self.drawings.iter().filter(|(_, d)| d.dirty && d.last_edit.elapsed() > wait).map(|(r, _)| r.clone()).collect();
        for rel in drawings {
            self.save_drawing(&rel);
        }
    }

    /// Pick up changes made by other programs: notes and drawings without unsaved edits are
    /// reloaded (DATA-002); one with unsaved edits keeps them and becomes a conflict copy when
    /// it is saved (DATA-003).
    fn rescan(&mut self) {
        let Some(v) = self.vault.as_mut() else { return };
        let changed = v.rescan();
        for rel in changed {
            if let Some(doc) = self.notes.get_mut(&rel) {
                if !doc.dirty {
                    if let Some(n) = v.notes.get(&rel) {
                        doc.reload(n.text.clone(), n.unreadable.clone());
                    }
                }
            }
        }
        for (rel, d) in self.drawings.iter_mut() {
            if d.dirty {
                continue;
            }
            let meta = std::fs::metadata(v.abs(rel)).ok().map(|m| (m.modified().ok(), m.len()));
            if meta == d.seen && d.error.is_none() {
                continue;
            }
            d.seen = meta;
            match crate::fsutil::read_to_string(v.abs(rel)) {
                Ok(text) if crate::vault::Stamp::of(&text) != d.base || d.error.is_some() => {
                    let fresh = DrawingDoc::new(rel.clone(), &text);
                    self.scenes.put(rel, fresh.scene.clone());
                    *d = fresh;
                }
                _ => {}
            }
        }
        // tabs pointing at files that vanished
        let files: HashSet<PathBuf> = v.files.iter().map(|f| f.rel.clone()).collect();
        for t in &mut self.tabs {
            if let Some(p) = t.view.path() {
                if !files.contains(p) {
                    t.view = View::Empty;
                }
            }
        }
    }

    pub fn rename(&mut self, rel: &Path, new_name: &str) -> Result<(), String> {
        self.flush_all();
        let v = self.vault.as_mut().ok_or("no vault")?;
        // one name, inside the note's folder (VAULT-018)
        let new_name = VaultPath::name(new_name).map_err(|e| e.to_string())?;
        let is_dir = v.abs(rel).is_dir();
        let kind = v.file(rel).map(|f| f.kind);
        let file_name = match kind {
            Some(FileKind::Note) => format!("{}.md", markdown::strip_md_ext(&new_name)),
            Some(FileKind::Drawing) => {
                // keep the format: an Obsidian `.excalidraw.md` drawing stays one
                let stem = crate::excalidraw_md::drawing_stem(&new_name);
                if crate::excalidraw_md::is_plugin_file(rel) {
                    format!("{stem}.excalidraw.md")
                } else {
                    format!("{stem}.excalidraw")
                }
            }
            _ => new_name.clone(),
        };
        let parent = VaultPath::new(rel.parent().unwrap_or(Path::new(""))).map_err(|e| e.to_string())?;
        let to = parent.join_name(&file_name).map_err(|e| e.to_string())?.into_path_buf();
        if to == rel {
            return Ok(());
        }
        let changed = v.rename(rel, &to, self.cfg.update_links_on_rename).map_err(|e| e.to_string())?;
        let failed = v.failed_rewrites.clone();
        self.after_move(rel, &to, is_dir, &changed);
        if let Some(m) = rewrite_problems("Renamed", &changed, &failed) {
            self.toast(m);
        } else if !changed.is_empty() {
            self.toast(format!("Renamed and updated links in {} note(s)", changed.len()));
        } else {
            self.toast(format!("Renamed to “{}”", markdown::strip_md_ext(&file_name)));
        }
        self.remember_open_files();
        Ok(())
    }

    /// Create the folder `name` (may be nested: `a/b`) inside `parent`. The result always stays
    /// inside the vault (VAULT-018).
    pub fn create_folder(&mut self, parent: &Path, name: &str) -> Result<PathBuf, String> {
        let rel = VaultPath::new(parent).and_then(|p| p.join(name)).map_err(|e| e.to_string())?;
        if rel.as_path().as_os_str().is_empty() {
            return Err("The name can't be empty".into());
        }
        let v = self.vault.as_mut().ok_or("no vault")?;
        v.create_folder(&rel).map_err(|e| e.to_string())?;
        Ok(rel.into_path_buf())
    }

    /// Point the red links `target` in `note` at the file `to` ("Fix link", LINK-012).
    pub fn fix_link(&mut self, note: &Path, target: &str, to: &Path) {
        self.flush_all();
        let Some(v) = self.vault.as_mut() else { return };
        match v.fix_link(note, target, to) {
            Ok(n) => {
                if let (Some(doc), Some(idx)) = (self.notes.get_mut(note), v.notes.get(note)) {
                    doc.reload(idx.text.clone(), idx.unreadable.clone());
                }
                let shown = markdown::strip_md_ext(&rel_to_string(to)).to_string();
                self.toast(if n > 0 { format!("The link now points to “{shown}”") } else { "Nothing to fix".to_string() });
            }
            Err(e) => self.toast(format!("Could not fix the link: {e}")),
        }
    }

    /// After a file or folder moved from `rel` to `to`: open notes, drawings and tabs follow it,
    /// and open notes whose text changed on disk (updated links) are reloaded.
    fn after_move(&mut self, rel: &Path, to: &Path, is_dir: bool, changed: &[PathBuf]) {
        let to = to.to_path_buf();
        // move open buffers and tabs
        let remap = |p: &Path| -> Option<PathBuf> {
            if p == rel {
                Some(to.clone())
            } else if is_dir {
                p.strip_prefix(rel).ok().map(|rest| to.join(rest))
            } else {
                None
            }
        };
        let keys: Vec<PathBuf> = self.notes.keys().cloned().collect();
        for k in keys {
            if let Some(nk) = remap(&k) {
                if let Some(d) = self.notes.remove(&k) {
                    self.notes.insert(nk, d);
                }
            }
        }
        let dkeys: Vec<PathBuf> = self.drawings.keys().cloned().collect();
        for k in dkeys {
            if let Some(nk) = remap(&k) {
                if let Some(mut d) = self.drawings.remove(&k) {
                    d.rel = nk.clone();
                    self.drawings.insert(nk, d);
                }
            }
        }
        for t in &mut self.tabs {
            for view in std::iter::once(&mut t.view).chain(t.back.iter_mut()).chain(t.fwd.iter_mut()) {
                match view {
                    View::Note(p) | View::Drawing(p) => {
                        if let Some(np) = remap(p) {
                            *p = np;
                        }
                    }
                    _ => {}
                }
            }
            t.title_for = None;
        }
        let v = self.vault.as_ref().unwrap();
        for c in changed {
            if let (Some(doc), Some(n)) = (self.notes.get_mut(c), v.notes.get(c)) {
                doc.reload(n.text.clone(), n.unreadable.clone());
            }
        }
        // open folders in the file list move along
        let exp: Vec<PathBuf> = self.expanded.iter().cloned().collect();
        for e in exp {
            if let Ok(rest) = e.strip_prefix(rel) {
                self.expanded.remove(&e);
                self.expanded.insert(to.join(rest));
            }
        }
    }

    /// Move a note, drawing, file or folder into another folder (drag and drop in the file list).
    pub fn move_into(&mut self, rel: &Path, into: &Path) -> Result<PathBuf, String> {
        if rel.parent().unwrap_or(Path::new("")) == into {
            return Ok(rel.to_path_buf());
        }
        self.flush_all();
        let v = self.vault.as_mut().ok_or("no vault")?;
        let is_dir = v.abs(rel).is_dir();
        let (to, changed) = v.move_into(rel, into).map_err(|e| e.to_string())?;
        let failed = v.failed_rewrites.clone();
        self.after_move(rel, &to, is_dir, &changed);
        if let Some(m) = rewrite_problems("Moved", &changed, &failed) {
            self.toast(m);
        }
        if !into.as_os_str().is_empty() {
            self.expanded.insert(into.to_path_buf());
        }
        let name = rel.file_name().map(|n| markdown::strip_md_ext(&n.to_string_lossy()).to_string()).unwrap_or_default();
        let place = if into.as_os_str().is_empty() { "the vault".to_string() } else { format!("“{}”", rel_to_string(into)) };
        self.toast(if changed.is_empty() { format!("Moved “{name}” to {place}") } else { format!("Moved “{name}” to {place} and updated links in {} note(s)", changed.len()) });
        self.remember_open_files();
        Ok(to)
    }

    pub fn trash(&mut self, rel: &Path) {
        self.flush_all();
        // unsaved edits that could not be saved must not go away with the buffer (DATA-009)
        if let Some(bad) = self.notes.iter().find(|(k, d)| k.starts_with(rel) && d.dirty).map(|(k, _)| rel_to_string(k)).or_else(|| self.drawings.iter().find(|(k, d)| k.starts_with(rel) && d.dirty).map(|(k, _)| rel_to_string(k))) {
            self.toast(format!("“{bad}” has changes that could not be saved, so “{}” is not moved to the trash", rel_to_string(rel)));
            return;
        }
        let Some(v) = self.vault.as_mut() else { return };
        match v.trash(rel) {
            Ok(()) => {
                self.notes.retain(|k, _| !k.starts_with(rel));
                self.drawings.retain(|k, _| !k.starts_with(rel));
                for t in &mut self.tabs {
                    if t.view.path().map(|p| p.starts_with(rel)).unwrap_or(false) {
                        t.view = t.back.pop().unwrap_or(View::Empty);
                    }
                    t.back.retain(|b| !b.path().map(|p| p.starts_with(rel)).unwrap_or(false));
                    t.fwd.retain(|b| !b.path().map(|p| p.starts_with(rel)).unwrap_or(false));
                }
                self.toast(format!("Moved “{}” to the vault's .trash folder", rel_to_string(rel)));
                self.remember_open_files();
            }
            Err(e) => self.toast(format!("Could not move to trash: {e}")),
        }
    }

    /// Open the note before (-1) or after (+1) the current one in the file list (same folder,
    /// the list's sort order), in the same tab.
    /// Put the keyboard into the left sidebar (Ctrl+H, Ctrl+Shift+B): browse the file list with
    /// the open note selected, or type in the search box.
    pub fn focus_sidebar(&mut self, ctx: &egui::Context) {
        self.cfg.show_left = true;
        if let Some(f) = ctx.memory(|m| m.focused()) {
            ctx.memory_mut(|m| m.surrender_focus(f));
        }
        match self.left {
            LeftPanel::Files => {
                let cur = self.current_path();
                // the open note's folders open, so it can be selected
                let mut p = cur.as_deref().and_then(|c| c.parent());
                while let Some(d) = p.filter(|d| !d.as_os_str().is_empty()) {
                    self.expanded.insert(d.to_path_buf());
                    p = d.parent();
                }
                let rows = self.visible_tree_rows();
                self.tree_sel = cur.filter(|c| rows.iter().any(|(r, _)| r == c)).or_else(|| rows.first().map(|(r, _)| r.clone()));
                self.tree_scroll = true;
            }
            LeftPanel::Search => self.focus_search = true,
            LeftPanel::Tags => {}
        }
    }

    /// Put the keyboard back into the editor of the open note (Ctrl+L).
    pub fn focus_editor(&mut self) {
        self.tree_sel = None;
        self.search_sel = None;
        if let Some(t) = self.tabs.get_mut(self.active) {
            t.editor.request_focus = true;
        }
    }

    /// Do what the calendar panel asked for.
    pub fn calendar_action(&mut self, a: crate::cal_ui::CalAction) {
        use crate::cal_ui::CalAction;
        match a {
            CalAction::Settings => self.open_calendar_settings(),
            CalAction::OpenUrl(url) => {
                if let Err(e) = crate::opener::open_url(&url) {
                    self.toast(e);
                }
            }
            CalAction::OpenLarge => self.show_view(View::Calendar, false),
            CalAction::NewEventAt(start) => {
                let srcs = &self.calendar.sources;
                let target = self.last_event_calendar.as_ref().and_then(|id| srcs.iter().find(|s| s.id == *id)).or_else(|| srcs.iter().find(|s| s.enabled)).or_else(|| srcs.first()).map(|s| s.id.clone());
                match target {
                    Some(id) => self.modal = Some(Modal::Event(Box::new(crate::cal_dialog::EventDialog::new_at(id, start)))),
                    None => self.open_calendar_settings(),
                }
            }
            CalAction::Edit(occ) => {
                let found = self.calendar.sources.get(occ.source).map(|s| s.id.clone()).zip(self.calendar.event(&occ).cloned());
                if let Some((sid, ev)) = found {
                    self.modal = Some(Modal::Event(Box::new(crate::cal_dialog::EventDialog::edit(sid, ev))));
                }
            }
            CalAction::Delete(occ) => {
                let found = self.calendar.sources.get(occ.source).map(|s| s.id.clone()).zip(self.calendar.event(&occ).cloned());
                if let Some((sid, ev)) = found {
                    match self.calendar.delete_event(&sid, ev, self.ctx.clone()) {
                        Ok(job) => self.cal_jobs.push((job, sid)),
                        Err(e) => self.toast(format!("Could not delete the event: {e}")),
                    }
                }
            }
        }
    }

    pub fn open_calendar_settings(&mut self) {
        self.modal = Some(Modal::Calendars(Box::new(crate::cal_dialog::CalendarDialog::new(self.cfg.calendars.clone(), self.cfg.week_starts_monday))));
    }

    /// Apply a calendar settings dialog action. Returns true when the dialog should close.
    pub fn calendar_dialog_action(&mut self, d: &mut crate::cal_dialog::CalendarDialog, a: crate::cal_dialog::CalDialogAction) -> bool {
        use crate::cal_dialog::CalDialogAction;
        match a {
            CalDialogAction::Done => {
                self.cfg.calendars = d.sources.clone();
                self.cfg.week_starts_monday = d.week_monday;
                self.calendar.set_sources(self.cfg.calendars.clone());
                self.save_config();
                true
            }
            CalDialogAction::Cancel => true,
            CalDialogAction::Refresh => {
                self.calendar.set_sources(d.sources.clone());
                self.calendar.reload_local();
                self.calendar.refresh_remote();
                false
            }
            CalDialogAction::SyncNow(id) => {
                self.calendar.set_sources(d.sources.clone());
                self.calendar.sync_now(&id);
                false
            }
            CalDialogAction::FillSample(id) => {
                // the calendar may be new in the dialog: save the list first
                self.cfg.calendars = d.sources.clone();
                self.calendar.set_sources(self.cfg.calendars.clone());
                self.save_config();
                match self.calendar.fill_sample(&id) {
                    Ok(n) => d.message = Some(format!("Added {n} sample events around today.")),
                    Err(e) => d.message = Some(format!("Could not add sample events: {e}")),
                }
                false
            }
        }
    }

    /// Show the export dialog with sensible defaults (saved to `<vault>/exports/<name>.pdf`).
    pub fn open_export_dialog(&mut self, rel: &Path) {
        let Some(v) = &self.vault else { return };
        let Some(f) = v.file(rel) else { return };
        let drawing = f.kind == FileKind::Drawing;
        let name = f.display_name();
        let opts = if drawing {
            let scene = self.drawings.get(rel).map(|d| d.scene.clone()).or_else(|| crate::drawing::Scene::read_file(&v.abs(rel)).ok()).unwrap_or_default();
            crate::pdfexport::ExportOptions::for_drawing(&scene)
        } else {
            crate::pdfexport::ExportOptions::for_note()
        };
        let path = v.root.join("exports").join(format!("{name}.pdf"));
        self.modal = Some(Modal::Export(Box::new(crate::export_dialog::ExportDialog::new(rel.to_path_buf(), drawing, name, opts, path))));
    }

    /// Write the PDF for the dialog's file and options. Returns where it was saved.
    pub fn run_export(&mut self, d: &crate::export_dialog::ExportDialog) -> Result<PathBuf, String> {
        self.flush_all();
        let v = self.vault.as_ref().ok_or("No vault is open")?;
        let bytes = if d.drawing {
            let scene = match self.drawings.get(&d.rel) {
                Some(doc) => doc.scene.clone(),
                None => crate::drawing::Scene::read_file(&v.abs(&d.rel))?,
            };
            crate::pdfexport::export_drawing(&scene, &d.name, &d.opts)?
        } else {
            let text = self.notes.get(&d.rel).map(|n| n.text.clone()).or_else(|| v.notes.get(&d.rel).map(|n| n.text.clone())).unwrap_or_default();
            crate::pdfexport::export_note(v, &d.rel, &text, &d.opts)?
        };
        let out = PathBuf::from(crate::layout::shellexpand(&d.path));
        if out.as_os_str().is_empty() {
            return Err("Choose where to save the PDF".into());
        }
        if let Some(dir) = out.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("Could not create the folder: {e}"))?;
        }
        crate::fsutil::write_atomic(&out, &bytes).map_err(|e| format!("Could not save the PDF: {e}"))?;
        if let Some(v) = self.vault.as_mut() {
            v.rescan();
        }
        Ok(out)
    }

    pub fn close_tab(&mut self, i: usize) {
        if i < self.tabs.len() {
            self.flush_all();
            self.tabs.remove(i);
            if self.active >= self.tabs.len() && self.active > 0 {
                self.active = self.tabs.len() - 1;
            } else if i < self.active {
                self.active -= 1;
            }
            self.remember_open_files();
        }
    }

    pub fn go_back(&mut self) {
        if let Some(t) = self.tabs.get_mut(self.active) {
            if let Some(prev) = t.back.pop() {
                let cur = std::mem::replace(&mut t.view, prev);
                t.fwd.push(cur);
            }
        }
    }

    pub fn go_forward(&mut self) {
        if let Some(t) = self.tabs.get_mut(self.active) {
            if let Some(next) = t.fwd.pop() {
                let cur = std::mem::replace(&mut t.view, next);
                t.back.push(cur);
            }
        }
    }

    fn process_actions(&mut self) {
        let actions = std::mem::take(&mut self.actions);
        for a in actions {
            match a {
                Action::Open { rel, new_tab } => self.open(&rel, new_tab),
                Action::OpenLink { target, heading, from, new_tab } => self.open_link(&target, heading, from, new_tab),
                Action::FixLink { note, target, to } => self.fix_link(&note, &target, &to),
                Action::NewNote { folder } => self.new_untitled(&folder),
                Action::NewDrawing { folder, embed_into } => self.new_drawing(&folder, embed_into),
                Action::ToggleTask { rel, offset } => self.toggle_task(&rel, offset),
                Action::GotoLine { rel, line } => self.goto_line(&rel, line),
                Action::GotoMatch { rel, line, col } => self.goto_match(&rel, line, col),
                Action::ShowView(v) => self.show_view(v, false),
                Action::CloseTab(i) => self.close_tab(i),
                Action::Rename(rel) => {
                    let name = self.vault.as_ref().and_then(|v| v.file(&rel).map(|f| f.display_name())).unwrap_or_else(|| rel.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                    self.modal = Some(Modal::Rename { rel, name, error: None });
                }
                Action::Trash(rel) => self.modal = Some(Modal::ConfirmTrash { rel }),
                Action::NewFolder(parent) => self.modal = Some(Modal::NewFolder { parent, name: String::new() }),
                Action::OpenVault(p) => self.open_vault(&p),
                Action::Export(rel) => self.open_export_dialog(&rel),
                Action::Rescan => self.rescan(),
                Action::OpenUrl(url) => {
                    if let Err(e) = crate::opener::open_url(&url) {
                        self.toast(e);
                    }
                }
                Action::CreatePerson(name) => self.create_person(&name),
                Action::Move { from, into } => {
                    if let Err(e) = self.move_into(&from, &into) {
                        self.toast(format!("Could not move: {e}"));
                    }
                }
            }
        }
    }

    fn shortcuts(&mut self, ui: &Ui) {
        // mouse side buttons: back / forward through the notes opened in this tab
        let (mouse_back, mouse_fwd) = ui.input(|i| (i.pointer.button_pressed(egui::PointerButton::Extra1), i.pointer.button_pressed(egui::PointerButton::Extra2)));
        // Ctrl+H / Ctrl+L (like vim's h / l): keyboard into the sidebar / back into the editor;
        // Ctrl+Shift+B: the file list
        let (to_sidebar, to_editor, files) = ui.input_mut(|i| (i.consume_key(Modifiers::COMMAND, Key::H), i.consume_key(Modifiers::COMMAND, Key::L), i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::B)));
        let mail_view = ui.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::M));
        let (quick, new_note, toggle, back, fwd, close, search, save, settings) = ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::COMMAND, Key::O),
                i.consume_key(Modifiers::COMMAND, Key::N),
                i.consume_key(Modifiers::COMMAND, Key::E),
                i.consume_key(Modifiers::ALT, Key::ArrowLeft),
                i.consume_key(Modifiers::ALT, Key::ArrowRight),
                i.consume_key(Modifiers::COMMAND, Key::W),
                i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::F),
                i.consume_key(Modifiers::COMMAND, Key::S),
                i.consume_key(Modifiers::COMMAND, Key::Comma),
            )
        });
        if self.vault.is_none() {
            return;
        }
        if mail_view {
            self.show_view(View::Mail, false);
        }
        if quick {
            self.modal = Some(Modal::QuickSwitch { query: String::new(), sel: 0 });
        }
        if new_note {
            let f = self.current_folder();
            self.new_untitled(&f);
        }
        if toggle {
            if let Some(t) = self.tabs.get_mut(self.active) {
                t.reading = !t.reading;
            }
        }
        if files {
            self.left = LeftPanel::Files;
            self.focus_sidebar(ui.ctx());
        } else if to_sidebar {
            self.focus_sidebar(ui.ctx());
        }
        if to_editor {
            self.focus_editor();
        }
        if back || mouse_back {
            self.go_back();
        }
        if fwd || mouse_fwd {
            self.go_forward();
        }
        if close {
            let a = self.active;
            self.close_tab(a);
        }
        if search {
            self.left = LeftPanel::Search;
            self.cfg.show_left = true;
            self.focus_search = true;
        }
        if save {
            self.flush_all();
            self.toast("Saved");
        }
        if settings {
            let vp = self.cfg.vault.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
            self.modal = Some(Modal::Settings { vault_path: vp });
        }
    }

    fn screenshot_tick(&mut self, ui: &Ui) {
        self.frames += 1;
        let Some(path) = self.screenshot.clone() else { return };
        ui.ctx().request_repaint();
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::RequestPaintWhileHidden);
        if self.frames >= 120 && self.script.is_empty() && !self.shot_requested {
            self.shot_requested = true;
            let done = self.shot_done.clone();
            ui.ctx().request_screenshot(move |img| {
                let [w, h] = img.size;
                let mut bytes = Vec::with_capacity(w * h * 4);
                for p in &img.pixels {
                    bytes.extend_from_slice(&p.to_srgba_unmultiplied());
                }
                match image::RgbaImage::from_raw(w as u32, h as u32, bytes) {
                    Some(buf) => {
                        if let Some(dir) = path.parent() {
                            let _ = std::fs::create_dir_all(dir);
                        }
                        let mut png = std::io::Cursor::new(Vec::new());
                        let saved = buf.write_to(&mut png, image::ImageFormat::Png).map_err(|e| e.to_string()).and_then(|_| crate::fsutil::write_atomic(&path, png.get_ref()).map_err(|e| e.to_string()));
                        if let Err(e) = saved {
                            eprintln!("could not save screenshot: {e}");
                        }
                    }
                    None => eprintln!("screenshot had an unexpected size"),
                }
                done.store(true, Ordering::SeqCst);
            });
        }
        if self.shot_done.load(Ordering::SeqCst) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.frames > 900 {
            eprintln!("screenshot timed out");
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl ShaleApp {
    /// Testing aid (`--keys`): add the next scripted step to this frame's input.
    fn feed_script(&mut self, raw: &mut egui::RawInput) {
        if self.script.is_empty() || self.frames < 30 || !self.frames.is_multiple_of(6) {
            return;
        }
        let Some(step) = self.script.pop_front() else { return };
        let key_event = |name: &str| {
            // "ctrl+shift+F": any number of modifiers before the key
            let mut mods = Modifiers::NONE;
            let mut k = name;
            while let Some((m, rest)) = k.split_once('+') {
                match m {
                    "shift" => mods.shift = true,
                    "alt" => mods.alt = true,
                    "ctrl" => mods |= Modifiers::COMMAND,
                    _ => break,
                }
                k = rest;
            }
            let key = Key::from_name(k).unwrap_or(Key::Escape);
            vec![egui::Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers: mods }, egui::Event::Key { key, physical_key: Some(key), pressed: false, repeat: false, modifiers: mods }]
        };
        if let Some(t) = step.strip_prefix("text:") {
            raw.events.push(egui::Event::Text(t.to_string()));
        } else if let Some(k) = step.strip_prefix("key:") {
            raw.events.extend(key_event(k));
        } else if let Some(xy) = step.strip_prefix("move:").or_else(|| step.strip_prefix("down:")).or_else(|| step.strip_prefix("up:")).or_else(|| step.strip_prefix("click:")).or_else(|| step.strip_prefix("dblclick:")) {
            // pointer steps in window points: move:x,y  down:x,y  up:x,y  click:x,y  dblclick:x,y
            let v: Vec<f32> = xy.split(',').filter_map(|n| n.trim().parse().ok()).collect();
            if v.len() == 2 {
                let pos = egui::pos2(v[0], v[1]);
                let btn = |pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
                raw.events.push(egui::Event::PointerMoved(pos));
                if step.starts_with("down:") {
                    raw.events.push(btn(true));
                } else if step.starts_with("up:") {
                    raw.events.push(btn(false));
                } else if step.starts_with("click:") {
                    raw.events.extend([btn(true), btn(false)]);
                } else if step.starts_with("dblclick:") {
                    raw.events.extend([btn(true), btn(false), btn(true), btn(false)]);
                }
            }
        } else if let Some(p) = step.strip_prefix("drop:") {
            // a file dropped through the window system: drop:/path/to/file
            raw.dropped_files.push(std::sync::Arc::new(ScriptDrop(PathBuf::from(p))));
        } else if let Some(rest) = step.strip_prefix("dropdata:") {
            // data dropped by another program at the pointer: dropdata:text/plain:hello
            // (several kinds: dropdata:text/uri-list:|text/x-moz-url:link — an empty part is allowed)
            #[cfg(target_os = "linux")]
            if let Some((mime, data)) = rest.split_once(':') {
                let at = self.ctx.as_ref().and_then(|c| c.input(|i| i.pointer.latest_pos())).unwrap_or_default();
                let data = if let Some(f) = data.strip_prefix('@') { crate::fsutil::read(f).unwrap_or_default() } else { data.replace("\\n", "\n").into_bytes() };
                let mut parts = vec![(mime.to_string(), data)];
                // more kinds follow as ";;kind:data"
                if let Some(pos) = parts[0].1.windows(2).position(|w| w == b";;") {
                    let rest = String::from_utf8_lossy(&parts[0].1[pos + 2..]).into_owned();
                    parts[0].1.truncate(pos);
                    for p in rest.split(";;") {
                        if let Some((m, d)) = p.split_once(':') {
                            parts.push((m.to_string(), d.as_bytes().to_vec()));
                        }
                    }
                }
                let offered = parts.iter().map(|p| p.0.clone()).collect();
                self.fake_drops.push(crate::wl_drop::DropEvent::Drop { x: at.x, y: at.y, parts, offered, report: vec![] });
            }
            #[cfg(not(target_os = "linux"))]
            let _ = rest;
        } else if step == "focus-end" {
            if let (Some(t), Some(p)) = (self.tabs.get(self.active), self.current_path()) {
                let n = self.notes.get(&p).map(|d| d.text.chars().count()).unwrap_or(0);
                let id = t.id;
                if let Some(t) = self.tabs.iter_mut().find(|t| t.id == id) {
                    t.editor.pending_cursor = Some(n);
                }
            }
        }
        if std::env::var_os("SHALE_DEBUG").is_some() {
            if let Some(d) = self.current_path().and_then(|p| self.drawings.get(&p)) {
                eprintln!("step {step:?}: drawing {}", d.debug_state());
            }
            if let (Some(t), Some(p)) = (self.tabs.get(self.active), self.current_path()) {
                let tail: String = self.notes.get(&p).map(|d| d.text.chars().rev().take(40).collect::<Vec<_>>().into_iter().rev().collect()).unwrap_or_default();
                eprintln!("step {step:?}: tabs={} active={} tab_id={} editor={:?} picker_visible={} {} tail={tail:?}", self.tabs.len(), self.active, t.id, t.editor.id, t.editor.picker_visible(), t.editor.debug_state());
            }
        }
    }

    /// Start a fresh troubleshooting recording (with a header describing the setup).
    pub fn start_input_log(&self) {
        let path = crate::config::input_log_path();
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let env = |k: &str| std::env::var(k).unwrap_or_else(|_| "-".into());
        let header = format!(
            "Shale key recording\nbuild: {}\nsession: {}  desktop: {}\ninput method: GTK_IM_MODULE={} QT_IM_MODULE={} XMODIFIERS={}\nkeyboard: XKB_DEFAULT_LAYOUT={} LANG={}\n---\n",
            crate::config::build_stamp(),
            env("XDG_SESSION_TYPE"),
            env("XDG_CURRENT_DESKTOP"),
            env("GTK_IM_MODULE"),
            env("QT_IM_MODULE"),
            env("XMODIFIERS"),
            env("XKB_DEFAULT_LAYOUT"),
            env("LANG"),
        );
        if let Ok(mut f) = crate::fsutil::open_private_log(&path, false) {
            use std::io::Write;
            let _ = f.write_all(header.as_bytes());
        }
    }

    /// Passwords are typed in the calendar and mail account settings: no key of theirs is recorded
    /// (MAIL-041).
    pub(crate) fn input_is_secret(&self) -> bool {
        matches!(self.modal, Some(Modal::Calendars(_)) | Some(Modal::MailAccounts(_)))
    }

    /// Append the keyboard-related events of this frame, plus the editor's picker state.
    fn log_input(&mut self, raw: &egui::RawInput) {
        use std::io::Write;
        // the calendar settings hold the CalDAV password field: record nothing typed there
        // (not even key names, they would spell the password)
        if self.input_is_secret() {
            if raw.events.iter().any(|e| matches!(e, egui::Event::Key { .. } | egui::Event::Text(_) | egui::Event::Paste(_) | egui::Event::Ime(_))) {
                if let Ok(mut f) = crate::fsutil::open_private_log(&crate::config::input_log_path(), true) {
                    let _ = writeln!(f, "frame {} | (input in the account settings is not recorded)", self.frames);
                }
            }
            return;
        }
        let lines: Vec<String> = raw
            .events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Key { key, physical_key, pressed, repeat, modifiers } => Some(format!("key {key:?} physical={physical_key:?} pressed={pressed} repeat={repeat} mods={modifiers:?}")),
                egui::Event::Text(t) => Some(format!("text {t:?}")),
                egui::Event::Ime(i) => Some(format!("ime {i:?}")),
                egui::Event::Paste(t) => Some(format!("paste {t:?}")),
                egui::Event::WindowFocused(f) => Some(format!("window focused={f}")),
                _ => None,
            })
            .collect();
        if lines.is_empty() {
            return;
        }
        let state = match (self.tabs.get(self.active), self.current_path()) {
            (Some(t), Some(p)) => {
                let line = self.notes.get(&p).map(|d| d.text.lines().last().unwrap_or("").chars().rev().take(50).collect::<Vec<_>>().into_iter().rev().collect::<String>()).unwrap_or_default();
                format!("{} | last line: {line:?}", t.editor.debug_state())
            }
            _ => "no note open".into(),
        };
        if let Ok(mut f) = crate::fsutil::open_private_log(&crate::config::input_log_path(), true) {
            let _ = writeln!(f, "frame {} | before: {state}", self.frames);
            for l in lines {
                let _ = writeln!(f, "    {l}");
            }
        }
    }
}

impl eframe::App for ShaleApp {
    /// Testing aid: feed scripted keys into the real window, one step every few frames.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if std::env::var_os("SHALE_DEBUG").is_some() && !self.script.is_empty() {
            eprintln!("focus: {:?} wants_keys={}", ctx.memory(|m| m.focused()), ctx.egui_wants_keyboard_input());
        }
        self.feed_script(raw);
        if self.cfg.input_log {
            self.log_input(raw);
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.run_frame(ui);
    }

    fn on_exit(&mut self) {
        // flush_all waits for a running sync's merge step first
        self.flush_all();
        self.rescue_unsaved();
        self.remember_open_files();
        self.save_config();
    }
}

impl ShaleApp {
    /// One frame of the window (the body of `eframe::App::ui`; tests call it directly).
    pub(crate) fn run_frame(&mut self, ui: &mut Ui) {
        self.drop_zones.clear();
        self.calendar_areas.clear();
        self.tree_targets.clear();
        self.screenshot_tick(ui);
        self.shortcuts(ui);
        self.autosave();
        let ctx = ui.ctx().clone();
        self.sync_tick(&ctx);
        self.mail_tick(&ctx);
        if self.calendar.poll() {
            ui.ctx().request_repaint();
        }
        if self.ctx.is_none() {
            self.ctx = Some(ui.ctx().clone());
        }
        let mut finished = Vec::new();
        self.cal_jobs.retain(|(job, sid)| match crate::cal_store::job_result(job) {
            Some(r) => {
                finished.push((r, sid.clone()));
                false
            }
            None => true,
        });
        for (r, sid) in finished {
            self.calendar.after_write(&[sid]);
            match r {
                Ok(m) | Err(m) => self.toast(m),
            }
        }
        if !self.cal_jobs.is_empty() {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
        if self.calendar.is_loading() {
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        }
        if self.last_scan.elapsed() > Duration::from_secs(3) {
            self.last_scan = Instant::now();
            self.rescan();
        }
        if self.notes.values().any(|d| d.dirty) || self.drawings.values().any(|d| d.dirty) {
            ui.ctx().request_repaint_after(Duration::from_millis(300));
        }
        ui.ctx().request_repaint_after(Duration::from_secs(3));

        if self.vault.is_none() {
            self.welcome(ui);
        } else {
            self.layout(ui);
        }
        self.modals(ui);
        self.handle_drops(ui);
        self.learn_tags_tick();
        self.paint_toasts(ui);
        self.paint_recovery_notice(ui);
        self.process_actions();
    }
}

/// After a rename or move that could not update the links in every note (VAULT-016): exactly
/// which notes were updated and which were not. `None` when all went well.
fn rewrite_problems(what: &str, changed: &[PathBuf], failed: &[(PathBuf, String)]) -> Option<String> {
    if failed.is_empty() {
        return None;
    }
    let not: Vec<String> = failed.iter().map(|(p, why)| format!("“{}” ({why})", rel_to_string(p))).collect();
    let done = if changed.is_empty() { "no note was updated".to_string() } else { format!("updated: {}", changed.iter().map(|p| format!("“{}”", rel_to_string(p))).collect::<Vec<_>>().join(", ")) };
    Some(format!("{what}, but the links in {} note(s) could not be updated: {}. {done}. Links in those notes may point to the old name.", failed.len(), not.join(", ")))
}

/// The message after a save found the file changed by another program (DATA-003).
fn conflict_message(name: &str, copy: &str) -> String {
    format!("“{name}” was changed by another program while you were editing it. Your version is saved as “{copy}”; the other version stays in “{name}”.")
}

pub fn handle_editor_action(app_actions: &mut Vec<Action>, a: EditorAction, from: &Path) {
    match a {
        EditorAction::OpenLink { target, heading, new_tab } => app_actions.push(Action::OpenLink { target, heading, from: Some(from.to_path_buf()), new_tab }),
        EditorAction::OpenUrl(url) => app_actions.push(Action::OpenUrl(url)),
        EditorAction::FilesChanged => app_actions.push(Action::Rescan),
        EditorAction::CreatePerson(name) => app_actions.push(Action::CreatePerson(name)),
    }
}

pub fn handle_reader_action(app_actions: &mut Vec<Action>, a: ReaderAction, from: &Path) {
    match a {
        ReaderAction::OpenLink { target, new_tab } => app_actions.push(Action::OpenLink { target, heading: None, from: Some(from.to_path_buf()), new_tab }),
        ReaderAction::ToggleTask(offset) => app_actions.push(Action::ToggleTask { rel: from.to_path_buf(), offset }),
        ReaderAction::OpenFile(rel) => app_actions.push(Action::Open { rel, new_tab: false }),
        ReaderAction::OpenUrl(url) => app_actions.push(Action::OpenUrl(url)),
    }
}

pub fn graph_click(app_actions: &mut Vec<Action>, c: GraphClick) {
    match c {
        GraphClick::Open(rel) => app_actions.push(Action::Open { rel, new_tab: false }),
        GraphClick::Create(name) => app_actions.push(Action::OpenLink { target: name, heading: None, from: None, new_tab: false }),
    }
}

pub fn muted(t: impl Into<String>) -> egui::RichText {
    egui::RichText::new(t).color(theme::MUTED)
}

#[cfg(test)]
mod data_fuzz_tests;
#[cfg(test)]
mod data_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn test_app(tag: &str) -> (ShaleApp, PathBuf) {
        let d = std::env::temp_dir().join(format!("shale-app-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        vault::write_sample_vault(&d).unwrap();
        let app = ShaleApp::build(Options { screenshot: None, vault: Some(d.clone()), view: None, keys: None }, false);
        (app, d)
    }

    // spec: LINK-005, NAV-001
    #[test]
    fn blue_link_opens_existing_note() {
        let (mut app, d) = test_app("open");
        app.open_link("Ideas", None, Some(PathBuf::from("Welcome.md")), false);
        assert_eq!(app.current_path(), Some(PathBuf::from("Ideas.md")));
        // back goes to the previous note
        app.go_back();
        assert_eq!(app.current_path(), Some(PathBuf::from("Welcome.md")));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: VAULT-018
    #[test]
    fn rename_refuses_names_that_leave_the_folder() {
        let (mut app, d) = test_app("escape");
        for bad in ["../../x", "/tmp/x", "..", "a/b", "   "] {
            assert!(app.rename(Path::new("Ideas.md"), bad).is_err(), "{bad} must be refused");
        }
        assert!(d.join("Ideas.md").exists());
        assert!(!d.parent().unwrap().join("x.md").exists());
        app.rename(Path::new("Projects/Garden plan.md"), " Garden: v2 ").unwrap();
        assert!(d.join("Projects/Garden- v2.md").exists(), "renamed inside its folder");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: VAULT-018
    #[test]
    fn new_folder_stays_inside_the_vault() {
        let (mut app, d) = test_app("newfolder");
        for bad in ["../x", "/../../x", "a/../../x", "~/x"] {
            assert!(app.create_folder(Path::new("Projects"), bad).is_err(), "{bad} must be refused");
        }
        assert!(!d.join("x").exists() && !d.parent().unwrap().join("x").exists());
        assert_eq!(app.create_folder(Path::new("Projects"), "/a/ b ").unwrap(), PathBuf::from("Projects/a/b"));
        assert!(d.join("Projects/a/b").is_dir());
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: VAULT-013, VAULT-016
    #[test]
    fn folder_rename_updates_links() {
        let (mut app, d) = test_app("folderrename");
        app.open(Path::new("Ideas.md"), false);
        app.rename(Path::new("Projects"), "Work").unwrap();
        // Ideas links `[[Projects/Garden plan|garden]]`; the open buffer follows
        assert!(app.notes[Path::new("Ideas.md")].text.contains("[[Work/Garden plan|garden]]"));
        assert!(std::fs::read_to_string(d.join("Ideas.md")).unwrap().contains("[[Work/Garden plan|garden]]"));
        // `[[Garden plan]]` by name keeps working as it is
        assert!(std::fs::read_to_string(d.join("Daily log.md")).unwrap().contains("[[Garden plan]]"));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: LINK-012
    #[test]
    fn opening_a_note_fixes_its_broken_path_links() {
        let (mut app, d) = test_app("autofix");
        std::fs::write(d.join("Moved.md"), "See [[Archive/Garden plan]] and [[Archive/Nothing]]").unwrap();
        app.rescan();
        app.open(Path::new("Moved.md"), false);
        assert_eq!(app.notes[Path::new("Moved.md")].text, "See [[Projects/Garden plan]] and [[Archive/Nothing]]");
        assert_eq!(std::fs::read_to_string(d.join("Moved.md")).unwrap(), "See [[Projects/Garden plan]] and [[Archive/Nothing]]");
        assert!(app.toasts.iter().any(|(t, _)| t.contains("Fixed 1 link")));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: LINK-012
    #[test]
    fn fix_link_action_points_the_link_at_the_chosen_file() {
        let (mut app, d) = test_app("fixaction");
        std::fs::create_dir_all(d.join("Archive")).unwrap();
        std::fs::write(d.join("Archive/Ideas.md"), "").unwrap();
        std::fs::write(d.join("Moved.md"), "[[Old/Ideas]]").unwrap();
        app.rescan();
        app.open(Path::new("Moved.md"), false);
        assert_eq!(app.notes[Path::new("Moved.md")].text, "[[Old/Ideas]]", "two candidates: not fixed by itself");
        app.actions.push(Action::FixLink { note: PathBuf::from("Moved.md"), target: "Old/Ideas".into(), to: PathBuf::from("Archive/Ideas.md") });
        app.process_actions();
        assert_eq!(app.notes[Path::new("Moved.md")].text, "[[Archive/Ideas]]");
        assert!(app.vault.as_ref().unwrap().exists("Archive/Ideas", None));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: LINK-010
    #[test]
    fn red_relative_link_creates_the_note_where_it_points() {
        let (mut app, d) = test_app("relcreate");
        app.open_link("../Notes/Fresh", None, Some(PathBuf::from("Projects/Garden plan.md")), false);
        assert!(d.join("Notes/Fresh.md").exists());
        assert!(app.vault.as_ref().unwrap().exists("../Notes/Fresh", Some(Path::new("Projects/Garden plan.md"))));
        app.open_link("./Sub", None, Some(PathBuf::from("Projects/Garden plan.md")), false);
        assert!(d.join("Projects/Sub.md").exists());
        let files = app.vault.as_ref().unwrap().files.len();
        app.open_link("../../Outside", None, Some(PathBuf::from("Projects/Garden plan.md")), false);
        assert_eq!(app.vault.as_ref().unwrap().files.len(), files, "nothing is created outside");
        assert!(!d.parent().unwrap().join("Outside.md").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: LINK-005
    #[test]
    fn red_link_creates_note_and_turns_blue() {
        let (mut app, d) = test_app("create");
        assert!(!app.vault.as_ref().unwrap().exists("Reading list", None));
        app.open_link("Reading list", None, Some(PathBuf::from("Welcome.md")), false);
        assert!(d.join("Reading list.md").exists());
        assert_eq!(app.current_path(), Some(PathBuf::from("Reading list.md")));
        assert!(app.vault.as_ref().unwrap().exists("Reading list", None));
        // the welcome note now shows up as a backlink of the new note
        let bl = app.vault.as_ref().unwrap().backlinks(Path::new("Reading list.md"));
        assert!(bl.iter().any(|(p, _)| p == Path::new("Welcome.md")));
        // a red link to a drawing creates a drawing file
        app.open_link("Plan.excalidraw", None, None, false);
        assert!(d.join("Plan.excalidraw").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: TASK-003
    #[test]
    fn task_toggle_and_drawing_embed() {
        let (mut app, d) = test_app("tasks");
        let before = app.vault.as_ref().unwrap().all_tasks().iter().filter(|(_, t)| t.done).count();
        let (rel, t) = app.vault.as_ref().unwrap().all_tasks().into_iter().find(|(_, t)| !t.done).unwrap();
        app.toggle_task(&rel, t.check_offset);
        let after = app.vault.as_ref().unwrap().all_tasks().iter().filter(|(_, t)| t.done).count();
        assert_eq!(after, before + 1);
        assert!(std::fs::read_to_string(d.join(&rel)).unwrap().contains("[x]"));
        app.open(Path::new("Ideas.md"), false);
        app.new_drawing(Path::new(""), Some(PathBuf::from("Ideas.md")));
        app.flush_all();
        let ideas = std::fs::read_to_string(d.join("Ideas.md")).unwrap();
        assert!(ideas.contains("![[Drawing.excalidraw]]"));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: VAULT-013
    #[test]
    fn rename_keeps_open_buffers_in_step() {
        let (mut app, d) = test_app("rename");
        app.open(Path::new("Welcome.md"), false);
        app.open(Path::new("Ideas.md"), true);
        app.rename(Path::new("Ideas.md"), "Thoughts").unwrap();
        assert_eq!(app.current_path(), Some(PathBuf::from("Thoughts.md")));
        assert!(app.notes[Path::new("Welcome.md")].text.contains("[[Thoughts]]"));
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[cfg(test)]
mod export_tests {
    use super::*;

    fn test_app(tag: &str) -> (ShaleApp, PathBuf) {
        let d = std::env::temp_dir().join(format!("shale-appexport-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        vault::write_sample_vault(&d).unwrap();
        (ShaleApp::build(Options { screenshot: None, vault: Some(d.clone()), view: None, keys: None }, false), d)
    }

    // spec: PDF-003
    #[test]
    fn export_menu_entry_opens_the_dialog_and_writes_a_pdf() {
        for (rel, tag) in [("Welcome.md", "note"), ("Sketch.excalidraw", "drawing")] {
            let (mut app, d) = test_app(tag);
            app.actions.push(Action::Export(PathBuf::from(rel)));
            app.process_actions();
            let Some(Modal::Export(dialog)) = app.modal.take() else { panic!("export dialog should open for {rel}") };
            assert_eq!(dialog.drawing, rel.ends_with(".excalidraw"));
            assert!(dialog.path.ends_with(&format!("exports/{}.pdf", rel.split('.').next().unwrap())), "default path {}", dialog.path);
            let out = app.run_export(&dialog).unwrap();
            let bytes = std::fs::read(&out).unwrap();
            let pdf = hayro::hayro_syntax::Pdf::new(bytes).expect("a valid PDF was written");
            assert!(!pdf.pages().is_empty());
            // the export shows up in the vault and can be embedded in a note
            assert!(app.vault.as_ref().unwrap().exists(&format!("exports/{}.pdf", rel.split('.').next().unwrap()), None));
            let _ = std::fs::remove_dir_all(&d);
        }
    }

    // spec: PDF-001
    #[test]
    fn export_reports_a_bad_path() {
        let (mut app, d) = test_app("badpath");
        app.open_export_dialog(Path::new("Ideas.md"));
        let Some(Modal::Export(mut dialog)) = app.modal.take() else { panic!() };
        dialog.path = "/proc/definitely/not/writable/x.pdf".into();
        assert!(app.run_export(&dialog).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
