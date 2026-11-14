# ADR-0005: Where state lives — global settings vs. per-vault data

- **Status**: Proposed
- **Date**: 2026-11-14
- **Specs**: CFG-001…CFG-005, VAULT-004

## Context

`~/.config/shale/config.json` holds two kinds of state:

- **Machine and user settings**: font size, sort order, the calendar list, the recent vaults.
- **Per-vault state**: `open_files`, `people_dir`, `last_import_folder` and `recent_tags`
  (`config.rs:51, 64-69`).

`<vault>/.shale/` exists, but it holds only `tag-words.json` (`tagwords.rs:150`).

Because of this:

- Switching vaults keeps the other vault's people folder, import folder and tag chips.
- Two machines that sync one vault do not share its people folder setting.
- The config has no format version (`#[serde(default)]` hides every change). A parse error
  resets everything (see [ADR-0003](0003-data-safety.md)).

## Decision (proposed)

1. **Global config** keeps only machine and user settings: appearance, input behaviour,
   calendars and their accounts (passwords stay in the keyring), recent vaults, and the
   troubleshooting switches.
2. **`<vault>/.shale/settings.json`** holds vault settings that should travel with the vault:
   the people folder, the default folder for new notes, and the media folder (`z_media`).
3. **`<vault>/.shale/state.json`** holds session state that may be lost: open tabs, the
   last import folder, recent tags. It can be git-ignored.
4. Both files carry `"version": N`. Loading runs explicit migrations in order, and each
   migration has a test. The first migration moves the per-vault fields out of the global
   config into the vault that is open.
5. All of them are written atomically (ADR-0003).

## Consequences

- Vaults become self-contained; a synced vault behaves the same on every machine.
- Writing into `.shale/` changes the vault folder. Users who keep vaults in git may want
  `.shale/state.json` in `.gitignore`; FEATURES must say so.
- **Open**: should the media folder name (`z_media`) become a setting at all? (OPEN-004)

## Decisions

- settings for the vault should stay in the vault, so that we can achieve same behaviour of the vault on every machine.
- I say that it needs to be part of a git-sync feature, to add the <vault>/.shale/* files with a sync. we will add a git sync later, I want that anyway. please keep this behaviour for later use - shale has integrated git sync. you need git setup on your OS to use, you can add a git repo url via the shale GUI or simply git init in the folder and our git sync will auto sync changes. the used commit messages will be configurable in the settings to get a defined prefix and/or suffix. git sync intervall is configurable.