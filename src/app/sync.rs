//! Git sync in the app: when it runs, what happens to open buffers, and its settings
//! (ADR-0008; SYNC-002, SYNC-013…SYNC-015).

use super::{muted, ShaleApp};
use crate::drawing::DrawingDoc;
use crate::gitsync::{self, Git, Report};
use crate::theme;
use crate::vault::rel_to_string;
use eframe::egui::{self, Ui};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The sync's state in the app.
#[derive(Default)]
pub struct SyncState {
    pub git: Git,
    /// A running sync: its vault and where its result arrives.
    job: Option<(PathBuf, Receiver<Result<Report, String>>)>,
    /// Set by the job while it changes files in the vault (autosave waits).
    pub tree_busy: Arc<AtomicBool>,
    /// When the last sync started (None = sync as soon as possible).
    last_start: Option<Instant>,
    /// The last result: (local time, summary or error).
    pub last: Option<(String, Result<String, String>)>,
    /// `git --version`, looked up once (None inside = git is missing).
    git_version: Option<Option<String>>,
    /// The URL field in the settings.
    pub url: String,
    /// `origin` of the open vault.
    pub remote: Option<String>,
    /// Where the settings controls were drawn (for UI tests).
    pub(crate) rects: Vec<(&'static str, egui::Rect)>,
}

impl SyncState {
    pub fn running(&self) -> bool {
        self.job.is_some()
    }
}

impl ShaleApp {
    /// A vault was opened: sync it soon, show its remote.
    pub(crate) fn sync_vault_opened(&mut self) {
        self.sync.last_start = None;
        self.sync.last = None;
        self.sync.remote = self.vault.as_ref().and_then(|v| gitsync::remote_url(&self.sync.git, &v.root));
        self.sync.url = self.sync.remote.clone().unwrap_or_default();
    }

    fn git_version(&mut self) -> Option<String> {
        if self.sync.git_version.is_none() {
            self.sync.git_version = Some(self.sync.git.version());
        }
        self.sync.git_version.clone().flatten()
    }

    /// Wait (briefly: a local merge) until the sync no longer changes files, so a save never
    /// lands in the middle of it.
    pub(crate) fn wait_for_sync_tree(&self) {
        let t = Instant::now();
        while self.sync.tree_busy.load(Ordering::SeqCst) && t.elapsed() < Duration::from_secs(30) {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Every frame: collect a finished sync, start one when it is due (SYNC-002).
    pub(crate) fn sync_tick(&mut self, ctx: &egui::Context) {
        if let Some((root, rx)) = &self.sync.job {
            let r = match rx.try_recv() {
                Ok(r) => r,
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(200));
                    return;
                }
                Err(TryRecvError::Disconnected) => Err("the sync stopped unexpectedly".into()),
            };
            let root = root.clone();
            self.sync.job = None;
            self.after_sync(&root, r);
            return;
        }
        if !self.persist_config || !self.vdata.settings.sync.enabled {
            return;
        }
        let Some(root) = self.vault.as_ref().map(|v| v.root.clone()) else { return };
        let interval = self.vdata.settings.sync.interval();
        if self.sync.last_start.is_some_and(|t| t.elapsed() < interval) {
            ctx.request_repaint_after(interval.saturating_sub(self.sync.last_start.map(|t| t.elapsed()).unwrap_or_default()).max(Duration::from_secs(1)));
            return;
        }
        if !gitsync::is_repo(&root) || self.git_version().is_none() {
            self.sync.last_start = Some(Instant::now());
            return;
        }
        self.start_sync();
    }

    /// Save everything, then sync in the background.
    pub(crate) fn start_sync(&mut self) {
        if self.sync.running() {
            return;
        }
        let Some(root) = self.vault.as_ref().map(|v| v.root.clone()) else { return };
        self.sync.last_start = Some(Instant::now());
        self.flush_all();
        self.remember_open_files();
        self.save_config();
        let s = &self.vdata.settings.sync;
        let opts = gitsync::Options { commit_prefix: s.commit_prefix.clone(), commit_suffix: s.commit_suffix.clone(), host: gitsync::host_name(), stamp: gitsync::stamp_now() };
        let git = self.sync.git.clone();
        let busy = self.sync.tree_busy.clone();
        let ctx = self.ctx.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let job_root = root.clone();
        std::thread::spawn(move || {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| gitsync::sync(&git, &job_root, &opts, &busy))).unwrap_or_else(|_| Err("the sync stopped with an internal error".into()));
            busy.store(false, Ordering::SeqCst);
            let _ = tx.send(r);
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
        self.sync.job = Some((root, rx));
    }

