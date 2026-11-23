---
features:
  - "Tabs"
  - "Panes side by side"
---
![[tab-pinned-menu.png]]

- Drag a tab left or right to reorder it; a marker shows where it will land. A click without
  moving just selects the tab; the ✕ or a middle click closes it.
- Right-click a tab and choose **Pin tab** (or **Unpin tab**): pinned tabs move to the far
  left, show a small pin and keep their order. A pinned tab is never replaced: whatever you open while it is the open tab
  (an import, a link, a search result, the quick switcher, a file in the tree, back and forward) opens in a
  new tab, and the pinned tab keeps its note. Pinned tabs can still be closed.
- Ctrl+PageUp / Ctrl+PageDown switch to the previous / next tab, after the last comes the first (on macOS Cmd+Opt+← / Cmd+Opt+→, because Cmd+Opt+H would hide the other apps).
- Right-click a tab and choose **Open in split right**, or press Ctrl+\, to open it in a new
  **pane** to the right: the middle area can hold any number of panes side by side, each with its
  own tab bar, tabs and back / forward history.
- The pane with the focus has an accent line under its tab bar. Click into a pane, or press
  F6 / Shift+F6 to move the focus. Opening a note or link, the quick switcher, new
  notes, closing tabs, previous / next tab, back / forward and the reading view all act on the
  focused pane; renaming or moving a file updates the tabs of every pane.
- Ctrl+H / Ctrl+L (macOS: Cmd+Shift+H / Cmd+Shift+L) walk through the panes in screen order, like vim's window moves: Ctrl+H goes
  from a pane into the pane on its left and from the first pane into the left sidebar, Ctrl+L
  from the sidebar into the first pane and on to the right; at the last pane it stays. The
  keyboard lands in the editor of the pane's open note.
- Drag the border between two panes to change their widths (a pane is never narrower than 240
  points). Closing the last tab of a pane, or Ctrl+Shift+W (macOS: Cmd+Shift+W), closes the pane; the last pane stays.
  After closing a tab or a pane the keyboard goes into the editor of the open note of the focused
  pane, so no key is lost.
- The panes and the order of all open tabs (notes, drawings, Feeds, Mail, Calendar, Tasks, Graph and Settings-file tabs; an empty tab is not kept) and which are pinned, the open tab of each pane, the focused pane and the widths come back at
  the next start (they are kept in `.shale/state.json`, see [[Vault settings]]).
- Each note also comes back as you left it: the cursor or selection, how far it was scrolled and whether it was in the
  reading view, in every pane and also for tabs that were not the open one (they get their place when
  you switch to them). A Mail tab comes back on the folder and the message you had open (the default folder if it is gone), a Calendar tab on the mode and the day you looked at, a Tasks tab with its filter and a Graph tab with its switches. Nothing is asked when they come back: the mail view opens the way it does when you open it yourself. If the note got shorter in the meantime, the cursor goes to its end. Shale writes this every few
  seconds while you work and when it closes, so even a crash loses at most a few seconds.

Back to [[Start here]]; Feeds tabs are in [[Feeds]].

#userguide #tabs
