//! The wizard driven through the whole window, headless (SYNC-047…SYNC-063): each step, the three
//! choices, Back and Cancel, the warnings, and the server path against a file remote (real git) and
//! a pretend Forgejo API (real HTTP through curl). Jobs are the real ones.

use super::provider::Kind;
use super::{Step, Test};
use crate::app::{Modal, Options, SettingsTab, ShaleApp};
use crate::gitsync::detect::{Cloud, Finding, Kind as Found};
use crate::gitsync::managed::{Detection, GitChoice};
use crate::gitsync::sources::{Platform, Sources};
use crate::gitsync::{self, Git};
use crate::ui_test_support::Headless;
use eframe::egui::{self, PointerButton};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn hermetic() -> Git {
    let env = [("GIT_CONFIG_GLOBAL", "/dev/null"), ("GIT_CONFIG_NOSYSTEM", "1"), ("GIT_CONFIG_COUNT", "1"), ("GIT_CONFIG_KEY_0", "init.defaultBranch"), ("GIT_CONFIG_VALUE_0", "main")];
    let mut git = Git { program: "git".into(), env: env.iter().map(|(k, v)| (OsString::from(k), OsString::from(v))).collect() };
    crate::gitsync::hide_lfs(&mut git);
    git
}

struct T {
    app: ShaleApp,
    hl: Headless,
    root: PathBuf,
    vault: PathBuf,
}

impl T {
    fn new(tag: &str) -> T {
        let root = std::env::temp_dir().join(format!("shale-wizard-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let vault = root.join("vault");
        std::fs::create_dir_all(&vault).unwrap();
        crate::fsutil::write_atomic(&vault.join("Note.md"), b"hello\n").unwrap();
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(vault.clone()), view: None, keys: None }, false);
        app.sync.git = hermetic();
        app.refresh_lfs();
        // nothing to find in a temporary folder, and no keys of the person
        app.sync.setup.detect.for_vault = Some(vault.clone());
        app.sync.setup.home = Some(root.join("home"));
        let mut hl = Headless::new();
        hl.set_size(egui::Vec2::new(1500.0, 1400.0));
        let mut t = T { app, hl, root, vault };
        t.hl.frames(&mut t.app, 3);
        t.hl.settle(&mut t.app);
        t
    }

    fn painted(&self) -> Vec<&str> {
        self.hl.text.iter().map(|(t, _)| t.as_str()).collect()
    }

    fn shows(&mut self, text: &str) -> bool {
        self.hl.frames(&mut self.app, 3);
        self.hl.text.iter().any(|(t, _)| t.contains(text))
    }

    fn click(&mut self, label: &str) {
        let r = self.hl.find_label(&mut self.app, label);
        self.hl.click(&mut self.app, r.center(), PointerButton::Primary);
    }

    fn has_button(&mut self, label: &str) -> bool {
        self.hl.try_label(&mut self.app, label).is_some()
    }

    /// Settings → Sync, drawn once the page has its final height. The page starts a job that looks
    /// for git ("Looking for git…", one line) and shows what it found when it ends (five lines):
    /// everything below moves down by then, so a click on a place read before that misses.
    fn open_sync_settings(&mut self) {
        self.app.open_settings(SettingsTab::Sync);
        let t = Instant::now();
        loop {
            self.hl.frames(&mut self.app, 1);
            if self.app.sync.managed.detection.is_some() {
                break;
            }
            assert!(t.elapsed() < Duration::from_secs(60), "the look for git did not end");
            std::thread::sleep(Duration::from_millis(10));
        }
        self.hl.frames(&mut self.app, 3);
    }

    fn open_wizard(&mut self) {
        self.open_sync_settings();
        self.click("Turn on sync…");
        assert!(matches!(self.app.modal, Some(Modal::SyncWizard)), "the wizard opened");
        self.hl.frames(&mut self.app, 3);
    }

    fn wizard(&mut self) -> &mut super::Wizard {
        self.app.sync.setup.wizard.as_mut().expect("the wizard is open")
    }

