# Spec: Tasks, drawings, calendar, import, people, tags, PDF (TASK, DRAW, CAL, SEC, IMP, PPL, TAG, PDF)

**IS NOW** = true of the code today (⚠ = known defect, described as it behaves) ·
**SHOULD BE** = agreed target. Format: [ADR-0001](../decisions/0001-spec-driven-development.md).
Tests are named `file.rs::test_fn` (all under `src/`). These areas are large; the rules below
are the guarantees, not every detail. Heuristics (Thunderbird lookup, tag learning thresholds)
are deliberately not specced beyond their guarantees; their tests check the details.

## Tasks (TASK)

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| TASK-001 | A task is a list item `- [ ]`, `* [ ]`, `+ [ ]` or `N. [ ]` (done: `[x]`), outside code blocks. | IS NOW | `markdown::parse_task_line`, `find_tasks` | `markdown.rs::tasks_parse_and_toggle` |
| TASK-002 | A due date is `📅 YYYY-MM-DD` or `due:YYYY-MM-DD`. | IS NOW | `markdown.rs` | `markdown.rs::tasks_parse_and_toggle`, `panels.rs::due_is_stripped`, `panels.rs::today_looks_like_a_date` |
| TASK-003 | The task list shows every task of the vault grouped by note; ticking writes `[x]` into the note (or its open buffer). | IS NOW | `panels.rs`, `Action::ToggleTask` | `vault.rs::tasks_across_vault`, `app.rs::task_toggle_and_drawing_embed` |
| TASK-004 | The task list and the status-bar count are computed once per vault change, not every frame. | SHOULD BE | today `Vault::all_tasks()` each frame (`layout.rs:620`, `panels.rs`) | — |

## Drawings (DRAW)

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| DRAW-001 | Drawings read and write the Excalidraw scene format; unknown elements are kept and drawn as a dashed box; a file from excalidraw.com survives a round trip. | IS NOW | `drawing.rs` | `drawing.rs::sample_parses_and_round_trips`, `drawing.rs::new_elements_have_excalidraw_fields` |
| DRAW-002 | The hand-drawn look is deterministic per element (`seed`), so it does not flicker; hatch fills stay inside their shape. | IS NOW | `drawing.rs` | `drawing.rs::hatching_stays_inside` (determinism untested) |
| DRAW-003 | Labels are Excalidraw text elements with `containerId`; connections are `startBinding`/`endBinding` with `fixedPoint` and follow their shapes. | IS NOW | `drawing_connect.rs` | `drawing_connect.rs::labels_are_centred_and_follow`, `drawing_connect.rs::connections_follow_moved_shapes`, `drawing_connect.rs::snapping_finds_edge_middles`, `drawing_connect.rs::line_endings_and_patterns_are_saved` |
| DRAW-004 | Auto arrange / Spread out / Pull together never make the drawing worse (more crossings or overlaps); if they would, nothing moves and a message says why. Undo restores the previous state. | IS NOW | `drawing_layout.rs` | `drawing_layout.rs::auto_layout_untangles_and_stops`, `drawing_layout.rs::bigger_graph_gets_no_worse`, `drawing_layout.rs::spreading_keeps_shapes_apart`, `drawing_layout.rs::crossings_are_counted`, `drawing_layout.rs::anchors_are_on_the_shape`, `drawing_connect.rs::arrange_and_spread_move_shapes_and_keep_connections` |
| DRAW-005 | Every style-panel and toolbar control works (a click reaches it, not the canvas). | IS NOW | `drawing.rs` `ui_rects` | all 23 tests in `drawing_ui_tests.rs` |
| DRAW-006 | Undo/redo keeps 200 steps. | IS NOW | `drawing.rs` | `drawing.rs::undo_redo` (partly: the 200-step limit is untested) |
| DRAW-007 | Drawings of the Obsidian Excalidraw plugin (`*.excalidraw.md`) open in the drawing editor, whether their data is compressed (`compressed-json`, LZ-String) or plain `json`. Obsidian's link forms `[[Name.excalidraw]]` and `[[Name.excalidraw.md]]` resolve to them, also as embeds. | IS NOW | `excalidraw_md.rs`, `vault::kind_of`, `vault::link_name_of` | `excalidraw_md.rs::lz_string_matches_the_plugin`, `excalidraw_md.rs::plugin_files_are_drawings`, `notes_vault_tests.rs::notes_every_drawing_opens_and_saves_without_loss` |
| DRAW-008 | Saving a plugin drawing keeps its layout: front matter, notes, `## Element Links`, `## Embedded Files` and the compression stay as they were. `## Text Elements` is written again from the scene (the plugin takes texts from there), and texts and linked elements get the 8-character ids the plugin needs (stable between saves). When reading, the texts in `## Text Elements` win over the scene, as in the plugin. | IS NOW | `excalidraw_md::write`, `excalidraw_md::read` | `excalidraw_md.rs::saving_keeps_the_plugin_layout`, `excalidraw_md.rs::section_text_wins_and_ids_get_eight_characters`, `excalidraw_md.rs::broken_plugin_files_are_errors`, `drawing_ui_tests.rs::opening_a_drawing_changes_nothing_until_it_is_edited` |
| DRAW-009 | Opening or looking at a drawing never changes its file; only an edit does. | IS NOW | `DrawingDoc::ui` (`settled`) | `drawing_ui_tests.rs::opening_a_drawing_changes_nothing_until_it_is_edited` |
| DRAW-010 | Links written in a plugin drawing's texts (`[[Note]]` in a label) are not counted as links: no backlinks from drawings. Pictures embedded in plugin drawings (`## Embedded Files`) are kept in the file but drawn as a dashed box. | IS NOW ⚠ | `Vault::rescan` reads notes only | untested |

