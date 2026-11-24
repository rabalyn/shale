# ADR-0033: Files, Search and Tags on either side: move or copy

- **Status**: Accepted (2026-11-23), amended by [0038](0038-main-views-as-sidebar-companions.md)
- **Date**: 2026-11-23
- **Specs**: LAY-020…LAY-027
- **Issues**: rabalyn/shale#573
- **Amends**: [ADR-0014](0014-sidebar-layout.md) (decision 2, the refusal of ribbon items on the right, and decision 3, "an icon exists once")

## Context

ADR-0014 lets only the panel icons (Backlinks, Outline, …) live in the right sidebar. The views
of the **left** sidebar (Files, Search, Tags) stay there. The owner wants to place any view on
either side: the file list beside the outline, Search on the right, and back. On release of a
dragged view Shale asks whether to **move** or **copy** it (copy: the same view on both sides).

What the left sidebar holds today is exactly Files, Search and Tags (one at a time, chosen by the
ribbon icons). Mail, Calendar, Graph, Tasks and Feeds are not panes of the sidebar but views of
the main area (tabs), and Quick switcher, Import, New drawing and New note are commands; none of
them is a pane that could be drawn in a sidebar.

## Decision

1. **Movable views**: Files, Search and Tags. Each can be in the ribbon (the left sidebar shows
   it when its icon is clicked), in a view of the right sidebar, or in both. The other ribbon
   items (main views and commands) are still refused on the right (ADR-0014, decision 2).
2. **Move or copy.** A drag of a movable view from one side to the other (the ribbon to a right
   menubar or to the content of the right sidebar; a right menubar to the ribbon) does not
   change anything on release; Shale asks "Move or copy?" with **Move**, **Copy** and
   **Cancel** (Esc cancels). Move takes the icon from the side it came from; copy keeps it there.
   A drag within one side only reorders (no question). Dragging a copy to the other side moves it
   there: nothing is added when it is already there, it only goes from the source side (a copy is
   removed that way). Panel icons (Backlinks, …) have one place and are only moved, as before.
3. **A non-drag path**: right-click a movable icon. In the ribbon the menu has **Move to the
   right sidebar** and **Copy to the right sidebar** (the end of the last view; a new view when
   there is none); in a right menubar **Move to the left sidebar** and **Copy to the left
   sidebar** (the end of the ribbon); the entry that would change nothing is not offered. This
   is the keyboard- and test-friendly way (the context menu opens with the menu key).
4. **A view that is shown twice has one state.** The file tree (expanded folders, selection,
   scroll to the open note), the search box and its results and the tag list are fields of the
   window, not of a place; both copies draw the same state, so typing in one search box shows in
   the other and a folder opened in one tree is open in the other. Only the scroll position of
   the scroll area is per place. Alternative considered: independent state per copy; refused as
   it needs a second tree/search state, more memory and a second background search, for little
   gain.
5. **The left sidebar without its view.** When the view the left sidebar was showing has moved to
   the right, it shows the first of Files, Search and Tags that is still in the ribbon; with none
   it says so ("No view is here …"). The keyboard shortcuts for Files and Search (Ctrl+Shift+F …)
   show the view on the right when it is only there.
6. **Stored in the same `sidebars`** (ADR-0005, ADR-0014 decision 5): the ids `files`, `search`
   and `tags` may also be in `sidebars.right[].icons`. No version number is needed. Loading
   repairs per side: a double inside the ribbon or inside the views goes, but one id in the
   ribbon and in a view is a copy and stays; an id in no list and not hidden comes back to the
   ribbon; a movable view only in a view stays moved. An old layout (these ids in the ribbon
   only) loads exactly as before. An older Shale reading a new file drops the ids from the views
   (they are no panels there) and puts them back in the ribbon: nothing breaks.
7. **Removing**: the right-click entry "Remove from the ribbon" / "Remove from this view" takes
   the icon from that side only; it is hidden (and listed under "Restore …") when it is in no
   menubar any more. "Restore" puts a movable view back in the ribbon.

## Consequences

- "An icon exists once" holds for every icon except these three, which can be on two sides.
- The model has two places per movable item, so `Layout::apply` needs the side the drag started
  at and the choice; the question is a state of the window (`SideState::ask`), not of the model.
- The left sidebar can be empty of views; its toggle still works and says what to do.
- Feeds, Mail and the Calendar cannot become sidebar panes without a new decision (they are tabs
  with their own state and size needs); the drop refuses them with the no-drop cue. (ADR-0038 is
  that decision: they become compact companions with a state of their own, Mail first.)
