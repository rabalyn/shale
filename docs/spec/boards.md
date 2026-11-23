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
| BOARD-001 | **A board is a note** (#601). A Markdown note with `type: board` in its front matter. It is a normal note: it opens in the editor, is searchable and linkable, and other apps read it. | SHOULD BE | `board/file.rs` (planned) | — |
| BOARD-002 | **A lane is a `##` heading** (#601). Lanes live in the body, not in the front matter, because `markdown::front_matter_values` reads only flat `key: value` lines. The heading text is the lane's name; the order in the file is the order of the lanes. | SHOULD BE | `board/file.rs` (planned) | — |
| BOARD-003 | **Lane lines** (#601). Under a lane heading, `key: value` lines set `query:` (QUERY-001…QUERY-006), `sort:` (BOARD-015), `drop:` (BOARD-019) and `done: true` (BOARD-024). The value is everything after the first colon, so `drop: status: doing` sets the property `status: doing`. | SHOULD BE | `board/file.rs` (planned) | — |
| BOARD-004 | **Pinned cards** (#601). A list of `- [[links]]` under a lane are pinned cards in that order (BOARD-022). A lane with links and no `query:` is a manual lane. | SHOULD BE | `board/file.rs` (planned) | — |
| BOARD-005 | **Board-level properties** (#601). `cards: every-lane` in the front matter shows a card in every lane it matches; the default is exclusive (BOARD-009). `card-shows: due, status, tags` chooses the chips of the cards (BOARD-010). | SHOULD BE | `board/file.rs` (planned) | — |
| BOARD-006 | **View and file agree** (#601). Adding, removing, renaming and reordering lanes in the view rewrites the file; editing the file by hand changes the view. Lines the reader does not know stay as they are. | SHOULD BE | `board/file.rs` (planned) | — |

## The view

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| BOARD-007 | **A main-area tab** (#601). The board is a view of the main area, a tab like Graph and Calendar (ADR-0033). A strip at its top lists every `type: board` note of the vault; one click switches. | SHOULD BE | `board/view.rs` (planned) | — |
| BOARD-008 | **A key for the boards** (#601). The definable action `main.boards` (scope Main views, KEY-040) opens the board view; the proposed default is `Space b` (free: the defaults of KEY-040 are `Space f f` and `Space f g`). The clash rules (KEY-004, KEY-017) apply. | SHOULD BE | `keys::DEFS` (planned) | — |
| BOARD-009 | **Exclusive lanes** (#601). A card is in the first lane whose query matches. With `cards: every-lane` (BOARD-005) it is in every lane it matches. | SHOULD BE | `board/lanes.rs` (planned) | — |
| BOARD-010 | **Cards** (#601). A card shows the title, chips for the due date (red when overdue), status, priority (VAULT-047) and tags, as `card-shows` (BOARD-005) says, and a thumbnail for a drawing. | SHOULD BE | `board/view.rs` (planned) | — |
| BOARD-011 | **Drawings on a board** (#601). A drawing can be matched by name, path and links only, until drawings get tags; its other fields are empty. | SHOULD BE | `board/lanes.rs` (planned) | — |
| BOARD-012 | **Click selects and opens the peek** (#601). A click on a card selects it and opens its peek: the floating frame of the link preview (EDIT-033, size of EDIT-037, draggable and resizable). Esc closes the peek. | SHOULD BE | `board/peek.rs` (planned) | — |
| BOARD-013 | **The peek is an editor** (#601). The peek holds a real editor that saves as the editor does (a checked save, `Vault::save_checked`, DATA-003). It has a control to set the priority (VAULT-047). | SHOULD BE | `board/peek.rs` (planned) | — |
| BOARD-014 | **Open in a tab** (#601). Ctrl+Click on a card, or Enter on the selected card, opens its note in a tab. | SHOULD BE | `board/view.rs` (planned) | — |
| BOARD-032 | **Pinned cards are taken first** (#601). A card pinned in a lane (BOARD-004) is in that lane; the lanes before and after it do not show it, even when their query matches. | SHOULD BE | `board/lanes.rs` (planned) | — |

## Sorting

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| BOARD-015 | **The sort line** (#601, #603). `sort: priority desc, due asc` sorts the lane by the keys in order. A key has `asc` (default) or `desc`, except `dependencies` (BOARD-026). | SHOULD BE | `board/sort.rs` (planned) | — |
| BOARD-016 | **Sort keys** (#601, #603). `created` is the front matter `created:` (VAULT-045), else the `YYMMDD_` name prefix (VAULT-025), else the file time. `modified` is `modified:`, else the file time. `due` is `due:`, else the earliest open task due date of the note (TASK-002). `priority` is VAULT-047. `title` is the title line. `dependencies` is BOARD-026. | SHOULD BE | `board/sort.rs` (planned) | — |
| BOARD-017 | **No value sorts last** (#601). A note without a value for a key sorts after the others, in either direction. | SHOULD BE | `board/sort.rs` (planned) | — |
| BOARD-018 | **Without a sort line** (#601). Pinned cards first, then the others by title. | SHOULD BE | `board/sort.rs` (planned) | — |

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
| BOARD-030 | **The board as a PDF, built-in engine** (#604). Export writes the board as landscape page(s) with the lanes as columns (`pdf/write.rs::add_page` takes a size per page), then the notes of the cards in portrait through the book export (BOOK-001…BOOK-009), in lane order and then card order, with a table of contents. Drawings are vector (PDF-001). | SHOULD BE | `pdf/board.rs` (planned) | — |
| BOARD-031 | **The board as a PDF, LaTeX engine** (#605). The same order through the book templates (BOOK-013…BOOK-016), with the board on a landscape page (`pdflscape`). | SHOULD BE | `tex/board.rs` (planned) | — |
