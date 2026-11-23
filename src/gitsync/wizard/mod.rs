//! The "Turn on sync" wizard (SYNC-047…SYNC-058, ADR-0029): its state, and the jobs that talk to
//! the network, the keyring and git. The dialog is `ui.rs`; the pieces are `provider` (what a
//! server is), `api` (list and create repositories), `ssh` (keys) and `test_sync` (what the
//! remote holds, the words for the result).
//!
//! The state is one small struct owned by the sync state (`Setup`), not fields of the window.
//! Every slow thing is a background job (JOB-001) that reports a [`Msg`] or a message in words.

pub mod api;
pub mod provider;
pub mod ssh;
pub mod test_sync;
mod ui;

pub(crate) use ui::warnings_ui;

// Linux only: its keyring is a pretend `secret-tool`, the one backend a separate process can fake.
#[cfg(all(test, target_os = "linux"))]
mod forgejo_env_tests;
#[cfg(test)]
mod parts_tests;
#[cfg(all(test, unix))]
mod ui_tests;

/// Tests: a hermetic git (no user or system configuration, branch `main`, no git-lfs) for a test folder.
#[cfg(all(test, target_os = "linux"))]
fn ui_git(_root: &Path) -> Git {
    let env = [("GIT_CONFIG_GLOBAL", "/dev/null"), ("GIT_CONFIG_NOSYSTEM", "1"), ("GIT_CONFIG_COUNT", "1"), ("GIT_CONFIG_KEY_0", "init.defaultBranch"), ("GIT_CONFIG_VALUE_0", "main")];
    let mut git = Git { program: "git".into(), env: env.iter().map(|(k, v)| (std::ffi::OsString::from(k), std::ffi::OsString::from(v))).collect() };
    super::hide_lfs(&mut git);
    git
}

use super::detect::Finding;
use super::{connect_moving, credentials, Git, TreeLock};
use crate::busy::{self, Busy};
use crate::secret::Secret;
use api::RepoInfo;
use eframe::egui;
use provider::{Kind, Server};
use ssh::Key;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use test_sync::{Problem, Remote};

/// The steps of the wizard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// "How do you want to keep your notes safe?"
    Choose,
    /// The server: a provider button or an address.
    Server,
    /// Sign in: a token or an SSH key.
    SignIn,
    /// Create or pick the repository.
    Repo,
    /// What will happen, and the test sync.
    Test,
    /// The history choices end here.
    Done,
}

/// The three ways to keep notes safe (SYNC-049, SYNC-050).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Local,
    Synced,
    Server,
}

/// How the person signs in to the server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Token,
    Ssh,
}

/// How the test sync is going.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Test {
    Idle,
    /// Asked for; it starts when no snapshot or sync runs.
    Pending,
    Running,
    Worked(String),
    Failed(Problem),
}

/// What a finished sign-in job brings.
pub struct SignedIn {
    pub login: String,
    pub repos: Vec<RepoInfo>,
}

/// What a job reports.
pub enum Msg {
    SignedIn(SignedIn),
    Key(Key),
    Created(RepoInfo),
    /// What the server's repository holds, and whether the vault has notes.
    /// What the server holds, whether the vault has notes, and whether a local history moved in (SYNC-072).
    Inspected(Remote, bool, bool),
}

type Job = busy::Job<Result<Msg, String>>;

/// One call of the wizard (sign in, make a key, create a repository, connect and ask the server) as
/// a task (JOB-033). It cannot be stopped: a provider call or a connect that is dropped half way
/// would leave a token, a key or a repository behind that nobody was told about. Closing the dialog
/// leaves it running and in the indicator, and its result is dropped.
pub(crate) const WIZARD_CALL: busy::Key = busy::Key::new("git-wizard-call");
/// Looking at where the vault is (SYNC-061), as a quiet task (JOB-033): it runs whenever the page is shown for a new vault.
pub(crate) const VAULT_PLACE: busy::Key = busy::Key::quiet("vault-place");
/// Moving `.git` out of the vault (SYNC-063): not stopped half way.
pub(crate) const MOVE_OUT: busy::Key = busy::Key::new("git-move-out");
/// Advanced → Connect, which may move a local history into the vault (SYNC-072): not stopped half way.
pub(crate) const CONNECT: busy::Key = busy::Key::new("git-connect");

