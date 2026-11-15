//! `Secret`: a password, token or client secret that is wiped from memory when it is dropped
//! (SEC-014, SEC-015).
//!
//! The text lives in a fixed-size heap buffer that never reallocates, so typing into it leaves no
//! stray copies behind (a growing `String` leaves its old, smaller buffers in freed memory). The
//! buffer is locked into RAM with `mlock` where the system allows it, so it is not written to
//! swap (SEC-018 says what happens when it does not), and every byte of it is zeroed when the
//! `Secret` is dropped.
//!
//! `Secret` is also an egui `TextBuffer`: a password field edits the locked buffer directly.

use eframe::egui;
use eframe::egui::text::CharIndex;
use std::fmt;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use zeroize::{Zeroize, Zeroizing};

/// The smallest buffer: room for any normal password or token. A bigger buffer is chosen at
/// creation for a longer text; it never grows afterwards.
const MIN_CAPACITY: usize = 256;
/// Buffer sizes are a multiple of this.
const ROUND: usize = 64;

/// How many times `mlock` failed in this process (RLIMIT_MEMLOCK too low, or no such call).
static LOCK_FAILURES: AtomicUsize = AtomicUsize::new(0);
/// The debug notice about a failed lock is printed once.
static LOCK_FAILURE_TOLD: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// Tests: make `mlock` "fail" on this thread.
    #[cfg(test)]
    static FORCE_LOCK_FAILURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Tests: how many buffers this thread has wiped (the drop path).
    #[cfg(test)]
    static WIPES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Tests: make `mlock` fail (or work again) for buffers made on this thread.
#[cfg(test)]
pub fn fail_locks_on_this_thread(on: bool) {
    FORCE_LOCK_FAILURE.with(|f| f.set(on));
}

/// How many secret buffers could not be locked into memory so far in this run (SEC-018).
pub fn lock_failures() -> usize {
    LOCK_FAILURES.load(Ordering::Relaxed)
}

fn capacity_for(len: usize) -> usize {
    len.max(MIN_CAPACITY).div_ceil(ROUND) * ROUND
}

/// Try to lock `buf` into RAM. False if that did not work.
fn lock(buf: &[u8]) -> bool {
    #[cfg(test)]
    if FORCE_LOCK_FAILURE.with(|f| f.get()) {
        return false;
    }
    #[cfg(unix)]
    {
        // SAFETY: the pointer and length describe a live allocation owned by the caller.
        unsafe { libc::mlock(buf.as_ptr().cast(), buf.len()) == 0 }
    }
    #[cfg(not(unix))]
    {
        let _ = buf;
        false
    }
}

fn unlock(buf: &[u8]) {
    #[cfg(unix)]
    {
        // SAFETY: the pointer and length describe a live allocation that was locked by `lock`.
        // A failure here changes nothing we could do, the memory is freed right after.
        let _ = unsafe { libc::munlock(buf.as_ptr().cast(), buf.len()) };
    }
    #[cfg(not(unix))]
    let _ = buf;
}

/// A secret text in a locked, fixed-size buffer that is zeroed on drop.
pub struct Secret {
    /// The whole capacity; never reallocated. Bytes past `len` are always zero.
    buf: Box<[u8]>,
    /// Bytes in use; `buf[..len]` is valid UTF-8.
    len: usize,
    locked: bool,
}

impl Secret {
    /// A secret holding `text`. The buffer is at least 256 bytes; a longer text gets a bigger
    /// one (rounded up to 64 bytes). It never grows afterwards.
    pub fn new(text: &str) -> Secret {
        let mut buf = vec![0u8; capacity_for(text.len())].into_boxed_slice();
        let locked = lock(&buf);
        if !locked {
            LOCK_FAILURES.fetch_add(1, Ordering::Relaxed);
            if cfg!(debug_assertions) && !LOCK_FAILURE_TOLD.swap(true, Ordering::Relaxed) {
                eprintln!("shale: secrets could not be locked into memory (RLIMIT_MEMLOCK too low?); they are still wiped when dropped");
            }
        }
        buf[..text.len()].copy_from_slice(text.as_bytes());
        Secret { buf, len: text.len(), locked }
    }

