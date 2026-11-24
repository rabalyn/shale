//! Compiling a document with LuaLaTeX and biber (TEX-004): in a folder of its own, without a
//! shell, with a timeout per run, as often as the log asks for (like `latexmk`, at most
//! [`MAX_RUNS`] runs of LuaLaTeX), and with the log turned into readable errors.

use super::engine::Engine;
use super::fonts::{self, FontOverride};
use super::proc::{self, Ending};
use std::collections::hash_map::DefaultHasher;
use std::ffi::OsString;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// The most runs of LuaLaTeX for one document.
pub const MAX_RUNS: u32 = 4;

/// How long one run of LuaLaTeX or biber may take.
pub const RUN_TIMEOUT: Duration = Duration::from_secs(120);

/// How many lines of the log a failure carries.
const TAIL_LINES: usize = 40;

#[cfg(test)]
static NEXT_IN: AtomicU64 = AtomicU64::new(0);

/// A temporary folder for one compile: private to the user, removed when it is dropped, so it is
/// also removed after a failure.
pub struct WorkDir(PathBuf);

impl WorkDir {
    pub fn new() -> std::io::Result<WorkDir> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = crate::fsutil::private_temp_root("shale-tex")?;
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
        let dir = root.join(format!("{}-{}-{nanos}", std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst)));
        crate::fsutil::create_private_dir_new(&dir)?;
        Ok(WorkDir(dir))
    }

    /// Tests: a work folder below `parent` (whose name may have spaces).
    #[cfg(test)]
    pub fn new_in(parent: &Path) -> std::io::Result<WorkDir> {
        std::fs::create_dir_all(parent)?;
        let dir = parent.join(format!("work-{}", NEXT_IN.fetch_add(1, Ordering::SeqCst)));
        crate::fsutil::create_private_dir_new(&dir)?;
        Ok(WorkDir(dir))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Put a file into the folder (a name without folders; an existing file is not replaced).
    pub fn write(&self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        crate::fsutil::create_new(&self.0.join(name), bytes)
    }

    /// Put new content into a file that is there.
    fn replace(&self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        crate::fsutil::write_atomic(&self.0.join(name), bytes)
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// How to compile.
#[derive(Clone, Debug)]
pub struct Options {
    /// Folders with the template files (classes, packages, logos): searched below, with all
    /// their sub folders, before TeX's own folders.
    pub tex_inputs: Vec<PathBuf>,
    /// The time one run of LuaLaTeX or biber may take.
    pub timeout: Duration,
    /// The most runs of LuaLaTeX.
    pub max_runs: u32,
    /// Bibliography files (`.bib`) copied into the work folder under their own names, for
    /// `\addbibresource` (TEX-043).
    pub bib_files: Vec<PathBuf>,
    /// Set to stop the compile: the running program and everything it started are killed.
    pub cancel: Option<Arc<AtomicBool>>,
    /// The font lines of `main.tex` that the writer added (TEX-054). When LuaLaTeX stops because
    /// fontspec cannot find one of these fonts, the lines of the missing fonts are taken out and
    /// the compile starts again, once (TEX-057).
    pub fonts: Vec<FontOverride>,
}

impl Default for Options {
    fn default() -> Self {
        Options { tex_inputs: vec![], timeout: RUN_TIMEOUT, max_runs: MAX_RUNS, bib_files: vec![], cancel: None, fonts: vec![] }
    }
}

/// What a compile made.
#[derive(Debug)]
pub struct CompileOutput {
    pub pdf: Vec<u8>,
    /// How many times LuaLaTeX ran.
    pub runs: u32,
    /// The font names database was built in this compile (the first run after an install is slow).
    pub font_cache_built: bool,
    /// What the export should say without failing: a citation key biber did not find, biber
    /// missing (the PDF has no bibliography), numbers that may be stale after the last run (TEX-043).
    pub warnings: Vec<String>,
}

/// One problem found in a log or in what biber said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub message: String,
    /// The file, from `-file-line-error`.
    pub file: Option<String>,
    pub line: Option<u32>,
    /// The file TeX could not find (`foo.sty`).
    pub missing: Option<String>,
}

impl Problem {
    /// "main.tex:12: Undefined control sequence." or "package foo is missing".
    pub fn text(&self) -> String {
        let what = match &self.missing {
            Some(m) => missing_text(m),
            None => self.message.clone(),
        };
        match (&self.file, self.line) {
            (Some(f), Some(l)) => format!("{}:{l}: {what}", f.trim_start_matches("./")),
            _ => what,
        }
    }
}

/// What TeX and luaotfload say when a font cannot be loaded although it is named right: the
/// installation's fonts or its font names database are incomplete (#362). Compared in lower case.
const FONT_LOADING: [&str; 2] = ["not loadable", "metric data not found"];

impl Problem {
    /// Whether this is a font that could not be loaded (`Font \TU/lmr/m/n/10.95=[lmroman10-regular]:+tlig;
    /// not loadable: …`), not a font that does not exist (fontspec's "cannot be found").
    pub fn is_font_loading(&self) -> bool {
        let m = self.message.to_lowercase();
        FONT_LOADING.iter().any(|p| m.contains(p))
    }
}

/// "package foo is missing" for `foo.sty`.
fn missing_text(file: &str) -> String {
    let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
    match file.rsplit_once('.').map(|(_, e)| e) {
        Some("sty") => format!("package {stem} is missing"),
        Some("cls") => format!("class {stem} is missing"),
        _ => format!("file {file} is missing"),
    }
}

/// Why a compile failed: a headline, the problems, and the end of the log.
#[derive(Debug)]
pub struct CompileError {
    pub headline: String,
    pub problems: Vec<Problem>,
    pub log_tail: String,
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.headline)?;
        for p in &self.problems {
            write!(f, "\n- {}", p.text())?;
        }
        if self.font_problem() {
            write!(f, "\n{FONT_HINT}")?;
        }
        Ok(())
    }
}

