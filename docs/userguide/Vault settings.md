---
features:
  - "Settings that travel with the vault"
---
Back to [[Start here]]; the complete list is [[Settings reference]].

Shale keeps settings in two places:

| Where | What | Travels with the vault? |
|---|---|---|
| `<vault>/.shale/settings.json` | people folder, bibliography file, git sync, what is published as a website ([[Publishing a vault]]) | yes (git sync commits it, [[Git sync]]) |
| `<vault>/.shale/state.json` | open tabs with their cursor, scroll and view and which are pinned, the folded blocks of your notes, the toolbar's colours, last import folder, recent tags, the last LaTeX export choices | yes, but it is only session state |
| `~/.config/shale/config.toml` | the program itself: appearance, calendars, mail and Feeds accounts, keybindings, the list of known vaults with their names | no, this computer only |

- The files carry a format version and are upgraded on load. A file that cannot be read is
  kept as `….broken-<date>` and you are told; a file from a newer Shale is used but not
  changed.
- On the first start after an upgrade from an old version, the people folder, open notes,
  import folder and recent tags move from the old global settings into the vault you open.
- The vaults Shale knows are listed in `config.toml` with a name each (by default the name of the
  folder) and a path with symbolic links resolved, so a folder is never listed twice. The start
  screen lists them and the window title shows the name of the open vault; when two vaults have
  the same name, the folder they are in follows the name. A vault is added when it has opened
  (also one given with `--vault` or passed on by a second start). The list replaces the old
  "recent" list, which is converted at the first start.
- The vault name at the bottom left of the window, with a ▾, opens the list of known vaults. Click
  one (or move with the arrows and press Enter) to open it; **Open…** and **Create…** add a
  folder, and Create only takes an empty folder (hidden files such as `.DS_Store` do not count; a
  folder that does not exist yet is made) and can start with sample notes. Right-click a vault and
  choose **Rename** to give it another name: the name stays on this computer, the folder keeps its
  name. If a vault's folder is gone (moved, a drive that is not mounted), its entry is greyed out,
  here and on the start screen: **Locate…** points it at the folder's new place and **Forget**
  removes the entry (the folder is never touched). Shale does not make that folder again by itself.
  A folder inside another known vault, or around one, may be added; Shale warns once, because the
  notes then show up in both vaults.
- **The file picker.** Open…, Create…, Locate…, the library folders, the template sets of the
  LaTeX export, mail attachments, the place of a PDF export and the Import dialog all choose files
  and folders the same way: with the dialog of your system first (on Linux the portal of your
  desktop; it has its own shortcuts, search and New folder button), or with Shale's own picker
  when there is none or when **Use the system's file dialog** is off (Settings → Vault). Shale's
  picker has a path field (`~` is your home folder), shortcuts on the side (Home, Desktop,
  Documents, Downloads, Pictures, Videos, Music, your vaults, your library folders, your drives),
  the folders and files of the shown folder, several files at once and the kinds of file. A
  **New folder** field makes a folder inside the shown one and uses it; a name that Windows would
  not accept (`/`, `\`, `:`, `CON`, a trailing dot, …) is refused with the reason, and a name that
  exists offers **Use that folder**. Create… asks for "Start with sample notes" in the picker, or,
  with the system's dialog, in a small step after it.

  ![[file-picker.png]]
- **Close** takes a vault off the list; its notes stay where they are. Right-click the vault in the
  list and choose **Close**, or choose **Close current vault** at the end of the list (the quick
  switcher also has a command "Switch vault", which shows the same list). A question follows: you
  can tick **Also remove .shale/** (the vault's settings and state) and **Also remove .trash/**
  (the notes you deleted; they are gone for good). Both boxes are off, so by default nothing is
  removed; notes, attachments and a `.git` folder are never touched. Closing the open vault saves
  your edits and, when a sync is running, waits for it (the window stays usable; the indicator shows
  "Waiting for the sync to finish…" with Cancel), then shows the start screen with the list.
  Switching to another vault does the same, and nothing of the first vault (tabs, search,
  history, sync state) is left in the window; mail, the calendar and the global settings stay.
- Learned tag words are in `.shale/tag-words.json` ([[Tags and people]]).

# Names that work on every system

Files and folders that Shale makes get names that also work on Windows and macOS: the characters
Windows forbids (`< > : " / \ | ? *`) and control characters become `-`, a trailing dot is dropped,
a Windows device name such as `CON` or `NUL` gets a `_` (`CON_`), and a name is cut to 200 bytes when
you type it (127 when Shale makes it, as the imports do). If a name you typed had to change, the
message says what and why: "Saved as `a-b.md`: `:` is not allowed on Windows". Links to the file use
the name it got. Files that are in the vault already are never renamed by this.

If the vault is shared with a Windows or macOS computer (for example through [[Git sync]]), find the
names that would break there: **Settings → Vault → Check names for Windows and macOS…**. The check runs
in the background (a spinner says so, **Cancel** stops it, see [[Background work]]) and lists each file or folder with the reason and the name it would get:

- characters Windows forbids, a trailing dot or space, a device name;
- a name longer than 200 bytes, or a path longer than 240 characters (counted from the vault folder);
- names that differ only in letter case in one folder (`Plan.md` and `plan.md`), which Windows and
  macOS see as one.

**Rename** changes one, **Rename all…** all of them after a question. They are renamed like any rename
([[Notes and links]]): links in other notes are rewritten, and a name that is already taken gets
` 2`, ` 3` and so on (a long name is shortened to make room for the number). `.git`, `.shale` and
the trash are not looked at.

The same check lists the vault's **links to media outside the vault** ([[Sounds and videos]]) in a
second part, "Media outside the vault": how many there are, the libraries that this computer has not
set up (or whose folder is not there), and by note the links that need a look: a file that is not
found, a library that is missing or a path that leaves the library folder, and file links
(`file://…`), which only work on this computer. Library links whose file is there are not listed.
The files are looked up in the background, so a slow or unplugged disk does not stop the window.

#userguide #settings
