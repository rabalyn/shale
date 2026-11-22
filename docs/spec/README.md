# Shale specs

Living specs: what Shale guarantees today (**IS NOW**) and what it should guarantee (**SHOULD BE**).
How they work: [ADR-0001](../decisions/0001-spec-driven-development.md).

| File | Areas | IDs |
|---|---|---|
| [vault-and-files.md](vault-and-files.md) | Vault, create/rename/move/trash, writing data, settings, git sync, the file history panel and comparison, network shares and cloud folders, data usage and caches | VAULT, DATA, CFG, SYNC, HIST, NET, USE |
| [links-and-editor.md](links-and-editor.md) | Link syntax and resolution, editing, navigation, keys and keybindings | LINK, EDIT, NAV, KEY |
| [features.md](features.md) | Tasks, drawings, calendar, network security, import, people, tags, PDF, window layout (ribbon, right sidebar) | TASK, DRAW, CAL, SEC, IMP, PPL, TAG, PDF, LAY |
| [tex-export.md](tex-export.md) | PDF export through LaTeX templates, the managed TeX Live, template sets, the test stack | TEX |
| [book-export.md](book-export.md) | A folder, up to the vault, as one PDF: the folder convention, the book layout, the dialog and `--export-pdf` | BOOK |
| [word-export.md](word-export.md) | A note as a Word file through pandoc: the dialog, the filter, the reference document, finishing the file | DOCX |
| [social.md](social.md) | Mastodon and Bluesky feeds, saving posts as notes | SOC |
| [updates.md](updates.md) | Checking GitHub releases, the checksum, notify / download / install, the install kinds, restart | UPD |
| [engineering.md](engineering.md) | Forge and work tracking, development rules, build and release, background jobs, performance | FORGE, DEV, BUILD, JOB, PERF |
| [mail.md](mail.md) | Local mail store, IMAP account and sync, flags and moves, sending, signing and encrypting | MAIL |
| [open-points.md](open-points.md) | Undecided questions (open work: GitHub issues) | OPEN |

## Rules for editing

- One rule per row, one testable sentence, present tense. Describe behaviour, not code.
- IDs are stable: never renumber or reuse; mark a dropped rule *Retired*.
- A known defect is an `IS NOW ⚠` row (how it behaves) next to the `SHOULD BE` row (the fix).
- Change the spec **before** the code; flip `SHOULD BE` → `IS NOW` in the same change that
  makes it true, with the test name in the Test column.
- Tests that check a rule carry its ID in the name or a `// spec: ID` comment; a test checks that
  every such comment names an existing row (DEV-014, `tests/spec_refs.rs`).
- The Test column of an `IS NOW` row names tests that exist (`file.rs::test_fn`, a `cargo test` filter or a script),
  or starts with `process`, `CI` or `untested`; a test checks this (DEV-033, `tests/spec_refs.rs`). A test a row names carries that row's ID in its name or in a `// spec: ID` comment above it (DEV-039; the exceptions of today are in `tests/spec_refs_allowlist.txt` and only shrink).
- A `SHOULD BE` row with planned work names its GitHub issue (`#N`; `raba/shale#N` in older rows is the same number); open work is
  tracked in the issues, not in these files (FORGE-004, FORGE-007).
