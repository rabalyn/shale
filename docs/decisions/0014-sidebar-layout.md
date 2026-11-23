# ADR-0014: The sidebar layout: one list of icons per menubar, views stacked on the right

- **Status**: Accepted (2026-11-15), amended by [0032](0032-views-on-either-side.md)
- **Date**: 2026-11-15
- **Specs**: LAY-001…LAY-011
- **Issues**: Forgejo raba/shale#20, #21, #22

## Context

The ribbon (left menubar) had a fixed set of icons in a fixed order and the right sidebar showed
one panel at a time, chosen by a row of text tabs. The owner wants the ribbon icons in any order,
the right sidebar split into stacked views that show several panels at once, each with its own
menubar, and icons that move between menubars by drag and drop. The layout has to survive a
restart and must not break on an old or damaged settings file.

## Decision

1. **Every icon is an `Item`** with a stable string id (`files`, `mail`, `panel:outline`, …).
   Items are of two kinds. A *ribbon item* opens or does something: a left panel (Files, Search,
   Tags), a main view (Graph, Calendar, Mail, Tasks, Feeds) or a command (Quick switcher,
   Import, New drawing, New note). A *panel item* is something the right sidebar can show:
   Backlinks, Outgoing links, Outline, Local graph, Calendar (side).
2. **What an icon means depends on the menubar it is in.** In the ribbon a click does the
   item's action; a panel item there is a shortcut that shows that panel in the right sidebar
   (in a temporary view at the bottom of the stack that the same click closes again). In a right
   menubar a click shows the panel in that view. Ribbon items that are no panels cannot live
   in a right menubar: they are refused (no-drop cursor and a red cue; nothing changes).
3. **An icon exists once.** Dragging moves it; it is never copied. Each item is in the ribbon
   or in exactly one view.
4. **The right sidebar is a list of views** (top to bottom, equal height). A view is a menubar
   of panel items and the panel of its active icon. A view without icons does not exist: the
   last icon leaving a view removes it. A new view is made by dropping an icon on the content of
   the sidebar (#22). With no views the sidebar says so and still takes a drop.
5. **Stored in the global settings** (`sidebars` in `config.json`, ADR-0005): the ribbon as a list
   of ids and the right sidebar as a list of views, each `{icons, active}`. Loading repairs the
   list (unknown ids and doubles go, missing items come back: ribbon items at the end of the
   ribbon, panel items at the end of the last view); a stored value of the wrong shape, or an
   empty one, gives the default (one view holding all five panels). The model (`sidebar.rs`)
   has no drawing code, so these rules are tested without a window.

## Consequences

- New icons are added to the `Item` list with a new id and appear at the end of old layouts.
- Removing or renaming an id loses the icon from stored layouts silently; ids are therefore
  never renamed.
- The view heights are equal; resizing the split is not part of this decision (a follow-up).
- A panel can be in only one place, so two views never show the same panel and the panels
  keep their single state (the side calendar, the local graph depth).
