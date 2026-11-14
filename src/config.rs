//! App settings, saved as JSON in the user's config folder (~/.config/shale/config.json).

use crate::vault_state::{LegacyVaultFields, Versioned};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// How the file tree is ordered.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FileSort {
    #[default]
    NameAsc,
    NameDesc,
    CreatedAsc,
    CreatedDesc,
    ModifiedAsc,
    ModifiedDesc,
}

impl FileSort {
    pub const ALL: [FileSort; 6] = [FileSort::NameAsc, FileSort::NameDesc, FileSort::ModifiedDesc, FileSort::ModifiedAsc, FileSort::CreatedDesc, FileSort::CreatedAsc];
    pub fn label(self) -> &'static str {
        match self {
            FileSort::NameAsc => "File name (A to Z)",
            FileSort::NameDesc => "File name (Z to A)",
            FileSort::ModifiedDesc => "Modified (new to old)",
            FileSort::ModifiedAsc => "Modified (old to new)",
            FileSort::CreatedDesc => "Created (new to old)",
            FileSort::CreatedAsc => "Created (old to new)",
        }
    }
}

/// How a mail account signs in.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MailAuth {
    /// Password or app password, kept in the keyring.
    #[default]
    Password,
    /// OAuth2 (XOAUTH2); the sign-in itself comes with phase 1c.
    OAuth2,
}

/// How the connection to the mail server is secured. There is no plain-text choice.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MailSecurity {
    /// TLS from the first byte (port 993).
    #[default]
    ImplicitTls,
    /// Plain start, then the upgrade to TLS is mandatory (port 143); nothing is sent before it.
    StartTls,
}

impl MailSecurity {
    /// The usual port for this mode.
    #[allow(dead_code)] // used by the account settings (phase 1b)
    pub fn default_port(self) -> u16 {
        match self {
            MailSecurity::ImplicitTls => 993,
            MailSecurity::StartTls => 143,
        }
    }
}

/// One IMAP account. The password is not here: it is in the keyring (`secrets::mail_secret`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct MailAccount {
    /// Stable name of the account; used for its folder in the mail store (letters, digits, `-`, `_`).
    pub id: String,
    /// What the account is called in the window.
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth: MailAuth,
    pub security: MailSecurity,
}

impl Default for MailAccount {
    fn default() -> Self {
        MailAccount { id: String::new(), name: String::new(), host: String::new(), port: 993, user: String::new(), auth: MailAuth::Password, security: MailSecurity::ImplicitTls }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    /// Format version (CFG-004); see `Versioned for Config`.
    pub version: u64,
    /// The vault opened last.
    pub vault: Option<PathBuf>,
    pub recent: Vec<PathBuf>,
    /// Follow links only with Ctrl+click (plain click edits). Default: plain click follows.
    pub ctrl_to_follow: bool,
    /// Notes created from a red link go next to the current note instead of the vault root.
    pub new_notes_next_to_current: bool,
    /// Rewrite links in other notes when a note is renamed.
    pub update_links_on_rename: bool,
    /// Draw on a dark canvas.
    pub dark_canvas: bool,
    pub font_size: f32,
    /// Keep the text in a comfortable column instead of the full width.
    pub readable_width: bool,
    pub show_left: bool,
    pub show_right: bool,
    /// Order of the file tree.
    pub file_sort: FileSort,
    /// Show embedded images as pictures inside the editor (the link shows when the cursor is on it).
    pub inline_images: bool,
    /// Calendars shown in the calendar panel (local .ics files and CalDAV calendars).
    pub calendars: Vec<crate::cal_store::CalendarSource>,
    /// Calendar weeks start on Monday (otherwise Sunday).
    pub week_starts_monday: bool,
    /// Troubleshooting: record every key, typed text and input-method event to a file.
    pub input_log: bool,
    /// Vim-style editing in notes: normal / insert / visual mode (off by default).
    pub vim_mode: bool,
    /// IMAP accounts of the mail view.
    pub mail_accounts: Vec<MailAccount>,
    // Per-vault fields of older versions, read only to move them into the vault that is opened
    // first (CFG-008); dropped from the file after that.
    #[serde(rename = "open_files", skip_serializing_if = "Option::is_none")]
    pub legacy_open_files: Option<Vec<String>>,
    #[serde(rename = "last_import_folder", skip_serializing_if = "Option::is_none")]
    pub legacy_last_import_folder: Option<String>,
    #[serde(rename = "recent_tags", skip_serializing_if = "Option::is_none")]
    pub legacy_recent_tags: Option<Vec<String>>,
    #[serde(rename = "people_dir", skip_serializing_if = "Option::is_none")]
    pub legacy_people_dir: Option<String>,
    /// Fields of a newer Shale, written back unchanged.
    #[serde(flatten)]
    pub other: serde_json::Map<String, serde_json::Value>,
    /// The file came from a newer Shale or could not be read: never overwrite it (CFG-007).
    #[serde(skip)]
    pub read_only: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            version: <Config as Versioned>::CURRENT,
            vault: None,
            recent: vec![],
            ctrl_to_follow: false,
            new_notes_next_to_current: false,
            update_links_on_rename: true,
            dark_canvas: true,
            font_size: 15.5,
            readable_width: true,
            show_left: true,
            show_right: true,
            file_sort: FileSort::NameAsc,
            inline_images: true,
            calendars: vec![],
            week_starts_monday: true,
            input_log: false,
            vim_mode: false,
            mail_accounts: vec![],
            legacy_open_files: None,
            legacy_last_import_folder: None,
            legacy_recent_tags: None,
            legacy_people_dir: None,
            other: serde_json::Map::new(),
            read_only: false,
        }
    }
}

