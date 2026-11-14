# Spec: Links, editor and navigation (LINK, EDIT, NAV)

**IS NOW** = true of the code today (⚠ = known defect, described as it behaves) ·
**SHOULD BE** = agreed target. Format: [ADR-0001](../decisions/0001-spec-driven-development.md).
Tests are named `file.rs::test_fn` (all under `src/`).

## Link syntax and resolution (LINK)

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| LINK-001 | Recognised links: `[[Target]]`, `[[Target\|alias]]`, `[[Target#Heading]]`, embeds `![[Target]]`, and markdown links `[text](Note.md)` (with `%20`). Links inside inline or fenced code are ignored. | IS NOW | `markdown::find_links` | `markdown.rs::wiki_links_basic`, `markdown.rs::embeds_and_md_links`, `markdown.rs::links_skip_code`, `markdown.rs::unicode_links`, `markdown.rs::offsets` |
| LINK-002 | Resolution by **name** is case-insensitive and ignores `.md`. If several files share the name, the one in the linking note's folder wins, then the one with the fewest folder levels, then alphabetical path. | IS NOW | `Vault::resolve` | `vault.rs::resolve_and_backlinks` (partly: case; the tie-break order is untested) |
| LINK-003 | Resolution by **path** (target contains `/`): first relative to the linking note's folder, then from the vault root (`../` is not normalised); if neither exists, the last part is resolved **by name**. ⚠ So `[[Archive/Plan]]` shows blue and opens `Projects/Plan.md` when `Archive/Plan.md` does not exist. | IS NOW ⚠ | `Vault::resolve` (`vault.rs:236-238`) | `vault.rs::resolve_and_backlinks` (partly: vault-root path; the fallback is untested) |
| LINK-004 | A link that resolves is shown **blue**; one that does not is **red** (editor, reading view, task list, outgoing links). | IS NOW | `editor.rs`, `reader.rs` | `editor.rs::highlight_colours_links`, `editor.rs::highlight_handles_unicode_and_headings` |
| LINK-005 | Clicking a blue link opens the target. Clicking a red link creates the note (vault root, or next to the current note if that setting is on) and opens it; a red `*.excalidraw` link creates a drawing. Middle-click or Shift-click opens in a new tab; Ctrl+click puts the cursor into the link (swappable in Settings). | IS NOW | `editor.rs`, `Action::OpenLink` | `editor.rs::clicking_links_in_the_editor`, `app.rs::blue_link_opens_existing_note`, `app.rs::red_link_creates_note_and_turns_blue` |
| LINK-006 | A `#` in a link starts a heading — **unless** the part after it ends in a file extension (`[[Mail #2583.eml]]` is a file name). A `#` inside `[[…]]` is never a tag. | IS NOW | `markdown::find_links` | `markdown.rs::file_names_with_a_hash_sign` |
| LINK-007 | Backlinks, unlinked mentions (the note's name in plain text), outgoing links (existing / missing) and the outline are shown for the open note. | IS NOW | `Vault::backlinks`, `unlinked_mentions`, `outgoing`; `markdown::headings` | `vault.rs::resolve_and_backlinks` (backlinks), `markdown.rs::headings` (outline); mentions and outgoing untested |
| LINK-008 | When two notes share a name, the ambiguity is visible to the user (e.g. the link tooltip names the chosen file) instead of being resolved silently. | SHOULD BE | — | ADR-0004 |
| LINK-009 | Block links (`#^id`) and heading links that scroll in the reading view. | SHOULD BE (backlog) | — | — |
| LINK-010 | A path link resolves from the vault root; `./` and `../` relative to the linking note; a path that matches nothing is **red** (no fallback to the name). | SHOULD BE | — | [ADR-0004](../decisions/0004-link-resolution-and-paths.md) |
| LINK-011 | The graph has one node per note and drawing, plus one node per unresolved link target (red); edges are resolved links. | IS NOW | `Vault::graph` | `vault.rs::graph_has_edges` |

## Editing (EDIT)

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| EDIT-001 | Typing `[[` lists matching notes and drawings (fuzzy); Enter/Tab picks, the last entry creates a new note, `]]` is added. Esc closes the list and it stays closed for that `[[`. | IS NOW | `editor.rs` | `editor.rs::typing_double_brackets_suggests_and_completes`, `editor.rs::escape_closes_the_note_suggestions`, `editor.rs::link_query_detects_open_brackets`, `markdown.rs::fuzzy` |
| EDIT-002 | Typing `[!` (also `[[!`) opens the file picker: `/` = vault root, `~/` = home, other paths relative to the vault root. Tab completes like a shell and never finishes the link; Enter or click picks; Esc closes. | IS NOW | `editor.rs`, `media.rs` | `editor.rs::file_picker_completes_paths_and_links_the_file`, `editor.rs::file_picker_arrows_and_tab_keep_working`, `editor.rs::escape_closes_the_file_picker`, `editor.rs::picker_safety_net_when_keys_reach_the_text_box`, `editor.rs::double_bracket_bang_opens_only_the_file_picker`, `media.rs::query_detection`, `media.rs::splitting_paths`, `media.rs::tab_completion_like_a_shell` |
| EDIT-003 | Picking a file outside the vault copies it to `z_media/<YYYY>/<MM>/<YYMMDD>_<name>` (number added if taken) and links the copy; files inside the vault are linked in place. Images become `![[…]]`, other files `[[…]]`. | IS NOW | `media::import_file`, `media::link_for` | `media.rs::importing_copies_into_dated_media_folder`, `media.rs::local_date_is_sane` |
| EDIT-004 | `[[p ` + name lists only contact notes from the people folder and writes a normal `[[Name]]` link (`[[Folder/Name\|Name]]` if the name is not unique). | IS NOW | `editor.rs`, `people.rs` | `editor.rs::double_brackets_p_lists_people` |
| EDIT-005 | Enter continues `-`, `*`, `+`, `1.`, `1)` lists and to-dos; Enter on an empty item ends or outdents it; Shift+Enter is a plain new line. Tab/Shift+Tab indent list items by 4 spaces (renumbering) or change heading level 1–6. Alt+↑/↓ move lines and tidy the list. | IS NOW | `lists.rs` | `lists.rs::prefixes`, `lists.rs::enter_continues_lists`, `lists.rs::enter_on_empty_item_ends_or_outdents`, `lists.rs::enter_in_the_middle_renumbers`, `lists.rs::enter_splits_line_at_cursor`, `lists.rs::tab_indents_lists_and_headings`, `lists.rs::moving_lines`, `editor.rs::enter_and_tab_keep_lists_going`, `editor.rs::alt_arrows_move_lines` |
| EDIT-006 | Pictures, drawings and PDFs embedded with `![[…]]` are shown inside the editor (setting, on by default) and turn back into link text while the cursor is in the link; typing does not make them flicker. | IS NOW | `editor.rs` | `editor.rs::inline_images_show_until_the_cursor_enters_the_link`, `editor.rs::drawings_and_pdfs_show_inline_like_pictures`, `editor.rs::typing_keeps_pictures_in_place`, `editor.rs::remapping_follows_edits` |
| EDIT-007 | Clicking `[ ]` in the editor or reading view ticks the task and saves it. | IS NOW | `editor.rs`, `reader.rs` | `editor.rs::clicking_a_checkbox_in_the_editor_ticks_it` (editor only) |
| EDIT-008 | The editor stays responsive for notes up to 1 MB (colouring is not recomputed for the whole note on every key). | SHOULD BE | today whole-note recolouring | — |
| EDIT-009 | `[name](https://…)` and bare `https://…` addresses are web links; a click opens them in the default browser, Ctrl+click edits them. | IS NOW | `lists.rs` (`open_in_browser`), `editor.rs` | `lists.rs::web_links`, `editor.rs::clicking_a_web_link_opens_the_browser_action` |

## Navigation (NAV)

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| NAV-001 | Each tab has its own back/forward history (Alt+←/→, mouse side buttons). | IS NOW | `app.rs` `Tab` | `app.rs::blue_link_opens_existing_note` (partly: Back) |
| NAV-002 | Ctrl+H / Ctrl+L open the previous / next note or drawing in the same folder in the file list's sort order, skipping attachments; at either end a message says so. | IS NOW | `ShaleApp::open_neighbour`, `panels::neighbour_note` | `panels.rs::ctrl_h_and_l_follow_the_file_list_order` |
| NAV-003 | The file list sorts by name, modified or created time (each both ways); folders use the same order; created time falls back to modified time. | IS NOW | `panels::sorted_files_in`, `config::FileSort` | `panels.rs::sorting_modes` |
| NAV-004 | Ctrl+Shift+F always focuses the search box and selects its text. | IS NOW | `panels.rs` | `panels.rs::the_search_shortcut_always_focuses_and_selects_the_old_search` |
| NAV-005 | Ctrl+O quick switcher finds or creates a note. | IS NOW | `panels.rs` | untested |
| NAV-007 | Search results are a list of stops: every found line (up to 20 per file, all shown), and a file found only by its name is one stop. ↓ or Enter in the search box moves into the list with the first stop selected; Esc or `/` go back to the box. | IS NOW | `panels::search_panel` | `panels.rs::search_results_are_browsed_with_j_k_and_ctrl_j_k`, `panels.rs::enter_in_the_box_selects_and_esc_goes_back` |
| NAV-008 | In the result list, j / ↓ and k / ↑ select the next / previous stop (across files); Ctrl+J / Ctrl+K select the first stop of the next / previous file. The selection stays in view and stops at the ends. | IS NOW | `panels::search_panel` | `panels.rs::search_results_are_browsed_with_j_k_and_ctrl_j_k` |
| NAV-009 | Enter opens the selected stop in the current tab with the keyboard in the editor and the cursor at the start of the found text (matching ignores case, also for non-ASCII letters); a name-only stop opens the file at the top. | IS NOW | `Action::GotoMatch`, `ShaleApp::goto_match`, `markdown::find_ci` | `panels.rs::search_results_are_browsed_with_j_k_and_ctrl_j_k`, `panels.rs::enter_in_the_box_selects_and_esc_goes_back`, `markdown.rs::find_ignores_case_also_for_other_letters` |
| NAV-006 | There is one keyboard reference (single source) used by README and FEATURES. | SHOULD BE | today two tables that differ | — |