/// What a font-loading problem means, in plain words (#362).
pub const FONT_HINT: &str =
    "A font could not be loaded: the fonts of this TeX installation or its font names database look incomplete. `luaotfload-tool --update --force` rebuilds the database, and Settings → LaTeX can install TeX Live again.";

impl CompileError {
    /// Whether a problem is a font that could not be loaded (see [`Problem::is_font_loading`]).
    pub fn font_problem(&self) -> bool {
        self.problems.iter().any(Problem::is_font_loading)
    }
}

/// The messages `latex` prints after the error that stopped it: not a problem of their own.
fn is_follow_up(message: &str) -> bool {
    message.contains("Emergency stop") || message.contains("Fatal error occurred") || message.contains("no output PDF file produced")
}

/// The file in `File `x.sty' not found` / `File 'x.sty' not found` / `I can't find file `x'`.
fn missing_file(message: &str) -> Option<String> {
    let quoted = |after: &str| -> Option<String> {
        let rest = message.split_once(after)?.1;
        let start = rest.find(['`', '\'', '"'])? + 1;
        let end = rest[start..].find(['\'', '`', '"'])? + start;
        Some(rest[start..end].to_string())
    };
    if message.contains("not found") {
        quoted("File ").filter(|f| !f.is_empty())
    } else {
        quoted("I can't find file ").filter(|f| !f.is_empty())
    }
}

/// `main.tex:12: Message` as (file, line, message); the file must have an extension, so that a
/// line of text that happens to have `: 12: ` in it is not taken.
pub(super) fn split_file_line(line: &str) -> Option<(&str, u32, &str)> {
    for (i, _) in line.match_indices(':') {
        let (file, rest) = (&line[..i], &line[i + 1..]);
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        if digits == 0 || file.is_empty() || !rest[digits..].starts_with(": ") {
            continue;
        }
        let name = file.rsplit(['/', '\\']).next().unwrap_or(file);
        if file.starts_with(char::is_whitespace) || !name.contains('.') || name.ends_with('.') {
            continue;
        }
        return Some((file, rest[..digits].parse().ok()?, rest[digits + 2..].trim()));
    }
    None
}

