# ADR-0003: Data safety — never lose or corrupt a user's file

- **Status**: Proposed
- **Date**: 2026-11-14
- **Specs**: DATA-003, DATA-004, DATA-007, DATA-008, DATA-009, VAULT-017

## Context

Shale promises that "nothing is lost" (trash instead of delete, moving calendar events safely).
The review found four ways in which this does not hold today:

1. **Non-atomic writes.** These all use plain `fs::write`, so a crash or full disk can leave a
   truncated file:
   - `Vault::write_note`, `create_file` and `write_raw` (drawings), `vault.rs:418, 435, 449`
   - the link rewrites during rename and move, `vault.rs:516, 556`

   `config::write_private` (`config.rs:160-169`) already shows the safe pattern.
2. **Lost update.** If another program changes a note (Syncthing, `git pull`, a second editor)
   while it has unsaved edits, `ShaleApp::rescan` skips the buffer (`app.rs:592-602`).
   `Vault::rescan` has already stored the disk text, and the next autosave overwrites the other
   change. It is never noticed again, because `write_note` stamps the new mtime. The window is
   any moment of typing (autosave runs 0.8 s after the last key).
3. **Unreadable notes overwritten.** A note that fails to read (invalid UTF-8, no permission)
   becomes `""` (`vault.rs:131` `unwrap_or_default`). It opens as an empty note, and the first
   keystroke replaces the original bytes. Drawings already do this right: they open read-only
   with an error (`drawing.rs:731-733`).
4. **Silent failures.** These problems are never shown to the user:
   - `let _ = v.write_note(…)` (`app.rs:538, 543`)
   - autosave errors (`app.rs:572`)
   - a broken settings file resets to defaults (`config.rs:178-181`), and the next save wipes
     the calendar list

Open drawings are also never reloaded when changed outside.

## Decision (proposed)

1. **All writes of user files go through one function**, `vault::write_atomic`: write a temp
   file in the same folder, fsync, rename over the original, keep the permissions. It is used by
   notes, drawings, link rewrites, imports, `.shale/` data and the config.
2. **Detect conflicts before saving**: remember the disk mtime (and size or hash) when a buffer
   was loaded. If the file on disk differs at save time:
   - write the buffer to `Name (conflict YYYY-MM-DD HHMM).md` next to the original;
   - keep the other program's version as `Name.md`;
   - show a message with both links.

   No dialog is needed, and both versions survive. Drawings get the same rule.
3. **A file that fails to read is opened read-only** with the reason shown. Shale never writes
   over bytes it could not read.
4. **Write and read errors are always shown** (toast, plus a mark on the tab while unsaved). A
   settings file that fails to parse is renamed to `config.json.broken-<date>` before defaults
   are used.
5. **Trash keeps the origin**: `.trash/<date>/<original relative path>`, so a restore is
   possible later and same-named files do not collide.

## Consequences

- The promise "nothing is ever lost" becomes true and testable (DATA-00x).
- Conflict copies may show up when sync tools are busy. That is the intended trade-off: a
  duplicate is better than a lost edit.
- A small amount of extra I/O per save (fsync).

## Alternatives considered

- *Prompt on conflict*: this blocks autosave and is easy to click away. Rejected in favour of
  keeping both versions.
- *File watcher (inotify) instead of polling*: it shrinks the window but does not close it. It
  is still useful later (OPEN-006).

## Decisions

- this is top priority - WE NEVER LOOSE OR CORRUPT DATA. I would even say that its worth to add fuzzy tests for all our write operations (I consider updates/puts as writes in this context). loosing / deleting because of a bug is not tolerable, if this would happen to me I would never touch this application again. therefore I agree to the proposed decisions.
- the consequences are fine.
- inotify is not supported on all filesystems IIRC? its the better approach, but I think we might still need polling as a fallback anyway ?