/// The wizard's state while it is open.
pub struct Wizard {
    pub step: Step,
    pub choice: Option<Choice>,
    pub kind: Kind,
    pub address: String,
    pub server: Option<Server>,
    /// A repository named by the pasted address, or picked.
    pub repo_named: Option<(String, String)>,
    /// A folder or file address (no sign-in).
    pub local_remote: Option<String>,
    pub method: Method,
    pub user: String,
    pub token: Secret,
    pub signed_in: bool,
    pub keys: Vec<Key>,
    pub key: Option<usize>,
    pub repos: Vec<RepoInfo>,
    pub repo: Option<usize>,
    pub repo_text: String,
    pub new_name: String,
    pub remote: Option<String>,
    pub inspected: Option<Remote>,
    pub vault_has_notes: bool,
    /// The versions kept on this computer moved into the vault's `.git` when it was connected (SYNC-075).
    pub history_moved: bool,
    pub test: Test,
    /// What the last step said went wrong (or right), in words.
    pub message: Option<(String, bool)>,
    pub(super) job: Option<Job>,
    /// The text of the final screen of the history choices.
    pub done: Option<String>,
}

impl Default for Wizard {
    fn default() -> Wizard {
        Wizard {
            step: Step::Choose,
            choice: None,
            kind: Kind::Other,
            address: String::new(),
            server: None,
            repo_named: None,
            local_remote: None,
            method: Method::Token,
            user: String::new(),
            token: Secret::default(),
            signed_in: false,
            keys: vec![],
            key: None,
            repos: vec![],
            repo: None,
            repo_text: String::new(),
            new_name: "notes".into(),
            remote: None,
            inspected: None,
            vault_has_notes: false,
            history_moved: false,
            test: Test::Idle,
            message: None,
            job: None,
            done: None,
        }
    }
}

impl Wizard {
    /// A job runs.
    pub fn busy(&self) -> bool {
        self.job.is_some() || matches!(self.test, Test::Pending | Test::Running)
    }

    /// The step before this one (SYNC-047: Back changes nothing but the step).
    pub fn back(&mut self) {
        self.message = None;
        self.step = match self.step {
            Step::Choose | Step::Server | Step::Done => Step::Choose,
            Step::SignIn => Step::Server,
            Step::Repo => {
                if self.local_remote.is_some() {
                    Step::Server
                } else {
                    Step::SignIn
                }
            }
            Step::Test => {
                self.test = Test::Idle;
                if self.local_remote.is_some() {
                    Step::Server
                } else {
                    Step::Repo
                }
            }
        };
    }

    /// The key picked in the list of SSH keys.
    pub fn chosen_key(&self) -> Option<&Key> {
        self.key.and_then(|i| self.keys.get(i))
    }

    pub fn say(&mut self, text: impl Into<String>, ok: bool) {
        self.message = Some((text.into(), ok));
    }
}

/// An absolute path to a folder (or a `file://` address): a remote that needs no sign-in.
pub fn is_local_remote(text: &str) -> bool {
    let t = text.trim();
    super::is_folder_path(t) || t.starts_with("file://")
}

/// What the Settings page and the wizard know about where the vault is (SYNC-059…SYNC-062).
#[derive(Default)]
pub struct Detect {
    /// The vault the findings are for.
    pub for_vault: Option<PathBuf>,
    pub findings: Vec<Finding>,
    pub(super) job: Option<busy::Job<Vec<Finding>>>,
}

/// A running or finished move of `.git` out of the vault (SYNC-063).
#[derive(Default)]
pub struct MoveOut {
    pub(super) job: Option<(PathBuf, busy::Job<Result<PathBuf, String>>)>,
    /// How the last move ended: words and whether it went well.
    pub result: Option<(String, bool)>,
}

/// The job of an Advanced connection: whether a history moved (the outer error: the job died).
type JoinJob = busy::Job<Result<bool, String>>;

/// Connecting that may move a local-only history into the vault (SYNC-072): while one runs, no
/// snapshot, thinning or read of the history starts, and Advanced → Connect keeps its job here.
#[derive(Default)]
pub struct Joining {
    count: Arc<AtomicUsize>,
    pub(super) job: Option<(PathBuf, String, JoinJob)>,
}

/// Counts a connecting job as running until it is dropped (also when the job panics).
pub struct JoinGuard(Arc<AtomicUsize>);

impl Drop for JoinGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Joining {
    /// A connecting job runs.
    pub fn active(&self) -> bool {
        self.count.load(Ordering::SeqCst) > 0
    }

    /// Count a job that starts now; the job holds the guard.
    pub fn begin(&self) -> JoinGuard {
        self.count.fetch_add(1, Ordering::SeqCst);
        JoinGuard(self.count.clone())
    }
}

/// The wizard, the detection and the move, as one sub-state of the sync state.
#[derive(Default)]
pub struct Setup {
    pub joining: Joining,
    pub wizard: Option<Wizard>,
    pub detect: Detect,
    pub moveout: MoveOut,
    /// Settings → Sync shows the address field and **Connect** (SYNC-047).
    pub advanced: bool,
    /// The home folder for SSH keys (tests use another).
    pub home: Option<PathBuf>,
}

