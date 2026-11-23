# ADR-0035: A windowed editor for very long notes: stitched block layouts first, a window over the text only if that is not enough

- **Status**: Proposed (the owner accepts it; the open questions at the end are his)
- **Date**: 2026-11-23
- **Specs**: EDIT-099, EDIT-104 to EDIT-109 (what exists today); no new rows until the owner accepts a step
- **Issues**: #399 (this decision), #375 (block-incremental formatting), #343 (the benchmark harness)
- **Amends**: nothing

## Context

Issue #399 asks what it takes to lay out and paint only the visible blocks of a very long note,
after #375 made the *format* of the text block by block (EDIT-109). The owner decided on
2026-11-22 to write this ADR before any code, and to decide a windowed editor only if the cost left
after #375 is not good enough.

### What a key press costs today (measured, v0.7.6, `docs/perf/results/v0.7.6.json`)

The generated long note of the harness has about 20 000 lines (`Generated::long_note`).

| Scenario | Median | Allocations |
|---|---|---|
| `ui.frame.editor.long` (a frame with nothing typed) | 2.3 ms | 9 936 |
| `ui.scroll.editor.long` (one wheel step) | 2.4 ms | 10 060 |
| `ui.type.editor.long` (one `x` in the middle) | 23.4 ms | 145 954 (72 MB) |

EDIT-099/EDIT-109 name the parts: the format of the text 1.5 ms, "egui laying out the one galley"
about 8 ms. The rest of the 23 ms (about 14 ms) is **not yet split by anyone**. Reading the code
and the egui 0.37 sources it is made of more than the layout, all of it growing with the note:

- **egui's layout of the job.** `Fonts::layout_job` already caches each paragraph on its own
  (`GalleyCache`, `allow_split_paragraphs`), so the shaping of unchanged text is not repeated. What
  remains is O(note): hashing the whole `LayoutJob`, cutting it into one job per paragraph (text
  and sections copied), hashing each, and merging the paragraph galleys into one.
- **`TextEdit` copies the text.** `egui::TextEdit::show` takes `text.to_owned()` for `prev_text` and
  feeds the text to its own undoer twice per frame (`widgets/text_edit/builder.rs`, lines 556,
  1215, 1519 of egui 0.37), in the frames where it handles input. That is three copies of the whole
  note, and an `Undoer` that keeps whole-text snapshots.
- **Shale's own whole-note work**: `FoldUi::begin` (1 to 3 ms after an edit, EDIT-099), the number
  and formula marks, `GalleyIndex::new` for the new galley (`galley_index.rs`), the comparison
  that finds the cached layout.

### How the editor is built (what a change has to live with)

- **One `egui::TextEdit::multiline` over the whole `String`** (`editor.rs`, `NoteEditor::show`,
  about line 1872) with a `.layouter()` that returns one `Arc<Galley>` for the whole note, cached
  as `Laid` in `LayoutCache` (`editor_cache.rs`, EDIT-105) under a `LayoutKey` of the text, links,
  fonts, wrap width, raw lines and the rows that embeds, tables and folds change.
- **The note scrolls in `layout.rs`** (`egui::ScrollArea::vertical`, `("note-scroll", tab id)`,
  about line 964). The title, the banners and the toolbar are inside that scroll area; the editor
  is one tall widget in it. `TextEdit` itself does not scroll.
- **Everything is addressed by a char or byte offset into the one text and by a position in the one
  galley.** `editor.rs` (5 700 lines) holds 66 uses of the galley or `last_layout`; the painters
  `paint_numbers`, `paint_math`, `paint_page_breaks`, `task_box::paint_in_galley`, the inline
  pictures (`images`, `remap_images`), the cursor shapes (EDIT-039), `fold_ui.rs` (hidden rows,
  badges), `table_grid.rs` (tables as grids over the text), `galley_index.rs` and the link
  preview all take the galley and the text, and all turn an offset into a rectangle by
  `galley.pos_from_cursor`.
