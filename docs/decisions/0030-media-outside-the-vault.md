# ADR-0030: Media outside the vault: library links and file links

- **Status**: Accepted (owner decision, 2026-11-22)
- **Date**: 2026-11-22
- **Specs**: MEDIA-014…MEDIA-024, CFG-069 ([`docs/spec/features.md`](../spec/features.md), [`docs/spec/vault-and-files.md`](../spec/vault-and-files.md)); SEC-006, VAULT-018, LINK-004, LINK-010…LINK-012 are touched
- **Issues**: #499 (this record, the specs, resolving and showing), #500 (the setting, the library folders in Settings, every import entry point), #501 (Copy into the vault, the vault check, data usage, the sync note), #504 (no file system access on the window's thread)
- **Builds on**: [ADR-0015](0015-media-playback-and-storage.md) (players, media in `z_media/`), [ADR-0004](0004-link-resolution-and-paths.md) (link resolution, paths), [ADR-0005](0005-where-state-lives.md) (what is synced and what is device-local)
- **Amends**: the rule that a vault is self-contained: its media are in the vault (EDIT-003, IMP-031)

## Context

Some people keep big collections of videos, sound and pictures on a disk of their own and do not want a
second copy of each file in `z_media/` (which is also synced and versioned, ADR-0015). Obsidian users
link such files with `file://` addresses. Until now Shale copies every file into the vault, and a link to a
file outside the vault is no link: a `file://` address is plain text, and a path that leads out of the vault
is a red link (LINK-010).

## Decision

1. **A vault is no longer strictly self-contained for media.** A note may link a picture, PDF, sound or
   video that stays where it is. It is shown (MEDIA-016) and opened with the system's program
   (MEDIA-017), nothing more.
2. **Two forms of link**, both chosen by the owner:
   - **A library link**, `![[@Videos/2024/a.mp4]]` (`[[@Videos/doc.pdf]]` for a file that is not
     embedded). `Videos` is a *library folder*: each device maps the name to a local folder in its
     **global** settings (`media_libraries`, CFG-069), not in the vault's `.shale/settings.json`, because
     the folder differs from device to device (`/media/ada/Videos`, `D:\Videos`). The link is written in
     the vault, which a sync carries to every device, and means the same file on each. This is the form
     for big collections.
   - **A file link**, `![](<file:///home/ada/Videos/a.mp4>)` (also `%20`, and `file:///C:/Users/…` on
     Windows). It is Obsidian's form, so notes written there keep working and the other way round. It is
     one file where it is on this computer, for a one-off file outside every library folder; it does not
     travel to other devices.
3. **Resolution.** A file of the vault at the link's path always wins over a library (a folder `@Videos`
   in the vault), so existing links keep working. The rest of a library path must stay inside the library
   folder: no `..`, no drive, no absolute path. A library that is not set up on this device says "Library
   “Videos” is not set up on this device", with no crash and no note made. A new function
   (`Vault::resolve_media`) answers for the callers that **show** a file; `Vault::resolve` keeps
   answering with vault paths only, so an outside path can never reach `abs_checked` or any code that
   writes (VAULT-018).
4. **Media kinds only, display only.** Only files that `media::media_kind` knows (picture, PDF, sound,
   video) are outside links. Other outside targets stay unresolved and are never read; an outside file is
   never read as text and never written, renamed or deleted. `opener` opens an outside file after
   checking its kind (never a program or script, SEC-006).
5. **An outside link is no note.** It is not red, not a node of the graph, not offered by LINK-012's fix
   and not a "not created yet" link; a click opens the file with the system's program and never makes a
   note. A missing file shows a "not found" state with its path.
6. **Outside files are not synced or versioned.** Sync and the history (ADR-0008, ADR-0028) only know the
   vault; the files stay where they are. A vault on a second device shows "not found" or "not set up"
   until the library is set up there. Phase 3 (#501) says so in the sync settings and the player.
7. **The default is Ask.** The setting for what an import, a pick, a paste or a drop does with a file from
   outside the vault is **Ask** by default, with **Copy into the vault** and **Link where they are** as
   the other values (MEDIA-019, #500). Shale never links a file by itself because it lies under a
   library folder: every entry point follows the setting.
8. **The window never waits for these files** (#504, MEDIA-024). The drive of an outside collection
   may be a NAS, a share or a USB disk, where one `stat` hangs for seconds. A frame therefore never asks
   the disk: whether a file is there, its date and whether it is an LFS pointer are looked up on a
   worker and kept per path (asked again after 5 seconds, the old answer shown meanwhile, one check in
   flight per path), pictures are decoded on workers, and the embed shows "Loading …" until the answer is
   there. `Vault::resolve_media` keeps asking the disk for jobs; frames use `Vault::look_media`.

## Consequences

- A vault can show "not found" for a file the vault does not hold; the vault check and data usage
  (#501) list such links so they are not a surprise.
- Backups of the vault no longer hold everything its notes show: the user guide says so.
- A new global setting `media_libraries`; a new field with a default needs no version bump (CFG-001).
- The index does not depend on the libraries: whether a link is an outside link follows from its text
  alone, so the libraries of a device never change what the vault's index holds.

## Alternatives considered

- *Only `file://` links*: simple and compatible, but a link has the folder of one computer in it and breaks
  on every other device of a synced vault. Kept as the second form.
- *Only library links*: portable, but not Obsidian's form and no way to link one file without setting a
  library up first.
- *Libraries in the vault's settings*: they would sync, but the folder differs from device to device.
- *A link that is resolved like a note name over the libraries' contents* (no `@Name/`): needs an index of
  every file of every library, which is the cost these people are avoiding.
- *Resolving outside paths in `Vault::resolve`*: one change in one place, but then every caller that
  turns the result into a path to write would have to know; a separate function keeps the write paths
  vault-only by their types.
