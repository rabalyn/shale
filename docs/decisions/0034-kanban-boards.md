# ADR-0034: Kanban boards, one query language and a background search

- **Status**: Accepted (2026-11-23)
- **Date**: 2026-11-23
- **Specs**: BOARD-001…BOARD-032 ([boards.md](../spec/boards.md)), QUERY-001…QUERY-028 ([search-queries.md](../spec/search-queries.md)), VAULT-044…VAULT-046 (note properties)
- **Issues**: rabalyn/shale#596 (epic), #597, #598, #599, #600, #601, #602, #603, #604, #605

## Context

The owner wants a Kanban board: lanes that fill themselves from a query (`status:doing`, `#todo`),
cards that open in a small editor, drag between lanes, dependencies between cards, and a PDF of
the board with its notes. Three things stand in the way.

- **No shared query language.** The search over everything (NAV-022, NAV-007) takes text with
  wildcards (NAV-023) and source prefixes (NAV-050). It has no tags, no properties, no dates, no
  `AND` / `OR` / `NOT`. A board lane needs all of them, and the picker and the sidebar should
  speak the same language as the board.
- **Search runs on the UI thread.** `search_all::all_hits` is called from `picker.rs`. NAV-024
  keeps it short with limits and a debounce, but a board with six lanes would run six searches
  per vault change on the frame thread. Also `matches_at` in `src/search_all.rs` does not
  backtrack exponentially, but it retries at every start position, so a pattern like `*a*a*a*b`
  costs time quadratic in the line. A saved lane query is read on every start, so a bad query
  must not be able to hang a window.
- **Notes carry no dates or priority.** Sorting by created, modified or priority needs values
  that Shale writes and keeps.

## Decision

1. **A board is a note.** A Markdown note with `type: board` in its front matter. It stays
   searchable, linkable and readable in other apps (BOARD-001). Lanes live in the body, not in
   the front matter, because `markdown::front_matter_values` reads only flat `key: value` lines:
   a lane is a `##` heading, with `query:`, `sort:`, `drop:` and `done:` lines under it, and a
   list of `- [[links]]` for pinned cards (BOARD-002…BOARD-005). Changing lanes in the view
   rewrites the file; editing it by hand works too (BOARD-006).
2. **One query language** for boards, the sidebar search and the picker: `AND`, `OR`, `NOT` in
   upper case only, parentheses, `#tag`, `prop:value`, `path:`, `due:`, `links:`, `blocked`,
   `blocking`, and text with the wildcards of NAV-023 (QUERY-001…QUERY-006). A lane matches
   notes as a whole, not lines. The prefixes of NAV-050 keep working.
3. **A validator in front of every search.** A query is parsed into a tree and checked before
   anything runs, against fixed limits (QUERY-007…QUERY-013). A search also has a work budget and
   ends early with a partial result, marked as such. A property test (DEV-042) checks that every
   input is rejected or ends within the budget and never panics.
4. **One background worker per window** (QUERY-014…QUERY-020), plain threads in the style of
   `jobs::spawn`, no async runtime. The vault holds each note as `Arc<NoteIndex>` so a snapshot
   copies pointers only. Every query has a number; a newer one cancels the older; a result that
   is not the latest is dropped. Board lanes use the same worker.
5. **One filter and sort form** for the lane settings and the picker (QUERY-021…QUERY-028),
   with an **Advanced** switch to the text. The form never simplifies a query: when a query does
   not fit the form it is shown read-only.
6. **Note properties** `created:`, `modified:` and `priority:` (VAULT-044…VAULT-046). `modified:`
   changes only when the body changes.
7. **The board view** is a main-area tab like Graph and Calendar (BOARD-007…BOARD-014). The
   peek is the floating frame of the link preview (EDIT-033, EDIT-037) with a real editor in it.
8. **Drag** writes a `drop:` property into the card's note, without touching `modified:`
   (BOARD-019…BOARD-022). **Dependencies** are `blocked-by:`, `due:` and `start:` in the card's
   front matter; `sort: dependencies` is a topological order (BOARD-023…BOARD-029).
9. **PDF**: the built-in engine writes the board as landscape pages, then the notes of the cards
   in portrait through the book export; the LaTeX engine does the same through the book templates
   (BOARD-030, BOARD-031).

### Alternatives considered

- **TOML or JSON for the board.** No gain, and the board would stop being a note: not
  searchable, not linkable, not readable in another app. Refused.
- **Lanes in the front matter.** The reader of `front_matter_values` handles flat keys only;
  nested lanes need a new YAML reader. Refused for the body format, which any Markdown app shows.
- **Keep searching on the UI thread and only add limits.** The limits stay (they are the
  validator), but six lanes per vault change on the frame thread would still cost frames.
- **A regex or a PCRE-like language.** A wider language is a wider attack on the budget. The
  wildcards of NAV-023 are enough for text.
- **Lower-case `and` / `or` as operators.** "salt and pepper" is a text search; only upper case
  is an operator.
- **An async runtime for the worker.** One thread with a channel is enough and matches `jobs.rs`.
- **Simplify a query the form cannot show.** That would change what a lane shows without the
  owner noticing. The form shows it read-only instead.

## Consequences

- `search_all` gets a parser, a checker and a budget; the picker and the sidebar change from a
  synchronous call to a request and a result that arrives later (a spinner, the last list stays).
- The vault changes its note store to `Arc<NoteIndex>`; an edit replaces one pointer.
- Notes written by Shale get `created:` and `modified:` lines in their front matter. Notes
  edited by another editor are noticed only at the next save from Shale (VAULT-045).
- A drag changes a card's note on disk (its front matter), so a board in a git-synced vault
  produces diffs in the cards. That is the price of the notes being the data.
- A drawing can be matched only by name, path and links until drawings get tags.
- **Out of scope, a possible follow-up**: due dates and deadlines of cards in the Calendar. No
  spec row.