- **The cursor and the selection live in egui's `TextEditState`** (`TextEditState::load(ctx, id)`,
  more than 30 places in `editor.rs`), as char ranges. `vim.rs` (`Vim`, 960 lines, no UI) works on
  the text and a char cursor; `NoteEditor::vim_apply_cursor` copies its cursor into the state and
  `vim_scroll` scrolls to it.
- **Two undo mechanisms exist.** Typing in the box is undone by egui's own `Undoer` inside
  `TextEditState` (whole-text snapshots; `record_edit`, `editor.rs` ~1413, adds program changes to
  it); vim-lite and the files of EDIT-022 use `undo::History`, which keeps small edits
  (`Edit { at, removed, inserted }`).
- **A precedent for windows exists**: `virtual_list.rs` (`RowHeights`, PERF-001) draws only the
  rows in view and keeps the scroll bar's length with measured and estimated heights; the feed view
  (PERF-006) keeps a scroll anchor.
- **Jumps** (outline, links, vault search hits) are `pending_cursor: Option<usize>`
  (`editor.rs`, set from `app.rs`), which `show` turns into `ui.scroll_to_rect` after the galley
  is known. There is **no find-in-note bar** today (none in `editor.rs` or `layout.rs`); search is
  the vault search, which opens the note at the hit.

## Decision (proposed)

Do it in steps (0 to 3) that each pay on their own and each have a gate, and **stop at the first step
whose gate meets the target**. Steps 1 and 2 keep one `TextEdit` over the whole text; only step 3
is a windowed editor, and it is decided only if the numbers after step 2 say it is needed.

1. **Measure first (step 0, no behaviour change).** Split the 23 ms with `scripts/bench.sh
   --flamegraph --filter ui.type.editor.long` and write the split into EDIT-099. Add scenarios to
   the harness that the decision depends on: the same note at 5 000, 20 000 and 100 000 lines
   (`ui.type.editor.long`, `.xl`, `.xxl`), with a narrow wrap width (`ui.type.editor.long.narrow`:
   more wrapped rows), a jump to the last line (`ui.jump.editor.long`), a select-all
   (`ui.select_all.editor.long`) and a scrollbar drag (`ui.scroll.editor.long.drag`: ten wheel
   steps of 1 000 points). They are the gates below; nothing else may be called "better".
2. **Stitched block layouts (step 1).** Lay out each block of EDIT-109 (`format::cuts`) as its own
   `LayoutJob`, keep each block's `Arc<Galley>` and its height in the block cache under the key it
   already has, and build the note's `Galley` by **stitching** the blocks' rows (`Galley` and
   `PlacedRow` have public fields and rows are `Arc<Row>` in epaint 0.37) with the y offsets and the
   whole `job` text that `TextEdit` and the painters expect. A block is a run of whole paragraphs
   cut at a blank line, so its rows are the rows the whole job gives (to be proven by a test that
   compares both galleys, glyph for glyph, on the test notes). A typed key lays out one block (about
   0.1 ms) and stitches (O(rows) pointer copies) instead of hashing and cutting the whole job. No
   widget, no offset, no painter changes. This removes the 8 ms of egui's layout and keeps every
   whole-note operation working as today.
3. **Take the O(note) work out of the frame (step 2).** What step 0 finds and is not the layout:
   the three copies in `TextEdit` and its snapshot `Undoer`, `FoldUi::begin`, `GalleyIndex::new`,
   the cache comparison. The candidates: move typing undo to `undo::History` (edit-based, no
   snapshots; needs the owner's answer to question 3, and removes the use of egui's `Undoer` in
   `record_edit`), make `GalleyIndex` incremental per block, and give the fold frame a per-block
   cache. Each is its own change with its own scenario gate.
4. **A window over the text (step 3, only if the gates are still missed).** `TextEdit` gets a
   **slice** of the note (the blocks in view plus a margin, whole blocks only) and Shale maps
   offsets between the slice and the note; the blocks above and below are an empty space of their
   cached or estimated height, as in `virtual_list.rs`. Described below; the **own text widget is
   rejected** (option B), unless step 3 turns out to be impossible for a reason found while doing
   it, in which case the owner decides again.

