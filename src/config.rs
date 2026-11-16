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
    /// OAuth2 (XOAUTH2 / OAUTHBEARER); the sign-in is in `oauth.rs`.
    OAuth2,
}

/// Where an OAuth2 account signs in. Google and Microsoft have fixed endpoints; Custom has the
/// ones typed in the account settings (an own provider, or the local test environment).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OAuthProvider {
    #[default]
    Google,
    Microsoft,
    Custom,
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
    pub fn default_port(self) -> u16 {
        match self {
            MailSecurity::ImplicitTls => 993,
            MailSecurity::StartTls => 143,
        }
    }

    /// The usual port of the mail submission service (SMTP) for this mode (MAIL-120).
    pub fn smtp_default_port(self) -> u16 {
        match self {
            MailSecurity::ImplicitTls => 465,
            MailSecurity::StartTls => 587,
        }
    }
}

/// The two standards for signing and encrypting mail (ADR-0011): OpenPGP (`gpg`) and S/MIME (`gpgsm`).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CryptoStandard {
    #[default]
    OpenPgp,
    Smime,
}

impl CryptoStandard {
    pub fn label(self) -> &'static str {
        match self {
            CryptoStandard::OpenPgp => "OpenPGP",
            CryptoStandard::Smime => "S/MIME",
        }
    }
    /// The GnuPG program that does it.
    pub fn program(self) -> &'static str {
        match self {
            CryptoStandard::OpenPgp => "gpg",
            CryptoStandard::Smime => "gpgsm",
        }
    }
}

impl MailAccount {
    /// The fingerprint of this account's key for a standard (empty: none).
    pub fn key_for(&self, s: CryptoStandard) -> &str {
        match s {
            CryptoStandard::OpenPgp => &self.pgp_key,
            CryptoStandard::Smime => &self.smime_key,
        }
    }
}

/// What a person has to type, exactly, to switch encryption on (ADR-0011 §3, OPEN-017).
pub const CONSENT_PHRASE: &str = "I agree to decrypted local storage";

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
    /// OAuth2 only: the provider, and the client ID registered with it. Shale has no built-in
    /// client IDs. The client secret (some providers need one) is in the keyring.
    pub oauth_provider: OAuthProvider,
    pub oauth_client_id: String,
    /// OAuth2 with the Custom provider only: the authorization and token endpoints and the scope.
    pub oauth_auth_url: String,
    pub oauth_token_url: String,
    pub oauth_scope: String,
    /// Sending (phase 3, MAIL-120): the SMTP server. The sign-in is the one above (same
    /// password, or the same OAuth2 sign-in). Never plain text: 465 implicit TLS or 587 STARTTLS.
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_security: MailSecurity,
    /// The sender: name and address (empty address = the user name, if it is an address).
    pub from_name: String,
    pub from_address: String,
    /// Added below a new mail (and above the quote of a reply).
    pub signature: String,
    /// The server saves a copy of what is sent in Sent by itself (Gmail): Shale then does not append one.
    pub server_stores_sent: bool,
    /// Add a `User-Agent` header to sent mail (off: privacy).
    pub user_agent_header: bool,
    /// Signing and encrypting (phase 4, MAIL-190): the fingerprint of the OpenPGP key and of the S/MIME
    /// certificate in GnuPG that belong to this account (empty: none), sign new mail by default, and which
    /// standard a new mail uses.
    pub pgp_key: String,
    pub smime_key: String,
    pub sign_by_default: bool,
    pub preferred_standard: CryptoStandard,
}

impl Default for MailAccount {
    fn default() -> Self {
        MailAccount {
            id: String::new(),
            name: String::new(),
            host: String::new(),
            port: 993,
            user: String::new(),
            auth: MailAuth::Password,
            security: MailSecurity::ImplicitTls,
            oauth_provider: OAuthProvider::Google,
            oauth_client_id: String::new(),
            oauth_auth_url: String::new(),
            oauth_token_url: String::new(),
            oauth_scope: String::new(),
            smtp_host: String::new(),
            smtp_port: 465,
            smtp_security: MailSecurity::ImplicitTls,
            from_name: String::new(),
            from_address: String::new(),
            signature: String::new(),
            server_stores_sent: false,
            user_agent_header: false,
            pgp_key: String::new(),
            smime_key: String::new(),
            sign_by_default: false,
            preferred_standard: CryptoStandard::OpenPgp,
        }
    }
}

