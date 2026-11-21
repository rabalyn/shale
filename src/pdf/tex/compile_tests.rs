//! The compile of the TeX export (TEX-004): the log turned into readable errors, the reruns, the
//! timeout and the command line, tested with logs LuaLaTeX really wrote (`src/testdata/tex/`) and
//! with fake engines (scripts in a temporary folder, Unix only); and against the real LuaLaTeX of
//! the container (`tests/tex-env/`, ignored unless asked for: `cargo test -- --ignored tex_env`).

use super::compile::*;
use super::engine::{Engine, Kind};
use std::path::{Path, PathBuf};

const MISSING_PACKAGE: &str = include_str!("../../testdata/tex/missing-package.log");
const UNDEFINED: &str = include_str!("../../testdata/tex/undefined-control-sequence.log");
const RERUN: &str = include_str!("../../testdata/tex/rerun-references.log");

// spec: TEX-004
#[test]
fn a_missing_package_is_said_in_words() {
    let p = problems(MISSING_PACKAGE);
    assert_eq!(p.len(), 1, "the follow-up messages are left out: {p:?}");
    assert_eq!(p[0].missing.as_deref(), Some("nosuchpackage.sty"));
    assert_eq!(p[0].text(), "package nosuchpackage is missing");
}

// spec: TEX-004
#[test]
fn an_error_has_its_file_and_line_from_file_line_error() {
    let p = problems(UNDEFINED);
    assert_eq!(p.len(), 1, "{p:?}");
    assert_eq!((p[0].file.as_deref(), p[0].line, p[0].message.as_str()), (Some("./err.tex"), Some(4), "Undefined control sequence."));
    assert_eq!(p[0].text(), "err.tex:4: Undefined control sequence.");
}

// spec: TEX-004
#[test]
fn problems_of_both_forms_and_other_missing_files() {
    let log = "! LaTeX Error: File `a.cls' not found.\n./main.tex:9: Package fontspec Error: The font \"Foo\" cannot be found.\n./main.tex:9: Package fontspec Error: The font \"Foo\" cannot be found.\n./main.tex:11: LaTeX Error: File 'b.sty' not found.\n! I can't find file `c.png'.\n! Emergency stop.\n";
    let texts: Vec<String> = problems(log).iter().map(Problem::text).collect();
    assert_eq!(texts, ["class a is missing", "main.tex:9: Package fontspec Error: The font \"Foo\" cannot be found.", "main.tex:11: package b is missing", "file c.png is missing"], "the same error once, Emergency stop left out");
    // nothing but the follow-up: it is kept, so there is something to say
    assert_eq!(problems("! Emergency stop.\n").len(), 1);
    assert!(problems("Overfull \\hbox (3.0pt too wide) in paragraph at lines 5--6\n").is_empty());
}

// spec: TEX-004
#[test]
fn only_a_line_with_a_file_name_and_a_number_is_an_error_line() {
    assert_eq!(split_file_line("./a.tex:12: Undefined control sequence."), Some(("./a.tex", 12, "Undefined control sequence.")));
    assert_eq!(split_file_line("C:\\work\\a.tex:3: Missing $ inserted."), Some(("C:\\work\\a.tex", 3, "Missing $ inserted.")));
    assert_eq!(split_file_line("Package foo Warning: x: 12: y"), None, "no extension, text before it");
    assert_eq!(split_file_line("a.tex:x: no number"), None);
    assert_eq!(split_file_line("a.tex:12 no colon"), None);
    assert_eq!(split_file_line(" a.tex:1: starts with a space"), None);
}

// spec: TEX-004
#[test]
fn biber_errors_are_found() {
    let out = "INFO - This is Biber 2.21\nINFO - Found 1 citekeys\nERROR - BibTeX subsystem: /work/x.bib, line 3, syntax error\nWARN - x\n";
    let p = biber_problems(out);
    assert_eq!(p.len(), 1);
    assert!(p[0].text().starts_with("BibTeX subsystem"), "{}", p[0].text());
}

