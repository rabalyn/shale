//! Running a program for the TeX export (TEX-002, TEX-004): without a shell, in a process group of
//! its own, with a timeout and a cancel flag. A program that is stopped takes its children with
//! it (a `sh` that started `sleep`, `install-tl` that started `perl`, a container client), so no
//! pipe stays open and nothing is left running.

use std::collections::VecDeque;
use std::io::{self, Read};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How many of the last output lines a run keeps (for the tail of a failure).
const KEEP_LINES: usize = 400;

/// How long to keep reading what a finished program left in its pipes.
const DRAIN: Duration = Duration::from_secs(2);

/// How a run ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Ending {
    /// The program ended with this exit code (`None`: stopped by a signal).
    Exited(Option<i32>),
    /// The program ran longer than the timeout and was killed.
    TimedOut,
    /// The cancel flag was set and the program was killed.
    Cancelled,
}

/// What a run did.
#[derive(Debug)]
pub struct Run {
    pub ending: Ending,
    /// The last lines of its standard output and standard error, mixed, in order.
    pub tail: Vec<String>,
}

impl Run {
    pub fn success(&self) -> bool {
        self.ending == Ending::Exited(Some(0))
    }

    /// The tail as text.
    pub fn tail_text(&self) -> String {
        self.tail.join("\n")
    }
}

/// Start `cmd` and wait for it. `on_line` sees every output line (split at `\n` and `\r`, so a
/// progress bar that rewrites its line is seen too) as it arrives. Fails only when the program
/// cannot be started.
pub fn run(cmd: Command, timeout: Duration, cancel: Option<&AtomicBool>, on_line: impl FnMut(&str)) -> io::Result<Run> {
    run_with(cmd, Stdio::piped(), timeout, cancel, on_line)
}

/// [`run`] with the standard output of the program written to `out` (a program that makes a
/// binary file on its standard output): the tail holds its error lines only.
pub fn run_to_file(cmd: Command, out: std::fs::File, timeout: Duration, cancel: Option<&AtomicBool>) -> io::Result<Run> {
    run_with(cmd, Stdio::from(out), timeout, cancel, |_| {})
}

fn run_with(mut cmd: Command, stdout: Stdio, timeout: Duration, cancel: Option<&AtomicBool>, mut on_line: impl FnMut(&str)) -> io::Result<Run> {
    cmd.stdin(Stdio::null()).stdout(stdout).stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = crate::program::spawn(&mut cmd)?;
    let (tx, rx) = mpsc::channel::<String>();
    let readers: Vec<_> = [child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>), child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>)]
        .into_iter()
        .flatten()
        .map(|pipe| {
            let tx = tx.clone();
            std::thread::spawn(move || read_lines(pipe, tx))
        })
        .collect();
    drop(tx);
    let deadline = Instant::now() + timeout;
    let mut tail: VecDeque<String> = VecDeque::new();
    let mut take = |line: String, tail: &mut VecDeque<String>| {
        on_line(&line);
        if tail.len() == KEEP_LINES {
            tail.pop_front();
        }
        tail.push_back(line);
    };
    let mut ending = None;
    let mut ended_at: Option<Instant> = None;
    loop {
        let mut closed = false;
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(line) => take(line, &mut tail),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => closed = true,
        }
        if ending.is_none() {
            if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
                kill_tree(&mut child);
                ending = Some(Ending::Cancelled);
            } else if Instant::now() >= deadline {
                kill_tree(&mut child);
                ending = Some(Ending::TimedOut);
            } else if let Ok(Some(status)) = child.try_wait() {
                ending = Some(Ending::Exited(status.code()));
            }
            if ending.is_some() {
                ended_at = Some(Instant::now());
            }
        }
        if closed {
            if ending.is_some() {
                break;
            }
            // both pipes are closed but the program lives on: wait for it
            std::thread::sleep(Duration::from_millis(20));
        }
        // a grandchild that keeps a pipe open must not hold us for ever
        if ended_at.is_some_and(|t| t.elapsed() > DRAIN) {
            break;
        }
    }
    let _ = child.wait();
    // the readers end by themselves; one whose pipe a stray grandchild still holds is not waited for
    drop(readers);
    Ok(Run { ending: ending.unwrap_or(Ending::Exited(None)), tail: tail.into() })
}

/// Send the lines of a pipe (split at `\n` and `\r`, empty ones left out) until it closes.
fn read_lines(mut pipe: Box<dyn Read + Send>, tx: mpsc::Sender<String>) {
    let mut pending: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 8192];
    let send = |bytes: &[u8]| bytes.is_empty() || tx.send(String::from_utf8_lossy(bytes).into_owned()).is_ok();
    loop {
        let n = match pipe.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        pending.extend_from_slice(&chunk[..n]);
        while let Some(i) = pending.iter().position(|&b| b == b'\n' || b == b'\r') {
            let line: Vec<u8> = pending.drain(..=i).collect();
            if !send(&line[..line.len() - 1]) {
                return;
            }
        }
        // a program that never ends its line must not fill the memory
        if pending.len() > 1 << 20 {
            let line = std::mem::take(&mut pending);
            if !send(&line) {
                return;
            }
        }
    }
    send(&pending);
}

/// Kill the program and everything it started.
fn kill_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        // the program leads a process group of its own (`process_group(0)`): kill all of it
        // SAFETY: `kill` with a negative process id signals a process group; the id is the child's, still unreaped.
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
    }
    #[cfg(windows)]
    {
        let _ = crate::program::command("taskkill").args(["/PID", &child.id().to_string(), "/T", "/F"]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    let _ = child.kill();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    // spec: IMP-035
    #[cfg(unix)]
    #[test]
    fn a_program_that_is_still_open_for_writing_is_started_when_the_file_is_closed() {
        let dir = std::env::temp_dir().join(format!("shale-proc-busy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("prog");
        let mut open = std::fs::File::create(&script).unwrap();
        io::Write::write_all(&mut open, b"#!/bin/sh\necho started\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        // while it is open for writing the system refuses to run it (ETXTBSY); it is closed in a moment
        let closer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            drop(open);
        });
        let run = run(Command::new(&script), Duration::from_secs(20), None, |_| {}).unwrap();
        closer.join().unwrap();
        assert!(run.success() && run.tail == ["started"], "{run:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: DOCX-002
    #[cfg(unix)]
    #[test]
    fn the_standard_output_of_a_program_can_go_to_a_file() {
        let dir = std::env::temp_dir().join(format!("shale-proc-file-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "printf 'a\\000b'; echo oops >&2"]);
        let file = std::fs::File::create(dir.join("out.bin")).unwrap();
        let run = run_to_file(cmd, file, Duration::from_secs(20), None).unwrap();
        assert!(run.success() && run.tail == ["oops"], "{run:?}");
        assert_eq!(std::fs::read(dir.join("out.bin")).unwrap(), b"a\0b");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: IMP-035
    #[test]
    fn a_program_that_is_missing_is_an_error_at_once() {
        let started = Instant::now();
        let e = run(Command::new("/no/such/program"), Duration::from_secs(20), None, |_| {}).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::NotFound);
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