impl Versioned for Config {
    const CURRENT: u64 = 1;
    const WHAT: &'static str = "settings";
    fn step(_json: &mut serde_json::Map<String, serde_json::Value>, from: u64) {
        match from {
            // 0 → 1: the first versioned format. The per-vault fields stay in the JSON; they
            // are moved into the first vault that is opened (`take_legacy`, CFG-008).
            0 => {}
            _ => unreachable!("no migration from version {from}"),
        }
    }
}

/// Where the troubleshooting key recording goes (~/.cache/shale/input-debug.log).
pub fn input_log_path() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("shale").join("input-debug.log")
}

/// When this program was built, as "YYYY-MM-DD HH:MM UTC".
pub fn build_stamp() -> String {
    let secs: i64 = env!("SHALE_BUILD_SECS").parse().unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", rem / 3600, rem % 3600 / 60)
}

/// Replace a private file in one step (owner-only, in an owner-only folder), so a crash never
/// leaves a half-written file. See `fsutil::write_atomic_private`.
pub fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        crate::fsutil::ensure_private_dir(dir)?;
    }
    crate::fsutil::write_atomic_private(path, data)
}

pub fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("shale").join("config.json"))
}

impl Config {
    /// Load the settings; a problem the user should hear about comes back too. A file that
    /// fails to parse is kept as `config.json.broken-<date>` (CFG-004).
    pub fn load() -> (Config, Option<String>) {
        let Some(p) = config_path() else { return (Config::default(), None) };
        let (mut c, problem) = Self::load_from(&p);
        crate::secrets::load_passwords(&mut c.calendars);
        (c, problem)
    }

    pub fn load_from(path: &Path) -> (Config, Option<String>) {
        let l = crate::vault_state::load::<Config>(path);
        let mut c = l.value;
        c.read_only = !l.writable;
        (c, l.problem)
    }

    /// The per-vault fields of an old config file, removed from this config (CFG-008).
    pub fn take_legacy(&mut self) -> LegacyVaultFields {
        LegacyVaultFields { people_dir: self.legacy_people_dir.take(), open_files: self.legacy_open_files.take(), last_import_folder: self.legacy_last_import_folder.take(), recent_tags: self.legacy_recent_tags.take() }
    }

    /// Put back what `take_legacy` took (the vault could not take it over).
    pub fn restore_legacy(&mut self, l: LegacyVaultFields) {
        self.legacy_people_dir = l.people_dir;
        self.legacy_open_files = l.open_files;
        self.legacy_last_import_folder = l.last_import_folder;
        self.legacy_recent_tags = l.recent_tags;
    }

    pub fn save(&self) {
        if self.read_only {
            return;
        }
        let Some(p) = config_path() else { return };
        // CalDAV passwords go to the system keyring when there is one
        let mut out = self.clone();
        out.calendars = crate::secrets::for_file(&self.calendars);
        if let Ok(s) = serde_json::to_string_pretty(&out) {
            if let Err(e) = write_private(&p, s.as_bytes()) {
                eprintln!("could not save the settings: {e}");
            }
        }
    }