// spec: TEX-004
#[test]
fn the_log_decides_whether_another_run_is_needed() {
    assert!(asks_rerun(RERUN), "labels may have changed");
    assert!(!asks_rerun(UNDEFINED) && !asks_rerun(MISSING_PACKAGE));
    assert!(asks_rerun("Package rerunfilecheck Warning: File `a.out' has changed.\n(rerunfilecheck)                Rerun to get outlines right"));
    assert!(asks_rerun("Package biblatex Warning: Please (re)run Biber on the file:\n"));
    assert!(!asks_rerun("Package rerunfilecheck Info: File `a.out' has not changed."), "only the name of a package says rerun");
}

// spec: TEX-004
#[test]
fn the_first_run_after_an_install_is_told_by_the_font_database_line() {
    assert!(built_font_cache(RERUN));
    assert!(!built_font_cache("luaotfload | db : Font names database loaded: /x/luaotfload-names.luc.gz"));
}

// spec: TEX-004
#[test]
fn the_command_line_stops_for_nothing_and_allows_no_shell_escape() {
    let args = lualatex_args("main.tex");
    for a in ["-interaction=nonstopmode", "-halt-on-error", "-file-line-error", "-no-shell-escape"] {
        assert!(args.iter().any(|x| x == a), "{a} in {args:?}");
    }
    assert_eq!(args.last().map(String::as_str), Some("main.tex"));
    assert!(!args.iter().any(|a| a.contains("shell-escape") && !a.starts_with("-no-")), "{args:?}");
}

// spec: TEX-004
#[test]
fn the_main_file_name_cannot_be_a_command() {
    for ok in ["main.tex", "a_b-1.tex", "Thesis.2.tex"] {
        assert!(valid_main_name(ok), "{ok}");
    }
    for bad in [".tex", "-x.tex", "\\input{x}.tex", "a b.tex", "../a.tex", "a/b.tex", "a.sty", "a;b.tex", "$(x).tex", ".hidden.tex", ""] {
        assert!(!valid_main_name(bad), "{bad:?}");
    }
}

// spec: TEX-004
#[test]
fn the_template_folders_are_searched_with_their_sub_folders_before_the_default() {
    let v = tex_inputs(&[PathBuf::from("/t/one"), PathBuf::from("/t/two")]).unwrap();
    let parts: Vec<PathBuf> = std::env::split_paths(&v).collect();
    assert_eq!(parts, [PathBuf::from("/t/one//"), PathBuf::from("/t/two//"), PathBuf::new()], "the empty last entry is TeX's own path");
    assert_eq!(std::env::split_paths(&tex_inputs(&[]).unwrap()).count(), 1);
    assert!(tex_inputs(&[PathBuf::from(format!("/a{SEARCH_PATH_SEP}b"))]).is_err(), "a folder with the separator cannot be put in a search path");
}

// spec: TEX-004
#[test]
fn the_search_path_separator_is_the_one_of_the_system_and_a_drive_letter_is_no_separator() {
    assert_eq!(SEARCH_PATH_SEP, if cfg!(windows) { ';' } else { ':' });
    // Unix: a colon separates, a semicolon is part of a name
    assert_eq!(search_path(&[PathBuf::from("/t/one"), PathBuf::from("/t/a;b")], ':').unwrap(), "/t/one//:/t/a;b//:");
    assert!(search_path(&[PathBuf::from("/t/a:b")], ':').is_err());
    // Windows: a semicolon separates; the colon after the drive letter is part of the folder
    assert_eq!(search_path(&[PathBuf::from(r"C:\t\one"), PathBuf::from(r"D:\two")], ';').unwrap(), r"C:\t\one//;D:\two//;");
    assert!(search_path(&[PathBuf::from(r"C:\t\a;b")], ';').is_err(), "kpathsea knows no quoting");
    assert_eq!(search_path(&[], ';').unwrap(), "", "only TeX's own path");
}

