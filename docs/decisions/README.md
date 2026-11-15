# Decision records

This folder holds the **decision records** (ADRs, `NNNN-title.md`): *why* Shale is built the
way it is. They are append-only. *What* Shale does today is in the living specs in
[`docs/spec/`](../spec/README.md). How the two work together: [ADR-0001](0001-spec-driven-development.md).

## Decision records

| ADR | Title | Status |
|---|---|---|
| [0001](0001-spec-driven-development.md) | Spec-driven development with IS NOW / SHOULD BE specs | Proposed |
| [0002](0002-documentation-structure.md) | One job per document | Proposed |
| [0003](0003-data-safety.md) | Data safety: never lose or corrupt a user's file | Proposed |
| [0004](0004-link-resolution-and-paths.md) | Link resolution, link rewriting and vault paths | Accepted |
| [0005](0005-where-state-lives.md) | Where state lives: global settings vs. per-vault data | Proposed |
| [0006](0006-engineering-baseline.md) | Engineering baseline: CI gates, background jobs, external programs, structure | Proposed |
| [0007](0007-mail-client.md) | A built-in mail view (IMAP), and later sending and signing | Proposed |
| [0008](0008-git-sync.md) | Integrated git sync | Proposed |
| [0009](0009-mail-server-writes.md) | Mail phase 2: writing to the server (flags, move, delete) | Proposed |
| [0010](0010-sending-mail.md) | Mail phase 3: writing and sending mail (SMTP) | Proposed |
| [0011](0011-mail-encryption.md) | Mail phase 4: OpenPGP and S/MIME with the system's GnuPG | Proposed |
| [0012](0012-social-feeds.md) | Social feeds: Mastodon and Bluesky, saved as notes | Proposed |
| [0013](0013-forgejo-and-issue-tracking.md) | Forgejo is the home of development; its issues track all work; GitHub builds and distributes releases | Accepted |
| [0014](0014-sidebar-layout.md) | The sidebar layout: one list of icons per menubar, stacked views on the right | Accepted |

Format: Context, Decision, Consequences (and Alternatives if any). Statuses: *Proposed* →
*Accepted* → (*Superseded by NNNN*). An accepted ADR is not edited; a new one supersedes it.

## Review 2026-11-14

Two reviewers looked at the README, the docs and the code: the lead, and a second agent who
checked every claim against the code and looked for anything missed. The findings below are the
basis for the specs and ADRs.

### Good: keep it

- **Plain data, no lock-in**: `.md` notes, Excalidraw JSON with unknown fields kept, iCalendar.
  No database.
- **Pure, well-tested core modules**: `markdown`, `lists`, `cal_ics`, `cal_time`, `mail` and
  `drawing_layout`.
- **Strong testing culture**: 179 tests.
  - Headless egui UI tests click the real controls.
  - Real-window runs use `--screenshot --keys`.
  - A pretend CalDAV server is driven through real `curl`.
  - A UTF-8 torture test.
- **Security for CalDAV is thought through**:
  - One chokepoint, `request_with`, with https only and limited redirects.
  - Passwords in the keyring, sent to `curl` on stdin with `-q`.
  - XML read without DTDs.
  - External programs started without a shell.
- **Safe-by-default moves in places**:
  - Trash instead of delete.
  - Calendar moves write the new copy before deleting the old one.
  - Drops ask mail programs for a copy only.
  - Import folders cannot leave the vault.
  - Broken drawings open read-only.
- **An action queue** (`enum Action`) decouples the UI from the main mutations.
- **Release discipline**: semver and a changelog, and CI checks that the tag matches the version
  and that release notes exist.
- **Few dependencies**: a pure-Rust PDF stack and no date library.

### Bad: fix or decide

| Finding | Severity | Spec / ADR |
|---|---|---|
| Note and drawing writes are not atomic (plain `fs::write`) | high | DATA-004, ADR-0003 |
| An outside change made while typing is silently overwritten by autosave; drawings are never reloaded | high | DATA-002/003, ADR-0003 |
| A note that cannot be read opens empty and is overwritten on the first key | high | DATA-008, ADR-0003 |
| Write errors are swallowed; a broken config resets to defaults and loses the calendars | high | DATA-009, CFG-003/004 |
| Rename can move a note outside the vault (`../../x`, `/tmp/x`) | medium | VAULT-018, ADR-0004 |
| Rename rewrites links by name text: it drops folder parts, rewrites links to other same-named files, loses `#heading` in markdown links, and compares ASCII only. Folder rename rewrites nothing | medium | VAULT-013/016, ADR-0004 |
| A path link to a missing file silently falls back to any same-named file and shows blue | medium | LINK-003/010, ADR-0004 |
| Per-vault state lives in the global config; the config has no version | medium | CFG-005, ADR-0005 |
| A panic in a background job leaves its spinner running forever | medium | JOB-002, ADR-0006 |
| `xdg-open` gets server URLs and mail attachments without a check | medium | SEC-006, ADR-0006 |
| CI does not run on PRs and has no clippy, fmt or `-D warnings`; actions are not pinned by SHA | low | BUILD-004 |
| 22 crates are vendored only for an old offline sandbox, so they get no upstream fixes | low | BUILD-006 |
| Per frame: `all_tasks()` and `backlinks()`; the rescan is O(n²) | low | PERF-001/002 |
| `ShaleApp` is a god object with ~50 fields, its `impl` is spread over 3 files, and 38 modules sit flat | low | ADR-0006 §6 |
| No spec layer; `IMPLEMENTATION.md` mixes four jobs and has drifted (stale backlog, dependency list, test count, sandbox notes, two keyboard tables) | process | ADR-0001, ADR-0002 |
| Symlinks are skipped silently; the trash forgets where things came from | low | VAULT-007/019 |

### Where the reviewers disagreed

- **Specs and ADRs in one folder?** The second reviewer argued that ADRs are fixed records and
  specs are living, so they should not be mixed. Resolution: two sibling folders,
  `docs/decisions/` (append-only) and `docs/spec/` (changes with the code).
- **How far to spec**: we agreed to spec guarantees (data, formats, links, security, sync,
  keys), about 5–15 rules per area, and not layout, wording or heuristics.
- **The lead's first draft** said rename at least handled path links. The second reviewer showed
  that it matches them but rewrites them wrongly (it drops the folder and picks links by name).
  That changed VAULT-013 into a documented defect.