    /// The text. Do not copy it into a `String`; pass the `&str` on.
    pub fn expose(&self) -> &str {
        // SAFETY: `buf[..len]` only ever receives whole `&str`s and is cut at char boundaries.
        unsafe { std::str::from_utf8_unchecked(&self.buf[..self.len]) }
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Length in bytes.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Empty the secret (the used bytes are zeroed).
    pub fn clear(&mut self) {
        self.buf[..self.len].zeroize();
        self.len = 0;
    }

    /// Tests: did `mlock` work for this buffer?
    #[cfg(test)]
    fn is_locked(&self) -> bool {
        self.locked
    }

    /// Zero the whole buffer (the first step of dropping).
    fn wipe(&mut self) {
        self.buf.zeroize();
        self.len = 0;
        #[cfg(test)]
        WIPES.with(|w| w.set(w.get() + 1));
    }

    fn byte_of_char(&self, index: CharIndex) -> usize {
        let s = self.expose();
        s.char_indices().nth(index.0).map_or(s.len(), |(i, _)| i)
    }

    /// Tests: the full buffer, including the unused (zero) tail.
    #[cfg(test)]
    fn raw(&self) -> &[u8] {
        &self.buf
    }
}

impl Default for Secret {
    fn default() -> Secret {
        Secret::new("")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.wipe();
        if self.locked {
            unlock(&self.buf);
        }
    }
}

impl Clone for Secret {
    /// A new buffer of its own, locked like any other.
    fn clone(&self) -> Secret {
        Secret::new(self.expose())
    }
}

impl PartialEq for Secret {
    fn eq(&self, other: &Secret) -> bool {
        self.expose() == other.expose()
    }
}
impl Eq for Secret {}

impl PartialEq<str> for Secret {
    fn eq(&self, other: &str) -> bool {
        self.expose() == other
    }
}
impl PartialEq<String> for Secret {
    fn eq(&self, other: &String) -> bool {
        self.expose() == other
    }
}
impl PartialEq<&str> for Secret {
    fn eq(&self, other: &&str) -> bool {
        self.expose() == *other
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl From<&str> for Secret {
    fn from(s: &str) -> Secret {
        Secret::new(s)
    }
}

/// A plain string on the wire: the settings file without a keyring still holds secrets (SEC-003).
impl serde::Serialize for Secret {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.expose())
    }
}

impl<'de> serde::Deserialize<'de> for Secret {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Secret, D::Error> {
        // the intermediate String is zeroed when it goes
        let text = Zeroizing::new(String::deserialize(d)?);
        Ok(Secret::new(&text))
    }
}

impl egui::TextBuffer for Secret {
    fn is_mutable(&self) -> bool {
        true
    }

    fn as_str(&self) -> &str {
        self.expose()
    }

    /// Inserts nothing and returns 0 when the text would not fit: the buffer never grows.
    fn insert_text(&mut self, text: &str, char_index: CharIndex) -> usize {
        if self.len + text.len() > self.buf.len() {
            return 0;
        }
        let at = self.byte_of_char(char_index);
        self.buf.copy_within(at..self.len, at + text.len());
        self.buf[at..at + text.len()].copy_from_slice(text.as_bytes());
        self.len += text.len();
        text.chars().count()
    }

    fn delete_char_range(&mut self, char_range: Range<CharIndex>) {
        let (a, b) = (self.byte_of_char(char_range.start), self.byte_of_char(char_range.end));
        if a >= b {
            return;
        }
        self.buf.copy_within(b..self.len, a);
        let new_len = self.len - (b - a);
        // the freed tail must not keep a copy of what was deleted
        self.buf[new_len..self.len].zeroize();
        self.len = new_len;
    }

    fn clear(&mut self) {
        Secret::clear(self);
    }

    /// Replaces the text in place. A text that does not fit is refused (nothing changes): the
    /// buffer never grows. To hold a longer text make a new `Secret`.
    fn replace_with(&mut self, text: &str) {
        if text.len() > self.buf.len() {
            return;
        }
        self.buf[..self.len].zeroize();
        self.buf[..text.len()].copy_from_slice(text.as_bytes());
        self.len = text.len();
    }

    /// The trait wants a `String` back, so this makes an ordinary copy that is not wiped.
    /// Do not call it for secrets; use `expose` and `clear`.
    fn take(&mut self) -> String {
        let s = self.expose().to_owned();
        self.clear();
        s
    }

    fn type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<Self>()
    }
}

/// The parts joined into one `String` that is zeroed when dropped. The string is made with its
/// exact size, so it never reallocates (a `format!` may, and leaves its old buffers unwiped).
pub fn concat(parts: &[&str]) -> Zeroizing<String> {
    let mut s = Zeroizing::new(String::with_capacity(parts.iter().map(|p| p.len()).sum()));
    for p in parts {
        s.push_str(p);
    }
    s
}