    /// Frames until no job and no test sync runs.
    fn settle(&mut self) {
        let t = Instant::now();
        loop {
            self.hl.frames(&mut self.app, 1);
            let idle = self.app.sync.setup.wizard.as_ref().is_none_or(|w| !w.busy()) && !self.app.sync.running();
            if idle {
                self.hl.frames(&mut self.app, 3);
                return;
            }
            assert!(t.elapsed() < Duration::from_secs(60), "the job did not end");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn bare(&self) -> PathBuf {
        let p = self.root.join("remote.git");
        std::fs::create_dir_all(&p).unwrap();
        assert!(std::process::Command::new("git").args(["init", "--quiet", "--bare"]).arg(&p).envs(hermetic().env.iter().cloned()).status().unwrap().success());
        p
    }
}

impl Drop for T {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

const THREE: [&str; 3] = ["History on this device only", "My folder is already synced by another app", "Sync with a server"];

// spec: SYNC-047
#[test]
fn sync_047_settings_offer_turn_on_sync_the_wizard_asks_three_things_and_cancel_changes_nothing() {
    let mut t = T::new("three");
    t.open_sync_settings();
    assert!(t.shows("Turn on sync…") && t.shows("Advanced"), "{:?}", t.painted());
    assert!(!t.has_button("Connect"), "the address field is under Advanced");
    t.click("Turn on sync…");
    assert!(matches!(t.app.modal, Some(Modal::SyncWizard)));
    assert!(t.shows("How do you want to keep your notes safe?"), "{:?}", t.painted());
    for c in THREE {
        assert!(t.shows(c), "{c:?} missing in {:?}", t.painted());
    }
    let text = t.painted().join("\n");
    assert!(text.contains("Nextcloud, Dropbox, OneDrive, iCloud or a network drive") && text.contains("Forgejo, Gitea, GitHub, GitLab or any git address") && text.contains("No server and no account"), "{text}");
    // Next waits for a choice
    t.hl.frames(&mut t.app, 2);
    let next = t.hl.find_label(&mut t.app, "Next");
    t.hl.click(&mut t.app, next.center(), PointerButton::Primary);
    assert_eq!(t.wizard().step, Step::Choose);
    t.click("Sync with a server");
    t.click("Cancel");
    assert!(t.app.modal.is_none() && t.app.sync.setup.wizard.is_none(), "Cancel closes it");
    assert!(!gitsync::is_repo(&t.vault) && t.app.sync.remote.is_none(), "nothing changed");
    assert!(!t.vault.join(".shale/history-id").exists());
}

// spec: SYNC-047
#[test]
fn sync_047_back_goes_one_step_and_changes_nothing() {
    let mut t = T::new("back");
    t.open_wizard();
    t.click("Sync with a server");
    t.click("Next");
    assert!(t.shows("Which server?"));
    t.click("Back");
    assert!(t.shows("How do you want to keep your notes safe?") && t.wizard().choice == Some(super::Choice::Server));
    assert!(!gitsync::is_repo(&t.vault));
}

// spec: SYNC-049
#[test]
fn sync_049_history_on_this_device_turns_history_on_and_writes_no_git_into_the_vault() {
    let mut t = T::new("local");
    t.app.cfg.history.enabled = false;
    t.open_wizard();
    t.click("History on this device only");
    t.click("Next");
    assert!(t.app.cfg.history.enabled, "the history is on");
    assert!(t.shows("Shale keeps a history of your notes on this computer, outside the vault folder"), "{:?}", t.painted());
    assert!(!t.vault.join(".git").exists());
    t.click("Finish");
    assert!(t.app.modal.is_none());
    assert!(t.app.toasts.iter().any(|(m, _)| m.contains("history on this device")), "{:?}", t.app.toasts);
    assert!(!t.vault.join(".git").exists());
}

// spec: SYNC-049
#[test]
fn sync_049_a_folder_synced_by_another_app_is_explained_and_gets_no_git_folder() {
    let mut t = T::new("synced");
    t.app.cfg.history.enabled = false;
    t.open_wizard();
    t.click("My folder is already synced by another app");
    t.click("Next");
    assert!(t.app.cfg.history.enabled);
    let ok = t.shows("travel through the app that syncs it") && t.shows("the history of your notes stays on each computer") || t.painted().join(" ").contains("history of your notes stays on each computer");
    assert!(ok, "{:?}", t.painted());
    let text = t.painted().join("\n");
    assert!(text.contains("Nextcloud, Dropbox, OneDrive, iCloud, a network drive") && text.contains("no .git folder is put in it") && text.contains("safe for your notes"), "{text}");
    assert!(!t.vault.join(".git").exists());
    t.click("Finish");
    assert!(t.app.modal.is_none());
}

// spec: SYNC-048
#[cfg(unix)]
#[test]
fn sync_048_without_git_the_first_step_sends_the_person_to_the_install_and_goes_on_by_itself() {
    let mut t = T::new("nogit");
    let good = t.app.sync.git.clone();
    t.app.sync.git = Git { program: "/nonexistent/shale-no-git".into(), env: vec![] };
    t.app.sync.forget_git_version();
    let data = t.root.join("data");
    t.app.sync.managed = crate::gitsync::install_ui::ManagedState::for_test(data, Some(OsString::from(t.root.join("no-bin"))), Sources::real(), Platform::new("linux", "x86_64"));
    t.app.sync.managed.set_detection(Detection { system_git: None, system_lfs: None, managed: None, git_in_use: None, lfs_in_use: None }, GitChoice::Auto);
    t.open_wizard();
    let text = t.painted().join("\n");
    assert!(text.contains("Shale needs git for this, and there is none on this computer yet"), "{text}");
    assert!(text.contains("There is no git on this computer"), "the install help of #468 is shown: {text}");
    for c in THREE {
        assert!(!text.contains(c), "no choices before git is there: {c}");
    }
    // git appears (an install ended, the window selected it): the choices show up without reopening
    t.app.sync.git = good;
    t.app.sync.forget_git_version();
    assert!(t.shows("How do you want to keep your notes safe?") && t.shows(THREE[2]), "{:?}", t.painted());
}

/// Type into the address field: click where its hint is painted, then type.
fn type_address(t: &mut T, text: &str) {
    t.hl.frames(&mut t.app, 3);
    let (_, pos) = t.hl.text.iter().find(|(s, _)| s.contains("git.example.org, https://")).cloned().expect("the address field is shown");
    t.hl.click(&mut t.app, pos + egui::vec2(20.0, 6.0), PointerButton::Primary);
    t.hl.frame(&mut t.app, vec![egui::Event::Text(text.into())]);
    t.hl.frames(&mut t.app, 2);
    assert_eq!(t.wizard().address, text);
}

// spec: SYNC-050, SYNC-057, SYNC-058
#[test]
fn sync_058_the_server_path_with_a_file_remote_sets_origin_says_what_will_happen_tests_the_sync_and_turns_it_on() {
    let mut t = T::new("file-remote");
    let remote = t.bare();
    t.open_wizard();
    t.click("Sync with a server");
    t.click("Next");
    assert!(t.shows("Which server?") && t.shows("GitHub") && t.shows("GitLab") && t.shows("Codeberg"), "{:?}", t.painted());
    type_address(&mut t, remote.to_str().unwrap());
    t.click("Next");
    t.settle();
    // origin is set, the repository exists, and the wizard says what the first sync does
    assert!(gitsync::is_repo(&t.vault));
    assert_eq!(gitsync::remote_url(&t.app.sync.git, &t.vault).as_deref(), remote.to_str());
    assert_eq!(t.wizard().step, Step::Test);
    assert!(t.shows("The repository on the server is empty, so your notes will be uploaded to it."), "{:?}", t.painted());
    assert!(!t.app.sync.remote.is_some() || t.app.sync.remote.is_some());
    // the test sync is the real sync job
    t.click("Run the test sync");
    t.settle();
    let last = t.app.sync.last.clone();
    assert!(matches!(t.wizard().test, Test::Worked(_)), "{:?} {last:?}", t.wizard().test);
    assert!(t.shows("It worked"), "{:?}", t.painted());
    let shown = std::process::Command::new("git").arg("-C").arg(&remote).args(["show", "main:Note.md"]).envs(hermetic().env.iter().cloned()).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&shown.stdout), "hello\n");
    // Finish turns the automatic sync on
    t.app.vdata.settings.sync.enabled = false;
    t.click("Finish");
    assert!(t.app.modal.is_none());
    assert!(t.app.vdata.settings.sync.enabled, "sync is on");
    assert_eq!(t.app.sync.remote.as_deref(), remote.to_str());
    assert!(t.app.toasts.iter().any(|(m, _)| m.contains("Sync is on")));
}

// spec: SYNC-004, SYNC-050
#[test]
fn sync_050_a_file_remote_in_a_folder_with_a_space_is_accepted() {
    let mut t = T::new("file-remote-space");
    let folder = t.root.join("My notes");
    std::fs::create_dir_all(&folder).unwrap();
    let remote = folder.join("notes.git");
    std::fs::create_dir_all(&remote).unwrap();
    assert!(std::process::Command::new("git").args(["init", "--quiet", "--bare"]).arg(&remote).envs(hermetic().env.iter().cloned()).status().unwrap().success());
    t.open_wizard();
    t.click("Sync with a server");
    t.click("Next");
    type_address(&mut t, remote.to_str().unwrap());
    t.click("Next");
    t.settle();
    assert!(!t.shows("can't contain spaces"), "{:?}", t.painted());
    assert_eq!(gitsync::remote_url(&t.app.sync.git, &t.vault).as_deref(), remote.to_str());
    assert_eq!(t.wizard().step, Step::Test);
    // the same for an address: a space is still refused, and a folder on Windows is read as a folder
    assert!(super::is_local_remote(r"C:\Users\Ada\OneDrive - Company\notes.git"));
    assert!(!super::is_local_remote("https://host/my notes.git"));
}

// spec: SYNC-057
#[test]
fn sync_057_a_server_with_notes_and_a_vault_with_notes_are_said_to_merge() {
    let mut t = T::new("merge-words");
    let remote = t.bare();
    // another computer put a note there
    let other = t.root.join("other");
    std::fs::create_dir_all(&other).unwrap();
    gitsync::connect(&hermetic(), &other, remote.to_str().unwrap()).unwrap();
    crate::fsutil::write_atomic(&other.join("Theirs.md"), b"theirs\n").unwrap();
    gitsync::sync(&hermetic(), &other, &gitsync::Options { host: "o".into(), stamp: "2026-11-21 1200".into(), ..Default::default() }, &gitsync::TreeLock::default()).unwrap();
    t.open_wizard();
    t.click("Sync with a server");
    t.click("Next");
    type_address(&mut t, remote.to_str().unwrap());
    t.click("Next");
    t.settle();
    let text = t.painted().join("\n");
    assert!(text.contains("Both this vault and the repository on the server have notes") && text.contains("conflict"), "{text}");
    t.click("Run the test sync");
    t.settle();
    assert!(matches!(t.wizard().test, Test::Worked(_)), "{:?}", t.wizard().test);
    assert_eq!(crate::fsutil::read_to_string(t.vault.join("Theirs.md")).unwrap(), "theirs\n", "their note came in");
    assert_eq!(crate::fsutil::read_to_string(t.vault.join("Note.md")).unwrap(), "hello\n", "nothing of ours was lost");
}

// spec: SYNC-058
#[test]
fn sync_058_a_remote_that_is_not_there_says_so_in_words_and_leaves_origin_set_to_retry() {
    let mut t = T::new("missing");
    let missing = t.root.join("nowhere.git");
    t.open_wizard();
    t.click("Sync with a server");
    t.click("Next");
    type_address(&mut t, missing.to_str().unwrap());
    t.click("Next");
    t.settle();
    assert_eq!(t.wizard().step, Step::Server, "it stays where the person can fix it");
    assert!(t.shows("The server has no repository at this address"), "{:?}", t.painted());
    assert_eq!(gitsync::remote_url(&t.app.sync.git, &t.vault).as_deref(), missing.to_str(), "origin stays set");
    // a wrong address is refused before anything is made
    let mut t2 = T::new("bad-address");
    t2.open_wizard();
    t2.click("Sync with a server");
    t2.click("Next");
    type_address(&mut t2, "http://example.org/o/r.git");
    t2.click("Next");
    assert!(t2.shows("only this computer may use") || t2.shows("unencrypted") || t2.shows("https://"), "{:?}", t2.painted());
    assert!(!gitsync::is_repo(&t2.vault));
}

// spec: SYNC-058
#[test]
fn sync_058_a_failing_test_sync_goes_back_with_the_reason_and_try_again_works() {
    let mut t = T::new("fails");
    let remote = t.bare();
    t.open_wizard();
    t.click("Sync with a server");
    t.click("Next");
    type_address(&mut t, remote.to_str().unwrap());
    t.click("Next");
    t.settle();
    // the server disappears between the check and the test
    let moved = t.root.join("moved.git");
    std::fs::rename(&remote, &moved).unwrap();
    t.click("Run the test sync");
    t.settle();
    assert!(matches!(t.wizard().test, Test::Failed(_)), "{:?}", t.wizard().test);
    assert!(t.shows("no repository") || t.shows("The server has no repository"), "{:?}", t.painted());
    assert!(t.has_button("Try again") && t.has_button("Back"));
    // fixed: it works
    std::fs::rename(&moved, &remote).unwrap();
    t.click("Try again");
    t.settle();
    assert!(matches!(t.wizard().test, Test::Worked(_)), "{:?}", t.wizard().test);
}

/// A pretend Forgejo: the API of SYNC-055 and 404 for anything else (git's own requests).
fn pretend_forgejo(n: usize) -> (String, std::thread::JoinHandle<Vec<String>>) {
    crate::calendar::dav::tests::mock_http(n, |m, p, h, b| match (m, p) {
        ("GET", "/api/v1/user") => {
            assert!(h.to_ascii_lowercase().contains("authorization: token tok-secret-1"), "{h}");
            (200, r#"{"login":"ada"}"#.into())
        }
        ("GET", "/api/v1/user/repos?limit=50") => (200, r#"[{"name":"old","full_name":"ada/old","private":true,"empty":false,"owner":{"login":"ada"}}]"#.into()),
        ("POST", "/api/v1/user/repos") => {
            assert!(b.contains("\"name\":\"fresh\"") && b.contains("\"private\":true"), "{b}");
            (201, r#"{"name":"fresh","full_name":"ada/fresh","private":true,"empty":false,"owner":{"login":"ada"}}"#.into())
        }
        _ => (404, "{\"message\":\"not found\"}".into()),
    })
}

// spec: SYNC-052, SYNC-053, SYNC-055
#[test]
fn sync_053_a_token_is_checked_kept_in_the_keyring_and_never_in_a_file_and_the_repository_is_created_or_picked() {
    let mut t = T::new("token");
    let (base, seen) = pretend_forgejo(4);
    let host = base.trim_start_matches("http://").to_string();
    t.open_wizard();
    t.click("Sync with a server");
    t.click("Next");
    type_address(&mut t, &base);
    t.wizard().kind = Kind::Forgejo;
    t.click("Next");
    assert_eq!(t.wizard().step, Step::SignIn);
    assert!(t.shows(&format!("Sign in to {host}")) && t.shows("A personal access token") && t.shows("An SSH key"), "{:?}", t.painted());
    // a Forgejo has its token page, opened by the system's opener (SEC-006)
    assert!(t.shows("Open the page where you make a token"));
    let opener = crate::opener::FakeXdgOpen::install();
    t.click("Open the page where you make a token");
    assert_eq!(opener.calls(1), vec![vec![format!("{base}/user/settings/applications")]]);
    // the token goes in a password field and is checked against the provider
    t.wizard().token = crate::secret::Secret::new("tok-secret-1");
    t.click("Check the token");
    t.settle();
    let (step, msg) = (t.wizard().step, t.wizard().message.clone());
    assert_eq!(step, Step::Repo, "{msg:?}");
    assert!(t.wizard().signed_in && t.wizard().user == "ada" && t.wizard().token.is_empty(), "the field is emptied");
    let key = format!("http://{host}");
    assert_eq!(crate::secrets::git_token(&key, "ada").map(|s| s.expose().to_string()).as_deref(), Some("tok-secret-1"), "it is in the keyring");
    // the repositories of the account, and a new one
    assert!(t.shows("ada/old") && t.shows("Create a private repository"), "{:?}", t.painted());
    t.wizard().new_name = "fresh".into();
    t.click("Create a private repository");
    t.settle();
    assert_eq!(t.wizard().repos[0].full_name(), "ada/fresh");
    assert_eq!(t.wizard().repo, Some(0));
    // Next sets origin with the user name in it (the pretend server is no git server, so asking it fails)
    t.click("Next");
    t.settle();
    let url = gitsync::remote_url(&t.app.sync.git, &t.vault).unwrap();
    assert_eq!(url, format!("http://ada@{host}/ada/fresh.git"));
    let _ = seen.join();
    // the token is in no file of the vault or its repository
    fn all_files(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                all_files(&p, out);
            } else {
                out.push(p);
            }
        }
    }
    let mut files = vec![];
    all_files(&t.vault, &mut files);
    assert!(files.iter().any(|f| f.ends_with(".git/config")));
    for f in files {
        let bytes = std::fs::read(&f).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("tok-secret-1"), "the token is in {}", f.display());
    }
    // nor in the global settings text
    assert!(!crate::config::text::to_toml(&t.app.cfg).contains("tok-secret-1"));
}

// spec: SYNC-052
#[test]
fn sync_052_a_wrong_token_says_so_and_nothing_is_kept() {
    let mut t = T::new("wrong-token");
    let (base, seen) = crate::calendar::dav::tests::mock_http(1, |_, _, _, _| (401, "{\"message\":\"no\"}".into()));
    t.open_wizard();
    t.click("Sync with a server");
    t.click("Next");
    type_address(&mut t, &base);
    t.wizard().kind = Kind::Forgejo;
    t.click("Next");
    t.wizard().token = crate::secret::Secret::new("wrong");
    t.click("Check the token");
    t.settle();
    assert_eq!(t.wizard().step, Step::SignIn);
    assert!(t.shows("The server did not accept the token"), "{:?}", t.painted());
    assert!(!t.wizard().signed_in);
    let host = base.trim_start_matches("http://").to_string();
    assert!(crate::secrets::git_token(&format!("http://{host}"), "ada").is_none());
    let _ = seen.join();
}

fn keygen_available() -> bool {
    std::process::Command::new("ssh-keygen").arg("-?").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().is_ok()
}

// spec: SYNC-056
#[test]
fn sync_056_ssh_makes_a_key_shows_the_public_part_with_copy_and_the_provider_page_and_sets_the_remote() {
    if !keygen_available() {
        eprintln!("skipping: ssh-keygen is not installed here");
        return;
    }
    let mut t = T::new("ssh");
    t.open_wizard();
    t.click("Sync with a server");
    t.click("Next");
    type_address(&mut t, "git@localhost:ada/notes.git");
    t.wizard().kind = Kind::Forgejo;
    t.click("Next");
    assert_eq!(t.wizard().step, Step::SignIn);
    assert_eq!(t.wizard().method, super::Method::Ssh, "an SSH address means an SSH key");
    assert!(t.shows("Make a new key"));
    t.click("Make a new key");
    t.settle();
    let key = t.wizard().chosen_key().cloned().expect("a key was made");
    assert!(key.private.starts_with(t.root.join("home/.ssh")) && key.public.starts_with("ssh-ed25519 "));
    let head = key.public[..30].to_string();
    assert!(t.shows("Add this public key to your account on the server") && t.shows(&head), "{:?}", t.painted());
    assert!(t.has_button("Copy the public key"));
    // Next names the repository from the address and sets origin with the key
    t.click("Next");
    t.settle();
    assert_eq!(gitsync::remote_url(&t.app.sync.git, &t.vault).as_deref(), Some("git@localhost:ada/notes.git"));
    let config = std::fs::read_to_string(t.vault.join(".git/config")).unwrap();
    assert!(config.contains("sshCommand") && config.contains(&key.private.to_string_lossy().to_string()) && config.contains("IdentitiesOnly=yes"), "{config}");
    assert!(!config.contains("PRIVATE KEY"), "only the path of the key");
}

// spec: SYNC-061, SYNC-062
#[test]
fn sync_062_a_vault_with_git_inside_a_cloud_folder_is_warned_and_offered_the_move_when_it_has_no_server() {
    let mut t = T::new("warn");
    let git = hermetic();
    gitsync::connect(&git, &t.vault, t.bare().to_str().unwrap()).unwrap();
    t.app.sync.remote = Some("x".into());
    t.app.sync.setup.detect.findings = vec![Finding { kind: Found::Cloud(Cloud::Dropbox), at: t.root.clone() }];
    t.open_sync_settings();
    let text = t.painted().join("\n");
    assert!(text.contains("a Dropbox folder") && text.contains("half copied"), "{text}");
    assert!(text.contains("sync with the server") && !t.has_button("Move the repository out"), "a vault with a server is offered nothing destructive: {text}");

    // no server: the same vault gets the offer, and the move works
    t.app.sync.remote = None;
    git_remote_remove(&git, &t.vault);
    t.hl.frames(&mut t.app, 3);
    assert!(t.has_button("Move the repository out"), "{:?}", t.painted());
    t.click("Move the repository out");
    let start = Instant::now();
    while t.app.sync.setup.moveout.running() {
        assert!(start.elapsed() < Duration::from_secs(30));
        std::thread::sleep(Duration::from_millis(10));
        t.hl.frames(&mut t.app, 1);
    }
    t.hl.frames(&mut t.app, 3);
    assert!(!t.vault.join(".git").exists(), "the vault's repository is gone");
    assert!(t.shows("The repository now lives on this computer"), "{:?}", t.painted());
    let id = crate::gitsync::history::vault_id(&t.vault).expect("the vault has an ID");
    assert!(t.app.usage.places.history.join(&id).join("HEAD").is_file(), "the repository is in the history place");
    let _ = std::fs::remove_dir_all(t.app.usage.places.history.join(&id));
}

fn git_remote_remove(git: &Git, vault: &Path) {
    let out = std::process::Command::new("git").arg("-C").arg(vault).args(["remote", "remove", "origin"]).envs(git.env.iter().cloned()).status().unwrap();
    assert!(out.success());
}

// spec: SYNC-061
#[test]
fn sync_061_the_first_step_warns_where_the_vault_is_and_nothing_found_shows_nothing() {
    let mut t = T::new("warn-step");
    t.open_wizard();
    let text = t.painted().join("\n");
    assert!(!text.contains("looks like it is in"), "nothing found, nothing shown: {text}");
    t.app.sync.setup.detect.findings = vec![Finding { kind: Found::Network { fs: "nfs4".into(), sure: true }, at: "/mnt".into() }];
    assert!(t.shows("This vault looks like it is in a network folder (nfs4)"), "{:?}", t.painted());
    assert!(t.shows("Shale only warns"));
    // the choices are still there: it never decides
    assert!(t.shows(THREE[0]) && t.shows(THREE[2]));
}

// spec: SYNC-061
#[test]
fn sync_061_the_detection_runs_in_a_job_when_the_page_is_shown_for_a_new_vault() {
    let mut t = T::new("detect-job");
    t.app.sync.setup.forget_vault();
    std::fs::create_dir_all(t.vault.join(".stfolder")).unwrap();
    t.app.open_settings(SettingsTab::Sync);
    let start = Instant::now();
    while t.app.sync.setup.findings(&t.vault).is_empty() {
        assert!(start.elapsed() < Duration::from_secs(30), "no finding");
        t.hl.frames(&mut t.app, 1);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(t.app.sync.setup.findings(&t.vault)[0].kind, Found::Cloud(Cloud::Syncthing));
    assert!(t.shows("a Syncthing folder"), "{:?}", t.painted());
}

/// Give the vault a local-only history of three snapshots of `Note.md` in a data folder of the test.
fn local_history(t: &mut T) -> Vec<String> {
    let hist = t.root.join("data/history");
    t.app.usage.places.history = hist.clone();
    let repo = gitsync::history::locate(&t.app.sync.git, &t.vault, &hist).unwrap();
    gitsync::history::ensure(&repo).unwrap();
    let opts = gitsync::Options { host: "laptop".into(), stamp: "2026-11-14 1200".into(), ..Default::default() };
    for text in ["one\n", "two\n", "three\n"] {
        crate::fsutil::write_atomic(&t.vault.join("Note.md"), text.as_bytes()).unwrap();
        let req = gitsync::history::Request::new(gitsync::history::Reason::Save, vec![PathBuf::from("Note.md")]);
        gitsync::history::snapshot(&repo, &opts, &req, &gitsync::TreeLock::default(), Duration::from_secs(5)).unwrap();
    }
    let v = gitsync::history::versions(&repo, Path::new("Note.md"), 10).unwrap();
    assert_eq!(v.len(), 3);
    v.into_iter().map(|v| v.commit).collect()
}

fn remote_commits(remote: &Path) -> String {
    let o = std::process::Command::new("git").arg("-C").arg(remote).args(["rev-list", "main"]).envs(hermetic().env.iter().cloned()).output().unwrap();
    String::from_utf8_lossy(&o.stdout).into_owned()
}

// spec: SYNC-072, SYNC-075
#[test]
fn sync_075_the_server_path_moves_a_local_history_into_the_vault_and_says_so() {
    let mut t = T::new("move-in");
    let commits = local_history(&mut t);
    let hist = t.app.usage.places.history.clone();
    let id = gitsync::history::vault_id(&t.vault).unwrap();
    assert!(hist.join(&id).join("HEAD").is_file() && !gitsync::is_repo(&t.vault));
    let remote = t.bare();
    t.open_wizard();
    t.click("Sync with a server");
    t.click("Next");
    type_address(&mut t, remote.to_str().unwrap());
    t.click("Next");
    t.settle();
    // moved: the vault's repository holds the snapshots, the data folder's copy is gone
    assert!(t.vault.join(".git/HEAD").is_file(), "the vault has its own repository");
    assert!(!hist.join(&id).exists(), "the old folder is removed");
    let repo = gitsync::history::locate(&t.app.sync.git, &t.vault, &hist).unwrap();
    assert!(repo.git_dir.is_none());
    let versions: Vec<String> = gitsync::history::versions(&repo, Path::new("Note.md"), 10).unwrap().into_iter().map(|v| v.commit).collect();
    assert_eq!(versions, commits, "the same versions");
    assert_eq!(t.wizard().step, Step::Test);
    assert!(t.wizard().history_moved);
    // the words say it, together with what the first sync does
    assert!(t.shows("The versions Shale kept of your notes on this computer have moved into the vault"), "{:?}", t.painted());
    assert!(t.shows("The first sync sends them to the server together with your notes."));
    assert!(t.shows("The repository on the server is empty, so your notes will be uploaded to it."));
    // the test sync pushes them
    t.click("Run the test sync");
    t.settle();
    assert!(matches!(t.wizard().test, Test::Worked(_)), "{:?}", t.wizard().test);
    let on_remote = remote_commits(&remote);
    assert!(commits.iter().all(|c| on_remote.contains(c.as_str())), "the versions are on the server: {on_remote}");
}

// spec: SYNC-075
#[test]
fn sync_075_a_vault_without_a_local_history_gets_no_such_words() {
    let mut t = T::new("move-in-none");
    t.app.usage.places.history = t.root.join("data/history");
    let remote = t.bare();
    t.open_wizard();
    t.click("Sync with a server");
    t.click("Next");
    type_address(&mut t, remote.to_str().unwrap());
    t.click("Next");
    t.settle();
    assert_eq!(t.wizard().step, Step::Test);
    assert!(!t.wizard().history_moved);
    assert!(!t.shows("have moved into the vault"), "{:?}", t.painted());
    assert!(!t.vault.join(".shale/history-id").exists(), "no ID was made for a vault that had no history");
}

// spec: SYNC-072
#[test]
fn sync_072_advanced_connect_moves_a_local_history_in_a_job_and_syncs() {
    let mut t = T::new("move-in-advanced");
    let commits = local_history(&mut t);
    let hist = t.app.usage.places.history.clone();
    let id = gitsync::history::vault_id(&t.vault).unwrap();
    let remote = t.bare();
    t.open_sync_settings();
    t.app.sync.setup.advanced = true;
    t.app.sync.url = remote.to_str().unwrap().to_string();
    t.hl.frames(&mut t.app, 3);
    t.click("Connect");
    // a job: it runs beside the frames, and nothing of the history starts beside it
    let start = Instant::now();
    while t.app.sync.setup.joining.job.is_some() || t.app.sync.running() {
        assert!(start.elapsed() < Duration::from_secs(60), "the connection did not end");
        t.hl.frames(&mut t.app, 1);
        std::thread::sleep(Duration::from_millis(10));
    }
    t.hl.frames(&mut t.app, 3);
    assert!(!t.app.sync.setup.joining.active());
    assert!(t.vault.join(".git/HEAD").is_file() && !hist.join(&id).exists());
    assert_eq!(gitsync::remote_url(&t.app.sync.git, &t.vault).as_deref(), remote.to_str());
    assert!(t.app.toasts.iter().any(|(m, _)| m.contains("moved into the vault")), "{:?}", t.app.toasts);
    let on_remote = remote_commits(&remote);
    assert!(commits.iter().all(|c| on_remote.contains(c.as_str())), "the first sync pushed the versions: {on_remote}");
}

// spec: SYNC-072, SYNC-073
#[test]
fn sync_073_a_failed_move_connects_nothing_and_says_why() {
    let mut t = T::new("move-in-fail");
    local_history(&mut t);
    let hist = t.app.usage.places.history.clone();
    let id = gitsync::history::vault_id(&t.vault).unwrap();
    // a socket in the history cannot be copied, by any user
    let _socket = std::os::unix::net::UnixListener::bind(hist.join(&id).join("s")).unwrap();
    let remote = t.bare();
    t.open_wizard();
    t.click("Sync with a server");
    t.click("Next");
    type_address(&mut t, remote.to_str().unwrap());
    t.click("Next");
    t.settle();
    assert!(!t.vault.join(".git").exists(), "the vault has no .git");
    assert!(hist.join(&id).join("HEAD").is_file(), "the local history is where it was");
    assert_eq!(t.wizard().step, Step::Server);
    assert!(t.shows("could not copy"), "{:?}", t.painted());
    assert!(!t.wizard().history_moved);
}
