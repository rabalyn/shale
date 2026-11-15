//! A generator for Thunderbird profile folders, for the tests of `tb_store` (DEV-004): a throwaway
//! tree with `profiles.ini`, `prefs.js` (accounts and servers), mbox folders (`Inbox.sbd/…` for
//! nested ones) or maildir folders, `.msf` indexes, non-ASCII folder names, messages with
//! `X-Mozilla-Status` flags (also "deleted, not compacted yet"), large messages and broken files.
//!
//! ```ignore
//! let fx = TbFixture::new("name");
//! let local = fx.account("server1", Kind::Local, "Local Folders", "nobody", "Mail/Local Folders");
//! let (file, at) = fx.mbox(&local, &["Inbox", "Projekte"], &[Msg::new("Erste", "eins")]);
//! // fx.root is the folder to hand to `tb_store::find_in(&[fx.root.clone()], …)`
//! ```
//! The folder is removed when the fixture is dropped.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// What `mail.server.<id>.type` says.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Kind {
    /// "Local Folders" (`none`)
    Local,
    Pop,
    Imap,
}

impl Kind {
    fn pref(self) -> &'static str {
        match self {
            Kind::Local => "none",
            Kind::Pop => "pop3",
            Kind::Imap => "imap",
        }
    }
}

/// One message of a folder.
#[derive(Clone)]
pub(crate) struct Msg {
    /// as written in the header (may be an encoded word)
    pub subject: String,
    pub id: String,
    pub body: String,
    /// `X-Mozilla-Status` (0x0001 read, 0x0002 replied, 0x0008 deleted, 0x0010 flagged …)
    pub status: u16,
}

impl Msg {
    pub fn new(subject: &str, body: &str) -> Msg {
        let id = format!("{}-{}@fixture.test", subject.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>(), NEXT.fetch_add(1, Ordering::Relaxed));
        Msg { subject: subject.into(), id, body: body.into(), status: 0x0001 }
    }

    pub fn status(mut self, s: u16) -> Msg {
        self.status = s;
        self
    }

    pub fn id(mut self, id: &str) -> Msg {
        self.id = id.into();
        self
    }

    /// The message as Thunderbird writes it into an mbox file (with its "From " line and the
    /// blank line after it). Lines of the body that start with "From " are escaped.
    pub fn mbox_text(&self) -> String {
        let body: String = self.body.split_inclusive('\n').map(|l| if l.starts_with("From ") { format!(">{l}") } else { l.to_string() }).collect();
        format!(
            "From - Thu Nov 12 10:00:00 2026\nX-Mozilla-Status: {:04x}\nX-Mozilla-Status2: 00000000\nSubject: {}\nMessage-ID: <{}>\nFrom: Ada <ada@uni.example>\nDate: Thu, 12 Nov 2026 10:00:00 +0100\n\n{body}\n\n",
            self.status, self.subject, self.id
        )
    }

    /// The same as a maildir file (no "From " line).
    pub fn maildir_text(&self) -> String {
        let t = self.mbox_text();
        let rest = t.split_once('\n').map(|x| x.1).unwrap_or("");
        rest.trim_end().to_string() + "\n"
    }
}

pub(crate) struct TbFixture {
    /// what `find_in` gets as a root (the `.thunderbird` folder)
    pub root: PathBuf,
    /// the main profile folder inside it
    pub profile: PathBuf,
}

/// Mork text values: `)`, `\` and `$` are escaped.
fn mork_escape(s: &str) -> String {
    s.chars().flat_map(|c| if matches!(c, ')' | '\\' | '$') { vec!['\\', c] } else { vec![c] }).collect()
}