### The target

Proposed gates, to be set by the owner (question 1), all on the owner's workstation (ADR-0031), in
the `quick` profile of the harness, release build:

| Scenario | Today | After step 1 | After step 2 | After step 3 |
|---|---|---|---|---|
| `ui.type.editor.long` (20 000 lines) | 23.4 ms | at most 12 ms | at most 6 ms | at most 4 ms |
| the same at 100 000 lines | not measured | within 1.5 x of the 20 000 | within 1.5 x | within 1.2 x (flat) |
| `ui.frame.editor.long` / `ui.scroll.editor.long` | 2.3 / 2.4 ms | not worse | not worse | at most 1.5 ms |
| peak bytes of `ui.type.editor.long` | 22 MB | not worse | at most 12 MB | at most 8 MB |

The numbers after step 1 and 2 are estimates from the split above, not measurements; step 0 replaces
them by measured ones before the owner signs them.

## The options and what they cost

### A. Window the text: `TextEdit` over a slice (step 3 of the decision)

`TextEdit::multiline(&mut slice)` where `slice` is a `String` made of the blocks of the window.
Everything `TextEdit` does (cursor, selection, IME, accessibility, clipboard, word movement,
multi-click) stays egui's. Costs:

- **A mapping layer** `Window { first_block, byte_base, char_base, top_space, bottom_space }` and
  one rule: *the note is the master, the slice is a view*. Edits are written back (`replace_range`
  on the note, by the changed range only, not the whole slice, to keep the cost O(window)). Every
  consumer that takes the galley and the text (the 66 uses) takes the window's text or an offset
  base: `last_layout`, `pending_cursor`, `vim_cursor_rect`, `selection_bytes`, `restore_selection_chars`,
  `fold.at(text)`, `remap_images`, `paint_*`. This is mechanical but wide; a wrapper `View` with
  `to_note(char)` / `to_slice(char)` and no raw `galley.job.text` outside it is the way to make
  it reviewable and to keep it from rotting.
- **The window moves while the cursor stays**: when the window shifts, the stored
  `TextEditState` cursor range is rewritten with the new base in the same frame, before
  `TextEdit` runs. The block with the cursor (and the anchor of a selection) is **pinned** into
  the window, so a long scroll away from the cursor does not drop it.
- **Typing undo** by egui's `Undoer` holds slice snapshots and breaks when the window moves; so step
  3 requires step 2's move of typing undo to `undo::History` on the master text.
- **Height of the unseen**: the space above the window is the sum of the heights of the blocks
  above (measured when laid out once, else estimated). A block measured later moves everything
  after it; anchor the scroll to the first visible block so the text in view does not jump
  (the way PERF-006 anchors the feed). The estimate of a block is its wrapped row count (chars
  times the average glyph width of the fonts over the wrap width, per line) times the row height,
  with the heading scale (`heading_scale`) and fixed heights for the ranges the embeds, tables and
  folds already know (`LayoutKey::ranges`); calibrate the average glyph width from the blocks
  measured so far. A width change or a font change invalidates every measured height (the
  `LayoutKey` already contains the wrap width and the fonts); the old heights, scaled by the ratio of
  the widths, stay as the estimate so the bar does not jump to the default.
- **Block boundaries** are the cuts of EDIT-109 (`format::cuts`): never inside the front matter, a
  code fence, a multi-line formula, a link, or before a `---` line. The window needs more:
  **a table is never cut** (`table_grid.rs` edits it as a grid over its text), a folded block
  with its hidden lines is one unit (`fold.rs`), and a list or a quote that a fold can hide is
  not cut at its folds. A block of the cut kind that is huge (a code fence of 5 000 lines) is
  laid out as a whole: the window cannot be smaller than one block; the limit is stated, not
  hidden (question 5).
