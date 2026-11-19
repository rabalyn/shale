//! How long GnuPG's agent keeps a passphrase (MAIL-235, MAIL-236, MAIL-237, raba/shale#363).
//!
//! Without a `gpg-agent.conf` the agent forgets a passphrase 600 seconds after its last use and after 7200 seconds at
//! the latest, so the signing key has to be unlocked again and again. Shale never handles a passphrase (MAIL-171,
//! ADR-0011): this module only reads the two times with `gpgconf --list-options gpg-agent`, and, when the person
//! clicks twice, changes them with GnuPG's own `gpgconf --change-options gpg-agent`, which writes a marked block into
//! `gpg-agent.conf` and touches nothing else. `pinentry-program` is not a gpgconf option and is never touched.
//! Every call runs in a job with a time-out, never on the UI thread.

use crate::mail::gpg::{Gnupg, Run, SHORT};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

/// The two ways the cache times are changed by the choice "Ask for the passphrase again after".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Choice {
    #[default]
    Hours8,
    Day1,
    Week1,
}

impl Choice {
    pub const ALL: [Choice; 3] = [Choice::Hours8, Choice::Day1, Choice::Week1];

    pub fn seconds(self) -> u64 {
        match self {
            Choice::Hours8 => 8 * 3600,
            Choice::Day1 => 24 * 3600,
            Choice::Week1 => 7 * 24 * 3600,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Choice::Hours8 => "8 hours",
            Choice::Day1 => "1 day",
            Choice::Week1 => "1 week",
        }
    }

    /// A short word for the keys of the UI tests.
    pub fn key(self) -> &'static str {
        match self {
            Choice::Hours8 => "8h",
            Choice::Day1 => "1d",
            Choice::Week1 => "1w",
        }
    }
}

/// What the agent is set to now, and where its config file is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentInfo {
    /// Seconds after the last use (`default-cache-ttl`).
    pub default_ttl: u64,
    /// Seconds after the passphrase was first entered, at the latest (`max-cache-ttl`).
    pub max_ttl: u64,
    /// `no-allow-external-cache` is set: pinentry cannot save the passphrase in the system keyring.
    pub no_external_cache: bool,
    /// GnuPG's home as `gpgconf --list-dirs` says.
    pub homedir: PathBuf,
}

impl AgentInfo {
    /// The sentence for the Encryption section (MAIL-235).
    pub fn words(&self) -> String {
        format!("GnuPG asks for the passphrase again {} after the last use, at the latest {} after it was first entered.", duration_words(self.default_ttl), duration_words(self.max_ttl))
    }

    /// The warning that the keyring is blocked, if it is.
    pub fn external_cache_note(&self) -> Option<&'static str> {
        self.no_external_cache.then_some("no-allow-external-cache is set in gpg-agent.conf: the passphrase dialog cannot save the passphrase in the system keyring until that line is removed.")
    }
}

/// A time in words: the largest of weeks, days, hours and minutes that divides it exactly, else seconds.
pub fn duration_words(secs: u64) -> String {
    for (unit, size) in [("week", 7 * 24 * 3600), ("day", 24 * 3600), ("hour", 3600), ("minute", 60)] {
        if secs > 0 && secs.is_multiple_of(size) {
            let n = secs / size;
            return format!("{n} {unit}{}", if n == 1 { "" } else { "s" });
        }
    }
    format!("{secs} second{}", if secs == 1 { "" } else { "s" })
}

/// The cache times and the external-cache flag out of `gpgconf --list-options gpg-agent`: colon lines
/// `name:flags:level:description:type:alt-type:argname:default:argdef:value`; the value counts when there is one,
/// else the default. Lines that do not fit are skipped, and so is the description (it may be translated).
pub fn parse_options(text: &str) -> Result<(u64, u64, bool), String> {
    let (mut default_ttl, mut max_ttl, mut no_external) = (None, None, false);
    for line in text.lines() {
        let f: Vec<&str> = line.split(':').collect();
        if f.len() < 10 {
            continue;
        }
        let current = if f[9].is_empty() { f[7] } else { f[9] };
        match f[0] {
            "default-cache-ttl" => default_ttl = current.parse::<u64>().ok().or(default_ttl),
            "max-cache-ttl" => max_ttl = current.parse::<u64>().ok().or(max_ttl),
            "no-allow-external-cache" => no_external = !f[9].is_empty() && f[9] != "0",
            _ => {}
        }
    }
    match (default_ttl, max_ttl) {
        (Some(d), Some(m)) => Ok((d, m, no_external)),
        _ => Err("gpgconf did not list the cache times of gpg-agent".into()),
    }
}

