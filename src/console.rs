//! The console of a Windows release build (BUILD-041).
//!
//! A release `shale.exe` is a program of the "windows" subsystem, so starting it from Explorer
//! opens no console window. Started from a terminal it has no console either, so `main` calls
//! [`attach_parent`] first: it attaches to the console of the parent process and gives the three
//! standard streams a handle on it, so `--help`, `--version`-like output and command-line errors
//! show up there. A stream that already has a handle (a pipe or a file: `shale … > out.txt`, git's
//! askpass helper, SYNC-054) is left alone.

use std::fs::OpenOptions;
use std::os::windows::io::IntoRawHandle;
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::System::Console::{AttachConsole, GetStdHandle, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};

/// Has a standard stream no handle? A GUI program that was not given one by its parent has a null
/// one (or an invalid one when the call failed).
fn is_missing(handle: windows_sys::Win32::Foundation::HANDLE) -> bool {
    handle.is_null() || handle == INVALID_HANDLE_VALUE
}

/// Attach to the console of the parent process, when it has one, and point the standard streams
/// that have no handle at it. Does nothing when started from Explorer (no parent console).
pub fn attach_parent() {
    // SAFETY: plain Win32 calls without pointers; a failure is the normal case without a console
    if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } == 0 {
        return;
    }
    for (id, device, write) in [(STD_INPUT_HANDLE, "CONIN$", false), (STD_OUTPUT_HANDLE, "CONOUT$", true), (STD_ERROR_HANDLE, "CONOUT$", true)] {
        // SAFETY: as above
        if !is_missing(unsafe { GetStdHandle(id) }) {
            continue;
        }
        if let Ok(file) = OpenOptions::new().read(!write).write(write).open(device) {
            // the handle stays open for the life of the process: it is the stream
            // SAFETY: the handle is valid and owned by nobody else after `into_raw_handle`
            unsafe { SetStdHandle(id, file.into_raw_handle()) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: BUILD-041
    #[test]
    fn build_041_a_stream_without_a_handle_is_missing_and_one_with_a_handle_is_not() {
        assert!(is_missing(std::ptr::null_mut()));
        assert!(is_missing(INVALID_HANDLE_VALUE));
        let file = std::fs::File::open("NUL").expect("the null device opens");
        assert!(!is_missing(std::os::windows::io::AsRawHandle::as_raw_handle(&file)));
    }

    // spec: BUILD-041
    #[test]
    fn build_041_attaching_without_a_parent_console_changes_nothing() {
        // the test runner may or may not have a console; either way the call must neither
        // panic nor close a stream that has a handle
        let before = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
        attach_parent();
        let after = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
        assert!(is_missing(before) || before == after);
    }
}
