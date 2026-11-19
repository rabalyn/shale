//! Application state, actions and the main window layout.

use crate::config::Config;
use crate::drawing::DrawingDoc;
use crate::editor::{EditorAction, NoteEditor};
use crate::fsutil;
use crate::graph::{GraphClick, Graphs};
use crate::keys::{Action as KeyAction, Scope};
use crate::markdown;
use crate::panels::{FileTree, SearchBox};
use crate::reader::{ReaderAction, SceneCache};
use crate::theme;
use crate::vault::{self, rel_to_string, FileKind, Saved, Vault};
use crate::vault_path::VaultPath;
use eframe::egui::{self, Key, Modifiers, Ui};
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
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
    /// Mastodon / Bluesky feeds: the feed of this tab (None: not chosen yet, the last one is shown)
    Social(Option<crate::social::FeedRef>),
    /// The text of the settings file (`~/.config/shale/config.toml`), outside every vault (CFG-011)
    SettingsFile,
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
    /// The line of a heading the reading view scrolls to when it next draws (LINK-015).
    pub reader_line: Option<usize>,
    /// Pinned tabs sit left of all others (NAV-016).
    pub pinned: bool,
    /// Where the cursor and the scroll of the note are (NAV-038).
    pub place: NotePlace,
    pub editor: NoteEditor,
    pub title_buf: String,
    pub title_for: Option<PathBuf>,
}

/// Where a note tab is looked at: the selection (char indices) and the scroll offset of the last frame
/// it was shown, which are saved with the tab (NAV-038), and what a restored tab still has to put
/// into its view when it is first shown (a tab that is never shown keeps what it was given).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NotePlace {
    pub cursor: Option<(usize, usize)>,
    pub scroll: Option<f32>,
    pub restore_cursor: Option<(usize, usize)>,
    pub restore_scroll: Option<f32>,
}

