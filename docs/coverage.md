# Test coverage and gaps

Spec: DEV-003 (coverage is measured and the gaps are tracked), DEV-010 (the ratchet). Measured on
2026-11-15 at `df259ac` (Wave 3) with `cargo llvm-cov` 0.9.1 on Rust 1.100.0, the normal `cargo test`
(751 tests; the 13 ignored ones, the real-server suites of DEV-005, are not run) and no `notes/`
test vault.

| | Covered | Total | Percent |
|---|---:|---:|---:|
| **Lines** | 42,452 | 47,832 | **88.75 %** |
| Functions | 6,243 | 7,056 | 88.48 % |
| Regions | 76,724 | 86,090 | 89.12 % |

## How it is measured

- CI: the `coverage` job of `.forgejo/workflows/ci.yml` (separate from the build, on pushes and
  pull requests) installs `cargo-llvm-cov`, runs the tests, prints the table per file into the log,
  writes `lcov.info` (no artifact: upload-artifact is a JavaScript action, which the job containers
  do not run) and checks the ratchet.
- Locally (needs `rustup component add llvm-tools-preview` and `cargo install cargo-llvm-cov --locked`;
  about 10 minutes the first time):

  ```sh
  cargo llvm-cov --locked --lcov --output-path lcov.info   # test run + lcov file
  cargo llvm-cov report                                    # the per-file table
  cargo llvm-cov report --html                             # target/llvm-cov/html/index.html, line by line
  sh .forgejo/scripts/check-coverage.sh lcov.info .forgejo/coverage-baseline
  ```