// spec: TEX-004
#[test]
fn the_work_folder_is_private_and_removed_when_dropped() {
    let w = WorkDir::new().unwrap();
    let path = w.path().to_path_buf();
    w.write("a.tex", b"x").unwrap();
    assert!(path.join("a.tex").is_file());
    assert!(w.write("a.tex", b"y").is_err(), "an existing file is not replaced");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o700);
    }
    let other = WorkDir::new().unwrap();
    assert_ne!(other.path(), path, "every compile has a folder of its own");
    drop(w);
    assert!(!path.exists());
}

// ---------------------------------------------------------------- fake engines (Unix)

// What the font loading says in the install that failed once (#362) and what a missing font says.
const FONT_NOT_LOADABLE: &str =
    "(/opt/texlive/texmf-dist/tex/latex/koma-script/scrsize11pt.clo)\nkoma-script/scrsize11pt.clo:100: Font \\TU/lmr/m/n/10.95=[lmroman10-regular]:+tlig; not loadable: Metric (TFM) file or installed font not found.\n! Emergency stop.\n";

// spec: TEX-004
#[test]
fn a_font_that_cannot_be_loaded_is_a_font_problem_with_a_plain_words_line_and_a_missing_font_is_not() {
    let p = problems(FONT_NOT_LOADABLE);
    assert_eq!(p.len(), 1, "{p:?}");
    assert!(p[0].is_font_loading(), "{p:?}");
    let e = CompileError { headline: "LuaLaTeX stopped with an error".into(), problems: p, log_tail: String::new() };
    assert!(e.font_problem());
    let shown = e.to_string();
    assert!(shown.contains("lmroman10-regular") && shown.ends_with(FONT_HINT), "{shown}");
    assert!(FONT_HINT.contains("font names database") && FONT_HINT.contains("luaotfload-tool --update --force"));
    let metric = Problem { message: "Package luaotfload Error: Metric data not found".into(), file: None, line: None, missing: None };
    assert!(metric.is_font_loading(), "the other wording of a font that is there but cannot be used");
    let missing = CompileError { headline: "x".into(), problems: problems("./main.tex:9: Package fontspec Error: The font \"Foo\" cannot be found.\n! Undefined control sequence.\n"), log_tail: String::new() };
    assert!(!missing.font_problem() && !missing.to_string().contains("font names database"), "a font that does not exist is another problem: {missing}");
}

