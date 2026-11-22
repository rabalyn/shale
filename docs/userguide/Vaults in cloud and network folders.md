Where to keep a vault: on a network drive, or in a folder that Nextcloud, Dropbox, OneDrive,
iCloud or Syncthing syncs. Back to [[Start here]].

# What is safe

- **Notes, drawings and pictures in such a folder.** Shale saves a note by writing a hidden
  temporary file next to it and renaming it over the note, so the other computers and the sync
  app see the old note or the new one, never half of one.
- **Two computers editing the same note.** Shale checks the file's content before every save. If
  another computer changed the note, your text is saved next to it as
  `Note (conflict 2026-11-21 1530).md` and the other version stays where it is. Nothing is
  overwritten ([[Git sync]] explains the same for git).
- **The sync app's own conflict copies** (`Note.sync-conflict-….md`, `Note (… conflicted copy …).md`,
  `Note_conflict-….md`, `Note (SFConflict …).md`, and Shale's own `Note (conflict …).md`) are
  recognised. In the file list they carry a "conflict" badge. Open one and a banner says another
  device changed the note at the same time (the original note shows the same banner): **Compare** shows the two texts (the changes, or the
  Markdown side by side), **Keep this one** puts the copy's text into the original, **Keep the
  other** leaves the original as it is. Either way the copy goes to the trash, and when the copy's text replaces the original, the
  original's old text goes to the trash too. While the original
  exists, the copy is left out of search, links, the graph, tasks and tags. Copies that
  OneDrive, iCloud and Google Drive make (`Note 2.md`, `Note (1).md`) look like real notes by
  name, so Shale cannot tell and shows them as ordinary notes.
- **Changes from other computers** show up within a few seconds on a local folder that a sync
  app keeps current. On an NFS share it can take up to a minute (NFS caches folder listings).
  Shale looks at the folder every 3 seconds; it does not rely on the system telling it.

# Shale tells you

[[Turn on sync]] and Settings → Sync warn when Shale finds that the vault is on a network share or in a folder that Dropbox, Nextcloud,
Syncthing, Google Drive, iCloud or OneDrive syncs (a guess from the file system and from marker files; it never decides anything). For a
vault that has a `.git` folder inside such a folder and no server, **Move the repository out** puts the repository in a private place on this
computer and removes `.git` from the vault, after checking the copy.

# What to avoid

- **A git repository in a synced folder.** Do not run `git init` in a vault that Nextcloud,
  Dropbox, OneDrive, iCloud or Syncthing syncs, and do not clone one there. The sync app copies the
  files of `.git` one at a time and in any order, which breaks the repository. Use [[Git sync]]
  with a server for the vault, in a folder no sync app touches, or keep your notes' history on
  this computer only (it is then stored outside the vault).
- **Both computers open on the same note at the same moment.** It works (see above) but you get a
  conflict copy. Close the note on one computer, or wait for the sync app to finish.
- **A big vault on a slow network drive.** Shale reads the whole folder at start, and the window can
  pause while it does. It checks the folder again every 3 seconds in the background (less often when a
  check took long), so the window stays responsive. A vault of a few thousand notes is fine on
  a local disk or a fast share; put a large vault on the local disk and sync it with an app.
- **Files that only exist online** (OneDrive Files On-Demand, iCloud, Dropbox Smart Sync): Shale
  has to download one to read it, and waits. Mark the vault folder "always keep on this device".
- **Names that differ only in letter case, or in accents typed differently** (`Café` made on a Mac
  and on Linux). They can look like the same name and be two files. Links do not mind the accent
  form: `[[Café]]` finds the note whichever way its name was typed, and Shale never renames the
  file for that. If both spellings exist as two files, a link opens the one in its own folder, else
  the first by path, and the tooltip says that two notes share the name. [[Vault settings]] has a
  check for names that break on Windows and macOS.

# Things you may notice

- Your open tabs and the undo histories are not in the vault's `.shale` folder when Shale finds the
  vault in such a folder: each computer keeps its own in Shale's data folder, so two computers do not
  overwrite each other's tabs. The first time, Shale moves what an older version left in `.shale`
  there. A file named `state.sync-conflict-….json` from the time before is removed by Shale.
- Hidden files starting with `.shale-tmp-` (only for a moment while Shale saves, or after a crash).
  Shale removes one that is older than a day when it opens the vault. To keep them out of your sync app, add `.shale-tmp-*` to its
  ignore list (Syncthing: `.stignore`, Nextcloud: the exclude list).
