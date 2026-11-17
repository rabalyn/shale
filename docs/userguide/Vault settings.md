---
features:
  - "Settings that travel with the vault"
---
Back to [[Start here]]; the complete list is [[Settings reference]].

Shale keeps settings in two places:

| Where | What | Travels with the vault? |
|---|---|---|
| `<vault>/.shale/settings.json` | people folder, git sync | yes (git sync commits it, [[Git sync]]) |
| `<vault>/.shale/state.json` | open tabs and which are pinned, last import folder, recent tags | yes, but it is only session state |
| `~/.config/shale/config.toml` | the program itself: appearance, calendars, mail and Feeds accounts, keybindings, recent vaults | no, this computer only |

- The files carry a format version and are upgraded on load. A file that cannot be read is
  kept as `….broken-<date>` and you are told; a file from a newer Shale is used but not
  changed.
- On the first start after an upgrade from an old version, the people folder, open notes,
  import folder and recent tags move from the old global settings into the vault you open.
- Learned tag words are in `.shale/tag-words.json` ([[Tags and people]]).

## Names that work on every system

Files and folders that Shale makes get names that also work on Windows and macOS: the characters
Windows forbids (`< > : " / \ | ? *`) and control characters become `-`, a trailing dot is dropped,
a Windows device name such as `CON` or `NUL` gets a `_` (`CON_`), and a name is cut to 200 bytes when
you type it (127 when Shale makes it, as the imports do). If a name you typed had to change, the
message says what and why: "Saved as `a-b.md`: `:` is not allowed on Windows". Links to the file use
the name it got. Files that are in the vault already are never renamed by this.

If the vault is shared with a Windows or macOS computer (for example through [[Git sync]]), find the
names that would break there: **Settings → Vault → Check names for Windows and macOS…**. The check runs
in the background and lists each file or folder with the reason and the name it would get:

- characters Windows forbids, a trailing dot or space, a device name;
- a name longer than 200 bytes, or a path longer than 240 characters (counted from the vault folder);
- names that differ only in letter case in one folder (`Plan.md` and `plan.md`), which Windows and
  macOS see as one.

**Rename** changes one, **Rename all…** all of them after a question. They are renamed like any rename
([[Notes and links]]): links in other notes are rewritten, and a name that is already taken gets
` 2`, ` 3` and so on. `.git`, `.shale` and the trash are not looked at.

#userguide #settings
