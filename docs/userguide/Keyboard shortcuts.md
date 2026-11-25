---
features:
  - "Keyboard"
  - "Keymap: every shortcut on the keyboard"
---
Back to [[Start here]]. Editing keys are in [[Editor]], the settings window is in [[Settings reference]].

![[keybindings.png]]

To find an action, press **Ctrl+F** (or **/**) on the Keybindings tab and type part of its name or of
its keys, such as `reading`, `ctrl+h` or `F6`; the list keeps the actions that fit, hides the
groups with none, and says "No action matches" when nothing does. **Esc** clears the field. Change, Add, Clear and Reset
work on the rows shown, **Reset all** puts every action back, also those the filter hides. The
search at the top of Settings finds an action too and pulses its row, see [[Settings reference]].

![[keybindings-filter.png]]

# The keyboard

The **Keymap** tab shows the same keys on a drawn keyboard: open it with the keyboard icon of the ribbon (after Feeds) or with
**Ctrl+Shift+Y** (Cmd+Shift+Y on a Mac), or in Settings → Keybindings with the **Keyboard** switch above the list. Pick a
system (Linux, Windows or macOS, yours first), an ANSI or an ISO keyboard and a layer: none, Shift, Ctrl (Cmd), Alt (Option),
Ctrl+Shift, Alt+Shift, Ctrl+Alt, or click the modifier keys of the drawing, or hold them to look at their layer for a moment.
The chips on a key are coloured by the kind of action; **a hatched red key is one the system keeps**
for itself, a chip with an outline is a key you changed, a paler chip is a fixed key (text editing, vim, drawing), `»` is the
first key of a sequence such as `Space f f`. The legend switches a kind of action off, the **Where** buttons a place.
Click a key to see all its actions on every layer, and **Change** to go to the row of an action in the list.

![[keymap-linux-ctrl.png]]

Ctrl is Cmd on macOS. No default key is one that the system reserves (the app menu's Cmd+H, AltGr on Windows, the workspace keys of the Linux desktops), so macOS has other keys for six actions, named in the rows below; a key you set yourself is never refused for that reason. The keys below are the defaults of the shortcuts that can be changed in **Settings → Keybindings** (Change replaces the keys of an action, Add gives it one more, Clear leaves it without a key)
(everywhere, file list, search results, mail view, note view, vim normal mode; a key that another action uses asks whether to overwrite it: Yes takes it from that action, No changes nothing).
Text editing keys and vim keys are fixed.

| Keys | Action |
|---|---|
| Ctrl+N | New note |
| Ctrl+O | Quick switcher (find or create a note) |
| Ctrl+E | Edit / reading view |
| Ctrl+, | Settings |
| Ctrl+Shift+D | Data usage: what Shale keeps on disk, and its caches |
| Ctrl+Shift+Y | Keymap: the keyboard with every shortcut, by system and layer, in a tab (macOS: Cmd+Shift+Y); Change on a key goes to its row in Settings → Keybindings. Definable (Everywhere) |
| Ctrl+Shift+PageUp / Ctrl+Shift+PageDown | Previous / next mail of the thread of the note, by date (see [[Thread view]]) |
| Ctrl+Shift+T | Thread overview: opens the Thread view in the right sidebar and gives it the keyboard (↑ ↓, Enter, Esc) |
| F1 | Open the user guide (built into Shale, unpacked into its data folder and opened as a vault) |
| Ctrl+S | Save now (notes also save automatically) |
| Ctrl+W | Close tab |
| Ctrl+Shift+F | Search notes, mail and posts |
| Ctrl+H / Ctrl+L | Keyboard one place left / right: through the split panes in screen order, left of the first pane into the left sidebar, from there into the first pane (macOS: Cmd+Shift+H / Cmd+Shift+L, because Cmd+H hides the app) |
| Ctrl+PageUp / Ctrl+PageDown, Ctrl+Shift+Tab / Ctrl+Tab | Previous / next tab of the focused pane in the order of the tab bar (from the last tab on to the first), wherever the keyboard is (macOS: Cmd+Opt+← / Cmd+Opt+→ only, Cmd+Tab is the system's) |
| Ctrl+\ | Split right: open this tab in a new pane to the right (panes sit side by side, each with its own tabs) |
| F6 / Shift+F6 | Focus the next / previous pane |
| Ctrl+Shift+W | Close the pane with all its tabs (macOS: Cmd+Shift+W) |
| Ctrl+Shift+B | File list with the keyboard in it: j / k (or ↓ / ↑) next / previous file, Ctrl+J / Ctrl+K next / previous folder, Space opens or closes a folder, Enter opens, Esc back |
| ↓ or Enter in the search box | Browse the results (notes, mail and posts): j / k (or ↓ / ↑) next / previous hit, Ctrl+J / Ctrl+K first hit of the next / previous file or group, Enter opens the hit, Esc back to the box |
| Tab / Shift+Tab | In the search over everything (the sidebar search, in its box and while you browse the results, and the picker of Space f g): list only the next / previous source (Notes, Mail, Posts), then the default sources again. Not while the query starts with a prefix (`f:`, `m:`, `s:`). Both can be changed in Settings → Keybindings ("Search results", "List windows") |
| Ctrl+Shift+G | In the search over everything (the sidebar search box and the picker of Space f g): show or hide the filter form (the gear button) and put the keyboard in it. Can be changed in Settings → Keybindings ("Search results", "List windows") |
| Ctrl+Shift+M | Mail view (its keys are listed below) |
| Alt+← / Alt+→ | Back / forward (macOS: Cmd+[ / Cmd+], because Option+← / → move by word in text) |
| Alt+B, Alt+I, Alt+S, Alt+H, Alt+C, Alt+Y, Alt+F, Alt+Shift+Y, Alt+Shift+F, Alt+E, Alt+U, Alt+O, Alt+T, Alt+Q, Alt+K, Alt+R, Alt+J, Alt+A, Alt+L, Alt+W, Alt+M, Alt+X | The buttons of the formatting toolbar of a note, each doing what a click on it does (the keyboard stays in the text and nothing is typed): Bold, Italic, Strikethrough, Highlight, Inline code, Highlight colour and Font colour (apply the current colour: the main part of the split button; Alt+Shift+Y and Alt+Shift+F open the palette, the ▾ part, where the digits 1 to 8 pick a swatch and 0 picks none, shown as ⇧Y and ⇧F while Alt is held), Heading level (opens its menu), Bullet list, Numbered list, Task, Quote, Code block, Horizontal rule, Table, Link to a note, Web link, Embed, Clear formatting. A key whose button is not there (reading view) or is greyed out (in a table cell) does nothing |
| Ctrl+Shift+K / Ctrl+Shift+J | Fold / unfold the block at the cursor (a heading section, quote, list item or list; again on a folded line folds the block around it); the cursor then goes to the block's first line |
| Ctrl+Shift+O / Ctrl+Shift+I | Fold all / unfold all blocks of the note (also in the reading view) |
| Ctrl+Shift+C | Copy the code of the code block at the cursor (no fences, no newline at the end) |
| Alt+V, Alt+P, Alt+D, Alt+N | The buttons of the view header and tab bar of a note: edit / reading view, Markdown preview, the ⋯ menu (opens it), New tab |
| Alt+1, Alt+2, Alt+3, Alt+4, Alt+5, Alt+6, Alt+7, Alt+8, Alt+9, Alt+0 | The ribbon items by position: the first ten icons of the ribbon, as shown (also after you reordered or removed some) |
| Alt+, / Alt+. | Show or hide the left / right sidebar |
| Hold Alt | A small badge with its key appears at the corner of each of these buttons that has an Alt key (it follows a key you changed). On macOS, where Option+letter types characters, these actions, and the Alt keys of the open mail, have no key until you give them one in Settings → Keybindings (scope Note view, Mail view and Everywhere) |
| Ctrl+6 | Alternate view, in every view (also mail and drawings, where Space is taken): swap with the view this tab showed just before; again: back |
| Space Tab | Vim-style editing, normal mode (not in the mail view, where Ctrl+6 does it): back to the view this tab showed just before (note, calendar, Feeds, …); again: forward to this one |
| Space … | A started key sequence (e.g. `Space f`) shows in the status bar and is dropped after 1.5 s without a key |
| Space f f | In every main view (editor in normal mode, reading view, mail, calendar, Feeds, tasks, graph; not while a text field has the keyboard or a dialog is open; scope "Main views" in Settings → Keybindings): find a file by name (↓ / ↑ or Ctrl+J / Ctrl+K select, Enter opens it in this tab, Esc closes) |
| Space f g | In every main view (as Space f f): search the text of notes, mail and posts (`*` and `?` are wildcards; `f:` `m:` `s:` at the start look in notes, mail or posts only; results grouped with counts; ↓ / ↑ or Ctrl+J / Ctrl+K select, Enter opens the hit where it belongs, Esc closes) |
| Space Space | Check or uncheck the task: in the editor (vim normal mode) the task on the cursor's line; in the reading view and the task list the task row under the pointer; a line without a task changes nothing and says so. Not in insert mode or a text field. Definable (Settings → Keybindings, Main views) |
| Space b | In every main view (as Space f f): show the boards, the Kanban view of [[Boards]] (a main-area tab with a strip of every note that has `type: board`). Definable (Settings → Keybindings, Main views) |
| Alt+Shift+→ / Alt+Shift+← | In the [[Boards]] view, with a card selected: move the card to the next / previous lane, like a drop below the pinned cards of that lane; the card stays selected. Definable (Settings → Keybindings, Main views: `board.card_next_lane`, `board.card_prev_lane`) |
| Alt+Shift+L | In the `[!` file picker of a note: for this pick, a file from outside the vault is linked where it is instead of copied, or copied instead of linked (with the setting Ask, the first press means link). The last line of the picker shows what a pick does. Not changeable. After a pick with the setting Ask: C or Enter copy, L links, Esc goes back |
| ↓ or Ctrl+J, ↑ or Ctrl+K, Enter, Esc | In a picker window (Space f f, Space f g), the quick switcher (Ctrl+O) and the font list: next line, previous line, open, close. The hint between the search field and the list shows the keys as they are bound now; all four can be changed in Settings → Keybindings ("List windows"; a bare key must be an arrow, Enter, Esc or similar, as the field has the keyboard) |
| Shift+K | Vim-style editing, normal mode: preview the link under the cursor (a note as in the reading view, a web page's title and text; press it again to move the keyboard into the preview: j / k or ↓ / ↑ scroll, Enter opens the link like `gt`, Ctrl+Enter opens it in a new pane on the right, Esc closes). Definable, e.g. as Space P |
| Alt+↑ / Alt+↓ | Move the current line |
| Tab / Shift+Tab | Indent lists or change heading levels |
| Shift+arrows | In a table cell edited in the grid: select a block of cells (formatting buttons and Delete / Backspace work on all of them; typing ends it) |

# Mail view keys

In the mail view (Ctrl+Shift+M) one of the three panes (folders, message list, message) has the
keyboard; it has an accent-coloured frame. **Ctrl+H / Ctrl+L** (macOS: Cmd+Shift+H / Cmd+Shift+L) move it one pane left / right (from
the folder pane Ctrl+H goes on into the pane on the left, or the left sidebar, from there Ctrl+L comes back). Letters work
only while no text field has the keyboard. Every key can be changed in Settings → Keybindings
(scope Mail view); Enter and Esc are fixed.

| Keys | Action |
|---|---|
| j / ↓, k / ↑ | Next / previous: folder in the folder pane, message in the list, scroll in the message pane |
| Enter | Folder pane: go to the list; list: open the message (Drafts: open the draft; Outbox: show the mail) |
| Ctrl+J / Ctrl+K | Next / previous folder, from any pane |
| x | Mark the message for a selection (list); Space is the leader of Space f f / Space f g in the mail view too |
| / | Search mail (Esc leaves the field, ↓ or Enter go into the results) |
| t | Search all folders or only this one |
| i | Import to vault |
| u | Mark read or unread |
| s or f | Set or clear the flag |
| d or Delete | Delete (Trash: asks first); in Drafts and Outbox discard the draft or the record |
| Shift+D | Delete the local copy of a mail that was deleted on the server (the dimmed ones; asks first; the server and imported notes are not touched) |
| m | Move to a folder |
| c | Write a new mail |
| r, Shift+R, Shift+F | Reply, reply all, forward |
| Ctrl+R | Sync, or cancel the sync |
| a | Account settings |
| Shift+L | Load the open message (or the list of a folder that is not synced) from the server |
| ] | Select the next attachment (marked with ▸) |
| o, Shift+O | Open the selected attachment, save it to the vault |
| z | Take back a mail waiting in the outbox |
| e | Edit: open the draft, or take a mail of the outbox that was not sent back into the editor |
| Shift+S | Retry a mail of the outbox that was not sent (or send it again when it may have been sent) |
| Menu, Shift+F10 | Open the menu of the message or draft under the cursor (↓ / ↑ or j / k, Enter, Esc) |
| Shift+U | Show or hide the sent mails that are only on this computer (j / k, Enter = Upload again, d = Dismiss, Esc) |
| Esc | Step back: close the message, clear the search, clear the marks, dismiss a message |

## Under a dialog

While a dialog is open (Settings, an import, a confirmation, a wizard, …), Shale's shortcuts and the editor's keys do nothing. Esc closes the dialog, and its own keys work: Enter, Tab, typing in its fields and, in Settings → Keybindings, the key capture. Shortcuts of your system (screenshots, switching windows) are not affected.

#userguide #keys