- **Everything outside the window is untouched text**: saving, the vault index, search, export,
  the status bar counts (EDIT-107) and the link parse work on the full `String` as today. The
  slice is a copy for the box only.

### B. An own text widget (rejected)

A widget that holds cursor, selection and the per-block galleys itself. It would remove every
`TextEdit` limit (selections over any length, no slice copy, own undo). It costs: the cursor and
selection model across block galleys (up and down by x position over a block boundary, word and
line movement, double and triple click, drag selection with autoscroll), IME composition and
pre-edit text, the accessibility tree (egui's `TextEdit` feeds AccessKit), the platform clipboard
and primary selection, the key map of text editing per platform (Alt, Ctrl, Cmd, Home, End,
Page keys), the context menu, and every one of the 30 places that now read `TextEditState`
(and the tests behind them). egui's `text_selection` helpers cover the cursor over one galley,
not over many. It is months, not weeks, with the highest risk of regressions in the base of the
app (typing), for a gain that A gives for the case that matters: a note too long for the box. Only
if A fails (for example IME or accessibility cannot be kept with a slice) is B back on the table.

### C. Stitched layouts without a window (step 1 and 2 of the decision)

Described above. It is the cheapest and removes the *layout* floor of #399, not the O(note) work of
`TextEdit` (the copies, the snapshots), the painters (`tessellator` culls rows outside the clip, so
the paint is already O(visible) — to be confirmed by step 0) and the number of rows `pos_from_cursor`
searches. Its limit is a note so long that the copies alone matter (hundreds of thousands of lines);
that is where the window is the only fix.

### D. Do nothing

23 ms for a key in a 20 000-line note is a frame at 43 fps: noticeable, not broken. A note that long
is rare and the owner's own large-note case (EDIT-099). The cost of D is that every future feature
that goes over the whole text adds to the 23 ms.

## Whole-note operations in a window (step 3)

| Operation | Rule |
|---|---|
| **Select all** (Ctrl+A) | The window **grows to the whole note** while a selection spans more than the window (the slice is the selection plus the margin). It is the rare case: the cost is today's cost for that frame (cheaper after step 1) and ends when the selection ends. Copy, cut and typing over it then work as now. Alternative: a virtual selection kept by Shale in note coordinates (question 4); more code, no growth of the window. |
| **Copy / cut across the window** | Same rule: a selection that does not fit makes the window follow the selection. Copy of what is in the window is the slice's text. |
| **Drag selection with autoscroll** | The anchor block is pinned; the window grows toward the pointer as it scrolls. |
| **Typing undo / redo** | `undo::History` on the note (needs step 2). The cursor after an undo comes from the history (`History::undo` returns it) and is a jump, below. |
| **Vim motions** | `vim.rs` works on the whole text and a char cursor and does not care about the window. `G`, `gg`, `/`-like searches and `%` set `vim.cursor` far from the window: `vim_scroll` becomes a jump. Vim keeps the **master text** and its cursor; the slice only shows. A motion that needs the layout (`j` / `k` over wrapped rows, `H` `M` `L`) uses the rows of the window and, when the target row is outside it, moves by logical lines (the same as for a note that is long today, to be checked in step 3). |
| **Jumps** (outline, links, search hits, `pending_cursor`, `vim_scroll`, the cursor after undo) | The target char is mapped to its block by its byte position (a binary search in the block table), the window is moved there **before** the layout of the frame, and the scroll is set to the block's top (measured or estimated), then corrected in the next frame once the block is measured (the anchor rule above). A jump therefore takes two frames to be exact. |
| **Find / replace in the note** | There is none today. When it comes it works on the master text and jumps like the search hits; replace-all is one `String` operation and one undo step. |
| **Fold commands** | The fold engine already works on the whole text (`fold.rs`, `fold_ui.rs`); folding or unfolding a block changes the height of its block: the block's height is invalidated and the anchor rule keeps the view still. |
| **The cursor leaves the window** (arrow keys, Page Down) | The cursor block is pinned; at the slice edge the window moves by a block first, so `TextEdit` never meets the end of the slice while the note goes on. |
| **Saving, the index, search, export, counts** | Unchanged: they take the full `String`. |
| **Tables, embeds, pictures, math** | Painted from the same marks as today, restricted to the window; the marks are found per block (the ranges of `LayoutKey::ranges` and the `MathMark` / `NumberMark` lists are already made per block in `FormatCache`). |

## Consequences

- Steps 0 to 2 change no behaviour and no spec row; they change numbers (EDIT-099, EDIT-109 get the
  new timings) and may change how typing undo is stored (question 3).
- Step 3 is the large one: the window layer, the offset wrapper in every galley consumer, the
  estimated heights and anchoring, the typing-undo move. It is a feature branch of its own
  (several issues), and it brings new tests: a differential test of the window against the plain
  editor (the same keys, the same note, the same final text and cursor), property tests of the
  mapping, and the large-note scenarios as gates.
- Two states of the editor (window or whole) must both be kept working and tested; below a
  threshold (proposed: 3 000 lines, question 2) the editor is exactly today's, so a normal note
  runs no new code.
- The scroll bar of a long note is approximate until its blocks have been measured once: a small
  jitter of the thumb, which is the price named in #399.
- The behaviour the owner sees changes in three places only: select all in a very long note
  takes longer than a key press, a jump into an unmeasured part scrolls with a small correction,
  and a block longer than the window is laid out whole.

## Open questions for the owner (not decided)

1. **The targets.** Are 6 ms (step 2) and 4 ms (step 3) for a key in a 20 000-line note right, and
   is "flat from 20 000 to 100 000 lines" a goal, or only the 20 000-line case? Is the 23 ms of
   today already good enough, so that only step 0 and 1 are done?
2. **The threshold.** Window only above N lines (proposed 3 000), or always for every note (one code
   path, less tested for small notes)?
3. **Typing undo on `undo::History`.** Step 3 (and the copies of step 2) want the snapshot
   `Undoer` of egui out of the way. It changes how Ctrl+Z groups typing (today egui's timing
   merge; then `History` checkpoints). Is it acceptable that undo steps of typing change
   slightly, with the same tests (`EDIT-021`, `EDIT-022`) kept as they are?
4. **Select all in a long note**: the window grows to the whole note (simple, slow for that frame)
   or Shale keeps its own virtual selection (more code, always fast)?
5. **A block longer than the window** (a code fence or a table of thousands of lines): laid out whole
   (simple, slow for that note) or split at lines inside the block where it is safe (code fences:
   yes; tables: no)?
6. **Estimated heights and the scroll bar**: is a thumb that moves a little while scrolling through
   unmeasured text acceptable, or must the first open of a long note measure every block once (a
   background pass of the layout, about the cost of today's one-time layout, spread over frames)?
7. **Is step 1 (stitched block layouts) approved on its own** even if the window is never built? It
   is useful without it, but it puts a Shale-made `Galley` in front of egui's `TextEdit`, which
   depends on `Galley`'s public fields in egui 0.37 and must be re-checked at each egui upgrade
   (a test that compares the stitched and the plain galley does that).
8. **Accessibility and IME** in a window: a screen reader would read the slice. Is that acceptable for
   notes above the threshold, or must the windowed editor wait until AccessKit can be given the whole
   note?

## Alternatives not taken

- **An own text widget** (B above): too large and too risky for the base of typing.
- **Cut the long note into several `TextEdit`s, one per block**: simple, but a selection, a
  cursor movement or an IME composition cannot cross widgets, and every key at a block edge becomes a
  focus change; the same problems as B with less of the control.
- **Layout on a thread**: the galley must be there in the frame that types the key, or the text lags
  behind the key.
- **Skipping the layout for text outside the clip rect, inside one `TextEdit`**: egui lays the whole
  job out before it knows the clip; the culling the tessellator does is after the work.