/// The problems in a TeX log: `! Message` lines, and `file:line: message` lines of
/// `-file-line-error`; the messages that only follow an error are left out.
pub fn problems(log: &str) -> Vec<Problem> {
    let mut found: Vec<Problem> = vec![];
    for line in log.lines() {
        let (file, lineno, message) = if let Some(m) = line.strip_prefix("! ") {
            (None, None, m.trim())
        } else if let Some((f, n, m)) = split_file_line(line) {
            (Some(f.to_string()), Some(n), m)
        } else {
            continue;
        };
        if message.is_empty() {
            continue;
        }
        let p = Problem { message: message.to_string(), file, line: lineno, missing: missing_file(message) };
        if !found.contains(&p) {
            found.push(p);
        }
    }
    let real: Vec<Problem> = found.iter().filter(|p| !is_follow_up(&p.message)).cloned().collect();
    if real.is_empty() {
        found
    } else {
        real
    }
}

/// The `ERROR` lines of biber's output as problems.
pub fn biber_problems(output: &str) -> Vec<Problem> {
    output.lines().filter_map(|l| l.split_once("ERROR - ").map(|(_, m)| Problem { message: m.trim().to_string(), file: None, line: None, missing: None })).collect()
}

/// Whether a `.bcf` (biblatex's control file) has citations: a document with only a
/// `\printbibliography`, or none, gives biber nothing to resolve.
pub fn bcf_has_citations(bcf: &[u8]) -> bool {
    String::from_utf8_lossy(bcf).contains("<bcf:citekey")
}

/// The citation keys biber did not find (`WARN - I didn't find a database entry for 'key' (section 0)`)
/// as warnings of the export.
pub fn biber_warnings(output: &str) -> Vec<String> {
    let mut found: Vec<String> = vec![];
    for line in output.lines() {
        if let Some(rest) = line.split_once("I didn't find a database entry for '").map(|(_, r)| r) {
            if let Some((key, _)) = rest.split_once('\'') {
                let w = format!("citation key '{key}' was not found in the bibliography");
                if !found.contains(&w) {
                    found.push(w);
                }
            }
        }
    }
    found
}

/// Whether the log asks for another run: references, outlines or labels that changed, or a
/// bibliography that is not made yet.
pub fn asks_rerun(log: &str) -> bool {
    const PHRASES: [&str; 5] = ["rerun to get", "please rerun", "rerun latex", "(re)run biber", "label(s) may have changed"];
    let lower = log.to_lowercase();
    PHRASES.iter().any(|p| lower.contains(p))
}

/// Whether luaotfload built its font names database in this run.
pub fn built_font_cache(log: &str) -> bool {
    log.contains("Font names database not found") || log.contains("Reload initiated")
}

/// The last lines of a text.
fn tail_of(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(TAIL_LINES)..].join("\n")
}

/// A file name that is safe on the command line: no folders, no leading `-` or `\`, and only
/// letters, digits, `_`, `-` and `.`.
pub fn valid_main_name(name: &str) -> bool {
    name.ends_with(".tex") && name.len() > 4 && !name.starts_with(['-', '.']) && name.chars().all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c))
}

/// The separator of TeX's search paths (kpathsea's `ENV_SEP`): `;` on Windows, `:` elsewhere.
pub const SEARCH_PATH_SEP: char = if cfg!(windows) { ';' } else { ':' };

/// `TEXINPUTS`: the template folders with all their sub folders, then TeX's own (the empty last
/// entry stands for the default path).
pub fn tex_inputs(dirs: &[PathBuf]) -> Result<OsString, String> {
    search_path(dirs, SEARCH_PATH_SEP)
}

/// [`tex_inputs`] joined with `sep`. A folder with the separator in its name is refused: kpathsea
/// splits at every separator and knows no quoting (`std::env::join_paths` quotes such a folder on
/// Windows, which TeX does not read).
pub fn search_path(dirs: &[PathBuf], sep: char) -> Result<OsString, String> {
    let mut v = OsString::new();
    for d in dirs {
        if d.to_string_lossy().contains(sep) {
            return Err(format!("the template folder {} has a {sep:?}, which cannot be used in a search path", d.display()));
        }
        v.push(d.as_os_str());
        v.push("//");
        v.push(sep.to_string());
    }
    Ok(v)
}

