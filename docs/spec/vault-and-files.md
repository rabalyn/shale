# Spec: Vault, files, data on disk, settings (VAULT, DATA, CFG)

**IS NOW** = true of the code today (⚠ = known defect, described as it behaves) ·
**SHOULD BE** = agreed target. Format: [ADR-0001](../decisions/0001-spec-driven-development.md).
Tests are named `file.rs::test_fn` (all under `src/`); "partly" means the test covers only part
of the rule.

## Vault

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| VAULT-001 | A vault is any folder. Opening a path that does not exist creates it. | IS NOW | `Vault::open` | untested (create-if-missing) |
| VAULT-002 | Drawings are `*.excalidraw` and `*.excalidraw.md` (Obsidian Excalidraw plugin), notes are all other `*.md`; every other file is an attachment. | IS NOW | `vault::kind_of`, `excalidraw_md::is_drawing_path` | `vault.rs::resolve_and_backlinks` (partly), `excalidraw_md.rs::plugin_files_are_drawings`, `notes_vault_tests.rs::notes_every_file_is_listed_and_every_note_is_readable` |
| VAULT-003 | Files and folders whose name starts with `.` are ignored (`.git`, `.obsidian`, `.trash`, `.shale`). | IS NOW | `vault::walk` | untested |
| VAULT-004 | Shale's per-vault data lives in `<vault>/.shale/` (today only `tag-words.json`). | IS NOW | `tagwords.rs` | `tagwords.rs::learns_typical_words_and_suggests_tags` (save/load) |
| VAULT-005 | The vault is re-scanned every 3 s; files with an unchanged modification time are not re-read. | IS NOW | `ShaleApp` `last_scan`, `Vault::rescan` | `vault.rs::outside_change_is_detected` (partly: no interval) |
| VAULT-006 | The last vault is reopened at start; a vault can also be opened from the start screen, `--vault`, Settings or the recent list (no duplicates). | IS NOW | `main.rs`, `config.rs` | `config.rs::recent_list_is_unique` (partly) |
| VAULT-007 | Symbolic links (to files or folders) inside the vault are skipped silently. | IS NOW ⚠ | `vault::walk` (`file_type` does not follow links) | untested |
| VAULT-008 | Symbolic links are either followed (with a loop guard and staying inside the vault) or shown as such; never silently missing. | SHOULD BE | — | OPEN-005 |

## Creating, renaming, moving, deleting

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| VAULT-010 | New note names have `\ : * ? " < > \|` and NUL replaced by `-` and leading/trailing dots and spaces removed; an empty name becomes `Untitled`. | IS NOW | `markdown::sanitize_note_name`, `Vault::new_note_path` | untested (only "no panic" in `utf8_tests.rs::text_functions_never_cut_a_character`) |
| VAULT-011 | A taken name gets a number (`Name 1.md`, `Name 1.excalidraw`, `Name 1.excalidraw.md` …); "new note" never overwrites a file. | IS NOW | `Vault::new_note_path` | `vault.rs::create_from_missing_link_and_rename` |
| VAULT-012 | Names written by imports and the file picker are link-safe: `#` removed, `\|` and `^` → `-`, `[ ]` → `( )`. | IS NOW | `markdown::link_safe_name` | `media.rs::saved_names_work_in_links`, `markdown.rs::file_names_with_a_hash_sign` |
| VAULT-013 | Rename of a note, drawing, file or folder (setting "update links", on by default) rewrites the links that pointed to it with the routine of VAULT-016. | IS NOW | `Vault::rename`, `Vault::relocate` | `vault.rs::create_from_missing_link_and_rename`, `vault.rs::rename_rewrites_links_by_what_they_resolve_to`, `app.rs::rename_keeps_open_buffers_in_step` |
| VAULT-014 | Rename refuses a name that is already taken. | IS NOW | `Vault::rename` | untested |
| VAULT-015 | Moving a file or folder into another folder (drag and drop in the file list) refuses taken names and a folder into itself; links written **as a path** to anything that moved are rewritten; links by name are left alone. Open tabs, history and buffers follow. | IS NOW | `Vault::move_into`, `ShaleApp::move_into`, `after_move` | `panels.rs::moving_notes_and_folders_into_folders` |
| VAULT-016 | Rename and move share one rewrite routine. It selects links by **what they resolve to** before the change (not by their text) and rewrites every link whose resolution would otherwise change, so each link keeps pointing at the same file: links to what moved, relative links inside a moved note, and links by name that a new name would steal. It writes the shortest form that resolves to the right file: a link written by name stays a name (a path only if the name is ambiguous there), a path keeps its folder part and its `./`/`../` style; alias, heading, `<…>` and `.md` are kept; Unicode case is ignored. Renaming a folder rewrites links to its contents. Rewritten notes are replaced atomically. | IS NOW | `Vault::relocate`, `Vault::link_text_for`, `markdown::replace_target` | `vault.rs::rename_rewrites_links_by_what_they_resolve_to`, `vault.rs::rename_keeps_folder_alias_heading_brackets_and_md`, `vault.rs::rename_keeps_other_links_pointing_where_they_did`, `vault.rs::folder_rename_rewrites_links_to_its_contents`, `vault.rs::moving_a_note_keeps_its_relative_links_working`, `markdown.rs::replace_target_keeps_heading_and_brackets`, `app.rs::folder_rename_updates_links`, `panels.rs::moving_notes_and_folders_into_folders` |
| VAULT-017 | "Delete" moves to `<vault>/.trash/`; a name already there gets a number prefix (`1 Name.md`). The original folder is not recorded; there is no restore and the trash is never emptied by Shale. | IS NOW | `Vault::trash` | `vault.rs::create_from_missing_link_and_rename` (partly: no name clash) |
| VAULT-018 | Every path a user types (rename, new note, new folder, picker, import) stays inside the vault: no absolute paths, no `..` escaping the root. All of them are checked by one path type (`VaultPath`) that can only be built by its checking constructors; a rename name is one name (no `/`). `Vault::abs` still takes plain paths inside the code (the inward step of ADR-0004 is left for later). | IS NOW | `vault_path::VaultPath`, `ShaleApp::rename`, `ShaleApp::create_folder`, `Vault::new_note_path`, `import::clean_folder`, `media::media_rel_path` | `vault_path.rs::user_paths_stay_inside_the_vault`, `vault_path.rs::picker_names_stay_inside_the_vault`, `app.rs::rename_refuses_names_that_leave_the_folder`, `app.rs::new_folder_stays_inside_the_vault`, `import.rs::tags_and_folders` |
| VAULT-019 | Trash keeps the original relative path under a dated folder so items can be restored. | SHOULD BE | — | [ADR-0003](../decisions/0003-data-safety.md) |

