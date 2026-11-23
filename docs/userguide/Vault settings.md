---
features:
  - "Settings that travel with the vault"
---
Back to [[Start here]]; the complete list is [[Settings reference]].

Shale keeps settings in two places:

| Where | What | Travels with the vault? |
|---|---|---|
| `<vault>/.shale/settings.json` | people folder, git sync | yes (git sync commits it, [[Git sync]]) |
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
