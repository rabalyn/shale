# ADR-0043: One file and folder picker: the system's dialog first, Shale's own as the fallback

- **Status**: Accepted (2026-11-25; the owner's go on #690 and #688, with the recommendation of #690)
- **Date**: 2026-11-25
- **Specs**: PICK-001 to PICK-011 ([`docs/spec/file-picker.md`](../spec/file-picker.md)), CFG-084, IMP-075, MAIL-243, VAULT-033, VAULT-034
- **Issues**: #690 (one picker everywhere, a picker in the Import dialog), #688 (New folder, folded into #690)
- **Builds on**: [ADR-0001](0001-spec-driven-development.md) (spec-driven)

## Context

Shale had one picker of its own, `vault_menu::FolderPicker`, for folders of vaults only. Every
other place that needs a path (the Import dialog, the library folders, a template set, the logos,
mail attachments, the place of an export) had a text field to type or paste a path into. The
compose box of the feeds already used the system's dialog through `rfd` (SOC-058). People expect
a file dialog with the shortcuts, recent files, search and previews of their system.

## Decision

1. **The system's dialog first.** `rfd` 0.18 is a dependency already (xdg-desktop-portal on Linux;
   Windows and macOS have their native dialog). It runs on a worker thread (`jobs::spawn`), so the
   window keeps drawing. It has its own New folder button, so Shale adds none to it.
2. **Shale's own picker is the fallback and a setting.** It opens when no system dialog can be
   shown, when the setting "Use the system's file dialog" (`system_file_dialog`, on by default,
   CFG-084) is off, and when a choice was refused and comes back with its reason (the system's dialog
   cannot show one). It is the former folder picker, grown: files, several files, kinds of file,
   a file name for a file to write, shortcuts (PICK-006), and the **New folder** field of #688 (PICK-008).
3. **"No dialog" is detected as in SOC-058.** `rfd` answers `None` for Cancel and for a dialog that
   could not open. On Linux an answer of nothing within 250 ms is "no dialog" and the own picker
   opens; later it is Cancel. Windows and macOS always have one.
4. **One mailbox, not one dialog per place.** A part of the window asks with a key and a
   `Request` and takes the `Picked` answer on a later frame (`file_picker::Hub`). The `Host` in
   `ShaleApp` answers one ask at a time. The hub is also put into the egui context each frame, so
   widgets that have a `Ui` and no app (the mail compose box, the TeX settings, the export dialog)
   ask with `file_picker::ask(ctx, …)`. This keeps the change at each call site to a button and a
   `take`, and no part needs to know which dialog answered.
5. **The system's dialog is behind a trait** (`Native`), with a pretend dialog in the tests; a test
   build has `NoSystem`, so no test can open a real dialog.
6. **Refusals come back in Shale's own picker.** Open…, Create… and Locate… check the folder after
   the dialog answered (it must be there, be empty, not be known already). A refused folder is asked
   again with the reason, the typed path and, for Create…, the offer to open it as a vault.
7. **Filters.** A filter is a name and extensions; none means every file. The system's dialog opens
   on its first filter, so a list that starts with "All files" (the Import dialog) gives it no filter,
   so that nothing is hidden. Shale's own picker shows the list as given.
8. **A name that exists in New folder** (the point #688 left to the spec) is not an error: the
   picker says it is there and offers **Use that folder**, which uses it without touching it. A file of
   that name, a refused name and a failure show their reason and choose nothing.
9. **A box of the request is asked after the system's dialog.** The system's dialog cannot hold the
   box "Start with sample notes" of Create… (VAULT-034). When it has chosen the folder, a small step
   of Shale's shows the folder, the box and Create or Cancel (PICK-011), so both pickers offer the
   same choice. A request without such a box is answered at once.

## Consequences

- `vault_menu::FolderPicker`, `VaultModal::Folder` and `choose_folder` are gone; the vault
  dropdown closes and asks, and `vault_pick_tick` takes the answer.
- `userdirs.rs` has the folders Desktop, Downloads, Pictures, Videos and Music (the only place that
  asks `dirs`, DEV-047).
- Left as it was: the feed compose box keeps its own use of `rfd` (SOC-058). The export dialogs of a
  note, a book and a Word file, and the start screen's path field have a button too (PICK-010,
  VAULT-021); the user guide
  folder (#687) asks with `Request::folder`.
