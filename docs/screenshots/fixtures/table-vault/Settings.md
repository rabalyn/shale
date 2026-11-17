# Where Shale keeps settings

Shale keeps settings in three places:

| Where | What | Travels with the vault? |
|:---|:---|:---:|
| `<vault>/.shale/settings.json` | people folder, **git sync** | yes (git sync commits it, [[Git sync]]) |
| `<vault>/.shale/state.json` | open tabs and which are pinned, last import folder, recent tags | yes, but it is only session state |
| `~/.config/shale/config.toml` | the program itself: appearance, calendars, mail and Feeds accounts, keybindings, recent vaults | *no*, this computer only |

A note on a file in another folder is fine too.