impl TbFixture {
    /// A root with `profiles.ini` and one profile "abc12345.default-release" with an empty `prefs.js`.
    pub fn new(tag: &str) -> TbFixture {
        let root = std::env::temp_dir().join(format!("shale-tbfix-{tag}-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let profile = root.join("abc12345.default-release");
        let fx = TbFixture { root, profile };
        std::fs::write(fx.root.join("profiles.ini"), "[General]\nStartWithLastProfile=1\n\n[Profile0]\nName=default-release\nIsRelative=1\nPath=abc12345.default-release\nDefault=1\n").unwrap();
        fx.add_profile("abc12345.default-release");
        fx
    }

    /// Another profile folder next to the first (with an empty `prefs.js`).
    pub fn add_profile(&self, name: &str) -> PathBuf {
        let p = self.root.join(name);
        std::fs::create_dir_all(&p).unwrap();
        let prefs = p.join("prefs.js");
        if !prefs.exists() {
            std::fs::write(prefs, "// Mozilla User Preferences\n\nuser_pref(\"app.update.lastUpdateTime.background-update-timer\", 1);\n").unwrap();
        }
        p
    }

    /// Add an account to the main profile's `prefs.js` and make its folder (`dir_rel` below the
    /// profile). Returns the folder.
    pub fn account(&self, id: &str, kind: Kind, host: &str, user: &str, dir_rel: &str) -> PathBuf {
        self.account_in(&self.profile, id, kind, host, user, dir_rel)
    }

    pub fn account_in(&self, profile: &Path, id: &str, kind: Kind, host: &str, user: &str, dir_rel: &str) -> PathBuf {
        let dir = profile.join(dir_rel);
        std::fs::create_dir_all(&dir).unwrap();
        let mut prefs = std::fs::read_to_string(profile.join("prefs.js")).unwrap_or_default();
        prefs.push_str(&format!(
            "user_pref(\"mail.server.{id}.hostname\", \"{host}\");\nuser_pref(\"mail.server.{id}.type\", \"{}\");\nuser_pref(\"mail.server.{id}.userName\", \"{user}\");\nuser_pref(\"mail.server.{id}.directory-rel\", \"[ProfD]{dir_rel}\");\nuser_pref(\"mail.account.account1.server\", \"{id}\");\nuser_pref(\"mail.identity.id1.useremail\", \"{user}@{host}\");\n",
            kind.pref()
        ));
        std::fs::write(profile.join("prefs.js"), prefs).unwrap();
        dir
    }

    /// Append a raw line to the main profile's `prefs.js`.
    pub fn prefs_line(&self, line: &str) {
        let mut p = std::fs::read_to_string(self.profile.join("prefs.js")).unwrap();
        p.push_str(line);
        p.push('\n');
        std::fs::write(self.profile.join("prefs.js"), p).unwrap();
    }

    /// The path of a folder in an account: `["Inbox", "Projekte"]` is `Inbox.sbd/Projekte`
    /// (the parents are made, as empty mbox files with their `.sbd` folder).
    pub fn folder_path(&self, account: &Path, folder: &[&str]) -> PathBuf {
        let (last, parents) = folder.split_last().unwrap();
        let mut p = account.to_path_buf();
        std::fs::create_dir_all(&p).unwrap();
        for f in parents {
            let file = p.join(f);
            if !file.exists() {
                std::fs::create_dir_all(&p).unwrap();
                std::fs::write(&file, "").unwrap();
            }
            p = p.join(format!("{f}.sbd"));
            std::fs::create_dir_all(&p).unwrap();
        }
        p.join(last)
    }

    /// An mbox folder with these messages. Returns its file and the byte position of each message
    /// (what a `mailbox-message://…#position` link and the index's `storeToken` hold).
    pub fn mbox(&self, account: &Path, folder: &[&str], msgs: &[Msg]) -> (PathBuf, Vec<u64>) {
        let path = self.folder_path(account, folder);
        let mut text = String::new();
        let mut at = Vec::new();
        for m in msgs {
            at.push(text.len() as u64);
            text.push_str(&m.mbox_text());
        }
        std::fs::write(&path, text).unwrap();
        (path, at)
    }

    /// A maildir folder (`cur/`, `new/`, `tmp/`) with these messages in `cur/`, named
    /// `1700000000.<n>.fixture`. Returns its folder and the file names.
    pub fn maildir(&self, account: &Path, folder: &[&str], msgs: &[Msg]) -> (PathBuf, Vec<String>) {
        let dir = self.folder_path(account, folder);
        for s in ["cur", "new", "tmp"] {
            std::fs::create_dir_all(dir.join(s)).unwrap();
        }
        let mut names = Vec::new();
        for (i, m) in msgs.iter().enumerate() {
            let name = format!("1700000000.{i}.fixture");
            std::fs::write(dir.join("cur").join(&name), m.maildir_text()).unwrap();
            names.push(name);
        }
        (dir, names)
    }

    /// The `.msf` index next to a folder: one row per `(key, subject, message-id, storeToken)`.
    pub fn index(&self, folder: &Path, rows: &[(u64, &str, &str, &str)]) -> PathBuf {
        let mut text = String::from("// <!-- <mdb:mork:z v=\"1.4\"/> -->\n< <(a=c)> // (f=iso-8859-1)\n  (81=subject)(82=message-id)(83=storeToken)>\n{1:^80 {(k^80:c)(s=9)}\n");
        for (key, subject, id, token) in rows {
            text.push_str(&format!("  [{key:X}(^81={})(^82={})(^83={})]\n", mork_escape(subject), mork_escape(id), mork_escape(token)));
        }
        text.push_str("}\n");
        let mut name = folder.file_name().unwrap().to_os_string();
        name.push(".msf");
        let path = folder.with_file_name(name);
        std::fs::write(&path, text).unwrap();
        path
    }
}

impl Drop for TbFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
