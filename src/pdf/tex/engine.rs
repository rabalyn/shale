//! Finding a LuaLaTeX to use (TEX-001): one found through `PATH`, or the managed TeX Live that
//! Shale installs itself (`install.rs`, TEX-002). Detection runs programs (a probe document, `biber`,
//! `kpsewhich`), so it belongs in a background job and never in a frame.

use super::compile::WorkDir;
use super::proc;
use serde::{Deserialize, Serialize};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Where the managed install gets its packages from when the settings say nothing else:
/// CTAN's mirror redirector, the repository that always has the newest TeX Live.
pub const DEFAULT_REPOSITORY: &str = "https://mirror.ctan.org/systems/texlive/tlnet";

/// The oldest LaTeX kernel Shale uses (the `pdfmanagement` of tuda-ci needs 2022-06-01), as a date.
pub const MIN_KERNEL: &str = "2022-06-01";

/// A probe or a package search that takes longer than this is given up.
const PROBE_TIMEOUT: Duration = Duration::from_secs(120);

/// Which TeX Shale uses (CFG-044).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TexChoice {
    /// The managed TeX Live when it is installed, else the one in `PATH` when it is good enough.
    #[default]
    Auto,
    /// The `lualatex` of `PATH`.
    System,
    /// The managed TeX Live.
    Managed,
}

impl TexChoice {
    pub const ALL: [TexChoice; 3] = [TexChoice::Auto, TexChoice::System, TexChoice::Managed];

    pub fn label(self) -> &'static str {
        match self {
            TexChoice::Auto => "Automatic",
            TexChoice::System => "The TeX of this computer",
            TexChoice::Managed => "The TeX Live that Shale installs",
        }
    }
}

/// The settings of the LaTeX export (CFG-044).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct TexConfig {
    pub engine: TexChoice,
    /// The TeX Live repository the managed install downloads from.
    pub repository: String,
}

impl Default for TexConfig {
    fn default() -> Self {
        TexConfig { engine: TexChoice::Auto, repository: DEFAULT_REPOSITORY.to_string() }
    }
}

/// Which kind of TeX an engine is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    System,
    Managed,
}

/// A TeX that can run: where its programs are, and what was found out about it.
#[derive(Clone, Debug)]
pub struct Engine {
    pub kind: Kind,
    /// The folder with `lualatex` and `biber`; empty: look in `PATH`.
    pub bin_dir: PathBuf,
    /// The date of its LaTeX kernel, `2025-11-01`.
    pub kernel: String,
    /// The TeX Live packages (of `assets/tex/packages.txt`) whose files it lacks.
    pub missing: Vec<String>,
    /// Tests: the programs run through this command (the TeX of the container, `tex-env.sh exec`).
    #[cfg(test)]
    pub wrapper: Vec<String>,
}

impl Engine {
    /// An engine that is not looked at yet.
    pub fn new(kind: Kind, bin_dir: PathBuf) -> Engine {
        Engine {
            kind,
            bin_dir,
            kernel: String::new(),
            missing: vec![],
            #[cfg(test)]
            wrapper: vec![],
        }
    }

    /// The command that runs one of the TeX programs (`lualatex`, `biber`, `kpsewhich`, `tlmgr`):
    /// no shell, and the engine's folder first in `PATH` so that programs find each other.
    pub fn command(&self, program: &str) -> Command {
        #[cfg(test)]
        if let Some((first, rest)) = self.wrapper.split_first() {
            let mut c = Command::new(first);
            c.args(rest).arg(program);
            return c;
        }
        let mut c = if self.bin_dir.as_os_str().is_empty() { crate::program::command(program) } else { crate::program::command(self.bin_dir.join(program)) };
        if !self.bin_dir.as_os_str().is_empty() {
            c.env("PATH", path_with(&self.bin_dir));
        }
        c
    }
}

/// `PATH` with `dir` in front.
pub fn path_with(dir: &Path) -> OsString {
    let old = std::env::var_os("PATH").unwrap_or_default();
    let all = std::iter::once(dir.to_path_buf()).chain(std::env::split_paths(&old));
    std::env::join_paths(all).unwrap_or(old)
}