/// `%3a`-style escapes of `gpgconf --list-dirs`.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            if let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The home out of `gpgconf --list-dirs` (the line `homedir:/home/x/.gnupg`).
pub fn parse_homedir(text: &str) -> Option<PathBuf> {
    text.lines().find_map(|l| l.strip_prefix("homedir:")).map(percent_decode).filter(|p| !p.is_empty()).map(PathBuf::from)
}

/// What gpgconf said when it failed, in a line.
fn failure(run: &Run, what: &str) -> String {
    let text = String::from_utf8_lossy(&run.stderr);
    let text = text.trim();
    if text.is_empty() {
        format!("{what} failed (exit code {})", run.code)
    } else {
        format!("{what} failed: {text}")
    }
}

fn gpgconf(g: &Gnupg, args: &[&str], input: &[u8], what: &str, cancel: &AtomicBool) -> Result<Run, String> {
    let run = g.run_gpgconf(args, input, SHORT, cancel).map_err(|e| format!("{what}: {e}"))?;
    if run.code == 0 {
        Ok(run)
    } else {
        Err(failure(&run, what))
    }
}

/// Ask the agent's settings (a job; MAIL-235): the cache times, the keyring flag and GnuPG's home.
pub fn read(g: &Gnupg, cancel: &AtomicBool) -> Result<AgentInfo, String> {
    let options = gpgconf(g, &["--list-options", "gpg-agent"], b"", "gpgconf --list-options gpg-agent", cancel)?;
    let (default_ttl, max_ttl, no_external_cache) = parse_options(&String::from_utf8_lossy(&options.stdout))?;
    let dirs = gpgconf(g, &["--list-dirs"], b"", "gpgconf --list-dirs", cancel)?;
    let homedir = parse_homedir(&String::from_utf8_lossy(&dirs.stdout)).ok_or("gpgconf did not say where GnuPG's home is")?;
    Ok(AgentInfo { default_ttl, max_ttl, no_external_cache, homedir })
}

/// What will be written, shown before anything is (MAIL-236).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub file: PathBuf,
    pub seconds: u64,
}

impl Plan {
    pub fn new(homedir: &Path, choice: Choice) -> Plan {
        Plan { file: homedir.join("gpg-agent.conf"), seconds: choice.seconds() }
    }

    /// The two lines as they appear in the file.
    pub fn lines(&self) -> String {
        manual_lines(self.seconds)
    }

    /// What gpgconf is given on stdin: `name:flags:value`.
    fn input(&self) -> String {
        format!("default-cache-ttl:0:{0}\nmax-cache-ttl:0:{0}\n", self.seconds)
    }

    /// The words shown before the click that writes.
    pub fn describe(&self) -> String {
        format!(
            "gpgconf will write these two lines into {}:\n\n{}\nIt puts them into a block marked ###+++--- GPGConf ---+++###, comments out earlier lines for these two options and changes nothing else in the file. Then the agent is reloaded (gpgconf --reload gpg-agent). Nothing is written until you click “Write with gpgconf”.",
            self.file.display(),
            self.lines()
        )
    }
}

/// The lines for `gpg-agent.conf` (MAIL-237).
pub fn manual_lines(seconds: u64) -> String {
    format!("default-cache-ttl {seconds}\nmax-cache-ttl {seconds}\n")
}

/// The command that makes the agent read its file again.
pub const RELOAD_COMMAND: &str = "gpgconf --reload gpg-agent";

/// Change the two cache times with gpgconf, then reload the agent (a job; MAIL-236). The error is gpgconf's message.
pub fn apply(g: &Gnupg, plan: &Plan, cancel: &AtomicBool) -> Result<(), String> {
    gpgconf(g, &["--change-options", "gpg-agent"], plan.input().as_bytes(), "gpgconf --change-options gpg-agent", cancel)?;
    gpgconf(g, &["--reload", "gpg-agent"], b"", "The lines were written, but gpgconf --reload gpg-agent", cancel)?;
    Ok(())
}

/// The platforms whose pinentry differs (MAIL-237).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Linux,
    Mac,
    Windows,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Mac
        } else {
            Os::Linux
        }
    }
}

/// How to keep the passphrase across restarts in the system keyring (MAIL-237).
pub fn keyring_tip(os: Os) -> &'static str {
    match os {
        Os::Linux => "Tick “Save in password manager” in the passphrase dialog (the Qt and GNOME pinentry have it): the passphrase then stays in the Secret Service (KWallet, GNOME Keyring) and survives restarts.",
        Os::Mac => "Tick “Save in Keychain” in the passphrase dialog (pinentry-mac): the passphrase then stays in your Keychain and survives restarts.",
        Os::Windows => "Gpg4win's passphrase dialog has no option to save the passphrase; use the cache times above.",
    }
}

