//! Running git for a publish so that Cancel can stop it (SITE-065): the program is started like
//! every git of Shale (`Git::command`: no prompts, a clean environment), and a request to stop
//! kills it between two looks. A publish that is stopped never moved the remote's branch: the
//! push is the last step and git changes the remote's ref only when it has everything.

use super::Failure;
use crate::busy::Cancel;
use crate::gitsync::{hide_passwords, Git, Out};
use std::ffi::OsStr;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

fn drain(pipe: Option<impl Read + Send + 'static>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut p) = pipe {
            let _ = p.read_to_end(&mut bytes);
        }
        bytes
    })
}

/// Run `git -C dir args…` and wait for it; a request to stop kills it and gives [`Failure::Cancelled`].
pub(super) fn run(git: &Git, dir: &Path, args: &[&OsStr], cancel: &Cancel) -> Result<Out, Failure> {
    if cancel.is_cancelled() {
        return Err(Failure::Cancelled);
    }
    let mut child = git.command(dir, args).spawn().map_err(|e| Failure::Error(format!("git could not be started ({}): {e}", git.program.display())))?;
    let (out, err) = (drain(child.stdout.take()), drain(child.stderr.take()));
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if cancel.is_cancelled() => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = (out.join(), err.join());
                return Err(Failure::Cancelled);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => return Err(Failure::Error(format!("git could not be waited for: {e}"))),
        }
    };
    let stdout = out.join().unwrap_or_default();
    let stderr = hide_passwords(String::from_utf8_lossy(&err.join().unwrap_or_default()).trim());
    Ok(Out { ok: status.success(), stdout, stderr })
}

/// [`run`], and a failed command is an error with git's own message.
pub(super) fn ok(git: &Git, dir: &Path, args: &[&OsStr], cancel: &Cancel) -> Result<Out, Failure> {
    let o = run(git, dir, args, cancel)?;
    if o.ok {
        return Ok(o);
    }
    let what = args.iter().take(3).map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" ");
    Err(Failure::Error(if o.stderr.is_empty() { format!("git {what} failed") } else { format!("git {what}: {}", o.stderr) }))
}