/// What is looked for, to see whether a TeX has what the templates need: the TeX Live package
/// and one file it brings (`kpsewhich` finds it or not). Not every package of `packages.txt`:
/// the ones the templates and the fonts stand on.
pub const CHECKED: &[(&str, &str)] = &[
    ("koma-script", "scrartcl.cls"),
    ("pdfmanagement", "pdfmanagement-testphase.sty"),
    ("roboto", "Roboto-Regular.otf"),
    ("xcharter", "XCharter-Roman.otf"),
    ("fontspec", "fontspec.sty"),
    ("luaotfload", "luaotfload.sty"),
    ("unicode-math", "unicode-math.sty"),
    ("biblatex", "biblatex.sty"),
    ("csquotes", "csquotes.sty"),
    ("hyperref", "hyperref.sty"),
    ("microtype", "microtype.sty"),
    ("babel", "babel.sty"),
    ("xcolor", "xcolor.sty"),
    ("graphics", "graphicx.sty"),
    ("geometry", "geometry.sty"),
    ("tcolorbox", "tcolorbox.sty"),
    ("pgf", "tikz.sty"),
    ("beamer", "beamer.cls"),
    ("pdfx", "pdfx.sty"),
    // what the writer of the export adds to a document (TEX-008)
    ("ulem", "ulem.sty"),
    ("booktabs", "booktabs.sty"),
    ("amsmath", "amsmath.sty"),
    ("amsfonts", "amssymb.sty"),
    ("xltabular", "xltabular.sty"),
];

/// What detection found out (TEX-001).
#[derive(Clone, Debug)]
pub struct Detection {
    /// The `lualatex` of `PATH`, or why it is not usable.
    pub system: Result<Engine, String>,
    /// The managed TeX Live, or why it is not usable (usually: not installed).
    pub managed: Result<Engine, String>,
    /// The bytes the managed TeX Live takes on disk.
    pub managed_bytes: Option<u64>,
}

impl Detection {
    /// The engine the export uses, or why there is none.
    pub fn selected(&self, choice: TexChoice) -> Result<&Engine, String> {
        let managed = || self.managed.as_ref().map_err(|e| format!("the managed TeX Live: {e}"));
        let system = || self.system.as_ref().map_err(|e| format!("the TeX of this computer: {e}"));
        match choice {
            TexChoice::Managed => managed(),
            TexChoice::System => system(),
            TexChoice::Auto => match (&self.managed, &self.system) {
                (Ok(m), _) => Ok(m),
                (_, Ok(s)) if s.missing.is_empty() => Ok(s),
                _ => Err(format!("{}; {}", self.managed.as_ref().err().map_or_else(|| "?".to_string(), |e| format!("the managed TeX Live: {e}")), self.system_reason())),
            },
        }
    }

    /// Why the TeX of this computer is not (fully) usable, in a line; empty when it is.
    pub fn system_reason(&self) -> String {
        match &self.system {
            Err(e) => format!("the TeX of this computer: {e}"),
            Ok(s) if !s.missing.is_empty() => format!("the TeX of this computer misses {}; use the managed one", s.missing.join(", ")),
            Ok(_) => String::new(),
        }
    }
}

/// Look for a TeX: the managed one under `data` (the user's data folder) and the one in
/// `path_var`, which is `PATH`. Runs programs; call it from a job.
pub fn detect(data: Option<&Path>, path_var: Option<&OsStr>) -> Detection {
    let managed_root = data.map(managed_root);
    let managed = match managed_root.as_deref() {
        None => Err("there is no data folder to keep it in".to_string()),
        Some(root) => match managed_bin_dir(root) {
            None => Err("not installed".to_string()),
            Some(bin) => probe(Engine::new(Kind::Managed, bin)),
        },
    };
    let system = match find_in_path("lualatex", path_var) {
        None => Err("no lualatex in PATH".to_string()),
        Some(dir) => probe(Engine::new(Kind::System, dir)),
    };
    let managed_bytes = managed.as_ref().ok().and(managed_root.as_deref()).map(folder_bytes);
    Detection { system, managed, managed_bytes }
}

/// Where the managed TeX Live lives below the user's data folder (`<data>/shale/texlive`).
pub fn managed_root(data: &Path) -> PathBuf {
    data.join("shale").join("texlive")
}

/// Where it is installed to first (`<data>/shale/texlive.partial`), and renamed from on success.
pub fn partial_root(data: &Path) -> PathBuf {
    data.join("shale").join("texlive.partial")
}

/// The name of a program file (`lualatex.exe` on Windows).
pub fn exe_name(program: &str) -> String {
    format!("{program}{}", std::env::consts::EXE_SUFFIX)
}