    /// Bring buffers in line with what the sync changed (SYNC-013) and tell the user (SYNC-014).
    pub(crate) fn after_sync(&mut self, root: &Path, r: Result<Report, String>) {
        let when = {
            let (_, sod) = crate::cal_time::utc_to_local(crate::cal_time::now());
            crate::cal_time::hhmm(sod)
        };
        let report = match r {
            Ok(rep) => rep,
            Err(e) => {
                let repeated = matches!(&self.sync.last, Some((_, Err(old))) if *old == e);
                if !repeated {
                    self.toast(format!("Git sync failed: {e}"));
                }
                self.sync.last = Some((when, Err(e)));
                return;
            }
        };
        self.sync.last = Some((when, Ok(report.summary())));
        if self.vault.as_ref().map(|v| v.root.as_path()) != Some(root) {
            return; // another vault was opened meanwhile
        }
        self.sync.remote = self.vault.as_ref().and_then(|v| gitsync::remote_url(&self.sync.git, &v.root));
        let mut copies: Vec<PathBuf> = vec![];
        let mut failures: Vec<String> = vec![];
        for rel in &report.changed {
            if rel == Path::new(".shale").join(crate::vault_state::SETTINGS_FILE).as_path() {
                if let Some(p) = self.vdata.reload_settings() {
                    failures.push(p);
                }
                continue;
            }
            let abs = root.join(rel);
            let stamp = gitsync::stamp_now();
            let taken = |p: &Path| root.join(p).exists() || copies.iter().any(|c| c == p);
            // edited while the sync ran: the buffer goes to a conflict copy, never over the file
            let note_copy = match self.notes.get(rel) {
                Some(doc) if doc.dirty => Some((gitsync::conflict_name(rel, &stamp, taken), doc.text.clone().into_bytes())),
                _ => None,
            };
            let drawing_copy = match self.drawings.get(rel) {
                Some(d) if d.dirty => d.file_text().ok().map(|t| (gitsync::conflict_name(rel, &stamp, taken), t.into_bytes())),
                _ => None,
            };
            let mut keep_buffer = false;
            if let Some((copy, bytes)) = note_copy.or(drawing_copy) {
                match crate::fsutil::write_atomic(&root.join(&copy), &bytes) {
                    Ok(()) => copies.push(copy),
                    Err(e) => {
                        failures.push(format!("Could not save your edits of “{}” as a copy: {e}", rel_to_string(rel)));
                        keep_buffer = true;
                    }
                }
            }
            if keep_buffer {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&abs) else { continue };
            if let Some(doc) = self.notes.get_mut(rel) {
                doc.text = text;
                doc.dirty = false;
            } else if self.drawings.contains_key(rel) {
                let d = DrawingDoc::new(rel.clone(), &text);
                self.scenes.put(rel, d.scene.clone());
                self.drawings.insert(rel.clone(), d);
            }
        }
        if let Some(v) = self.vault.as_mut() {
            v.rescan();
        }
        self.rescan();
        let incoming = report.changed.len().saturating_sub(report.conflicts.len());
        let mut msg = vec![];
        if incoming > 0 {
            msg.push(format!("{incoming} file(s) updated from the remote"));
        }
        let all_copies: Vec<String> = report.conflicts.iter().map(|(_, c)| c).chain(copies.iter()).map(|c| rel_to_string(c)).collect();
        if !all_copies.is_empty() {
            msg.push(format!("both versions kept, see {}", all_copies.join(", ")));
        }
        if !msg.is_empty() {
            self.toast(format!("Git sync: {}", msg.join("; ")));
        }
        for f in failures {
            self.toast(f);
        }
    }