/// What the tab bar drew last frame besides the tabs themselves.
#[derive(Default)]
pub(crate) struct TabMarks {
    /// the drop marker while a tab is dragged (NAV-014)
    pub drop_marker: Option<egui::Rect>,
    /// the tabs (by index) that show a pin (NAV-016)
    pub pinned: Vec<usize>,
    /// the "Pin tab" / "Unpin tab" entry of an open tab menu
    pub menu_pin: Option<egui::Rect>,
    /// the "Open in split right" entry of an open tab menu (NAV-033)
    pub menu_split: Option<egui::Rect>,
    /// the line that marks this tab bar as the one of the focused pane, when there are several (NAV-032)
    pub focus_line: Option<egui::Rect>,
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

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum LeftPanel {
    #[default]
    Files,
    Search,
    Tags,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum RightPanel {
    Backlinks,
    Outgoing,
    Outline,
    LocalGraph,
    Calendar,
}

mod drops;
#[cfg(all(test, target_os = "linux"))]
mod drops_tests;
#[cfg(test)]
mod handover_tests;
mod link_preview;
mod mail;
pub mod panes;
mod screenshot;
mod social;
mod sync;
mod tabs;
mod tex_setup;
#[cfg(all(test, target_os = "linux"))]
mod tree_ui_tests;
mod usage;
mod userguide;

/// A file "dropped" by a `--keys` script.
#[derive(Debug)]
pub(crate) struct ScriptDrop(pub(crate) PathBuf);

impl egui::DroppedFile for ScriptDrop {
    fn path(&self) -> &Path {
        &self.0
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        crate::fsutil::read(&self.0).map_err(|e| e.to_string())
    }
}

pub use crate::settings::SettingsTab;

pub enum Modal {
    /// The picker window (`Space f f`, NAV-020)
    Picker(crate::picker::Picker),
    QuickSwitch {
        query: String,
        sel: usize,
    },
    Rename {
        rel: PathBuf,
        name: String,
        error: Option<String>,
    },
    NewFolder {
        parent: PathBuf,
        name: String,
    },
    ConfirmTrash {
        rel: PathBuf,
    },
    Settings {
        vault_path: String,
        tab: SettingsTab,
        /// State of the Keybindings tab (KEY-006).
        keys: Box<crate::keys_ui::KeyDialog>,
    },
    /// the Data usage dialog (USE-001)
    DataUsage,
    Export(Box<crate::pdf::export_dialog::ExportDialog>),
    /// the guided setup of the LaTeX export (TEX-017); its state is `ShaleApp::tex.setup`
    TexSetup,
    Calendars(Box<crate::calendar::dialog::CalendarDialog>),
    Event(Box<crate::calendar::dialog::EventDialog>),
    Import(Box<crate::import::dialog::ImportDialog>),
    /// what an import of two or more items made (IMP-056)
    ImportSummary(Box<crate::import::summary::ImportSummary>),
    MailAccounts(Box<crate::mail::dialog::MailDialog>),
    Social(Box<crate::social::accounts_ui::SocialDialog>),
    /// pasted text with headings, none of them `#`: lift them? (EDIT-038)
    LiftPaste {
        text: String,
    },
    /// an Obsidian canvas is being opened: convert it or open it read-only?
    ConvertCanvas {
        rel: PathBuf,
        new_tab: bool,
        always: bool,
        existing: Option<PathBuf>,
    },
}

pub enum Action {
    /// put the keyboard into the editor of the open note (after opening it)
    FocusEditor,
    /// open the "Export to PDF" dialog for a note or drawing
    Export(PathBuf),
    /// open the "Export to PDF" dialog for a folder as one PDF, a book; the empty path is the vault (BOOK-010)
    ExportFolder(PathBuf),
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
    /// A heading of the Outline: scroll to it in the tab's mode (LINK-015).
    GotoHeading {
        rel: PathBuf,
        line: usize,
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
    ShowViewInNewTab(View),
    /// show a day (a day number) in the calendar: the sidebar's, else the big one (EDIT-108)
    OpenCalendarDay(i64),
    /// open a post found by the search in its feed, in a new tab (SOC-035)
    OpenPost(Box<crate::social::post_index::PostHit>),
    /// open a mail found by the search in the mail view (NAV-025)
    OpenMail(Box<crate::mail::db::MessageSummary>),
    CloseTab(usize),
    /// open tab `tab` of pane `pane` in a new pane to its right (NAV-033)
    SplitTabRight {
        pane: usize,
        tab: usize,
    },
    Rename(PathBuf),
    Trash(PathBuf),
    /// put the item moved to the trash last (in this session) back where it was (VAULT-020)
    RestoreTrashed,
    NewFolder(PathBuf),
    OpenVault(PathBuf),
    /// Look at the vault folder again (e.g. after a file was copied in).
    Rescan,
    /// show a message (the app's toast)
    Toast(String),
    /// open a web or mail address (checked, see `opener`)
    OpenUrl(String),
    /// make a contact note in the people folder (from `[[p Name`)
    CreatePerson(String),
    /// move a file or folder into a folder (drag and drop in the file list)
    Move {
        from: PathBuf,
        into: PathBuf,
    },
    /// make an editable drawing from an Obsidian canvas and open it
    ConvertCanvas {
        rel: PathBuf,
        new_tab: bool,
    },
    /// the pasted text has no `#` heading: ask whether to lift its headings (EDIT-038)
    AskLiftHeadings(String),
    /// paste this text into the open note's editor (the answer to the question above)
    PasteText(String),
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
    /// the open tabs, the active one and what the tab bar drew last frame
    pub tabbar: tabs::TabBar,
    /// the panes of the middle area; `tabbar` is the tab bar of the focused one (NAV-032)
    pub panes: panes::Panes,
    pub notes: HashMap<PathBuf, NoteDoc>,
    /// The undo history of every note that vim-lite edited, by vault path (EDIT-021).
    pub undo: crate::undo::Histories,
    pub drawings: HashMap<PathBuf, DrawingDoc>,
    /// The icons of the ribbon and the right sidebar, and a drag of one of them (LAY-001).
    pub side: crate::sidebar_ui::SideState,
    /// the file list: the open folders and the row the keyboard is on
    pub tree: FileTree,
    /// the search box of the left sidebar and the selected result
    pub search: SearchBox,
    pub tasks: TasksFilter,
    /// the graph of the whole vault and the local one in the side panel
    pub graphs: Graphs,
    pub calendar: crate::calendar::store::CalendarStore,
    /// The mail view (ADR-0007 phase 1b).
    pub mail: crate::mail::ui::MailView,
    /// the counts of the status bar, kept for a note and a vault that did not change (EDIT-107)
    pub status: crate::editor_cache::StatusCache,
    /// the calendar views, the jobs of their popups and the default calendar for new events
    pub cal: crate::calendar::ui::CalendarUi,
    /// the feed view (Mastodon / Bluesky)
    pub social: crate::social::state::SocialState,
    /// for background jobs to wake the window up when they finish
    pub(crate) ctx: Option<egui::Context>,
    /// drag and drop: where things can be dropped, what is dragged over the window, drops being read
    pub(crate) drag: drops::DragState,
    /// the tag words learned from the vault, and the job that learns them (see tagwords)
    pub tag_learner: crate::import::tagwords::TagLearner,
    pub scenes: SceneCache,
    /// the one media player state (MEDIA-007); the players in the views draw from it
    pub media: crate::media::MediaPlayer,
    /// the Data usage dialog's numbers, the cache jobs and the daily cleanup (USE)
    pub usage: crate::data_usage::DataUsage,
    /// the check of the vault's names for other systems (VAULT-028)
    pub vault_check: crate::vault_check::VaultCheck,
    /// the LaTeX page of the settings: the TeX found, the managed TeX Live and its jobs (TEX-001)
    pub tex: crate::pdf::tex::settings_ui::TexState,
    /// Where pandoc is, for Settings → Import (IMP-036).
    pub word: crate::settings::import::WordProbe,
    /// where the user guide comes from and where its copy goes (DEV-020)
    pub guide: crate::userguide::Places,
    /// paths that a second start of Shale hands over (DATA-015); none in tests and screenshots
    pub handover: Option<crate::instance::Handover>,
    pub modal: Option<Modal>,
    /// The key registry with the user's changes (KEY-001…009).
    pub keymap: crate::keys::Keymap,
    pub actions: Vec<Action>,
    pub toasts: Vec<(String, Instant)>,
    /// What this session moved to the trash, newest last: where it is in `.trash/` (VAULT-020).
    pub trashed: Vec<PathBuf>,
    last_scan: Instant,
    /// When the session state was last looked at for a change to write, and what the file was
    /// then (NAV-041).
    pub(crate) state_tick: (Instant, Option<crate::vault_state::VaultState>),
    pub welcome_path: String,
    /// screenshot mode: where the picture goes and whether it was saved
    shot: screenshot::ScreenshotRun,
    /// the `cursor_blink` setting that the visuals of egui follow (true at the start; screenshots start steady)
    blink_applied: bool,
    /// the frames drawn so far (paces screenshots and scripted input)
    frames: u32,
    /// the last failure to save the global settings (shown once)
    cfg_error: Option<String>,
    /// The settings file in its tab (CFG-011)
    pub sf: crate::settings_file::SettingsFile,
    /// The fonts of the areas of the window and the installed fonts (CFG-030).
    pub fonts: crate::fonts::FontState,
    /// the scripted input of tests of the real window (BUILD-007)
    script: crate::script::ScriptRun,
    /// where unsaved text is rescued to, and the rescues not dismissed yet (DATA-011)
    pub recovery: crate::recovery::RecoveryState,
    /// The preview of the link under vim's cursor and the web pages it read (EDIT-033).
    pub link_preview: crate::link_preview::LinkPreview,
}

impl ShaleApp {
    pub fn new(cc: &eframe::CreationContext<'_>, opts: Options, handover: Option<crate::instance::Handover>) -> Self {
        theme::apply(&cc.egui_ctx);
        if opts.screenshot.is_some() {
            // no fades, no scroll easing and a steady text cursor: a picture shows the end state,
            // whatever the speed of the machine (DEV-007: two runs give the same bytes)
            cc.egui_ctx.all_styles_mut(|s| {
                s.animation_time = 0.0;
                s.scroll_animation = egui::style::ScrollAnimation::none();
                s.visuals.text_cursor.blink = false;
            });
        }
        #[cfg(target_os = "linux")]
        {
            // drops from other programs on Wayland (a second listener; see wl_drop)
            crate::import::wl_drop::set_context(&cc.egui_ctx);
            crate::import::wl_drop::attach(cc);
        }
        let persist = opts.screenshot.is_none();
        let mut app = Self::build(opts, persist);
        if let Some(h) = &handover {
            h.attach(&cc.egui_ctx);
        }
        app.handover = handover;
        app
    }

    /// What later starts of Shale handed over (DATA-015): come forward, open a folder as the vault
    /// or a file of the open vault in a tab.
    pub(crate) fn take_handover(&mut self, ctx: &egui::Context) {
        let Some(batches) = self.handover.as_ref().map(|h| h.take()) else { return };
        for paths in batches {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            for p in paths {
                self.open_handed_over(&p);
            }
        }
    }

    fn open_handed_over(&mut self, path: &Path) {
        if path.is_dir() {
            let same = self.vault.as_ref().is_some_and(|v| std::fs::canonicalize(&v.root).ok() == std::fs::canonicalize(path).ok());
            if !same {
                self.open_vault(path);
            }
            return;
        }
        let rel = self.vault.as_ref().and_then(|v| {
            let root = std::fs::canonicalize(&v.root).ok()?;
            std::fs::canonicalize(path).ok()?.strip_prefix(root).ok().map(Path::to_path_buf)
        });
        match rel {
            Some(rel) => self.open(&rel, true),
            None => self.toast(format!("“{}” is not in the open vault", path.display())),
        }
    }

    /// Set up the app without a window. `persist` = read/write the user's config file.
    pub fn build(opts: Options, persist: bool) -> Self {
        let screenshot = opts.screenshot.clone();
        // screenshots only: SHALE_FAKE_NOW=2026-11-15T10:00:00 freezes the clock and the build stamp
        if screenshot.is_some() {
            if let Ok(v) = std::env::var("SHALE_FAKE_NOW") {
                match crate::calendar::time::parse_fake_now(&v) {
                    Some(secs) => crate::calendar::time::freeze(secs),
                    None => eprintln!("SHALE_FAKE_NOW={v}: not a date-time like 2026-11-15T10:00:00, the real clock is used"),
                }
            }
        }
        let (mut cfg, cfg_problem) = if persist { Config::load() } else { (Config::default(), None) };
        let persist_config = persist;
        crate::data_usage::dialog::sanitize(&mut cfg.caches);
        // screenshots and scripted checks of vim-lite: SHALE_VIM=1 turns it on
        if opts.screenshot.is_some() && std::env::var("SHALE_VIM").as_deref() == Ok("1") {
            cfg.vim_mode = true;
        }
        // Which vault to open: --vault, else (normal start) the last one, else a demo for screenshots.
        let vault_path = match (&opts.vault, &screenshot) {
            (Some(v), _) => Some(v.clone()),
            (None, Some(_)) => {
                // one folder per run: pictures made at the same time must not wipe each other's vault
                let d = std::env::temp_dir().join(format!("shale-demo-{}", std::process::id())).join("shale-demo-vault");
                let _ = std::fs::remove_dir_all(&d);
                let _ = vault::write_sample_vault(&d);
                Some(d)
            }
            (None, None) => cfg.vault.clone().filter(|p| p.is_dir()),
        };
        let default_welcome = dirs::document_dir().or_else(dirs::home_dir).unwrap_or_default().join("Notes");
        // where the data lives: the user's folders, or temporary ones for tests and screenshots
        let n = NEXT_MAIL_ROOT.fetch_add(1, Ordering::SeqCst);
        // tests: one folder per process (and app); screenshots: a fixed name, emptied first, because
        // the data usage dialog shows these paths and two runs must give the same picture
        let tag = if screenshot.is_some() { "screenshot".to_string() } else { std::process::id().to_string() };
        let mail_root = if persist {
            crate::mail::store::default_root()
        } else if screenshot.is_some() {
            std::env::temp_dir().join("shale-mail-screenshot")
        } else {
            std::env::temp_dir().join(format!("shale-mail-{tag}-{n}"))
        };
        let social_dir = if persist { dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("shale").join("social") } else { std::env::temp_dir().join(format!("shale-social-{tag}")) };
        let calendar_dir = if persist { crate::calendar::store::default_local_dir() } else { std::env::temp_dir().join(format!("shale-calendars-{tag}")) };
        let recovery_root = if persist { crate::recovery::default_root() } else { std::env::temp_dir().join(format!("shale-recovery-{tag}")) };
        if screenshot.is_some() {
            for d in [&mail_root, &social_dir, &calendar_dir, &recovery_root] {
                let _ = std::fs::remove_dir_all(d);
            }
        }
        let places = if persist {
            crate::data_usage::Places {
                pictures: social_dir.clone(),
                post_index: Some(crate::social::post_index::default_path()),
                mail_root: mail_root.clone(),
                debug_log: crate::config::input_log_path(),
                calendars: calendar_dir.clone(),
                calendar_trash: crate::calendar::store::default_trash_dir(),
                recovery: recovery_root.clone(),
                cleanup_file: crate::data_usage::cleanup::default_path(),
                vault: None,
            }
        } else {
            crate::data_usage::Places {
                pictures: social_dir.clone(),
                post_index: None,
                mail_root: mail_root.clone(),
                calendars: calendar_dir.clone(),
                calendar_trash: calendar_dir.join(".calendar-trash"),
                recovery: recovery_root.clone(),
                ..crate::data_usage::Places::in_folder(&std::env::temp_dir().join(format!("shale-usage-{tag}-{n}")))
            }
        };
        let mut app = ShaleApp {
            keymap: crate::keys::Keymap::from_overrides(&cfg.keybindings),
            cfg: cfg.clone(),
            persist_config,
            vdata: crate::vault_state::VaultData::default(),
            sync: sync::SyncState::default(),
            vault: None,
            tabbar: tabs::TabBar::new(),
            panes: panes::Panes::new(),
            notes: HashMap::new(),
            undo: crate::undo::Histories::default(),
            drawings: HashMap::new(),
            side: crate::sidebar_ui::SideState::new(crate::sidebar::Layout::from_config(&cfg)),
            tree: FileTree::default(),
            search: SearchBox::default(),
            tasks: TasksFilter::default(),
            graphs: Graphs::new(),
            calendar: {
                let mut c = crate::calendar::store::CalendarStore::new(cfg.calendars.clone(), calendar_dir);
                if persist {
                    c.trash_dir = crate::calendar::store::default_trash_dir();
                }
                c
            },
            mail: crate::mail::ui::MailView::new(mail_root),
            status: Default::default(),
            cal: crate::calendar::ui::CalendarUi::new(),
            social: {
                let mut s = crate::social::state::SocialState::new(social_dir);
                if persist {
                    s.open_index(&crate::social::post_index::default_path());
                }
                s
            },
            ctx: None,
            drag: drops::DragState::default(),
            tag_learner: crate::import::tagwords::TagLearner::new(),
            scenes: SceneCache::default(),
            media: crate::media::MediaPlayer::default(),
            modal: None,
            actions: vec![],
            toasts: vec![],
            trashed: vec![],
            last_scan: Instant::now(),
            state_tick: (Instant::now(), None),
            welcome_path: default_welcome.to_string_lossy().into_owned(),
            shot: screenshot::ScreenshotRun::new(screenshot),
            frames: 0,
            blink_applied: true,
            cfg_error: None,
            sf: crate::settings_file::SettingsFile::new(),
            fonts: crate::fonts::FontState::new(),
            script: crate::script::ScriptRun::new(opts.keys.as_deref().inspect(|_| crate::script::collect_hints()).map(|k| crate::script::parse(k).unwrap_or_default().into()).unwrap_or_default()),
            recovery: crate::recovery::RecoveryState { root: recovery_root, notice: vec![] },
            link_preview: Default::default(),
            usage: crate::data_usage::DataUsage::new(places),
            vault_check: Default::default(),
            tex: crate::pdf::tex::settings_ui::TexState::new(persist, &tag),
            word: Default::default(),
            guide: if persist { crate::userguide::Places::system() } else { crate::userguide::Places::none() },
            handover: None,
        };
        app.recovery.notice = crate::recovery::unseen(&app.recovery.root);
        if let Some(p) = cfg_problem {
            app.toast(p);
        }
        if let Some(p) = vault_path {
            app.open_vault(&p);
            if app.shot.path.is_some() && p.join("Welcome.md").is_file() {
                app.open(Path::new("Welcome.md"), false);
            }
            // screenshots: a demo calendar full of sample events
            if app.shot.path.is_some() && app.cfg.calendars.is_empty() {
                let src = app.calendar.new_local_source("Demo calendar", &[]);
                let id = src.id.clone();
                app.cfg.calendars = vec![src];
                app.calendar.set_sources(app.cfg.calendars.clone());
                let _ = app.calendar.fill_sample(&id);
            }
            // screenshots: a made-up mail store and account (no server, no real mail)
            if app.shot.path.is_some() && opts.view.as_deref() == Some("mail") && app.cfg.mail_accounts.is_empty() {
                match crate::mail::demo::seed(&app.mail.root) {
                    Ok(()) => {
                        app.cfg.mail_accounts = vec![crate::config::MailAccount { id: crate::mail::demo::ACCOUNT.into(), name: "Demo mail".into(), host: "imap.example.com".into(), user: "ada@example.com".into(), ..Default::default() }];
                        app.cfg.mail_sync_minutes = 0;
                    }
                    Err(e) => eprintln!("could not make the demo mail: {e}"),
                }
            }
            match opts.view.as_deref() {
                Some("calendar") => app.show_view(View::Calendar, false),
                Some("mail") => app.show_view(View::Mail, false),
                Some("calendar-side") => {
                    app.side.layout.show_panel(RightPanel::Calendar);
                }
                Some("tasks") => app.show_view(View::Tasks, false),
                Some("keybindings") => app.open_settings(SettingsTab::Keybindings),
                // the Data usage dialog with made-up cache files in the temporary folders
                Some("data-usage") => {
                    if let Err(e) = crate::data_usage::seed_demo(&app.usage.places) {
                        eprintln!("could not make the demo caches: {e}");
                    }
                    app.open_data_usage();
                }
                Some("graph") => app.show_view(View::Graph, false),
                // the feed view with made-up posts (no network); "social-import": and the dialog for saving some of them
                Some(v @ ("social" | "social-import")) => {
                    let demo = crate::social::demo::account();
                    app.cfg.social_accounts = vec![demo.clone()];
                    app.social.load_demo(&demo);
                    app.show_view(View::Social(None), false);
                    if v == "social-import" {
                        let posts = crate::social::demo::posts();
                        let items = [3, 4].iter().map(|i| crate::import::social_item(crate::social::demo::saved(&posts[*i]), "Mastodon", false)).collect();
                        app.open_import(items, vec![], Some((PathBuf::from("Social"), None)));
                    }
                }
                Some("drawing") => app.open(Path::new("Sketch.excalidraw"), false),
                // any file of the vault, e.g. `--view "open:Excalidraw/Plan.excalidraw.md"`
                Some(v) if v.starts_with("open:") => app.open(Path::new(&v["open:".len()..]), false),
                // a canvas shown read-only without the question, e.g. `--view "read-only:Map.canvas"`
                Some(v) if v.starts_with("read-only:") => app.open_canvas_read_only(Path::new(&v["read-only:".len()..]), false),
                // a canvas converted to a drawing (as the "Convert" button does), e.g. `--view "convert:Map.canvas"`
                Some(v) if v.starts_with("convert:") => {
                    app.convert_canvas(Path::new(&v["convert:".len()..]), false);
                }
                Some("reading") => {
                    if let Some(t) = app.tabbar.tabs.get_mut(app.tabbar.active) {
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
            // an old setting must not keep recording unnoticed; the start of the window is slow, so the
            // toast gets a later start time to stay visible long enough (SEC-009)
            app.toasts.push((format!("Keyboard recording is on: everything typed outside password fields is written to {}. Switch it off in Settings.", crate::config::input_log_path().display()), Instant::now() + Duration::from_secs(8)));
        }
        cfg = app.cfg.clone();
        let _ = cfg;
        app.settings_file_baseline();
        app
    }

    /// The player settings go to the media player each frame; a volume that was set in a player
    /// is saved once the mouse is let go (MEDIA-006).
    fn media_tick(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.pointer.any_down()) {
            if let Some(v) = self.media.take_volume_change() {
                self.cfg.media_volume = v;
                self.save_config();
            }
        }
        self.media.configure(self.cfg.media_volume, self.cfg.media_play_button_opacity);
        // videos that cannot play here (no ffmpeg) go to the system's player (MEDIA-010)
        for abs in self.media.take_open_requests() {
            let opened = self.vault.as_ref().map(|v| crate::opener::open_vault_file(&abs, &v.root));
            if let Some(Err(e)) = opened {
                self.toast(e);
            }
        }
    }

    pub fn toast(&mut self, msg: impl Into<String>) {
        self.toasts.push((msg.into(), Instant::now()));
    }

    /// Save the global config and the open vault's `.shale/` files.
    pub fn save_config(&mut self) {
        // (which tab is open in each pane is saved with the rest)
        self.remember_open_files();
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
            self.settings_file_written();
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
        self.undo.open_vault(root, unix_now());
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
                self.media.stop();
                self.vault = Some(v);
                self.tabbar.tabs.clear();
                self.panes.reset();
                self.notes.clear();
                self.drawings.clear();
                self.tabbar.active = 0;
                self.cfg.remember_vault(path.to_path_buf());
                // (opening a note rewrites the saved list, so read it first)
                let saved_panes: Vec<crate::vault_state::SavedPane> = if self.vdata.state.panes.is_empty() {
                    // (a file from before panes: its last tab was the open one)
                    vec![crate::vault_state::SavedPane { tabs: self.vdata.state.tabs.clone(), active: self.vdata.state.tabs.len().saturating_sub(1), ..Default::default() }]
                } else {
                    self.vdata.state.panes.clone()
                };
                let saved_focus = self.vdata.state.focused_pane;
                for (pi, sp) in saved_panes.iter().enumerate() {
                    if pi > 0 {
                        self.add_pane_right();
                    }
                    self.restore_pane_tabs(sp);
                    // a pane whose tabs are all gone is not brought back
                    if pi > 0 && self.tabbar.tabs.is_empty() {
                        self.close_pane(self.panes.focused);
                    }
                }
                if self.tabbar.tabs.is_empty() && self.panes.len() == 1 {
                    let first = self.vault.as_ref().and_then(|v| v.files.iter().find(|f| f.link_name.eq_ignore_ascii_case("welcome")).or_else(|| v.files.iter().find(|f| f.kind == FileKind::Note)).map(|f| f.rel.clone()));
                    match first {
                        Some(r) => self.open(&r, false),
                        None => self.tabbar.new_tab(View::Empty),
                    }
                }
                // the widths as they were (only when every pane that was saved came back)
                if saved_panes.len() == self.panes.len() && saved_panes.iter().all(|p| p.weight.is_finite() && p.weight > 0.0) {
                    self.panes.weights = saved_panes.iter().map(|p| p.weight).collect();
                }
                self.focus_pane(saved_focus);
                self.remember_open_files();
                if !had_files {
                    self.toast("Empty vault opened. Create a note with Ctrl+N.");
                }
                self.sync_vault_opened();
                self.save_config();
            }
            Err(e) => self.toast(format!("Could not open vault: {e}")),
        }
    }

    /// Open the tabs of one saved pane in the (empty) focused pane: pinned tabs left-most, the
    /// open one as saved (the last one when it is gone).
    fn restore_pane_tabs(&mut self, sp: &crate::vault_state::SavedPane) {
        let mut open_id = None;
        for (k, st) in sp.tabs.iter().enumerate() {
            let before = self.tabbar.tabs.len();
            let new_tab = !self.tabbar.tabs.is_empty();
            if let Some(f) = &st.file {
                let rel = PathBuf::from(f);
                if !self.vault.as_ref().map(|v| v.file(&rel).is_some()).unwrap_or(false) {
                    continue;
                }
                // a canvas tab comes back as it was left: read-only, without the question
                if crate::drawing::canvas::is_canvas_path(&rel) {
                    self.open_canvas_read_only(&rel, new_tab);
                } else {
                    self.open(&rel, new_tab);
                }
            } else if let Some(kind) = &st.view {
                self.restore_view_tab(kind, st, new_tab);
            } else if let Some(f) = &st.feed {
                // (the account must still exist)
                if self.cfg.social_accounts.iter().any(|e| e.account.id == f.account) {
                    self.show_view(View::Social(Some(f.clone())), new_tab);
                }
            }
            if self.tabbar.tabs.len() > before {
                let tab = &mut self.tabbar.tabs[self.tabbar.active];
                tab.pinned = st.pinned;
                // the reading view, the cursor and the scroll as they were, put into the view when it is first shown (NAV-038, NAV-039)
                if matches!(tab.view, View::Note(_)) {
                    tab.reading = st.reading;
                    let (cursor, scroll) = (st.cursor.map(|[a, b]| (a, b)), st.scroll.map(|y| y as f32));
                    tab.place = NotePlace { cursor, scroll, restore_cursor: cursor, restore_scroll: scroll };
                }
            }
            if k == sp.active && self.tabbar.tabs.len() > before {
                open_id = self.tabbar.tabs.get(self.tabbar.active).map(|t| t.id);
            }
        }
        // pinned tabs are left-most (a hand-edited file may say otherwise)
        let last_id = self.tabbar.tabs.get(self.tabbar.active).map(|t| t.id);
        self.tabbar.tabs.sort_by_key(|t| !t.pinned);
        self.tabbar.active = open_id.or(last_id).and_then(|id| self.tabbar.tabs.iter().position(|t| t.id == id)).unwrap_or(0);
    }

    /// Bring back a tab of a view that has no file (NAV-044): the view as it is shown when opened by
    /// hand, with the place it was left at (NAV-045 to NAV-047). The mail view and the calendar
    /// are one each in the window, so their place is the one of the first such tab. A kind that is
    /// not known is skipped.
    fn restore_view_tab(&mut self, kind: &str, st: &crate::vault_state::SavedTab, new_tab: bool) {
        use crate::vault_state::SavedTab;
        let view = match kind {
            SavedTab::MAIL => {
                if self.mail.restore.is_none() {
                    self.mail.restore = st.mail.clone().filter(|m| !m.account.is_empty());
                }
                View::Mail
            }
            SavedTab::CALENDAR => {
                if let Some(c) = st.calendar.as_ref().filter(|c| c.day != 0) {
                    if let Some(mode) = crate::calendar::ui::Mode::from_name(&c.mode) {
                        self.cal.main.mode = mode;
                    }
                    self.cal.main.anchor = c.day;
                    self.cal.main.selected = c.day;
                }
                View::Calendar
            }
            SavedTab::TASKS => {
                if let Some(t) = &st.tasks {
                    self.tasks = TasksFilter { show_done: t.show_done, query: t.query.clone(), only_due: t.only_due };
                }
                View::Tasks
            }
            SavedTab::GRAPH => {
                if let Some(g) = &st.graph {
                    self.graphs.global.show_missing = !g.hide_missing;
                    self.graphs.global.show_orphans = !g.hide_orphans;
                }
                View::Graph
            }
            SavedTab::SETTINGS_FILE => View::SettingsFile,
            _ => return,
        };
        self.show_view(view, new_tab);
    }

    pub fn show_view(&mut self, view: View, new_tab: bool) {
        if self.tabbar.tabs.is_empty() || new_tab {
            self.tabbar.new_tab(view);
            return;
        }
        // the Feeds button always opens a new Feeds tab (to add a feed to the workflow); the same
        // feed may be open in several tabs, so a feed is never "found" by equality. A new tab that
        // is still empty becomes the Feeds tab instead of leaving an empty one behind.
        if view == View::Social(None) && self.tabbar.tabs[self.tabbar.active].view != View::Empty {
            self.tabbar.new_tab(view);
            return;
        }
        // reuse a tab already showing it
        if let Some(i) = self.tabbar.tabs.iter().position(|t| t.view == view && !matches!(t.view, View::Social(_))) {
            self.tabbar.active = i;
            return;
        }
        let tab = &mut self.tabbar.tabs[self.tabbar.active];
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
            self.tree.expanded.insert(dir.to_path_buf());
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
                // the folds it had when it was left (EDIT-079)
                if let (Some(saved), Some(t)) = (self.vdata.state.folds.get(&rel_to_string(rel)), self.tabbar.tabs.get_mut(self.tabbar.active)) {
                    t.editor.fold.restore(rel, saved);
                }
            }
            FileKind::Drawing if crate::drawing::canvas::is_canvas_path(rel) && !self.drawings.contains_key(rel) => {
                // a canvas: open the copy converted earlier, convert it, or ask
                let existing = crate::drawing::canvas::find_converted(v, rel);
                match existing {
                    Some(copy) if self.cfg.auto_convert_canvas => self.open(&copy, new_tab),
                    None if self.cfg.auto_convert_canvas => {
                        self.convert_canvas(rel, new_tab);
                    }
                    _ => self.modal = Some(Modal::ConvertCanvas { rel: rel.to_path_buf(), new_tab, always: false, existing }),
                }
                return;
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

    /// Show an Obsidian canvas read-only (no question asked).
    pub fn open_canvas_read_only(&mut self, rel: &Path, new_tab: bool) {
        let Some(v) = &self.vault else { return };
        let doc = match fsutil::read_to_string(v.abs(rel)) {
            Ok(text) => DrawingDoc::new(rel.to_path_buf(), &text),
            Err(e) => DrawingDoc::unreadable(rel.to_path_buf(), &e.to_string()),
        };
        self.drawings.insert(rel.to_path_buf(), doc);
        self.show_view(View::Drawing(rel.to_path_buf()), new_tab);
        self.remember_open_files();
    }

    /// Make `Name.excalidraw.md` next to the canvas `rel` and open it. The canvas is not changed.
    pub fn convert_canvas(&mut self, rel: &Path, new_tab: bool) -> Option<PathBuf> {
        let v = self.vault.as_mut()?;
        let made = fsutil::read_to_string(v.abs(rel)).map_err(|e| e.to_string()).and_then(|text| crate::drawing::canvas::converted_file(rel, &text)).and_then(|content| {
            let to = v.new_note_path_max(rel.parent().unwrap_or(Path::new("")), &crate::drawing::canvas::drawing_name_for(rel), fsutil::MADE_NAME_MAX);
            v.create_file(&to, &content).map_err(|e| e.to_string())
        });
        match made {
            Ok(to) => {
                self.toast(format!("Converted to “{}” (the canvas is unchanged)", rel_to_string(&to)));
                // the canvas's read-only tab (if open) shows the new drawing instead
                if let Some(i) = self.tabbar.tabs.iter().position(|t| t.view.path() == Some(rel)) {
                    self.tabbar.active = i;
                    self.open(&to, false);
                } else {
                    self.open(&to, new_tab);
                }
                Some(to)
            }
            Err(e) => {
                self.toast(format!("Could not convert the canvas: {e}"));
                None
            }
        }
    }

    pub(crate) fn remember_open_files(&mut self) {
        if self.vault.is_none() {
            return;
        }
        self.vdata.state.panes = self.saved_panes();
        self.remember_folds();
        self.vdata.state.focused_pane = self.panes.focused;
        // (the tabs of the first pane stay where a Shale without panes reads them)
        self.vdata.state.tabs = self.vdata.state.panes.first().map(|p| p.tabs.clone()).unwrap_or_default();
    }

    /// Take the folds of the notes the editors showed into the vault state, and drop the ones of
    /// notes that are gone (EDIT-079). A note kept as it starts has no entry.
    fn remember_folds(&mut self) {
        let shown: Vec<(PathBuf, Option<Vec<crate::fold::Key>>)> = self.all_tabs().flat_map(|t| t.editor.fold.snapshot()).collect();
        for (rel, keys) in shown {
            match keys {
                Some(keys) => self.vdata.state.folds.insert(rel_to_string(&rel), keys),
                None => self.vdata.state.folds.remove(&rel_to_string(&rel)),
            };
        }
        if let Some(v) = &self.vault {
            self.vdata.state.folds.retain(|k, _| v.file(Path::new(k)).is_some());
        }
    }

    /// The session state is written when something in it changed, looked at every 5 seconds: the
    /// cursor and scroll of a note change all the time and are not written at every frame (NAV-041).
    /// A failed write is not told here (it is, at the next save of the config).
    fn state_tick(&mut self) {
        if self.vault.is_none() || !self.persist_config || self.state_tick.0.elapsed() < Duration::from_secs(5) {
            return;
        }
        self.state_tick.0 = Instant::now();
        self.remember_open_files();
        if self.state_tick.1.as_ref() != Some(&self.vdata.state) {
            let _ = self.vdata.save();
            self.state_tick.1 = Some(self.vdata.state.clone());
        }
    }

    pub fn current_path(&self) -> Option<PathBuf> {
        self.tabbar.tabs.get(self.tabbar.active).and_then(|t| t.view.path().map(|p| p.to_path_buf()))
    }

    pub fn current_folder(&self) -> PathBuf {
        self.current_path().and_then(|p| p.parent().map(|x| x.to_path_buf())).unwrap_or_default()
    }

    /// Follow a link: open the note if it exists, otherwise create it and open it.
    pub fn open_link(&mut self, target: &str, heading: Option<String>, from: Option<PathBuf>, new_tab: bool) {
        let Some(v) = &self.vault else { return };
        if let Some(rel) = v.resolve(target, from.as_deref()) {
            self.open(&rel, new_tab);
            if let Some(h) = heading.filter(|h| !h.trim().is_empty()) {
                let found = self.notes.get(&rel).and_then(|d| markdown::find_heading(&d.text, &h));
                match found {
                    Some(line) => self.goto_heading(&rel, line),
                    None => {
                        let name = self.vault.as_ref().and_then(|v| v.file(&rel)).map(|f| f.display_name()).unwrap_or_default();
                        self.toast(format!("No heading “{}” in {name}", h.trim()));
                    }
                }
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
                    let made = self.create_note(&folder, &name, new_tab);
                    self.relink_red_link(from.as_deref(), t, made.as_deref());
                }
                _ => self.toast("This link points outside the vault"),
            }
            return;
        }
        let folder = if self.cfg.new_notes_next_to_current { from.as_ref().and_then(|f| f.parent()).map(|p| p.to_path_buf()).unwrap_or_default() } else { PathBuf::new() };
        let made = self.create_note(&folder, target, new_tab);
        self.relink_red_link(from.as_deref(), t, made.as_deref());
    }

    /// A red link in `from` to `target` made the note `made`; when the note got another name than
    /// the link says, a date prefix (CFG-043) or a portable name (VAULT-027), the links of `from`
    /// to that name are rewritten so they reach it.
    fn relink_red_link(&mut self, from: Option<&Path>, target: &str, made: Option<&Path>) {
        let (Some(from), Some(made)) = (from, made) else { return };
        let Some(stem) = self.vault.as_ref().and_then(|v| v.file(made)).map(|f| f.link_name.clone()) else { return };
        let old_last = markdown::strip_md_ext(target.trim()).rsplit('/').next().unwrap_or_default();
        if stem.rsplit('/').next().unwrap_or(&stem).eq_ignore_ascii_case(old_last) {
            return;
        }
        let Some(doc) = self.notes.get_mut(from).filter(|d| d.unreadable.is_none()) else { return };
        let Some(text) = markdown::relink_to(&doc.text, target, stem.rsplit('/').next().unwrap_or(&stem)) else { return };
        doc.text = text.clone();
        doc.dirty = true;
        doc.last_edit = Instant::now();
        if let Some(v) = self.vault.as_mut() {
            v.set_note_text(from, &text);
        }
    }

    /// `name` (may have folders) with today's `YYMMDD_` in front of its last part, when the setting
    /// is on and the name has no date prefix yet (CFG-043).
    fn with_date_prefix(&self, name: &str) -> String {
        let name = name.trim();
        let (dir, file) = name.rsplit_once('/').map_or(("", name), |(d, f)| (d, f));
        if !self.cfg.date_prefix_new_notes || file.is_empty() || vault::date_prefix_of(file).is_some() {
            return name.to_string();
        }
        let prefixed = format!("{}{file}", vault::date_prefix_today());
        if name.contains('/') {
            format!("{dir}/{prefixed}")
        } else {
            prefixed
        }
    }

    pub fn create_note(&mut self, folder: &Path, typed: &str, new_tab: bool) -> Option<PathBuf> {
        let name = self.with_date_prefix(typed);
        let v = self.vault.as_mut()?;
        let rel = v.new_note_path(folder, &name);
        let content = if crate::drawing::excalidraw_md::is_plugin_file(&rel) {
            crate::drawing::excalidraw_md::new_file(&crate::drawing::Scene::default())
        } else if crate::drawing::excalidraw_md::is_drawing_path(&rel) {
            crate::drawing::Scene::default().to_json()
        } else {
            String::new()
        };
        match v.create_file(&rel, &content) {
            Ok(rel) => {
                let shown = v.file(&rel).map(|f| f.display_name()).unwrap_or_default();
                // a typed name that had to change is told (VAULT-027)
                let got = rel.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.toast(fsutil::portable_notice("Saved as", typed, &got, fsutil::TYPED_NAME_MAX).unwrap_or_else(|| format!("Created “{shown}”")));
                self.open(&rel, new_tab);
                if let Some(t) = self.tabbar.tabs.get_mut(self.tabbar.active) {
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
        let name = self.with_date_prefix("Drawing.excalidraw");
        let Some(v) = self.vault.as_mut() else { return };
        let rel = v.new_note_path(folder, &name);
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

    /// Open `rel` with the heading on `line` at the top of the view, in the mode the tab is in:
    /// the cursor on the line in the editor, the reading view scrolled to it (LINK-015).
    pub fn goto_heading(&mut self, rel: &Path, line: usize) {
        self.open(rel, false);
        if let Some(tab) = self.tabbar.tabs.get_mut(self.tabbar.active).filter(|t| t.reading && t.view.path() == Some(rel)) {
            tab.reader_line = Some(line);
        } else {
            self.goto_line(rel, line);
            if let Some(tab) = self.tabbar.tabs.get_mut(self.tabbar.active) {
                tab.editor.scroll_to_top = true;
            }
        }
    }

    pub fn goto_line(&mut self, rel: &Path, line: usize) {
        self.goto_match(rel, line, 0);
    }

    /// Open `rel` with the cursor at byte `col` of `line` (clamped to the line) and the keyboard
    /// in the editor.
    pub fn goto_match(&mut self, rel: &Path, line: usize, col: usize) {
        self.open(rel, false);
        if let (Some(doc), Some(tab)) = (self.notes.get(rel), self.tabbar.tabs.get_mut(self.tabbar.active)) {
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
        let Some(_tree) = self.hold_sync_tree() else { return };
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
        // after the wait limit nothing is saved into the merge: the buffers stay dirty
        let Some(_tree) = self.hold_sync_tree() else { return };
        self.flush_dirty();
    }

    /// Save every dirty buffer; the caller holds the tree lock.
    fn flush_dirty(&mut self) {
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
    /// (DATA-011, and the text of the settings tab, CFG-015). Returns where each text went. Failures are the last resort: stderr.
    pub fn rescue_unsaved(&mut self) -> Vec<PathBuf> {
        let date = crate::mail::store::today_string();
        let mut texts: Vec<(PathBuf, Vec<u8>)> = self.notes.iter().filter(|(_, d)| d.dirty && d.unreadable.is_none()).map(|(r, d)| (r.clone(), d.text.clone().into_bytes())).collect();
        for (rel, d) in &self.drawings {
            if d.dirty {
                if let Ok(t) = d.file_text() {
                    texts.push((rel.clone(), t.into_bytes()));
                }
            }
        }
        // the settings tab: text with an error, or kept while the file changed outside (CFG-015)
        if self.sf.dirty {
            texts.push((PathBuf::from("settings/config.toml"), self.sf.text.clone().into_bytes()));
        }
        let mut out = Vec::new();
        for (rel, bytes) in texts {
            match crate::recovery::rescue(&self.recovery.root, &date, &rel, &bytes) {
                Ok(p) => out.push(p),
                Err(e) => eprintln!("could not save “{}” and could not rescue it either: {e}", rel_to_string(&rel)),
            }
        }
        out
    }

    /// An open mail draft is saved when the window closes; if that fails its text goes to the recovery folder (MAIL-201).
    fn save_mail_draft_at_exit(&mut self) {
        if let Err(e) = self.mail.flush_draft() {
            match self.mail.draft_rescue_text() {
                Some((rel, text)) => match crate::recovery::rescue(&self.recovery.root, &crate::mail::store::today_string(), &rel, text.as_bytes()) {
                    Ok(_) => eprintln!("the mail draft could not be saved ({e}); its text is in the recovery folder"),
                    Err(e2) => eprintln!("the mail draft could not be saved ({e}) and could not be rescued either: {e2}"),
                },
                None => eprintln!("the mail draft could not be saved: {e}"),
            }
        }
    }

    fn autosave(&mut self) {
        // never save into the middle of a git merge (SYNC-013): the lock is held for the saves,
        // and while the sync has it the buffers stay dirty and this frame saves nothing
        let tree = self.sync.tree.clone();
        let Some(_tree) = tree.try_hold() else { return };
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
        for t in self.all_tabs_mut() {
            if let Some(p) = t.view.path() {
                if !files.contains(p) {
                    t.view = View::Empty;
                }
            }
        }
    }

    pub fn rename(&mut self, rel: &Path, new_name: &str) -> Result<(), String> {
        let to = self.renamed_path(rel, new_name)?;
        if to == rel {
            return Ok(());
        }
        // a typed name that had to change is told (VAULT-027)
        let notice = to.file_name().and_then(|n| fsutil::portable_notice("Renamed to", new_name, &n.to_string_lossy(), fsutil::TYPED_NAME_MAX));
        let (changed, failed) = self.rename_path(rel, &to)?;
        if let Some(m) = rewrite_problems("Renamed", &changed.changed, &failed) {
            self.toast(m);
        } else {
            let shown = to.file_name().map(|n| markdown::strip_md_ext(&n.to_string_lossy()).to_string()).unwrap_or_default();
            let head = notice.unwrap_or_else(|| format!("Renamed to “{shown}”"));
            self.toast(format!("{head}{}", links_message(&changed, "name")));
        }
        Ok(())
    }

    /// The vault path `rel` gets when the user types `new_name`: one name, in the same folder
    /// (VAULT-018), made portable, with the extension of a note or drawing kept.
    fn renamed_path(&self, rel: &Path, new_name: &str) -> Result<PathBuf, String> {
        let v = self.vault.as_ref().ok_or("no vault")?;
        let new_name = VaultPath::name(new_name).map_err(|e| e.to_string())?;
        let file_name = match v.file(rel).map(|f| f.kind) {
            Some(FileKind::Note) => format!("{}.md", markdown::strip_md_ext(&new_name)),
            Some(FileKind::Drawing) => {
                // keep the format: an Obsidian `.excalidraw.md` drawing or `.canvas` stays one
                let stem = crate::drawing::excalidraw_md::drawing_stem(&new_name);
                if crate::drawing::excalidraw_md::is_plugin_file(rel) {
                    format!("{stem}.excalidraw.md")
                } else if crate::drawing::canvas::is_canvas_path(rel) {
                    format!("{stem}.canvas")
                } else {
                    format!("{stem}.excalidraw")
                }
            }
            _ => new_name,
        };
        let parent = VaultPath::new(rel.parent().unwrap_or(Path::new(""))).map_err(|e| e.to_string())?;
        Ok(parent.join_name(&file_name).map_err(|e| e.to_string())?.into_path_buf())
    }

    /// Rename or move `rel` to `to` the way VAULT-016 says: links elsewhere are rewritten (when the
    /// setting is on), open buffers and tabs follow. Returns what happened to links and the notes
    /// that could not be updated. No toast: the callers tell.
    pub(crate) fn rename_path(&mut self, rel: &Path, to: &Path) -> Result<(crate::vault::LinkUpdate, Vec<(PathBuf, String)>), String> {
        let _tree = self.hold_sync_tree().ok_or("The sync is still changing files in the vault; try again in a moment")?;
        self.flush_dirty();
        let update_links = self.cfg.update_links_on_rename;
        let v = self.vault.as_mut().ok_or("no vault")?;
        let is_dir = v.abs(rel).is_dir();
        let changed = v.rename(rel, to, update_links).map_err(|e| e.to_string())?;
        let failed = v.failed_rewrites.clone();
        self.after_move(rel, to, is_dir, &changed.changed);
        self.remember_open_files();
        Ok((changed, failed))
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
        let got = rel.as_path().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if let Some(m) = fsutil::portable_notice("Saved as", &name.replace('\\', "/"), &got, fsutil::TYPED_NAME_MAX) {
            self.toast(m);
        }
        Ok(rel.into_path_buf())
    }

    /// Point the red links `target` in `note` at the file `to` ("Fix link", LINK-012).
    pub fn fix_link(&mut self, note: &Path, target: &str, to: &Path) {
        let Some(_tree) = self.hold_sync_tree() else { return };
        self.flush_dirty();
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
        self.undo.moved(remap);
        // the folds of the moved notes follow (EDIT-079)
        for t in self.all_tabs_mut() {
            t.editor.fold.moved(remap);
        }
        for k in self.vdata.state.folds.keys().cloned().collect::<Vec<_>>() {
            if let (Some(nk), Some(v)) = (remap(Path::new(&k)), self.vdata.state.folds.remove(&k)) {
                self.vdata.state.folds.insert(rel_to_string(&nk), v);
            }
        }
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
        for t in self.all_tabs_mut() {
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
            // an open drawing whose links were rewritten shows the file as it is now (it was saved
            // before the rename, so nothing is lost)
            if self.drawings.contains_key(c) {
                if let Ok(text) = fsutil::read_to_string(v.abs(c)) {
                    self.drawings.insert(c.clone(), DrawingDoc::new(c.clone(), &text));
                }
            }
        }
        // open folders in the file list move along
        let exp: Vec<PathBuf> = self.tree.expanded.iter().cloned().collect();
        for e in exp {
            if let Ok(rest) = e.strip_prefix(rel) {
                self.tree.expanded.remove(&e);
                self.tree.expanded.insert(to.join(rest));
            }
        }
    }

    /// Move a note, drawing, file or folder into another folder (drag and drop in the file list).
    pub fn move_into(&mut self, rel: &Path, into: &Path) -> Result<PathBuf, String> {
        if rel.parent().unwrap_or(Path::new("")) == into {
            return Ok(rel.to_path_buf());
        }
        let _tree = self.hold_sync_tree().ok_or("The sync is still changing files in the vault; try again in a moment")?;
        self.flush_dirty();
        let v = self.vault.as_mut().ok_or("no vault")?;
        let is_dir = v.abs(rel).is_dir();
        let (to, changed) = v.move_into(rel, into).map_err(|e| e.to_string())?;
        let failed = v.failed_rewrites.clone();
        self.after_move(rel, &to, is_dir, &changed.changed);
        if let Some(m) = rewrite_problems("Moved", &changed.changed, &failed) {
            self.toast(m);
        }
        if !into.as_os_str().is_empty() {
            self.tree.expanded.insert(into.to_path_buf());
        }
        let name = rel.file_name().map(|n| markdown::strip_md_ext(&n.to_string_lossy()).to_string()).unwrap_or_default();
        let place = if into.as_os_str().is_empty() { "the vault".to_string() } else { format!("“{}”", rel_to_string(into)) };
        self.toast(format!("Moved “{name}” to {place}{}", links_message(&changed, "place")));
        self.remember_open_files();
        Ok(to)
    }

    pub fn trash(&mut self, rel: &Path) {
        let Some(_tree) = self.hold_sync_tree() else { return };
        self.flush_dirty();
        // unsaved edits that could not be saved must not go away with the buffer (DATA-009)
        if let Some(bad) = self.notes.iter().find(|(k, d)| k.starts_with(rel) && d.dirty).map(|(k, _)| rel_to_string(k)).or_else(|| self.drawings.iter().find(|(k, d)| k.starts_with(rel) && d.dirty).map(|(k, _)| rel_to_string(k))) {
            self.toast(format!("“{bad}” has changes that could not be saved, so “{}” is not moved to the trash", rel_to_string(rel)));
            return;
        }
        let Some(v) = self.vault.as_mut() else { return };
        match v.trash(rel) {
            Ok(in_trash) => {
                self.trashed.push(in_trash);
                self.notes.retain(|k, _| !k.starts_with(rel));
                self.drawings.retain(|k, _| !k.starts_with(rel));
                for t in self.all_tabs_mut() {
                    if t.view.path().map(|p| p.starts_with(rel)).unwrap_or(false) {
                        t.view = t.back.pop().unwrap_or(View::Empty);
                    }
                    t.back.retain(|b| !b.path().map(|p| p.starts_with(rel)).unwrap_or(false));
                    t.fwd.retain(|b| !b.path().map(|p| p.starts_with(rel)).unwrap_or(false));
                }
                self.toast(format!("Moved “{}” to the vault's .trash folder. Right-click the empty space in the file list and choose “Restore last deleted” to put it back", rel_to_string(rel)));
                self.remember_open_files();
            }
            Err(e) => self.toast(format!("Could not move to trash: {e}")),
        }
    }

    /// Whether "Restore last deleted" has something to restore: an item this session moved to
    /// the trash that is still there.
    pub fn can_restore_trashed(&self) -> bool {
        let Some(v) = &self.vault else { return false };
        self.trashed.iter().any(|t| std::fs::symlink_metadata(v.abs(t)).is_ok())
    }

    /// Put the item moved to the trash last back at its original path (VAULT-020). Refuses, and
    /// keeps it in the trash, when something is there already.
    pub fn restore_trashed(&mut self) {
        let Some(_tree) = self.hold_sync_tree() else { return };
        let Some(v) = self.vault.as_mut() else { return };
        while let Some(t) = self.trashed.last().cloned() {
            if std::fs::symlink_metadata(v.abs(&t)).is_err() {
                self.trashed.pop(); // taken out of the trash by hand
                continue;
            }
            match v.restore(&t) {
                Ok(back) => {
                    self.trashed.pop();
                    self.toast(format!("Restored “{}”", rel_to_string(&back)));
                }
                Err(e) => self.toast(format!("Could not restore: {e}")),
            }
            return;
        }
        self.toast("There is nothing to restore from this session");
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
        match self.side.left {
            LeftPanel::Files => {
                let cur = self.current_path();
                // the open note's folders open, so it can be selected
                let mut p = cur.as_deref().and_then(|c| c.parent());
                while let Some(d) = p.filter(|d| !d.as_os_str().is_empty()) {
                    self.tree.expanded.insert(d.to_path_buf());
                    p = d.parent();
                }
                let rows = self.visible_tree_rows();
                self.tree.sel = cur.filter(|c| rows.iter().any(|(r, _)| r == c)).or_else(|| rows.first().map(|(r, _)| r.clone()));
                self.tree.scroll = true;
            }
            LeftPanel::Search => self.search.focus = true,
            LeftPanel::Tags => {}
        }
    }

    /// Focus pane `i` and put the keyboard into the editor of its open note (Ctrl+H / Ctrl+L
    /// between panes, NAV-011).
    pub(crate) fn focus_pane_keyboard(&mut self, i: usize) {
        self.focus_pane(i);
        self.focus_editor();
    }

    /// After a button or key that only changes what the window shows: the editor of the open note has the
    /// keyboard again, with the cursor and selection where they were (EDIT-042). Nothing in the reading
    /// view or another view, which have no editor to give it to.
    pub(crate) fn focus_editor_if_note(&mut self) {
        if self.tabbar.tabs.get(self.tabbar.active).is_some_and(|t| matches!(t.view, View::Note(_)) && !t.reading) {
            self.focus_editor();
        }
    }

    /// The edit / reading view button and its keys (Ctrl+E, KEY-015).
    pub(crate) fn toggle_reading_view(&mut self) {
        if let Some(t) = self.tabbar.tabs.get_mut(self.tabbar.active) {
            t.reading = !t.reading;
        }
        self.focus_editor_if_note();
    }

    /// The Markdown preview button and its key (KEY-015).
    pub(crate) fn toggle_live_preview(&mut self) {
        self.cfg.live_preview = !self.cfg.live_preview;
        self.save_config();
        self.focus_editor_if_note();
    }

    pub(crate) fn toggle_left_sidebar(&mut self) {
        self.cfg.show_left = !self.cfg.show_left;
        self.focus_editor_if_note();
    }

    pub(crate) fn toggle_right_sidebar(&mut self) {
        self.cfg.show_right = !self.cfg.show_right;
        self.focus_editor_if_note();
    }

    /// Do the keys of the note view (KEY-014) count now: the focused pane shows a note, no dialog or link
    /// preview is open, and the keyboard is in its text or in no text field.
    fn note_keys_active(&self, ctx: &egui::Context) -> bool {
        let Some(tab) = self.tabbar.tabs.get(self.tabbar.active) else { return false };
        let View::Note(rel) = &tab.view else { return false };
        self.modal.is_none() && self.link_preview.open.is_none() && self.notes.contains_key(rel) && tab.editor.owns_keyboard(ctx)
    }

    /// An Alt key of a button of the note view: the same as a click on that button; a button that is not
    /// there (the toolbar in the reading view, the preview button there) or greyed out does nothing (KEY-015).
    fn note_key(&mut self, ctx: &egui::Context, a: KeyAction) {
        let active = self.tabbar.active;
        let Some(tab) = self.tabbar.tabs.get(active) else { return };
        let (View::Note(rel), reading, tab_id) = (&tab.view, tab.reading, tab.id) else { return };
        let bar = self.cfg.format_toolbar && !reading && self.notes.get(rel).is_some_and(|d| d.unreadable.is_none());
        match a {
            KeyAction::NoteReading => self.toggle_reading_view(),
            KeyAction::NotePreview if !reading => self.toggle_live_preview(),
            KeyAction::NoteNewTab => self.tabbar.new_tab(View::Empty),
            KeyAction::NoteMenu => {
                ctx.data_mut(|d| d.insert_temp(egui::Id::unique(("note-menu-key", tab_id)), true));
                ctx.request_repaint();
            }
            // fold all and unfold all work in the reading view too; the others act on the cursor, which it has not (EDIT-098)
            KeyAction::NoteFoldAll | KeyAction::NoteUnfoldAll => {
                use crate::fold::FoldCmd;
                self.tabbar.tabs[active].editor.key_fold(if a == KeyAction::NoteFoldAll { FoldCmd::FoldAll } else { FoldCmd::UnfoldAll });
            }
            KeyAction::NoteFold | KeyAction::NoteUnfold if !reading => {
                use crate::fold::FoldCmd;
                self.tabbar.tabs[active].editor.key_fold(if a == KeyAction::NoteFold { FoldCmd::Fold } else { FoldCmd::Unfold });
            }
            KeyAction::NoteCopyCode if !reading => self.tabbar.tabs[active].editor.key_copy_code(),
            KeyAction::NoteHighlightColour if bar => self.tabbar.tabs[active].editor.key_menu(crate::format_bar::Menu::Highlight),
            KeyAction::NoteFontColour if bar => self.tabbar.tabs[active].editor.key_menu(crate::format_bar::Menu::Font),
            KeyAction::NoteHeading if bar => self.tabbar.tabs[active].editor.key_menu(crate::format_bar::Menu::Heading),
            _ => {
                if let Some(cmd) = crate::format_bar::cmd_of(a).filter(|_| bar) {
                    self.tabbar.tabs[active].editor.key_format(cmd);
                }
            }
        }
        ctx.request_repaint();
    }

    /// Put the keyboard back into the editor of the open note (Ctrl+L).
    pub fn focus_editor(&mut self) {
        self.tree.sel = None;
        self.search.sel = None;
        if let Some(t) = self.tabbar.tabs.get_mut(self.tabbar.active) {
            t.editor.request_focus = true;
        }
    }

    /// Show `day` in the calendar (EDIT-108): the panel of the sidebar when a view has it (the sidebar
    /// is shown), else the calendar tab; its month grid selects the day.
    pub fn open_calendar_day(&mut self, day: i64) {
        let side = self.side.layout.show_panel(RightPanel::Calendar);
        let view = if side { &mut self.cal.side } else { &mut self.cal.main };
        view.mode = crate::calendar::ui::Mode::Month;
        view.anchor = day;
        view.selected = day;
        if side {
            self.cfg.show_right = true;
        } else {
            self.show_view(View::Calendar, false);
        }
    }

    /// Do what the calendar panel asked for.
    pub fn calendar_action(&mut self, a: crate::calendar::ui::CalAction) {
        use crate::calendar::ui::CalAction;
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
                let target = self.cal.last_event_calendar.as_ref().and_then(|id| srcs.iter().find(|s| s.id == *id)).or_else(|| srcs.iter().find(|s| s.enabled)).or_else(|| srcs.first()).map(|s| s.id.clone());
                match target {
                    Some(id) => {
                        let mut d = crate::calendar::dialog::EventDialog::new_at(id, start);
                        d.week_monday = self.cfg.week_starts_monday;
                        self.modal = Some(Modal::Event(Box::new(d)));
                    }
                    None => self.open_calendar_settings(),
                }
            }
            CalAction::Edit(occ) => {
                let found = self.calendar.sources.get(occ.source).map(|s| s.id.clone()).zip(self.calendar.event(&occ).cloned());
                if let Some((sid, ev)) = found {
                    // "all repeats" asked on a changed single date: the series itself is edited
                    let series = self.calendar.events.get(&sid).and_then(|v| v.iter().find(|x| x.uid == ev.uid && x.recurrence_id.is_none())).cloned();
                    let ev = if ev.recurrence_id.is_some() { series.unwrap_or(ev) } else { ev };
                    let mut d = crate::calendar::dialog::EventDialog::edit(sid, ev);
                    d.week_monday = self.cfg.week_starts_monday;
                    self.modal = Some(Modal::Event(Box::new(d)));
                }
            }
            CalAction::EditOne(occ) => {
                let found = self.calendar.sources.get(occ.source).map(|s| s.id.clone()).zip(self.calendar.event(&occ).cloned());
                if let Some((sid, ev)) = found {
                    // a date that was changed before is edited as it is; else it becomes a copy of the series for that date
                    let one = if ev.recurrence_id.is_some() { ev } else { ev.override_at(occ.start, occ.end) };
                    let mut d = crate::calendar::dialog::EventDialog::edit(sid, one);
                    d.week_monday = self.cfg.week_starts_monday;
                    self.modal = Some(Modal::Event(Box::new(d)));
                }
            }
            CalAction::DeleteOne(occ) => {
                let found = self.calendar.sources.get(occ.source).map(|s| s.id.clone()).zip(self.calendar.event(&occ).cloned());
                if let Some((sid, ev)) = found {
                    match self.calendar.delete_occurrence(&sid, &ev, occ.start, self.ctx.clone()) {
                        Ok(job) => self.cal.jobs.push((job, sid)),
                        Err(e) => self.toast(format!("Could not remove the date: {e}")),
                    }
                }
            }
            CalAction::Delete(occ) => {
                let found = self.calendar.sources.get(occ.source).map(|s| s.id.clone()).zip(self.calendar.event(&occ).cloned());
                if let Some((sid, ev)) = found {
                    match self.calendar.delete_event(&sid, ev, self.ctx.clone()) {
                        Ok(job) => self.cal.jobs.push((job, sid)),
                        Err(e) => self.toast(format!("Could not delete the event: {e}")),
                    }
                }
            }
        }
    }

    pub fn open_calendar_settings(&mut self) {
        self.modal = Some(Modal::Calendars(Box::new(crate::calendar::dialog::CalendarDialog::new(self.cfg.calendars.clone(), self.cfg.week_starts_monday))));
    }

    /// Apply a calendar settings dialog action. Returns true when the dialog should close.
    pub fn calendar_dialog_action(&mut self, d: &mut crate::calendar::dialog::CalendarDialog, a: crate::calendar::dialog::CalDialogAction) -> bool {
        use crate::calendar::dialog::CalDialogAction;
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
    /// A note's dialog also offers the LaTeX templates of the vault (TEX-016): it starts on the
    /// engine, set and kind of the last export, or on the ones the note's `latex-template:` names.
    pub fn open_export_dialog(&mut self, rel: &Path) {
        use crate::pdf::export_dialog::ExportEngine;
        let Some(v) = &self.vault else { return };
        let Some(f) = v.file(rel) else { return };
        let drawing = f.kind == FileKind::Drawing;
        let name = f.display_name();
        let opts = if drawing {
            let scene = self.drawings.get(rel).map(|d| d.scene.clone()).or_else(|| crate::drawing::Scene::read_file(&v.abs(rel)).ok()).unwrap_or_default();
            crate::pdf::export::ExportOptions::for_drawing(&scene)
        } else {
            // the dialog starts with what the note's `print-*` properties say (PDF-013, PDF-019)
            crate::pdf::export::ExportOptions { numbered_headings: self.cfg.numbered_headings, ..crate::pdf::export::ExportOptions::for_note() }.with_note_properties(&self.note_text(rel))
        };
        let path = v.root.join("exports").join(format!("{name}.pdf"));
        let mut dialog = crate::pdf::export_dialog::ExportDialog::new(rel.to_path_buf(), drawing, name, opts, path);
        if !drawing {
            let (title, root) = (f.title(), v.root.clone());
            let text = self.note_text(rel);
            let last = &self.vdata.state.latex_export;
            let props = crate::markdown::front_matter_values(&text);
            let noted = crate::pdf::tex::dialog::note_choice(&props);
            // a note that names its template is exported through it; else the last choice of the vault
            let engine = if noted.is_some() || last.latex { ExportEngine::Latex } else { ExportEngine::Builtin };
            let prefer = noted.or_else(|| (!last.set.is_empty()).then(|| (last.set.clone(), last.kind.clone())));
            dialog = dialog.with_latex(crate::pdf::tex::dialog::TexExport::new(&root, title, &text, prefer), engine);
        }
        self.modal = Some(Modal::Export(Box::new(dialog)));
    }

    /// Show the export dialog for a folder as one PDF, a book (BOOK-010): `dir` is the folder, the
    /// empty path the vault. It is saved to `<vault>/exports/<folder name>.pdf`. It offers the LaTeX
    /// templates that can hold a folder too (BOOK-013), starting as a note's dialog does: on the
    /// template the top folder note names, else on the last export's engine, set and kind.
    pub fn open_export_folder_dialog(&mut self, dir: &Path) {
        use crate::pdf::export_dialog::ExportEngine;
        self.flush_all();
        let Some(v) = &self.vault else { return };
        let name = dir.file_name().or_else(|| v.root.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Vault".to_string());
        let opts = crate::pdf::export::ExportOptions { numbered_headings: self.cfg.numbered_headings, ..crate::pdf::export::ExportOptions::for_note() };
        let path = v.root.join("exports").join(format!("{name}.pdf"));
        let book = crate::pdf::book_dialog::BookExport::new(v, dir);
        let plan = book.plan.as_ref().ok();
        let title = plan.map(|p| p.title.clone()).unwrap_or_else(|| name.clone());
        let text = plan.and_then(|p| p.note.as_deref()).map(|n| self.note_text(n)).unwrap_or_default();
        let last = &self.vdata.state.latex_export;
        let noted = crate::pdf::tex::dialog::note_choice(&crate::markdown::front_matter_values(&text));
        let engine = if noted.is_some() || last.latex { ExportEngine::Latex } else { ExportEngine::Builtin };
        let prefer = noted.or_else(|| (!last.set.is_empty()).then(|| (last.set.clone(), last.kind.clone())));
        let tex = crate::pdf::tex::dialog::TexExport::for_folder(&v.root, title, &text, prefer);
        self.modal = Some(Modal::Export(Box::new(crate::pdf::export_dialog::ExportDialog::new(dir.to_path_buf(), false, name, opts, path).with_book(book).with_latex(tex, engine))));
    }

    /// Lay a folder's book out as a job (BOOK-010).
    pub fn start_book_export(&mut self, d: &mut crate::pdf::export_dialog::ExportDialog, ctx: &egui::Context) -> Result<(), String> {
        self.flush_all();
        let root = self.vault.as_ref().ok_or("No vault is open")?.root.clone();
        let book = d.book.as_mut().ok_or("This is not the export of a folder")?;
        let ctx = ctx.clone();
        book.start(root, d.opts.clone(), move || ctx.request_repaint());
        self.remember_export_engine(false, d);
        Ok(())
    }

    /// Where a folder's export is: when the job has ended, the PDF is saved (or why not).
    pub fn book_export_tick(&mut self, d: &mut crate::pdf::export_dialog::ExportDialog) -> Option<Result<PathBuf, String>> {
        let made = d.book.as_mut()?.poll()?;
        Some(made.and_then(|bytes| self.save_pdf(&d.path, &bytes)))
    }

    /// The text of a note as the window has it: the editor's buffer, else what the vault read.
    fn note_text(&self, rel: &Path) -> String {
        self.notes.get(rel).map(|n| n.text.clone()).or_else(|| self.vault.as_ref().and_then(|v| v.notes.get(rel)).map(|n| n.text.clone())).unwrap_or_default()
    }

    /// Write the PDF for the dialog's file and options. Returns where it was saved.
    pub fn run_export(&mut self, d: &crate::pdf::export_dialog::ExportDialog) -> Result<PathBuf, String> {
        self.flush_all();
        let v = self.vault.as_ref().ok_or("No vault is open")?;
        let bytes = if d.drawing {
            let scene = match self.drawings.get(&d.rel) {
                Some(doc) => doc.scene.clone(),
                None => crate::drawing::Scene::read_file(&v.abs(&d.rel))?,
            };
            crate::pdf::export::export_drawing(&scene, &d.name, &d.opts, &crate::drawing::pictures::images(&scene, v, &d.rel))?
        } else {
            let text = self.note_text(&d.rel);
            crate::pdf::export::export_note(v, &d.rel, &text, &crate::pdf::export::ExportOptions { column: self.editor_column(&d.rel), ..d.opts.clone() })?
        };
        self.remember_export_engine(false, d);
        self.save_pdf(&d.path, &bytes)
    }

    /// The width of the editor's text column where a note is shown: embeds are as big in
    /// proportion to the text as there (PDF-005).
    fn editor_column(&self, rel: &Path) -> Option<f32> {
        self.all_tabs().filter(|t| matches!(&t.view, View::Note(p) if p == rel)).find_map(|t| t.editor.column)
    }

    /// Save the PDF bytes at the dialog's path (made, with its folder, when it is not there yet).
    fn save_pdf(&mut self, path: &str, bytes: &[u8]) -> Result<PathBuf, String> {
        let out = PathBuf::from(crate::layout::shellexpand(path));
        if out.as_os_str().is_empty() {
            return Err("Choose where to save the PDF".into());
        }
        if let Some(dir) = out.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("Could not create the folder: {e}"))?;
        }
        crate::fsutil::write_atomic(&out, bytes).map_err(|e| format!("Could not save the PDF: {e}"))?;
        if let Some(v) = self.vault.as_mut() {
            v.rescan();
        }
        Ok(out)
    }

    /// The engine, set and kind of this export are what the dialog starts on next time.
    fn remember_export_engine(&mut self, latex: bool, d: &crate::pdf::export_dialog::ExportDialog) {
        if d.drawing {
            return;
        }
        let state = &mut self.vdata.state.latex_export;
        state.latex = latex;
        if let Some((set, kind)) = d.tex.as_ref().and_then(|t| Some((t.current_set()?.id.clone(), t.kind.clone()))) {
            (state.set, state.kind) = (set, kind);
        }
        self.save_config();
    }

    /// Where the window is with a LaTeX export: what TeX it found, and the job that is running.
    /// Returns where the PDF was saved when the job has just finished.
    pub fn tex_export_tick(&mut self, d: &mut crate::pdf::export_dialog::ExportDialog, ctx: &egui::Context) -> Option<Result<PathBuf, String>> {
        use crate::pdf::tex::dialog::TexStatus;
        if !d.latex() {
            return None;
        }
        if self.tex.detection.is_none() {
            self.tex.check_again(ctx);
        }
        let status = match self.tex.detection.as_ref().map(|det| det.selected(self.cfg.tex.engine)) {
            None => TexStatus::Checking,
            Some(Ok(e)) => TexStatus::Ready(format!("Using {} (LaTeX kernel {})", crate::pdf::tex::settings_ui::engine_name(e), e.kernel)),
            Some(Err(why)) => TexStatus::Missing(why),
        };
        let tex = d.tex.as_mut()?;
        tex.status = status;
        let made = tex.poll()?;
        let saved = self.save_pdf(&d.path, &made.pdf);
        if saved.is_ok() {
            let mut notes = Vec::new();
            if made.placeholder_logo {
                notes.push("the logo is a placeholder, so it is not CI-conformant".to_string());
            }
            if !made.warnings.is_empty() {
                notes.push(format!("{} embed(s) could not be used: {}", made.warnings.len(), made.warnings.join(", ")));
            }
            self.toast(if notes.is_empty() { "Exported through LaTeX".to_string() } else { format!("Exported through LaTeX ({})", notes.join("; ")) });
        }
        Some(saved)
    }

    /// Export through LaTeX: write the note as the chosen kind now, compile it as a job.
    pub fn start_tex_export(&mut self, d: &mut crate::pdf::export_dialog::ExportDialog, ctx: &egui::Context) -> Result<(), String> {
        self.flush_all();
        let engine = match self.tex.detection.as_ref().map(|det| det.selected(self.cfg.tex.engine)) {
            Some(Ok(e)) => e.clone(),
            Some(Err(why)) => return Err(format!("No TeX can be used: {why}")),
            None => return Err("Shale is still looking for a TeX: try again in a moment".into()),
        };
        let text = self.note_text(&d.rel);
        let column = self.editor_column(&d.rel);
        let v = self.vault.as_ref().ok_or("No vault is open")?;
        let tex = d.tex.as_mut().ok_or("This note has no LaTeX choices")?;
        let (set, kind) = (tex.current_set().ok_or("Choose a template set first: import one in Settings → LaTeX")?, tex.current_kind().ok_or("Choose a kind of document")?);
        let doc = match &d.book {
            // a folder: its book as it is in the vault now, which has the saved notes (BOOK-014)
            Some(book) => {
                let plan = crate::pdf::book::plan(v, &book.folder)?;
                crate::pdf::tex::book::write_book(&crate::pdf::tex::book::BookRequest { vault: v, plan: &plan, opts: &d.opts, set, kind, fields: &tex.values })?
            }
            None => {
                let opts = crate::pdf::export::ExportOptions { column, ..d.opts.clone() };
                crate::pdf::tex::writer::write(&crate::pdf::tex::writer::Request { vault: v, rel: &d.rel, text: &text, opts: &opts, set, kind, fields: &tex.values })?
            }
        };
        let ctx = ctx.clone();
        tex.start(engine, doc, move || ctx.request_repaint());
        self.remember_export_engine(true, d);
        Ok(())
    }

    /// "Save the .tex" of a failed export: the folder next to the PDF path.
    pub fn save_failed_tex(&mut self, d: &mut crate::pdf::export_dialog::ExportDialog) {
        let Some(tex) = d.tex.as_mut() else { return };
        tex.save_requested = false;
        let Some(f) = &tex.failure else { return };
        match crate::pdf::tex::dialog::save_tex(f, Path::new(&crate::layout::shellexpand(&d.path))) {
            Ok(dir) => self.toast(format!("Saved main.tex and its files in {}", dir.display())),
            Err(e) => d.error = Some(e),
        }
    }

    pub fn close_tab(&mut self, i: usize) {
        if i < self.tabbar.tabs.len() {
            self.flush_all();
            self.undo.flush_all();
            let closed = self.tabbar.tabs.remove(i);
            // the sound of a note stops with its tab (MEDIA-007)
            if let View::Note(rel) = &closed.view {
                if !self.all_tabs().any(|t| t.view == closed.view) {
                    self.media.stop_for_note(rel);
                }
            }
            if self.tabbar.active >= self.tabbar.tabs.len() && self.tabbar.active > 0 {
                self.tabbar.active = self.tabbar.tabs.len() - 1;
            } else if i < self.tabbar.active {
                self.tabbar.active -= 1;
            }
            // the last tab of a pane takes the pane with it (NAV-035)
            if self.tabbar.tabs.is_empty() && self.panes.len() > 1 {
                self.close_pane(self.panes.focused);
            }
            self.remember_open_files();
            self.keyboard_after_close();
        }
    }

    /// Make the previous (-1) or next (1) tab the open one, wrapping around at the ends
    /// (NAV-018). `tabs` is the order the tab bar shows.
    pub fn step_tab(&mut self, delta: isize) {
        let n = self.tabbar.tabs.len() as isize;
        if n > 1 {
            self.tabbar.active = (self.tabbar.active as isize + delta).rem_euclid(n) as usize;
        }
    }

    /// Do the keys of the "Editor normal mode" scope apply now (EDIT-015)? Vim-style editing is
    /// on and either the editor of the open note has the keyboard in normal mode, or the tab
    /// shows a view without a text box (calendar, Feeds, tasks, graph, a note in reading view)
    /// and no field has the keyboard. Not in the mail view (Space and f are mail keys, MAIL-061,
    /// MAIL-110), not in a drawing (Space pans), not with a dialog open.
    pub(crate) fn normal_keys_active(&self, ctx: &egui::Context) -> bool {
        if !self.cfg.vim_mode || self.modal.is_some() {
            return false;
        }
        let Some(tab) = self.tabbar.tabs.get(self.tabbar.active) else { return false };
        match tab.view {
            View::Note(_) if !tab.reading => tab.editor.vim_normal_keys(ctx),
            View::Mail | View::Drawing(_) => false,
            _ => ctx.memory(|m| m.focused().is_none()),
        }
    }

    /// Swap the view of this tab with the one before it in its history, so that doing it again
    /// swaps back (NAV-019). A note gets the keyboard.
    pub fn swap_alternate(&mut self) {
        let Some(t) = self.tabbar.tabs.get_mut(self.tabbar.active) else { return };
        let Some(prev) = t.back.last_mut() else { return };
        std::mem::swap(&mut t.view, prev);
        if matches!(t.view, View::Note(_)) {
            self.focus_editor();
        }
        self.remember_open_files();
    }

    /// Where a tab dragged to the pointer at `x` would land: the gap index in `0..=tabs.len()`,
    /// kept inside the tab's own group, pinned tabs first (NAV-017). `mids` are the tabs' centres.
    pub(crate) fn tab_slot(&self, from: usize, x: f32, mids: &[f32]) -> usize {
        let pinned = self.tabbar.tabs.iter().take_while(|t| t.pinned).count();
        let (lo, hi) = if self.tabbar.tabs[from].pinned { (0, pinned) } else { (pinned, self.tabbar.tabs.len()) };
        mids.iter().filter(|m| **m < x).count().clamp(lo, hi)
    }

    /// Move the tab `from` into the gap `slot` (as `tab_slot` counts it); the open tab stays open.
    pub(crate) fn move_tab(&mut self, from: usize, slot: usize) {
        self.move_tab_to(from, if slot > from { slot - 1 } else { slot });
    }

    fn move_tab_to(&mut self, from: usize, to: usize) {
        if from >= self.tabbar.tabs.len() || to == from {
            return;
        }
        let active_id = self.tabbar.tabs.get(self.tabbar.active).map(|t| t.id);
        let t = self.tabbar.tabs.remove(from);
        self.tabbar.tabs.insert(to, t);
        if let Some(id) = active_id {
            self.tabbar.active = self.tabbar.tabs.iter().position(|t| t.id == id).unwrap_or(0);
        }
        self.remember_open_files();
    }

    /// Pin or unpin a tab: it moves to the end of the pinned tabs (NAV-016).
    pub(crate) fn set_tab_pinned(&mut self, i: usize, pinned: bool) {
        let Some(t) = self.tabbar.tabs.get_mut(i) else { return };
        if t.pinned == pinned {
            return;
        }
        t.pinned = pinned;
        let count = self.tabbar.tabs.iter().filter(|t| t.pinned).count();
        // the end of the pinned group (the unpinned tab is not counted: it is the first outside)
        self.move_tab_to(i, if pinned { count - 1 } else { count });
        self.remember_open_files();
    }

    pub fn go_back(&mut self) {
        if let Some(t) = self.tabbar.tabs.get_mut(self.tabbar.active) {
            if let Some(prev) = t.back.pop() {
                let cur = std::mem::replace(&mut t.view, prev);
                t.fwd.push(cur);
            }
        }
        self.remember_open_files();
    }

    pub fn go_forward(&mut self) {
        if let Some(t) = self.tabbar.tabs.get_mut(self.tabbar.active) {
            if let Some(next) = t.fwd.pop() {
                let cur = std::mem::replace(&mut t.view, next);
                t.back.push(cur);
            }
        }
        self.remember_open_files();
    }

    pub(crate) fn process_actions(&mut self) {
        let actions = std::mem::take(&mut self.actions);
        for a in actions {
            match a {
                Action::FocusEditor => self.focus_editor(),
                Action::Open { rel, new_tab } => self.open(&rel, new_tab),
                Action::OpenLink { target, heading, from, new_tab } => self.open_link(&target, heading, from, new_tab),
                Action::FixLink { note, target, to } => self.fix_link(&note, &target, &to),
                Action::NewNote { folder } => self.new_untitled(&folder),
                Action::NewDrawing { folder, embed_into } => self.new_drawing(&folder, embed_into),
                Action::ToggleTask { rel, offset } => self.toggle_task(&rel, offset),
                Action::GotoLine { rel, line } => self.goto_line(&rel, line),
                Action::GotoHeading { rel, line } => self.goto_heading(&rel, line),
                Action::GotoMatch { rel, line, col } => self.goto_match(&rel, line, col),
                Action::ShowView(v) => self.show_view(v, false),
                Action::ShowViewInNewTab(v) => self.show_view(v, true),
                Action::OpenCalendarDay(day) => self.open_calendar_day(day),
                Action::OpenPost(hit) => self.open_post_hit(&hit),
                Action::OpenMail(msg) => self.open_mail_hit(*msg),
                Action::CloseTab(i) => self.close_tab(i),
                Action::SplitTabRight { pane, tab } => {
                    self.focus_pane(pane);
                    self.open_tab_in_split_right(tab);
                }
                Action::Rename(rel) => {
                    let name = self.vault.as_ref().and_then(|v| v.file(&rel).map(|f| f.display_name())).unwrap_or_else(|| rel.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                    self.modal = Some(Modal::Rename { rel, name, error: None });
                }
                Action::Trash(rel) => self.modal = Some(Modal::ConfirmTrash { rel }),
                Action::RestoreTrashed => self.restore_trashed(),
                Action::NewFolder(parent) => self.modal = Some(Modal::NewFolder { parent, name: String::new() }),
                Action::OpenVault(p) => self.open_vault(&p),
                Action::Export(rel) => self.open_export_dialog(&rel),
                Action::ExportFolder(dir) => self.open_export_folder_dialog(&dir),
                Action::Rescan => self.rescan(),
                Action::Toast(m) => self.toast(m),
                Action::OpenUrl(url) => {
                    if let Err(e) = crate::opener::open_url(&url) {
                        self.toast(e);
                    }
                }
                Action::CreatePerson(name) => self.create_person(&name),
                Action::ConvertCanvas { rel, new_tab } => {
                    self.convert_canvas(&rel, new_tab);
                }
                Action::AskLiftHeadings(text) => self.modal = Some(Modal::LiftPaste { text }),
                Action::PasteText(text) => {
                    if let Some(tab) = self.tabbar.tabs.get_mut(self.tabbar.active) {
                        tab.editor.pending_paste = Some(text);
                    }
                }
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
        // Ctrl+Shift+B: the file list; all of them are in the key registry (KEY-001)
        let action = ui.input_mut(|i| self.keymap.poll(i, Scope::Global));
        let is = |k: KeyAction| action == Some(k);
        let (mail_view, quick, new_note, toggle, back, fwd) = (is(KeyAction::MailView), is(KeyAction::QuickSwitcher), is(KeyAction::NewNote), is(KeyAction::ToggleReading), is(KeyAction::Back), is(KeyAction::Forward));
        let (close, search, save, settings, files, to_sidebar, to_editor) =
            (is(KeyAction::CloseTab), is(KeyAction::Search), is(KeyAction::Save), is(KeyAction::Settings), is(KeyAction::ShowFiles), is(KeyAction::FocusSidebar), is(KeyAction::FocusEditor));
        if self.vault.is_none() {
            return;
        }
        if is(KeyAction::PrevTab) {
            self.step_tab(-1);
        } else if is(KeyAction::NextTab) {
            self.step_tab(1);
        }
        if is(KeyAction::SplitRight) {
            self.split_right();
        } else if is(KeyAction::NextPane) {
            self.step_pane(1);
        } else if is(KeyAction::PrevPane) {
            self.step_pane(-1);
        } else if is(KeyAction::ClosePane) {
            self.close_pane(self.panes.focused);
        }
        // the alternate view from every view, also mail and drawings (KEY-011)
        if is(KeyAction::AlternateView) && self.modal.is_none() {
            self.swap_alternate();
        }
        // the Alt keys of the ribbon (KEY-015): what a click on the item or the sidebar toggle does
        if self.modal.is_none() {
            if let Some(&item) = action.and_then(KeyAction::ribbon_slot).and_then(|n| self.side.layout.ribbon.get(n)) {
                self.ribbon_click(item);
            } else if is(KeyAction::SidebarLeft) {
                self.toggle_left_sidebar();
            } else if is(KeyAction::SidebarRight) {
                self.toggle_right_sidebar();
            }
        }
        // the keys of the open link preview come before everything the editor reads (EDIT-035)
        self.link_preview_keys(ui);
        // the Alt keys of the buttons of a note, with the keyboard in its text or not (KEY-014, KEY-015)
        if self.note_keys_active(ui.ctx()) {
            if let Some(a) = ui.input_mut(|i| self.keymap.poll(i, Scope::NoteView)) {
                self.note_key(ui.ctx(), a);
            }
        } else {
            self.keymap.cancel(Scope::NoteView);
        }
        // Space sequences of vim-style normal mode (EDIT-015); a half typed one is forgotten
        // when the keys are not read any more
        if self.normal_keys_active(ui.ctx()) {
            match ui.input_mut(|i| self.keymap.poll(i, Scope::EditorNormal)) {
                Some(KeyAction::NormalAlternate) => self.swap_alternate(),
                Some(KeyAction::NormalFindFile) => self.modal = Some(Modal::Picker(crate::picker::Picker::new(crate::picker::Source::FileNames))),
                Some(KeyAction::NormalSearchAll) => self.modal = Some(Modal::Picker(crate::picker::Picker::new(crate::picker::Source::Everything))),
                Some(KeyAction::NormalLinkPreview) => self.link_preview_key(ui.ctx()),
                _ => {}
            }
        } else {
            self.keymap.cancel(Scope::EditorNormal);
        }
        let in_mail = matches!(self.tabbar.tabs.get(self.tabbar.active).map(|t| &t.view), Some(View::Mail));
        let sidebar_keys = self.tree.sel.is_some() || self.search.sel.is_some();
        // the left sidebar has the keyboard: a marked row, or the search box
        let in_sidebar = sidebar_keys || ui.ctx().memory(|m| m.has_focus(egui::Id::unique("search-input")));
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
            self.toggle_reading_view();
        }
        if files {
            self.side.left = LeftPanel::Files;
            self.focus_sidebar(ui.ctx());
        } else if to_sidebar && !(in_mail && self.mail.step_pane(ui.ctx(), &self.cfg, -1, sidebar_keys)) {
            // from a pane right of the first one: into the pane to its left (NAV-011)
            if !in_sidebar && self.panes.focused > 0 {
                self.focus_pane_keyboard(self.panes.focused - 1);
            } else {
                self.focus_sidebar(ui.ctx());
            }
        }
        if to_editor {
            // from the left sidebar the first pane is next; from a pane the one to its right,
            // from the last one the keyboard stays (NAV-011)
            if in_sidebar {
                self.focus_pane(0);
            }
            let in_mail = matches!(self.tabbar.tabs.get(self.tabbar.active).map(|t| &t.view), Some(View::Mail));
            // in the mail view the keyboard goes to its panes (MAIL-063)
            if in_mail && self.mail.step_pane(ui.ctx(), &self.cfg, 1, sidebar_keys) {
                self.tree.sel = None;
                self.search.sel = None;
            } else if !in_sidebar && self.panes.focused + 1 < self.panes.len() {
                self.focus_pane_keyboard(self.panes.focused + 1);
            } else {
                self.focus_editor();
            }
        }
        if back || mouse_back {
            self.go_back();
        }
        if fwd || mouse_fwd {
            self.go_forward();
        }
        if close {
            let a = self.tabbar.active;
            self.close_tab(a);
        }
        if search {
            self.side.left = LeftPanel::Search;
            self.cfg.show_left = true;
            self.search.focus = true;
        }
        if save {
            self.flush_all();
            self.settings_file_flush(true);
            self.toast("Saved");
        }
        if settings {
            self.open_settings(crate::settings::last_tab(ui.ctx()));
        }
        if is(KeyAction::UserGuide) {
            self.open_user_guide();
        }
        if is(KeyAction::DataUsage) {
            self.open_data_usage();
        }
    }

    /// Open the Settings dialog on this tab.
    pub fn open_settings(&mut self, tab: SettingsTab) {
        let vault_path = self.cfg.vault.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        self.modal = Some(Modal::Settings { vault_path, tab, keys: Box::default() });
    }

    fn screenshot_tick(&mut self, ui: &Ui) {
        self.frames += 1;
        let Some(path) = self.shot.path.clone() else { return };
        ui.ctx().request_repaint();
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::RequestPaintWhileHidden);
        if self.shot.recording() {
            self.record_tick(ui, path);
            self.send_close_when_done(ui);
            return;
        }
        if self.frames >= 120 && self.script.steps.is_empty() && self.frames >= self.script.last_frame + 12 && !self.shot.requested {
            self.shot.requested = true;
            let done = self.shot.done.clone();
            ui.ctx().request_screenshot(move |img| {
                let [w, h] = img.size;
                let mut bytes = Vec::with_capacity(w * h * 4);
                for p in &img.pixels {
                    bytes.extend_from_slice(&p.to_srgba_unmultiplied());
                }
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let saved = screenshot::encode_png(w as u32, h as u32, &bytes).and_then(|png| crate::fsutil::write_atomic(&path, &png).map_err(|e| e.to_string()));
                if let Err(e) = saved {
                    eprintln!("could not save screenshot: {e}");
                }
                done.store(true, Ordering::SeqCst);
            });
        }
        if self.shot.done.load(Ordering::SeqCst) {
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
        use crate::script::{Button, Step};
        if self.script.steps.is_empty() || self.frames < 30 || !self.frames.is_multiple_of(6) {
            return;
        }
        // waiting for a text to be drawn (a view that fills in later) holds the whole script
        if let Some(Step::WaitFor(l, n)) = self.script.steps.front() {
            if crate::script::find_label(&self.script.texts, l, *n).is_none() {
                return;
            }
        }
        let Some(step) = self.script.steps.pop_front() else { return };
        self.script.last_frame = self.frames;
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
        let button = |pos, button, pressed| egui::Event::PointerButton { pos, button, pressed, modifiers: Modifiers::NONE };
        let left = egui::PointerButton::Primary;
        match &step {
            Step::Text(t) => raw.events.push(egui::Event::Text(t.clone())),
            Step::Key(k) => raw.events.extend(key_event(k)),
            Step::Move(t) => raw.events.push(egui::Event::PointerMoved(self.script.point(t))),
            Step::Down(t) => {
                let pos = self.script.point(t);
                raw.events.extend([egui::Event::PointerMoved(pos), button(pos, left, true)]);
            }
            Step::Up(t) => {
                let pos = self.script.point(t);
                raw.events.extend([egui::Event::PointerMoved(pos), button(pos, left, false)]);
            }
            Step::Click(t, b) => {
                let pos = self.script.point(t);
                let b = if *b == Button::Right { egui::PointerButton::Secondary } else { left };
                raw.events.extend([egui::Event::PointerMoved(pos), button(pos, b, true), button(pos, b, false)]);
            }
            Step::DoubleClick(t) => {
                let pos = self.script.point(t);
                raw.events.extend([egui::Event::PointerMoved(pos), button(pos, left, true), button(pos, left, false), button(pos, left, true), button(pos, left, false)]);
            }
            Step::Drag(from, to) => {
                // press, two moves on the way, release: one step each, so the window sees a drag
                let (a, b) = (self.script.point(from), self.script.point(to));
                let at = |f: f32| crate::script::Target::At(a.x + (b.x - a.x) * f, a.y + (b.y - a.y) * f);
                let steps = [Step::Down(at(0.0)), Step::Move(at(0.1)), Step::Move(at(0.5)), Step::Move(at(1.0)), Step::Up(at(1.0))];
                for s in steps.into_iter().rev() {
                    self.script.steps.push_front(s);
                }
            }
            Step::Scroll(dy) => {
                // the mouse wheel at the pointer, in points (positive: down)
                raw.events.push(egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, -dy), modifiers: Modifiers::NONE, phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Wheel });
            }
            Step::Wait(n) => {
                if *n > 1 {
                    self.script.steps.push_front(Step::Wait(n - 1));
                }
            }
            Step::WaitFor(..) => {}
            Step::Drop(at, paths) => match at {
                // files dropped at a place by another program: a file list at the pointer, the
                // way `wl_drop` delivers it (IMP-002, BUILD-007)
                #[cfg(target_os = "linux")]
                Some(t) => {
                    let pos = self.script.point(t);
                    raw.events.push(egui::Event::PointerMoved(pos));
                    let list: String = paths.iter().map(|p| format!("{}\r\n", crate::import::file_uri(p))).collect();
                    self.drag.fake_drops.push(crate::import::wl_drop::DropEvent::Drop { x: pos.x, y: pos.y, parts: vec![("text/uri-list".into(), list.into_bytes())], offered: vec!["text/uri-list".into()], report: vec![] });
                }
                _ => paths.iter().for_each(|p| raw.dropped_files.push(std::sync::Arc::new(ScriptDrop(p.clone())))),
            },
            Step::DropData(rest) => {
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
                    self.drag.fake_drops.push(crate::import::wl_drop::DropEvent::Drop { x: at.x, y: at.y, parts, offered, report: vec![] });
                }
                #[cfg(not(target_os = "linux"))]
                let _ = rest;
            }
            Step::Dump => {
                for (t, r) in &self.script.texts {
                    eprintln!("script: {:?} at {:.0},{:.0}", t.chars().take(60).collect::<String>(), r.center().x, r.center().y);
                }
            }
            Step::FocusEnd => {
                if let (Some(t), Some(p)) = (self.tabbar.tabs.get(self.tabbar.active), self.current_path()) {
                    let n = self.notes.get(&p).map(|d| d.text.chars().count()).unwrap_or(0);
                    let id = t.id;
                    if let Some(t) = self.tabbar.tabs.iter_mut().find(|t| t.id == id) {
                        t.editor.pending_cursor = Some(n);
                    }
                }
            }
        }
        let step = format!("{step:?}");
        if std::env::var_os("SHALE_DEBUG").is_some() {
            if let Some(d) = self.current_path().and_then(|p| self.drawings.get(&p)) {
                eprintln!("step {step:?}: drawing {}", d.debug_state());
            }
            if let (Some(t), Some(p)) = (self.tabbar.tabs.get(self.tabbar.active), self.current_path()) {
                let tail: String = self.notes.get(&p).map(|d| d.text.chars().rev().take(40).collect::<Vec<_>>().into_iter().rev().collect()).unwrap_or_default();
                eprintln!("step {step:?}: tabs={} active={} tab_id={} editor={:?} picker_visible={} {} tail={tail:?}", self.tabbar.tabs.len(), self.tabbar.active, t.id, t.editor.id, t.editor.picker_visible(), t.editor.debug_state());
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

    /// Passwords are typed in the calendar and mail account settings and in every other password
    /// field (`secrets::password_edit`): no key of theirs is recorded (MAIL-041, SEC-009).
    pub(crate) fn input_is_secret(&self) -> bool {
        matches!(self.modal, Some(Modal::Calendars(_)) | Some(Modal::MailAccounts(_))) | crate::secrets::secret_field_shown()
    }

    /// Append the keyboard-related events of this frame, plus the editor's picker state.
    pub(crate) fn log_input(&mut self, raw: &egui::RawInput) {
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
        let state = match (self.tabbar.tabs.get(self.tabbar.active), self.current_path()) {
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
        if std::env::var_os("SHALE_DEBUG").is_some() && !self.script.steps.is_empty() {
            eprintln!("focus: {:?} wants_keys={}", ctx.memory(|m| m.focused()), ctx.egui_wants_keyboard_input());
        }
        // a window without a window manager (the private X display of the pictures) never gets the
        // focus from the system; the picture shows the window as a person would see it, focused
        if self.shot.path.is_some() {
            raw.focused = true;
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
        // a valid text of the settings tab is saved now; what is left is rescued (CFG-015)
        self.settings_file_flush(true);
        self.undo.flush_all();
        self.rescue_unsaved();
        self.save_mail_draft_at_exit();
        self.remember_open_files();
        self.save_config();
    }
}

impl ShaleApp {
    /// One frame of the window (the body of `eframe::App::ui`; tests call it directly).
    pub(crate) fn run_frame(&mut self, ui: &mut Ui) {
        self.drag.drop_zones.clear();
        self.drag.calendar_areas.clear();
        self.drag.tree_targets.clear();
        self.screenshot_tick(ui);
        if self.blink_applied != self.cfg.cursor_blink {
            self.blink_applied = self.cfg.cursor_blink;
            let blink = self.cfg.cursor_blink && !self.shot.active();
            ui.ctx().all_styles_mut(|s| s.visuals.text_cursor.blink = blink);
        }
        self.keyboard_guard(ui.ctx());
        self.shortcuts(ui);
        self.autosave();
        self.state_tick();
        self.settings_file_tick(ui.ctx());
        if let Some(m) = self.fonts.sync(ui.ctx(), &self.cfg.fonts) {
            self.toast(m);
        }
        self.undo.tick(unix_now());
        for p in self.undo.take_problems() {
            self.toast(p);
        }
        if self.undo.has_pending() {
            ui.ctx().request_repaint_after(Duration::from_secs(crate::undo::DEBOUNCE));
        }
        // secrets the keyring refused to keep (SEC-011)
        for m in crate::secrets::take_notices() {
            self.toast(m);
        }
        let ctx = ui.ctx().clone();
        self.media_tick(&ctx);
        self.tex.poll(&ctx);
        self.take_handover(&ctx);
        self.sync_tick(&ctx);
        self.mail_tick(&ctx);
        if self.calendar.poll() {
            ui.ctx().request_repaint();
        }
        if self.ctx.is_none() {
            self.ctx = Some(ui.ctx().clone());
        }
        let mut finished = Vec::new();
        self.cal.jobs.retain(|(job, sid)| match crate::calendar::store::job_result(job) {
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
        if !self.cal.jobs.is_empty() {
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
        self.social_tick(ui.ctx());
        self.link_preview_ui(ui);
        self.modals(ui);
        self.handle_drops(ui);
        self.learn_tags_tick();
        self.usage_tick();
        self.paint_toasts(ui);
        self.paint_recovery_notice(ui);
        if !self.script.steps.is_empty() {
            self.script.note_texts(ui.ctx());
        }
        self.process_actions();
    }
}

/// After a rename or move that could not update the links in every note (VAULT-016): exactly
/// which notes were updated and which were not. `None` when all went well.
/// The clock the undo histories use: unix seconds.
fn unix_now() -> u64 {
    crate::calendar::time::now().max(0) as u64
}

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

/// The end of a rename / move message: which files got new links, which canvases did not.
fn links_message(u: &crate::vault::LinkUpdate, old: &str) -> String {
    let mut out = String::new();
    if !u.changed.is_empty() {
        out.push_str(&format!(" and updated links in {} file(s)", u.changed.len()));
    }
    if !u.canvases.is_empty() {
        let names: Vec<String> = u.canvases.iter().map(|c| format!("“{}”", rel_to_string(c))).collect();
        out.push_str(&format!(". Still linking to the old {old} (canvases are never changed): {}", names.join(", ")));
    }
    out
}

pub fn handle_editor_action(app_actions: &mut Vec<Action>, a: EditorAction, from: &Path) {
    match a {
        EditorAction::OpenLink { target, heading, new_tab } => app_actions.push(Action::OpenLink { target, heading, from: Some(from.to_path_buf()), new_tab }),
        EditorAction::OpenUrl(url) => app_actions.push(Action::OpenUrl(url)),
        EditorAction::FilesChanged => app_actions.push(Action::Rescan),
        EditorAction::Pasted(msg) => {
            app_actions.push(Action::Rescan);
            app_actions.push(Action::Toast(msg));
        }
        EditorAction::CreatePerson(name) => app_actions.push(Action::CreatePerson(name)),
        EditorAction::AskLiftHeadings(text) => app_actions.push(Action::AskLiftHeadings(text)),
    }
}

pub fn handle_reader_action(app_actions: &mut Vec<Action>, a: ReaderAction, from: &Path) {
    match a {
        ReaderAction::OpenLink { target, heading, new_tab } => app_actions.push(Action::OpenLink { target, heading, from: Some(from.to_path_buf()), new_tab }),
        ReaderAction::ToggleTask(offset) => app_actions.push(Action::ToggleTask { rel: from.to_path_buf(), offset }),
        ReaderAction::OpenFile(rel) => app_actions.push(Action::Open { rel, new_tab: false }),
        ReaderAction::OpenUrl(url) => app_actions.push(Action::OpenUrl(url)),
        ReaderAction::OpenDay(day) => app_actions.push(Action::OpenCalendarDay(day)),
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
pub(crate) mod tests {
    use super::*;

    pub(crate) fn test_app(tag: &str) -> (ShaleApp, PathBuf) {
        let d = std::env::temp_dir().join(format!("shale-app-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        vault::write_sample_vault(&d).unwrap();
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(d.clone()), view: None, keys: None }, false);
        // most tests name the notes they make; those of the prefix switch it on
        app.cfg.date_prefix_new_notes = false;
        (app, d)
    }

    // spec: USE-001, USE-006, USE-012, USE-015
    #[test]
    fn the_data_usage_dialog_opens_counts_and_clears_and_the_mail_index_waits_for_the_store() {
        use crate::data_usage::{dialog::Ask, CacheId};
        let (mut app, d) = test_app("usage");
        let ctx = egui::Context::default();
        app.ctx = Some(ctx.clone());
        assert!(!app.usage.places.pictures.starts_with(dirs::cache_dir().unwrap_or_default()) || app.usage.places.pictures.starts_with(std::env::temp_dir()), "a test never sees the real caches");
        assert_eq!(app.usage.places.post_index, None);
        std::fs::create_dir_all(d.join(".shale")).unwrap();
        crate::fsutil::write_atomic(&crate::import::tagwords::file_in(&d), b"{}").unwrap();
        app.open_data_usage();
        assert!(matches!(app.modal, Some(Modal::DataUsage)));
        let mut n = 0;
        while (app.usage.saved.is_none() || app.usage.stats.len() < CacheId::ALL.len()) && n < 300 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| app.run_frame(ui));
            out.textures_delta.clear();
            std::thread::sleep(Duration::from_millis(10));
            n += 1;
        }
        let saved = app.usage.saved.clone().unwrap().unwrap();
        assert!(saved.vault.iter().any(|l| l.label == "Notes" && l.files > 0), "{:?}", saved.vault);
        assert_eq!(app.usage.stats[&CacheId::TagWords].as_ref().unwrap().entries, 1);
        // the mail store is not open: the rebuild is refused with the reason and nothing runs
        app.usage_ask(Ask::Clear(CacheId::MailIndex));
        assert!(!app.usage.running(CacheId::MailIndex));
        assert!(app.toasts.iter().any(|(t, _)| t.contains("not now")), "{:?}", app.toasts);
        assert!(app.usage.record.caches[&CacheId::MailIndex].error.is_some());
        // tag words: cleared, and learned again at once
        app.tag_learner.generation = 0;
        app.usage_ask(Ask::Clear(CacheId::TagWords));
        let mut n = 0;
        while app.usage.running(CacheId::TagWords) && n < 300 {
            app.usage_tick();
            std::thread::sleep(Duration::from_millis(10));
            n += 1;
        }
        assert!(!crate::import::tagwords::file_in(&d).exists() || app.tag_learner.running());
        assert_eq!(app.tag_learner.generation, u64::MAX, "learning starts again");
        assert!(app.toasts.iter().any(|(t, _)| t.starts_with("Tag words cleared")), "{:?}", app.toasts);
        // undo histories (USE-017): counted, and a clear empties the files and the loaded histories
        app.undo.open_vault(&d, unix_now());
        app.undo.of(Path::new("Ideas.md")).checkpoint("one");
        app.undo.of(Path::new("Ideas.md")).checkpoint("one two");
        app.undo.flush_all();
        app.usage.refresh(|| {});
        let mut n = 0;
        while app.usage.any_running() && n < 300 {
            app.usage_tick();
            std::thread::sleep(Duration::from_millis(10));
            n += 1;
        }
        assert_eq!(app.usage.stats[&CacheId::UndoHistories].as_ref().unwrap().entries, 1);
        app.usage_ask(Ask::Clear(CacheId::UndoHistories));
        let mut n = 0;
        while app.usage.running(CacheId::UndoHistories) && n < 300 {
            app.usage_tick();
            std::thread::sleep(Duration::from_millis(10));
            n += 1;
        }
        assert!(app.toasts.iter().any(|(t, _)| t.starts_with("Undo histories cleared")), "{:?}", app.toasts);
        assert_eq!(crate::undo::stats_of_files(&d).entries, 0, "the files are deleted");
        let mut text = "one two".to_string();
        assert_eq!(app.undo.of(Path::new("Ideas.md")).undo(&mut text), None, "u has nothing to undo after a clear");
        assert!(app.usage_ask(Ask::Open(PathBuf::from("Ideas.md"))), "opening a file closes the dialog");
        assert_eq!(app.current_path(), Some(PathBuf::from("Ideas.md")));
        app.usage_ask(Ask::ShowFolder(PathBuf::from("Projects")));
        assert!(app.tree.expanded.contains(Path::new("Projects")));
        let _ = std::fs::remove_dir_all(&d);
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

    // spec: LINK-015
    #[test]
    fn a_heading_link_puts_the_cursor_on_the_heading_in_the_editor_and_scrolls_the_reading_view_in_the_reading_view() {
        let (mut app, d) = test_app("heading-link");
        std::fs::write(d.join("Roadmap.md"), "# Roadmap\nintro\n## **Plan** v2\ntext\n## Review\n").unwrap();
        app.vault.as_mut().unwrap().rescan();
        let from = Some(PathBuf::from("Welcome.md"));
        let tab = |app: &ShaleApp| app.tabbar.active;
        fn active(app: &mut ShaleApp) -> &mut Tab {
            let i = app.tabbar.active;
            &mut app.tabbar.tabs[i]
        }
        // editor: cursor at the start of the line, shown at the top
        app.open_link("Roadmap", Some("plan V2".into()), from.clone(), false);
        assert_eq!(app.current_path(), Some(PathBuf::from("Roadmap.md")));
        let t = &app.tabbar.tabs[tab(&app)];
        assert!(!t.reading);
        assert_eq!(t.editor.pending_cursor, Some("# Roadmap\nintro\n".chars().count()), "the heading line, found ignoring case and markup");
        assert!(t.editor.scroll_to_top);
        // reading view: the tab stays there and the view is told where to scroll
        active(&mut app).reading = true;
        active(&mut app).reader_line = None;
        app.open_link("Roadmap", Some(" Review ".into()), from.clone(), false);
        let t = &app.tabbar.tabs[tab(&app)];
        assert!(t.reading, "the tab keeps its mode");
        assert_eq!(t.reader_line, Some(4));
        // [[#Heading]] is the open note
        app.open_link("", Some("Plan v2".into()), Some(PathBuf::from("Roadmap.md")), false);
        assert_eq!(app.tabbar.tabs[tab(&app)].reader_line, Some(2));
        assert_eq!(app.current_path(), Some(PathBuf::from("Roadmap.md")));
        // a heading that is not there: the note opens at the top and says so
        active(&mut app).reader_line = None;
        app.open_link("Roadmap", Some("Nope".into()), from.clone(), false);
        assert!(app.toasts.iter().any(|(t, _)| t == "No heading “Nope” in Roadmap"), "{:?}", app.toasts);
        assert_eq!(app.tabbar.tabs[tab(&app)].reader_line, None);
        assert_eq!(app.current_path(), Some(PathBuf::from("Roadmap.md")));
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
        // the vault sits in the shared temp folder, where another program may have made an `x`:
        // check that it is not created or changed, not that it is absent
        let outside = d.parent().unwrap().join("x");
        let before = std::fs::metadata(&outside).ok().map(|m| (m.is_dir(), m.modified().ok()));
        for bad in ["../x", "/../../x", "a/../../x", "~/x"] {
            assert!(app.create_folder(Path::new("Projects"), bad).is_err(), "{bad} must be refused");
        }
        assert!(!d.join("x").exists());
        assert_eq!(std::fs::metadata(&outside).ok().map(|m| (m.is_dir(), m.modified().ok())), before, "nothing outside the vault is touched");
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

    // spec: CFG-043, VAULT-025
    #[test]
    fn a_new_note_gets_todays_date_prefix_when_the_setting_is_on_and_shows_without_it() {
        let (mut app, d) = test_app("prefix");
        let today = vault::date_prefix_today();
        assert!(today.len() == 7 && vault::date_prefix_of(&today).is_some(), "{today}");
        app.cfg.date_prefix_new_notes = true;
        // a new note, in a folder, twice (the second one gets the number)
        let rel = app.create_note(Path::new("Projects"), "Plan", false).unwrap();
        assert_eq!(rel, PathBuf::from(format!("Projects/{today}Plan.md")));
        let second = app.create_note(Path::new("Projects"), "Plan", false).unwrap();
        assert_eq!(second, PathBuf::from(format!("Projects/{today}Plan 1.md")));
        // the title line and the tab show the name without the prefix; the file list and links keep the full name
        let f = app.vault.as_ref().unwrap().file(&second).unwrap().clone();
        assert_eq!((f.title().as_str(), f.display_name()), ("Plan 1", format!("{today}Plan 1")));
        assert_eq!(app.tab_title(&View::Note(second)), "Plan 1");
        app.new_untitled(Path::new(""));
        assert_eq!(app.current_path(), Some(PathBuf::from(format!("{today}Untitled.md"))));
        // a name that has a prefix already, or folders in the name
        assert_eq!(app.create_note(Path::new(""), "261117_Kept", false).unwrap(), PathBuf::from("261117_Kept.md"));
        assert_eq!(app.create_note(Path::new(""), "Sub/Deep", false).unwrap(), PathBuf::from(format!("Sub/{today}Deep.md")));
        // a drawing
        app.new_drawing(Path::new(""), None);
        assert!(d.join(format!("{today}Drawing.excalidraw")).exists());
        // off: the name as given
        app.cfg.date_prefix_new_notes = false;
        assert_eq!(app.create_note(Path::new(""), "Plain", false).unwrap(), PathBuf::from("Plain.md"));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: CFG-043, LINK-005
    #[test]
    fn a_red_link_that_makes_a_prefixed_note_is_rewritten_so_it_resolves() {
        let (mut app, d) = test_app("relink");
        app.cfg.date_prefix_new_notes = true;
        let today = vault::date_prefix_today();
        std::fs::write(d.join("Source.md"), "a [[Plan]] b [[Plan#Goals]] c [t](Plan.md) d [[Other]] e [[Plan|kept]]\n").unwrap();
        app.rescan();
        app.open(Path::new("Source.md"), false);
        app.open_link("Plan", None, Some(PathBuf::from("Source.md")), false);
        assert_eq!(app.current_path(), Some(PathBuf::from(format!("{today}Plan.md"))));
        let want = format!("a [[{today}Plan|Plan]] b [[{today}Plan#Goals|Plan > Goals]] c [t]({today}Plan.md) d [[Other]] e [[{today}Plan|kept]]\n");
        assert_eq!(app.notes[Path::new("Source.md")].text, want);
        let v = app.vault.as_ref().unwrap();
        assert!(v.exists(&format!("{today}Plan"), None), "the link reaches the note");
        app.flush_all();
        assert_eq!(std::fs::read_to_string(d.join("Source.md")).unwrap(), want, "saved");
        assert!(app.vault.as_ref().unwrap().backlinks(Path::new(&format!("{today}Plan.md"))).iter().any(|(p, _)| p == Path::new("Source.md")), "a backlink");
        // a relative link (LINK-010) is made where it points, with the prefix, and rewritten alike
        std::fs::write(d.join("Projects/Rel.md"), "x [[./Sub]] y\n").unwrap();
        app.rescan();
        app.open(Path::new("Projects/Rel.md"), false);
        app.open_link("./Sub", None, Some(PathBuf::from("Projects/Rel.md")), false);
        assert!(d.join(format!("Projects/{today}Sub.md")).exists());
        assert_eq!(app.notes[Path::new("Projects/Rel.md")].text, format!("x [[./{today}Sub|./Sub]] y\n"));
        assert!(app.vault.as_ref().unwrap().exists(&format!("./{today}Sub"), Some(Path::new("Projects/Rel.md"))));
        // off: the link is left alone
        app.cfg.date_prefix_new_notes = false;
        std::fs::write(d.join("Source.md"), "[[Later]]\n").unwrap();
        app.rescan();
        app.open(Path::new("Source.md"), false);
        app.open_link("Later", None, Some(PathBuf::from("Source.md")), false);
        assert!(d.join("Later.md").exists());
        assert_eq!(app.notes[Path::new("Source.md")].text, "[[Later]]\n");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: SEC-009
    #[test]
    fn nothing_typed_or_pasted_into_a_password_field_is_recorded() {
        use eframe::egui::{Event as Ev, Modifiers};
        let (mut app, d) = test_app("keyrec");
        let log = d.join("input-debug.log");
        crate::config::INPUT_LOG_TEST_PATH.with(|c| *c.borrow_mut() = Some(log.clone()));
        app.cfg.input_log = true;
        let typed = |s: &str| egui::RawInput {
            events: vec![Ev::Key { key: egui::Key::H, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }, Ev::Text(s.into()), Ev::Paste(s.into()), Ev::Ime(egui::ImeEvent::Commit(s.into()))],
            ..Default::default()
        };
        let read = || std::fs::read_to_string(&log).unwrap_or_default();
        // a normal field is recorded (so the checks below are not vacuous)
        app.log_input(&typed("visible-text"));
        assert!(read().contains("visible-text"));
        // while a password field is on screen, nothing of it is
        let ctx = egui::Context::default();
        let mut pw = crate::secret::Secret::default();
        let mut show = || {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    crate::secrets::password_edit(ui, &mut pw, "", 200.0);
                });
            });
            // no renderer here: drop the font texture upload on purpose (debug builds assert)
            out.textures_delta.clear();
        };
        show();
        app.log_input(&typed("hunter2-secret"));
        show();
        app.log_input(&typed("pasted-secret"));
        assert!(!read().contains("secret"), "{}", read());
        assert!(!read().contains("Key H"), "not even key names: {}", read());
        // a few frames after the field is gone, recording goes on
        for _ in 0..4 {
            app.log_input(&egui::RawInput::default());
        }
        app.log_input(&typed("visible-again"));
        assert!(read().contains("visible-again"));
        crate::config::INPUT_LOG_TEST_PATH.with(|c| *c.borrow_mut() = None);
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

    // spec: SOC-007, SOC-012
    #[test]
    fn saving_a_post_opens_the_import_dialog_and_the_note_lands_in_the_vault() {
        use crate::social::accounts_ui::DialogAction;
        let (mut app, d) = test_app("social");
        let entry = crate::social::demo::account();
        assert!(!app.social_dialog_action(DialogAction::Added(entry.clone())));
        assert_eq!(app.cfg.social_accounts.len(), 1);
        app.social.load_demo(&entry);
        let ctx = egui::Context::default();
        let post = crate::social::demo::posts().remove(4);
        let wait_for_dialog = |app: &mut ShaleApp| {
            for _ in 0..300 {
                app.social_tick(&ctx);
                if matches!(app.modal, Some(Modal::Import(_))) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("the Import dialog did not open");
        };
        app.social.save(&ctx, &entry, "home", &post);
        wait_for_dialog(&mut app);
        let Some(Modal::Import(mut dlg)) = app.modal.take() else { panic!() };
        assert_eq!((dlg.items.len(), dlg.folder.as_str(), dlg.items[0].kind().as_ref()), (1, "Social", "Thread (3 posts)"));
        assert!(dlg.items[0].include && dlg.items[0].already.is_none() && dlg.link_into.is_none());
        assert!(app.import_action(&mut dlg, crate::import::dialog::ImportAction::Import));
        assert_eq!(app.vdata.state.last_social_folder, "Social");
        assert!(app.vdata.state.last_import_folder.is_empty(), "posts have their own last folder");
        let rel = d.join("Social").read_dir().unwrap().flatten().find(|f| f.file_name().to_string_lossy().ends_with(".md")).unwrap().path();
        let text = std::fs::read_to_string(&rel).unwrap();
        assert!(text.contains("## Thread") && text.contains("\nurl: ") && !text.contains("**Source:**") && text.contains("![[z_media/"), "{text}");
        assert!(app.actions.iter().any(|a| matches!(a, Action::Open { .. })), "the new note is opened");
        // the same post again: offered, but marked as imported before
        app.rescan();
        app.social.save(&ctx, &entry, "home", &post);
        wait_for_dialog(&mut app);
        let Some(Modal::Import(dlg)) = app.modal.take() else { panic!() };
        assert!(dlg.items[0].already.is_some() && !dlg.items[0].include);
        // removing the account forgets it
        app.social_dialog_action(DialogAction::Remove(entry.account.id.clone()));
        assert!(app.cfg.social_accounts.is_empty() && !app.social.feeds.contains_key(&entry.account.id));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: SOC-043
    #[test]
    fn signing_in_again_replaces_only_the_secret_of_the_account() {
        use crate::social::accounts_ui::DialogAction;
        let (mut app, d) = test_app("social-resign");
        let other = crate::social::SocialAccountEntry { account: crate::social::Account::new(crate::social::Network::Bluesky, "https://bsky.soc043.test", "bob.test", "Bob"), secret: Default::default() };
        let mut entry = crate::social::SocialAccountEntry::new(crate::social::demo::account().account, &crate::secret::Secret::new("old-token"));
        entry.account.server = "https://demo.soc043.test".into();
        app.social_dialog_action(DialogAction::Added(entry.clone()));
        app.social_dialog_action(DialogAction::Added(other.clone()));
        app.social.load_demo(&entry);
        let posts = app.social.feeds.len();
        assert!(posts > 0);
        // what the dialog's job did: the new secret went to the keyring and the entry is the account as it was
        let new = crate::social::SocialAccountEntry::new(entry.account.clone(), &crate::secret::Secret::new("new-token"));
        assert!(!app.social_dialog_action(DialogAction::Updated(new)));
        assert_eq!(app.cfg.social_accounts.iter().map(|e| e.account.id.as_str()).collect::<Vec<_>>(), [entry.account.id.as_str(), other.account.id.as_str()], "same place, same id");
        assert_eq!(app.cfg.social_accounts[0].account, entry.account, "name and server stay");
        assert_eq!(app.cfg.social_accounts[0].secret().expose(), "new-token");
        assert!(app.social.feeds.len() == posts && app.social.feeds.contains_key(&entry.account.id), "its feeds stay");
        assert!(app.toasts.iter().any(|t| t.0.contains("Signed in again")), "it says so");
        // the account was removed while the form ran: the secret that the sign-in stored goes again
        app.social_dialog_action(DialogAction::Remove(entry.account.id.clone()));
        let late = crate::social::SocialAccountEntry::new(entry.account.clone(), &crate::secret::Secret::new("late-token"));
        app.social_dialog_action(DialogAction::Updated(late));
        assert!(app.cfg.social_accounts.iter().all(|e| e.account.id != entry.account.id), "not added again");
        assert_eq!(crate::secrets::social_lookup(&entry.account.server, &entry.account.handle), None);
        crate::secrets::social_clear(&other.account.server, &other.account.handle);
        let _ = std::fs::remove_dir_all(&d);
    }

    pub(crate) const CANVAS: &str =
        r#"{"nodes":[{"id":"a","type":"text","text":"devices","x":0,"y":0,"width":250,"height":60},{"id":"b","type":"file","file":"Ideas.md","x":0,"y":200,"width":250,"height":60}],"edges":[{"id":"e","fromNode":"a","toNode":"b"}]}"#;

    /// The sample vault with `Maps/Map.canvas`.
    pub(crate) fn app_with_canvas(tag: &str) -> (ShaleApp, PathBuf) {
        let (mut app, d) = test_app(tag);
        std::fs::create_dir_all(d.join("Maps")).unwrap();
        std::fs::write(d.join("Maps/Map.canvas"), CANVAS).unwrap();
        app.rescan();
        (app, d)
    }

    // spec: DRAW-011, DRAW-012, DRAW-013
    #[test]
    fn opening_a_canvas_asks_converts_and_never_changes_it() {
        let (mut app, d) = app_with_canvas("canvas");
        let canvas = Path::new("Maps/Map.canvas");
        assert_eq!(app.vault.as_ref().unwrap().file(canvas).map(|f| f.kind), Some(FileKind::Drawing));
        // the question comes first; nothing is opened yet
        app.open(canvas, false);
        assert!(matches!(&app.modal, Some(Modal::ConvertCanvas { existing: None, always: false, .. })));
        assert_ne!(app.current_path().as_deref(), Some(canvas));
        // read-only
        app.modal = None;
        app.open_canvas_read_only(canvas, false);
        assert_eq!(app.current_path().as_deref(), Some(canvas));
        assert!(app.drawings[canvas].read_only.is_some());
        app.flush_all();
        // convert: a plugin drawing next to it, shown in the canvas's tab; the canvas is unchanged
        let to = app.convert_canvas(canvas, false).unwrap();
        assert_eq!(to, Path::new("Maps/Map.excalidraw.md"));
        assert_eq!(app.current_path(), Some(to.clone()));
        assert_eq!(app.tabbar.tabs.iter().filter(|t| t.view.path() == Some(canvas)).count(), 0, "the canvas tab now shows the drawing");
        let made = std::fs::read_to_string(d.join(&to)).unwrap();
        assert!(crate::drawing::canvas::is_converted_from(&made, canvas));
        assert!(app.drawings[&to].read_only.is_none() && app.drawings[&to].error.is_none());
        assert_eq!(std::fs::read_to_string(d.join(canvas)).unwrap(), CANVAS);
        // opening the canvas again offers the copy made before
        app.drawings.remove(canvas);
        app.open(canvas, false);
        assert!(matches!(&app.modal, Some(Modal::ConvertCanvas { existing: Some(e), .. }) if *e == to));
        // with the setting on: no question, the copy opens, no second copy
        app.modal = None;
        app.cfg.auto_convert_canvas = true;
        app.open(Path::new("Welcome.md"), false);
        app.open(canvas, false);
        assert!(app.modal.is_none());
        assert_eq!(app.current_path(), Some(to.clone()));
        assert!(!d.join("Maps/Map 1.excalidraw.md").exists());
        // …and without a copy it converts right away
        app.drawings.remove(&to);
        std::fs::remove_file(d.join(&to)).unwrap();
        app.rescan();
        app.open(canvas, false);
        assert!(app.modal.is_none() && d.join(&to).exists());
        assert_eq!(std::fs::read_to_string(d.join(canvas)).unwrap(), CANVAS);
        // renaming keeps it a canvas
        app.rename(canvas, "Network").unwrap();
        assert_eq!(std::fs::read_to_string(d.join("Maps/Network.canvas")).unwrap(), CANVAS);
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: DRAW-017
    #[test]
    fn rename_updates_an_open_drawing_and_names_the_canvas_left_behind() {
        let (mut app, d) = test_app("rename-drawing");
        let scene = crate::drawing::SAMPLE_SCENE.replace("\"Idea\"", "\"see [[Ideas]]\"");
        std::fs::write(d.join("Map.excalidraw"), scene).unwrap();
        app.rescan();
        app.open(Path::new("Map.excalidraw"), false);
        app.rename(Path::new("Ideas.md"), "Thoughts").unwrap();
        let shown: Vec<String> = app.drawings[Path::new("Map.excalidraw")].scene.live().filter_map(|e| e.text.clone()).collect();
        assert!(shown.contains(&"see [[Thoughts]]".to_string()), "the open drawing shows the new link: {shown:?}");
        let msg = &app.toasts.last().unwrap().0;
        assert!(msg.contains("updated links in") && msg.contains("“Overview.canvas”") && msg.contains("canvases are never changed"), "{msg}");
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
            assert!(dialog.path.replace('\\', "/").ends_with(&format!("exports/{}.pdf", rel.split('.').next().unwrap())), "default path {}", dialog.path);
            let out = app.run_export(&dialog).unwrap();
            let bytes = std::fs::read(&out).unwrap();
            let pdf = hayro::hayro_syntax::Pdf::new(bytes).expect("a valid PDF was written");
            assert!(!pdf.pages().is_empty());
            // the export shows up in the vault and can be embedded in a note
            assert!(app.vault.as_ref().unwrap().exists(&format!("exports/{}.pdf", rel.split('.').next().unwrap()), None));
            let _ = std::fs::remove_dir_all(&d);
        }
    }

    // spec: PDF-005
    #[test]
    fn the_export_takes_the_text_column_of_the_editor_that_shows_the_note() {
        let (mut app, d) = test_app("column");
        image::RgbaImage::from_pixel(400, 200, image::Rgba([10, 120, 10, 255])).save(d.join("wide.png")).unwrap();
        let note = d.join("Welcome.md");
        let text = std::fs::read_to_string(&note).unwrap();
        std::fs::write(&note, format!("{text}\n![[wide.png|300]]\n")).unwrap();
        app.vault.as_mut().unwrap().rescan();
        app.open(Path::new("Welcome.md"), false);
        let page_w = |app: &mut ShaleApp, column: Option<f32>| {
            let a = app.tabbar.active;
            app.tabbar.tabs[a].editor.column = column;
            app.open_export_dialog(Path::new("Welcome.md"));
            let Some(Modal::Export(dialog)) = app.modal.take() else { panic!("the export dialog opens") };
            let out = app.run_export(&dialog).unwrap();
            crate::pdf::export::tests::image_sizes(&std::fs::read(out).unwrap())[0].0
        };
        let (a, b) = (page_w(&mut app, Some(600.0)), page_w(&mut app, Some(1200.0)));
        assert!((a / b - 2.0).abs() < 0.05, "half a 600 column is twice a quarter of a 1200 one: {a} {b}");
        let none = page_w(&mut app, None);
        assert!((none / a - 600.0 / 720.0).abs() < 0.05, "no column known: 720 is used: {none} {a}");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: PDF-013, PDF-019
    #[test]
    fn the_export_dialog_opens_with_the_print_properties_of_the_note_and_what_it_holds_is_exported() {
        let (mut app, d) = test_app("print-props");
        let note = d.join("Printed.md");
        std::fs::write(&note, "---\nprint-paper: Letter\nprint-margins: 30 10 25 15\nprint-font-size: 12\nprint-title: false\nprint-toc: false\n---\n# Head\n\nBody text.\n").unwrap();
        app.vault.as_mut().unwrap().rescan();
        app.open_export_dialog(Path::new("Printed.md"));
        let Some(Modal::Export(mut dialog)) = app.modal.take() else { panic!("the export dialog opens") };
        let o = &dialog.opts;
        assert_eq!((o.paper, o.margins_mm, o.font_pt, o.title, o.toc), (crate::pdf::export::Paper::Letter, [30.0, 10.0, 25.0, 15.0], 12.0, false, false));
        // a note without properties gets the usual dialog
        app.open_export_dialog(Path::new("Ideas.md"));
        let Some(Modal::Export(plain)) = app.modal.take() else { panic!() };
        assert_eq!((plain.opts.paper, plain.opts.margins_mm, plain.opts.title, plain.opts.toc), (crate::pdf::export::Paper::A4, [20.0; 4], true, true));
        // what the dialog holds is what is exported: A4 chosen in it wins over the note's Letter
        dialog.opts.paper = crate::pdf::export::Paper::A4;
        let out = app.run_export(&dialog).unwrap();
        let bytes = std::fs::read(out).unwrap();
        assert!(String::from_utf8_lossy(&bytes).contains("/MediaBox [0 0 595.00 842.00]"));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: PDF-001
    #[test]
    fn export_reports_a_bad_path() {
        let (mut app, d) = test_app("badpath");
        app.open_export_dialog(Path::new("Ideas.md"));
        let Some(Modal::Export(mut dialog)) = app.modal.take() else { panic!() };
        // a file where a folder is needed: cannot be written on any system
        dialog.path = d.join("Ideas.md").join("x.pdf").to_string_lossy().into_owned();
        assert!(app.run_export(&dialog).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    fn last_toast(app: &ShaleApp) -> String {
        app.toasts.last().map(|(t, _)| t.clone()).unwrap_or_default()
    }

    // spec: VAULT-027
    #[test]
    fn a_new_note_with_a_character_windows_forbids_is_saved_under_a_portable_name_and_the_toast_says_so() {
        let (mut app, d) = test_app("portable-new-note");
        app.cfg.date_prefix_new_notes = false;
        let rel = app.create_note(Path::new(""), "a:b", false).unwrap();
        assert_eq!(rel, PathBuf::from("a-b.md"));
        assert!(d.join("a-b.md").exists());
        assert_eq!(last_toast(&app), "Saved as `a-b.md`: `:` is not allowed on Windows");
        // nothing to tell for a good name
        app.create_note(Path::new(""), "Plain", false).unwrap();
        assert_eq!(last_toast(&app), "Created “Plain”");
        // a device name, a trailing dot, a long name
        let rel = app.create_note(Path::new(""), "aux", false).unwrap();
        assert_eq!(rel, PathBuf::from("aux_.md"));
        assert_eq!(last_toast(&app), "Saved as `aux_.md`: `aux` is a reserved name on Windows");
        // in the temp folder its path is longer than 260 characters on Windows (DATA-013)
        let rel = app.create_note(Path::new(""), &"x".repeat(250), false).unwrap();
        assert_eq!(rel.file_name().unwrap().len(), 200);
        assert!(last_toast(&app).ends_with("the name is longer than 200 bytes"), "{}", last_toast(&app));
        // with the date prefix the prefix is part of the 200 bytes
        app.cfg.date_prefix_new_notes = true;
        let rel = app.create_note(Path::new(""), "p?q", false).unwrap();
        assert_eq!(rel, PathBuf::from(format!("{}p-q.md", vault::date_prefix_today())));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: VAULT-027
    #[test]
    fn a_red_link_with_a_forbidden_character_is_rewritten_to_the_name_its_note_really_got() {
        let (mut app, d) = test_app("portable-red-link");
        app.cfg.date_prefix_new_notes = false;
        std::fs::write(d.join("Source.md"), "see [[Plan: v2]] and [x](Plan: v2.md) and [[Other]]\n").unwrap();
        app.rescan();
        app.open(Path::new("Source.md"), false);
        app.open_link("Plan: v2", None, Some(PathBuf::from("Source.md")), false);
        assert!(d.join("Plan- v2.md").exists());
        let text = app.notes[Path::new("Source.md")].text.clone();
        assert_eq!(text, "see [[Plan- v2|Plan: v2]] and [x](Plan-%20v2.md) and [[Other]]\n");
        assert!(app.vault.as_ref().unwrap().exists("Plan- v2", None), "the link reaches the note");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: VAULT-027
    #[test]
    fn renaming_to_a_name_with_a_forbidden_character_uses_a_portable_one_tells_it_and_keeps_links() {
        let (mut app, d) = test_app("portable-rename");
        app.cfg.date_prefix_new_notes = false;
        std::fs::write(d.join("Linker.md"), "to [[Mine]]\n").unwrap();
        std::fs::write(d.join("Mine.md"), "mine\n").unwrap();
        app.rescan();
        app.rename(Path::new("Mine.md"), "Ideas: 2026?").unwrap();
        assert!(d.join("Ideas- 2026-.md").exists() && !d.join("Mine.md").exists());
        assert_eq!(last_toast(&app), "Renamed to `Ideas- 2026-.md`: `:` `?` are not allowed on Windows and updated links in 1 file(s)");
        assert_eq!(std::fs::read_to_string(d.join("Linker.md")).unwrap(), "to [[Ideas- 2026-]]\n", "the link has the real name");
        // a folder, and a plain rename says nothing about portability
        app.rename(Path::new("Projects"), "Work*").unwrap();
        assert!(d.join("Work-").is_dir());
        assert!(last_toast(&app).starts_with("Renamed to `Work-`: `*` is not allowed"), "{}", last_toast(&app));
        app.rename(Path::new("Work-"), "Work").unwrap();
        assert!(last_toast(&app).starts_with("Renamed to “Work”"), "{}", last_toast(&app));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: VAULT-016, DATA-013
    #[test]
    fn a_rename_that_only_changes_letter_case_is_done_also_where_letter_case_is_ignored() {
        let (mut app, d) = test_app("case-rename");
        std::fs::write(d.join("Linker.md"), "to [[mine]]\n").unwrap();
        std::fs::write(d.join("mine.md"), "mine\n").unwrap();
        std::fs::create_dir(d.join("work")).unwrap();
        app.rescan();
        app.rename(Path::new("mine.md"), "Mine").unwrap();
        assert!(last_toast(&app).starts_with("Renamed to “Mine”"), "{}", last_toast(&app));
        app.rename(Path::new("work"), "Work").unwrap();
        // the folder lists the new spelling (on macOS and Windows the old one would still open it)
        let listed: Vec<String> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        for (new, old) in [("Mine.md", "mine.md"), ("Work", "work")] {
            assert!(listed.iter().any(|n| n == new) && !listed.iter().any(|n| n == old), "{listed:?}");
        }
        assert_eq!(std::fs::read_to_string(d.join("Mine.md")).unwrap(), "mine\n");
        assert!(app.vault.as_ref().unwrap().exists("Mine", None), "the vault knows it by its new name");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: VAULT-027
    #[test]
    fn a_new_folder_with_a_forbidden_character_gets_a_portable_name_and_a_toast() {
        let (mut app, d) = test_app("portable-folder");
        app.cfg.date_prefix_new_notes = false;
        assert_eq!(app.create_folder(Path::new(""), "Notes: a").unwrap(), PathBuf::from("Notes- a"));
        assert!(d.join("Notes- a").is_dir());
        assert_eq!(last_toast(&app), "Saved as `Notes- a`: `:` is not allowed on Windows");
        let n = app.toasts.len();
        app.create_folder(Path::new(""), "Fine").unwrap();
        assert_eq!(app.toasts.len(), n, "no toast for a good name");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: VAULT-027
    #[test]
    fn a_new_drawing_and_a_converted_canvas_get_portable_names_within_127_bytes() {
        let (mut app, d) = test_app("portable-drawing");
        app.cfg.date_prefix_new_notes = false;
        app.new_drawing(Path::new(""), None);
        assert!(d.join("Drawing.excalidraw").exists());
        // a canvas whose name is long and has a colon: the drawing made from it is portable
        let long = format!("{}: map", "k".repeat(200));
        std::fs::write(d.join(format!("{long}.canvas")), r#"{"nodes":[],"edges":[]}"#).unwrap();
        app.rescan();
        let to = app.convert_canvas(Path::new(&format!("{long}.canvas")), false).unwrap();
        let name = to.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.len() <= 127 && !name.contains(':') && name.ends_with(".excalidraw.md"), "{name}");
        assert!(d.join(&to).exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