/// Where the icons of the window are (LAY-001, LAY-002). The ids are the strings of `sidebar::Item`;
/// what they mean and how a stored list is repaired is in `sidebar.rs`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(default)]
pub struct SidebarCfg {
    /// The ribbon (left menubar), top to bottom. Empty: the default order.
    pub ribbon: Vec<String>,
    /// The views of the right sidebar, top to bottom. Empty: the default (one view with all panels).
    pub right: Vec<RightViewCfg>,
    /// Icons the user removed (LAY-013); the context menu brings them back.
    pub hidden: Vec<String>,
}

/// One view of the right sidebar: its menubar (icon ids, left to right) and the icon it shows.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(default)]
pub struct RightViewCfg {
    pub icons: Vec<String>,
    pub active: String,
    /// Share of the height of the right sidebar (LAY-012); 0: the equal share.
    pub weight: u32,
}

/// A value of the wrong shape (an old or hand-edited file) loads as the default instead of
/// making the whole settings file unreadable.
fn lenient<'de, D: serde::Deserializer<'de>, T: serde::de::DeserializeOwned + Default>(d: D) -> Result<T, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(serde_json::from_value(v).unwrap_or_default())
}

/// The retention settings of the caches (USE-020). `None` means "forever" for an age and "no
/// limit" for a size or count.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct CacheRetention {
    /// Social pictures: days since a picture was fetched.
    pub picture_days: Option<u32>,
    /// Social pictures: the size of the folder in MB.
    pub picture_mb: Option<u32>,
    /// Social post index: days by the date of the post (SOC-032).
    pub post_days: Option<u32>,
    /// Social post index: posts per account (SOC-032).
    pub posts_per_account: Option<u32>,
    /// Mail index backups: how many of the newest are kept (MAIL-008).
    pub mail_backups: Option<u32>,
    /// Damaged mail index copies: days by the date in their name.
    pub broken_days: Option<u32>,
    /// Input debug log: its size in MB.
    pub debug_log_mb: Option<u32>,
}