## Writing data (DATA)

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| DATA-001 | Notes save 0.8 s after the last edit, on tab close and on exit; Ctrl+S saves at once. | IS NOW | `ShaleApp` autosave | untested |
| DATA-002 | A note changed by another program is reloaded within ~3 s if it has no unsaved edits. Open **drawings** are not reloaded. | IS NOW ⚠ | `ShaleApp::rescan` | `vault.rs::outside_change_is_detected` (index only; reload of the open buffer untested) |
| DATA-003 | If a note or drawing changed on disk while it has unsaved edits, both versions survive (conflict copy next to it) and the user is told. ⚠ Today the next autosave silently overwrites the other program's change. | SHOULD BE | `app.rs:592-602`, `Vault::write_note` | [ADR-0003](../decisions/0003-data-safety.md) |
| DATA-004 | All writes of user files (notes, drawings, link rewrites, imports, `.shale/`) are atomic: temp file in the same folder, fsync, rename. ⚠ Today plain `fs::write`; only the config is atomic. | SHOULD BE | `vault.rs:418, 435, 449, 516, 556`; pattern in `config::write_private` | ADR-0003 |
| DATA-005 | Files keep open formats: notes are plain markdown; drawings are Excalidraw scene JSON (`type: "excalidraw", version: 2`) with unknown fields written back unchanged; calendars are iCalendar. | IS NOW | `drawing.rs` (`#[serde(flatten)]`), `cal_ics.rs` | `drawing.rs::sample_parses_and_round_trips`, `drawing.rs::new_elements_have_excalidraw_fields`, `cal_ics.rs::writes_and_reads_back` |
| DATA-006 | Text handling never splits a UTF-8 character. | IS NOW | across modules | `utf8_tests.rs::text_functions_never_cut_a_character`, `utf8_tests.rs::reading_view_and_pdf_export_take_any_text` |
| DATA-007 | A drawing that can't be read or parsed (also a broken `.excalidraw.md`) opens read-only with the error; it is never written. | IS NOW | `DrawingDoc::new`, `DrawingDoc::unreadable`, `DrawingDoc::file_text` | `drawing.rs::empty_file_is_empty_scene`, `excalidraw_md.rs::broken_plugin_files_are_errors` |
| DATA-008 | A note that fails to read (invalid UTF-8, no permission) opens read-only with the reason and is never overwritten. ⚠ Today it opens as an empty note and the first keystroke replaces the original bytes (`vault.rs:131` `unwrap_or_default`). | SHOULD BE | `Vault::rescan`, `app.rs:423` | ADR-0003 |
| DATA-009 | Every failed write or read of a user file is shown to the user (message, and a mark on the tab while unsaved). ⚠ Today several are dropped (`let _ = write_note`, `app.rs:538, 543, 572`). | SHOULD BE | `app.rs` | ADR-0003 |

## Settings (CFG)

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| CFG-001 | Settings are in `~/.config/shale/config.json`; missing fields take defaults, so older files load. | IS NOW | `config.rs` (`#[serde(default)]`) | `config.rs::old_config_files_still_load`, `cal_store.rs::config_round_trip` |
| CFG-002 | The settings file is created owner-only (0600) and replaced atomically. | IS NOW | `config::write_private` | `config.rs::private_files_are_owner_only_and_replaced_whole` |
| CFG-003 | A settings file that fails to parse is replaced by defaults; the next save overwrites it (calendars lost). | IS NOW ⚠ | `config.rs:178-181` | untested |
| CFG-004 | A broken settings file is kept as `config.json.broken-<date>` and the user is told; settings carry `"version"` and migrate explicitly with tests. | SHOULD BE | — | [ADR-0005](../decisions/0005-where-state-lives.md) |
| CFG-005 | Per-vault state (people folder, last import folder, recent tags, open tabs) lives in `<vault>/.shale/`, not in the global config. Today it is global. | SHOULD BE | `config.rs:51, 64-69` | ADR-0005 |