/// Make the process refuse core dumps and reads of its memory by other processes of the same
/// user (`PR_SET_DUMPABLE` 0), but only for a release build: debug builds stay debuggable
/// (SEC-019). Where that is not possible (not Linux) nothing happens.
pub fn harden_process(release: bool) -> std::io::Result<()> {
    if !release {
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        // SAFETY: PR_SET_DUMPABLE takes plain integers and touches no memory of ours.
        if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::TextBuffer;

    fn typed(s: &mut Secret, text: &str) {
        for c in text.chars() {
            let at = CharIndex(s.as_str().chars().count());
            assert_eq!(s.insert_text(&c.to_string(), at), 1);
        }
    }

    // spec: SEC-014
    #[test]
    fn the_capacity_is_at_least_256_and_a_multiple_of_64() {
        assert_eq!(Secret::new("").raw().len(), 256);
        assert_eq!(Secret::new(&"a".repeat(256)).raw().len(), 256);
        assert_eq!(Secret::new(&"a".repeat(257)).raw().len(), 320);
        assert_eq!(Secret::new(&"a".repeat(1000)).raw().len(), 1024);
        assert_eq!(Secret::new(&"a".repeat(1000)).expose().len(), 1000, "a long text is kept whole");
    }

    // spec: SEC-014
    #[test]
    fn typing_up_to_the_capacity_never_moves_or_grows_the_buffer() {
        let mut s = Secret::new("");
        let (addr, cap) = (s.raw().as_ptr(), s.raw().len());
        typed(&mut s, &"x".repeat(cap));
        assert_eq!(s.len(), cap);
        assert_eq!((s.raw().as_ptr(), s.raw().len()), (addr, cap));
        // a move of the struct leaves the bytes where they are
        let moved = Box::new(s);
        assert_eq!(moved.raw().as_ptr(), addr);
    }

    // spec: SEC-014
    #[test]
    fn an_insert_beyond_the_capacity_is_refused() {
        let mut s = Secret::new(&"y".repeat(255));
        assert_eq!(s.insert_text("zz", CharIndex(10)), 0, "two bytes do not fit into one");
        assert_eq!(s.expose(), "y".repeat(255));
        assert_eq!(s.insert_text("z", CharIndex(255)), 1);
        assert_eq!(s.insert_text("z", CharIndex(0)), 0, "full");
        assert_eq!(s.len(), 256);
        assert_eq!(s.raw().len(), 256);
    }

    // spec: SEC-014
    #[test]
    fn inserting_and_deleting_in_the_middle_handles_multibyte_text() {
        let mut s = Secret::new("aöb");
        assert_eq!(s.insert_text("€", CharIndex(2)), 1);
        assert_eq!(s.expose(), "aö€b");
        s.delete_char_range(CharIndex(1)..CharIndex(3));
        assert_eq!(s.expose(), "ab");
        s.delete_char_range(CharIndex(1)..CharIndex(1));
        assert_eq!(s.expose(), "ab", "an empty range deletes nothing");
        s.delete_char_range(CharIndex(1)..CharIndex(99));
        assert_eq!(s.expose(), "a", "past the end is the end");
        assert_eq!(s.insert_text("!", CharIndex(99)), 1);
        assert_eq!(s.expose(), "a!");
    }

    // spec: SEC-014
    #[test]
    fn deleting_zeroes_the_freed_tail() {
        let mut s = Secret::new("hunter2-hunter2");
        s.delete_char_range(CharIndex(0)..CharIndex(8));
        assert_eq!(s.expose(), "hunter2");
        assert!(s.raw()[7..].iter().all(|b| *b == 0), "nothing of the deleted text is left");
        s.clear();
        assert!(s.is_empty());
        assert!(s.raw().iter().all(|b| *b == 0));
    }

    // spec: SEC-014
    #[test]
    fn replace_with_works_in_place_and_refuses_a_text_that_does_not_fit() {
        let mut s = Secret::new("a-longer-old-secret");
        let addr = s.raw().as_ptr();
        s.replace_with("new");
        assert_eq!(s.expose(), "new");
        assert!(s.raw()[3..].iter().all(|b| *b == 0), "no rest of the old text");
        assert_eq!(s.raw().as_ptr(), addr);
        s.replace_with(&"q".repeat(257));
        assert_eq!(s.expose(), "new", "refused: the buffer never grows");
    }

    // spec: SEC-014
    #[test]
    fn take_gives_the_text_and_wipes_the_buffer() {
        let mut s = Secret::new("tok");
        assert_eq!(s.take(), "tok");
        assert!(s.is_empty() && s.raw().iter().all(|b| *b == 0));
        assert!(s.is_mutable());
        let _ = s.type_id();
    }

    // spec: SEC-014
    #[test]
    fn dropping_wipes_the_whole_buffer() {
        let mut s = Secret::new("hunter2");
        // the function the drop runs first
        s.wipe();
        assert!(s.raw().iter().all(|b| *b == 0) && s.is_empty());
        // and a drop really runs it
        let before = WIPES.with(|w| w.get());
        drop(Secret::new("another"));
        assert_eq!(WIPES.with(|w| w.get()), before + 1);
    }

    // spec: SEC-014
    #[test]
    fn debug_and_display_never_show_the_content() {
        let s = Secret::new("hunter2");
        assert_eq!(format!("{s:?}"), "Secret(***)");
        assert_eq!(format!("{s}"), "Secret(***)");
        assert_eq!(format!("{:?}", Some(&s)), "Some(Secret(***))");
    }

    // spec: SEC-003
    #[test]
    fn it_is_a_plain_string_in_json_and_back() {
        let s = Secret::new("p\"w\u{e4}");
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, "\"p\\\"w\u{e4}\"");
        let back: Secret = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
        assert!(serde_json::from_str::<Secret>("5").is_err());
    }

    // spec: SEC-017
    #[test]
    fn concat_joins_without_growing() {
        let s = concat(&["AUTH ", "PLAIN", " ", "abc", "\r\n"]);
        assert_eq!(*s, "AUTH PLAIN abc\r\n");
        assert_eq!(s.capacity(), s.len(), "made with its exact size");
        assert_eq!(*concat(&[]), "");
    }

    // spec: SEC-014
    #[test]
    fn clone_and_compare() {
        let a = Secret::new("one");
        let b = a.clone();
        assert_eq!(a, b);
        assert_ne!(a.raw().as_ptr(), b.raw().as_ptr(), "a clone has its own buffer");
        assert_ne!(a, Secret::new("two"));
        assert_eq!(a, "one");
        assert!(a == *"one");
        assert_eq!(Secret::from("x").expose(), "x");
        assert!(Secret::default().is_empty());
    }

    // spec: SEC-014, SEC-018
    #[test]
    fn a_buffer_is_locked_when_the_system_lets_it_and_still_works_when_not() {
        let before = lock_failures();
        let s = Secret::new("abc");
        // either it worked, or it was counted
        assert!(s.is_locked() || lock_failures() > before);
        drop(s);
        fail_locks_on_this_thread(true);
        let before = lock_failures();
        let mut s = Secret::new("abc");
        fail_locks_on_this_thread(false);
        assert!(!s.is_locked(), "the lock was made to fail");
        assert!(lock_failures() > before, "and it was counted");
        assert_eq!(s.expose(), "abc", "an unlocked secret works the same");
        typed(&mut s, "def");
        assert_eq!(s.expose(), "abcdef");
        drop(s);
    }

    // spec: SEC-019
    #[test]
    fn harden_process_does_nothing_in_a_debug_build() {
        harden_process(false).unwrap();
        #[cfg(target_os = "linux")]
        // SAFETY: PR_GET_DUMPABLE reads one integer.
        assert_eq!(unsafe { libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) }, 1, "still dumpable");
    }

    // spec: SEC-019
    #[cfg(target_os = "linux")]
    #[test]
    fn harden_process_makes_a_release_process_not_dumpable() {
        // dumpable is a property of the whole process, so look at it in a child
        if std::env::var("SHALE_HARDEN_CHILD").is_ok() {
            harden_process(true).unwrap();
            // SAFETY: PR_GET_DUMPABLE reads one integer.
            let d = unsafe { libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) };
            println!("dumpable={d}");
            return;
        }
        let exe = std::env::current_exe().unwrap();
        let out = std::process::Command::new(exe).args(["--exact", "secret::tests::harden_process_makes_a_release_process_not_dumpable", "--nocapture"]).env("SHALE_HARDEN_CHILD", "1").output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("dumpable=0"), "{text}");
    }
}