    /// The "Git sync" part of the settings. Returns whether a setting changed.
    pub(crate) fn sync_settings_ui(&mut self, ui: &mut Ui) -> bool {
        let mut changed = false;
        self.sync.rects.clear();
        ui.label(egui::RichText::new("Git sync").color(theme::MUTED));
        let Some(root) = self.vault.as_ref().map(|v| v.root.clone()) else { return false };
        if self.git_version().is_none() {
            ui.label("git is not installed. Install git to sync this vault with a git repository.");
            return false;
        }
        let repo = gitsync::is_repo(&root);
        if !repo {
            ui.label(muted("This vault is not a git repository yet. Enter the URL of a remote repository, or run “git init” in the vault folder; Shale then syncs it on its own.").size(12.0));
        } else {
            match &self.sync.remote {
                Some(u) => ui.label(muted(format!("Remote: {u}")).size(12.0)),
                None => ui.label(muted("No remote: changes are committed in the vault's own history only.").size(12.0)),
            };
        }
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.sync.url).hint_text("https://… or git@host:notes.git").desired_width(300.0));
            self.sync.rects.push(("url", r.rect));
            let label = if self.sync.remote.is_some() { "Change remote" } else { "Connect" };
            let b = ui.button(label);
            self.sync.rects.push(("connect", b.rect));
            if b.clicked() {
                let url = self.sync.url.trim().to_string();
                match gitsync::connect(&self.sync.git, &root, &url) {
                    Ok(()) => {
                        self.sync.remote = Some(url);
                        self.sync.last_start = None;
                        self.toast("Remote set; syncing now");
                        self.start_sync();
                    }
                    Err(e) => self.toast(format!("Could not set the remote: {e}")),
                }
            }
        });
        if !repo {
            return false;
        }
        let writable = self.vdata.settings_writable();
        let rects = ui.add_enabled_ui(writable, |ui| {
            let s = &mut self.vdata.settings.sync;
            let r = ui.checkbox(&mut s.enabled, "Sync this vault automatically");
            changed |= r.changed();
            let enabled_rect = r.rect;
            let mut prefix_rect = egui::Rect::NOTHING;
            ui.horizontal(|ui| {
                ui.label("Every");
                changed |= ui.add(egui::DragValue::new(&mut s.interval_minutes).range(1..=1440)).changed();
                ui.label("minutes");
            });
            ui.horizontal(|ui| {
                ui.label("Commit message prefix");
                let r = ui.add(egui::TextEdit::singleline(&mut s.commit_prefix).desired_width(120.0));
                changed |= r.changed();
                prefix_rect = r.rect;
                ui.label("suffix");
                changed |= ui.add(egui::TextEdit::singleline(&mut s.commit_suffix).desired_width(120.0)).changed();
            });
            ui.label(muted(format!("Commits look like: {}Sync from {}: 3 file(s){}", s.commit_prefix, gitsync::host_name(), s.commit_suffix)).size(12.0));
            [("enabled", enabled_rect), ("prefix", prefix_rect)]
        });
        self.sync.rects.extend(rects.inner);
        ui.horizontal(|ui| {
            let b = ui.add_enabled(!self.sync.running(), egui::Button::new("Sync now"));
            self.sync.rects.push(("sync-now", b.rect));
            if b.clicked() {
                self.start_sync();
            }
            let status = if self.sync.running() {
                muted("Syncing…")
            } else {
                match &self.sync.last {
                    Some((t, Ok(s))) => muted(format!("Last sync {t}: {s}")),
                    Some((t, Err(e))) => egui::RichText::new(format!("Last sync {t} failed: {e}")).color(theme::LINK_MISSING),
                    None => muted("Not synced yet in this session"),
                }
            };
            ui.label(status.size(12.0));
        });
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{Options, View};
    use std::ffi::OsString;

    fn hermetic() -> Git {
        let env = [("GIT_CONFIG_GLOBAL", "/dev/null"), ("GIT_CONFIG_NOSYSTEM", "1"), ("GIT_CONFIG_COUNT", "1"), ("GIT_CONFIG_KEY_0", "init.defaultBranch"), ("GIT_CONFIG_VALUE_0", "main")];
        Git { program: "git".into(), env: env.iter().map(|(k, v)| (OsString::from(k), OsString::from(v))).collect() }
    }

    fn app_on(vault: &Path) -> ShaleApp {
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(vault.to_path_buf()), view: None, keys: None }, false);
        app.sync.git = hermetic();
        app
    }

    fn wait(app: &mut ShaleApp) {
        let ctx = egui::Context::default();
        let t = Instant::now();
        while app.sync.running() {
            assert!(t.elapsed() < Duration::from_secs(30), "sync did not finish");
            std::thread::sleep(Duration::from_millis(10));
            app.sync_tick(&ctx);
        }
    }

    // spec: SYNC-013, SYNC-014
    #[test]
    fn buffers_follow_the_sync_and_edits_made_meanwhile_become_copies() {
        let root = std::env::temp_dir().join(format!("shale-app-sync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let git = hermetic();
        let remote = root.join("remote.git");
        std::fs::create_dir_all(&remote).unwrap();
        assert!(std::process::Command::new("git").args(["init", "--quiet", "--bare"]).arg(&remote).envs(git.env.iter().cloned()).status().unwrap().success());
        let a = root.join("a");
        let b = root.join("b");
        for v in [&a, &b] {
            std::fs::create_dir_all(v).unwrap();
            gitsync::connect(&git, v, remote.to_str().unwrap()).unwrap();
        }
        crate::fsutil::write_atomic(&a.join("Clean.md"), b"base\n").unwrap();
        crate::fsutil::write_atomic(&a.join("Busy.md"), b"base\n").unwrap();
        let busy = AtomicBool::new(false);
        let opts = gitsync::Options { host: "a".into(), stamp: "2026-11-14 1200".into(), ..Default::default() };
        gitsync::sync(&git, &a, &opts, &busy).unwrap();

        let mut app = app_on(&b);
        app.start_sync();
        wait(&mut app);
        assert!(matches!(&app.sync.last, Some((_, Ok(_)))), "{:?}", app.sync.last);
        app.open(Path::new("Clean.md"), false);
        app.open(Path::new("Busy.md"), true);
        assert_eq!(app.notes[Path::new("Clean.md")].text, "base\n");

        // the other machine changes both notes
        crate::fsutil::write_atomic(&a.join("Clean.md"), b"remote clean\n").unwrap();
        crate::fsutil::write_atomic(&a.join("Busy.md"), b"remote busy\n").unwrap();
        gitsync::sync(&git, &a, &opts, &busy).unwrap();

        // a sync whose result arrives while "Busy.md" has unsaved edits
        let rep = gitsync::sync(&git, &b, &opts, &busy).unwrap();
        let doc = app.notes.get_mut(Path::new("Busy.md")).unwrap();
        doc.text = "typed during the sync\n".into();
        doc.dirty = true;
        app.after_sync(&b, Ok(rep));

        assert_eq!(app.notes[Path::new("Clean.md")].text, "remote clean\n");
        assert_eq!(app.notes[Path::new("Busy.md")].text, "remote busy\n");
        assert!(!app.notes[Path::new("Busy.md")].dirty);
        assert_eq!(std::fs::read_to_string(b.join("Busy.md")).unwrap(), "remote busy\n");
        let copies: Vec<_> = std::fs::read_dir(&b).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("Busy (conflict ")).collect();
        assert_eq!(copies.len(), 1, "{copies:?}");
        assert_eq!(std::fs::read_to_string(b.join(&copies[0])).unwrap(), "typed during the sync\n");
        assert!(app.toasts.iter().any(|(t, _)| t.contains("both versions kept")), "{:?}", app.toasts);
        assert!(matches!(app.tabs[app.active].view, View::Note(_)));

        // the copy reaches the other machine with the next sync
        app.start_sync();
        wait(&mut app);
        gitsync::sync(&git, &a, &opts, &busy).unwrap();
        assert_eq!(std::fs::read_to_string(a.join(&copies[0])).unwrap(), "typed during the sync\n");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Headless frames of the settings section (like the calendar dialog tests).
    struct H {
        ctx: egui::Context,
        t: f64,
    }

    impl H {
        fn frame(&mut self, app: &mut ShaleApp, events: Vec<egui::Event>) -> bool {
            self.t += 0.05;
            let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 900.0))), time: Some(self.t), events, ..Default::default() };
            let mut changed = false;
            let mut out = self.ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| changed = app.sync_settings_ui(ui));
            });
            out.textures_delta.clear();
            changed
        }

        fn click(&mut self, app: &mut ShaleApp, key: &str) -> bool {
            let p = app.sync.rects.iter().find(|r| r.0 == key).unwrap_or_else(|| panic!("no {key}: {:?}", app.sync.rects)).1.center();
            let b = |pressed| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
            let mut changed = self.frame(app, vec![egui::Event::PointerMoved(p)]);
            changed |= self.frame(app, vec![b(true)]);
            changed |= self.frame(app, vec![b(false)]);
            changed | self.frame(app, vec![])
        }

        fn typ(&mut self, app: &mut ShaleApp, key: &str, text: &str) -> bool {
            let mut changed = self.click(app, key);
            changed |= self.frame(app, vec![egui::Event::Text(text.into())]);
            changed | self.frame(app, vec![])
        }
    }

    // spec: SYNC-004, SYNC-015
    #[test]
    fn settings_connect_a_remote_and_change_the_sync_settings() {
        let root = std::env::temp_dir().join(format!("shale-app-sync-ui-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let git = hermetic();
        let remote = root.join("remote.git");
        std::fs::create_dir_all(&remote).unwrap();
        assert!(std::process::Command::new("git").args(["init", "--quiet", "--bare"]).arg(&remote).envs(git.env.iter().cloned()).status().unwrap().success());
        let v = root.join("vault");
        std::fs::create_dir_all(&v).unwrap();
        crate::fsutil::write_atomic(&v.join("Note.md"), b"hi\n").unwrap();
        let mut app = app_on(&v);
        let mut h = H { ctx: egui::Context::default(), t: 0.0 };
        h.frame(&mut app, vec![]);
        assert!(!app.sync.rects.iter().any(|r| r.0 == "enabled"), "no sync settings before there is a repository");
        h.typ(&mut app, "url", remote.to_str().unwrap());
        h.click(&mut app, "connect");
        assert!(gitsync::is_repo(&v));
        assert_eq!(gitsync::remote_url(&git, &v).as_deref(), remote.to_str());
        wait(&mut app);
        assert!(matches!(&app.sync.last, Some((_, Ok(s))) if s.contains("up to date")), "{:?}", app.sync.last);
        let o = std::process::Command::new("git").arg("-C").arg(&remote).args(["show", "main:Note.md"]).envs(git.env.iter().cloned()).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&o.stdout), "hi\n");

        h.frame(&mut app, vec![]);
        assert!(h.click(&mut app, "enabled"));
        assert!(!app.vdata.settings.sync.enabled);
        assert!(h.typ(&mut app, "prefix", "[n] "));
        assert_eq!(app.vdata.settings.sync.commit_prefix, "[n] ");
        crate::fsutil::write_atomic(&v.join("Note.md"), b"changed\n").unwrap();
        h.click(&mut app, "sync-now");
        wait(&mut app);
        let o = std::process::Command::new("git").arg("-C").arg(&remote).args(["log", "-1", "--format=%s", "main"]).envs(git.env.iter().cloned()).output().unwrap();
        assert!(String::from_utf8_lossy(&o.stdout).starts_with("[n] Sync from "), "{}", String::from_utf8_lossy(&o.stdout));
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: SYNC-013
    #[test]
    fn autosave_waits_while_the_sync_changes_files() {
        let root = std::env::temp_dir().join(format!("shale-app-sync-busy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        crate::fsutil::write_atomic(&root.join("N.md"), b"disk\n").unwrap();
        let mut app = app_on(&root);
        app.open(Path::new("N.md"), false);
        let doc = app.notes.get_mut(Path::new("N.md")).unwrap();
        doc.text = "edited\n".into();
        doc.dirty = true;
        doc.last_edit = Instant::now() - Duration::from_secs(5);
        app.sync.tree_busy.store(true, Ordering::SeqCst);
        app.autosave();
        assert_eq!(std::fs::read_to_string(root.join("N.md")).unwrap(), "disk\n");
        assert!(app.notes[Path::new("N.md")].dirty);
        app.sync.tree_busy.store(false, Ordering::SeqCst);
        app.autosave();
        assert_eq!(std::fs::read_to_string(root.join("N.md")).unwrap(), "edited\n");
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: CFG-006, SYNC-015
    #[test]
    fn vault_settings_and_state_live_in_the_vault() {
        let root = std::env::temp_dir().join(format!("shale-app-vdata-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        crate::fsutil::write_atomic(&root.join("One.md"), b"1\n").unwrap();
        crate::fsutil::write_atomic(&root.join(".shale/settings.json"), br#"{"version": 1, "people_dir": "Kontakte", "sync": {"interval_minutes": 2}}"#).unwrap();
        crate::fsutil::write_atomic(&root.join(".shale/state.json"), br#"{"version": 1, "open_files": ["One.md"]}"#).unwrap();
        let mut app = app_on(&root);
        assert_eq!(app.vdata.settings.people_dir, "Kontakte");
        assert_eq!(app.vdata.settings.sync.interval(), Duration::from_secs(120));
        assert_eq!(app.current_path().as_deref(), Some(Path::new("One.md")));
        app.vdata.state.recent_tags = vec!["t".into()];
        app.vdata.save().unwrap();
        let text = std::fs::read_to_string(root.join(".shale/state.json")).unwrap();
        assert!(text.contains("\"t\""), "{text}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
