# Spec: the shared query language, its validator, the search worker and the filter form (QUERY)

**IS NOW** = true of the code today (⚠ = known defect) · **SHOULD BE** = agreed target.
Format: [ADR-0001](../decisions/0001-spec-driven-development.md). Decision:
[ADR-0034](../decisions/0034-kanban-boards.md). Issues: #596 (epic), #597, #598, #599.

One query language is read by the board lanes (BOARD-002), the sidebar search (NAV-007) and the
picker (NAV-022). Every query is checked before it runs, runs on a background worker and ends
within a budget. The same filter and sort form edits it everywhere. Every row here is planned
work; none is true yet.

## The language

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| QUERY-001 | **Operators** (#597). `AND`, `OR` and `NOT` are operators only in upper case, so "salt and pepper" stays a text search. Parentheses group. Precedence is NOT, then AND, then OR. Two terms side by side are AND. `-x` is `NOT x`. | SHOULD BE | `search_all/query.rs` (planned) | — |
| QUERY-002 | **Fields** (#597). `#tag` matches a tag; `prop:value` matches any front-matter key (`status:doing`, `type:drawing`); `path:Folder/` matches the path prefix; `links:[[X]]` matches notes that link to X; `blocked` and `blocking` match the dependency state of BOARD-023; every other word is text. | SHOULD BE | `search_all/query.rs` (planned) | — |
| QUERY-003 | **Dates** (#597). `due:` takes a date (`due:2026-12-01`), a range relative to today (`due:<+7d`, `due:>-3d`) or `due:overdue`. A bad date is a syntax error (QUERY-008). | SHOULD BE | `search_all/query.rs` (planned) | — |
| QUERY-004 | **Text** (#597). A word that is no field is text matched with the wildcards `*` and `?` of NAV-023. In a board lane the text and the other terms match a note as a whole, not a line. | SHOULD BE | `search_all/query.rs` (planned) | — |
| QUERY-005 | **The prefixes of NAV-050 keep working** (#597). `f:`, `m:` and `s:` at the start of the query choose the sources as before; the rest is read as this language. A board lane searches notes only, so a prefix there is a syntax error. | SHOULD BE | `search_all/query.rs` (planned) | — |
| QUERY-006 | **A source says which fields it supports** (#597). Each source (notes, mail, posts) declares its fields. A query with a field a source lacks skips that source and says so ("Mail: `status:` not supported"); it is not shown as an empty result. | SHOULD BE | `search_all.rs` (planned) | — |

## Validator and budget

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| QUERY-007 | **Parsed and checked first** (#597). Every query is parsed into a tree and checked before anything runs. | SHOULD BE | `search_all/query.rs` (planned) | — |
| QUERY-008 | **Syntax errors stop the search** (#597). Unbalanced parentheses, a dangling operator, an unknown field and a bad date or priority value are errors. The error is underlined in place with a short reason, nothing runs, and the last valid result stays shown. | SHOULD BE | `search_all/query.rs`, `picker.rs` (planned) | — |
| QUERY-009 | **Limits** (#597; these are the spec's numbers). A query has at most 500 characters, 32 terms and nesting depth 8; a text pattern has at most 8 `*`. A query over a limit is a syntax error (QUERY-008). | SHOULD BE | `search_all/query.rs` (planned) | — |
| QUERY-010 | **A work budget per search** (#597). A search counts the characters it compares and has a time limit (the numbers are constants of `search_all`). When one is used up the search ends and the result is partial, marked "stopped early, narrow the query". The reason: `matches_at` retries at every start position, so `*a*a*a*…b` costs time quadratic in the line. | SHOULD BE | `search_all.rs` (planned) | — |
| QUERY-011 | **A property test** (#597, DEV-042). For every input, the checker rejects it or the search ends within the budget (QUERY-010), and nothing panics. | SHOULD BE | `search_all/query.rs` (planned) | — |
| QUERY-012 | **A bad lane query stays in its lane** (#597, #601). A board lane whose saved query fails the check (QUERY-008) shows the error in its header and is empty; the rest of the board works. | SHOULD BE | `board/` (planned) | — |
| QUERY-013 | **Checked once per text** (#597). A query is parsed and checked when its text changes, not every frame; the result is kept with the text. | SHOULD BE | `search_all/query.rs` (planned) | — |

## The background worker

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| QUERY-014 | **One worker per window** (#598). `search_all::all_hits` no longer runs on the UI thread (`picker.rs`). One thread per window, started once and not per keystroke, receives queries and returns hits over a channel. Plain threads in the style of `jobs::spawn`, no async runtime. | SHOULD BE | `search_all/worker.rs` (planned) | — |
| QUERY-015 | **A snapshot copies pointers** (#598). The vault keeps each note as `Arc<NoteIndex>`. A query takes a snapshot that copies only the pointers. An edit replaces the note's `Arc`; the worker keeps reading its snapshot. | SHOULD BE | `vault.rs` (planned) | — |
| QUERY-016 | **Its own read-only connections** (#598). The worker opens its own read-only SQLite connections for the mail and post indexes. | SHOULD BE | `search_all/worker.rs` (planned) | — |
| QUERY-017 | **Numbered queries** (#598). Every query has a number. A newer query sets a cancel flag that the running search checks per line. A result whose number is not the latest is dropped. | SHOULD BE | `search_all/worker.rs` (planned) | — |
| QUERY-018 | **The window keeps drawing** (#598). While a search runs, the last valid list stays with a small spinner and the window keeps drawing; typing is never blocked (NAV-024 keeps its limits and debounce). | SHOULD BE | `picker.rs`, `search_all.rs` (planned) | — |
| QUERY-019 | **Lanes use the worker** (#598, #601). Board lanes search through the same worker. A vault change re-runs only the lanes whose result can change. | SHOULD BE | `board/` (planned) | — |
| QUERY-020 | **Headless tests wait for it** (#598, DEV-041). `Headless::settle` waits for the worker, so a test sees the result of a search it started. | SHOULD BE | `headless.rs` (planned) | — |

## The filter and sort form

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| QUERY-021 | **Filter rows** (#599). The form has filter rows: tag is, property is, due within, is blocked, in folder, text contains. Each has a "not" switch. | SHOULD BE | `search_form.rs` (planned) | — |
| QUERY-022 | **All of, Any of** (#599). The rows are combined by **All of** (AND) or **Any of** (OR). | SHOULD BE | `search_form.rs` (planned) | — |
| QUERY-023 | **Sort rows** (#599). A sort row is a field and asc or desc. Rows can be added, reordered by drag and removed. The field `dependencies` (BOARD-026) has no direction. | SHOULD BE | `search_form.rs` (planned) | — |
| QUERY-024 | **Advanced** (#599). An **Advanced** toggle swaps the form for the text field, with inline errors (QUERY-008) and a live count of matches. Form and text edit the same text. | SHOULD BE | `search_form.rs` (planned) | — |
| QUERY-025 | **Form to text always, text to form when it fits** (#599). The form always writes its text. The text opens in the form only when it fits (one level of AND or OR). Otherwise the form shows the query read-only with "This query is too complex for the form" and Advanced stays on. Nothing is simplified or dropped. | SHOULD BE | `search_form.rs` (planned) | — |
| QUERY-026 | **The last choice is remembered** (#599). The last Advanced choice is a setting (a new field with a default, no version bump, CFG-001). | SHOULD BE | `config.rs` (planned) | — |
| QUERY-027 | **In the picker behind a ⚙ button** (#599). The picker window stays one text box; the form is behind a ⚙ button. | SHOULD BE | `picker.rs` (planned) | — |
| QUERY-028 | **In a board, the lane's settings popover** (#599, #601). The form is the popover of a lane (the ⋯ button of its header or a double-click on its heading). It also shows the lane's `drop:` property (BOARD-019), filled from the filter and editable. | SHOULD BE | `board/` (planned) | — |