    pub fn remember_vault(&mut self, path: PathBuf) {
        self.recent.retain(|p| p != &path);
        self.recent.insert(0, path.clone());
        self.recent.truncate(8);
        self.vault = Some(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: CFG-001
    #[test]
    fn old_config_files_still_load() {
        let (c, newer) = crate::vault_state::parse::<Config>(br#"{"vault": "/tmp/v"}"#).unwrap();
        assert!(!newer);
        assert_eq!(c.version, 1);
        assert_eq!(c.vault, Some(PathBuf::from("/tmp/v")));
        assert!(c.update_links_on_rename);
        assert!(!c.ctrl_to_follow);
        assert!(c.inline_images);
        assert_eq!(c.file_sort, FileSort::NameAsc);
    }

    // spec: CFG-002
    #[test]
    fn private_files_are_owner_only_and_replaced_whole() {
        let dir = std::env::temp_dir().join(format!("shale-private-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let p = dir.join("sub").join("config.json");
        write_private(&p, b"one").unwrap();
        write_private(&p, b"two").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "two");
        assert_eq!(std::fs::read_dir(dir.join("sub")).unwrap().count(), 1, "no temporary file left");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
            assert_eq!(std::fs::metadata(p.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: MAIL-020
    #[test]
    fn mail_accounts_load_and_have_no_plain_text_mode() {
        let c: Config = serde_json::from_str(r#"{"mail_accounts": [{"id": "a1", "host": "imap.example.com", "user": "ada"}]}"#).unwrap();
        let a = &c.mail_accounts[0];
        assert_eq!((a.port, a.auth, a.security), (993, MailAuth::Password, MailSecurity::ImplicitTls));
        assert_eq!(MailSecurity::StartTls.default_port(), 143);
        let back: Config = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back.mail_accounts, c.mail_accounts);
        // there is no way to ask for an unencrypted connection, and no password field to leak
        assert!(serde_json::from_str::<Config>(r#"{"mail_accounts": [{"security": "Plain"}]}"#).is_err());
        assert!(!serde_json::to_string(&c).unwrap().contains("password"));
    }

    // spec: CFG-004, CFG-008
    #[test]
    fn config_migration_0_to_1_keeps_the_old_vault_fields_until_taken() {
        let old = br#"{"vault": "/v", "font_size": 17.0, "people_dir": "Adressbuch", "open_files": ["a.md"], "recent_tags": ["x"], "last_import_folder": "In"}"#;
        let (mut c, _) = crate::vault_state::parse::<Config>(old).unwrap();
        assert_eq!(c.font_size, 17.0);
        let out = serde_json::to_string(&c).unwrap();
        assert!(out.contains("\"people_dir\":\"Adressbuch\""), "{out}");
        let legacy = c.take_legacy();
        assert_eq!(legacy.people_dir.as_deref(), Some("Adressbuch"));
        assert_eq!(legacy.open_files, Some(vec!["a.md".to_string()]));
        assert_eq!(legacy.last_import_folder.as_deref(), Some("In"));
        assert_eq!(legacy.recent_tags, Some(vec!["x".to_string()]));
        let out = serde_json::to_string(&c).unwrap();
        for k in ["people_dir", "open_files", "recent_tags", "last_import_folder"] {
            assert!(!out.contains(k), "{k} still in {out}");
        }
        assert!(out.contains("\"version\":1"));
        assert!(c.take_legacy().is_empty());
        c.restore_legacy(legacy.clone());
        assert_eq!(c.take_legacy(), legacy);
    }

    // spec: CFG-004
    #[test]
    fn a_broken_config_is_kept_and_a_newer_one_is_not_overwritten() {
        let dir = std::env::temp_dir().join(format!("shale-config-broken-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let p = dir.join("config.json");
        write_private(&p, b"{\"calendars\": [oops").unwrap();
        let (c, problem) = Config::load_from(&p);
        assert!(!c.read_only);
        assert!(problem.unwrap().contains("config.json.broken-"));
        let kept: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(kept.len(), 1);
        assert!(kept[0].starts_with("config.json.broken-"), "{kept:?}");
        write_private(&p, br#"{"version": 7, "font_size": 20.0, "new_thing": true}"#).unwrap();
        let (c, problem) = Config::load_from(&p);
        assert!(c.read_only && problem.is_some());
        assert_eq!(c.font_size, 20.0);
        assert_eq!(c.other.get("new_thing"), Some(&serde_json::Value::Bool(true)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: VAULT-006
    #[test]
    fn recent_list_is_unique() {
        let mut c = Config::default();
        c.remember_vault("/a".into());
        c.remember_vault("/b".into());
        c.remember_vault("/a".into());
        assert_eq!(c.recent, vec![PathBuf::from("/a"), PathBuf::from("/b")]);
    }
}