## Calendar (CAL)

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| CAL-001 | Calendars are local `.ics` files in `~/.config/shale/calendars/` or CalDAV calendars. | IS NOW | `cal_store.rs` | `cal_store.rs::local_calendar_sample_add_delete`, `cal_store.rs::config_round_trip` |
| CAL-002 | CalDAV loads events from 4 months back to 13 months ahead; each calendar syncs on its own timer (default 15 min, 0 = only at start and by hand); unchanged ctag/sync-token skips the download. | IS NOW | `cal_store.rs`, `cal_dav.rs` | `cal_store.rs::each_calendar_has_its_own_sync_timer`, `cal_store.rs::unchanged_calendars_are_not_downloaded_again_and_read_only_ones_refuse_writes`, `cal_store.rs::caldav_source_loads_in_the_background`, `cal_store.rs::a_change_during_a_sync_loads_again`, `cal_dav.rs::discovers_and_fetches_from_a_server`, `cal_dav.rs::calendar_state_reads_tag_and_permission`, `cal_dav.rs::task_lists_and_read_only_calendars` |
| CAL-003 | Moving an event to another calendar writes it there **first** and removes the original only if that worked; if removal fails the event stays in both and the user is told. | IS NOW | `cal_store.rs` | `cal_store.rs::moving_an_event_writes_first_and_removes_after` |
| CAL-004 | Editing a CalDAV event only succeeds if nobody changed it on the server meanwhile (ETag / `If-Match`); a refusal is shown in the dialog, which stays open. | IS NOW | `cal_dav.rs` | `cal_dav.rs::writes_and_removes_events`, `cal_dav.rs::headers_come_from_the_last_answer`, `cal_dav.rs::fetched_events_know_their_address`, `cal_store.rs::a_write_keeps_the_new_version_tag`, `cal_dialog.rs::event_dialog_shows_write_errors_and_saves_elsewhere` |
| CAL-005 | Editing keeps all fields the dialog does not show (UID, repeats, attendees, organizer…) and changes the event in place. | IS NOW | `cal_dialog.rs`, `cal_ics.rs` | `cal_dialog.rs::event_dialog_fields` (UID, in place), `cal_ics.rs::writes_and_reads_back` |
| CAL-006 | Repeats: daily/weekly (BYDAY)/monthly/yearly with INTERVAL, COUNT, UNTIL, EXDATE and RECURRENCE-ID overrides. Edits and deletes apply to the whole series. | IS NOW | `cal_ics.rs` | `cal_ics.rs::parses_events`, `cal_ics.rs::weekly_repeats_with_exdate_and_count`, `cal_ics.rs::other_frequencies_and_ranges`, `cal_ics.rs::durations` |
| CAL-007 | Changing or deleting a single occurrence of a repeating event. | SHOULD BE (backlog) | — | — |
| CAL-008 | A double-click on a day or time (or ＋) creates an event at the next full or half hour, 30 minutes long; a single click creates nothing. | IS NOW | `cal_ui.rs`, `cal_dialog.rs` | `cal_ui.rs::next_half_hour_rounds_up`, `cal_ui.rs::double_click_creates_events_in_every_view`, `cal_dialog.rs::event_dialog_fields` |
| CAL-010 | Settings → Calendars adds local calendars, or CalDAV calendars found by discovery or entered as one address; an unreachable server is an error shown in the dialog. | IS NOW | `cal_dialog.rs`, `cal_dav.rs` | `cal_dialog.rs::calendar_dialog_controls`, `cal_dialog.rs::find_calendars_button_uses_the_server`, `cal_dav.rs::unreachable_server_is_an_error` |
| CAL-011 | Deleting an event from its popup removes it (a repeating event with all its repeats) from its calendar, local or CalDAV; a refusal is shown. | IS NOW | `cal_ui.rs`, `cal_store.rs` | `cal_ui.rs::week_view_blocks_and_delete_from_popup`, `cal_dav.rs::writes_and_removes_events` |
| CAL-009 | Times with a named zone use the system's zoneinfo files (summer/winter time); dates are computed without a date library; zone names that are paths are refused. | IS NOW | `cal_time.rs` | `cal_time.rs::civil_round_trip`, `cal_time.rs::named_zones_from_zoneinfo`, `cal_time.rs::local_round_trip` |

