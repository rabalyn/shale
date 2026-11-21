//! The machine-wide lock of the podman test stacks (DEV-034, #420), the Rust side of
//! `tests/env-lock.sh`. The stacks (`shale-mail-env`, `shale-caldav-env`, the image `shale-tex-env`)
//! have fixed names per machine, so two checkouts must not use them at the same time. Every
//! ignored env test calls [`hold`] first; it takes the one lock file for the life of the test
//! process (a `static`, released by the OS at exit), so `cargo test -- --ignored mail_env_ …` in
//! one worktree waits while `scripts/land.sh` or another worktree runs its suites.
//!
//! The protocol is the shell's: the holder writes a random token into the lock file and exports it
//! as `SHALE_ENV_LOCK_TOKEN`; a process started by a holder trusts it only when the file holds
//! exactly that token and the lock is held right now, so it does not wait for its own parent (and a
//! stale or inherited token never skips the lock). A holder here exports the token too, so the
//! `mail-env.sh` and `tex-env.sh` the tests start do not wait for the test process.
//!
//! Unix only: the stacks run on Linux only (CI env-tests, `scripts/land.sh`), and a Windows file
//! lock is mandatory, so the holder's token in the locked file could not be read by anyone else
//! (os error 33). On other systems [`hold`] does nothing.
#![cfg(test)]

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::{acquire, held_by_parent, hold};

/// No lock where the stacks do not run (see the module comment).
#[cfg(not(unix))]
pub fn hold() {}