impl Default for CacheRetention {
    fn default() -> Self {
        CacheRetention { picture_days: Some(30), picture_mb: Some(500), post_days: Some(365), posts_per_account: Some(10_000), mail_backups: Some(3), broken_days: None, debug_log_mb: Some(10) }
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
    /// The order of the ribbon icons (and the views of the right sidebar).
    #[serde(deserialize_with = "lenient")]
    pub sidebars: SidebarCfg,
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
    /// The mail view fetches new mail by itself every this many minutes while the app runs
    /// (0 = only with the Sync button).
    pub mail_sync_minutes: u32,
    /// Opening a message in the mail view marks it read on the server (MAIL-109).
    pub mail_mark_read: bool,
    /// ... after this many seconds (0 = at once).
    pub mail_mark_read_secs: u32,
    /// Seconds a sent mail waits in the outbox, where it can still be pulled back (0 = send at once) (MAIL-134).
    pub mail_undo_secs: u32,
    /// The date and time the person typed the consent to decrypted local storage (ADR-0011, MAIL-180);
    /// None = encryption is off: nothing is decrypted.
    pub mail_encryption_consent: Option<String>,
    /// Opening an Obsidian canvas converts it to an editable drawing without asking.
    pub auto_convert_canvas: bool,
    /// Mastodon and Bluesky accounts for the feed view (secrets are in the keyring).
    pub social_accounts: Vec<crate::social::SocialAccountEntry>,
    /// The account and feed shown last in the feed view.
    pub social_last: Option<(String, String)>,
    /// Include replies from other people when saving a post (the last choice in the Import dialog).
    pub social_replies: bool,
    /// Minutes between looks for new posts in the feeds open in tabs (0 = never, SOC-025).
    pub social_check_minutes: u32,
    /// How long each cache is kept (USE-020).
    #[serde(deserialize_with = "lenient")]
    pub caches: CacheRetention,
    /// Keybindings the user changed: action id -> bindings as text (KEY-007). Missing = defaults.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub keybindings: std::collections::BTreeMap<String, Vec<String>>,
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
            sidebars: SidebarCfg::default(),
            file_sort: FileSort::NameAsc,
            inline_images: true,
            calendars: vec![],
            week_starts_monday: true,
            input_log: false,
            vim_mode: false,
            mail_accounts: vec![],
            mail_sync_minutes: 10,
            mail_mark_read: true,
            mail_mark_read_secs: 2,
            mail_undo_secs: 10,
            mail_encryption_consent: None,
            auto_convert_canvas: false,
            social_accounts: vec![],
            social_last: None,
            social_replies: false,
            social_check_minutes: 5,
            caches: CacheRetention::default(),
            keybindings: Default::default(),
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

#[cfg(test)]
thread_local! {
    /// Tests: where the key recording of this thread goes (never the real cache folder).
    pub static INPUT_LOG_TEST_PATH: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Where the troubleshooting key recording goes (~/.cache/shale/input-debug.log).
pub fn input_log_path() -> PathBuf {
    #[cfg(test)]
    if let Some(p) = INPUT_LOG_TEST_PATH.with(|c| c.borrow().clone()) {
        return p;
    }
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("shale").join("input-debug.log")
}

/// When this program was built, as "YYYY-MM-DD HH:MM UTC".
/// In screenshot mode with a frozen clock (DEV-007) it shows that time instead, so that the
/// pictures do not change with every build.
pub fn build_stamp() -> String {
    stamp_of(crate::cal_time::frozen().unwrap_or_else(|| env!("SHALE_BUILD_SECS").parse().unwrap_or(0)))
}

fn stamp_of(secs: i64) -> String {
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

#[cfg(test)]
thread_local! {
    /// Tests: the settings file of this thread (never the real one in ~/.config).
    pub static CONFIG_TEST_PATH: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

pub fn config_path() -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(p) = CONFIG_TEST_PATH.with(|c| c.borrow().clone()) {
        return Some(p);
    }
    dirs::config_dir().map(|d| d.join("shale").join("config.json"))
}

impl Config {
    /// Load the settings; a problem the user should hear about comes back too. A file that
    /// fails to parse is kept as `config.json.broken-<date>` (CFG-004).
    pub fn load() -> (Config, Option<String>) {
        let Some(p) = config_path() else { return (Config::default(), None) };
        let (mut c, problem) = Self::load_from(&p);
        crate::secrets::load_passwords(&mut c.calendars);
        for e in c.social_accounts.iter_mut() {
            crate::secrets::social_move_to_keyring(&e.account.server, &e.account.handle, &mut e.secret);
        }
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

    /// Write the settings. A failure is returned for the caller to show (DATA-009).
    pub fn save(&self) -> Result<(), String> {
        if self.read_only {
            return Ok(());
        }
        let Some(p) = config_path() else { return Ok(()) };
        // CalDAV passwords go to the system keyring when there is one
        let mut out = self.clone();
        out.calendars = crate::secrets::for_file(&self.calendars);
        let s = serde_json::to_string_pretty(&out).map_err(|e| format!("could not save the settings: {e}"))?;
        write_private(&p, s.as_bytes()).map_err(|e| format!("could not save the settings: {e}"))
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

    // spec: DEV-007
    #[test]
    fn dev_007_the_build_stamp_is_the_build_time_unless_the_clock_is_frozen() {
        assert_eq!(stamp_of(crate::cal_time::days_from_civil(2026, 11, 15) * 86_400 + 36_000), "2026-11-15 10:00 UTC");
        assert_eq!(crate::cal_time::frozen(), None);
        assert_eq!(build_stamp(), stamp_of(env!("SHALE_BUILD_SECS").parse().unwrap()));
    }

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

    // spec: LAY-002
    #[test]
    fn lay_002_a_layout_of_the_wrong_shape_loads_as_the_default() {
        let c: Config = serde_json::from_str(r#"{"sidebars": "oops", "font_size": 18.0}"#).unwrap();
        assert_eq!((c.sidebars, c.font_size), (SidebarCfg::default(), 18.0), "the rest of the file still loads");
        let c: Config = serde_json::from_str(r#"{"sidebars": {"ribbon": [1, 2]}}"#).unwrap();
        assert_eq!(c.sidebars, SidebarCfg::default());
        let c: Config = serde_json::from_str(r#"{"sidebars": {"ribbon": ["mail", "files"]}}"#).unwrap();
        assert_eq!(c.sidebars.ribbon, ["mail", "files"]);
        let back: Config = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back.sidebars, c.sidebars);
        let old: Config = serde_json::from_str(r#"{"vault": "/tmp/v"}"#).unwrap();
        assert_eq!(old.sidebars, SidebarCfg::default(), "files from before the layout have the default");
    }

    // spec: MAIL-120
    #[test]
    fn mail_accounts_of_older_files_load_with_sensible_sending_defaults_and_never_a_plain_text_choice() {
        let c: Config = serde_json::from_str(r#"{"mail_accounts": [{"id": "a1", "host": "imap.example.com", "user": "ada@example.com"}]}"#).unwrap();
        let a = &c.mail_accounts[0];
        assert_eq!((a.smtp_host.as_str(), a.smtp_port, a.smtp_security, a.server_stores_sent, a.user_agent_header), ("", 465, MailSecurity::ImplicitTls, false, false));
        assert_eq!(c.mail_undo_secs, 10, "the undo-send time defaults to 10 seconds");
        assert_eq!((MailSecurity::ImplicitTls.smtp_default_port(), MailSecurity::StartTls.smtp_default_port()), (465, 587));
        assert!(serde_json::from_str::<Config>(r#"{"mail_accounts": [{"smtp_security": "Plain"}]}"#).is_err(), "there is no plain-text sending");
        let back = serde_json::to_string(&c).unwrap();
        assert_eq!(serde_json::from_str::<Config>(&back).unwrap().mail_accounts, c.mail_accounts);
    }

    // spec: SOC-003
    #[test]
    fn social_accounts_are_saved_without_secrets_that_the_keyring_holds() {
        use crate::social::{Account, Network, SocialAccountEntry};
        let in_keyring = SocialAccountEntry { account: Account::new(Network::Mastodon, "https://m.test", "ada@m.test", "Ada"), secret: Default::default() };
        let kept = SocialAccountEntry { account: Account::new(Network::Bluesky, "https://bsky.social", "ada.bsky.social", ""), secret: "app-pw-123".into() };
        let c = Config { social_accounts: vec![in_keyring.clone(), kept.clone()], social_last: Some(("a".into(), "home".into())), ..Config::default() };
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(json.matches("\"secret\"").count(), 1, "only the account without a keyring has a secret in the file: {json}");
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(back.social_accounts, vec![in_keyring, kept]);
        assert_eq!(back.social_last, Some(("a".into(), "home".into())));
        assert!(!format!("{:?}", back.social_accounts).contains("app-pw-123"), "never in debug output");
        // an old file has none
        let old: Config = serde_json::from_str(r#"{"vault": "/tmp/v"}"#).unwrap();
        assert!(old.social_accounts.is_empty() && old.social_last.is_none());
        assert_eq!(old.social_check_minutes, 5, "files from before the checks use the default");
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
    // spec: MAIL-190, MAIL-180
    #[test]
    fn older_settings_load_without_keys_or_consent_and_the_new_ones_round_trip() {
        let c: Config = serde_json::from_str(r#"{"mail_accounts": [{"id": "a1", "host": "imap.example.com", "user": "ada"}]}"#).unwrap();
        let a = &c.mail_accounts[0];
        assert_eq!((a.pgp_key.as_str(), a.smime_key.as_str(), a.sign_by_default, a.preferred_standard), ("", "", false, CryptoStandard::OpenPgp));
        assert_eq!(c.mail_encryption_consent, None, "encryption is off until the consent is typed");
        assert_eq!((a.key_for(CryptoStandard::OpenPgp), a.key_for(CryptoStandard::Smime)), ("", ""));
        let mut c = c;
        c.mail_accounts[0].pgp_key = "0123456789ABCDEF0123456789ABCDEF01234567".into();
        c.mail_accounts[0].preferred_standard = CryptoStandard::Smime;
        c.mail_accounts[0].sign_by_default = true;
        c.mail_encryption_consent = Some("2026-11-14 12:00".into());
        let back: Config = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back.mail_accounts, c.mail_accounts);
        assert_eq!(back.mail_encryption_consent.as_deref(), Some("2026-11-14 12:00"));
        assert_eq!(CONSENT_PHRASE, "I agree to decrypted local storage");
        assert_eq!((CryptoStandard::OpenPgp.program(), CryptoStandard::Smime.program()), ("gpg", "gpgsm"));
    }

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

    /// Points the settings file of this test thread at `dir/shale/config.json`, and back at the
    /// end (also when the test fails).
    struct TestConfig(PathBuf);

    impl TestConfig {
        fn new(tag: &str) -> TestConfig {
            static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let dir = std::env::temp_dir().join(format!("shale-config-{tag}-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            CONFIG_TEST_PATH.with(|c| *c.borrow_mut() = Some(dir.join("shale").join("config.json")));
            TestConfig(dir)
        }
        fn file(&self) -> PathBuf {
            self.0.join("shale").join("config.json")
        }
    }

    impl Drop for TestConfig {
        fn drop(&mut self) {
            CONFIG_TEST_PATH.with(|c| *c.borrow_mut() = None);
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // spec: CFG-001
    #[test]
    fn cfg_001_the_settings_are_saved_to_the_config_folder_and_loaded_again() {
        let t = TestConfig::new("roundtrip");
        let (c, problem) = Config::load();
        assert!(problem.is_none() && !c.read_only, "a first start has no file and no problem");
        let mut c = c;
        c.font_size = 21.0;
        c.remember_vault("/some/vault".into());
        c.save().unwrap();
        assert!(t.file().exists());
        let (back, problem) = Config::load();
        assert!(problem.is_none());
        assert_eq!(back.font_size, 21.0);
        assert_eq!(back.vault, Some(PathBuf::from("/some/vault")));
        assert_eq!(back.recent, vec![PathBuf::from("/some/vault")]);
    }

    // spec: CFG-004
    #[test]
    fn cfg_004_load_reports_a_broken_file_and_save_still_works_afterwards() {
        let t = TestConfig::new("corrupt");
        write_private(&t.file(), b"{not json").unwrap();
        let (mut c, problem) = Config::load();
        assert!(problem.unwrap().contains("config.json.broken-"));
        assert!(!c.read_only, "a broken file is replaced by the defaults, the original is kept");
        c.font_size = 19.0;
        c.save().unwrap();
        assert_eq!(Config::load().0.font_size, 19.0);
    }

    // spec: CFG-007
    #[test]
    fn cfg_007_a_config_of_a_newer_shale_is_not_overwritten_by_save() {
        let t = TestConfig::new("newer");
        let newer = br#"{"version": 99, "font_size": 20.0}"#;
        write_private(&t.file(), newer).unwrap();
        let (mut c, problem) = Config::load();
        assert!(c.read_only && problem.is_some());
        c.font_size = 12.0;
        assert!(c.save().is_ok(), "nothing to report: saving is skipped on purpose");
        assert_eq!(std::fs::read(t.file()).unwrap(), newer);
    }

    // spec: DATA-009
    #[test]
    fn data_009_a_settings_file_that_cannot_be_written_is_an_error_for_the_caller() {
        let t = TestConfig::new("unwritable");
        // a folder where the file should be: the replacing rename cannot succeed
        std::fs::create_dir_all(t.file().join("in-the-way")).unwrap();
        let c = Config { font_size: 17.0, ..Config::default() };
        let e = c.save().unwrap_err();
        assert!(e.starts_with("could not save the settings"), "{e}");
        assert!(t.file().is_dir(), "what was there is left alone");
        assert_eq!(std::fs::read_dir(t.file().parent().unwrap()).unwrap().count(), 1, "no temporary file left");
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