impl MoveOut {
    /// A move runs.
    pub fn running(&self) -> bool {
        self.job.is_some()
    }
}

impl Setup {
    pub fn home(&self) -> Option<PathBuf> {
        self.home.clone().or_else(dirs::home_dir)
    }

    /// Findings for this vault (empty while the job runs or when nothing was found).
    pub fn findings(&self, vault: &Path) -> &[Finding] {
        if self.detect.for_vault.as_deref() == Some(vault) {
            &self.detect.findings
        } else {
            &[]
        }
    }

    /// Look at where `vault` is, in a job, unless that was done (SYNC-061).
    pub fn detect_for(&mut self, vault: &Path, ctx: &egui::Context, busy: &Busy) {
        if self.detect.for_vault.as_deref() == Some(vault) || self.detect.job.is_some() {
            return;
        }
        let (v, ctx) = (vault.to_path_buf(), ctx.clone());
        let Some(job) = busy.spawn(VAULT_PLACE, "Looking at where the vault is…", move || super::detect::detect_real(&v), move || ctx.request_repaint()) else { return };
        self.detect.for_vault = Some(vault.to_path_buf());
        self.detect.findings.clear();
        self.detect.job = Some(job);
    }

    /// Take a finished detection.
    pub fn poll_detect(&mut self) {
        if let Some(job) = &self.detect.job {
            if let Ok(r) = job.try_recv() {
                self.detect.job = None;
                self.detect.findings = r.unwrap_or_default();
            }
        }
    }

    /// The vault was closed or another one opened: look again when asked.
    pub fn forget_vault(&mut self) {
        self.detect.for_vault = None;
        self.detect.findings.clear();
        self.moveout.result = None;
    }
}

// ---------------------------------------------------------------- jobs

/// Sign in with a token (SYNC-052, SYNC-053): ask the provider who the token belongs to and for the
/// person's repositories where it has an API, then keep the token in the keyring.
pub fn sign_in(server: &Server, user_hint: &str, token: &Secret) -> Result<SignedIn, String> {
    if token.is_empty() {
        return Err("Paste the token first".into());
    }
    let (login, repos) = if server.has_api() {
        let login = api::whoami(server, token).map_err(|e| e.message())?;
        // the list is a convenience: a token that may not list repositories can still sync one
        let repos = api::list(server, token).unwrap_or_default();
        (login, repos)
    } else {
        let user = user_hint.trim();
        if user.is_empty() {
            return Err("Enter your user name on the server".into());
        }
        if user.contains(['@', ':', '/', '\\']) || user.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err("A user name has no spaces, @, : or slashes".into());
        }
        (user.to_string(), vec![])
    };
    let key = credentials::server_key(&server.base()).ok_or("That server address cannot hold a token")?;
    if !crate::secrets::git_token_store(&key, &login, token.expose()) {
        return Err("Shale could not keep the token: this computer has no keyring, or it refused. Use an SSH key instead; Shale never writes a token to a file.".into());
    }
    Ok(SignedIn { login, repos })
}

/// Make the vault a repository whose `origin` is `url` (a local-only history moves into its `.git`
/// first, SYNC-072), set the SSH command for a key, and ask the server what its repository holds
/// (SYNC-057). The last value: a history moved.
pub fn connect_and_inspect(git: &Git, vault: &Path, url: &str, ssh_key: Option<&Path>, history_root: &Path, tree: &TreeLock) -> Result<(Remote, bool, bool), String> {
    let moved = connect_moving(git, vault, url, history_root, tree)?;
    if let Some(cmd) = ssh::ssh_command(ssh_key, false) {
        // the key is part of the repository's configuration (a path, nothing secret)
        if ssh_key.is_some() {
            git.ok(vault, args_of(&["config", "core.sshCommand", &cmd]).as_slice())?;
        }
    }
    let mut probe = git.clone();
    if let Some(cmd) = ssh::ssh_command(ssh_key, true) {
        probe.env.push(("GIT_SSH_COMMAND".into(), cmd.into()));
    }
    let remote = test_sync::inspect(&probe, vault).map_err(|e| test_sync::classify(&e).words())?;
    Ok((remote, test_sync::vault_has_notes(vault), moved))
}

fn args_of<'a>(a: &'a [&'a str]) -> Vec<&'a std::ffi::OsStr> {
    a.iter().map(std::ffi::OsStr::new).collect()
}

/// The address of the remote for a repository: with the user name for a token (SYNC-054), the SSH
/// address for a key.
pub fn remote_for(server: &Server, method: Method, user: &str, owner: &str, repo: &str) -> String {
    match method {
        Method::Token => server.https_remote(user, owner, repo),
        Method::Ssh => server.ssh_remote(owner, repo),
    }
}
