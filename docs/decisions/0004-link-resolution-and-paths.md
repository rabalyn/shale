# ADR-0004: Link resolution, link rewriting and vault paths

- **Status**: Accepted (2026-11-14)
- **Date**: 2026-11-14
- **Specs**: LINK-002, LINK-003, LINK-008, LINK-010, LINK-012, VAULT-013, VAULT-016, VAULT-018

## Context

Links are Shale's core feature, but their rules exist only in code (`Vault::resolve`,
`vault.rs:214-251`), and the code has rules nobody chose on purpose:

- A **path link with no exact match** silently falls back to any file with the same name
  (`vault.rs:236-238`). `[[Archive/Plan]]` shows **blue** and opens `Projects/Plan.md`.
- A path link is tried **relative to the linking note first**, then from the vault root. `../`
  is never normalised.
- When **names are equal**, the order is: the same folder first, then the fewest folder levels,
  then alphabetical. The user is never told that a link is ambiguous.

Rewriting on rename (`markdown::rename_links`, `markdown.rs:405-425`) has defects:

- It replaces the whole target with the bare new name: `[[Projects/Old]]` becomes `[[New]]`
  and the folder is lost.
- It picks links **by name text**, not by what they resolve to, so `[[Archive/Old]]` is
  rewritten too, even though it pointed to a different file.
- It compares case **ASCII-only** (`eq_ignore_ascii_case`), while the name index uses Unicode
  lowercase (`vault.rs:165`).
- For markdown links the raw target is replaced, so `[x](Old.md#Intro)` loses `#Intro`. This
  also happens in `move_into` (`vault.rs:497-509`).
- Renaming a **folder** rewrites no links at all (`app.rs:639`: `update_links && !is_dir`),
  while moving a folder does.

**Path containment**: import folders are checked (`import::clean_folder`), but rename names are
not. `sanitize_note_name` keeps `/` and `..`, and `parent.join(name)` (`app.rs:635`) puts the
note outside the vault. Typing `../../x` or `/tmp/x` as the new name moves it there.

## Decision (proposed)

1. **Resolution rules** (specified as LINK-002, LINK-003 and LINK-010, and kept compatible
   with Obsidian vaults):
   - A bare name resolves by name: same folder, then fewest levels, then alphabetical (as
     today).
   - A path resolves from the vault root; `./` and `../` resolve relative to the linking note.
     Only if that does not exist is the note's folder tried as a base (kept for compatibility).
   - A path that matches nothing is **red**. It no longer falls back to the name.
   - An ambiguous name is shown (tooltip: "2 notes are called Plan – opens Projects/Plan").
2. **One rewrite routine** for rename and move: select links by **what they resolve to**
   before the change, then write the shortest form that still resolves to the new place. A
   link that was written by name stays a name; a path keeps its folder part. Alias, heading,
   `<…>` and `.md` are kept. Folder rename uses the same routine.
3. **One checked path type**, `VaultPath`. It holds a relative path with no `..` and no
   absolute part, and it can only be built through a checking constructor. Every path the user
   types (rename, new note, new folder, picker, import) goes through it, and `Vault::abs` takes
   only `VaultPath`.

## Consequences

- Some links that are blue today turn red: path links pointing to a file that is not there. This
  is correct, but it is a visible change and needs a CHANGELOG entry.
- Folder rename becomes safe, and rename and move share one tested routine.
- `VaultPath` touches many signatures. It can be introduced gradually: first at the
  user-input boundaries, then inward.

## Open

- ~~Should Shale offer "fix this link" for red path links whose name exists elsewhere? (OPEN-003)~~
  Decided, see below.

## Decisions

Owner, 2026-11-14:

- Implement as proposed (§1–§3).
- OPEN-003: fix links automatically whenever the target can be determined unambiguously, with no
  user interaction. Show a "fix this link" action only where no automatic fix is possible
  (LINK-012).

Notes on the implementation:

- §2 is implemented as "every link keeps pointing at the file it pointed at": after a rename or
  move, any link whose resolution would change is rewritten. Besides links to what moved, this
  also covers relative links inside a moved note and `[[Name]]` links to another file that a new
  name would otherwise take over.
- §3: `VaultPath` checks every path at the user-input boundaries. `Vault::abs` still takes a
  plain `&Path` (the inward step from the Consequences is left for later).
- Automatic fixing (LINK-012) runs when a note is opened, not on every scan: a scan can see a
  half-synced vault, and writing to notes the user is not looking at would surprise them.
