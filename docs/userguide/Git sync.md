---
features:
  - "Git sync"
---
Keep a vault on several computers with git. Back to [[Start here]]. The easy way in is [[Turn on sync]]; this note is what runs behind it. A vault in a folder that Dropbox, Nextcloud or OneDrive syncs: [[Vaults in cloud and network folders]].

- Needs `git` (see "Installing git" below if you have none). In Settings → Sync press **Turn on sync…** ([[Turn on sync]]),
  or open **Advanced**, enter the URL of a git repository (`https://…`, `ssh://…`, `git@host:notes.git` or a folder path)
  and press **Connect** (a folder path may have spaces, like `…/Dropbox/My notes.git`; an address may not), or run `git init` in the vault folder yourself. An address with a password in it, and plain `http://`
  or `git://` (except on this computer), are refused: use the wizard's token, a credential helper or an SSH key.
  A token made through the wizard is kept in your system's keyring and given to git by Shale; it is never in a file.
- Shale then syncs on its own: when the vault opens, every few minutes (default 5; the setting
  is `sync.interval_minutes`) and on **Sync now**. "Sync this vault automatically" switches it
  off (`sync.enabled`). Those settings are in the vault's `.shale/settings.json`, so they travel
  with it ([[Vault settings]]). While a sync runs, **Sync now** is off, a spinner says "Syncing with git…" and the
  indicator at the top right of the editor lists it ([[Background work]]); a sync has no Cancel, because stopping it half way would
  leave the repository between two states.
- A sync commits every change (the `.shale/` files too, so settings and open tabs follow you),
  fetches, merges and pushes. Commit messages read `Sync from <computer>: 3 file(s)`, with an
  optional prefix and suffix, and end with the lines `Shale-Reason:` and `Shale-Device:` (the computer's name).
  Without a remote, Shale only keeps a local history.
- Besides the sync's commits, Shale makes a small commit of a note when you save it, stop typing or close it,
  so that you can go back to an earlier version ([[File history]]). The sync pushes them as they are, and
  a vault without a git repository gets the same history on this computer, outside the vault. When you
  connect a server to such a vault, that history moves into the vault's `.git` and the first sync sends it to
  the server (a server that has notes already is merged with them).
- Only the files of the vault are synced. A picture, PDF, sound or video that a note links from
  outside the vault ([[Sounds and videos]]) stays where it is: it is not committed or uploaded, and
  another computer shows "Not found" or "Library … is not set up" until it has the file or the library
  folder. Settings → Sync says how many files the vault links from outside.

# Installing git

Settings → Sync starts with what it found: the git and the git-lfs of your computer (through `PATH`),
Shale's own git if you installed one, and which of them is in use.

![[settings-sync.png]]

- **Windows.** Without git, **Download and install git** downloads MinGit (the official portable
  Git for Windows, about 40 MB) and git-lfs (about 6 MB) from their GitHub releases into
  `shale\git` in your data folder, about 110 MB on disk. No administrator rights are needed and
  nothing else on the computer changes. The page shows the sizes and the folder first; nothing is
  downloaded before you press the button. Shale checks every file against the SHA-256 it has
  pinned, builds the folder aside as `git.partial`, runs `git --version` from it, and only then
  renames it to `git`. **Cancel** or a failure leaves nothing behind.
- **macOS.** Apple ships git only with its command line tools. The button **Install git (Apple's
  command line tools)** opens Apple's installer (a system dialog, 1 to 2 GB); press **Check
  again** when it is done. git-lfs, if missing, is downloaded by Shale (about 6 MB).
- **Linux.** Install git with your package manager; the page shows the command for your
  distribution (for example `sudo apt install git git-lfs`) with a **Copy command** button, and
  Shale never runs `sudo`. When git is there and git-lfs is not, **Download and install git-lfs**
  puts it into `~/.local/share/shale/git` (about 6 MB).
- **Which git runs.** **Automatic** uses the git of your computer when there is one (its
  git-lfs first, Shale's as the fallback) and Shale's own git otherwise. **The git of this
  computer** and **The git that Shale installs** pin the choice (`git.choice` in the settings file,
  [[Settings reference]]).
- **Updates.** Shale never asks the network for a newer git unless you turn it on: **Look for a new
  version and tell me** (or **…and install it**) looks 30 seconds after Shale starts and then
  every week at the releases of git-for-windows and git-lfs, and checks the download against the
  SHA-256 GitHub lists. **Look for a new version** checks once, now. **Update** replaces Shale's git
  only when the new one runs; the old one stays until then. `SHALE_UPDATES=off` switches every
  check off. Shale's own git is not updated by your system's package manager.
- **Remove…** asks first and deletes the folder `shale/git` and nothing else.
- Passwords are git's business unless you used the wizard: then Shale gives git the token from the keyring (see
  [[Turn on sync]]); otherwise a credential helper or an SSH key. Shale's git is run the same way as any other git (no shell, no prompts).

# Nothing is ever lost

- A note changed on two computers keeps the remote version, and yours is kept next to it as
  `Note (conflict 2026-11-14 1530).md`. A note deleted on one side and changed on the other is
  kept.
- Shale never force-pushes, never writes conflict markers into notes, and pauses if git is in
  the middle of something you started (a merge, a rebase).
- Edits you type while a sync runs are never overwritten: your text is saved as a conflict copy
  next to the new version.

# Media with Git LFS

Sounds and videos are big and do not merge, so Shale can keep them in Git LFS: the history holds
a small pointer and the file itself lives in the remote's LFS store. See [[Sounds and videos]].

- **git-lfs on every computer.** It must be installed on each computer that commits media. The
  settings show whether it is. Without it the sync commits nothing, says why and where to get it
  (https://git-lfs.com; the package is called `git-lfs`), so that no media file lands in the
  history as a plain file.
- **Store media with Git LFS.** The button in Settings → Sync switches it on for
  this repository: audio and video files (mp3, ogg, oga, opus, flac, m4a, aac, wav, mp4, m4v, mov,
  webm, mkv, avi, ogv) are sent to LFS, and media already tracked are converted in one commit. Repositories
  Shale creates itself are set up this way from the start. Shale only changes this repository,
  never your global git configuration, and keeps your own lines in `.gitattributes`.
- **Old versions stay.** Files committed before that stay in the history as they were; Shale
  never rewrites history. To remove them, run `git lfs migrate import --include="*.mp3,*.mp4"`
  (list all the extensions you use) in the vault, then force-push. Shale never does either.
- **Uploading.** A sync runs `git lfs push` before `git push`, so the files reach the server.
- **Downloading.** Media from other computers arrive as pointers and are downloaded when you play
  or open them, or after every sync if you tick "Download media files after every sync"
  (`sync.lfs_download`, see [[Settings reference]]).
- **Conflicts.** The conflict copy of a file stored in LFS is stored in LFS too.
- **SSH and HTTPS** remotes both work with your normal git credentials.

## Forgejo

Forgejo has LFS switched on by default (`[server] LFS_START_SERVER = true`). To keep the LFS files
on S3 or MinIO instead of the server's disk, put this in `app.ini` (placeholder values):

```ini
[lfs]
STORAGE_TYPE = minio
MINIO_ENDPOINT = minio.example.com:9000
MINIO_BUCKET = forgejo-lfs
MINIO_ACCESS_KEY_ID = your-access-key
MINIO_SECRET_ACCESS_KEY = your-secret-key
MINIO_USE_SSL = true
```

This guide's own `.shale/settings.json` has sync switched off, because it lives inside the
Shale repository.

#userguide #git
