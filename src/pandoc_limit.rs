//! The memory limit of every pandoc that Shale starts (IMP-062, DOCX-012, rabalyn/shale#528). pandoc is a
//! Haskell program: its heap grows until the computer is out of memory unless GHC's runtime is told a
//! maximum (`-M`). A 17 KB Word file with deeply nested table cells made pandoc run for 4 hours and use 15 GB.
//! The time limit of the import and the export (120 s) does not help against a heap that fills the memory
//! before then, so every caller starts pandoc through [`run`].
//!
//! The limit goes in the environment (`GHCRTS=-M2g`), not among the arguments (`+RTS -M2g -RTS`): pandoc
//! accepts both, and the argument lists of the import (IMP-035) and the export (DOCX-002) stay as they are.
//! A pandoc built without `-rtsopts` refuses runtime options ("Most RTS options are disabled", exit 1): the
//! run is then started again without the limit, and the program is remembered, so that this costs one extra
//! start once. The time limit still holds for such a pandoc.

use crate::pdf::tex::proc::{Ending, Run};
use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{LazyLock, Mutex};

/// The most memory a pandoc run may use, as GHC's `-M` says it. Word files of hundreds of pages need less than
/// 300 MB; 2 GB leaves room for the largest real ones and stays far below the memory of a computer.
pub const MAX_HEAP: &str = "2g";

/// The same in words, for the message.
pub const MAX_HEAP_TEXT: &str = "2 GB";

/// The exit code of a GHC program whose heap overflowed.
const HEAP_OVERFLOW_EXIT: i32 = 251;

/// The programs that refused the runtime options: they are run without the limit.
static REFUSED: LazyLock<Mutex<HashSet<PathBuf>>> = LazyLock::new(Default::default);

/// Put the limit `heap` (GHC's size syntax, `2g`) in the environment of `cmd`, after the runtime options that the user may have set.
fn set_limit(cmd: &mut Command, heap: &str) {
    let mut value = std::env::var_os("GHCRTS").unwrap_or_default();
    if !value.is_empty() {
        value.push(" ");
    }
    value.push(format!("-M{heap}"));
    cmd.env("GHCRTS", value);
}

/// Start pandoc: `setup` adds the arguments (and the folder) to the command made for `pandoc`, and `go` runs it (for
/// example with `proc::run`). The command has the memory limit; if this pandoc refuses runtime options, `go` is called
/// again with a command without it.
pub fn run(pandoc: &Path, setup: impl Fn(&mut Command), go: impl Fn(Command) -> io::Result<Run>) -> io::Result<Run> {
    run_with(pandoc, MAX_HEAP, setup, go)
}

/// [`run`] with another limit (the tests use a tiny one).
pub fn run_with(pandoc: &Path, heap: &str, setup: impl Fn(&mut Command), go: impl Fn(Command) -> io::Result<Run>) -> io::Result<Run> {
    let limited = !REFUSED.lock().unwrap_or_else(|e| e.into_inner()).contains(pandoc);
    let make = |limit: bool| {
        let mut cmd = crate::program::command(pandoc);
        setup(&mut cmd);
        if limit {
            set_limit(&mut cmd, heap);
        }
        cmd
    };
    let run = go(make(limited))?;
    if limited && refused(&run) {
        REFUSED.lock().unwrap_or_else(|e| e.into_inner()).insert(pandoc.to_path_buf());
        return go(make(false));
    }
    Ok(run)
}

/// Whether pandoc refused the runtime options (a build without `-rtsopts`).
fn refused(run: &Run) -> bool {
    run.ending == Ending::Exited(Some(1)) && run.tail.iter().any(|l| l.contains("RTS options are disabled"))
}

/// Whether the run ended because its heap reached the limit: GHC's message ("Heap exhausted", "heap overflow") or its exit code 251.
pub fn overflowed(run: &Run) -> bool {
    run.ending == Ending::Exited(Some(HEAP_OVERFLOW_EXIT)) || (matches!(run.ending, Ending::Exited(Some(c)) if c != 0) && run.tail.iter().any(|l| l.contains("Heap exhausted") || l.contains("heap overflow")))
}

/// What the dialog says when the limit stopped pandoc; `file`: the Word file the import names.
pub fn overflow_message(file: Option<&str>) -> String {
    match file {
        Some(file) => format!("pandoc needed more than {MAX_HEAP_TEXT} of memory for “{file}” and was stopped"),
        None => format!("pandoc needed more than {MAX_HEAP_TEXT} of memory and was stopped"),
    }
}