## Security of external connections (SEC)

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| SEC-001 | CalDAV only over `https://` (`http://` only for localhost); no credentials inside addresses; no line breaks in headers; redirects only to allowed addresses; answers limited to 50 MB; XML read without DTDs. | IS NOW | `cal_dav::request_with` | `cal_dav.rs::only_https_or_this_computer`, `cal_dav.rs::headers_with_line_breaks_are_refused`, `cal_dav.rs::xml_and_addresses` (50 MB limit untested) |
| SEC-002 | The password reaches `curl` on standard input, never on the command line; `curl -q` ignores `~/.curlrc`. | IS NOW | `cal_dav.rs` | `cal_dav.rs::discovers_and_fetches_from_a_server` (partly: credentials arrive) |
| SEC-003 | Passwords are stored in the system keyring (`secret-tool`); without a keyring they fall back to the owner-only settings file. | IS NOW | `secrets.rs` | `secrets.rs::passwords_go_to_the_keyring_and_come_back` |
| SEC-004 | Event addresses on a different server than their calendar are never requested (the password never goes to another host). | IS NOW | `cal_dav::same_server` | `cal_dav.rs::only_https_or_this_computer` (partly: `same_server`) |
| SEC-005 | External programs (`curl`, `secret-tool`, `xdg-open`) are started with an argument list, never through a shell. | IS NOW | `std::process::Command` uses | untested (review rule) |
| SEC-006 | Only `http`, `https` and `mailto` URLs (scheme checked case-insensitively, control characters refused, `www.` gets `https://`) and files inside the vault, copies Shale made in its own private (0700) temp folder, or the PDF just exported are handed to `xdg-open`; programs, scripts, `.desktop` files and executable files (known document types excepted) never. `opener.rs` is the only code that starts `xdg-open`; a test enforces it. | IS NOW | `opener.rs`; used by `app.rs` (links, non-note files), `cal_ui.rs` (event links), `panels.rs` (exported PDF) | `opener.rs`: `only_web_and_mail_addresses_are_opened`, `vault_files_are_checked`, `programs_and_desktop_files_are_never_opened`, `executable_files_are_refused_but_documents_are_not`, `an_exported_pdf_is_a_pdf`, `temp_copies_live_in_a_private_folder_and_are_checked`, `no_other_code_starts_an_opener` |
| SEC-007 | Passwords never appear in debug output of calendar sources or accounts. | IS NOW | `cal_dav.rs`, `cal_store.rs` (`Debug` impls) | `cal_dav.rs::passwords_stay_out_of_debug_output`, `cal_store.rs::passwords_stay_out_of_debug_output` |
| SEC-008 | File names made from event UIDs never contain `/`, never start with `..`, and different UIDs never share a name. | IS NOW | `cal_dav::event_file` | `cal_dav.rs::event_files_never_collide` |