/// The agent's settings as the Encryption section knows them (a small sub-state of `MailView`).
#[derive(Default)]
pub struct AgentState {
    /// None while it is being read; the error is shown in words.
    pub info: Option<Result<AgentInfo, String>>,
    pub reading: bool,
    pub writing: bool,
    /// How the last write went.
    pub written: Option<Result<(), String>>,
}

/// What the person is doing in the section (state of the dialog).
#[derive(Default)]
pub struct AgentDialog {
    pub choice: Choice,
    /// The plan is shown and waits for the second click.
    pub plan_open: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mail::gpg::testkit::{have_gnupg, no_cancel, TestHome};

    const LISTING: &str = "default-cache-ttl:24:0:expire cached PINs after N seconds:3:3:N:600::\n\
        default-cache-ttl-ssh:24:1:expire SSH keys after N seconds:3:3:N:1800::\n\
        max-cache-ttl:24:2:set maximum PIN cache lifetime to N seconds:3:3:N:7200::\n\
        no-allow-external-cache:8:0:disallow the use of an external password cache:0:0::::\n";

    // spec: MAIL-235
    #[test]
    fn mail_235_the_defaults_are_read_when_nothing_is_set() {
        assert_eq!(parse_options(LISTING), Ok((600, 7200, false)));
    }

    // spec: MAIL-235
    #[test]
    fn mail_235_set_values_win_and_the_external_cache_flag_is_seen() {
        let text = "default-cache-ttl:24:0:x:3:3:N:600::28800\nmax-cache-ttl:24:2:x:3:3:N:7200::86400\nno-allow-external-cache:8:0:x:0:0::::1\n";
        assert_eq!(parse_options(text), Ok((28800, 86400, true)));
    }

    // spec: MAIL-235
    #[test]
    fn mail_235_garbage_lines_are_skipped_and_missing_times_are_an_error() {
        let text = format!("nonsense\n\n::\ndefault-cache-ttl:24:0:x:3:3:N:abc::\n{LISTING}default-cache-ttl:1\n");
        assert_eq!(parse_options(&text), Ok((600, 7200, false)));
        assert!(parse_options("nonsense\nfoo:1:2:3:4:5:6:7:8:9\n").is_err());
        assert!(parse_options("").is_err());
    }

    // spec: MAIL-235
    #[test]
    fn mail_235_times_are_said_in_words() {
        for (secs, words) in
            [(1, "1 second"), (45, "45 seconds"), (60, "1 minute"), (600, "10 minutes"), (5400, "90 minutes"), (7200, "2 hours"), (28800, "8 hours"), (86400, "1 day"), (172800, "2 days"), (604800, "1 week"), (0, "0 seconds")]
        {
            assert_eq!(duration_words(secs), words);
        }
        let info = AgentInfo { default_ttl: 600, max_ttl: 7200, no_external_cache: false, homedir: PathBuf::from("/h") };
        assert_eq!(info.words(), "GnuPG asks for the passphrase again 10 minutes after the last use, at the latest 2 hours after it was first entered.");
        assert!(info.external_cache_note().is_none());
        let blocked = AgentInfo { no_external_cache: true, ..info };
        assert!(blocked.external_cache_note().unwrap().contains("no-allow-external-cache"));
    }

    // spec: MAIL-235
    #[test]
    fn mail_235_the_home_is_decoded_from_the_directory_listing() {
        assert_eq!(parse_homedir("sysconfdir:/etc\nhomedir:/home/x/.gnupg\nsocketdir:/run\n"), Some(PathBuf::from("/home/x/.gnupg")));
        assert_eq!(parse_homedir("homedir:C%3a/Users/x/AppData/Roaming/gnupg\n"), Some(PathBuf::from("C:/Users/x/AppData/Roaming/gnupg")));
        assert_eq!(parse_homedir("homedir:100%/odd\n"), Some(PathBuf::from("100%/odd")));
        assert_eq!(parse_homedir("sysconfdir:/etc\n"), None);
        assert_eq!(parse_homedir("homedir:\n"), None);
    }

    // spec: MAIL-236
    #[test]
    fn mail_236_the_plan_names_the_file_and_the_two_lines_and_what_gpgconf_does_with_the_rest() {
        let plan = Plan::new(Path::new("/home/x/.gnupg"), Choice::Day1);
        assert_eq!(plan.file, PathBuf::from("/home/x/.gnupg/gpg-agent.conf"));
        assert_eq!(plan.lines(), "default-cache-ttl 86400\nmax-cache-ttl 86400\n");
        assert_eq!(plan.input(), "default-cache-ttl:0:86400\nmax-cache-ttl:0:86400\n");
        let text = plan.describe();
        // the file is shown as `Path::join` builds it, with the separator of the platform
        let file = Path::new("/home/x/.gnupg").join("gpg-agent.conf").display().to_string();
        for part in [file.as_str(), "default-cache-ttl 86400", "max-cache-ttl 86400", "###+++--- GPGConf ---+++###", "comments out earlier lines", "changes nothing else", "gpgconf --reload gpg-agent"] {
            assert!(text.contains(part), "{part} in {text}");
        }
        assert_eq!([Choice::Hours8, Choice::Day1, Choice::Week1].map(|c| Plan::new(Path::new("/h"), c).seconds), [28800, 86400, 604800]);
        assert_eq!(Choice::ALL.map(Choice::label), ["8 hours", "1 day", "1 week"]);
    }