// spec: TEX-004
#[test]
fn the_log_of_a_failed_compile_is_kept_under_a_unique_name_and_a_missing_log_or_a_bad_name_keeps_nothing() {
    let root = std::env::temp_dir().join(format!("shale-failed-logs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let folder = root.join("kept").join("logs");
    let work = WorkDir::new().unwrap();
    work.write("main.tex", b"x").unwrap();
    work.write("main.log", b"the whole log\n").unwrap();
    keep_failed_log(&folder, work.path(), "main.tex");
    keep_failed_log(&folder, work.path(), "main.tex");
    let mut names: Vec<String> = std::fs::read_dir(&folder).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    assert_eq!(names.len(), 2, "two failures, two files: {names:?}");
    assert!(names.iter().all(|n| n.starts_with(&format!("{}-", std::process::id())) && n.ends_with("-main.log")), "{names:?}");
    assert_eq!(std::fs::read_to_string(folder.join(&names[0])).unwrap(), "the whole log\n");
    // no log in the folder, a name that is no file name, or a folder that cannot be made: nothing, and no panic
    keep_failed_log(&folder, work.path(), "other.tex");
    keep_failed_log(&folder, work.path(), "../main.tex");
    keep_failed_log(&root.join("kept").join("logs").join(&names[0]).join("sub"), work.path(), "main.tex");
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 2);
    let _ = std::fs::remove_dir_all(&root);
}

#[cfg(unix)]
pub(super) mod fake {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    /// A folder with a fake `lualatex` and `biber`, and the state they keep in `state/`.
    pub struct Fake {
        pub dir: PathBuf,
    }

    /// An executable script `name` in `dir`. Another test thread that forks while the file is open for
    /// writing leaves a copy of the open file in its child for a moment, and starting the script then
    /// fails with "Text file busy": the script is started once here (it answers `--shale-warm-up` at
    /// once) until that is over, so the test that uses it later never meets it.
    pub fn script(dir: &Path, name: &str, body: &str) {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n[ \"$1\" = --shale-warm-up ] && exit 0\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        for _ in 0..500 {
            match std::process::Command::new(&path).arg("--shale-warm-up").output() {
                Err(e) if e.raw_os_error() == Some(26) => std::thread::sleep(std::time::Duration::from_millis(10)),
                _ => break,
            }
        }
    }

    impl Fake {
        pub fn new(tag: &str) -> Fake {
            let dir = std::env::temp_dir().join(format!("shale-fake-tex-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("state")).unwrap();
            Fake { dir }
        }

        pub fn state(&self) -> PathBuf {
            self.dir.join("state")
        }

        /// A LuaLaTeX that counts its runs, records its command line, environment and folder, copies
        /// `log.<n>` (or `log.last`) of the state folder to the log, and makes a PDF.
        pub fn lualatex(&self) {
            let st = self.state();
            script(
                &self.dir.join("bin"),
                "lualatex",
                &format!(
                    r#"S='{}'
n=$(cat "$S/count" 2>/dev/null || echo 0); n=$((n+1)); echo $n > "$S/count"
echo "$@" > "$S/args"; pwd > "$S/pwd"; env > "$S/env"
for a; do last=$a; done
stem=${{last%.tex}}
if [ -f "$S/log.$n" ]; then cp "$S/log.$n" "$stem.log"; else cp "$S/log.last" "$stem.log"; fi
if [ -f "$S/exit" ]; then exit $(cat "$S/exit"); fi
echo '%PDF-1.5 fake' > "$stem.pdf"
if [ -f "$S/bcf" ]; then cp "$S/bcf" "$stem.bcf"; fi
if [ -f "$S/toc.$n" ]; then cp "$S/toc.$n" "$stem.toc"; fi"#,
                    st.display()
                ),
            );
        }

        pub fn biber(&self, exit: i32, output: &str) {
            let st = self.state();
            script(&self.dir.join("bin"), "biber", &format!("echo \"$@\" >> '{0}/biber.calls'\ncat <<'EOF'\n{1}\nEOF\nexit {2}", st.display(), output, exit));
        }

        pub fn put(&self, name: &str, text: &str) {
            std::fs::write(self.state().join(name), text).unwrap();
        }

        pub fn get(&self, name: &str) -> String {
            std::fs::read_to_string(self.state().join(name)).unwrap_or_default()
        }

        pub fn engine(&self) -> Engine {
            Engine::new(Kind::System, self.dir.join("bin"))
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn work_with_main() -> WorkDir {
        let w = WorkDir::new().unwrap();
        w.write("main.tex", b"\\documentclass{article}").unwrap();
        w
    }

    // spec: TEX-004
    #[test]
    fn lualatex_is_started_without_a_shell_in_the_work_folder_with_the_search_path_and_paranoid_output() {
        let f = Fake::new("args");
        f.lualatex();
        f.put("log.last", "clean log\n");
        // the work folder is behind a symlink, as the temp folder is on macOS (/var -> /private/var, #301):
        // the engine reports the resolved folder, so the test compares resolved paths
        let tmp = std::env::temp_dir();
        let real = tmp.join(format!("shale-tex-linked-real-{}", std::process::id()));
        let link = tmp.join(format!("shale-tex-linked-{}", std::process::id()));
        let _ = std::fs::remove_file(&link);
        let _ = std::fs::remove_dir_all(&real);
        std::fs::create_dir_all(&real).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let work = WorkDir::new_in(&link).unwrap();
        work.write("main.tex", b"\\documentclass{article}").unwrap();
        let path = work.path().to_path_buf();
        // resolved before the compile, which removes the folder
        let resolved = std::fs::canonicalize(&path).unwrap();
        // a folder name that a shell would run
        let templates = std::env::temp_dir().join("a$(touch pwned)b");
        let opts = Options { tex_inputs: vec![templates.clone()], ..Options::default() };
        let out = compile(&f.engine(), work, "main.tex", &opts).unwrap();
        assert_eq!((out.pdf.starts_with(b"%PDF"), out.runs), (true, 1));
        let args = f.get("args");
        for a in ["-interaction=nonstopmode", "-halt-on-error", "-file-line-error", "-no-shell-escape", "main.tex"] {
            assert!(args.split_whitespace().any(|x| x == a), "{a} in {args}");
        }
        assert_eq!(PathBuf::from(f.get("pwd").trim()), resolved, "run in the work folder");
        let env = f.get("env");
        assert!(env.lines().any(|l| l == "openout_any=p"), "documents write only into their folder");
        let inputs = env.lines().find_map(|l| l.strip_prefix("TEXINPUTS=")).expect("TEXINPUTS is set");
        assert_eq!(inputs, format!("{}//:", templates.display()), "the folder, searched with its sub folders, then the default");
        assert!(!Path::new("pwned").exists() && !path.join("pwned").exists(), "nothing is run by a shell");
        assert!(!path.exists(), "the work folder is removed after the compile");
        let _ = std::fs::remove_file(&link);
        let _ = std::fs::remove_dir_all(&real);
    }

    // spec: TEX-004
    #[test]
    fn it_runs_again_while_the_log_asks_and_stops_when_it_does_not() {
        let f = Fake::new("rerun");
        f.lualatex();
        f.put("log.1", RERUN);
        f.put("log.2", RERUN);
        f.put("log.last", "all done\n");
        let out = compile(&f.engine(), work_with_main(), "main.tex", &Options::default()).unwrap();
        assert_eq!(out.runs, 3, "two asks, then a clean log");
        assert!(out.font_cache_built, "the fixture log says the font database was built");
        assert!(!f.state().join("biber.calls").exists(), "no .bcf, no biber");
    }

    // spec: TEX-004
    #[test]
    fn it_runs_again_while_the_contents_and_the_bookmarks_change_though_the_log_does_not_say_so() {
        // the first run makes the table of contents, the second fills it, the third finds nothing new
        let f = Fake::new("toc");
        f.lualatex();
        f.put("log.last", "all done\n");
        f.put("toc.1", "\\contentsline {section}{A}{1}\n");
        f.put("toc.2", "\\contentsline {section}{A}{2}\n");
        f.put("toc.3", "\\contentsline {section}{A}{2}\n");
        let out = compile(&f.engine(), work_with_main(), "main.tex", &Options::default()).unwrap();
        assert_eq!(out.runs, 3, "a new file, a changed one, then the same");
        // a document with a contents that does not change on its second run needs two runs, one without none
        let f = Fake::new("toc-same");
        f.lualatex();
        f.put("log.last", "all done\n");
        f.put("toc.1", "x\n");
        f.put("toc.2", "x\n");
        assert_eq!(compile(&f.engine(), work_with_main(), "main.tex", &Options::default()).unwrap().runs, 2);
        // no such file: one run
        let f = Fake::new("no-toc");
        f.lualatex();
        f.put("log.last", "all done\n");
        assert_eq!(compile(&f.engine(), work_with_main(), "main.tex", &Options::default()).unwrap().runs, 1);
    }

    // spec: TEX-004
    #[test]
    fn it_stops_after_five_runs_when_the_log_never_stops_asking() {
        let f = Fake::new("five");
        f.lualatex();
        f.put("log.last", RERUN);
        let out = compile(&f.engine(), work_with_main(), "main.tex", &Options::default()).unwrap();
        assert_eq!(out.runs, 5);
        assert_eq!(f.get("count").trim(), "5");
        let out = compile(&f.engine(), work_with_main(), "main.tex", &Options { max_runs: 2, ..Options::default() }).unwrap();
        assert_eq!(out.runs, 2, "the limit is an option");
    }

    // spec: TEX-004
    #[test]
    fn biber_runs_when_a_bcf_appears_and_again_only_when_it_changed() {
        let f = Fake::new("biber");
        f.lualatex();
        f.biber(0, "INFO - done");
        f.put("log.last", "no rerun asked\n");
        f.put("bcf", "<bcf>1</bcf>");
        let out = compile(&f.engine(), work_with_main(), "main.tex", &Options::default()).unwrap();
        assert_eq!(out.runs, 2, "after biber a new run");
        assert_eq!(f.get("biber.calls").lines().collect::<Vec<_>>(), ["main"], "biber ran once, for the job name");
    }

    // spec: TEX-004
    #[test]
    fn a_biber_error_is_a_readable_failure() {
        let f = Fake::new("biber-fail");
        f.lualatex();
        f.biber(2, "INFO - start\nERROR - BibTeX subsystem: x.bib, line 3, syntax error");
        f.put("log.last", "ok\n");
        f.put("bcf", "<bcf/>");
        let e = compile(&f.engine(), work_with_main(), "main.tex", &Options::default()).unwrap_err();
        assert_eq!(e.headline, "biber stopped with an error");
        assert!(e.problems[0].text().contains("syntax error"), "{e}");
        assert!(e.log_tail.contains("ERROR - BibTeX"));
    }

    // spec: TEX-004
    #[test]
    fn a_failed_run_gives_the_readable_errors_and_the_end_of_the_log_and_removes_the_folder() {
        let f = Fake::new("fail");
        f.lualatex();
        f.put("log.last", MISSING_PACKAGE);
        f.put("exit", "1");
        let work = work_with_main();
        let path = work.path().to_path_buf();
        let e = compile(&f.engine(), work, "main.tex", &Options::default()).unwrap_err();
        assert_eq!(e.headline, "LuaLaTeX stopped with an error");
        assert_eq!(e.problems.iter().map(Problem::text).collect::<Vec<_>>(), ["package nosuchpackage is missing"]);
        assert!(e.log_tail.contains("Fatal error occurred"), "the tail of the log: {}", e.log_tail);
        assert!(e.log_tail.lines().count() <= 40);
        assert!(e.to_string().contains("- package nosuchpackage is missing"), "{e}");
        assert!(!path.exists(), "the folder is removed after a failure too");
    }

    // spec: TEX-004
    #[test]
    fn an_engine_that_fails_without_a_log_still_says_something() {
        let f = Fake::new("nolog");
        script(&f.dir.join("bin"), "lualatex", "echo 'something broke'; exit 3");
        let e = compile(&f.engine(), work_with_main(), "main.tex", &Options::default()).unwrap_err();
        assert!(e.problems[0].message.contains("exit code 3"), "{e}");
        assert!(e.log_tail.contains("something broke"));
    }

    // spec: TEX-004
    #[test]
    fn a_run_that_makes_no_pdf_is_a_failure() {
        let f = Fake::new("nopdf");
        script(&f.dir.join("bin"), "lualatex", "exit 0");
        let e = compile(&f.engine(), work_with_main(), "main.tex", &Options::default()).unwrap_err();
        assert_eq!(e.headline, "LuaLaTeX made no PDF");
    }

    // spec: TEX-004
    #[test]
    fn a_run_that_takes_too_long_is_killed_with_what_it_started() {
        let f = Fake::new("timeout");
        script(&f.dir.join("bin"), "lualatex", "echo started\nsleep 30\necho never");
        let work = work_with_main();
        let path = work.path().to_path_buf();
        let t = Instant::now();
        let e = compile(&f.engine(), work, "main.tex", &Options { timeout: Duration::from_secs(1), ..Options::default() }).unwrap_err();
        assert!(t.elapsed() < Duration::from_secs(15), "the sleep was killed too, not waited for: {:?}", t.elapsed());
        assert!(e.headline.contains("ran for more than 1 seconds") && e.headline.starts_with("LuaLaTeX"), "{}", e.headline);
        assert!(e.log_tail.contains("started"));
        assert!(!path.exists());
    }

    // spec: TEX-004
    #[test]
    fn an_engine_that_cannot_be_started_says_so() {
        let f = Fake::new("nostart");
        let e = compile(&Engine::new(Kind::System, f.dir.join("nothing-here")), work_with_main(), "main.tex", &Options::default()).unwrap_err();
        assert!(e.headline.contains("could not be started"), "{e}");
    }

    // spec: TEX-004
    #[test]
    fn a_main_file_that_could_be_a_command_is_refused_before_anything_runs() {
        let f = Fake::new("badname");
        f.lualatex();
        let e = compile(&f.engine(), WorkDir::new().unwrap(), "\\input{x}.tex", &Options::default()).unwrap_err();
        assert!(e.headline.contains("not a usable file name"));
        assert_eq!(f.get("count"), "", "LuaLaTeX was not started");
    }

    // spec: TEX-004
    #[test]
    fn the_test_document_is_compiled_with_its_bibliography() {
        let f = Fake::new("testdoc");
        f.lualatex();
        f.put("log.last", "ok\n");
        let out = compile_test_document(&f.engine()).unwrap();
        assert!(out.pdf.starts_with(b"%PDF"));
        assert!(f.get("args").contains("test.tex"));
        assert!(TEST_DOCUMENT.contains("biblatex") && TEST_DOCUMENT.contains("test.bib") && TEST_BIB.contains("@book{shale"));
    }

    // spec: TEX-004
    #[test]
    fn a_failed_compile_keeps_its_log_when_a_folder_is_given_and_a_good_one_keeps_nothing() {
        let kept = std::env::temp_dir().join(format!("shale-failed-logs-compile-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&kept);
        let f = Fake::new("keep");
        f.lualatex();
        f.put("log.last", MISSING_PACKAGE);
        f.put("exit", "1");
        let e = compile_keeping_logs(&f.engine(), work_with_main(), "main.tex", &Options::default(), Some(&kept)).unwrap_err();
        assert_eq!(e.headline, "LuaLaTeX stopped with an error");
        let files: Vec<PathBuf> = std::fs::read_dir(&kept).unwrap().map(|e| e.unwrap().path()).collect();
        assert_eq!(files.len(), 1, "{files:?}");
        assert_eq!(std::fs::read_to_string(&files[0]).unwrap(), MISSING_PACKAGE, "the whole log, not its tail");
        // without a folder nothing is kept; a compile that works keeps nothing either
        compile_keeping_logs(&f.engine(), work_with_main(), "main.tex", &Options::default(), None).unwrap_err();
        std::fs::remove_file(f.state().join("exit")).unwrap();
        f.put("log.last", "all done\n");
        compile_keeping_logs(&f.engine(), work_with_main(), "main.tex", &Options::default(), Some(&kept)).unwrap();
        assert_eq!(std::fs::read_dir(&kept).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&kept);
    }
}

// ---------------------------------------------------------------- the real LuaLaTeX (container)

/// The real LuaLaTeX of the image `shale-tex-env`: every program runs through
/// `tests/tex-env/tex-env.sh exec <work folder> --`.
pub(super) fn container_engine(work: &Path) -> Engine {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/tex-env/tex-env.sh");
    let mut e = Engine::new(Kind::System, PathBuf::new());
    e.wrapper = vec!["sh".into(), script.to_string_lossy().into_owned(), "exec".into(), work.to_string_lossy().into_owned(), "--".into()];
    e
}

/// Compile a document in the container; the work folder is made here so the wrapper knows it.
fn container_compile(files: &[(&str, &str)], main: &str, opts: &Options) -> Result<CompileOutput, CompileError> {
    let work = WorkDir::new().unwrap();
    for (name, text) in files {
        work.write(name, text.as_bytes()).unwrap();
    }
    let engine = container_engine(work.path());
    compile(&engine, work, main, opts)
}

// spec: TEX-004
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_a_small_article_compiles_to_a_pdf() {
    crate::env_lock::hold();
    let out = container_compile(&[("main.tex", "\\documentclass{article}\n\\begin{document}\nHello \\LaTeX.\n\\end{document}\n")], "main.tex", &Options::default()).unwrap_or_else(|e| panic!("{e}\n{}", e.log_tail));
    assert!(out.pdf.starts_with(b"%PDF"), "a PDF");
    assert_eq!(out.runs, 1);
    assert!(!out.font_cache_built, "the image has the font names database (#327)");
}

// spec: TEX-004
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_a_work_folder_and_a_template_folder_with_spaces_work() {
    crate::env_lock::hold();
    let parent = std::env::temp_dir().join(format!("shale tex space {}", std::process::id()));
    let work = WorkDir::new_in(&parent).unwrap();
    let tpl = work.path().join("my templates");
    std::fs::create_dir_all(&tpl).unwrap();
    work.write("main.tex", b"\\documentclass{article}\n\\usepackage{spacey}\n\\begin{document}\\spacey\n\\end{document}\n").unwrap();
    std::fs::write(tpl.join("spacey.sty"), "\\ProvidesPackage{spacey}\\newcommand\\spacey{ok}").unwrap();
    let engine = container_engine(work.path());
    let opts = Options { tex_inputs: vec![tpl], ..Options::default() };
    let out = compile(&engine, work, "main.tex", &opts).unwrap_or_else(|e| panic!("{e}\n{}", e.log_tail));
    assert!(out.pdf.starts_with(b"%PDF"));
    let _ = std::fs::remove_dir_all(&parent);
}

// spec: TEX-004
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_a_missing_package_is_reported_as_missing() {
    crate::env_lock::hold();
    let e = container_compile(&[("main.tex", "\\documentclass{article}\n\\usepackage{nosuchpackage}\n\\begin{document}x\\end{document}\n")], "main.tex", &Options::default()).unwrap_err();
    assert_eq!(e.problems.iter().map(Problem::text).collect::<Vec<_>>(), ["package nosuchpackage is missing"], "{e}\n{}", e.log_tail);
}

// spec: TEX-004
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_a_tex_error_has_file_and_line() {
    crate::env_lock::hold();
    let e = container_compile(&[("main.tex", "\\documentclass{article}\n\\begin{document}\nfine\n\\nosuchmacro\n\\end{document}\n")], "main.tex", &Options::default()).unwrap_err();
    let p = &e.problems[0];
    assert_eq!((p.file.as_deref().map(|f| f.trim_start_matches("./")), p.line, p.message.as_str()), (Some("main.tex"), Some(4), "Undefined control sequence."), "{e}");
}

// spec: TEX-004
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_a_document_with_biblatex_runs_biber_and_resolves_the_citation() {
    crate::env_lock::hold();
    let out = container_compile(&[("test.tex", TEST_DOCUMENT), ("test.bib", TEST_BIB)], "test.tex", &Options::default()).unwrap_or_else(|e| panic!("{e}\n{}", e.log_tail));
    assert!(out.runs >= 2, "LuaLaTeX, biber, LuaLaTeX: {} runs", out.runs);
    assert!(!out.font_cache_built, "the image has the font names database, a fresh container does not build it (#327)");
}

// spec: TEX-004
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_write18_stays_disabled_and_output_stays_in_the_work_folder() {
    crate::env_lock::hold();
    // \write18 only prints the command instead of running it
    let work = WorkDir::new().unwrap();
    work.write("main.tex", b"\\documentclass{article}\n\\begin{document}\n\\immediate\\write18{touch hacked.txt}\nok\n\\end{document}\n").unwrap();
    let path = work.path().to_path_buf();
    let engine = container_engine(&path);
    // keep the folder to look into it: compile removes it, so run the program the way compile does
    let mut cmd = engine.command("lualatex");
    cmd.args(lualatex_args("main.tex")).current_dir(&path).env("openout_any", "p");
    let out = cmd.output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
    assert!(!path.join("hacked.txt").exists(), "\\write18 did not run a command");
    drop(work);
    // a document that writes into the parent folder is refused by the engine and the refusal is read from the log
    let escape = "\\documentclass{article}\n\\begin{document}\n\\immediate\\openout3=../escaped.txt\n\\immediate\\write3{x}\n\\end{document}\n";
    let e = container_compile(&[("main.tex", escape)], "main.tex", &Options::default()).unwrap_err();
    assert!(e.problems.iter().any(|p| p.message.contains("can't write on file")), "{e}");
    assert!(!std::env::temp_dir().join("escaped.txt").exists());
}