/// The arguments of every LuaLaTeX run: nothing stops for input, the first error stops the
/// run, errors carry `file:line`, and no program can be started from the document.
pub fn lualatex_args(main_tex: &str) -> Vec<String> {
    ["-interaction=nonstopmode", "-halt-on-error", "-file-line-error", "-no-shell-escape", main_tex].map(String::from).to_vec()
}

/// The files of a run that the next run reads without LaTeX saying so: the table of contents, the
/// lists of figures and tables, the bookmarks of the PDF and the navigation of slides. A run that
/// changes one of them (or makes it) needs another run, as `latexmk` sees it.
const WATCHED: [&str; 6] = ["toc", "lof", "lot", "out", "nav", "snm"];

/// What the watched files of `work` hold now, one hash each (none for a file that is not there).
fn watched_state(work: &Path, stem: &str) -> Vec<Option<u64>> {
    WATCHED.iter().map(|e| crate::fsutil::read(work.join(format!("{stem}.{e}"))).ok().map(|b| hash_of(&b))).collect()
}

fn hash_of(bytes: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

/// The debug setting `SHALE_TEX_FAILED_LOGS=<folder>`: a failed compile copies its full `.log` there
/// before its work folder is removed (#327), so a failure that does not come again can still be read.
pub const FAILED_LOGS_VAR: &str = "SHALE_TEX_FAILED_LOGS";

/// Copy `<stem>.log` of the work folder `work` into `folder` as `<pid>-<n>-<stem>.log` (the folder is
/// made when needed). Errors are ignored: keeping a log must never change what a compile reports.
pub fn keep_failed_log(folder: &Path, work: &Path, main_tex: &str) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let Some(stem) = main_tex.strip_suffix(".tex").filter(|_| valid_main_name(main_tex)) else { return };
    if std::fs::create_dir_all(folder).is_err() {
        return;
    }
    let to = folder.join(format!("{}-{}-{stem}.log", std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::copy(work.join(format!("{stem}.log")), to);
}

/// Compile `main_tex` (a file in `work`) to a PDF: LuaLaTeX, biber when the run made a `.bcf`,
/// and more runs while the log asks for them. The folder `work` is removed when this returns; after
/// a failure its log is kept first when `SHALE_TEX_FAILED_LOGS` names a folder.
pub fn compile(engine: &Engine, work: WorkDir, main_tex: &str, opts: &Options) -> Result<CompileOutput, CompileError> {
    let folder = std::env::var_os(FAILED_LOGS_VAR).filter(|f| !f.is_empty()).map(PathBuf::from);
    compile_keeping_logs(engine, work, main_tex, opts, folder.as_deref())
}

/// [`compile`], with the folder for the logs of failed compiles given (none: no log is kept).
pub(super) fn compile_keeping_logs(engine: &Engine, work: WorkDir, main_tex: &str, opts: &Options, failed_logs: Option<&Path>) -> Result<CompileOutput, CompileError> {
    let result = compile_in(engine, &work, main_tex, opts);
    if let (Err(_), Some(folder)) = (&result, failed_logs) {
        keep_failed_log(folder, work.path(), main_tex);
    }
    result
}

fn compile_in(engine: &Engine, work: &WorkDir, main_tex: &str, opts: &Options) -> Result<CompileOutput, CompileError> {
    let fail = |headline: String, problems: Vec<Problem>, log_tail: String| CompileError { headline, problems, log_tail };
    if !valid_main_name(main_tex) {
        return Err(fail(format!("{main_tex:?} is not a usable file name"), vec![], String::new()));
    }
    let inputs = tex_inputs(&opts.tex_inputs).map_err(|e| fail(e, vec![], String::new()))?;
    let stem = &main_tex[..main_tex.len() - 4];
    let base = |program: &str| {
        let mut c = engine.command(program);
        c.current_dir(work.path()).env("TEXINPUTS", &inputs).env("BIBINPUTS", &inputs);
        // a document writes only into its own folder (and not into a parent or a dot file)
        c.env("openout_any", "p");
        c
    };
    for bib in &opts.bib_files {
        let name = bib.file_name().and_then(|n| n.to_str()).filter(|n| n.ends_with(".bib") && !n.starts_with(['-', '.']));
        let copied = name.ok_or_else(|| "it has no usable file name".to_string()).and_then(|n| {
            let bytes = crate::fsutil::read(bib).map_err(|e| e.to_string())?;
            work.write(n, &bytes).map_err(|e| e.to_string())
        });
        copied.map_err(|e| fail(format!("the bibliography {} could not be copied into the work folder ({e})", bib.display()), vec![], String::new()))?;
    }
    let cancelled = || opts.cancel.as_deref().is_some_and(|c| c.load(Ordering::SeqCst));
    let read_log = || crate::fsutil::read(work.path().join(format!("{stem}.log"))).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let mut runs = 0;
    let mut bcf_seen: Option<u64> = None;
    let mut font_cache_built = false;
    let mut warnings: Vec<String> = vec![];
    let mut biber_usable = true;
    // fontspec's "cannot be found" is answered once, by compiling without the fonts it names (TEX-057)
    let mut font_retried = false;
    loop {
        // between two runs is a safe point for Cancel (JOB-006)
        if cancelled() {
            return Err(fail("the compile was cancelled".into(), vec![], String::new()));
        }
        let mut cmd = base("lualatex");
        cmd.args(lualatex_args(main_tex));
        runs += 1;
        let watched_before = watched_state(work.path(), stem);
        let run = proc::run(cmd, opts.timeout, opts.cancel.as_deref(), |_| {}).map_err(|e| fail(format!("lualatex could not be started ({e}); check Settings → LaTeX"), vec![], String::new()))?;
        let log = read_log();
        font_cache_built |= built_font_cache(&log);
        let (log_text, tail) = if log.is_empty() { (run.tail_text(), run.tail_text()) } else { (log.clone(), tail_of(&log)) };
        match run.ending {
            Ending::TimedOut => return Err(fail(format!("LuaLaTeX ran for more than {} seconds and was stopped", opts.timeout.as_secs()), problems(&log_text), tail)),
            Ending::Cancelled => return Err(fail("the compile was cancelled".into(), vec![], tail)),
            Ending::Exited(Some(0)) => {}
            Ending::Exited(code) => {
                let mut p = problems(&log_text);
                if !font_retried && !opts.fonts.is_empty() {
                    if let Some(made) = drop_missing_fonts(work, main_tex, &opts.fonts, &log_text) {
                        font_retried = true;
                        warnings.extend(made);
                        continue;
                    }
                }
                if p.is_empty() {
                    p.push(Problem { message: format!("LuaLaTeX stopped without saying why (exit code {})", code.map_or_else(|| "none".to_string(), |c| c.to_string())), file: None, line: None, missing: None });
                }
                return Err(fail("LuaLaTeX stopped with an error".into(), p, tail));
            }
        }
        let mut again = asks_rerun(&log) || watched_before != watched_state(work.path(), stem);
        let bcf = crate::fsutil::read(work.path().join(format!("{stem}.bcf"))).ok().filter(|b| bcf_has_citations(b));
        if let (Some(bcf), true) = (bcf, biber_usable) {
            let h = hash_of(&bcf);
            if bcf_seen != Some(h) {
                bcf_seen = Some(h);
                let mut cmd = base("biber");
                cmd.arg(stem);
                match proc::run(cmd, opts.timeout, opts.cancel.as_deref(), |_| {}) {
                    // biber is not there: no bibliography, but the document is still made
                    Err(e) => {
                        biber_usable = false;
                        warnings.push(format!("biber could not be started ({e}): the PDF has no bibliography. Install biber next to LuaLaTeX, or in Settings → LaTeX the TeX Live that Shale installs."));
                    }
                    Ok(b) => {
                        match b.ending {
                            Ending::TimedOut => return Err(fail(format!("biber ran for more than {} seconds and was stopped", opts.timeout.as_secs()), vec![], b.tail_text())),
                            Ending::Cancelled => return Err(fail("the compile was cancelled".into(), vec![], b.tail_text())),
                            Ending::Exited(Some(0)) => {}
                            Ending::Exited(_) => {
                                let blg = crate::fsutil::read(work.path().join(format!("{stem}.blg"))).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
                                let mut p = biber_problems(&b.tail_text());
                                if p.is_empty() {
                                    p.push(Problem { message: "biber stopped without saying why".into(), file: None, line: None, missing: None });
                                }
                                let named = if blg.is_empty() { String::new() } else { format!(" (its log is {stem}.blg)") };
                                return Err(fail(format!("biber stopped with an error{named}"), p, if blg.is_empty() { b.tail_text() } else { tail_of(&blg) }));
                            }
                        }
                        for w in biber_warnings(&b.tail_text()) {
                            if !warnings.contains(&w) {
                                warnings.push(w);
                            }
                        }
                        again = true;
                    }
                }
            }
        }
        if !again {
            break;
        }
        // the run that stopped on a missing font was not one of the runs the references need
        if runs >= opts.max_runs + u32::from(font_retried) {
            warnings.push(format!("LuaLaTeX was run {runs} times and still asked for another run: the numbers and references in the PDF may be stale"));
            break;
        }
    }
    match crate::fsutil::read(work.path().join(format!("{stem}.pdf"))) {
        Ok(pdf) if !pdf.is_empty() => Ok(CompileOutput { pdf, runs, font_cache_built, warnings }),
        _ => Err(fail("LuaLaTeX made no PDF".into(), problems(&read_log()), tail_of(&read_log()))),
    }
}

/// The compile stopped with fontspec's "cannot be found" for a font of the writer's lines: write
/// `main.tex` again without those lines (TEX-057). The warnings for the fonts taken out; none when
/// the error is another one, or names a font that is not one of the lines (the template's own).
fn drop_missing_fonts(work: &WorkDir, main_tex: &str, lines: &[FontOverride], log: &str) -> Option<Vec<String>> {
    let missing = fonts::missing_fonts(log);
    if missing.is_empty() {
        return None;
    }
    let source = String::from_utf8_lossy(&crate::fsutil::read(work.path().join(main_tex)).ok()?).into_owned();
    let (text, warnings) = fonts::without_missing(&source, lines, &missing)?;
    work.replace(main_tex, text.as_bytes()).ok()?;
    Some(warnings)
}

/// The small document of the check in Settings → LaTeX (and of the font cache after an install):
/// the fonts and packages the templates stand on, and a bibliography, so that LuaLaTeX and
/// biber are both tried.
pub const TEST_DOCUMENT: &str = "\\documentclass{scrartcl}\n\\usepackage{fontspec}\n\\setmainfont{XCharter}\n\\setsansfont{Roboto}\n\\usepackage[backend=biber]{biblatex}\n\\addbibresource{test.bib}\n\\begin{document}\n\\section{Shale}\nHello from \\textsf{LuaLaTeX}. \\cite{shale}\n\\printbibliography\n\\end{document}\n";

/// The bibliography of [`TEST_DOCUMENT`].
pub const TEST_BIB: &str = "@book{shale,\n  author = {Lovelace, Ada},\n  title = {Notes},\n  year = {1843},\n}\n";

/// Compile the test document with an engine.
pub fn compile_test_document(engine: &Engine) -> Result<CompileOutput, CompileError> {
    let fail = |m: String| CompileError { headline: m, problems: vec![], log_tail: String::new() };
    let work = WorkDir::new().map_err(|e| fail(format!("no temporary folder ({e})")))?;
    work.write("test.tex", TEST_DOCUMENT.as_bytes()).and_then(|_| work.write("test.bib", TEST_BIB.as_bytes())).map_err(|e| fail(format!("the test document could not be written ({e})")))?;
    compile(engine, work, "test.tex", &Options::default())
}
