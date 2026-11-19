---
features:
  - "Keyboard"
---
Back to [[Start here]]. Editing keys are in [[Editor]], the settings window is in [[Settings reference]].

![[keybindings.png]]

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
| F1 | Open the user guide (built into Shale, unpacked into its data folder and opened as a vault) |
| Ctrl+S | Save now (notes also save automatically) |
| Ctrl+W | Close tab |
| Ctrl+Shift+F | Search all notes |
| Ctrl+H / Ctrl+L | Keyboard one place left / right: through the split panes in screen order, left of the first pane into the left sidebar, from there into the first pane (macOS: Cmd+Shift+H / Cmd+Shift+L, because Cmd+H hides the app) |
| Ctrl+PageUp / Ctrl+PageDown | Previous / next tab (from the last tab on to the first), wherever the keyboard is (macOS: Cmd+Opt+← / Cmd+Opt+→) |
| Ctrl+\ | Split right: open this tab in a new pane to the right (panes sit side by side, each with its own tabs) |
| F6 / Shift+F6 | Focus the next / previous pane |
| Ctrl+Shift+W | Close the pane with all its tabs (macOS: Cmd+Shift+W) |
| Ctrl+Shift+B | File list with the keyboard in it: j / k (or ↓ / ↑) next / previous file, Ctrl+J / Ctrl+K next / previous folder, Space opens or closes a folder, Enter opens, Esc back |
| ↓ or Enter in the search box | Browse the results: j / k (or ↓ / ↑) next / previous hit, Ctrl+J / Ctrl+K next / previous file, Enter opens the note at the hit, Esc back to the box |
| Ctrl+Shift+M | Mail view (its keys are listed below) |
| Alt+← / Alt+→ | Back / forward (macOS: Cmd+[ / Cmd+], because Option+← / → move by word in text) |
| Alt+B, Alt+I, Alt+S, Alt+H, Alt+C, Alt+Y, Alt+F, Alt+E, Alt+U, Alt+O, Alt+T, Alt+Q, Alt+K, Alt+R, Alt+J, Alt+A, Alt+L, Alt+W, Alt+M, Alt+X | The buttons of the formatting toolbar of a note, each doing what a click on it does (the keyboard stays in the text and nothing is typed): Bold, Italic, Strikethrough, Highlight, Inline code, Highlight colour and Font colour (open their palette, the ▾ part of the split button), Heading level (opens its menu), Bullet list, Numbered list, Task, Quote, Code block, Horizontal rule, Table, Link to a note, Web link, Embed, Clear formatting. A key whose button is not there (reading view) or is greyed out (in a table cell) does nothing |
| Alt+V, Alt+P, Alt+D, Alt+N | The buttons of the view header and tab bar of a note: edit / reading view, Markdown preview, the ⋯ menu (opens it), New tab |
| Alt+1, Alt+2, Alt+3, Alt+4, Alt+5, Alt+6, Alt+7, Alt+8, Alt+9, Alt+0 | The ribbon items by position: the first ten icons of the ribbon, as shown (also after you reordered or removed some) |
| Alt+, / Alt+. | Show or hide the left / right sidebar |
| Hold Alt | A small badge with its key appears at the corner of each of these buttons that has an Alt key (it follows a key you changed). On macOS, where Option+letter types characters, these actions, and the Alt keys of the open mail, have no key until you give them one in Settings → Keybindings (scope Note view, Mail view and Everywhere) |
| Ctrl+6 | Alternate view, in every view (also mail and drawings, where Space is taken): swap with the view this tab showed just before; again: back |
| Space Tab | Vim-style editing, normal mode: back to the view this tab showed just before (note, calendar, Feeds, …); again: forward to this one |
| Space … | A started key sequence (e.g. `Space f`) shows in the status bar and is dropped after 1.5 s without a key |
| Space f f | Vim-style editing, normal mode: find a file by name (↓ / ↑ or Ctrl+J / Ctrl+K select, Enter opens it in this tab, Esc closes) |
| Space f g | Vim-style editing, normal mode: search the text of notes, mail and posts (`*` and `?` are wildcards; results grouped with counts; ↓ / ↑ or Ctrl+J / Ctrl+K select, Enter opens the hit where it belongs, Esc closes) |
| Shift+K | Vim-style editing, normal mode: preview the link under the cursor (a note as in the reading view, a web page's title and text; press it again to move the keyboard into the preview: j / k or ↓ / ↑ scroll, Enter opens the link like `gt`, Ctrl+Enter opens it in a new pane on the right, Esc closes). Definable, e.g. as Space Space |
| Alt+↑ / Alt+↓ | Move the current line |
| Tab / Shift+Tab | Indent lists or change heading levels |
| Shift+arrows | In a table cell edited in the grid: select a block of cells (formatting buttons and Delete / Backspace work on all of them; typing ends it) |

## Mail view keys

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
| Space | Mark the message for a selection (list) |
| / | Search mail (Esc leaves the field, ↓ or Enter go into the results) |
| t | Search all folders or only this one |
| i | Import to vault |
| u | Mark read or unread |
| s or f | Set or clear the flag |
| d or Delete | Delete (Trash: asks first); in Drafts and Outbox discard the draft or the record |
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

#userguide #keys