## Import (IMP)

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| IMP-001 | Dropping e-mails, events, files or text on the window (or the Import button) opens the Import dialog; nothing is written before the user presses Import. | IS NOW | `app/drops.rs`, `import_dialog.rs` | `import_dialog.rs::every_control_of_the_import_dialog` |
| IMP-002 | The drop target decides the folder: a folder → into it; a note → its folder plus "add links" ticked; elsewhere → the folder used last. | IS NOW | `app/drops.rs` | untested (real-window checks only) |
| IMP-003 | One note per e-mail / event, named `YYYY-MM-DD Subject` (link-safe, VAULT-012), with properties (type, subject, from, to, cc, date, message-id / event uid, tags). | IS NOW | `import::email_note`, `event_note` | `import.rs::an_email_becomes_a_tagged_note_with_its_attachments`, `import.rs::events_files_and_text` |
| IMP-004 | Attachments go to `z_media/<YYYY>/<MM>/<YYMMDD>_<name>` with the mail's date; small inline pictures are unticked by default. | IS NOW | `import.rs`, `media::save_bytes` | `import.rs::an_email_becomes_a_tagged_note_with_its_attachments` |
| IMP-005 | A message (Message-ID) or event (UID + date) imported before is marked and unticked. | IS NOW | `import::mark_duplicates` | `import.rs::an_email_becomes_a_tagged_note_with_its_attachments` (second import) |
| IMP-006 | A folder typed in the dialog can never point outside the vault (`..`, absolute paths refused). | IS NOW | `import::clean_folder` | `import.rs::tags_and_folders` |
| IMP-007 | Shale asks other programs for a **copy** only, so a drop never moves a message out of the mail program. | IS NOW | `wl_drop.rs` | untested (protocol) |
| IMP-008 | When Thunderbird hands over only a link, the message is looked up in Thunderbird's local folders (index `.msf` → Message-ID and position); if not found, the dialog explains why per place. | IS NOW | `tb_store.rs` | `tb_store.rs::reads_thunderbird_folder_indexes`, `tb_store.rs::reads_thunderbird_links`, `tb_store.rs::finds_messages_in_a_profile`, `import.rs::a_thunderbird_drag_with_an_empty_file_list` |
| IMP-009 | A drop is read from every useful kind offered (file list, whole message, calendar data, links, text) at the same time with a time limit; the best non-empty one is used; if none works, the dialog lists what each kind held. | IS NOW | `wl_drop.rs`, `import::items_from_parts` | `wl_drop.rs::picks_the_most_useful_kind`, `wl_drop.rs::reads_several_transfers_at_once_with_a_time_limit`, `import.rs::what_other_programs_drop` |
| IMP-010 | E-mails are read with encoded headers (UTF-8, Latin-1/-15, Windows-1252), multipart bodies, base64 / quoted-printable, RFC 2231 attachment names and attached messages; HTML-only mails become readable text. | IS NOW | `mail.rs` | `mail.rs::decoders`, `mail.rs::params_and_dates`, `mail.rs::html_mails_become_readable_text`, `mail.rs::a_thunderbird_message_with_attachments`, `mail.rs::html_only_and_single_part_messages` |

## People and tags (PPL, TAG)

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| PPL-001 | Contact notes live in the people folder (setting, default `Adressbuch`). | IS NOW | `config.rs`, `people.rs` | `editor.rs::double_brackets_p_lists_people` (partly) |
| PPL-002 | On import, senders, recipients, organizer and attendees are matched to contact notes by any e-mail address in the note, else by name (`"Müller, Hans"` = `Hans Müller`); robots (`noreply` …) are left out. | IS NOW | `people.rs` | `people.rs::addresses_and_names`, `people.rs::people_of_a_mail_and_their_notes` |
| PPL-003 | Known contacts and new senders are ticked; a ticked person without a note gets one; the imported note gets a "People" line of normal `[[Name]]` links. | IS NOW | `people::ensure_note`, `import.rs` | `people.rs::people_of_a_mail_and_their_notes` |
| TAG-001 | Tags are `#tag` in text (nested `a/b` allowed, not inside links or code) and `tags:` in front matter (`[a, b]`, `a b`, or a `- a` list). | IS NOW | `markdown.rs` | `markdown.rs::tags`, `markdown.rs::file_names_with_a_hash_sign`, `import.rs::an_email_becomes_a_tagged_note_with_its_attachments` (front matter) |
| TAG-002 | Vault tags whose words appear in an import are added automatically; learned tag suggestions are plain counting (no network, no AI) and deterministic: the same notes always give the same suggestions. | IS NOW | `tagwords.rs` | `tagwords.rs::words_and_direct_tags`, `tagwords.rs::learns_typical_words_and_suggests_tags` |
| TAG-003 | Tag words are re-learned in the background at most every 2 minutes after the vault changed and saved to `.shale/tag-words.json`. | IS NOW | `app.rs`, `tagwords.rs` | `tagwords.rs::learns_typical_words_and_suggests_tags` (partly: save/load; the interval is untested) |

## PDF (PDF)

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| PDF-001 | Notes and drawings export to PDF with no external tools; note text stays selectable (embedded fonts), web links stay clickable, drawings are vector; a bad save path gives an error. | IS NOW | `pdfexport.rs`, `pdfwrite.rs` | `pdfexport.rs::note_export_has_title_text_links_and_pages`, `pdfexport.rs::note_export_options_change_the_pages`, `pdfexport.rs::drawing_export_fit_and_paper`, `pdfwrite.rs::writes_a_pdf_that_renders`, `pdfwrite.rs::win_ansi_mapping`, `export_dialog.rs::note_dialog_controls`, `export_dialog.rs::drawing_dialog_controls`, `app.rs::export_reports_a_bad_path` |
| PDF-002 | Embedded PDFs render in the editor and reading view (up to 50 pages) on a background thread, with the bundled pure-Rust renderer. | IS NOW | `pdfview.rs` | `pdfview.rs::renders_pages`, `pdfview.rs::background_rendering_becomes_ready` |
| PDF-003 | Default export path is `<vault>/exports/<name>.pdf`, so the export shows up in the vault. | IS NOW | `ShaleApp` export (`app.rs:859`) | `app.rs::export_menu_entry_opens_the_dialog_and_writes_a_pdf` |
