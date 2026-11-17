---
features:
  - "Git sync"
---
# Git sync

Keep a vault on several computers with git. Back to [[Start here]].

- Needs `git` installed. In Settings → General → Git sync, enter the URL of a git repository
  (`https://…`, `ssh://…`, `git@host:notes.git` or a folder path) and press **Connect**, or run
  `git init` in the vault folder yourself. An address with a password in it, and plain `http://`
  or `git://` (except on this computer), are refused: use a credential helper or an SSH key.
  Shale never asks for a password; that is git's business.
- Shale then syncs on its own: when the vault opens, every few minutes (default 5; the setting
  is `sync.interval_minutes`) and on **Sync now**. "Sync this vault automatically" switches it
  off (`sync.enabled`). Those settings are in the vault's `.shale/settings.json`, so they travel
  with it ([[Vault settings]]).
- A sync commits every change (the `.shale/` files too, so settings and open tabs follow you),
  fetches, merges and pushes. Commit messages read `Sync from <computer>: 3 file(s)`, with an
  optional prefix and suffix. Without a remote, Shale only keeps a local history.

## Nothing is ever lost

- A note changed on two computers keeps the remote version, and yours is kept next to it as
  `Note (conflict 2026-11-14 1530).md`. A note deleted on one side and changed on the other is
  kept.
- Shale never force-pushes, never writes conflict markers into notes, and pauses if git is in
  the middle of something you started (a merge, a rebase).
- Edits you type while a sync runs are never overwritten: your text is saved as a conflict copy
  next to the new version.

## Media with Git LFS

Sounds and videos are big and do not merge, so Shale can keep them in Git LFS: the history holds
a small pointer and the file itself lives in the remote's LFS store. See [[Sounds and videos]].

- **git-lfs on every computer.** It must be installed on each computer that commits media. The
  settings show whether it is. Without it the sync commits nothing, says why and where to get it
  (https://git-lfs.com; the package is called `git-lfs`), so that no media file lands in the
  history as a plain file.
- **Store media with Git LFS.** The button in Settings → General → Git sync switches it on for
  this repository: audio and video files (mp3, ogg, oga, opus, flac, m4a, aac, wav, mp4, m4v, mov,
  webm, mkv) are sent to LFS, and media already tracked are converted in one commit. Repositories
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

### Forgejo

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
