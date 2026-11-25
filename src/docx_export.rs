//! A note as a Word file (.docx) through pandoc (DOCX-001 to DOCX-010), the reverse of the Word import
//! (`import::docx`). Shale prepares the text of the note ([`prepare_text`]), pandoc reads it as commonmark and
//! runs Shale's Lua filter (`assets/export/shale-docx-export.lua`) on it, writes the docx with a reference
//! document ([`reference`]: the original Word file of the note, or one made of its `print-*` properties), and
//! Shale finishes what pandoc's docx writer cannot say ([`finish`]). The program is the user's (BUILD-018),
//! run like the import runs it: an argument list, no shell, stdin closed, no console window, a time limit.
//! Nothing is written before the file is finished.

pub mod dialog;
mod finish;
pub mod look;
mod package;
mod reference;
mod thesis;
mod xml;

#[cfg(test)]
mod export_tests;
#[cfg(test)]
mod schema_check;
#[cfg(test)]
mod thesis_tests;

use crate::pdf::tex::compile::WorkDir;
use crate::pdf::tex::proc::{self, Ending};
use crate::vault::{rel_to_string, Vault};
pub use look::PrintLook;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// The filter, built into the program and written to the work folder for each run (DOCX-002).
const FILTER: &str = include_str!("../assets/export/shale-docx-export.lua");

/// How long pandoc may run before it is killed (DOCX-002).
pub const TIMEOUT: Duration = Duration::from_secs(120);

/// What pandoc reads a note as: Shale's Markdown, `==mark==` left to the filter (DOCX-002).
const READER: &str = "commonmark+pipe_tables+strikeout+footnotes+tex_math_dollars+wikilinks_title_after_pipe+autolink_bare_uris+task_lists";

/// The start of a code fence: the character and how many there are (DOCX-003).
fn fence(line: &str) -> Option<(char, usize)> {
    let line = line.trim_start_matches(' ');
    let c = line.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let n = line.chars().take_while(|x| *x == c).count();
    // the info string of a backtick fence has no backtick: ``` a ``` is code in a line
    (n >= 3 && !(c == '`' && line[n..].contains('`'))).then_some((c, n))
}

/// Whether `line` ends the fence of `n` times `c`.
fn fence_end(line: &str, (c, n): (char, usize)) -> bool {
    let line = line.trim_start_matches(' ').trim_end();
    line.len() >= n && line.chars().all(|x| x == c)
}

/// Where the code span of `run` backticks that `rest` starts with ends: after the next run of as many.
fn code_span_end(rest: &str, run: usize) -> Option<usize> {
    let mut at = run;
    while let Some(c) = rest[at..].chars().next() {
        if c == '`' {
            let n = rest[at..].chars().take_while(|x| *x == '`').count();
            if n == run {
                return Some(at + n);
            }
            at += n;
        } else {
            at += c.len_utf8();
        }
    }
    None
}