/// The folder with the programs of a TeX Live tree: the `bin/<platform>` that has a `lualatex`.
pub fn managed_bin_dir(root: &Path) -> Option<PathBuf> {
    bin_dir_with(root, &exe_name("lualatex"))
}

/// The `bin/<platform>` folder of a TeX Live tree that has this file (right after `install-tl`
/// it has `tlmgr` but no `lualatex` yet).
pub fn bin_dir_with(root: &Path, file: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root.join("bin")).ok()?.filter_map(|e| Some(e.ok()?.path())).filter(|p| p.is_dir()).collect();
    dirs.sort();
    dirs.into_iter().find(|d| d.join(file).is_file())
}

/// The folder in `path_var` (a `PATH`) that has this program.
pub fn find_in_path(program: &str, path_var: Option<&OsStr>) -> Option<PathBuf> {
    let name = exe_name(program);
    std::env::split_paths(path_var?).filter(|d| !d.as_os_str().is_empty()).find(|d| d.join(&name).is_file())
}

/// The size of all files below a folder; links are not followed.
pub(crate) fn folder_bytes(dir: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let Ok(t) = e.file_type() else { continue };
            if t.is_dir() {
                stack.push(e.path());
            } else if t.is_file() {
                total += e.metadata().map_or(0, |m| m.len());
            }
        }
    }
    total
}

/// The kernel date in the output of the probe (`SHALE-KERNEL 2025-11-01`), normalised to `YYYY-MM-DD`.
pub fn parse_kernel(output: &str) -> Option<String> {
    let rest = output.lines().find_map(|l| l.split_once("SHALE-KERNEL ").map(|(_, r)| r.trim().to_string()))?;
    let date: String = rest.split_whitespace().next()?.replace('/', "-");
    let ok = date.len() == 10 && date.char_indices().all(|(i, c)| if i == 4 || i == 7 { c == '-' } else { c.is_ascii_digit() });
    ok.then_some(date)
}

/// The packages of [`CHECKED`] whose file is not in the list of paths `kpsewhich` printed.
pub fn missing_packages(kpsewhich_output: &str) -> Vec<String> {
    let found: Vec<&str> = kpsewhich_output.lines().filter_map(|l| l.trim().rsplit(['/', '\\']).next()).collect();
    CHECKED.iter().filter(|(_, file)| !found.contains(file)).map(|(pkg, _)| pkg.to_string()).collect()
}

/// Look at a TeX: its kernel date (a probe document), `biber`, and the packages the templates
/// need. `Err` says in words why it cannot be used; missing packages are not an error here.
pub fn probe(mut engine: Engine) -> Result<Engine, String> {
    // the kernel: a one-line document in a folder of its own
    let work = WorkDir::new().map_err(|e| format!("no temporary folder ({e})"))?;
    let mut cmd = engine.command("lualatex");
    cmd.args(["-interaction=nonstopmode", "-halt-on-error", "-no-shell-escape", r"\typeout{SHALE-KERNEL \fmtversion}\stop"]).current_dir(work.path());
    let run = proc::run(cmd, PROBE_TIMEOUT, None, |_| {}).map_err(|e| format!("lualatex could not be started ({e})"))?;
    let kernel = parse_kernel(&run.tail_text()).ok_or_else(|| format!("lualatex did not tell its LaTeX kernel ({})", run.tail.last().map_or("no output", String::as_str)))?;
    if kernel.as_str() < MIN_KERNEL {
        return Err(format!("its LaTeX kernel is from {kernel}, Shale needs {MIN_KERNEL} or newer"));
    }
    engine.kernel = kernel;
    // biber next to it
    let mut biber = engine.command("biber");
    biber.arg("--version").current_dir(work.path());
    match proc::run(biber, PROBE_TIMEOUT, None, |_| {}) {
        Ok(r) if r.success() => {}
        _ => return Err("biber was not found next to lualatex".to_string()),
    }
    // the packages
    let mut kpse = engine.command("kpsewhich");
    kpse.args(CHECKED.iter().map(|(_, file)| file)).current_dir(work.path());
    let run = proc::run(kpse, PROBE_TIMEOUT, None, |_| {}).map_err(|e| format!("kpsewhich could not be started ({e})"))?;
    engine.missing = missing_packages(&run.tail_text());
    Ok(engine)
}
