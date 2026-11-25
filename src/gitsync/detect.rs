//! Where a vault is: on a network share or in a folder another app syncs (SYNC-059…SYNC-062,
//! ADR-0029, `docs/storage-on-shares.md`).
//!
//! The detection is a heuristic and only warns. It is a pure function of [`Inputs`] (the mount
//! table, the `statfs` magic number, the file system name of macOS, the drive type of Windows, the
//! environment, the `info.json` of Dropbox), plus the marker files that it looks for in the vault
//! and its parent folders, so every system's rules run on every system in tests. [`Inputs::real`]
//! reads the real ones; it touches the disk and belongs in a background job (JOB-001).

use std::path::{Component, Path, PathBuf};

/// The system whose rules apply.
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

/// The app that syncs a folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cloud {
    Dropbox,
    /// Nextcloud and ownCloud share their client's markers.
    Nextcloud,
    Syncthing,
    GoogleDrive,
    /// iCloud Drive.
    Apple,
    OneDrive,
    /// Another provider of `~/Library/CloudStorage` (by its folder's name).
    Other(String),
}

impl Cloud {
    pub fn name(&self) -> String {
        match self {
            Cloud::Dropbox => "Dropbox".into(),
            Cloud::Nextcloud => "Nextcloud or ownCloud".into(),
            Cloud::Syncthing => "Syncthing".into(),
            Cloud::GoogleDrive => "Google Drive".into(),
            Cloud::Apple => "iCloud Drive".into(),
            Cloud::OneDrive => "OneDrive".into(),
            Cloud::Other(n) => n.clone(),
        }
    }
}

/// What was found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A network file system: its name, and whether it surely is one (a plain `fuse` may be local).
    Network {
        fs: String,
        sure: bool,
    },
    Cloud(Cloud),
}

/// One finding: what it is and the folder or mount that shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub kind: Kind,
    pub at: PathBuf,
}

impl Finding {
    /// "a Dropbox folder", "a network folder (nfs4)".
    pub fn what(&self) -> String {
        match &self.kind {
            Kind::Network { fs, sure: true } => format!("a network folder ({fs})"),
            Kind::Network { fs, sure: false } => format!("maybe a network folder ({fs})"),
            Kind::Cloud(c) => format!("a {} folder", c.name()),
        }
    }

    pub fn is_network(&self) -> bool {
        matches!(self.kind, Kind::Network { .. })
    }
}

/// Everything the rules read from the system.
#[derive(Clone, Debug)]
pub struct Inputs {
    pub os: Os,
    pub home: Option<PathBuf>,
    /// `/proc/self/mountinfo` (Linux).
    pub mountinfo: Option<String>,
    /// `statfs` `f_type` of the vault (Linux), when the mount table did not say.
    pub magic: Option<u64>,
    /// `f_fstypename` of the vault (macOS).
    pub mac_fs: Option<String>,
    /// `GetDriveTypeW` of the vault's drive was `DRIVE_REMOTE` (Windows).
    pub drive_remote: Option<bool>,
    /// Environment variables (`OneDrive`, `OneDriveConsumer`, `OneDriveCommercial`).
    pub env: Vec<(String, String)>,
    /// The folders Dropbox names in its `info.json`.
    pub dropbox: Vec<PathBuf>,
}

impl Inputs {
    /// Nothing known: only the markers and the path rules can find something.
    pub fn none(os: Os) -> Inputs {
        Inputs { os, home: None, mountinfo: None, magic: None, mac_fs: None, drive_remote: None, env: vec![], dropbox: vec![] }
    }

    /// Read the real ones for `vault` (a folder that exists; touches the disk).
    pub fn real(vault: &Path) -> Inputs {
        let os = Os::current();
        let home = crate::userdirs::home_dir();
        let mut inp = Inputs::none(os);
        inp.env = ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"].iter().filter_map(|k| std::env::var(k).ok().map(|v| ((*k).to_string(), v))).collect();
        let mut infos: Vec<PathBuf> = vec![];
        if let Some(h) = &home {
            infos.push(h.join(".dropbox").join("info.json"));
        }
        for var in ["APPDATA", "LOCALAPPDATA"] {
            if let Some(d) = std::env::var_os(var) {
                infos.push(PathBuf::from(d).join("Dropbox").join("info.json"));
            }
        }
        inp.dropbox = infos.iter().filter_map(|p| crate::fsutil::read_to_string(p).ok()).flat_map(|t| dropbox_roots(&t)).collect();
        inp.home = home;
        let canon = vault.canonicalize().unwrap_or_else(|_| vault.to_path_buf());
        #[cfg(target_os = "linux")]
        {
            inp.mountinfo = crate::fsutil::read_to_string("/proc/self/mountinfo").ok();
            inp.magic = statfs_magic(&canon);
        }
        #[cfg(target_os = "macos")]
        {
            inp.mac_fs = mac_fs_name(&canon);
        }
        #[cfg(windows)]
        {
            inp.drive_remote = drive_is_remote(&canon);
        }
        inp
    }
}