/// `line` with the embeds `![[target…]]` whose target `resolve` knows written with the path it gives, and
/// inline code as it is.
fn embeds(line: &str, resolve: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < line.len() {
        let rest = &line[i..];
        if rest.starts_with('`') {
            // a code span ends at a run of as many backticks; without one the backticks are text
            let run = rest.chars().take_while(|c| *c == '`').count();
            let end = code_span_end(rest, run);
            let take = end.unwrap_or(run);
            out.push_str(&rest[..take]);
            i += take;
            continue;
        }
        if let Some(inner) = rest.strip_prefix("![[").and_then(|r| r.split_once("]]").map(|(inner, _)| inner)) {
            let at = inner.find(['#', '|', '\\']).unwrap_or(inner.len());
            if let Some(path) = resolve(&inner[..at]).filter(|p| !p.contains(['#', '|', '[', ']'])) {
                out.push_str(&format!("![[{path}{}]]", &inner[at..]));
                i += 3 + inner.len() + 2;
                continue;
            }
        }
        let c = rest.chars().next().expect("not at the end");
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// The text pandoc reads (DOCX-003): the front matter taken off, and every embed whose target `resolve`
/// knows (the vault path of the file it opens) written with that path, so that pandoc finds it below the
/// vault folder. An embed that does not resolve, and anything in a fenced code block or in inline code, stays.
pub fn prepare_text(text: &str, resolve: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut open: Option<(char, usize)> = None;
    for line in text.split_inclusive('\n').skip(crate::markdown::front_matter_lines(text)) {
        match open {
            Some(f) => {
                if fence_end(line, f) {
                    open = None;
                }
                out.push_str(line);
            }
            None => match fence(line) {
                Some(f) => {
                    open = Some(f);
                    out.push_str(line);
                }
                None => out.push_str(&embeds(line, resolve)),
            },
        }
    }
    out
}

/// The words pandoc is started with (DOCX-002); `text_width` is the width of the text of the page in inches, which the
/// filter makes the widths of pictures of (DOCX-005); `toc`: with a table of contents of the headings down to `###`.
pub fn args(note: &Path, filter: &Path, resources: &Path, reference: &Path, text_width: f32, out: &Path, toc: bool) -> Vec<OsString> {
    let mut a: Vec<OsString> = vec![note.into()];
    a.extend(["-f", READER, "-t", "docx"].map(OsString::from));
    for (flag, path) in [("--lua-filter=", filter), ("--resource-path=", resources), ("--reference-doc=", reference)] {
        let mut f = OsString::from(flag);
        f.push(path);
        a.push(f);
    }
    if toc {
        a.extend(["--toc", "--toc-depth=3"].map(OsString::from));
    }
    a.extend(["-M".into(), format!("shale-text-width={text_width:.3}").into()]);
    a.push("-o".into());
    a.push(out.into());
    a
}

/// The original Word file of a note (DOCX-001, DOCX-007): the file its `source:` property links to, when that is a
/// `.docx` file of the vault that exists. A vault path.
pub fn original_of(vault: &Vault, rel: &Path, text: &str) -> Option<PathBuf> {
    let (_, values) = crate::markdown::front_matter_values(text).into_iter().find(|(k, _)| k == "source")?;
    let link = values.first()?.trim().strip_prefix("[[")?.split_once("]]")?.0;
    let target = link.split(['|', '#']).next()?;
    let found = vault.resolve(target, Some(rel))?;
    let word = found.extension().is_some_and(|e| e.eq_ignore_ascii_case("docx"));
    (word && vault.abs(&found).is_file()).then_some(found)
}

/// What one export needs. It holds everything, so it can run on a thread of its own.
#[derive(Clone, Debug)]
pub struct Job {
    pub pandoc: PathBuf,
    /// the vault folder
    pub root: PathBuf,
    /// the text pandoc reads ([`prepare_text`])
    pub text: String,
    pub look: PrintLook,
    /// the original Word file as the template (the whole path)
    pub original: Option<PathBuf>,
    pub toc: bool,
    pub timeout: Duration,
    /// what the thesis syntax of the note needs of pandoc (DOCX-014); none for a note without any
    pub thesis: Option<thesis::Setup>,
    /// what the export has to say about the thesis syntax (DOCX-016)
    pub warnings: Vec<String>,
}

impl Job {
    /// The export of `text`, the text of the note `rel` as the window has it. `original`: the template, as [`original_of`] gives it; `toc_break`: a page break after the table of contents.
    pub fn new(pandoc: PathBuf, vault: &Vault, rel: &Path, text: &str, original: Option<&Path>, toc: bool, toc_break: bool) -> Job {
        let thesis = thesis::prepare(vault, rel, text);
        let prepared = prepare_text(&thesis.text, &|target| vault.resolve(target, Some(rel)).map(|p| rel_to_string(&p)));
        // the dialog's box says whether the table of contents is followed by a page break (DOCX-013)
        let look = PrintLook { toc_page_break: Some(toc && toc_break), ..PrintLook::of(text) };
        Job { pandoc, root: vault.root.clone(), text: prepared, look, original: original.map(|o| vault.abs(o)), toc, timeout: TIMEOUT, thesis: thesis.setup, warnings: thesis.warnings }
    }
}

/// What pandoc's ending says about a run that has not succeeded; `Ok` for one that has.
fn ended(run: &proc::Run, timeout: Duration) -> Result<(), String> {
    match run.ending {
        Ending::Exited(Some(0)) => Ok(()),
        Ending::TimedOut => Err(format!("pandoc needed more than {} seconds and was stopped", timeout.as_secs_f64())),
        Ending::Cancelled => Err("stopped".into()),
        Ending::Exited(_) if crate::pandoc_limit::overflowed(run) => Err(crate::pandoc_limit::overflow_message(None)),
        Ending::Exited(_) if run.tail.iter().any(|l| l.contains("citeproc") && (l.contains("nrecognized") || l.contains("nknown"))) => Err(thesis::OLD_PANDOC.into()),
        Ending::Exited(_) => Err(format!("pandoc could not make the Word file: {}", run.tail.iter().find(|l| !l.trim().is_empty()).map_or("it stopped without a message", |l| l.trim()))),
    }
}

/// Make the Word file of the job (DOCX-002): in a private temporary folder, removed afterwards. Returns the
/// finished file; nothing is written outside the folder. `cancel` stops it.
pub fn export(job: &Job, cancel: &AtomicBool) -> Result<Vec<u8>, String> {
    let work = WorkDir::new().map_err(|e| format!("could not make a temporary folder: {e}"))?;
    let write = |name: &str, bytes: &[u8]| work.write(name, bytes).map_err(|e| format!("could not write {name} to the temporary folder: {e}"));
    write("filter.lua", FILTER.as_bytes())?;
    write("note.md", job.text.as_bytes())?;
    if let Some(t) = &job.thesis {
        write(thesis::DATA_FILE, t.data.as_bytes())?;
        for (name, bytes) in &t.bibliography {
            write(name, bytes)?;
        }
        if let Some(csl) = &t.csl {
            write(thesis::CSL_FILE, csl)?;
        }
    }
    let default = reference::default_reference(&job.pandoc, work.path(), Some(cancel))?;
    let template = match &job.original {
        Some(original) => reference::from_original(&crate::fsutil::read(original).map_err(|e| format!("the original Word file cannot be read: {e}"))?, &default)?,
        None => reference::from_props(&default, &job.look)?,
    };
    write("reference.docx", &template)?;
    let out = work.path().join("out.docx");
    let width = reference::text_width(&template, &job.look);
    let setup = |cmd: &mut std::process::Command| {
        cmd.args(args(&work.path().join("note.md"), &work.path().join("filter.lua"), &job.root, &work.path().join("reference.docx"), width, &out, job.toc)).current_dir(work.path());
        if let Some(t) = &job.thesis {
            cmd.args(thesis::args(t, work.path()));
        }
    };
    let run = crate::pandoc_limit::run(&job.pandoc, setup, |cmd| proc::run(cmd, job.timeout, Some(cancel), |_| {})).map_err(|e| format!("pandoc could not be started: {e}"))?;
    ended(&run, job.timeout)?;
    let made = crate::fsutil::read(&out).map_err(|e| format!("pandoc made no file: {e}"))?;
    finish::finish(&made, &job.look)
}