    // spec: MAIL-237
    #[test]
    fn mail_237_the_manual_lines_and_the_keyring_tip_of_each_platform() {
        assert_eq!(manual_lines(28800), "default-cache-ttl 28800\nmax-cache-ttl 28800\n");
        assert_eq!(RELOAD_COMMAND, "gpgconf --reload gpg-agent");
        assert!(keyring_tip(Os::Linux).contains("Save in password manager") && keyring_tip(Os::Linux).contains("Secret Service"));
        assert!(keyring_tip(Os::Mac).contains("Save in Keychain"));
        assert!(keyring_tip(Os::Windows).contains("no option") && keyring_tip(Os::Windows).contains("cache times"));
        assert!(matches!(Os::current(), Os::Linux | Os::Mac | Os::Windows));
    }

    // spec: MAIL-235, MAIL-236
    #[test]
    fn mail_236_apply_writes_a_marked_block_keeps_the_rest_and_the_times_read_back() {
        if !have_gnupg() {
            return;
        }
        let home = TestHome::new();
        let conf = home.dir.join("gpg-agent.conf");
        std::fs::write(&conf, "allow-loopback-pinentry\ndefault-cache-ttl 100\n").unwrap();
        let before = read(&home.g, &no_cancel()).unwrap();
        assert_eq!((before.default_ttl, before.max_ttl, before.no_external_cache), (100, 7200, false));
        assert_eq!(before.homedir.canonicalize().unwrap(), home.dir.canonicalize().unwrap());

        apply(&home.g, &Plan::new(&before.homedir, Choice::Hours8), &no_cancel()).unwrap();
        let text = std::fs::read_to_string(&conf).unwrap();
        assert!(text.contains("###+++--- GPGConf ---+++###") && text.contains("\ndefault-cache-ttl 28800\nmax-cache-ttl 28800\n"), "{text}");
        assert!(text.starts_with("allow-loopback-pinentry\n"), "the person's own line stays: {text}");
        assert!(text.contains("# default-cache-ttl 100"), "the older line is commented out, not deleted: {text}");
        let after = read(&home.g, &no_cancel()).unwrap();
        assert_eq!((after.default_ttl, after.max_ttl), (28800, 28800));

        // a line below the block is never touched by a later change
        std::fs::write(&conf, format!("{text}verbose\n")).unwrap();
        apply(&home.g, &Plan::new(&after.homedir, Choice::Week1), &no_cancel()).unwrap();
        let text = std::fs::read_to_string(&conf).unwrap();
        assert!(text.trim_end().ends_with("verbose"), "{text}");
        let again = read(&home.g, &no_cancel()).unwrap();
        assert_eq!((again.default_ttl, again.max_ttl), (604800, 604800));
    }

    // spec: MAIL-235, MAIL-236
    #[test]
    fn mail_236_the_flag_is_seen_and_gpgconf_failures_and_a_missing_gpgconf_are_said_in_words() {
        if !have_gnupg() {
            return;
        }
        let home = TestHome::new();
        std::fs::write(home.dir.join("gpg-agent.conf"), "no-allow-external-cache\n").unwrap();
        assert!(read(&home.g, &no_cancel()).unwrap().no_external_cache);
        // a home that is not there: gpgconf cannot write its file and its message comes back (not an unknown option
        // in the file, which GnuPG 2.2 of Debian bookworm, the CI image, accepts)
        let gone = Gnupg { home: Some(home.dir.join("gone")), ..home.g.clone() };
        let e = apply(&gone, &Plan::new(&home.dir, Choice::Day1), &no_cancel()).unwrap_err();
        assert!(e.starts_with("gpgconf --change-options gpg-agent failed"), "{e}");
        // no gpgconf at all
        let none = Gnupg { gpgconf: "/nonexistent/gpgconf".into(), ..home.g.clone() };
        let e = read(&none, &no_cancel()).unwrap_err();
        assert!(e.starts_with("gpgconf --list-options gpg-agent: ") && e.contains("gpgconf was not found"), "{e}");
    }
}
