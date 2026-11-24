# Spec: Kanban boards (BOARD)

**IS NOW** = true of the code today (⚠ = known defect) · **SHOULD BE** = agreed target.
Format: [ADR-0001](../decisions/0001-spec-driven-development.md). Decision:
[ADR-0034](../decisions/0034-kanban-boards.md). Issues: #596 (epic), #601, #602, #603, #604, #605.
The query language of the lanes is in [search-queries.md](search-queries.md) (QUERY); the note
properties `created:`, `modified:` and `priority:` are VAULT-045…VAULT-047. Every row here is
planned work; none is true yet.

A board is a Markdown note. Its lanes are made from queries, its cards are notes.

```
---
type: board
card-shows: due, status, tags
---
# Sprint 48

## Backlog
query: #todo -#done

## Doing
query: status:doing
drop: status: doing

## Pinned by hand
- [[Release notes 0.9]]
- [[Kanban sketch.excalidraw]]
```

## The board file

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| BOARD-001 | **A board is a note** (#601). A Markdown note with `type: board` in its front matter. It is a normal note: it opens in the editor, is searchable and linkable, and other apps read it. | IS NOW | `board/file.rs` (`is_board`), `app/boards.rs` (`ShaleApp::new_board`) | `board/file.rs::board_001_only_type_board_makes_a_note_a_board`, `board/ui_tests.rs::board_001_new_board_makes_a_note_with_type_board_and_one_lane`, `board/ui_tests.rs::board_001_a_board_is_a_normal_note_that_opens_in_the_editor` |
| BOARD-002 | **A lane is a `##` heading** (#601). Lanes live in the body, not in the front matter, because `markdown::front_matter_values` reads only flat `key: value` lines. The heading text is the lane's name; the order in the file is the order of the lanes. | IS NOW | `board/file.rs` (`Board::parse`) | `board/file.rs::board_002_a_lane_is_a_level_two_heading_in_file_order` |
| BOARD-003 | **Lane lines** (#601). Under a lane heading, `key: value` lines set `query:` (QUERY-001…QUERY-006), `sort:` (BOARD-015), `drop:` (BOARD-019) and `done: true` (BOARD-024). The value is everything after the first colon, so `drop: status: doing` sets the property `status: doing`. | IS NOW | `board/file.rs` (`Lane::query`, `sort`, `drop`, `done`) | `board/file.rs::board_003_lane_lines_take_everything_after_the_first_colon` |
| BOARD-004 | **Pinned cards** (#601). A list of `- [[links]]` under a lane are pinned cards in that order (BOARD-022). A lane with links and no `query:` is a manual lane. | IS NOW | `board/file.rs` (`Lane::pins`, `pin_line`) | `board/file.rs::board_004_a_list_of_links_is_the_pinned_cards_and_no_query_is_a_manual_lane` |
| BOARD-005 | **Board-level properties** (#601). `cards: every-lane` in the front matter shows a card in every lane it matches; the default is exclusive (BOARD-009). `card-shows: due, status, tags` chooses the chips of the cards (BOARD-010). | IS NOW | `board/file.rs` (`props`), `board/view.rs` (the Board menu) | `board/file.rs::board_005_the_front_matter_sets_the_card_mode_and_the_chips`, `board/ui_tests.rs::board_009_cards_every_lane_shows_a_card_in_every_lane_it_matches`, `board/ui_tests.rs::board_010_the_board_menu_chooses_the_chips` |
| BOARD-006 | **View and file agree** (#601). Adding, removing, renaming and reordering lanes in the view rewrites the file; editing the file by hand changes the view. Lines the reader does not know stay as they are. | IS NOW | `board/file.rs` (`Board::write` and the lane edits), `app/boards.rs` (`edit_board`, `edit_note_text`) | `board/file.rs::board_006_reading_and_writing_a_file_gives_the_same_bytes`, `board/file.rs::board_006_changes_touch_only_their_lines_and_keep_the_unknown_ones`, `board/file.rs::board_006_lanes_are_added_removed_and_moved_with_their_lines`, `board/file.rs::board_006_a_lane_name_is_one_line_and_never_blank`, `board/file.rs::board_006_the_pins_are_replaced_in_place`, `board/file.rs::board_006_any_text_is_written_back_unchanged`, `board/file.rs::board_006_an_edit_reads_back_as_the_edit_and_keeps_the_unknown_lines` (property tests, DEV-042), `board/ui_tests.rs::board_006_adding_renaming_moving_and_removing_lanes_rewrites_the_file`, `board/ui_tests.rs::board_006_a_hand_edit_of_the_file_changes_the_view`, `board/ui_tests.rs::board_006_a_change_in_the_view_while_the_note_is_open_in_a_tab_goes_through_its_buffer` |

## The view

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| BOARD-007 | **A main-area tab** (#601). The board is a view of the main area, a tab like Graph and Calendar (ADR-0033). A strip at its top lists every `type: board` note of the vault; one click switches. The tab comes back after a restart with its board (NAV-044). The ribbon has a **Boards** icon after Tasks (`Item::Boards`, id `boards`, tooltip with the key); it shows the view, or the tab that has it (LAY-001, LAY-002: a stored layout gets it at the end of the ribbon). A lane has **+ Add lane** in the top bar, and **Delete lane…** asks before it takes the lane's lines out of the file. | IS NOW | `board/view.rs`, `board/mod.rs` (`list`), `app/boards.rs` (`boards_view`), `app.rs` (`View::Boards`) | `board/ui_tests.rs::board_007_the_view_is_a_main_area_tab_with_a_strip_of_every_board`, `board/ui_tests.rs::board_007_a_saved_boards_tab_comes_back_with_its_board`, `board/ui_tests.rs::board_007_the_ribbon_has_a_boards_icon_that_opens_or_shows_the_boards` |
| BOARD-008 | **A key for the boards** (#601). The definable action `main.boards` (scope Main views, KEY-040) opens the board view, like the Boards icon of the ribbon (BOARD-007); the proposed default is `Space b` (free: the defaults of KEY-040 are `Space f f` and `Space f g`). The clash rules (KEY-004, KEY-017) apply. | IS NOW | `keys::DEFS` (`main.boards`), `ShaleApp::shortcuts` | `board/ui_tests.rs::board_008_space_b_opens_the_boards_and_the_key_can_be_changed` |
| BOARD-009 | **Exclusive lanes** (#601). A card is in the first lane whose query matches. With `cards: every-lane` (BOARD-005) it is in every lane it matches. The note of the board itself is never one of its cards. | IS NOW | `board/lanes.rs` (`place`) | `board/lanes.rs::board_009_a_card_is_in_the_first_lane_that_matches_unless_every_lane_is_set`, `board/lanes.rs::board_009_the_board_is_not_a_card_of_its_own_lanes`, `board/ui_tests.rs::board_009_a_card_is_in_the_first_lane_that_matches_and_a_pinned_card_wins`, `board/ui_tests.rs::board_009_cards_every_lane_shows_a_card_in_every_lane_it_matches` |
| BOARD-010 | **Cards** (#601). A card shows the title, chips for the due date (red when overdue), status, priority (VAULT-047) and tags, as `card-shows` (BOARD-005) says, and a thumbnail for a drawing. | IS NOW | `board/card.rs`, `board/view.rs` (`chips`, `card_ui`) | `board/card.rs::board_010_a_card_has_the_chips_of_its_note`, `board/ui_tests.rs::board_010_a_card_shows_its_chips_as_card_shows_says`, `board/ui_tests.rs::board_010_the_board_menu_chooses_the_chips` |
| BOARD-011 | **Drawings on a board** (#601). A drawing can be matched by name, path and links only, until drawings get tags; its other fields are empty. It is a card with its picture, and a click opens it in a tab (it has no text to edit in the peek). | IS NOW | `board/card.rs` (`Card::of`), `board/view.rs` (`card_ui`) | `board/card.rs::board_011_a_drawing_is_a_card_with_a_title_and_no_fields`, `board/ui_tests.rs::board_011_a_drawing_is_a_card_found_by_name_and_has_a_picture` |
| BOARD-012 | **Click selects and opens the peek** (#601). A click on a card selects it and opens its peek: the floating frame of the link preview (EDIT-033, size of EDIT-037, draggable and resizable). Esc closes the peek. | IS NOW | `board/peek.rs`, `app/boards.rs` (`open_peek`, `close_peek`, `boards_keys`) | `board/ui_tests.rs::board_012_a_click_selects_the_card_and_opens_the_peek_and_esc_closes_it`, `board/ui_tests.rs::board_012_the_peek_has_the_size_of_the_link_preview_and_is_resized_by_its_edges`, `board/ui_tests.rs::board_012_the_peek_is_moved_by_its_title`, `board/ui_tests.rs::board_014_the_arrow_keys_move_the_selection_between_the_cards` |
| BOARD-013 | **The peek is an editor** (#601). The peek holds a real editor that saves as the editor does (a checked save, `Vault::save_checked`, DATA-003). It has a control to set the priority (VAULT-047). | IS NOW | `board/peek.rs` (`Peek::show`), `app/boards.rs` (`boards_peek`), `note_props.rs` (`with_priority`) | `board/ui_tests.rs::board_013_the_peek_is_an_editor_that_saves_with_the_note`, `board/ui_tests.rs::board_013_the_priority_control_writes_the_front_matter_and_not_modified`, `board/ui_tests.rs::board_013_space_space_ticks_the_task_in_the_peek_in_vim_normal_mode_and_esc_leaves_insert_first` |
| BOARD-014 | **Open in a tab** (#601). Ctrl+Click on a card, or Enter on the selected card, opens its note in a tab. | IS NOW | `board/view.rs` (`card_ui`, `keys`) | `board/ui_tests.rs::board_014_ctrl_click_and_enter_open_the_note_in_a_tab`, `board/ui_tests.rs::board_014_the_arrow_keys_move_the_selection_between_the_cards` |
| BOARD-032 | **Pinned cards are taken first** (#601). A card pinned in a lane (BOARD-004) is in that lane; the lanes before and after it do not show it, even when their query matches. | IS NOW | `board/lanes.rs` (`place`) | `board/lanes.rs::board_032_a_pinned_card_is_only_in_the_lane_that_pins_it_and_comes_first`, `board/ui_tests.rs::board_009_a_card_is_in_the_first_lane_that_matches_and_a_pinned_card_wins` |

## Sorting

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| BOARD-015 | **The sort line** (#601, #603). `sort: priority desc, due asc` sorts the lane by the keys in order. A key has `asc` (default) or `desc`, except `dependencies`, which has no direction and is ordered by BOARD-026 (#603: until then it orders nothing). The pinned cards stay on top in the order of their links (BOARD-022); the keys order the others. | IS NOW | `board/sort.rs` (`sort`), `search_form/sort.rs` (`parse`, `write`) | `board/sort.rs::board_015_the_keys_sort_in_order_with_a_direction` |
| BOARD-016 | **Sort keys** (#601). `created` is the front matter `created:` (VAULT-045), else the `YYMMDD_` name prefix (VAULT-025), else the file time. `modified` is `modified:`, else the file time. `due` is `due:`, else the earliest open task due date of the note (TASK-002). `priority` is VAULT-047. `title` is the title line (compared without case). | IS NOW | `board/card.rs` (`Card::of`), `board/sort.rs`, `note_props.rs` | `board/sort.rs::board_016_each_key_compares_its_own_value`, `board/card.rs::board_016_due_is_the_property_else_the_earliest_open_task`, `board/card.rs::board_016_created_and_modified_fall_back_to_the_name_prefix_and_the_file` |
| BOARD-017 | **No value sorts last** (#601). A note without a value for a key sorts after the others, in either direction. | IS NOW | `board/sort.rs` (`by`) | `board/sort.rs::board_017_a_card_without_a_value_is_last_in_either_direction` |
| BOARD-018 | **Without a sort line** (#601). Pinned cards first, then the others by title. | IS NOW | `board/sort.rs` (`sort`), `board/mod.rs` (`lane_views`) | `board/sort.rs::board_018_without_keys_the_cards_come_by_title`, `board/ui_tests.rs::board_009_a_card_is_in_the_first_lane_that_matches_and_a_pinned_card_wins` |

## Dragging

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| BOARD-019 | **A drop writes the lane's property** (#602). Dropping a card on a lane writes the lane's `drop:` property into the card note's front matter. A lane with a simple query such as `status:doing` and no `drop:` line uses the property the query says (`status: doing`). | SHOULD BE | `board/drag.rs` (planned) | — |
| BOARD-020 | **A drop does not count as a change** (#602). The front matter write of a drop does not change `modified:` (VAULT-046). The write is a checked save (DATA-003). | SHOULD BE | `board/drag.rs` (planned) | — |
| BOARD-021 | **A lane without a drop property** (#602). A drop on a lane with no `drop:` line and no simple query only reorders its pinned cards (BOARD-022); the card note is not changed. | SHOULD BE | `board/drag.rs` (planned) | — |
| BOARD-022 | **Manual order is the link list** (#602). The order of the pinned cards of a lane is the order of its `- [[link]]` lines in the board file; a drop inside a lane or onto a manual lane rewrites that list. | SHOULD BE | `board/drag.rs`, `board/file.rs` (planned) | — |

## Dependencies

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| BOARD-023 | **Dependency properties** (#603). `blocked-by: [[A]], [[B]]`, `due:` and `start:` in a card note's front matter. A card shows a "blocked" badge while any of its blockers is not done (BOARD-024). The query words `blocked` and `blocking` (QUERY-002) match by the same rule. | SHOULD BE | `board/deps.rs` (planned) | — |
| BOARD-024 | **What done means** (#603). A blocker is done when it has `status: done`, or when it is in a lane marked `done: true` on the board (BOARD-003). | SHOULD BE | `board/deps.rs` (planned) | — |
| BOARD-025 | **Arrows** (#603). Selecting a card draws arrows to its blockers and to the cards it blocks. | SHOULD BE | `board/view.rs` (planned) | — |
| BOARD-026 | **Dependency order** (#603). `sort: dependencies` is a topological order: every card comes after all its blockers. Among cards that are free at the same step, the earlier due date comes first, then the higher priority. | SHOULD BE | `board/deps.rs` (planned) | — |
| BOARD-027 | **Step numbers** (#603). In that order the cards show step numbers: "1", "2", "3a" and "3b" for cards that can run in parallel in step 3. | SHOULD BE | `board/deps.rs` (planned) | — |
| BOARD-028 | **A cycle is shown, not hidden** (#603). Cards in a cycle cannot be ordered; they are marked "circular" and listed after the ordered cards. | SHOULD BE | `board/deps.rs` (planned) | — |
| BOARD-029 | **A lane preset** (#603). "My dependency chain" is a lane preset: `query: blocked OR blocking`, `sort: dependencies`. | SHOULD BE | `board/lanes.rs` (planned) | — |

## PDF

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| BOARD-030 | **The board as a PDF, built-in engine** (#604). Export (the **Export PDF…** button of the board view, and **Export to PDF** of a board note) writes the board as landscape pages of the dialog's paper, the lanes as columns (`pdf/write.rs::add_page` takes a size per page), then the notes of the cards in portrait through the book export (BOOK-001…BOOK-009): a part for each lane that has cards and the cards as its chapters, in lane order and then card order, a card of several lanes once (in the first), with a table of contents whose first line is the board, bookmarks and a link from each card to its note. As many columns as fit the page width (190 pt at least, 260 pt at most) make a group; more lanes make more groups, left to right, one after the other; cards that do not fit the height go on the next page of the group with the same columns ("continued"); a card is never split. A card shows its title and the chips of `card-shows`; a drawing card also its drawing as vector paths (PDF-001), and its note page is the drawing embedded. A lane whose query fails shows the reason under its name and has no cards; a board without lanes or cards is the board page only. `src/board/resolve.rs::resolve` is the shared step (lanes and cards without the window's search worker) that BOARD-031 reuses. | IS NOW | `pdf/board.rs`, `board/resolve.rs`, `pdf/book_export.rs::lay_out` | `board/resolve_tests.rs::board_030_*`, `pdf/board_tests.rs::board_030_*`, `board/card.rs::board_030_the_chips_as_words_follow_card_shows_and_leave_out_what_the_card_has_not`, `board/ui_tests.rs::board_030_the_export_button_of_the_board_opens_the_export_dialog_of_the_open_board`, `dialogs_ui_tests.rs::board_030_the_export_of_a_board_note_opens_the_dialog_of_the_board_and_saves_the_pdf_where_asked` |
| BOARD-031 | **The board as a PDF, LaTeX engine** (#605). The same order through the book templates (BOOK-013…BOOK-016), with the board on a landscape page (`pdflscape`). | SHOULD BE | `tex/board.rs` (planned) | — |