- The numbers are the unit and UI tests only. The ignored suites against real servers
  (`caldav_env`, `mail_env`) are not in them; they reach more of the network code (see "Biggest
  gaps"). `cargo llvm-cov --locked -- --ignored caldav_env` with the stacks up measures them.

## The ratchet (DEV-010)

`.forgejo/coverage-baseline` holds one number, the total line coverage in percent. The job fails
when the total is more than 0.5 points below it (the half point absorbs differences between machines
and tests that depend on timing). The job log says when the total is a point or more above the
baseline. To raise it, in the same change that added the tests:

1. run `cargo llvm-cov --locked --lcov --output-path lcov.info` and
   `sh .forgejo/scripts/check-coverage.sh lcov.info .forgejo/coverage-baseline`,
2. write the printed total (two decimals) into `.forgejo/coverage-baseline`,
3. update the table above and the date of this page when the change is large.

Lowering the number needs a reason in the commit message (code that is deleted, or code that cannot
be tested without tooling and an issue for it).

## Coverage per file

Lowest first. `sidebar.rs` and `theme.rs` are at 100 %; `keys.rs` and `keys_ui.rs` at 99 %.

| File | Lines | Missed | Line coverage |
|---|---:|---:|---:|
| `src/main.rs` | 56 | 45 | 19.64 % |
| `src/instance.rs` | 28 | 16 | 42.86 % |
| `src/wl_drop.rs` | 292 | 162 | 44.52 % |
| `src/app/drops.rs` | 403 | 216 | 46.40 % |
| `src/reader.rs` | 398 | 149 | 62.56 % |
| `src/graph.rs` | 273 | 87 | 68.13 % |
| `src/panels.rs` | 1507 | 469 | 68.88 % |
| `src/layout.rs` | 710 | 218 | 69.30 % |
| `src/drawing.rs` | 1655 | 501 | 69.73 % |
| `src/app.rs` | 1883 | 540 | 71.32 % |
| `src/social/accounts_ui.rs` | 274 | 67 | 75.55 % |
| `src/social/images.rs` | 98 | 19 | 80.61 % |
| `src/app/mail.rs` | 160 | 30 | 81.25 % |
| `src/link_assist.rs` | 472 | 81 | 82.84 % |
| `src/mail_ui.rs` | 1836 | 269 | 85.35 % |
| `src/tb_store.rs` | 524 | 73 | 86.07 % |
| `src/opener.rs` | 309 | 42 | 86.41 % |
| `src/mail_compose.rs` | 782 | 106 | 86.45 % |
| `src/pdfexport.rs` | 655 | 84 | 87.18 % |
| `src/social/markup.rs` | 252 | 32 | 87.30 % |
| `src/drawing_connect.rs` | 877 | 109 | 87.57 % |
| `src/mail.rs` | 686 | 83 | 87.90 % |
| `src/social/demo.rs` | 142 | 17 | 88.03 % |
| `src/social/feed_ui.rs` | 780 | 88 | 88.72 % |
| `src/recovery.rs` | 81 | 9 | 88.89 % |
| `src/social/note.rs` | 364 | 40 | 89.01 % |
| `src/app/sync.rs` | 495 | 54 | 89.09 % |
| `src/import.rs` | 747 | 81 | 89.16 % |
| `src/imap_client.rs` | 1027 | 110 | 89.29 % |
| `src/config.rs` | 315 | 33 | 89.52 % |
| `src/mail_demo.rs` | 68 | 7 | 89.71 % |
| `src/gpg.rs` | 786 | 76 | 90.33 % |
| `src/imap_sync.rs` | 787 | 76 | 90.34 % |
| `src/vault_state.rs` | 301 | 28 | 90.70 % |
| `src/mail_view.rs` | 1095 | 101 | 90.78 % |
| `src/mail_send.rs` | 499 | 43 | 91.38 % |
| `src/social/state.rs` | 649 | 53 | 91.83 % |
| `src/social/mod.rs` | 329 | 26 | 92.10 % |
| `src/social/bluesky.rs` | 569 | 44 | 92.27 % |
| `src/app/social.rs` | 118 | 9 | 92.37 % |
| `src/smtp_client.rs` | 293 | 22 | 92.49 % |
| `src/mail_ops.rs` | 1204 | 90 | 92.52 % |
| `src/gitsync.rs` | 411 | 30 | 92.70 % |
| `src/social/mastodon.rs` | 362 | 26 | 92.82 % |
| `src/pdfview.rs` | 101 | 7 | 93.07 % |
| `src/mail_crypto.rs` | 1112 | 76 | 93.17 % |
| `src/media.rs` | 235 | 16 | 93.19 % |
| `src/fsutil.rs` | 522 | 35 | 93.30 % |
| `src/cal_dav.rs` | 875 | 58 | 93.37 % |
| `src/oauth.rs` | 856 | 56 | 93.46 % |
| `src/cal_ui.rs` | 969 | 58 | 94.01 % |
| `src/compose.rs` | 587 | 35 | 94.04 % |
| `src/cal_time.rs` | 194 | 11 | 94.33 % |
| `src/mail_dialog.rs` | 707 | 39 | 94.48 % |
| `src/mail_unsent.rs` | 129 | 7 | 94.57 % |
| `src/pdfwrite.rs` | 355 | 19 | 94.65 % |
| `src/script.rs` | 190 | 10 | 94.74 % |
| `src/cal_store.rs` | 884 | 46 | 94.80 % |
| `src/secrets.rs` | 289 | 15 | 94.81 % |
| `src/social/post_index.rs` | 197 | 10 | 94.92 % |
| `src/excalidraw_md.rs` | 474 | 24 | 94.94 % |
| `src/editor.rs` | 1357 | 66 | 95.14 % |
| `src/drawing_links.rs` | 216 | 10 | 95.37 % |
| `src/vim.rs` | 464 | 18 | 96.12 % |
| `src/people.rs` | 258 | 10 | 96.12 % |
| `src/canvas.rs` | 285 | 11 | 96.14 % |
| `src/lists.rs` | 446 | 17 | 96.19 % |
| `src/cal_dialog.rs` | 1391 | 50 | 96.41 % |
| `src/vault.rs` | 1302 | 43 | 96.70 % |
| `src/cal_ics.rs` | 1010 | 33 | 96.73 % |
| `src/drawing_layout.rs` | 397 | 13 | 96.73 % |
| `src/sidebar_ui.rs` | 184 | 6 | 96.74 % |
| `src/vault_path.rs` | 92 | 3 | 96.74 % |
| `src/import_dialog.rs` | 277 | 8 | 97.11 % |
| `src/mail_store.rs` | 944 | 27 | 97.14 % |
| `src/mail_db.rs` | 776 | 22 | 97.16 % |
| `src/tagwords.rs` | 157 | 4 | 97.45 % |
| `src/outbox.rs` | 925 | 23 | 97.51 % |
| `src/drawing_pictures.rs` | 169 | 4 | 97.63 % |
| `src/markdown.rs` | 531 | 11 | 97.93 % |
| `src/jobs.rs` | 52 | 1 | 98.08 % |
| `src/export_dialog.rs` | 191 | 3 | 98.43 % |
| `src/picker.rs` | 186 | 2 | 98.92 % |
| `src/dialog.rs` | 217 | 2 | 99.08 % |
| `src/keys_ui.rs` | 721 | 6 | 99.17 % |
| `src/keys.rs` | 533 | 3 | 99.44 % |
| `src/search_all.rs` | 185 | 1 | 99.46 % |
| `src/sidebar.rs` | 286 | 0 | 100.00 % |
| `src/theme.rs` | 52 | 0 | 100.00 % |

## Biggest gaps

The first ten are ordered by how much a silent bug costs (data loss, wrong mail or calendar state),
not by line count. **Tooling** means a plain test is not sensible and a helper (DEV-004) should be
proposed to the owner first; **test** means a plain unit or UI test is enough.

1. **Closed (#56): `vault.rs` `relocate` / `rewrite_drawing` (link rewrite, VAULT-016), 39 lines**: tests now cover a note changed after the scan, an unreadable note and a drawing that cannot be written; `trash` and `restore` onto a taken name were already tested. Original gap: the
   `failed_rewrites` branches (lines 914, 929, 951 to 952: a note or drawing that changes on disk
   between planning and writing, a write that fails) have no test, and neither do `trash` and
   `restore` when the name is taken (1056 to 1088: "already there; the trashed copy stays"). Test:
   rename a note whose referrer is made read-only or edited meanwhile, and check `failed_rewrites`,
   that the other files are rewritten and that nothing is lost; restore into an occupied name. Kind: test.
2. **Closed (#56): `fsutil.rs` `rename_noreplace` fallback (now `rename_noreplace_fallback`, tested directly) and `open_private_log`**; original gap: the
   path for systems without `renameat2` (hard link, then the exists check) is never run, yet every
   rename, trash and restore relies on its guarantee. Test: split the fallback into a function that
   takes "renameat2 unavailable" as an argument and check the three outcomes (new name, name taken,
   directory). Kind: test (small refactor).
3. **`mail_ops.rs` `exec_move`, `copy_then_delete`, `exec_expunge`**: closed (#58, DEV-004). The
   pretend IMAP server has the switches `move_cap`, `uidplus`, `no_copyuid` and `fail_every`
   (every `COPY`, `STORE` or `EXPUNGE` answered `NO`); `mail_ops.rs` tests cover the copy that the
   server refuses, the mark and the expunge that fail (the mail then exists in both folders and
   the mark is undone), verification by Message-ID with one mail not found, and "delete forever"
   without UIDPLUS. A Dovecot variant was not needed: the fake reaches the same client branches.
4. **`imap_client.rs` response parsing (`skip_value`, `skip_value_open`, `parse_fetch`,
   `parse_untagged`) and `append`**: closed (#57, MAIL-220, MAIL-221). Unit tests feed the parser
   odd but legal answers (literals, nested lists, NIL, escaped strings) and broken ones (also cut
   off after every byte, in a thread that must finish); a loopback `Wire` tests literals, NUL bytes,
   long lines and early ends; the pretend server has the switches `append_noise`,
   `append_cut_before_plus` and `append_answer` for the APPEND failures. One real gap found: a
   line longer than the limit passed when it arrived in one chunk; fixed.
5. **`imap_sync.rs` `real_factory`, `connector_for_account`, `connect_with_retry` and `mail_send.rs`
   `connector_for_account`, `connect_with_retry`, `smtp_settings`**: closed (#57, MAIL-222,
   MAIL-223), except the keyring paths. Counting auth factories test the retry; loopback ports
   (`closed_port`) and the pretend servers test refused connections and untrusted certificates;
   the settings mapping is checked. Not tested: `auth_factory` for a password account without a
   session password (it asks the real keyring; a fake keyring seam in `secrets.rs` would be
   needed) and the OAuth2 token fetch itself (covered in `oauth.rs`).
6. **`cal_dav.rs` `discover` (469 to 513), `request_bytes_opts` (245 to 315), `write_error`,
   `delete_event` (585 to 709)**: closed (#59, CAL-018, CAL-019).
   `cal_dav_status_tests.rs` runs the client against the scripted loopback server
   `cal_dav::tests::mock_http_with` (and a raw socket that is not HTTP). Originally: well-known and principal discovery with redirects and the words
   for 401, 403, 412 and 5xx are reached mainly by the ignored `caldav_env` suite. Test: a scripted
   HTTP server for the status and redirect cases (the "redirect and certificate rules" test of
   CALENV-002 shows the shape). Kind: test.
7. **`cal_store.rs` `delete_occurrence`, `save_event`, `poll`, `default_trash_dir` (139 to 638)**:
   closed (#59, CAL-020, CAL-021): `cal_store_failure_tests.rs`, with write failures made by a folder
   where the file should be (also holds for root, so no seam was needed). Originally: removing one date of a repeating event in a local calendar, saving to a read-only or missing
   directory, and the polling refresh. Test: a local calendar in a temp dir, delete one occurrence,
   save with the folder made read-only, poll after an outside change. Kind: test.
8. **`gpg.rs` `trust_words`, `inv_recp_words`, `op_failure`, `signatures`, `decrypt` (301 to 691) and
   `mail_crypto.rs` `analyze`, `classify_entity`, `rank` (184 to 664), 135 lines together**: the words
   shown for an expired, revoked or untrusted key, a missing recipient key, a bad signature and an
   unsupported entity. A wrong word here misleads the person about whether a mail is safe. Test: with
   the GnuPG test kit make an expired key, a revoked key and a corrupted signature, and check each
   message. Kind: test. Done in #60 (DEV-012): status-line tests for the words and verdicts
   (`signatures`, `decrypt_failure`, `inv_recp_words`, `op_failure`, `trust_words`), real-key tests
   for good, bad, unknown, expired-key, expired-signature and revoked signatures and for
   encrypting to or signing with an expired or revoked key (the test kit's `pgp_sign_expiring`),
   `classify_entity` for odd and nested mails, and `best_key`.
9. **`app/drops.rs` `handle_drops` (291 to 447, 46 % covered before) and `wl_drop.rs` `event` (264 to 397,
   45 %)**: dropping files, folders and mails from other programs (Wayland data offers, MIME kinds,
   the import dialog). Done in #61 (DEV-012): `wl_drop.rs` is split into a protocol-free part
   (`Drag`, `Landing`, `is_repeat`) with unit tests and a thin Wayland glue; `app/drops_tests.rs` feeds
   `DropEvent`s and window-system files to `handle_drops` in the headless window (drop on a folder, a
   note, the editor, nowhere; into an open dialog; hover, refusal and busy hints; unusable drops; no
   vault; a drag inside the file list); the `--keys` step `drop:T:/a;;/b` drops files at a place.
   Done in #75: the drag of a calendar event (`CalDrag`) is fed the same way (payload plus pointer
   in the headless window): onto a folder, a note, the editor and nowhere, with and without a
   title, dropped back on the calendar (nothing imported), into an open Import dialog, with no vault
   and with no pointer; the tests check the event, calendar name and `.ics` the dialog holds and the
   note that Import writes. There is no write to the calendar in this branch (a dragged event is
   only imported), so there is no failure path of that kind. Done in #89: dragging in the file list
   (`app/tree_ui_tests.rs`: into a folder, next to a note, to the top, into itself or its child,
   where it already is, a name that is taken, off the list, a closed folder opening). What stays
   outside the tests is the Wayland glue itself (the calls on a real data offer). Measured on
   2026-11-15 on `99e8a6e` plus the tests of #75 and #89 (`cargo llvm-cov --locked --lcov`, 988
   tests): `app/drops.rs` 403 lines, 42 missed, **89.58 %** (before: 216 missed, 46.40 %); the total
   is 52,421 lines, 48,408 covered, **92.34 %** (functions 7,171 of 7,812, 91.79 %; regions 87,332
   of 94,673, 92.25 %). The total includes the other waves merged into `99e8a6e`, not only these
   tests; `.forgejo/coverage-baseline` (88.75) is not changed here.
10. **`drawing.rs` `continue_drag`, `begin_drag`, `end_drag`, `click` (1450 to 1815), 270 lines, and
    `panels.rs` `modals`, `tasks_view`, `tree` (218 to 1110), 330 lines**: moving, resizing and
    rotating elements, the modal dialogs, the task list view and the folder tree. Test: headless UI
    tests with scripted pointer events as in `drawing_ui_tests.rs` (drag a rectangle, resize by a
    handle, shift-drag to constrain, drag after zoom), and one test per modal that opens it by its
    key, changes a value and closes it with Esc. Kind: test (large; split into issues per area).
    Done in #62: `drawing_pointer_tests.rs` (16 tests, DRAW-019) drives `begin_drag`,
    `continue_drag`, `end_drag` and `click` with real pointer events (select, Shift, move, zoomed
    drag, frame, corner resize with turn-over, text and scribbles, arrow ends, every shape tool,
    snapping, 45-degree lock, pencil, eraser, hand and Space pan, double-click editing) and found a
    bug (an arrow end dragged off its shape sprang back). `panels_ui_tests.rs` (8 tests,
    VAULT-023, VAULT-024, TASK-005) drives the quick switcher, Rename, New folder, Move to trash
    and Settings dialogs, the task list and the file tree. Done in #89: `dialogs_ui_tests.rs` (9
    tests) drives the file list's sort menu (name, modified, created; both directions), the
    Calendars dialog (add a local calendar, sample events, a bad and a good CalDAV address, Done,
    Cancel, Esc), the Event dialog (new event typed and saved, changed in place, Cancel, a date
    that does not exist, an end before the start), Export (the PDF on disk, an unwritable place, no
    place, Cancel), Import (a wrong folder, adding a file by path, the notes and the remembered
    folder and tags, Cancel), the mail accounts dialog (each wrong entry explained, a good
    account and the read settings saved, Cancel, Remove) and "Convert again" of the canvas
    question; `app/tree_ui_tests.rs` (5 tests) drives drag-and-drop in the file tree. Not done:
    the feed accounts dialog (already driven against pretend servers in `social/ui_tests.rs`, #63),
    the keyring and the real sign-in parts of the mail dialog (`mail_ui_tests.rs`), saving the sort
    choice to the settings file (the tests run without one), and the linear branch of the resize
    code, which only scribbles reach. Measured: `panels.rs` 1,553 lines, 137 missed, 91.18 %
    (before: 469 missed, 68.88 %).

More, in the order of the table:

- **`main.rs` `main`, `instance.rs` `show_already_running`: closed for everything that can be run
  without a window** (#63 part 2). The command line is read by `parse_args` (DEV-013, 7 tests), the
  lock check by `instance::claim` (DATA-012: owner, second start refused, stale file after a crash,
  broken data folder) and the start dialog is driven through the headless harness
  (`start_dialog_tests.rs`, VAULT-021, VAULT-022). What stays uncovered is `main` itself (the real
  window and event loop) and the little window of `show_already_running`: both need a real display.
  There is no hand-over of a second start's arguments to the running Shale (the second one only
  says it is running); that is a missing feature, not a missing test.
- **`reader.rs` `embed` (380 to 462, 63 %), `layout.rs` `welcome`, `view_header`,
  `paint_recovery_notice` (242 to 817), `graph.rs` `ui` (164 to 322, 68 %), `app.rs`
  `calendar_action`, `calendar_dialog_action`, `shortcuts`, `process_actions` (1420 to 1807),
  `mail_ui.rs` `handle`, `apply_compose_out`, `compose_send` (680 to 2301), `app/sync.rs`
  `after_sync`, `sync_tick` (84 to 223), `app/mail.rs` `mail_dialog_action` (56 to 143),
  `link_assist.rs` `after` (219 to 393, 83 %), `social/accounts_ui.rs` `sign_in`, `poll` (110 to
  161, 76 %)**: the app-level glue and the views. Test: UI tests that drive the action and check
  the state (the pattern of `mail_ui_tests.rs`). Kind: test. **Closed for the sign-in of the Feeds
  dialog and `oauth.rs` `sign_in` / `wait_for_code` (#63):** the pretend browser
  (`oauth_fake_tests.rs::pretend_browser`, `social/fake_server.rs::pretend_browser`) opens the login
  URL like a person would (approve, deny, wrong state, never answers); the Feeds dialog is tested
  against a pretend Mastodon and Bluesky server.
- **`opener.rs` `launch`, `open_url`, `open_recovery_folder` (17 to 293)**: starting the desktop's
  opener. Kind: tooling. **Closed (#63):** `FakeXdgOpen` is a recording fake `xdg-open` for one test
  thread (it never touches `PATH`); `opener.rs::real_opener_tests` check every SEC-006 rule through
  the real opener.
- **Closed (#56): `config.rs` `save`, `load`, `config_path` (a test-only path seam) and `vault_state.rs` `reload_settings`** (`adopt_legacy` was tested already). Original gap: reading and writing the settings file and the first-start
  migration. Test: point `XDG_CONFIG_HOME` at a temp dir; save, load, a corrupt file, a read-only
  directory. Kind: test.
- **`tb_store.rs`: closed** (#63 part 2). `tb_fixture_tests.rs` generates Thunderbird profiles
  (`profiles.ini`, `prefs.js`, mbox and maildir folders, `.sbd` nesting, non-ASCII names, `.msf`
  indexes, flags, large and broken files); `tb_store_tests.rs` has 21 tests on them (IMP-017).
- **`mail.rs` `entity` (372 to 409), `parse_date` (349 to 363), `decode_charset` (108 to 126),
  `html_to_text`**: HTML entities, odd date formats and charsets in incoming mail. Test: table tests
  with real-world strings. Kind: test.
- **`social/images.rs`, `social/markup.rs`, `social/note.rs`, `social/bluesky.rs`**: 80 to 92 %; the
  picture cache workers and the error branches of the feed fetchers. Test: a scripted HTTP server as
  for the CalDAV status cases. Kind: test.
- **Keys and the sidebar** are at 97 to 100 %: `keys.rs` `reserved` (line 40), `sidebar_ui.rs`
  `cue_at`, `side_drag_tick` (75 to 263). Kind: test.
