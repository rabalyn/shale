//! Media files in Git LFS (ADR-0015, SYNC-016…SYNC-019, SYNC-021, SYNC-022).
//!
//! Shale sets Git LFS up in the vault's own repository (`git lfs install --local --skip-smudge`,
//! never the global configuration) and routes the media extensions to it with a block in
//! `.gitattributes`; every other line of that file belongs to the user. History is never rewritten.

use super::*;

/// First line of Shale's block in `.gitattributes`.
pub const BEGIN: &str = "# shale-media-begin";
/// Last line of Shale's block in `.gitattributes`.
pub const END: &str = "# shale-media-end";
/// The audio and video extensions of MEDIA-001, in lower case (the block lists upper case too).
pub const MEDIA_EXTENSIONS: [&str; 13] = ["mp3", "ogg", "oga", "opus", "flac", "m4a", "aac", "wav", "mp4", "m4v", "mov", "webm", "mkv"];
const ATTRIBUTES: &str = ".gitattributes";
const POINTER_VERSION: &str = "version https://git-lfs.github.com/spec/v1";
/// Bigger local objects are left as pointer files when a conflict copy is written.
const MAX_EXPAND: u64 = 512 * 1024 * 1024;

/// What the sync settings show (SYNC-016).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    /// The version line of `git lfs version`; None: git-lfs is not installed.
    pub version: Option<String>,
    /// This repository sends media to LFS: Shale's block is in `.gitattributes` and the filter is set.
    pub stores_media: bool,
}

/// Where to get git-lfs (BUILD-018).
pub const INSTALL_HINT: &str = "Install it from https://git-lfs.com (or with your package manager: the package is called git-lfs).";

fn missing() -> String {
    format!("Git LFS is not installed. {INSTALL_HINT}")
}

impl Git {
    /// The installed git-lfs' version line, or None without it (SYNC-016).
    pub fn lfs_version(&self) -> Option<String> {
        let o = self.run(Path::new("."), args!["lfs", "version"]).ok()?;
        (o.ok && !o.text().is_empty()).then(|| o.text())
    }
}

/// Whether git-lfs is installed and whether this vault stores media with it (SYNC-016).
pub fn status(git: &Git, vault: &Path) -> Status {
    Status { version: git.lfs_version(), stores_media: is_repo(vault) && configured(git, vault) && read_attributes(vault).is_ok_and(|a| has_block(&a)) }
}

/// The Git LFS filter is set up for this repository (by `git lfs install`, in any config scope).
pub fn configured(git: &Git, vault: &Path) -> bool {
    git.run(vault, args!["config", "--get", "filter.lfs.clean"]).is_ok_and(|o| o.ok && !o.text().is_empty())
}

/// The patterns Shale routes to LFS: `*.mp3`, `*.MP3`, ….
pub fn patterns() -> Vec<String> {
    MEDIA_EXTENSIONS.iter().flat_map(|e| [format!("*.{e}"), format!("*.{}", e.to_ascii_uppercase())]).collect()
}

/// Shale's block, ready to write.
fn block() -> String {
    let mut b = format!("{BEGIN}\n");
    for p in patterns() {
        b.push_str(&format!("{p} filter=lfs diff=lfs merge=lfs -text\n"));
    }
    b.push_str(END);
    b.push('\n');
    b
}

/// Where Shale's block starts and ends (line numbers), if it is there.
fn block_lines(lines: &[&str]) -> Result<Option<(usize, usize)>, String> {
    let Some(b) = lines.iter().position(|l| l.trim_end() == BEGIN) else { return Ok(None) };
    match lines[b..].iter().position(|l| l.trim_end() == END) {
        Some(e) => Ok(Some((b, b + e))),
        None => Err(format!("{ATTRIBUTES} has “{BEGIN}” but no “{END}”; fix or remove that line and try again")),
    }
}

pub(super) fn has_block(attributes: &str) -> bool {
    let lines: Vec<&str> = attributes.split_inclusive('\n').collect();
    matches!(block_lines(&lines), Ok(Some(_)))
}

/// The text of `.gitattributes` with Shale's block in it: replaced where it is, appended when it
/// is not there; every line outside it stays as it is.
pub(super) fn with_block(existing: &str) -> Result<String, String> {
    let lines: Vec<&str> = existing.split_inclusive('\n').collect();
    Ok(match block_lines(&lines)? {
        Some((b, e)) => format!("{}{}{}", lines[..b].concat(), block(), lines[e + 1..].concat()),
        None if existing.is_empty() => block(),
        None if existing.ends_with('\n') => format!("{existing}\n{}", block()),
        None => format!("{existing}\n\n{}", block()),
    })
}

/// A `.gitattributes` text that sends paths to LFS.
pub(super) fn uses_lfs(attributes: &str) -> bool {
    attributes.lines().any(|l| !l.trim_start().starts_with('#') && l.contains("filter=lfs"))
}

