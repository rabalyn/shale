---
features:
  - "Settings that travel with the vault"
---
Back to [[Start here]]; the complete list is [[Settings reference]].

Shale keeps settings in two places:

| Where | What | Travels with the vault? |
|---|---|---|
| `<vault>/.shale/settings.json` | people folder, git sync | yes (git sync commits it, [[Git sync]]) |
| `<vault>/.shale/state.json` | open tabs and which are pinned, last import folder, recent tags, the last LaTeX export choices | yes, but it is only session state |
| `~/.config/shale/config.toml` | the program itself: appearance, calendars, mail and Feeds accounts, keybindings, recent vaults | no, this computer only |

- The files carry a format version and are upgraded on load. A file that cannot be read is
  kept as `….broken-<date>` and you are told; a file from a newer Shale is used but not
  changed.
- On the first start after an upgrade from an old version, the people folder, open notes,
  import folder and recent tags move from the old global settings into the vault you open.
- Learned tag words are in `.shale/tag-words.json` ([[Tags and people]]).

#userguide #settings
