//! Tests against the real LuaLaTeX of the container environment (`tests/tex-env/`, TEX-020):
//! the tuda-ci 4.06 demos must compile. They are ignored unless asked for, because they need the
//! image:
//!
//! ```text
//! tests/tex-env/tex-env.sh build
//! cargo test -- --ignored tex_env
//! ```
//!
//! Each demo is compiled in a fresh temporary copy of the demo files and the generated tuda-ci
//! classes, as the upstream `latexmkrc` does: from inside the examples, with graphicx' `demo`
//! option (boxes instead of the TU logo, which cannot be redistributed, TEX-007).

use super::compile::{keep_failed_log, FAILED_LOGS_VAR};
use std::path::{Path, PathBuf};
use std::process::Command;

fn env_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("tex-env")
}

/// The TeX put before every demo: graphicx draws boxes, and the version macros the `.dtx` fills in
/// when it is built from the repository are set.
const PRE: &str = r"\PassOptionsToPackage{demo}{graphicx}\def\fileversion{0.0}\def\filedate{0000/00/00}";

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        if e.path().is_file() {
            std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
        }
    }
}

/// Compile one demo in its own temporary folder; the PDF bytes or the tail of the output.
fn compile(demo: &str) -> Result<Vec<u8>, String> {
    let cache = env_dir().join("cache").join("tuda-ci");
    if !cache.join("tex").join("tudapub.cls").exists() {
        return Err("tuda-ci is not there: run tests/tex-env/tex-env.sh build".into());
    }
    let work = std::env::temp_dir().join(format!("shale-tex-env-{}-{demo}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    copy_dir(&cache.join("examples"), &work);
    copy_dir(&cache.join("tex"), &work.join("tex"));
    let out = Command::new("sh").arg(env_dir().join("tex-env.sh")).args(["compile"]).arg(&work).arg(format!("{demo}.tex")).env("SHALE_TEX_PRE", PRE).output().map_err(|e| format!("cannot start tex-env.sh: {e}"))?;
    let pdf = std::fs::read(work.join(format!("{demo}.pdf")));
    // the log of a failed demo is kept before its folder goes (SHALE_TEX_FAILED_LOGS, #327)
    if !out.status.success() || pdf.is_err() {
        if let Some(folder) = std::env::var_os(FAILED_LOGS_VAR).filter(|f| !f.is_empty()) {
            keep_failed_log(Path::new(&folder), &work, &format!("{demo}.tex"));
        }
    }
    let _ = std::fs::remove_dir_all(&work);
    match (out.status.success(), pdf) {
        (true, Ok(pdf)) => Ok(pdf),
        _ => {
            let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
            let tail: Vec<&str> = text.lines().rev().take(25).collect();
            Err(tail.into_iter().rev().collect::<Vec<_>>().join("\n"))
        }
    }
}

fn page_count(pdf: Vec<u8>) -> Result<usize, String> {
    let pdf = hayro::hayro_syntax::Pdf::new(pdf).map_err(|e| format!("not a readable PDF ({e:?})"))?;
    Ok(pdf.pages().len())
}

/// The demos of the pinned tuda-ci checkout, by file name without `.tex`.
fn demos() -> Vec<String> {
    let dir = env_dir().join("cache").join("tuda-ci").join("examples");
    let mut names: Vec<String> =
        std::fs::read_dir(&dir).unwrap_or_else(|_| panic!("{} is not there: run tests/tex-env/tex-env.sh build", dir.display())).filter_map(|e| e.ok()?.file_name().to_str()?.strip_suffix(".tex").map(str::to_owned)).collect();
    names.sort();
    names
}

// spec: TEX-020, TEX-021
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn every_tuda_ci_demo_compiles_to_a_pdf_with_pages_tex_env() {
    // the image follows the current TeX Live (ADR-0020): say which one this run is against
    if let Ok(out) = Command::new("sh").arg(env_dir().join("tex-env.sh")).arg("status").output() {
        eprintln!("{}", String::from_utf8_lossy(&out.stdout));
    }
    let names = demos();
    assert_eq!(names.len(), 25, "the demos of tuda-ci 4.06: {names:?}");
    // four at a time: each one is a container start and several LuaLaTeX runs
    let queue = std::sync::Mutex::new(names.iter());
    let failures: Vec<String> = std::thread::scope(|s| {
        let workers: Vec<_> = (0..4)
            .map(|_| {
                s.spawn(|| {
                    let mut failed = Vec::new();
                    loop {
                        let Some(name) = queue.lock().unwrap().next() else { return failed };
                        match compile(name).and_then(page_count) {
                            Ok(n) if n >= 1 => {}
                            Ok(_) => failed.push(format!("{name}: the PDF has no page")),
                            Err(e) => failed.push(format!("{name}:\n{e}")),
                        }
                    }
                })
            })
            .collect();
        workers.into_iter().flat_map(|w| w.join().unwrap()).collect()
    });
    assert!(failures.is_empty(), "{} of {} demos failed:\n{}", failures.len(), names.len(), failures.join("\n---\n"));
}

// spec: TEX-022
#[test]
fn the_package_list_is_one_name_per_line_without_duplicates() {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/tex/packages.txt")).unwrap();
    let names: Vec<&str> = text.lines().map(|l| l.split('#').next().unwrap().trim()).filter(|l| !l.is_empty()).collect();
    assert!(names.contains(&"luatex") && names.contains(&"biber"));
    for n in &names {
        assert!(n.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)), "one package per line: {n:?}");
    }
    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), names.len(), "no package twice");
}
