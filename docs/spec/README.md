# Shale specs

Living specs: what Shale guarantees today (**IS NOW**) and what it should guarantee (**SHOULD BE**).
How they work: [ADR-0001](../decisions/0001-spec-driven-development.md).

| File | Areas | IDs |
|---|---|---|
| [vault-and-files.md](vault-and-files.md) | Vault, create/rename/move/trash, writing data, settings, git sync | VAULT, DATA, CFG, SYNC |
| [links-and-editor.md](links-and-editor.md) | Link syntax and resolution, editing, navigation and keys | LINK, EDIT, NAV |
| [features.md](features.md) | Tasks, drawings, calendar, network security, import, people, tags, PDF | TASK, DRAW, CAL, SEC, IMP, PPL, TAG, PDF |
| [social.md](social.md) | Mastodon and Bluesky feeds, saving posts as notes | SOC |
| [engineering.md](engineering.md) | Forge and work tracking, development rules, build and release, background jobs, performance | FORGE, DEV, BUILD, JOB, PERF |
| [mail.md](mail.md) | Local mail store, IMAP account and sync, flags and moves, sending, signing and encrypting | MAIL |
| [open-points.md](open-points.md) | Undecided questions (open work: Forgejo issues) | OPEN |

## Rules for editing

- One rule per row, one testable sentence, present tense. Describe behaviour, not code.
- IDs are stable: never renumber or reuse; mark a dropped rule *Retired*.
- A known defect is an `IS NOW ⚠` row (how it behaves) next to the `SHOULD BE` row (the fix).
- Change the spec **before** the code; flip `SHOULD BE` → `IS NOW` in the same change that
  makes it true, with the test name in the Test column.
- Tests that check a rule carry its ID in the name or a `// spec: ID` comment.
- A `SHOULD BE` row with planned work names its Forgejo issue (`raba/shale#N`); open work is
  tracked in the issues, not in these files (FORGE-004, FORGE-007).