/// `f_type` of the file system under `path` (Linux).
#[cfg(target_os = "linux")]
fn statfs_magic(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut s = std::mem::MaybeUninit::<libc::statfs>::zeroed();
    // SAFETY: `c` is a NUL-terminated path; `s` is a valid out pointer for one `statfs`.
    if unsafe { libc::statfs(c.as_ptr(), s.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: statfs returned 0, so it filled the whole struct.
    let s = unsafe { s.assume_init() };
    Some(s.f_type as u64 & 0xFFFF_FFFF)
}

/// `f_fstypename` of the file system under `path` (macOS).
#[cfg(target_os = "macos")]
fn mac_fs_name(path: &Path) -> Option<String> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut s = std::mem::MaybeUninit::<libc::statfs>::zeroed();
    // SAFETY: `c` is a NUL-terminated path; `s` is a valid out pointer for one `statfs`.
    if unsafe { libc::statfs(c.as_ptr(), s.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: statfs returned 0, so it filled the whole struct.
    let s = unsafe { s.assume_init() };
    let bytes: Vec<u8> = s.f_fstypename.iter().take_while(|c| **c != 0).map(|c| *c as u8).collect();
    String::from_utf8(bytes).ok()
}

/// Is the drive of `path` a remote one (Windows `GetDriveTypeW`; a mapped network drive is)?
#[cfg(windows)]
fn drive_is_remote(path: &Path) -> Option<bool> {
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
    // `DRIVE_REMOTE` of winbase.h: the value of a network drive, mapped or not
    const DRIVE_REMOTE: u32 = 4;
    let text = path.to_string_lossy().replace('/', "\\");
    let text = text.strip_prefix("\\\\?\\").unwrap_or(&text);
    let mut chars = text.chars();
    let (letter, colon) = (chars.next()?, chars.next()?);
    if !letter.is_ascii_alphabetic() || colon != ':' {
        return None;
    }
    let root: Vec<u16> = format!("{letter}:\\").encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `root` is a NUL-terminated UTF-16 string that lives through the call.
    Some(unsafe { GetDriveTypeW(root.as_ptr()) } == DRIVE_REMOTE)
}

/// The folders Dropbox's `info.json` names (`personal` and `business`).
pub fn dropbox_roots(json: &str) -> Vec<PathBuf> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return vec![] };
    let Some(o) = v.as_object() else { return vec![] };
    o.values().filter_map(|account| account.get("path")?.as_str().map(PathBuf::from)).collect()
}

// ---------------------------------------------------------------- network file systems

/// The network file systems of the Linux kernel, `/proc/self/mountinfo` names.
const NETWORK_FS: &[&str] = &[
    "nfs",
    "nfs4",
    "cifs",
    "smb3",
    "smbfs",
    "smb2",
    "ncpfs",
    "9p",
    "afs",
    "ceph",
    "glusterfs",
    "lustre",
    "davfs",
    "fuse.davfs2",
    "fuse.sshfs",
    "fuse.rclone",
    "fuse.gvfsd-fuse",
    "fuse.s3fs",
    "fuse.cryptomator",
    "fuse.glusterfs",
    "fuse.ceph-fuse",
    "fuse.curlftpfs",
];

/// Is this mount table file system (`nfs4`, `fuse.sshfs`) a network one? `Some(sure)`; None: local.
pub fn linux_fs(fs: &str) -> Option<bool> {
    let lower = fs.to_ascii_lowercase();
    if NETWORK_FS.contains(&lower.as_str()) {
        return Some(true);
    }
    // any other fuse file system may be a network one (or a local archive or an encrypted folder)
    (lower == "fuse" || lower.starts_with("fuse.")).then_some(false)
}

/// The same for the `statfs` magic number.
pub fn linux_magic(magic: u64) -> Option<(&'static str, bool)> {
    match magic {
        0x6969 => Some(("nfs", true)),
        0x517B => Some(("smb", true)),
        0xFF53_4D42 => Some(("cifs", true)),
        0xFE53_4D42 => Some(("smb2", true)),
        0x0102_1997 => Some(("9p", true)),
        0x5346_414F => Some(("afs", true)),
        0x00C3_6400 => Some(("ceph", true)),
        0x564C => Some(("ncpfs", true)),
        0x6573_5546 => Some(("fuse", false)),
        _ => None,
    }
}

/// macOS `f_fstypename`.
pub fn mac_fs_network(fs: &str) -> Option<bool> {
    match fs.to_ascii_lowercase().as_str() {
        "nfs" | "smbfs" | "afpfs" | "webdav" | "webdavfs" | "cifs" => Some(true),
        "osxfuse" | "macfuse" | "fusefs" => Some(false),
        _ => None,
    }
}

/// `\\server\share\…` and `\\?\UNC\server\share\…` are network paths; `\\?\C:\…` and `\\.\…` are not.
pub fn is_unc(path: &str) -> bool {
    let p = path.replace('/', "\\");
    if let Some(rest) = p.strip_prefix("\\\\?\\") {
        return rest.len() >= 4 && rest[..4].eq_ignore_ascii_case("UNC\\");
    }
    p.starts_with("\\\\") && !p.starts_with("\\\\.\\")
}

/// Undo the octal escapes of a mount table field (`\040` is a space).
fn unescape_mount(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 4 <= b.len() && b[i + 1..i + 4].iter().all(|c| (b'0'..=b'7').contains(c)) {
            let v = (b[i + 1] - b'0') as u32 * 64 + (b[i + 2] - b'0') as u32 * 8 + (b[i + 3] - b'0') as u32;
            out.push(v as u8);
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The (mount point, file system type) of the mount that holds `path`: the longest mount point
/// that is a parent of it.
pub fn mount_of(mountinfo: &str, path: &Path) -> Option<(PathBuf, String)> {
    let mut best: Option<(PathBuf, String)> = None;
    for line in mountinfo.lines() {
        let fields: Vec<&str> = line.split(' ').collect();
        let Some(dash) = fields.iter().position(|f| *f == "-") else { continue };
        let (Some(mp), Some(fs)) = (fields.get(4), fields.get(dash + 1)) else { continue };
        let mp = PathBuf::from(unescape_mount(mp));
        if path.starts_with(&mp) && best.as_ref().is_none_or(|(b, _)| mp.components().count() >= b.components().count()) {
            best = Some((mp, (*fs).to_string()));
        }
    }
    best
}

fn network(vault: &Path, inp: &Inputs) -> Option<Finding> {
    let text = vault.to_string_lossy();
    match inp.os {
        Os::Linux => {
            if let Some((mp, fs)) = inp.mountinfo.as_deref().and_then(|m| mount_of(m, vault)) {
                return linux_fs(&fs).map(|sure| Finding { kind: Kind::Network { fs, sure }, at: mp });
            }
            let (name, sure) = linux_magic(inp.magic?)?;
            Some(Finding { kind: Kind::Network { fs: name.into(), sure }, at: vault.to_path_buf() })
        }
        Os::Mac => {
            let fs = inp.mac_fs.as_deref()?;
            mac_fs_network(fs).map(|sure| Finding { kind: Kind::Network { fs: fs.to_string(), sure }, at: vault.to_path_buf() })
        }
        Os::Windows => {
            if is_unc(&text) {
                return Some(Finding { kind: Kind::Network { fs: "UNC path".into(), sure: true }, at: vault.to_path_buf() });
            }
            inp.drive_remote.filter(|r| *r).map(|_| Finding { kind: Kind::Network { fs: "network drive".into(), sure: true }, at: vault.to_path_buf() })
        }
    }
}

// ---------------------------------------------------------------- cloud folders

/// Is `path` inside `root`? Windows paths compare without letter case and with either separator.
fn under(path: &Path, root: &Path, os: Os) -> bool {
    let parts = |p: &Path| -> Vec<String> {
        let text = p.to_string_lossy().replace('\\', "/");
        let text = text.strip_prefix("//?/").unwrap_or(&text).to_string();
        let fold = |s: &str| if os == Os::Windows { s.to_lowercase() } else { s.to_string() };
        Path::new(&text).components().filter_map(|c| if let Component::Normal(n) = c { Some(fold(&n.to_string_lossy())) } else { None }).collect()
    };
    let (p, r) = (parts(path), parts(root));
    !r.is_empty() && p.starts_with(&r)
}

/// The app of a folder under `~/Library/CloudStorage`, named `<Provider>-<account>`.
fn cloud_storage_app(name: &str) -> Cloud {
    let provider = name.split('-').next().unwrap_or(name);
    match provider {
        "OneDrive" => Cloud::OneDrive,
        "GoogleDrive" => Cloud::GoogleDrive,
        "Dropbox" => Cloud::Dropbox,
        other => Cloud::Other(other.to_string()),
    }
}

/// Cloud folders known by their place.
fn clouds_by_place(vault: &Path, inp: &Inputs) -> Vec<Finding> {
    let mut out = vec![];
    let mut add = |c: Cloud, at: PathBuf| {
        if !out.iter().any(|f: &Finding| f.kind == Kind::Cloud(c.clone())) {
            out.push(Finding { kind: Kind::Cloud(c), at });
        }
    };
    for root in &inp.dropbox {
        if under(vault, root, inp.os) {
            add(Cloud::Dropbox, root.clone());
        }
    }
    if inp.os == Os::Mac {
        if let Some(h) = &inp.home {
            let icloud = h.join("Library").join("Mobile Documents");
            if under(vault, &icloud, inp.os) {
                add(Cloud::Apple, icloud);
            }
            let storage = h.join("Library").join("CloudStorage");
            if let Ok(rest) = vault.strip_prefix(&storage) {
                if let Some(Component::Normal(first)) = rest.components().next() {
                    add(cloud_storage_app(&first.to_string_lossy()), storage.join(first));
                }
            }
        }
    }
    if inp.os == Os::Windows {
        for (k, v) in &inp.env {
            if k.starts_with("OneDrive") && !v.is_empty() && under(vault, Path::new(v), inp.os) {
                add(Cloud::OneDrive, PathBuf::from(v));
            }
        }
    }
    out
}

/// What one folder's entry names say about the app that syncs it.
fn marker(name: &str) -> Option<Cloud> {
    let n = name.to_ascii_lowercase();
    match n.as_str() {
        ".dropbox" | ".dropbox.cache" => Some(Cloud::Dropbox),
        ".stfolder" => Some(Cloud::Syncthing),
        ".owncloudsync.log" | ".nextcloudsync.log" | ".csync_journal.db" => Some(Cloud::Nextcloud),
        ".tmp.driveupload" | ".tmp.drivedownload" => Some(Cloud::GoogleDrive),
        _ if n.starts_with(".sync_") && n.ends_with(".db") => Some(Cloud::Nextcloud),
        _ => None,
    }
}

/// The markers in `vault` and its parent folders, and iCloud's `.<name>.icloud` stubs in the vault.
fn clouds_by_marker(vault: &Path) -> Vec<Finding> {
    let mut out: Vec<Finding> = vec![];
    for (depth, dir) in vault.ancestors().enumerate() {
        if depth > 32 {
            break;
        }
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let found = marker(&name).or_else(|| (depth == 0 && name.starts_with('.') && name.to_ascii_lowercase().ends_with(".icloud")).then_some(Cloud::Apple));
            if let Some(c) = found {
                if !out.iter().any(|f| f.kind == Kind::Cloud(c.clone())) {
                    out.push(Finding { kind: Kind::Cloud(c), at: dir.to_path_buf() });
                }
            }
        }
    }
    out
}

/// Everything found for `vault`: the network file system first, then the cloud folders.
pub fn detect(vault: &Path, inp: &Inputs) -> Vec<Finding> {
    let mut out: Vec<Finding> = network(vault, inp).into_iter().collect();
    for f in clouds_by_place(vault, inp).into_iter().chain(clouds_by_marker(vault)) {
        if !out.iter().any(|o| o.kind == f.kind) {
            out.push(f);
        }
    }
    out
}

/// Detect with the real inputs (a background job).
pub fn detect_real(vault: &Path) -> Vec<Finding> {
    let canon = vault.canonicalize().unwrap_or_else(|_| vault.to_path_buf());
    detect(&canon, &Inputs::real(&canon))
}

/// The warning for the wizard and Settings → Sync (SYNC-061): what was found and what it means.
/// Empty findings: no text.
pub fn warning(findings: &[Finding]) -> Option<String> {
    if findings.is_empty() {
        return None;
    }
    let what: Vec<String> = findings.iter().map(Finding::what).collect();
    let mut text = format!("This vault looks like it is in {}. ", what.join(" and "));
    if findings.iter().any(|f| matches!(f.kind, Kind::Cloud(_))) {
        text.push_str("The notes travel through that app, which is fine; Shale keeps the history of your notes on this device, outside the folder. ");
    }
    if findings.iter().any(Finding::is_network) {
        text.push_str("On a network folder Shale can be slow, and another computer's change may show up late. ");
    }
    text.push_str("A git repository (a .git folder) does not belong in such a folder. This is a guess from the folder's marker files and file system; Shale only warns.");
    Some(text)
}

/// The warning for a vault that has `.git` inside such a folder (SYNC-062): the risk, and what to do.
pub fn repo_warning(findings: &[Finding], has_remote: bool) -> Option<String> {
    if findings.is_empty() {
        return None;
    }
    let what: Vec<String> = findings.iter().map(Finding::what).collect();
    let risk = format!("This vault has a .git folder inside {}. Sync programs and shares move a repository's files one by one and in any order, so the repository can end up half copied, forked or locked.", what.join(" and "));
    Some(if has_remote {
        format!("{risk} Your notes are kept safe by the sync with the server, so Shale offers no change here; if the repository breaks, clone it again from the server into a folder that no other program syncs.")
    } else {
        format!("{risk} Because there is no server, Shale can move the repository out of the folder, to a private place on this computer, and keep the history there.")
    })
}