/// The text of the vault's `.gitattributes` (empty when there is none).
fn read_attributes(vault: &Path) -> Result<String, String> {
    match crate::fsutil::read_to_string(vault.join(ATTRIBUTES)) {
        Ok(t) => Ok(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(format!("{ATTRIBUTES} could not be read: {e}")),
    }
}

/// `git lfs install --local --skip-smudge`: filter and hooks in this repository only. A hook the
/// user wrote themselves makes git-lfs complain after it set the filter; the filter is what
/// counts here, because the sync pushes the files itself (SYNC-021).
fn install_local(git: &Git, vault: &Path) -> Result<(), String> {
    let o = git.run(vault, args!["lfs", "install", "--local", "--skip-smudge"])?;
    if o.ok || configured(git, vault) {
        Ok(())
    } else {
        Err(format!("git lfs install: {}", o.stderr))
    }
}

/// A new repository: filter and `.gitattributes`, nothing to convert and nothing to commit (the
/// first sync commits the file). Nothing happens without git-lfs (SYNC-017).
pub(super) fn prepare_new(git: &Git, vault: &Path) -> Result<(), String> {
    if git.lfs_version().is_none() {
        return Ok(());
    }
    install_local(git, vault)?;
    write_block(vault)
}

fn write_block(vault: &Path) -> Result<(), String> {
    let text = with_block(&read_attributes(vault)?)?;
    crate::fsutil::write_atomic(&vault.join(ATTRIBUTES), text.as_bytes()).map_err(|e| format!("{ATTRIBUTES} could not be written: {e}"))
}

/// Store media with Git LFS in this repository (SYNC-017, SYNC-018): filter, block, existing
/// media converted in one commit. Returns the number of files in that commit (0: nothing to do).
pub fn setup(git: &Git, vault: &Path, opts: &Options, tree: &TreeLock) -> Result<usize, String> {
    if !is_repo(vault) {
        return Err("The vault is not a git repository".into());
    }
    let _tree = tree.hold();
    ensure_idle(git, vault)?;
    if git.lfs_version().is_none() {
        return Err(missing());
    }
    let ident = identity(git, vault)?;
    install_local(git, vault)?;
    write_block(vault)?;
    git.ok(vault, args!["add", "--", ATTRIBUTES])?;
    // only tracked files can be converted (a pattern without a match would make `git add` fail);
    // files not tracked yet are cleaned by the next sync's `git add`
    let pats = patterns();
    let mut ls: Vec<&OsStr> = vec![OsStr::new("ls-files"), OsStr::new("-z"), OsStr::new("--")];
    ls.extend(pats.iter().map(OsStr::new));
    let listing = git.ok(vault, &ls)?;
    let tracked: Vec<String> = listing.stdout.split(|b| *b == 0).filter(|s| !s.is_empty()).map(|p| String::from_utf8_lossy(p).into_owned()).collect();
    for chunk in tracked.chunks(100) {
        let specs: Vec<String> = chunk.iter().map(|p| format!(":(literal){p}")).collect();
        let mut a: Vec<&OsStr> = vec![OsStr::new("add"), OsStr::new("--renormalize"), OsStr::new("--")];
        a.extend(specs.iter().map(OsStr::new));
        git.ok(vault, &a)?;
    }
    let staged = git.ok(vault, args!["diff", "--cached", "--name-only", "-z"])?.stdout.split(|b| *b == 0).filter(|s| !s.is_empty()).count();
    if staged > 0 {
        let msg = opts.message("Store media files with Git LFS");
        commit(git, vault, &ident, &["commit", "--quiet", "-m", &msg])?;
    }
    Ok(staged)
}

/// The guard of every sync (SYNC-019): when `.gitattributes` sends paths to LFS, the repository
/// is set up for it, or, without git-lfs, the sync stops before it commits anything, so a media
/// file never lands in the history as a plain file.
pub(super) fn guard(git: &Git, vault: &Path) -> Result<(), String> {
    if !uses_lfs(&read_attributes(vault)?) {
        return Ok(());
    }
    if git.lfs_version().is_none() {
        return Err(format!(
            "This vault stores files with Git LFS (see {ATTRIBUTES}), but Git LFS is not installed on this computer. Nothing was committed, so that no media file lands in the history as a plain file. {INSTALL_HINT} Then sync again."
        ));
    }
    if !configured(git, vault) {
        install_local(git, vault)?;
    }
    Ok(())
}

/// This repository sends files to LFS, so the sync uploads them before it pushes (SYNC-021).
pub(super) fn in_use(vault: &Path) -> bool {
    read_attributes(vault).is_ok_and(|a| uses_lfs(&a))
}

/// `git lfs push origin <branch>`: explicit, so it works with git hooks turned off.
pub(super) fn push(git: &Git, vault: &Path, branch: &str) -> Result<(), String> {
    git.ok(vault, args!["lfs", "push", REMOTE, format!("refs/heads/{branch}")]).map(|_| ())
}

/// What an LFS pointer file says.
#[derive(Debug, PartialEq)]
pub struct Pointer {
    pub oid: String,
    pub size: u64,
}

/// The pointer a file holds, if it is one.
pub fn parse_pointer(bytes: &[u8]) -> Option<Pointer> {
    if bytes.len() > 1024 {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let mut lines = text.lines();
    if lines.next()? != POINTER_VERSION {
        return None;
    }
    let (mut oid, mut size) = (None, None);
    for l in lines {
        if let Some(h) = l.strip_prefix("oid sha256:") {
            oid = Some(h.to_string()).filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()));
        } else if let Some(s) = l.strip_prefix("size ") {
            size = s.parse().ok();
        }
    }
    Some(Pointer { oid: oid?, size: size? })
}

/// For a conflict copy (SYNC-022): the real file when `blob` is a pointer whose object this
/// repository holds, so the copy is playable at once; `git add` turns it back into the same
/// pointer. Anything else (not a pointer, object not here, too big) stays as it is, so a pointer
/// never becomes a broken file and raw media never bypasses LFS.
pub(super) fn expand(git: &Git, vault: &Path, blob: Vec<u8>) -> Vec<u8> {
    let Some(p) = parse_pointer(&blob).filter(|p| p.size <= MAX_EXPAND) else { return blob };
    let Ok(common) = git.run(vault, args!["rev-parse", "--git-common-dir"]) else { return blob };
    let object = vault.join(common.text()).join("lfs").join("objects").join(&p.oid[..2]).join(&p.oid[2..4]).join(&p.oid);
    match crate::fsutil::read(&object) {
        Ok(real) if real.len() as u64 == p.size => real,
        _ => blob,
    }
}
